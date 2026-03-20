pub mod class_diagram;
pub mod er_diagram;
pub mod sequence;
pub mod zenuml;

use crate::model::CodeModel;

/// Trait for diagram emitters.
pub trait DiagramEmitter {
    fn emit(&self, model: &CodeModel, theme: &MermaidTheme) -> String;
}

/// Supported diagram types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramType {
    Class,
    Er,
    Sequence,
    Zenuml,
}

/// Mermaid theme configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidTheme {
    /// No theme directive — use Mermaid's default.
    Default,
    /// A named Mermaid theme (e.g. "neutral", "dark", "forest").
    Named(String),
}

impl MermaidTheme {
    /// Returns the `%%{init: ...}%%` directive to prepend, or empty string for default.
    pub fn directive(&self) -> String {
        match self {
            MermaidTheme::Default => String::new(),
            MermaidTheme::Named(name) => format!("%%{{init: {{'theme':'{}'}}}}%%\n", name),
        }
    }
}

/// Return the appropriate emitter for the given diagram type.
pub fn get_emitter(diagram_type: DiagramType) -> Box<dyn DiagramEmitter> {
    match diagram_type {
        DiagramType::Class => Box::new(class_diagram::ClassDiagramEmitter),
        DiagramType::Er => Box::new(er_diagram::ErDiagramEmitter),
        DiagramType::Sequence => Box::new(sequence::SequenceEmitter),
        DiagramType::Zenuml => Box::new(zenuml::ZenumlEmitter),
    }
}
