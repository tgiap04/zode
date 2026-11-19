//! What a compose project row and a compose service row offer, decided from
//! plain facts.
//!
//! Plain functions of a [`ComposeContext`], like `row_menu_items`: "Up needs a
//! file to bring up" and "Open Compose File is off for a remote file outside
//! the project's folders" are statements a test makes without drawing a menu.
//! Nothing here asks which engine it is holding -- the engine's answers arrive
//! as `offered` and `plugin`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use container::{ComposeLabels, ComposeOp, ComposeTarget, Resource, ResourceAction, RunState};
use ui::IconName;

use crate::render::action_is_useful;
use crate::row_menu::{MenuItem, grouped};

const NO_FILE: &str = "No compose file is recorded for this project";
const NO_DIRECTORY: &str = "No working directory is recorded for this project";
const REMOTE_DIRECTORY: &str = "The working directory is on a remote host";
const FILE_OUTSIDE: &str = "This file is on a remote host, outside the project's folders";
const NO_SUPPORT: &str = "This engine has no compose support";
const NO_PLUGIN: &str = "The compose plugin is not installed";
const PROBING: &str = "Checking for the compose plugin\u{2026}";
const NOTHING_RUNNING: &str = "No container of this service is running";
const NO_SHELL: &str = "This engine cannot open a shell here";

/// Whether the compose tool answered, as far as the panel has asked.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Plugin {
    /// Not asked yet, or asked and not answered.
    #[default]
    Unknown,
    Present,
    Missing,
}

/// One thing a compose entry does.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ComposeCommand {
    /// A compose verb on the whole project.
    Run(ComposeOp),
    /// A compose verb on the one service the row stands for.
    RunService(ComposeOp),
    /// A shell in a running container of the service.
    ServiceShell,
    /// `compose down`, through the confirmation.
    Down,
    OpenFile,
    RevealDirectory,
    /// Expands or collapses the project's services. `expanded` is how the row
    /// is *now*, so the label says what pressing it does.
    ToggleServices {
        expanded: bool,
    },
}

impl ComposeCommand {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ComposeCommand::Run(op) => match op {
                ComposeOp::Up => "Up",
                ComposeOp::Restart => "Restart",
                ComposeOp::Stop => "Stop",
                ComposeOp::Start => "Start",
                ComposeOp::Pull => "Pull Images",
                ComposeOp::Build => "Build",
                ComposeOp::Logs => "Logs",
            },
            ComposeCommand::RunService(ComposeOp::Logs) => "Follow Logs",
            ComposeCommand::RunService(op) => ComposeCommand::Run(op).label(),
            ComposeCommand::ServiceShell => "Open Shell",
            ComposeCommand::Down => "Down\u{2026}",
            ComposeCommand::OpenFile => "Open Compose File",
            ComposeCommand::RevealDirectory => "Reveal Working Directory",
            ComposeCommand::ToggleServices { expanded: false } => "Show Services",
            ComposeCommand::ToggleServices { expanded: true } => "Hide Services",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            ComposeCommand::Run(op) | ComposeCommand::RunService(op) => match op {
                ComposeOp::Up => IconName::ArrowUp,
                ComposeOp::Restart => IconName::RotateCw,
                ComposeOp::Stop => IconName::Power,
                ComposeOp::Start => IconName::PlayFilled,
                ComposeOp::Pull => IconName::Download,
                ComposeOp::Build => IconName::ToolHammer,
                ComposeOp::Logs => IconName::ListTree,
            },
            ComposeCommand::ServiceShell => IconName::Terminal,
            ComposeCommand::Down => IconName::ArrowDown,
            ComposeCommand::OpenFile => IconName::FileCode,
            ComposeCommand::RevealDirectory => IconName::FolderOpen,
            ComposeCommand::ToggleServices { expanded: false } => IconName::ChevronRight,
            ComposeCommand::ToggleServices { expanded: true } => IconName::ChevronDown,
        }
    }

    /// Whether the entry needs the compose tool to be there. Opening a file,
    /// revealing a folder and a shell in a container do not.
    fn needs_compose(self) -> bool {
        matches!(
            self,
            ComposeCommand::Run(_) | ComposeCommand::RunService(_) | ComposeCommand::Down
        )
    }

    /// Bringing up, pulling and building read the project's files; the others
    /// act on what already exists and find it by its labels.
    fn reads_files(self) -> bool {
        matches!(
            self,
            ComposeCommand::Run(ComposeOp::Up | ComposeOp::Pull | ComposeOp::Build)
                | ComposeCommand::RunService(ComposeOp::Up | ComposeOp::Pull | ComposeOp::Build)
        )
    }
}

