# test-data

`schema.docs.graphql` is GitHub's public GraphQL schema (github.com, the "Free, Pro and Team" plans).

- Source: https://docs.github.com/public/fpt/schema.docs.graphql
- Downloaded: 2026-11-23
- SHA-256: d928ebbb2d08cba348c0c5713ab3c8756a39e838324089ff66fcddf1bab2c496

`graphql_schema_tests.rs` checks every document in `src/graphql_documents/`
against it: each selected field and argument must exist on its type. Replace
the file with a fresh download (and update the date and hash above) when a
document needs a field GitHub added later.
