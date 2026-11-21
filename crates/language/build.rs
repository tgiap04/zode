fn main() {
    if let Ok(bundled) = std::env::var("ZODE_BUNDLE") {
        println!("cargo:rustc-env=ZODE_BUNDLE={}", bundled);
    }
}
