use github::{Issue, IssueFilter, IssueState, IssueStateFilter, Milestone};
use gpui::Hsla;

/// "Mine", as GitHub's own issue page offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MineFilter {
    Assigned,
    Created,
    Mentioned,
}

impl MineFilter {
    pub const ALL: [Self; 3] = [Self::Assigned, Self::Created, Self::Mentioned];

    pub fn label(self) -> &'static str {
        match self {
            Self::Assigned => "Assigned to me",
            Self::Created => "Created by me",
            Self::Mentioned => "Mentioning me",
        }
    }
}

/// What the list is narrowed to. Sent to GitHub with each load, and checked
/// again on what comes back, since a search cannot take a milestone number.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueListFilters {
    pub state: IssueStateFilter,
    pub mine: Option<MineFilter>,
    pub label: Option<String>,
    pub milestone: Option<Milestone>,
    /// An issue type's name.
    pub issue_type: Option<String>,
}

impl IssueListFilters {
    pub fn to_issue_filter(&self, login: &str) -> IssueFilter {
        let mine = |wanted: MineFilter| (self.mine == Some(wanted)).then(|| login.to_string());
        IssueFilter {
            state: self.state,
            labels: self.label.iter().cloned().collect(),
            assignee: mine(MineFilter::Assigned),
            creator: mine(MineFilter::Created),
            mentioned: mine(MineFilter::Mentioned),
            milestone: self.milestone.as_ref().map(|milestone| milestone.number),
            issue_type: self.issue_type.clone(),
        }
    }

    /// Whether a loaded issue passes. A mention cannot be seen on the row, so
    /// "Mentioning me" is left to GitHub.
    pub fn admits(&self, issue: &Issue, login: &str) -> bool {
        let state = match self.state {
            IssueStateFilter::Open => issue.state == IssueState::Open,
            IssueStateFilter::Closed => issue.state == IssueState::Closed,
            IssueStateFilter::All => true,
        };
        let mine = match self.mine {
            Some(MineFilter::Assigned) => issue.assignees.iter().any(|assignee| assignee == login),
            Some(MineFilter::Created) => issue.author.as_deref() == Some(login),
            Some(MineFilter::Mentioned) | None => true,
        };
        let label = self
            .label
            .as_ref()
            .is_none_or(|wanted| issue.labels.iter().any(|label| &label.name == wanted));
        let milestone = self.milestone.as_ref().is_none_or(|wanted| {
            issue
                .milestone
                .as_ref()
                .is_some_and(|milestone| milestone.number == wanted.number)
        });
        let issue_type = self
            .issue_type
            .as_ref()
            .is_none_or(|wanted| issue.issue_type.as_ref() == Some(wanted));
        state && mine && label && milestone && issue_type
    }
}

/// Whether every word of `query` is somewhere in the issue's number, title,
/// labels, author or assignees, ignoring case. Only what is loaded is looked
/// at; asking GitHub is what Enter does.
pub fn matches_query(issue: &Issue, query: &str) -> bool {
    let words = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if words.is_empty() {
        return true;
    }
    let mut haystack = format!("#{}\n{}", issue.number, issue.title);
    for label in &issue.labels {
        haystack.push('\n');
        haystack.push_str(&label.name);
    }
    for login in issue.author.iter().chain(&issue.assignees) {
        haystack.push('\n');
        haystack.push_str(login);
    }
    let haystack = haystack.to_lowercase();
    words
        .iter()
        .all(|word| haystack.contains(word.trim_start_matches('#')))
}

/// A label's colour as GitHub sends it, six hex digits without `#`.
pub fn label_color(hex: &str) -> Option<Hsla> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(gpui::rgb(value).into())
}
