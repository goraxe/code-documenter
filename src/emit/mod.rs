pub mod class_diagram;
pub mod er_diagram;
pub mod zenuml;

use crate::model::CodeModel;

/// Trait for diagram emitters.
pub trait DiagramEmitter {
    fn emit(&self, model: &CodeModel) -> String;
}

/// Supported diagram types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramType {
    Class,
    Er,
    Zenuml,
}

/// Return the appropriate emitter for the given diagram type.
pub fn get_emitter(diagram_type: DiagramType) -> Box<dyn DiagramEmitter> {
    match diagram_type {
        DiagramType::Class => Box::new(class_diagram::ClassDiagramEmitter),
        DiagramType::Er => Box::new(er_diagram::ErDiagramEmitter),
        DiagramType::Zenuml => Box::new(zenuml::ZenumlEmitter),
    }
}
