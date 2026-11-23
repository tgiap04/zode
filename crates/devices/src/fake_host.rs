use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures::channel::oneshot;
use parking_lot::Mutex;

use crate::host::{
    DetachedExit, HostOs, HostSystem, ToolFailure, ToolInvocation, ToolOutput, ToolRunner,
    ensure_absolute_program,
};

type ScriptKey = (PathBuf, Vec<String>);

/// A host that never leaves the process: tool runs answer from a script keyed
/// by program and arguments, files live in a map, and every call is recorded.
pub struct FakeHost {
    state: Mutex<FakeState>,
}

struct FakeState {
    os: HostOs,
    environment: HashMap<String, String>,
    path_programs: HashMap<String, PathBuf>,
    home: Option<PathBuf>,
    files: HashMap<PathBuf, String>,
    symlinks: HashSet<PathBuf>,
    scripts: HashMap<ScriptKey, VecDeque<Result<ToolOutput, ToolFailure>>>,
    invocations: Vec<ToolInvocation>,
    detached: Vec<(ToolInvocation, PathBuf)>,
    /// Held so each detached tool stays running until a test ends it.
    detached_exits: Vec<Option<oneshot::Sender<Option<i32>>>>,
    reads: Vec<PathBuf>,
    available_memory: Option<u64>,
}

impl FakeHost {
    pub fn new(os: HostOs) -> Self {
        Self {
            state: Mutex::new(FakeState {
                os,
                environment: HashMap::new(),
                path_programs: HashMap::new(),
                home: Some(PathBuf::from("/home/tester")),
                files: HashMap::new(),
                symlinks: HashSet::new(),
                scripts: HashMap::new(),
                invocations: Vec::new(),
                detached: Vec::new(),
                detached_exits: Vec::new(),
                reads: Vec::new(),
                available_memory: Some(16 * 1024 * 1024 * 1024),
            }),
        }
    }

    pub fn set_environment_variable(&self, name: &str, value: impl Into<String>) {
        self.state
            .lock()
            .environment
            .insert(name.to_string(), value.into());
    }

    pub fn set_on_path(&self, program: &str, path: impl Into<PathBuf>) {
        self.state
            .lock()
            .path_programs
            .insert(program.to_string(), path.into());
    }

    pub fn set_home(&self, home: Option<PathBuf>) {
        self.state.lock().home = home;
    }

    pub fn add_file(&self, path: impl Into<PathBuf>, contents: impl Into<String>) {
        self.state.lock().files.insert(path.into(), contents.into());
    }

    /// Marks `path` as a symbolic link; the files under it stay readable.
    pub fn add_symlink(&self, path: impl Into<PathBuf>) {
        self.state.lock().symlinks.insert(path.into());
    }

    pub fn has_file(&self, path: &Path) -> bool {
        self.state.lock().files.contains_key(path)
    }

    pub fn set_available_memory(&self, bytes: Option<u64>) {
        self.state.lock().available_memory = bytes;
    }

    /// Each call adds one answer for this program and arguments. Answers are
    /// handed out in order, and the last one repeats.
    pub fn script<Arg: Into<String>>(
        &self,
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = Arg>,
        result: Result<ToolOutput, ToolFailure>,
    ) {
        let key = (
            program.into(),
            args.into_iter().map(Into::into).collect::<Vec<_>>(),
        );
        self.state
            .lock()
            .scripts
            .entry(key)
            .or_default()
            .push_back(result);
    }

    pub fn script_output<Arg: Into<String>>(
        &self,
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = Arg>,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
    ) {
        self.script(
            program,
            args,
            Ok(ToolOutput {
                exit_code: Some(exit_code),
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            }),
        );
    }

    pub fn invocations(&self) -> Vec<ToolInvocation> {
        self.state.lock().invocations.clone()
    }

    pub fn detached(&self) -> Vec<(ToolInvocation, PathBuf)> {
        self.state.lock().detached.clone()
    }

