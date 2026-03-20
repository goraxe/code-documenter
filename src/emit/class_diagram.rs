use std::collections::BTreeMap;

use crate::model::{CodeModel, Entity, EntityKind, Field, Method, Relationship};

use super::{DiagramEmitter, MermaidTheme};

pub struct ClassDiagramEmitter;

impl ClassDiagramEmitter {
    /// Derive a namespace label from a source file path.
    /// e.g. `./src/parse/rust.rs` → `parse::rust`, `src/model.rs` → `model`
    fn namespace_from_path(path: &str) -> String {
        let p = path
            .strip_prefix("./")
            .unwrap_or(path);
        // Strip the `src/` prefix if present
        let p = p.strip_prefix("src/").unwrap_or(p);
        // Remove extension
        let p = p.strip_suffix(".rs")
            .or_else(|| p.strip_suffix(".go"))
            .or_else(|| p.strip_suffix(".tsx"))
            .or_else(|| p.strip_suffix(".ts"))
            .unwrap_or(p);
        // Convert path separators to :: and strip mod.rs / mod suffixes
        let p = p.replace('/', "::");
        let p = p.strip_suffix("::mod").unwrap_or(&p);
        p.to_string()
    }

    fn emit_entity(output: &mut String, entity: &Entity, indent: &str) {
        output.push_str(&format!("{}class {} {{\n", indent, entity.name));

        let inner = format!("{}    ", indent);

        // Emit annotation based on EntityKind
        if let Some(annotation) = Self::annotation_for_kind(&entity.kind) {
            output.push_str(&format!("{}{}\n", inner, annotation));
        }

        // Emit fields
        let is_enum = matches!(entity.kind, EntityKind::Enum);
        for field in &entity.fields {
            Self::emit_field(output, field, is_enum, &inner);
        }

        // Emit methods
        for method in &entity.methods {
            Self::emit_method(output, method, &inner);
        }

        output.push_str(&format!("{}}}\n", indent));
    }

    fn annotation_for_kind(kind: &EntityKind) -> Option<&'static str> {
        match kind {
            EntityKind::Struct => Some("<<Struct>>"),
            EntityKind::Enum => Some("<<Enumeration>>"),
            EntityKind::Interface => Some("<<Interface>>"),
            EntityKind::Class => None,
            EntityKind::Trait => Some("<<Interface>>"),
            EntityKind::TypeAlias => Some("<<Type>>"),
        }
    }

    fn emit_field(output: &mut String, field: &Field, is_enum: bool, indent: &str) {
        if is_enum {
            output.push_str(&format!("{}{}\n", indent, field.name));
        } else {
            output.push_str(&format!(
                "{}{}{} {}\n",
                indent,
                field.visibility.mermaid_prefix(),
                field.type_info.display_name(),
                field.name,
            ));
        }
    }

    fn emit_method(output: &mut String, method: &Method, indent: &str) {
        let params: Vec<String> = method
            .parameters
            .iter()
            .map(|p| format!("{} {}", p.name, p.type_info.display_name()))
            .collect();
        let params_str = params.join(", ");

        let return_type_str = match &method.return_type {
            Some(t) => format!(" {}", t.display_name()),
            None => String::new(),
        };

        let suffix = if method.is_abstract {
            "*"
        } else if method.is_static {
            "$"
        } else {
            ""
        };

        output.push_str(&format!(
            "{}{}{}({}){}{}\n",
            indent,
            method.visibility.mermaid_prefix(),
            method.name,
            params_str,
            return_type_str,
            suffix,
        ));
    }

    fn emit_relationship(output: &mut String, rel: &Relationship) {
        match rel {
            Relationship::Inheritance { child, parent } => {
                output.push_str(&format!("    {} <|-- {}\n", child, parent));
            }
            Relationship::Implementation {
                implementor,
                interface,
            } => {
                output.push_str(&format!("    {} ..|> {}\n", implementor, interface));
            }
            Relationship::Composition {
                owner,
                owned,
                field_name,
                ..
            } => {
                output.push_str(&format!("    {} *-- {} : {}\n", owner, owned, field_name));
            }
            Relationship::Aggregation {
                from,
                to,
                field_name,
                ..
            } => {
                output.push_str(&format!("    {} o-- {} : {}\n", from, to, field_name));
            }
            Relationship::Association { from, to, label } => {
                output.push_str(&format!("    {} --> {} : {}\n", from, to, label));
            }
        }
    }
}

