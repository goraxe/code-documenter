pub mod go;
pub mod rust;
pub mod typescript;

use std::path::Path;

use anyhow::Result;

use crate::model::CodeModel;

/// Trait for language-specific parsers.
pub trait LanguageParser {
    fn parse_file(&self, path: &Path, source: &str) -> Result<CodeModel>;
}

/// Supported source languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Go,
    TypeScript,
}

/// Detect the source language from a file extension.
pub fn detect_language(path: &Path) -> Option<Language> {
    match path.extension()?.to_str()? {
        "rs" => Some(Language::Rust),
        "go" => Some(Language::Go),
        "ts" | "tsx" => Some(Language::TypeScript),
        _ => None,
    }
}

/// Return the appropriate parser for the given language.
pub fn get_parser(lang: Language) -> Box<dyn LanguageParser> {
    match lang {
        Language::Rust => Box::new(rust::RustParser),
        Language::Go => Box::new(go::GoParser),
        Language::TypeScript => Box::new(typescript::TypeScriptParser),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_rust() {
        assert_eq!(detect_language(Path::new("foo.rs")), Some(Language::Rust));
    }

    #[test]
    fn test_detect_language_go() {
        assert_eq!(detect_language(Path::new("main.go")), Some(Language::Go));
    }

    #[test]
    fn test_detect_language_ts() {
        assert_eq!(
            detect_language(Path::new("index.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            detect_language(Path::new("component.tsx")),
            Some(Language::TypeScript)
        );
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(detect_language(Path::new("readme.md")), None);
    }
}