/// One entry, and why it cannot be pressed when it cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposeEntry {
    pub(crate) command: ComposeCommand,
    pub(crate) blocked: Option<String>,
}

/// Everything an entry's availability depends on, for one row.
#[derive(Clone, Debug)]
pub(crate) struct ComposeContext {
    pub(crate) offered: bool,
    pub(crate) plugin: Plugin,
    pub(crate) remote: bool,
    pub(crate) has_files: bool,
    pub(crate) has_directory: bool,
    /// The project's files are on a remote host and at least one lies outside
    /// the folders the project has open, so the editor cannot reach it.
    pub(crate) files_outside: bool,
    pub(crate) expanded: bool,
    pub(crate) state: RunState,
    pub(crate) member_running: bool,
    /// The engine has a shell command for a container.
    pub(crate) shell: bool,
}

impl ComposeContext {
    pub(crate) fn blocker(&self, command: ComposeCommand) -> Option<String> {
        if command.needs_compose() {
            if !self.offered {
                return Some(NO_SUPPORT.to_string());
            }
            match self.plugin {
                Plugin::Missing => return Some(NO_PLUGIN.to_string()),
                Plugin::Unknown => return Some(PROBING.to_string()),
                Plugin::Present => {}
            }
        }
        let reason = match command {
            _ if command.reads_files() && !self.has_files && !self.has_directory => NO_FILE,
            ComposeCommand::OpenFile if !self.has_files => NO_FILE,
            ComposeCommand::OpenFile if self.files_outside => FILE_OUTSIDE,
            ComposeCommand::RevealDirectory if !self.has_directory => NO_DIRECTORY,
            ComposeCommand::RevealDirectory if self.remote => REMOTE_DIRECTORY,
            ComposeCommand::ServiceShell if !self.shell => NO_SHELL,
            ComposeCommand::ServiceShell if !self.member_running => NOTHING_RUNNING,
            _ => return None,
        };
        Some(reason.to_string())
    }

    fn entry(&self, command: ComposeCommand) -> ComposeEntry {
        ComposeEntry {
            command,
            blocked: self.blocker(command),
        }
    }

    fn item(&self, command: ComposeCommand) -> MenuItem {
        MenuItem::Compose(self.entry(command))
    }

    /// Start, Stop and Restart as the state makes sense of them: the engine
    /// refuses the others, same as for a container row.
    fn lifecycle(&self, wrap: fn(ComposeOp) -> ComposeCommand) -> Vec<ComposeCommand> {
        [
            (ResourceAction::Start, ComposeOp::Start),
            (ResourceAction::Stop, ComposeOp::Stop),
            (ResourceAction::Restart, ComposeOp::Restart),
        ]
        .into_iter()
        .filter(|(action, _)| action_is_useful(*action, self.state))
        .map(|(_, op)| wrap(op))
        .collect()
    }

    /// The buttons at the end of a project row, in order. Collapsing is a
    /// button of its own, drawn first by the caller.
    pub(crate) fn project_buttons(&self) -> Vec<ComposeEntry> {
        let mut commands = vec![ComposeCommand::Run(ComposeOp::Up)];
        commands.extend(self.lifecycle(ComposeCommand::Run));
        commands.extend([
            ComposeCommand::Run(ComposeOp::Pull),
            ComposeCommand::Run(ComposeOp::Build),
            ComposeCommand::Run(ComposeOp::Logs),
            ComposeCommand::Down,
        ]);
        commands
            .into_iter()
            .map(|command| self.entry(command))
            .collect()
    }

