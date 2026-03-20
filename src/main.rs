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
    #[arg(short = 't', long = "theme", default_value = "dark")]
    theme: String,

    /// Output format: mmd (text), svg, png (svg/png require the "render" feature)
    #[arg(short = 'f', long = "format", default_value = "mmd")]
    format: String,
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

    let mermaid_text =
        code_documenter::run(&cli.path, diagram_type, language, cli.entry.as_deref(), theme)?;

    match cli.format.as_str() {
        "mmd" => {
            if let Some(output_path) = cli.output {
                std::fs::write(&output_path, &mermaid_text)?;
                eprintln!("Output written to {}", output_path.display());
            } else {
                print!("{mermaid_text}");
            }
        }
        #[cfg(feature = "render")]
        "svg" => {
            let svg = mermaid_rs_renderer::render(&mermaid_text)
                .map_err(|e| anyhow::anyhow!("SVG render failed: {e}"))?;
            if let Some(output_path) = cli.output {
                std::fs::write(&output_path, &svg)?;
                eprintln!("Output written to {}", output_path.display());
            } else {
                print!("{svg}");
            }
        }
        #[cfg(feature = "render")]
        "png" => {
            let output_path = cli
                .output
                .ok_or_else(|| anyhow::anyhow!("PNG format requires --output <file>"))?;
            let svg = mermaid_rs_renderer::render(&mermaid_text)
                .map_err(|e| anyhow::anyhow!("SVG render failed: {e}"))?;
            let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())?;
            let size = tree.size().to_int_size();
            let mut pixmap =
                tiny_skia::Pixmap::new(size.width(), size.height())
                    .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;
            resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
            pixmap.save_png(&output_path)?;
            eprintln!("Output written to {}", output_path.display());
        }
        #[cfg(not(feature = "render"))]
        "svg" | "png" => {
            anyhow::bail!(
                "SVG/PNG output requires the 'render' feature.\n\
                 Rebuild with: cargo install --features render"
            );
        }
        other => {
            anyhow::bail!("Unknown format '{other}'. Use: mmd, svg, png");
        }
    }

    Ok(())
}
