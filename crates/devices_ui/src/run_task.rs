//! Turns a [`RunPlan`] into the one terminal task that runs it.
//!
//! The terminal panel joins `command` and `args` with spaces and hands the
//! line to the shell without quoting (`ShellBuilder::build_no_quote`), so every
//! argument is quoted here, for the shell that will read it.

use std::fmt;

use devices::{HostOs, OpenInstalledAndroidApp, RunPlan, RunStep};
use task::{HideStrategy, RevealStrategy, Shell, ShellKind, SpawnInTerminal, TaskId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunTaskError {
    pub argument: String,
    pub reason: String,
}

impl fmt::Display for RunTaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "\"{}\" cannot be passed to the terminal's shell safely: {}.",
            self.argument, self.reason
        )
    }
}

impl RunTaskError {
    fn new(argument: &str, reason: impl Into<String>) -> Self {
        Self {
            argument: argument.to_string(),
            reason: reason.into(),
        }
    }
}

/// `run_number` makes the id and label unique: with a label shared by a run
/// that never exits (`flutter run`, Metro), the panel would queue this one
/// behind it forever.
///
/// A chained plan never runs in the user's shell: PowerShell 5, Nushell and
/// Elvish chain with `;`, which would install and open an old build after a
/// failed one. It runs in `/bin/sh` or `cmd.exe`, where `&&` stops at the
/// first failure. `cmd.exe` is named by its absolute path: a bare name is
/// looked up in the current directory first, which may be a project's.
pub(crate) fn spawn_in_terminal(
    plan: &RunPlan,
    user_shell: ShellKind,
    host_os: HostOs,
    run_number: u64,
) -> Result<SpawnInTerminal, RunTaskError> {
    let (command, args, shell) = if plan.is_chained() {
        let (shell_kind, shell) = match host_os {
            HostOs::Windows => (
                ShellKind::Cmd,
                windows_command_interpreter(
                    std::env::var("ComSpec").ok().as_deref(),
                    std::env::var("SystemRoot").ok().as_deref(),
                ),
            ),
            HostOs::MacOs | HostOs::Linux => (ShellKind::Posix, "/bin/sh".to_string()),
        };
        let (command, args) = chained_command_line(plan, shell_kind)?;
        (command, args, Shell::Program(shell))
    } else {
        let [step] = plan.steps.as_slice() else {
            return Err(RunTaskError::new("", "the plan has no command"));
        };
        let command = checked_program(&step.program)?;
        (command, quoted_args(step, user_shell)?, Shell::System)
    };
    let label = format!("{} #{run_number}", plan.label);
    Ok(SpawnInTerminal {
        id: TaskId(format!("devices-run-{run_number}-{}", plan.label)),
        full_label: label.clone(),
        label,
        command: Some(command),
        args,
        cwd: Some(plan.cwd.clone()),
        env: plan.env.iter().cloned().collect(),
        use_new_terminal: true,
        allow_concurrent_runs: true,
        reveal: RevealStrategy::Always,
        hide: HideStrategy::Never,
        shell,
        show_summary: true,
        show_command: true,
        show_rerun: true,
        ..Default::default()
    })
}

/// The steps joined by `&&`, a token of its own and never quoted, then the
/// script that opens the installed Android app.
fn chained_command_line(
    plan: &RunPlan,
    shell_kind: ShellKind,
) -> Result<(String, Vec<String>), RunTaskError> {
    let mut steps = plan.steps.iter();
    let Some(first) = steps.next() else {
        return Err(RunTaskError::new("", "the plan has no command"));
    };
    let command = chained_program(&first.program, shell_kind)?;
    let mut args = quoted_args(first, shell_kind)?;
    for step in steps {
        args.push("&&".to_string());
        args.push(chained_program(&step.program, shell_kind)?);
        args.extend(quoted_args(step, shell_kind)?);
    }
    if let Some(open) = &plan.open_installed_android_app {
        args.push("&&".to_string());
        args.push(open_android_app_script(open, shell_kind)?);
    }
    Ok((command, args))
}

const NOT_OPENED: &str =
    "Installed. Zode could not read the application id, so open the app on the device yourself.";

/// `%ComSpec%` when it names a `cmd.exe` by an absolute path, else the one in
/// `%SystemRoot%\System32`. The file name is always spelled `cmd.exe`: the
/// terminal tells cmd from PowerShell by the file stem, case-sensitively.
pub(crate) fn windows_command_interpreter(
    comspec: Option<&str>,
    system_root: Option<&str>,
) -> String {
    if let Some(comspec) = comspec.filter(|path| {
        is_windows_absolute(path)
            && path
                .rsplit(['\\', '/'])
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
    }) && let Some((folder, _)) = comspec.rsplit_once(['\\', '/'])
    {
        return format!("{folder}\\cmd.exe");
    }
    let system_root = system_root
        .filter(|root| is_windows_absolute(root))
        .unwrap_or("C:\\Windows");
    format!("{}\\System32\\cmd.exe", system_root.trim_end_matches('\\'))
}

/// `C:\`, `C:/` or a UNC `\\server\share` path.
fn is_windows_absolute(path: &str) -> bool {
    let mut characters = path.chars();
    let drive = characters
        .next()
        .is_some_and(|letter| letter.is_ascii_alphabetic())
        && characters.next() == Some(':')
        && matches!(characters.next(), Some('\\' | '/'));
    drive || path.starts_with("\\\\")
}

/// One letter, digit, `_` or `.`, spelled out: findstr's ranges follow the
/// collation order and let other characters through.
const CMD_APPLICATION_ID_CHARACTER: &str =
    "[abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.]";

