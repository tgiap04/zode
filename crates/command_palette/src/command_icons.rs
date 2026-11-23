//! The icon beside each command in the palette.
//!
//! Commands are named `namespace: action` after their action, so the icon is
//! read off that name: a namespace that is a feature of its own (git, terminal,
//! the debugger) says enough by itself; the broad ones (editor, workspace, the
//! app) say what the command does through its verb.

use ui::IconName;

/// Namespaces whose commands are all about one thing. Matched as substrings of
/// the namespace, first match wins, so the more specific entries come first.
const BY_NAMESPACE: &[(&str, IconName)] = &[
    ("git graph", IconName::GitGraph),
    ("worktree", IconName::GitWorktree),
    ("branch", IconName::GitBranch),
    ("commit", IconName::GitCommit),
    ("git", IconName::GitBranch),
    ("project diff", IconName::Diff),
    ("diff", IconName::Diff),
    ("terminal", IconName::Terminal),
    ("debug", IconName::Debug),
    ("repl", IconName::ReplNeutral),
    ("task", IconName::PlayOutlined),
    ("search", IconName::MagnifyingGlass),
    ("file finder", IconName::MagnifyingGlass),
    ("project panel", IconName::FileTree),
    ("outline", IconName::ListTree),
    ("recent projects", IconName::FolderOpen),
    ("projects", IconName::FolderOpen),
    ("agent", IconName::Sparkle),
    ("assistant", IconName::Sparkle),
    ("acp", IconName::Sparkle),
    ("edit prediction", IconName::ZedPredict),
    ("account", IconName::Person),
    ("collab", IconName::Person),
    ("channel", IconName::Person),
    ("auto update", IconName::CloudDownload),
    ("icon theme", IconName::Palette),
    ("theme", IconName::Palette),
    ("keymap", IconName::Keyboard),
    ("settings", IconName::Settings),
    ("vim", IconName::Keyboard),
    ("helix", IconName::Keyboard),
    ("database", IconName::Database),
    ("container", IconName::Docker),
    ("docker", IconName::Docker),
    ("remote", IconName::Server),
    ("dev container", IconName::Docker),
    ("diagnostics", IconName::Warning),
    ("markdown", IconName::FileMarkdown),
    ("lsp", IconName::Code),
    ("language", IconName::Code),
    ("toolchain", IconName::ToolHammer),
    ("snippet", IconName::TextSnippet),
    ("journal", IconName::Notepad),
    ("image", IconName::Image),
    ("extension", IconName::Blocks),
    ("notification", IconName::ToolNotification),
    ("reminder", IconName::CountdownTimer),
    ("keep awake", IconName::Screen),
    ("env", IconName::LockOutlined),
    ("encoding", IconName::FileTextOutlined),
    ("line ending", IconName::FileTextOutlined),
    ("tab switcher", IconName::Tab),
    ("notebook", IconName::Notepad),
    ("feedback", IconName::Envelope),
    ("cli", IconName::Terminal),
    ("console", IconName::Terminal),
    ("keystroke", IconName::Keyboard),
    ("variable", IconName::Debug),
    ("stash", IconName::Archive),
    ("zed predict", IconName::ZedPredict),
];

/// Namespaces too broad for their name to say anything; their commands go by verb.
const GENERIC_NAMESPACES: &[&str] = &[
    "editor",
    "workspace",
    "pane",
    "zode",
    "app",
    "menu",
    "window",
    "project",
    "picker",
];

