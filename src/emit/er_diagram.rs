use crate::model::{Cardinality, CodeModel, Entity, Field, Relationship};

use super::{DiagramEmitter, MermaidTheme};

pub struct ErDiagramEmitter;

impl ErDiagramEmitter {
    fn emit_entity(output: &mut String, entity: &Entity) {
        output.push_str(&format!("    {} {{\n", entity.name));

        let is_enum = matches!(entity.kind, crate::model::EntityKind::Enum);
        for field in &entity.fields {
            Self::emit_field(output, field, is_enum);
        }

        output.push_str("    }\n");
    }

    fn emit_field(output: &mut String, field: &Field, is_enum: bool) {
        let type_name = field.type_info.display_name();
        let field_name = &field.name;

        if is_enum {
            // Enum variants: just show the name as the attribute type
            output.push_str(&format!("        string {}\n", field_name));
        } else if field_name == "id" {
            output.push_str(&format!("        {} {} PK\n", type_name, field_name));
        } else if field_name.ends_with("_id") {
            output.push_str(&format!("        {} {} FK\n", type_name, field_name));
        } else {
            output.push_str(&format!("        {} {}\n", type_name, field_name));
        }
    }

    fn cardinality_notation(cardinality: &Cardinality) -> &'static str {
        match cardinality {
            Cardinality::One => "||--||",
            Cardinality::ZeroOrOne => "||--o|",
            Cardinality::OneOrMore => "||--|{",
            Cardinality::ZeroOrMore => "||--o{",
        }
    }

    fn emit_relationship(output: &mut String, rel: &Relationship) {
        match rel {
            Relationship::Composition {
                owner,
                owned,
                field_name,
                cardinality,
            } => {
                let notation = Self::cardinality_notation(cardinality);
                output.push_str(&format!(
                    "    {} {} {} : \"{}\"\n",
                    owner, notation, owned, field_name
                ));
            }
            Relationship::Aggregation {
                from,
                to,
                field_name,
                cardinality,
            } => {
                // Non-identifying uses dashed line (..)
                let notation = match cardinality {
                    Cardinality::One => "||..||",
                    Cardinality::ZeroOrOne => "||..o|",
                    Cardinality::OneOrMore => "||..|{",
                    Cardinality::ZeroOrMore => "||..o{",
                };
                output.push_str(&format!(
                    "    {} {} {} : \"{}\"\n",
                    from, notation, to, field_name
                ));
            }
            Relationship::Inheritance { child, parent } => {
                output.push_str(&format!(
                    "    {} ||--|| {} : \"inherits\"\n",
                    child, parent
                ));
            }
            Relationship::Implementation {
                implementor,
                interface,
            } => {
                output.push_str(&format!(
                    "    {} ||--|| {} : \"implements\"\n",
                    implementor, interface
                ));
            }
            Relationship::Association { from, to, label } => {
                output.push_str(&format!("    {} ||--|| {} : \"{}\"\n", from, to, label));
            }
        }
    }
}

impl DiagramEmitter for ErDiagramEmitter {
    fn emit(&self, model: &CodeModel, theme: &MermaidTheme) -> String {
        let mut output = theme.directive();
        output.push_str("erDiagram\n");

        for entity in &model.entities {
            // Skip entities with no fields — they produce empty blocks in ER diagrams
            if entity.fields.is_empty() {
                continue;
            }
            Self::emit_entity(&mut output, entity);
        }

        for rel in &model.relationships {
            Self::emit_relationship(&mut output, rel);
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cardinality, EntityKind, Field, TypeInfo, Visibility};

    #[test]
    fn test_emit_empty_model() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel::new();
        assert_eq!(emitter.emit(&model, &MermaidTheme::Default), "erDiagram\n");
    }

    #[test]
    fn test_emit_entity_with_fields_pk_fk() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "User".to_string(),
                kind: EntityKind::Struct,
                fields: vec![
                    Field {
                        name: "id".to_string(),
                        type_info: TypeInfo::Simple("i64".to_string()),
                        visibility: Visibility::Public,
                    },
                    Field {
                        name: "name".to_string(),
                        type_info: TypeInfo::Simple("String".to_string()),
                        visibility: Visibility::Public,
                    },
                    Field {
                        name: "team_id".to_string(),
                        type_info: TypeInfo::Simple("i64".to_string()),
                        visibility: Visibility::Public,
                    },
                ],
                methods: vec![],
                source_file: "user.rs".to_string(),
            }],
            functions: vec![],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        let expected = "\
erDiagram
    User {
        i64 id PK
        String name
        i64 team_id FK
    }
";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_emit_composition_relationship() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![Relationship::Composition {
                owner: "Car".to_string(),
                owned: "Engine".to_string(),
                field_name: "engine".to_string(),
                cardinality: Cardinality::One,
            }],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("Car ||--|| Engine : \"engine\""));
    }

    #[test]
    fn test_emit_aggregation_relationship() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![Relationship::Aggregation {
                from: "Library".to_string(),
                to: "Book".to_string(),
                field_name: "books".to_string(),
                cardinality: Cardinality::ZeroOrMore,
            }],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("Library ||..o{ Book : \"books\""));
    }

    #[test]
    fn test_emit_inheritance_relationship() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![Relationship::Inheritance {
                child: "Dog".to_string(),
                parent: "Animal".to_string(),
            }],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("Dog ||--|| Animal : \"inherits\""));
    }

    #[test]
    fn test_emit_implementation_relationship() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![Relationship::Implementation {
                implementor: "Dog".to_string(),
                interface: "Pet".to_string(),
            }],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("Dog ||--|| Pet : \"implements\""));
    }

    #[test]
    fn test_emit_association_relationship() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![Relationship::Association {
                from: "Student".to_string(),
                to: "Course".to_string(),
                label: "enrolls".to_string(),
            }],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("Student ||--|| Course : \"enrolls\""));
    }

    #[test]
    fn test_emit_all_cardinalities() {
        let emitter = ErDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![
                Relationship::Composition {
                    owner: "A".to_string(),
                    owned: "B".to_string(),
                    field_name: "one".to_string(),
                    cardinality: Cardinality::One,
                },
                Relationship::Composition {
                    owner: "A".to_string(),
                    owned: "C".to_string(),
                    field_name: "zero_or_one".to_string(),
                    cardinality: Cardinality::ZeroOrOne,
                },
                Relationship::Composition {
                    owner: "A".to_string(),
                    owned: "D".to_string(),
                    field_name: "one_or_more".to_string(),
                    cardinality: Cardinality::OneOrMore,
                },
                Relationship::Composition {
                    owner: "A".to_string(),
                    owned: "E".to_string(),
                    field_name: "zero_or_more".to_string(),
                    cardinality: Cardinality::ZeroOrMore,
                },
            ],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("A ||--|| B : \"one\""));
        assert!(result.contains("A ||--o| C : \"zero_or_one\""));
        assert!(result.contains("A ||--|{ D : \"one_or_more\""));
        assert!(result.contains("A ||--o{ E : \"zero_or_more\""));
    }
}