    /// The buttons at the end of a service row.
    pub(crate) fn service_buttons(&self) -> Vec<ComposeEntry> {
        let mut commands = self.lifecycle(ComposeCommand::RunService);
        commands.push(ComposeCommand::RunService(ComposeOp::Logs));
        if self.member_running {
            commands.push(ComposeCommand::ServiceShell);
        }
        commands
            .into_iter()
            .map(|command| self.entry(command))
            .collect()
    }

    pub(crate) fn project_menu_items(&self) -> Vec<MenuItem> {
        let mut manage = vec![self.item(ComposeCommand::Run(ComposeOp::Up))];
        manage.extend(
            self.lifecycle(ComposeCommand::Run)
                .into_iter()
                .map(|command| self.item(command)),
        );
        grouped(vec![
            manage,
            vec![
                self.item(ComposeCommand::Run(ComposeOp::Pull)),
                self.item(ComposeCommand::Run(ComposeOp::Build)),
            ],
            vec![
                self.item(ComposeCommand::Run(ComposeOp::Logs)),
                self.item(ComposeCommand::ToggleServices {
                    expanded: self.expanded,
                }),
            ],
            vec![
                self.item(ComposeCommand::OpenFile),
                self.item(ComposeCommand::RevealDirectory),
            ],
            vec![self.item(ComposeCommand::Down)],
            vec![MenuItem::CopyName],
        ])
    }

    pub(crate) fn service_menu_items(&self) -> Vec<MenuItem> {
        let mut manage: Vec<MenuItem> = self
            .lifecycle(ComposeCommand::RunService)
            .into_iter()
            .map(|command| self.item(command))
            .collect();
        manage.push(self.item(ComposeCommand::RunService(ComposeOp::Up)));
        grouped(vec![
            manage,
            vec![
                self.item(ComposeCommand::RunService(ComposeOp::Logs)),
                self.item(ComposeCommand::ServiceShell),
            ],
            vec![MenuItem::CopyName],
        ])
    }
}

/// A service row: a compose project row with a `parent`.
pub(crate) fn is_service_row(resource: &Resource) -> bool {
    resource.kind == container::ResourceKind::ComposeProject && resource.parent.is_some()
}

/// What `ComposeLabels` a service row stands for.
pub(crate) fn service_labels(resource: &Resource) -> Option<&ComposeLabels> {
    resource
        .compose
        .as_ref()
        .filter(|_| is_service_row(resource))
}

/// The detail label a service row carries its project's files under. Not a
/// column, so it is never drawn; it is there so a service row alone is enough
/// to build the project's [`ComposeTarget`].
pub(crate) const PROJECT_CONFIG: &str = "ProjectConfig";

fn detail<'a>(resource: &'a Resource, label: &str) -> &'a str {
    resource
        .detail
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, value)| value.as_str())
        .unwrap_or_default()
}

/// The directory some container of `project` was brought up from.
pub(crate) fn working_directory<'a>(members: &'a [Resource], project: &str) -> Option<&'a str> {
    members
        .iter()
        .filter_map(|member| member.compose.as_ref())
        .filter(|labels| labels.project == project)
        .find_map(|labels| labels.working_dir.as_deref())
        .filter(|directory| !directory.is_empty())
}

/// `project`'s files and directory as one target, from whichever row stands
/// for it.
pub(crate) fn target_of(row: &Resource, members: &[Resource]) -> Option<ComposeTarget> {
    if let Some(labels) = service_labels(row) {
        return Some(
            ComposeTarget::new(labels.project.clone())
                .with_config_text(detail(row, PROJECT_CONFIG))
                .with_working_dir(labels.working_dir.clone()),
        );
    }
    let directory = working_directory(members, &row.id).map(String::from);
    Some(ComposeTarget::from_project(row)?.with_working_dir(directory))
}

/// The members of `project` that are services of one name.
fn members_of<'a>(
    members: &'a [Resource],
    project: &'a str,
) -> impl Iterator<Item = (&'a Resource, &'a ComposeLabels)> {
    members.iter().filter_map(move |member| {
        let labels = member.compose.as_ref()?;
        (labels.project == project).then_some((member, labels))
    })
}

