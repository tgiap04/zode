use std::path::Path;
use std::sync::OnceLock;

use crate::graphql::documents;
use crate::graphql_schema_check::{Schema, check_document};

fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/schema.docs.graphql");
        let source = std::fs::read_to_string(&path).expect("the schema is checked in");
        Schema::parse(&source).expect("GitHub's schema parses")
    })
}

#[test]
fn every_document_in_the_folder_fits_githubs_schema() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/graphql_documents");
    let mut checked = 0;
    for entry in std::fs::read_dir(&folder).expect("the documents folder exists") {
        let path = entry.expect("a readable entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("graphql") {
            continue;
        }
        let document = std::fs::read_to_string(&path).expect("a readable document");
        let problems = check_document(schema(), &document);
        assert!(problems.is_empty(), "{}: {problems:#?}", path.display());
        checked += 1;
    }
    assert_eq!(
        checked,
        compiled_documents().len(),
        "every document should be checked"
    );
}

fn compiled_documents() -> Vec<&'static str> {
    vec![
        documents::REPOSITORY_ISSUE_TYPES,
        documents::UPDATE_ISSUE_ISSUE_TYPE,
        documents::ISSUE_PARENT,
        documents::REPRIORITIZE_SUB_ISSUE,
        documents::REPOSITORY_PROJECTS,
        documents::PROJECT_DETAILS,
        documents::PROJECT_ITEMS,
        documents::UPDATE_PROJECT_ITEM_FIELD,
        documents::CLEAR_PROJECT_ITEM_FIELD,
        documents::UPDATE_PROJECT_ITEM_POSITION,
        documents::ADD_PROJECT_ITEM,
        documents::DELETE_PROJECT_ITEM,
        documents::PROJECT_OWNERS,
        documents::CREATE_PROJECT,
        documents::REPOSITORY_PULL_REQUESTS,
        documents::REPOSITORY_PULL_REQUEST,
        documents::PULL_REQUEST_DETAILS,
        documents::MERGE_PULL_REQUEST,
        documents::DELETE_BRANCH_AT,
        documents::REPOSITORY_ASSIGNABLE_USERS,
    ]
}

#[test]
fn the_compiled_in_documents_are_the_checked_files() {
    for document in compiled_documents() {
        assert_eq!(check_document(schema(), document), Vec::<String>::new());
    }
}

#[test]
fn a_misspelled_field_is_caught() {
    let problems = check_document(
        schema(),
        "query { repository(owner: \"o\", name: \"r\") { issueTypez(first: 1) { nodes { id } } } }",
    );
    assert_eq!(
        problems,
        vec!["field `Repository.issueTypez` does not exist".to_string()]
    );
}

#[test]
fn an_unknown_argument_type_condition_and_variable_type_are_caught() {
    let problems = check_document(
        schema(),
        "query Q($id: NoSuchType!) {
            node(id: $id, colour: 1) { ... on NotAType { id } }
        }",
    );
    assert_eq!(
        problems,
        vec![
            "variable `$id` has unknown type `NoSuchType`".to_string(),
            "argument `colour` of `Query.node` does not exist".to_string(),
            "type `NotAType` does not exist".to_string(),
        ]
    );
}

#[test]
fn a_field_inside_a_named_fragment_is_checked_on_its_type() {
    let problems = check_document(
        schema(),
        "query { viewer { ...Me } }
         fragment Me on User { login loginName }",
    );
    assert_eq!(
        problems,
        vec!["field `User.loginName` does not exist".to_string()]
    );
}

#[test]
fn leaves_and_objects_need_the_right_shape() {
    let problems = check_document(schema(), "query { viewer { login { x } } rateLimit }");
    assert_eq!(
        problems,
        vec![
            "`User.login` is a leaf and takes no selection".to_string(),
            "`Query.rateLimit` needs a selection of fields".to_string(),
        ]
    );
}

#[test]
fn a_spread_of_an_undefined_fragment_is_caught() {
    let problems = check_document(schema(), "query { viewer { ...Missing } }");
    assert_eq!(
        problems,
        vec!["fragment `Missing` is not defined".to_string()]
    );
}
