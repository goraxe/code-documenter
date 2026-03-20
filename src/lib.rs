pub mod emit;
pub mod model;
pub mod parse;

use std::path::Path;

use anyhow::{Context, Result};
use walkdir::WalkDir;

use emit::DiagramType;
use model::CodeModel;
use parse::Language;

/// Run the code-documenter pipeline: walk files, parse, merge, and emit a diagram.
///
/// - `path`: a file or directory to analyze
/// - `diagram_type`: which diagram format to emit
/// - `language`: if `Some`, force this language for all files; if `None`, auto-detect
/// - `_entry`: optional entry function name for ZenUML diagrams (reserved for Phase 2)
pub fn run(
    path: &Path,
    diagram_type: DiagramType,
    language: Option<Language>,
    _entry: Option<&str>,
) -> Result<String> {
    let mut merged = CodeModel::new();

    if path.is_file() {
        let model = parse_single_file(path, language)?;
        merged.merge(model);
    } else if path.is_dir() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let file_path = entry.path();
            let lang = match language {
                Some(l) => Some(l),
                None => parse::detect_language(file_path),
            };
            if let Some(lang) = lang {
                match parse_single_file(file_path, Some(lang)) {
                    Ok(model) => merged.merge(model),
                    Err(e) => {
                        eprintln!("Warning: failed to parse {}: {e}", file_path.display());
                    }
                }
            }
        }
    } else {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    let emitter = emit::get_emitter(diagram_type);
    Ok(emitter.emit(&merged))
}

fn parse_single_file(path: &Path, language: Option<Language>) -> Result<CodeModel> {
    let lang = language
        .or_else(|| parse::detect_language(path))
        .context(format!("Could not detect language for {}", path.display()))?;

    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let parser = parse::get_parser(lang);
    parser
        .parse_file(path, &source)
        .with_context(|| format!("Failed to parse {}", path.display()))
}