/// A running container of the service `labels` names.
pub(crate) fn running_member<'a>(
    members: &'a [Resource],
    labels: &ComposeLabels,
) -> Option<&'a Resource> {
    members.iter().find(|member| {
        member.state == RunState::Running
            && member.compose.as_ref().is_some_and(|member_labels| {
                member_labels.project == labels.project && member_labels.service == labels.service
            })
    })
}

/// The services of `project`, one row each, sorted by name.
///
/// Rows of the same kind as the project with a `parent`, so the list draws them
/// with the columns it already has; the selection never contains one, because
/// they are added after it is decided what is selectable.
pub(crate) fn service_rows(project: &Resource, members: &[Resource]) -> Vec<Resource> {
    let mut services: BTreeMap<&str, Vec<(&Resource, &ComposeLabels)>> = BTreeMap::new();
    for (member, labels) in members_of(members, &project.name) {
        services
            .entry(&labels.service)
            .or_default()
            .push((member, labels));
    }
    let config = detail(project, "Config").to_string();
    services
        .into_iter()
        .map(|(service, containers)| {
            let running = containers
                .iter()
                .filter(|(member, _)| member.state == RunState::Running)
                .count();
            let state = if running > 0 {
                RunState::Running
            } else if containers
                .iter()
                .any(|(member, _)| member.state == RunState::Paused)
            {
                RunState::Paused
            } else if containers
                .iter()
                .all(|(member, _)| member.state == RunState::Unknown)
            {
                RunState::Unknown
            } else {
                RunState::Stopped
            };
            let working_dir = containers
                .iter()
                .find_map(|(_, labels)| labels.working_dir.clone());
            let mut detail = vec![("Status", format!("{running}/{} running", containers.len()))];
            if !config.is_empty() {
                detail.push((PROJECT_CONFIG, config.clone()));
            }
            Resource {
                kind: project.kind,
                id: format!("{}/{service}", project.id),
                name: service.to_string(),
                state,
                detail,
                parent: Some(project.id.clone()),
                compose: Some(ComposeLabels {
                    project: project.name.clone(),
                    service: service.to_string(),
                    working_dir,
                    config_files: Vec::new(),
                }),
            }
        })
        .collect()
}

/// The list as it is drawn: each project, and under an expanded one its
/// services.
pub(crate) fn with_services(
    projects: Vec<Resource>,
    members: &[Resource],
    expanded: &BTreeSet<String>,
) -> Vec<Resource> {
    if expanded.is_empty() {
        return projects;
    }
    let mut rows = Vec::with_capacity(projects.len());
    for project in projects {
        let open = expanded.contains(&project.id);
        let services = open.then(|| service_rows(&project, members));
        rows.push(project);
        rows.extend(services.into_iter().flatten());
    }
    rows
}