/// Reads `applicationId` from the build's `output-metadata.json`, keeping only
/// an id made of letters, digits, `_` and `.`, and opens it with monkey. The
/// id comes from the build, not `build.gradle`, so `applicationIdSuffix` is
/// included.
fn open_android_app_script(
    open: &OpenInstalledAndroidApp,
    shell_kind: ShellKind,
) -> Result<String, RunTaskError> {
    let adb = chained_program(&open.adb.to_string_lossy(), shell_kind)?;
    let serial = quote(&open.serial, shell_kind)?;
    let metadata = quote(&open.output_metadata, shell_kind)?;
    let launch = format!("{adb} -s {serial} shell monkey");
    let category = "-c android.intent.category.LAUNCHER 1";
    Ok(if shell_kind == ShellKind::Cmd {
        // The line must start with the id, so the second token `for /f`
        // takes is that id: findstr reads `\"` as a quote, and the quotes
        // stay balanced for cmd. Only the start is anchored: findstr's end
        // anchor needs a CR, and AGP writes the file with LF on Windows too. Nothing is piped, since each side
        // of a pipe is a new cmd that would parse the id again; a `for`
        // variable is expanded after this cmd parsed the line, so the id
        // reaches adb as text. adb joins `shell` arguments into one command
        // on the device, hence the strict match.
        let search = format!(
            "findstr /r /b /c:\" *\\\"applicationId\\\": \\\"{CMD_APPLICATION_ID_CHARACTER}\
             {CMD_APPLICATION_ID_CHARACTER}*\\\",\" {metadata}"
        );
        format!(
            "({search} >nul 2>&1 && (for /f \"tokens=2 delims=:, \" %i in ('{search}') do \
             {launch} -p %~i {category}) || echo {NOT_OPENED})"
        )
    } else {
        format!(
            "{{ id=$(sed -n 's/^[[:space:]]*\"applicationId\"[[:space:]]*:[[:space:]]*\
             \"\\([A-Za-z0-9][A-Za-z0-9_.]*\\)\".*/\\1/p' {metadata} 2>/dev/null | head -n 1); \
             if [ -n \"$id\" ]; then {launch} -p \"$id\" {category}; \
             else echo '{NOT_OPENED}'; fi; }}"
        )
    })
}

/// The project's own wrapper (`./gradlew`) or an absolute tool path, quoted
/// for the chain's shell. cmd.exe takes plain quotes: its escaped `^"` would
/// not keep a path with spaces together as the program name. An absolute
/// path is always quoted there, since `&`, `(` or `)` in a folder name would
/// otherwise end the command, and its separators are made backslashes.
fn chained_program(program: &str, shell_kind: ShellKind) -> Result<String, RunTaskError> {
    let relative_wrapper = ["./", ".\\"]
        .iter()
        .find_map(|prefix| program.strip_prefix(prefix))
        .is_some_and(|name| checked_program(name).is_ok());
    let windows_absolute = is_windows_absolute(program);
    let absolute = program.starts_with('/') || windows_absolute;
    if !(relative_wrapper || absolute || checked_program(program).is_ok()) {
        return Err(RunTaskError::new(
            program,
            "it is not a program name, the project's wrapper or an absolute path",
        ));
    }
    if let Some(character) = program
        .chars()
        .find(|character| matches!(character, '"' | '%' | '$' | '`') || character.is_control())
    {
        return Err(RunTaskError::new(
            program,
            format!("it contains {character:?}, which the shell would interpret"),
        ));
    }
    if shell_kind != ShellKind::Cmd {
        return quote(program, shell_kind);
    }
    // `^` is cmd's escape character, which quotes do not always keep
    // literal.
    if let Some(character) = program.chars().find(|character| *character == '^') {
        return Err(RunTaskError::new(
            program,
            format!("it contains {character:?}, which cmd would interpret"),
        ));
    }
    Ok(if windows_absolute {
        format!("\"{}\"", program.replace('/', "\\"))
    } else if program.contains(' ') {
        format!("\"{program}\"")
    } else {
        program.to_string()
    })
}

/// Programs come from the core as bare names. Quoting one would turn it into
/// a string in PowerShell, so instead only plain names are let through.
fn checked_program(program: &str) -> Result<String, RunTaskError> {
    let plain = !program.is_empty()
        && program
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        && !program.starts_with(['-', '.']);
    if plain {
        Ok(program.to_string())
    } else {
        Err(RunTaskError::new(program, "it is not a plain program name"))
    }
}

fn quoted_args(step: &RunStep, shell_kind: ShellKind) -> Result<Vec<String>, RunTaskError> {
    step.args
        .iter()
        .map(|argument| quote(argument, shell_kind))
        .collect()
}

/// `$` and `%` are refused before quoting: the panel later passes every
/// argument through `ShellKind::to_shell_variable`, which rewrites them for
/// PowerShell, cmd and Nushell even inside quotes. A backtick is PowerShell's
/// escape character and a command substitution in Posix shells.
fn quote(argument: &str, shell_kind: ShellKind) -> Result<String, RunTaskError> {
    if let Some(character) = argument
        .chars()
        .find(|character| matches!(character, '$' | '%' | '`'))
    {
        return Err(RunTaskError::new(
            argument,
            format!("it contains \"{character}\", which the shell would expand"),
        ));
    }
    shell_kind
        .try_quote(argument)
        .map(|quoted| quoted.into_owned())
        .ok_or_else(|| RunTaskError::new(argument, format!("{shell_kind:?} cannot quote it")))
}
