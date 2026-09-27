fn main() {
    println!("cargo:rerun-if-env-changed=HELIX_LIBGIT2_DIR");
    if std::env::var_os("CARGO_FEATURE_LIBGIT2").is_none() {
        return;
    }
    // On iOS and visionOS the host app links libgit2, so nothing is linked here.
    // Elsewhere (host tests), point HELIX_LIBGIT2_DIR at a static libgit2.a built from
    // the same revision as the one ffi.rs was transcribed from.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target_os.as_str(), "ios" | "visionos" | "tvos" | "watchos") {
        return;
    }
    let Some(dir) = std::env::var_os("HELIX_LIBGIT2_DIR") else {
        return;
    };
    println!(
        "cargo:rustc-link-search=native={}",
        std::path::Path::new(&dir).display()
    );
    println!("cargo:rustc-link-lib=static=git2");
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=iconv");
    }
}
