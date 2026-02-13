use helix_loader::grammar::GrammarLoader;
use std::ptr::NonNull;
use tree_house::tree_sitter::{Grammar, GrammarData};

/// Declare extern C tree-sitter grammar functions and implement GrammarLoader.
macro_rules! declare_grammars {
    ($($name:expr => $fn_name:ident),* $(,)?) => {
        extern "C" {
            $(fn $fn_name() -> NonNull<GrammarData>;)*
        }

        pub struct StaticGrammarLoader;

        impl GrammarLoader for StaticGrammarLoader {
            fn get_grammar(&self, name: &str) -> Option<Grammar> {
                let grammar = match name {
                    $($name => unsafe { Grammar::from_language_fn($fn_name) },)*
                    _ => return None,
                };
                match grammar {
                    Ok(g) => Some(g),
                    Err(e) => {
                        log::warn!("Static grammar '{}' failed ABI check: {}", name, e);
                        None
                    }
                }
            }
        }
    };
}

declare_grammars! {
    "bash" => tree_sitter_bash,
    "c" => tree_sitter_c,
    "comment" => tree_sitter_comment,
    "cpp" => tree_sitter_cpp,
    "css" => tree_sitter_css,
    "diff" => tree_sitter_diff,
    "dockerfile" => tree_sitter_dockerfile,
    "gitcommit" => tree_sitter_gitcommit,
    "git-rebase" => tree_sitter_git_rebase,
    "go" => tree_sitter_go,
    "html" => tree_sitter_html,
    "java" => tree_sitter_java,
    "javascript" => tree_sitter_javascript,
    "json" => tree_sitter_json,
    "kotlin" => tree_sitter_kotlin,
    "lua" => tree_sitter_lua,
    "markdown" => tree_sitter_markdown,
    "markdown_inline" => tree_sitter_markdown_inline,
    "python" => tree_sitter_python,
    "ruby" => tree_sitter_ruby,
    "rust" => tree_sitter_rust,
    "sql" => tree_sitter_sql,
    "swift" => tree_sitter_swift,
    "toml" => tree_sitter_toml,
    "tsx" => tree_sitter_tsx,
    "typescript" => tree_sitter_typescript,
    "xml" => tree_sitter_xml,
    "yaml" => tree_sitter_yaml,
    "zig" => tree_sitter_zig,
}
