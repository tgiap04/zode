use std::path::PathBuf;

/// A recorded fixture with the `# ` lines that describe how it was captured
/// removed, and every other byte (CRLF included) kept.
pub fn tool_output(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/fixtures")
        .join(name);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} is readable: {error}", path.display()));
    contents
        .split_inclusive('\n')
        .filter(|line| !line.starts_with("# "))
        .collect()
}