/// Words in the action part, matched as whole words (a trailing "s" allowed),
/// first match wins: nouns that name what a command acts on before verbs, and
/// bare directions last.
const BY_ACTION: &[(&str, IconName)] = &[
    ("settings", IconName::Settings),
    ("keymap", IconName::Keyboard),
    ("key binding", IconName::Keyboard),
    ("keystroke", IconName::Keyboard),
    ("breakpoint", IconName::DebugBreakpoint),
    ("debug", IconName::Debug),
    ("diff hunk", IconName::Diff),
    ("excerpt", IconName::ExpandVertical),
    ("bookmark", IconName::Bookmark),
    ("mark", IconName::Bookmark),
    ("notification", IconName::ToolNotification),
    ("extension", IconName::Blocks),
    ("database", IconName::Database),
    ("log", IconName::FileTextOutlined),
    ("folder", IconName::FolderOpen),
    ("file manager", IconName::FolderOpen),
    ("project panel", IconName::FileTree),
    ("source header", IconName::FileCode),
    ("task", IconName::PlayOutlined),
    ("snippet", IconName::TextSnippet),
    ("uuid", IconName::TextSnippet),
    ("completion", IconName::Code),
    ("edit prediction", IconName::ZedPredict),
    ("code action", IconName::ToolHammer),
    ("language server", IconName::Code),
    ("flycheck", IconName::Code),
    ("macro", IconName::Code),
    ("import", IconName::Code),
    ("trusted", IconName::LockOutlined),
    ("cursor", IconName::CursorIBeam),
    ("dock size", IconName::Maximize),
    ("feedback", IconName::Envelope),
    ("about", IconName::Info),
    ("hover", IconName::Info),
    ("crash", IconName::Warning),
    ("panic", IconName::Warning),
    ("selection", IconName::SelectAll),
    ("newline", IconName::Return),
    ("backspace", IconName::Backspace),
    ("unfold", IconName::ExpandVertical),
    ("bug", IconName::Warning),
    ("watch", IconName::Eye),
    ("reopen", IconName::RotateCcw),
    ("interrupt", IconName::Stop),
    ("dismiss", IconName::Close),
    ("finish", IconName::Check),
    ("drop", IconName::Trash),
    ("merge", IconName::ArrowRightLeft),
    ("new window", IconName::OpenNewWindow),
    ("split", IconName::Split),
    ("close", IconName::Close),
    ("cancel", IconName::Close),
    ("quit", IconName::Power),
    ("new", IconName::Plus),
    ("insert", IconName::Plus),
    ("save", IconName::Check),
    ("confirm", IconName::Check),
    ("accept", IconName::Check),
    ("apply", IconName::Check),
    ("copy", IconName::Copy),
    ("duplicate", IconName::Copy),
    ("paste", IconName::Copy),
    ("yank", IconName::Copy),
    ("cut", IconName::Scissors),
    ("undo", IconName::Undo),
    ("redo", IconName::RotateCw),
    ("delete", IconName::Trash),
    ("remove", IconName::Trash),
    ("clear", IconName::Eraser),
    ("reset", IconName::RotateCcw),
    ("rename", IconName::Pencil),
    ("convert", IconName::CaseSensitive),
    ("sort", IconName::ArrowDown10),
    ("shuffle", IconName::ArrowDown10),
    ("reverse", IconName::ArrowDown10),
    ("unique", IconName::Filter),
    ("font size", IconName::FontSize),
    ("zoom", IconName::FontSize),
    ("indent", IconName::Tab),
    ("outdent", IconName::Tab),
    ("tab", IconName::Tab),
    ("backtab", IconName::Tab),
    ("wrap", IconName::Code),
    ("rewrap", IconName::Code),
    ("unwrap", IconName::Code),
    ("join", IconName::Link),
    ("select", IconName::SelectAll),
    ("find", IconName::MagnifyingGlass),
    ("search", IconName::MagnifyingGlass),
    ("replace", IconName::Replace),
    ("format", IconName::Code),
    ("comment", IconName::Code),
    ("fold", IconName::ChevronDownUp),
    ("expand", IconName::ExpandVertical),
    ("collapse", IconName::ChevronDownUp),
    ("reload", IconName::RotateCw),
    ("refresh", IconName::RotateCw),
    ("restart", IconName::RotateCw),
    ("run", IconName::PlayOutlined),
    ("spawn", IconName::PlayOutlined),
    ("stop", IconName::Stop),
    ("shutdown", IconName::Stop),
    ("hide", IconName::EyeOff),
    ("suppress", IconName::EyeOff),
    ("show", IconName::Eye),
    ("display", IconName::Eye),
    ("view", IconName::Eye),
    ("reveal", IconName::Eye),
    ("restore", IconName::RotateCcw),
    ("toggle", IconName::ChevronUpDown),
    ("maximize", IconName::Maximize),
    ("minimize", IconName::Minimize),
    ("add", IconName::Plus),
    ("edit", IconName::Pencil),
    ("pin", IconName::Pin),
    ("unpin", IconName::Unpin),
    ("swap", IconName::ArrowRightLeft),
    ("transpose", IconName::ArrowRightLeft),
    ("alternate", IconName::ArrowRightLeft),
    ("follow", IconName::Person),
    ("unfollow", IconName::Person),
    ("activate", IconName::Crosshair),
    ("focus", IconName::Crosshair),
    ("send", IconName::Send),
    ("go to", IconName::ArrowRight),
    ("move", IconName::ArrowRight),
    ("next", IconName::ArrowDown),
    ("previous", IconName::ArrowUp),
    ("forward", IconName::ArrowRight),
    ("back", IconName::ArrowLeft),
    ("open", IconName::ArrowUpRight),
    ("down", IconName::ArrowDown),
    ("bottom", IconName::ArrowDown),
    ("up", IconName::ArrowUp),
    ("top", IconName::ArrowUp),
    ("center", IconName::Crosshair),
    ("left", IconName::ArrowLeft),
    ("right", IconName::ArrowRight),
];

