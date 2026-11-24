//! What a Projects (v2) board or table is drawn from: the project's fields,
//! its saved views, and the values an item holds in those fields.

/// A project's fields and views, read in one query. Items come separately,
/// a page at a time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectDetails {
    pub fields: Vec<ProjectField>,
    pub views: Vec<ProjectView>,
}

impl ProjectDetails {
    pub fn field(&self, field_id: &str) -> Option<&ProjectField> {
        self.fields.iter().find(|field| field.id == field_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectField {
    /// GraphQL node id.
    pub id: String,
    pub name: String,
    pub kind: ProjectFieldKind,
}

/// The built-in fields (title, assignees, labels, repository, milestone)
/// take their values from the issue or pull request itself; the rest from
/// the item's field values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectFieldKind {
    Title,
    Assignees,
    Labels,
    Repository,
    Milestone,
    Text,
    Number,
    Date,
    SingleSelect(Vec<ProjectFieldOption>),
    Iteration(Vec<ProjectIteration>),
    /// Pull requests linked to the issue, read from the item's values.
    LinkedPullRequests,
    /// Read from the issue itself, not from the item's values.
    SubIssuesProgress,
    /// Reviewers, parent issue and the like: listed, but not drawn.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFieldOption {
    pub id: String,
    pub name: String,
    /// GitHub's name for it (`GREEN`, `RED`, …), not a hex value.
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectIteration {
    pub id: String,
    pub title: String,
    /// `YYYY-MM-DD`.
    pub start_date: String,
    pub completed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectViewLayout {
    Board,
    Table,
    Roadmap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSort {
    pub field_id: String,
    pub descending: bool,
}

/// A saved view. The field lists hold field ids; `filter` is GitHub's own
/// filter text, which the API does not apply to the items it returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectView {
    pub id: String,
    pub number: u64,
    pub name: String,
    pub layout: ProjectViewLayout,
    pub filter: Option<String>,
    pub group_by_fields: Vec<String>,
    pub vertical_group_by_fields: Vec<String>,
    pub sort_by_fields: Vec<ProjectSort>,
    pub visible_fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectFieldValue {
    pub field_id: String,
    pub value: ProjectValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectValue {
    Text(String),
    Number(f64),
    /// `YYYY-MM-DD`.
    Date(String),
    SingleSelect {
        option_id: String,
        name: String,
    },
    Iteration {
        iteration_id: String,
        title: String,
        start_date: String,
    },
    Milestone(String),
    /// The numbers of the first linked pull requests, and how many there
    /// are in all.
    PullRequests {
        numbers: Vec<u64>,
        total: u64,
    },
}

/// A value written to an item's field: typed into a text, number or date
/// field, or the option or iteration picked (or the column a card is moved
/// to on a board).
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectFieldInput {
    Text {
        text: String,
    },
    Number {
        number: f64,
    },
    /// `YYYY-MM-DD`.
    Date {
        date: String,
    },
    SingleSelect {
        option_id: String,
    },
    Iteration {
        iteration_id: String,
    },
}

/// An account a new project can belong to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectOwner {
    /// GraphQL node id.
    pub id: String,
    pub login: String,
    pub avatar_url: Option<String>,
}

/// Who a project linked to a repository may be created under: the
/// signed-in account always, and the repository's owner when that is an
/// organization (a personal repository's owner is someone the viewer cannot
/// create projects for, or the viewer already).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectOwners {
    /// GraphQL node id of the repository the project is linked to.
    pub repository_id: String,
    pub viewer: ProjectOwner,
    pub organization: Option<ProjectOwner>,
}