/// Whether every one of `files` lies inside one of `roots`.
pub(crate) fn all_inside(files: &[String], roots: &[PathBuf]) -> bool {
    files
        .iter()
        .all(|file| roots.iter().any(|root| Path::new(file).starts_with(root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use container::ResourceKind;

    fn context() -> ComposeContext {
        ComposeContext {
            offered: true,
            plugin: Plugin::Present,
            remote: false,
            has_files: true,
            has_directory: true,
            files_outside: false,
            expanded: false,
            state: RunState::Running,
            member_running: true,
            shell: true,
        }
    }

    fn blocked(items: &[MenuItem], wanted: ComposeCommand) -> Option<Option<String>> {
        items.iter().find_map(|item| match item {
            MenuItem::Compose(entry) if entry.command == wanted => Some(entry.blocked.clone()),
            _ => None,
        })
    }

    fn member(name: &str, project: &str, service: &str, state: RunState) -> Resource {
        Resource {
            kind: ResourceKind::Container,
            id: format!("id-{name}"),
            name: name.into(),
            state,
            detail: Vec::new(),
            parent: None,
            compose: Some(ComposeLabels {
                project: project.into(),
                service: service.into(),
                working_dir: Some(format!("/work/{project}")),
                config_files: Vec::new(),
            }),
        }
    }

    fn project(name: &str) -> Resource {
        Resource {
            kind: ResourceKind::ComposeProject,
            id: name.into(),
            name: name.into(),
            state: RunState::Running,
            detail: vec![("Config", "/work/shop/compose.yaml".into())],
            parent: None,
            compose: None,
        }
    }

    #[test]
    fn a_project_menu_offers_every_verb_and_all_are_open_when_compose_is_there() {
        let items = context().project_menu_items();
        for wanted in [
            ComposeCommand::Run(ComposeOp::Up),
            ComposeCommand::Run(ComposeOp::Stop),
            ComposeCommand::Run(ComposeOp::Restart),
            ComposeCommand::Run(ComposeOp::Pull),
            ComposeCommand::Run(ComposeOp::Build),
            ComposeCommand::Run(ComposeOp::Logs),
            ComposeCommand::Down,
            ComposeCommand::OpenFile,
            ComposeCommand::RevealDirectory,
            ComposeCommand::ToggleServices { expanded: false },
        ] {
            assert_eq!(blocked(&items, wanted), Some(None), "{wanted:?}");
        }
        assert_eq!(
            blocked(&items, ComposeCommand::Run(ComposeOp::Start)),
            None,
            "a running project has nothing to start"
        );
        assert!(items.contains(&MenuItem::CopyName));
    }

    #[test]
    fn a_stopped_project_starts_and_neither_stops_nor_restarts() {
        let items = ComposeContext {
            state: RunState::Stopped,
            ..context()
        }
        .project_menu_items();
        assert_eq!(
            blocked(&items, ComposeCommand::Run(ComposeOp::Start)),
            Some(None)
        );
        assert_eq!(blocked(&items, ComposeCommand::Run(ComposeOp::Stop)), None);
        assert_eq!(
            blocked(&items, ComposeCommand::Run(ComposeOp::Restart)),
            None
        );
    }

    #[test]
    fn without_the_plugin_every_verb_and_down_say_so_and_the_file_entries_stay_open() {
        for (plugin, offered, words) in [
            (Plugin::Missing, true, "not installed"),
            (Plugin::Unknown, true, "Checking"),
            (Plugin::Present, false, "no compose support"),
        ] {
            let items = ComposeContext {
                plugin,
                offered,
                ..context()
            }
            .project_menu_items();
            for wanted in [
                ComposeCommand::Run(ComposeOp::Up),
                ComposeCommand::Run(ComposeOp::Logs),
                ComposeCommand::Down,
            ] {
                let reason = blocked(&items, wanted).flatten().expect("blocked");
                assert!(reason.contains(words), "{wanted:?}: {reason}");
            }
            assert_eq!(
                blocked(&items, ComposeCommand::OpenFile),
                Some(None),
                "reading a file needs no plugin"
            );
        }
    }

    #[test]
    fn with_no_file_label_up_pull_build_and_open_file_say_so_but_stop_does_not() {
        let items = ComposeContext {
            has_files: false,
            has_directory: false,
            ..context()
        }
        .project_menu_items();
        for wanted in [
            ComposeCommand::Run(ComposeOp::Up),
            ComposeCommand::Run(ComposeOp::Pull),
            ComposeCommand::Run(ComposeOp::Build),
            ComposeCommand::OpenFile,
        ] {
            let reason = blocked(&items, wanted).flatten().expect("blocked");
            assert!(reason.contains("No compose file"), "{wanted:?}: {reason}");
        }
        assert_eq!(
            blocked(&items, ComposeCommand::Run(ComposeOp::Stop)),
            Some(None),
            "found by its labels"
        );
        assert!(
            blocked(&items, ComposeCommand::RevealDirectory)
                .flatten()
                .expect("blocked")
                .contains("working directory")
        );
    }

    #[test]
    fn a_directory_alone_is_enough_to_bring_a_project_up() {
        let items = ComposeContext {
            has_files: false,
            ..context()
        }
        .project_menu_items();
        assert_eq!(
            blocked(&items, ComposeCommand::Run(ComposeOp::Up)),
            Some(None)
        );
    }

    #[test]
    fn a_remote_project_opens_its_files_only_inside_the_folders_it_has_open() {
        let items = ComposeContext {
            remote: true,
            files_outside: true,
            ..context()
        }
        .project_menu_items();
        assert!(
            blocked(&items, ComposeCommand::OpenFile)
                .flatten()
                .expect("blocked")
                .contains("outside the project")
        );
        assert!(
            blocked(&items, ComposeCommand::RevealDirectory)
                .flatten()
                .expect("blocked")
                .contains("remote host"),
            "the file manager is on this machine, the directory is not"
        );

        let roots = [PathBuf::from("/srv/app")];
        assert!(all_inside(&["/srv/app/compose.yaml".into()], &roots));
        assert!(!all_inside(
            &["/srv/app/compose.yaml".into(), "/etc/compose.yaml".into()],
            &roots
        ));
        assert!(!all_inside(&["/srv/application/x.yaml".into()], &roots));
    }

    #[test]
    fn a_service_menu_has_the_lifecycle_up_logs_and_a_shell_only_while_something_runs() {
        let items = context().service_menu_items();
        for wanted in [
            ComposeCommand::RunService(ComposeOp::Stop),
            ComposeCommand::RunService(ComposeOp::Restart),
            ComposeCommand::RunService(ComposeOp::Up),
            ComposeCommand::RunService(ComposeOp::Logs),
            ComposeCommand::ServiceShell,
        ] {
            assert_eq!(blocked(&items, wanted), Some(None), "{wanted:?}");
        }

        let idle = ComposeContext {
            state: RunState::Stopped,
            member_running: false,
            ..context()
        };
        let items = idle.service_menu_items();
        assert_eq!(
            blocked(&items, ComposeCommand::ServiceShell)
                .flatten()
                .as_deref(),
            Some(NOTHING_RUNNING)
        );
        assert_eq!(
            blocked(&items, ComposeCommand::RunService(ComposeOp::Start)),
            Some(None)
        );
        assert!(
            !idle
                .service_buttons()
                .iter()
                .any(|entry| entry.command == ComposeCommand::ServiceShell)
        );
    }

    #[test]
    fn services_group_their_containers_and_summarise_them() {
        let members = vec![
            member("shop-web-1", "shop", "web", RunState::Running),
            member("shop-web-2", "shop", "web", RunState::Stopped),
            member("shop-db-1", "shop", "db", RunState::Stopped),
            member("other-api-1", "other", "api", RunState::Running),
        ];
        let rows = service_rows(&project("shop"), &members);
        let summary: Vec<_> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.state, row.detail[0].1.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("db", RunState::Stopped, "0/1 running"),
                ("web", RunState::Running, "1/2 running"),
            ]
        );
        assert!(rows.iter().all(is_service_row));
        assert_eq!(rows[0].id, "shop/db");
        assert_eq!(
            service_labels(&rows[1]).map(|labels| labels.working_dir.as_deref()),
            Some(Some("/work/shop"))
        );
    }

    #[test]
    fn only_an_expanded_project_shows_services_and_they_follow_it() {
        let members = vec![member("shop-web-1", "shop", "web", RunState::Running)];
        let projects = vec![project("shop"), project("blog")];
        let none = BTreeSet::new();
        assert_eq!(with_services(projects.clone(), &members, &none).len(), 2);

        let expanded = BTreeSet::from(["shop".to_string()]);
        let names: Vec<_> = with_services(projects, &members, &expanded)
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(names, ["shop", "shop/web", "blog"]);
    }

    #[test]
    fn a_service_row_alone_rebuilds_its_projects_target() {
        let members = vec![member("shop-web-1", "shop", "web", RunState::Running)];
        let rows = service_rows(&project("shop"), &members);
        let target = target_of(&rows[0], &members).expect("a target");
        assert_eq!(target.project, "shop");
        assert_eq!(target.config_files, ["/work/shop/compose.yaml"]);
        assert_eq!(target.working_dir.as_deref(), Some("/work/shop"));

        let target = target_of(&project("shop"), &members).expect("a target");
        assert_eq!(target.working_dir.as_deref(), Some("/work/shop"));
        assert!(
            target_of(&members[0], &members).is_none(),
            "a container is not a project"
        );
    }
}
