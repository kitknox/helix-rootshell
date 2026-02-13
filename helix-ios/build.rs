fn main() {
    let helix_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let sources_dir = helix_dir.join("runtime/grammars/sources");

    // Each entry: (grammar_id, optional_subpath)
    // Grammar sources must be fetched first via `hx --grammar fetch`
    let grammars: &[(&str, Option<&str>)] = &[
        ("bash", None),
        ("c", None),
        ("comment", None),
        ("cpp", None),
        ("css", None),
        ("diff", None),
        ("dockerfile", None),
        ("gitcommit", None),
        ("git-rebase", None),
        ("go", None),
        ("html", None),
        ("java", None),
        ("javascript", None),
        ("json", None),
        ("kotlin", None),
        ("lua", None),
        ("markdown", Some("tree-sitter-markdown")),
        ("markdown_inline", Some("tree-sitter-markdown-inline")),
        ("python", None),
        ("ruby", None),
        ("rust", None),
        ("sql", None),
        ("swift", None),
        ("toml", None),
        ("tsx", Some("tsx")),
        ("typescript", Some("typescript")),
        ("xml", None),
        ("yaml", None),
        ("zig", None),
    ];

    for &(name, subpath) in grammars {
        let src_dir = match subpath {
            Some(sub) => sources_dir.join(name).join(sub).join("src"),
            None => sources_dir.join(name).join("src"),
        };

        let parser_c = src_dir.join("parser.c");
        if !parser_c.exists() {
            println!(
                "cargo:warning=Grammar source not found: {}",
                parser_c.display()
            );
            continue;
        }

        let lib_name = format!("tree_sitter_{}", name.replace('-', "_"));
        let mut build = cc::Build::new();
        build
            .file(&parser_c)
            .include(&src_dir)
            .std("c11")
            .warnings(false);

        // Check for scanner.c or scanner.cc
        let scanner_c = src_dir.join("scanner.c");
        let scanner_cc = src_dir.join("scanner.cc");
        if scanner_c.exists() {
            build.file(&scanner_c);
        } else if scanner_cc.exists() {
            // C++ scanner needs separate compilation
            let mut cpp_build = cc::Build::new();
            cpp_build
                .file(&scanner_cc)
                .include(&src_dir)
                .cpp(true)
                .std("c++14")
                .warnings(false)
                .compile(&format!("{lib_name}_scanner"));
        }

        build.compile(&lib_name);

        println!("cargo:rerun-if-changed={}", parser_c.display());
        if scanner_c.exists() {
            println!("cargo:rerun-if-changed={}", scanner_c.display());
        }
        if scanner_cc.exists() {
            println!("cargo:rerun-if-changed={}", scanner_cc.display());
        }
    }
}
