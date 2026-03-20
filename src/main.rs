use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum DiagramArg {
    Class,
    Er,
    Sequence,
    Zenuml,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum LanguageArg {
    Auto,
    Rust,
    Go,
    TypeScript,
}

#[derive(Parser, Debug)]
#[command(name = "code-documenter")]
#[command(about = "Generate Mermaid diagrams from codebases")]
struct Cli {
    /// Path to the source file or directory to analyze
    path: PathBuf,

    /// Type of diagram to generate
    #[arg(short = 'd', long = "diagram", default_value = "class")]
    diagram: DiagramArg,

    /// Source language (auto-detect from file extensions by default)
    #[arg(short = 'l', long = "language", default_value = "auto")]
    language: LanguageArg,

    /// Entry function for sequence diagrams
    #[arg(short = 'e', long = "entry")]
    entry: Option<String>,

    /// Output file (defaults to stdout)
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    /// Mermaid theme (default, neutral, dark, forest, base)
    #[arg(short = 't', long = "theme", default_value = "neutral")]
    theme: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let diagram_type = match cli.diagram {
        DiagramArg::Class => code_documenter::emit::DiagramType::Class,
        DiagramArg::Er => code_documenter::emit::DiagramType::Er,
        DiagramArg::Sequence => code_documenter::emit::DiagramType::Sequence,
        DiagramArg::Zenuml => code_documenter::emit::DiagramType::Zenuml,
    };

    let language = match cli.language {
        LanguageArg::Auto => None,
        LanguageArg::Rust => Some(code_documenter::parse::Language::Rust),
        LanguageArg::Go => Some(code_documenter::parse::Language::Go),
        LanguageArg::TypeScript => Some(code_documenter::parse::Language::TypeScript),
    };

    let theme = if cli.theme == "default" {
        code_documenter::emit::MermaidTheme::Default
    } else {
        code_documenter::emit::MermaidTheme::Named(cli.theme)
    };

    let result =
        code_documenter::run(&cli.path, diagram_type, language, cli.entry.as_deref(), theme)?;

    if let Some(output_path) = cli.output {
        std::fs::write(&output_path, &result)?;
        eprintln!("Output written to {}", output_path.display());
    } else {
        print!("{result}");
    }

    Ok(())
}