impl DiagramEmitter for ClassDiagramEmitter {
    fn emit(&self, model: &CodeModel, theme: &MermaidTheme) -> String {
        let mut output = theme.directive();
        output.push_str("classDiagram\n");

        // Group entities by namespace (derived from source_file)
        let mut by_namespace: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
        for entity in &model.entities {
            let ns = Self::namespace_from_path(&entity.source_file);
            by_namespace.entry(ns).or_default().push(entity);
        }

        // Emit each namespace block
        for (ns, entities) in &by_namespace {
            output.push_str(&format!("    namespace {} {{\n", ns));
            for entity in entities {
                Self::emit_entity(&mut output, entity, "        ");
            }
            output.push_str("    }\n");
        }

        // Relationships go outside namespace blocks
        for rel in &model.relationships {
            Self::emit_relationship(&mut output, rel);
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cardinality, EntityKind, Field, Method, Parameter, TypeInfo, Visibility};

    #[test]
    fn test_emit_empty_model() {
        let emitter = ClassDiagramEmitter;
        let model = CodeModel::new();
        assert_eq!(emitter.emit(&model, &MermaidTheme::Default), "classDiagram\n");
    }

    #[test]
    fn test_emit_struct_with_fields_and_methods() {
        let emitter = ClassDiagramEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "User".to_string(),
                kind: EntityKind::Struct,
                fields: vec![
                    Field {
                        name: "name".to_string(),
                        type_info: TypeInfo::Simple("String".to_string()),
                        visibility: Visibility::Public,
                    },
                    Field {
                        name: "age".to_string(),
                        type_info: TypeInfo::Simple("i32".to_string()),
                        visibility: Visibility::Private,
                    },
                ],
                methods: vec![
                    Method {
                        name: "get_name".to_string(),
                        parameters: vec![],
                        return_type: Some(TypeInfo::Simple("String".to_string())),
                        visibility: Visibility::Public,
                        is_static: false,
                        is_abstract: false,
                    },
                    Method {
                        name: "create".to_string(),
                        parameters: vec![
                            Parameter {
                                name: "name".to_string(),
                                type_info: TypeInfo::Simple("String".to_string()),
                            },
                            Parameter {
                                name: "age".to_string(),
                                type_info: TypeInfo::Simple("i32".to_string()),
                            },
                        ],
                        return_type: Some(TypeInfo::Simple("User".to_string())),
                        visibility: Visibility::Public,
                        is_static: true,
                        is_abstract: false,
                    },
                ],
                source_file: "user.rs".to_string(),
            }],
            functions: vec![],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("namespace user {"));
        assert!(result.contains("class User {"));
        assert!(result.contains("+String name"));
        assert!(result.contains("-i32 age"));
        assert!(result.contains("+get_name() String"));
        assert!(result.contains("+create(name String, age i32) User$"));
    }

    #[test]
    fn test_emit_relationships() {
        let emitter = ClassDiagramEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![],
            relationships: vec![
                Relationship::Inheritance {
                    child: "Dog".to_string(),
                    parent: "Animal".to_string(),
                },
                Relationship::Implementation {
                    implementor: "Dog".to_string(),
                    interface: "Pet".to_string(),
                },
                Relationship::Composition {
                    owner: "Car".to_string(),
                    owned: "Engine".to_string(),
                    field_name: "engine".to_string(),
                    cardinality: Cardinality::One,
                },
                Relationship::Aggregation {
                    from: "Library".to_string(),
                    to: "Book".to_string(),
                    field_name: "books".to_string(),
                    cardinality: Cardinality::ZeroOrMore,
                },
                Relationship::Association {
                    from: "Student".to_string(),
                    to: "Course".to_string(),
                    label: "enrolls".to_string(),
                },
            ],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        let expected = "\
classDiagram
    Dog <|-- Animal
    Dog ..|> Pet
    Car *-- Engine : engine
    Library o-- Book : books
    Student --> Course : enrolls
";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_emit_abstract_method() {
        let emitter = ClassDiagramEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "Shape".to_string(),
                kind: EntityKind::Interface,
                fields: vec![],
                methods: vec![Method {
                    name: "area".to_string(),
                    parameters: vec![],
                    return_type: Some(TypeInfo::Simple("f64".to_string())),
                    visibility: Visibility::Public,
                    is_static: false,
                    is_abstract: true,
                }],
                source_file: "shape.rs".to_string(),
            }],
            functions: vec![],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("+area() f64*"));
    }

    #[test]
    fn test_emit_enum_annotation() {
        let emitter = ClassDiagramEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "Color".to_string(),
                kind: EntityKind::Enum,
                fields: vec![],
                methods: vec![],
                source_file: "color.rs".to_string(),
            }],
            functions: vec![],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("<<Enumeration>>"));
    }

    #[test]
    fn test_emit_class_no_annotation() {
        let emitter = ClassDiagramEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "MyClass".to_string(),
                kind: EntityKind::Class,
                fields: vec![],
                methods: vec![],
                source_file: "my_class.rs".to_string(),
            }],
            functions: vec![],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("class MyClass"));
        assert!(!result.contains("<<"));
    }
}
