//! Local branch names for a checked-out pull request. The head branch's name
//! is chosen by the pull request's author and the local name may be edited,
//! so both are checked before git sees them: `git switch -c` is run without
//! `--`, and a hidden character would make the confirmation read wrong.

use crate::untrusted_text::is_hidden_character;

/// Why a name was refused, worded for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchNameError(pub String);

/// `pr-<number>-<head>`, or `pr-<number>` when the head's name would not make
/// a valid branch name.
pub fn default_branch_name(number: u64, head_ref_name: &str) -> String {
    let name = format!("pr-{number}-{head_ref_name}");
    if validate_branch_name(&name).is_ok() {
        name
    } else {
        format!("pr-{number}")
    }
}

/// `git check-ref-format --branch`'s rules, plus no control, bidirectional or
/// invisible characters.
pub fn validate_branch_name(name: &str) -> Result<(), BranchNameError> {
    let refuse = |reason: &str| Err(BranchNameError(reason.to_string()));
    if name.is_empty() {
        return refuse("The branch name is empty.");
    }
    if name.starts_with('-') {
        return refuse("A branch name cannot start with “-”.");
    }
    if name == "HEAD" || name == "@" {
        return refuse("“HEAD” and “@” are not branch names.");
    }
    if let Some(character) = name
        .chars()
        .find(|character| character.is_control() || is_hidden_character(*character))
    {
        return refuse(&format!(
            "The name holds a hidden or control character (U+{:04X}).",
            u32::from(character)
        ));
    }
    if let Some(character) = name
        .chars()
        .find(|character| character.is_whitespace() || "~^:?*[\\".contains(*character))
    {
        return refuse(&format!("A branch name cannot contain “{character}”."));
    }
    if name.contains("..") || name.contains("@{") || name.contains("//") {
        return refuse("A branch name cannot contain “..”, “@{” or “//”.");
    }
    if name.starts_with('/') || name.ends_with('/') || name.ends_with('.') {
        return refuse("A branch name cannot start with “/” or end with “/” or “.”.");
    }
    let bad_component = name
        .split('/')
        .any(|component| component.starts_with('.') || component.ends_with(".lock"));
    if bad_component {
        return refuse("No part of a branch name can start with “.” or end with “.lock”.");
    }
    Ok(())
}
