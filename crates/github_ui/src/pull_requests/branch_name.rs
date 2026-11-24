//! Local branch names for a checked-out pull request. The head branch's name
//! is chosen by the pull request's author and the local name may be edited,
//! so both are checked before git sees them: `git switch -c` is run without
//! `--`, and a hidden character would make the confirmation read wrong.

use git::repository::ref_name_problem;

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

/// `git check-ref-format --branch`'s rules (shared with the checks on a
/// fetch's remote name, in the git crate), plus no bidirectional or invisible
/// characters.
pub fn validate_branch_name(name: &str) -> Result<(), BranchNameError> {
    if let Some(character) = name
        .chars()
        .find(|character| character.is_control() || is_hidden_character(*character))
    {
        return Err(BranchNameError(format!(
            "The name holds a hidden or control character (U+{:04X}).",
            u32::from(character)
        )));
    }
    match ref_name_problem(name) {
        Some(problem) => Err(BranchNameError(problem.to_string())),
        None => Ok(()),
    }
}