/// Whether `text` holds `word` as whole words, a trailing "s" allowed.
fn has_word(text: &str, word: &str) -> bool {
    let padded = format!(" {text} ");
    padded.contains(&format!(" {word} ")) || padded.contains(&format!(" {word}s "))
}

fn by_action(action: &str) -> Option<IconName> {
    BY_ACTION
        .iter()
        .find(|(word, _)| has_word(action, word))
        .map(|(_, icon)| *icon)
}

/// The icon for a command named like `git: commit` (any case).
pub fn command_icon(name: &str) -> IconName {
    let name = name.to_lowercase();
    let (namespace, action) = name.split_once(':').unwrap_or(("", name.as_str()));
    let (namespace, action) = (namespace.trim(), action.trim());

    let generic = namespace.is_empty() || GENERIC_NAMESPACES.contains(&namespace);
    let from_namespace = || {
        BY_NAMESPACE
            .iter()
            .find(|(word, _)| namespace.contains(word))
            .map(|(_, icon)| *icon)
    };
    let icon = if generic {
        by_action(action).or_else(from_namespace)
    } else {
        from_namespace().or_else(|| by_action(action))
    };
    icon.unwrap_or(IconName::Command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_namespace_decides_the_icon() {
        assert_eq!(command_icon("git: commit"), IconName::GitBranch);
        assert_eq!(command_icon("git graph: open"), IconName::GitGraph);
        assert_eq!(
            command_icon("terminal panel: toggle focus"),
            IconName::Terminal
        );
        assert_eq!(command_icon("account: sign in"), IconName::Person);
        assert_eq!(command_icon("auto update: check"), IconName::CloudDownload);
        assert_eq!(
            command_icon("agent usage: toggle usage panel"),
            IconName::Sparkle
        );
    }

    #[test]
    fn a_broad_namespace_goes_by_the_verb() {
        assert_eq!(
            command_icon("workspace: new window"),
            IconName::OpenNewWindow
        );
        assert_eq!(command_icon("pane: close active item"), IconName::Close);
        assert_eq!(command_icon("editor: copy"), IconName::Copy);
        assert_eq!(command_icon("zode: open settings"), IconName::Settings);
        assert_eq!(
            command_icon("zode: open onboarding"),
            IconName::ArrowUpRight
        );
        assert_eq!(command_icon("zode: quit"), IconName::Power);
    }

    #[test]
    fn action_words_match_whole_words_only() {
        // "up" inside "setup" and "update" is not the direction.
        assert_eq!(command_icon("editor: setup thing"), IconName::Command);
        assert_eq!(command_icon("editor: line up"), IconName::ArrowUp);
        assert_eq!(
            command_icon("editor: sort lines by length"),
            IconName::ArrowDown10
        );
        assert_eq!(
            command_icon("workspace: clear all notifications"),
            IconName::ToolNotification
        );
        assert_eq!(
            command_icon("pane: swap item left"),
            IconName::ArrowRightLeft
        );
    }

    #[test]
    fn a_command_that_matches_nothing_gets_the_command_icon() {
        assert_eq!(command_icon("editor: handle input"), IconName::Command);
        assert_eq!(command_icon("unheard of: thing"), IconName::Command);
        assert_eq!(command_icon("no namespace"), IconName::Command);
    }
}