    /// Ends the `index`th detached tool with `exit_code`, as its reaper
    /// would, after appending `log` to the log file it writes.
    pub fn exit_detached(&self, index: usize, exit_code: Option<i32>, log: &str) {
        let mut state = self.state.lock();
        let Some((_, log_file)) = state.detached.get(index).cloned() else {
            panic!("no detached tool number {index}");
        };
        state.files.entry(log_file).or_default().push_str(log);
        if let Some(sender) = state.detached_exits.get_mut(index).and_then(Option::take) {
            sender.send(exit_code).ok();
        }
    }

    pub fn reads(&self) -> Vec<PathBuf> {
        self.state.lock().reads.clone()
    }
}

#[async_trait]
impl ToolRunner for FakeHost {
    async fn run(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolFailure> {
        let mut state = self.state.lock();
        state.invocations.push(invocation.clone());
        ensure_absolute_program(&invocation)?;
        let key = (invocation.program.clone(), invocation.args.clone());
        let Some(answers) = state.scripts.get_mut(&key) else {
            return Err(ToolFailure::Io {
                program: invocation.program,
                message: format!("no scripted answer for {:?}", invocation.args),
            });
        };
        let answer = if answers.len() > 1 {
            answers.pop_front()
        } else {
            answers.front().cloned()
        };
        answer.unwrap_or_else(|| Err(ToolFailure::NotFound(invocation.program)))
    }

    fn spawn_detached(
        &self,
        invocation: ToolInvocation,
        log_file: &Path,
    ) -> anyhow::Result<DetachedExit> {
        let mut state = self.state.lock();
        state.invocations.push(invocation.clone());
        ensure_absolute_program(&invocation)?;
        state.files.insert(log_file.to_path_buf(), String::new());
        state.detached.push((invocation, log_file.to_path_buf()));
        let (sender, exit) = oneshot::channel();
        state.detached_exits.push(Some(sender));
        Ok(exit)
    }
}

impl HostSystem for FakeHost {
    fn os(&self) -> HostOs {
        self.state.lock().os
    }

    fn environment_variable(&self, name: &str) -> Option<String> {
        self.state.lock().environment.get(name).cloned()
    }

    fn find_on_path(&self, program: &str) -> Option<PathBuf> {
        self.state.lock().path_programs.get(program).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.state.lock().home.clone()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.state.lock().files.contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.state
            .lock()
            .files
            .keys()
            .any(|file| file != path && file.starts_with(path))
    }

    fn is_symlink(&self, path: &Path) -> bool {
        self.state.lock().symlinks.contains(path)
    }

    fn read_to_string_at_most(&self, path: &Path, max_bytes: u64) -> io::Result<String> {
        let contents = self.read_to_string(path)?;
        if contents.len() as u64 > max_bytes {
            return Err(crate::local_host::too_large(max_bytes));
        }
        Ok(contents)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let mut state = self.state.lock();
        state.reads.push(path.to_path_buf());
        state
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn read_file_tail(&self, path: &Path, max_bytes: u64) -> io::Result<String> {
        let contents = self.read_to_string(path)?;
        let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        let mut start = contents.len().saturating_sub(max_bytes);
        while !contents.is_char_boundary(start) {
            start += 1;
        }
        Ok(contents[start..].to_string())
    }

    fn read_dir_names(&self, path: &Path) -> io::Result<Vec<String>> {
        let state = self.state.lock();
        // A directory exists here only through the files under it, so every
        // file below `path` names the child it sits in.
        let mut names: Vec<String> = state
            .files
            .keys()
            .filter_map(|file| file.strip_prefix(path).ok()?.components().next())
            .filter_map(|child| child.as_os_str().to_str().map(str::to_string))
            .collect();
        if names.is_empty() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.state
            .lock()
            .files
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn temp_file_path(&self, stem: &str) -> PathBuf {
        PathBuf::from("/fake-temp").join(stem)
    }

    fn available_memory_bytes(&self) -> Option<u64> {
        self.state.lock().available_memory
    }
}
