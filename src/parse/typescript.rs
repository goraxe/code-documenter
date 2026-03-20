use std::path::Path;

use anyhow::{Context, Result};
use tree_sitter::Node;

use crate::model::{
    CallExpr, Cardinality, CodeModel, Entity, EntityKind, Field, Function, Method, Parameter,
    Relationship, TypeInfo, Visibility,
};

use super::LanguageParser;

pub struct TypeScriptParser;

impl LanguageParser for TypeScriptParser {
    fn parse_file(&self, path: &Path, source: &str) -> Result<CodeModel> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .context("failed to set tree-sitter language to TypeScript")?;

        let tree = parser
            .parse(source, None)
            .context("tree-sitter failed to parse TypeScript source")?;

        let root = tree.root_node();
        let file_name = path.display().to_string();

        let mut model = CodeModel::new();

        collect_top_level(&root, source, &file_name, &mut model);

        Ok(model)
    }
}

// ---------------------------------------------------------------------------
// Top-level collection
// ---------------------------------------------------------------------------

fn collect_top_level(root: &Node, source: &str, file: &str, model: &mut CodeModel) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "class_declaration" => {
                handle_class(&child, source, file, model);
            }
            "interface_declaration" => {
                handle_interface(&child, source, file, model);
            }
            "enum_declaration" => {
                if let Some(entity) = parse_enum(&child, source, file) {
                    model.entities.push(entity);
                }
            }
            "type_alias_declaration" => {
                if let Some(entity) = parse_type_alias(&child, source, file) {
                    model.entities.push(entity);
                }
            }
            "function_declaration" => {
                if let Some(func) = parse_function(&child, source, file) {
                    model.functions.push(func);
                }
            }
            // Handle exported declarations: `export class Foo { ... }`
            "export_statement" => {
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    match inner_child.kind() {
                        "class_declaration" => {
                            handle_class(&inner_child, source, file, model);
                        }
                        "interface_declaration" => {
                            handle_interface(&inner_child, source, file, model);
                        }
                        "enum_declaration" => {
                            if let Some(entity) = parse_enum(&inner_child, source, file) {
                                model.entities.push(entity);
                            }
                        }
                        "type_alias_declaration" => {
                            if let Some(entity) = parse_type_alias(&inner_child, source, file) {
                                model.entities.push(entity);
                            }
                        }
                        "function_declaration" => {
                            if let Some(func) = parse_function(&inner_child, source, file) {
                                model.functions.push(func);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Class parsing
// ---------------------------------------------------------------------------

fn handle_class(node: &Node, source: &str, file: &str, model: &mut CodeModel) {
    if let Some(entity) = parse_class(node, source, file) {
        extract_class_heritage(node, source, &entity.name, &mut model.relationships);
        infer_field_relationships(&entity, &mut model.relationships);
        model.entities.push(entity);
    }
}

fn parse_class(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;
    let mut fields = Vec::new();
    let mut methods = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "public_field_definition" => {
                    if let Some(field) = parse_class_field(&child, source) {
                        fields.push(field);
                    }
                }
                "method_definition" => {
                    if let Some(method) = parse_class_method(&child, source) {
                        methods.push(method);
                    }
                }
                _ => {}
            }
        }
    }

    Some(Entity {
        name,
        kind: EntityKind::Class,
        fields,
        methods,
        source_file: file.to_string(),
    })
}

fn extract_class_heritage(
    node: &Node,
    source: &str,
    class_name: &str,
    relationships: &mut Vec<Relationship>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "class_heritage" {
            let mut heritage_cursor = child.walk();
            for clause in child.children(&mut heritage_cursor) {
                match clause.kind() {
                    "extends_clause" => {
                        // The first named child after the "extends" keyword is the type.
                        let mut clause_cursor = clause.walk();
                        for type_child in clause.children(&mut clause_cursor) {
                            if type_child.is_named() {
                                let parent_name = node_text(&type_child, source);
                                relationships.push(Relationship::Inheritance {
                                    child: class_name.to_string(),
                                    parent: parent_name,
                                });
                                break;
                            }
                        }
                    }
                    "implements_clause" => {
                        let mut clause_cursor = clause.walk();
                        for type_child in clause.children(&mut clause_cursor) {
                            if type_child.is_named() {
                                let iface_name = node_text(&type_child, source);
                                relationships.push(Relationship::Implementation {
                                    implementor: class_name.to_string(),
                                    interface: iface_name,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn parse_class_field(node: &Node, source: &str) -> Option<Field> {
    let name = child_by_field(node, "name", source)?;
    let visibility = extract_accessibility(node, source);
    let is_optional = has_optional_marker(node);

    let type_info = if let Some(type_ann) = node.child_by_field_name("type") {
        let ti = parse_type_node(&type_ann, source);
        if is_optional {
            wrap_optional_if_needed(ti)
        } else {
            ti
        }
    } else {
        TypeInfo::Simple("unknown".to_string())
    };

    Some(Field {
        name,
        type_info,
        visibility,
    })
}

fn parse_class_method(node: &Node, source: &str) -> Option<Method> {
    let name = child_by_field(node, "name", source)?;
    let visibility = extract_accessibility(node, source);
    let parameters = parse_parameters(node, source);
    let return_type = parse_return_type(node, source);
    let is_static = has_child_kind(node, "static");
    let is_abstract = has_child_kind(node, "abstract");

    Some(Method {
        name,
        parameters,
        return_type,
        visibility,
        is_static,
        is_abstract,
    })
}

// ---------------------------------------------------------------------------
// Interface parsing
// ---------------------------------------------------------------------------

fn handle_interface(node: &Node, source: &str, file: &str, model: &mut CodeModel) {
    if let Some(entity) = parse_interface(node, source, file) {
        extract_interface_heritage(node, source, &entity.name, &mut model.relationships);
        infer_field_relationships(&entity, &mut model.relationships);
        model.entities.push(entity);
    }
}

fn parse_interface(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;
    let mut fields = Vec::new();
    let mut methods = Vec::new();

    // Interface body is an object_type node.
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "property_signature" => {
                    if let Some(field) = parse_property_signature(&child, source) {
                        fields.push(field);
                    }
                }
                "method_signature" => {
                    if let Some(method) = parse_method_signature(&child, source) {
                        methods.push(method);
                    }
                }
                _ => {}
            }
        }
    }

    Some(Entity {
        name,
        kind: EntityKind::Interface,
        fields,
        methods,
        source_file: file.to_string(),
    })
}

fn extract_interface_heritage(
    node: &Node,
    source: &str,
    iface_name: &str,
    relationships: &mut Vec<Relationship>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "extends_type_clause" {
            let mut clause_cursor = child.walk();
            for type_child in child.children(&mut clause_cursor) {
                if type_child.is_named() {
                    let parent_name = node_text(&type_child, source);
                    relationships.push(Relationship::Inheritance {
                        child: iface_name.to_string(),
                        parent: parent_name,
                    });
                }
            }
        }
    }
}

fn parse_property_signature(node: &Node, source: &str) -> Option<Field> {
    let name = child_by_field(node, "name", source)?;
    let is_optional = has_optional_marker(node);

    let type_info = if let Some(type_ann) = node.child_by_field_name("type") {
        let ti = parse_type_node(&type_ann, source);
        if is_optional {
            wrap_optional_if_needed(ti)
        } else {
            ti
        }
    } else {
        TypeInfo::Simple("unknown".to_string())
    };

    Some(Field {
        name,
        type_info,
        visibility: Visibility::Public,
    })
}

fn parse_method_signature(node: &Node, source: &str) -> Option<Method> {
    let name = child_by_field(node, "name", source)?;
    let parameters = parse_parameters(node, source);
    let return_type = parse_return_type(node, source);

    Some(Method {
        name,
        parameters,
        return_type,
        visibility: Visibility::Public,
        is_static: false,
        is_abstract: true,
    })
}

// ---------------------------------------------------------------------------
// Enum parsing
// ---------------------------------------------------------------------------

fn parse_enum(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;
    let mut fields = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            // In tree-sitter-typescript, enum members appear as property_identifier
            // nodes directly inside the enum_body.
            if child.kind() == "property_identifier" {
                fields.push(Field {
                    name: node_text(&child, source),
                    type_info: TypeInfo::Simple("()".to_string()),
                    visibility: Visibility::Public,
                });
            }
            // Also handle enum_assignment (e.g., `Red = 0`).
            if child.kind() == "enum_assignment" {
                if let Some(member_name) = child_by_field(&child, "name", source) {
                    fields.push(Field {
                        name: member_name,
                        type_info: TypeInfo::Simple("()".to_string()),
                        visibility: Visibility::Public,
                    });
                }
            }
        }
    }

    Some(Entity {
        name,
        kind: EntityKind::Enum,
        fields,
        methods: Vec::new(),
        source_file: file.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Type alias parsing
// ---------------------------------------------------------------------------

fn parse_type_alias(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;

    Some(Entity {
        name,
        kind: EntityKind::TypeAlias,
        fields: Vec::new(),
        methods: Vec::new(),
        source_file: file.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Function parsing
// ---------------------------------------------------------------------------

fn parse_function(node: &Node, source: &str, file: &str) -> Option<Function> {
    let name = child_by_field(node, "name", source)?;
    let visibility = Visibility::Public; // TypeScript top-level functions are public by default.
    let parameters = parse_parameters(node, source);
    let return_type = parse_return_type(node, source);

    let mut calls = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        collect_call_exprs(&body, source, &mut calls);
    }

    Some(Function {
        name,
        parameters,
        return_type,
        visibility,
        calls,
        source_file: file.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Parameter and return-type parsing
// ---------------------------------------------------------------------------

fn parse_parameters(node: &Node, source: &str) -> Vec<Parameter> {
    let mut params = Vec::new();
    if let Some(param_list) = node.child_by_field_name("parameters") {
        let mut cursor = param_list.walk();
        for child in param_list.children(&mut cursor) {
            match child.kind() {
                "required_parameter" | "optional_parameter" => {
                    if let Some(param) = parse_single_parameter(&child, source) {
                        params.push(param);
                    }
                }
                _ => {}
            }
        }
    }
    params
}

fn parse_single_parameter(node: &Node, source: &str) -> Option<Parameter> {
    // In tree-sitter-typescript, parameters have a "pattern" field for the name
    // and a "type" field for the type annotation.
    let name = child_by_field(node, "pattern", source)?;
    let type_info = if let Some(type_ann) = node.child_by_field_name("type") {
        parse_type_node(&type_ann, source)
    } else {
        TypeInfo::Simple("unknown".to_string())
    };
    Some(Parameter { name, type_info })
}

fn parse_return_type(node: &Node, source: &str) -> Option<TypeInfo> {
    let ret = node.child_by_field_name("return_type")?;
    // The return_type field wraps a type_annotation; dig into its child.
    let mut cursor = ret.walk();
    for child in ret.children(&mut cursor) {
        if child.is_named() {
            return Some(parse_type_node(&child, source));
        }
    }
    Some(parse_type_node(&ret, source))
}

// ---------------------------------------------------------------------------
// Call expression extraction
// ---------------------------------------------------------------------------

fn collect_call_exprs(node: &Node, source: &str, calls: &mut Vec<CallExpr>) {
    if node.kind() == "call_expression" {
        if let Some(call) = parse_call_expr(node, source) {
            calls.push(call);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_call_exprs(&child, source, calls);
    }
}

fn parse_call_expr(node: &Node, source: &str) -> Option<CallExpr> {
    let func_node = node.child_by_field_name("function")?;
    let arguments = extract_arguments(node, source);

    match func_node.kind() {
        "member_expression" => {
            let receiver = func_node
                .child_by_field_name("object")
                .map(|n| node_text(&n, source));
            let method = func_node
                .child_by_field_name("property")
                .map(|n| node_text(&n, source))
                .unwrap_or_default();
            Some(CallExpr {
                receiver,
                method,
                arguments,
            })
        }
        "identifier" => {
            let text = node_text(&func_node, source);
            Some(CallExpr {
                receiver: None,
                method: text,
                arguments,
            })
        }
        _ => {
            let text = node_text(&func_node, source);
            Some(CallExpr {
                receiver: None,
                method: text,
                arguments,
            })
        }
    }
}

fn extract_arguments(node: &Node, source: &str) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(arg_list) = node.child_by_field_name("arguments") {
        let mut cursor = arg_list.walk();
        for child in arg_list.children(&mut cursor) {
            if child.is_named() {
                args.push(node_text(&child, source));
            }
        }
    }
    args
}

// ---------------------------------------------------------------------------
// Type parsing
// ---------------------------------------------------------------------------

fn parse_type_node(node: &Node, source: &str) -> TypeInfo {
    match node.kind() {
        "type_identifier" | "predefined_type" => TypeInfo::Simple(node_text(node, source)),
        "generic_type" => parse_generic_type(node, source),
        "array_type" => {
            // T[] => Collection(T)
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return TypeInfo::Collection(Box::new(parse_type_node(&child, source)));
                }
            }
            TypeInfo::Simple(node_text(node, source))
        }
        "union_type" => parse_union_type(node, source),
        "parenthesized_type" => {
            // (T) => unwrap
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return parse_type_node(&child, source);
                }
            }
            TypeInfo::Simple(node_text(node, source))
        }
        "type_annotation" => {
            // Unwrap type_annotation to get the inner type.
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return parse_type_node(&child, source);
                }
            }
            TypeInfo::Simple(node_text(node, source))
        }
        _ => TypeInfo::Simple(node_text(node, source)),
    }
}

fn parse_generic_type(node: &Node, source: &str) -> TypeInfo {
    let base = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    let mut params = Vec::new();
    if let Some(type_args) = node.child_by_field_name("type_arguments") {
        let mut cursor = type_args.walk();
        for child in type_args.children(&mut cursor) {
            if child.is_named() {
                params.push(parse_type_node(&child, source));
            }
        }
    }

    // Map Array<T> to Collection(T).
    if base == "Array" && params.len() == 1 {
        return TypeInfo::Collection(Box::new(params.remove(0)));
    }

    TypeInfo::Generic { base, params }
}

fn parse_union_type(node: &Node, source: &str) -> TypeInfo {
    let mut cursor = node.walk();
    let mut members: Vec<Node> = Vec::new();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            members.push(child);
        }
    }

    // Check for T | null or T | undefined => Optional(T)
    let null_types = ["null", "undefined"];
    let non_null: Vec<&Node> = members
        .iter()
        .filter(|m| {
            let text = node_text(m, source);
            !null_types.contains(&text.as_str())
        })
        .collect();

    let has_nullable = members.iter().any(|m| {
        let text = node_text(m, source);
        null_types.contains(&text.as_str())
    });

    if has_nullable && non_null.len() == 1 {
        return TypeInfo::Optional(Box::new(parse_type_node(non_null[0], source)));
    }

    // Otherwise, just return the full text as a simple type.
    TypeInfo::Simple(node_text(node, source))
}

// ---------------------------------------------------------------------------
// Relationship inference
// ---------------------------------------------------------------------------

fn infer_field_relationships(entity: &Entity, relationships: &mut Vec<Relationship>) {
    let owner = &entity.name;
    for field in &entity.fields {
        infer_from_type(owner, &field.name, &field.type_info, relationships);
    }
}

fn infer_from_type(
    owner: &str,
    field_name: &str,
    type_info: &TypeInfo,
    relationships: &mut Vec<Relationship>,
) {
    match type_info {
        TypeInfo::Simple(name) => {
            if is_user_type(name) {
                relationships.push(Relationship::Composition {
                    owner: owner.to_string(),
                    owned: name.clone(),
                    field_name: field_name.to_string(),
                    cardinality: Cardinality::One,
                });
            }
        }
        TypeInfo::Optional(inner) => {
            if let Some(name) = extract_user_type_name(inner) {
                relationships.push(Relationship::Composition {
                    owner: owner.to_string(),
                    owned: name,
                    field_name: field_name.to_string(),
                    cardinality: Cardinality::ZeroOrOne,
                });
            }
        }
        TypeInfo::Collection(inner) => {
            if let Some(name) = extract_user_type_name(inner) {
                relationships.push(Relationship::Composition {
                    owner: owner.to_string(),
                    owned: name,
                    field_name: field_name.to_string(),
                    cardinality: Cardinality::ZeroOrMore,
                });
            }
        }
        TypeInfo::Generic { base: _, params } => {
            for param in params {
                infer_from_type(owner, field_name, param, relationships);
            }
        }
        TypeInfo::Tuple(items) => {
            for item in items {
                infer_from_type(owner, field_name, item, relationships);
            }
        }
        TypeInfo::Reference(inner) => {
            infer_from_type(owner, field_name, inner, relationships);
        }
    }
}

/// Check if a type name looks like a user-defined type (starts with uppercase,
/// and is not a well-known primitive or built-in).
fn is_user_type(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_uppercase() {
        return false;
    }
    // Exclude well-known TypeScript/JavaScript built-in types.
    !matches!(
        name,
        "String"
            | "Number"
            | "Boolean"
            | "Object"
            | "Symbol"
            | "BigInt"
            | "Date"
            | "RegExp"
            | "Error"
            | "Promise"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Array"
    )
}

fn extract_user_type_name(type_info: &TypeInfo) -> Option<String> {
    match type_info {
        TypeInfo::Simple(name) if is_user_type(name) => Some(name.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Visibility helpers
// ---------------------------------------------------------------------------

fn extract_accessibility(node: &Node, source: &str) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            let text = node_text(&child, source);
            return match text.as_str() {
                "private" => Visibility::Private,
                "protected" => Visibility::Protected,
                _ => Visibility::Public,
            };
        }
    }
    Visibility::Public
}

/// Check whether the node has a `?` child (optional property/parameter marker).
fn has_optional_marker(node: &Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "?" {
            return true;
        }
    }
    false
}

fn has_child_kind(node: &Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

fn node_text(node: &Node, source: &str) -> String {
    source[node.byte_range()].to_string()
}

fn child_by_field(node: &Node, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(&n, source))
}

/// Wrap a TypeInfo in Optional if it is not already optional.
fn wrap_optional_if_needed(ti: TypeInfo) -> TypeInfo {
    match ti {
        TypeInfo::Optional(_) => ti,
        other => TypeInfo::Optional(Box::new(other)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> CodeModel {
        let parser = TypeScriptParser;
        parser
            .parse_file(Path::new("test.ts"), source)
            .expect("parse failed")
    }

    // -- Class parsing --

    #[test]
    fn test_parse_class_with_fields_and_methods() {
        let model = parse(
            r#"
            class User {
                public name: string;
                private age: number;
                protected email: string;

                constructor(name: string, age: number) {}

                public greet(): string {
                    return "hello";
                }

                private validate(): boolean {
                    return true;
                }
            }
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "User");
        assert!(matches!(entity.kind, EntityKind::Class));

        // Fields
        assert_eq!(entity.fields.len(), 3);
        assert_eq!(entity.fields[0].name, "name");
        assert!(matches!(entity.fields[0].visibility, Visibility::Public));
        assert_eq!(entity.fields[1].name, "age");
        assert!(matches!(entity.fields[1].visibility, Visibility::Private));
        assert_eq!(entity.fields[2].name, "email");
        assert!(matches!(
            entity.fields[2].visibility,
            Visibility::Protected
        ));

        // Methods (constructor + greet + validate)
        assert!(entity.methods.len() >= 2);
        let greet = entity.methods.iter().find(|m| m.name == "greet").unwrap();
        assert!(matches!(greet.visibility, Visibility::Public));
        assert!(greet.return_type.is_some());

        let validate = entity
            .methods
            .iter()
            .find(|m| m.name == "validate")
            .unwrap();
        assert!(matches!(validate.visibility, Visibility::Private));
    }

    #[test]
    fn test_parse_class_extends() {
        let model = parse(
            r#"
            class Animal {
                name: string;
            }

            class Dog extends Animal {
                breed: string;
            }
            "#,
        );

        assert_eq!(model.entities.len(), 2);

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Inheritance {
                    child,
                    parent,
                } if child == "Dog" && parent == "Animal"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_parse_class_implements() {
        let model = parse(
            r#"
            interface Serializable {
                serialize(): string;
            }

            class Config implements Serializable {
                serialize(): string {
                    return "{}";
                }
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Implementation {
                    implementor,
                    interface,
                } if implementor == "Config" && interface == "Serializable"
            )
        });
        assert!(rel.is_some());
    }

    // -- Interface parsing --

    #[test]
    fn test_parse_interface_with_properties_and_methods() {
        let model = parse(
            r#"
            interface Shape {
                x: number;
                y: number;
                area(): number;
                perimeter(): number;
            }
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "Shape");
        assert!(matches!(entity.kind, EntityKind::Interface));

        assert_eq!(entity.fields.len(), 2);
        assert_eq!(entity.fields[0].name, "x");
        assert_eq!(entity.fields[1].name, "y");

        assert_eq!(entity.methods.len(), 2);
        assert_eq!(entity.methods[0].name, "area");
        assert!(entity.methods[0].is_abstract);
        assert_eq!(entity.methods[1].name, "perimeter");
    }

    #[test]
    fn test_parse_interface_extends() {
        let model = parse(
            r#"
            interface Base {
                id: number;
            }

            interface Extended extends Base {
                name: string;
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Inheritance {
                    child,
                    parent,
                } if child == "Extended" && parent == "Base"
            )
        });
        assert!(rel.is_some());
    }

    // -- Enum parsing --

    #[test]
    fn test_parse_enum_with_members() {
        let model = parse(
            r#"
            enum Color {
                Red,
                Green,
                Blue
            }
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "Color");
        assert!(matches!(entity.kind, EntityKind::Enum));
        assert_eq!(entity.fields.len(), 3);
        assert_eq!(entity.fields[0].name, "Red");
        assert_eq!(entity.fields[1].name, "Green");
        assert_eq!(entity.fields[2].name, "Blue");
    }

    // -- Type alias --

    #[test]
    fn test_parse_type_alias() {
        let model = parse(
            r#"
            type ID = string | number;
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "ID");
        assert!(matches!(entity.kind, EntityKind::TypeAlias));
    }

    // -- Optional fields --

    #[test]
    fn test_parse_optional_field() {
        let model = parse(
            r#"
            interface Config {
                name: string;
                description?: string;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert_eq!(entity.fields.len(), 2);

        // The non-optional field.
        assert!(matches!(
            &entity.fields[0].type_info,
            TypeInfo::Simple(s) if s == "string"
        ));

        // The optional field should be wrapped in Optional.
        assert!(matches!(&entity.fields[1].type_info, TypeInfo::Optional(_)));
    }

    // -- Array type fields --

    #[test]
    fn test_parse_array_type_field() {
        let model = parse(
            r#"
            class Team {
                members: User[];
            }
            "#,
        );

        let entity = &model.entities[0];
        assert_eq!(entity.fields[0].name, "members");
        assert!(matches!(
            &entity.fields[0].type_info,
            TypeInfo::Collection(_)
        ));

        // Should produce a Composition relationship with ZeroOrMore cardinality.
        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Composition {
                    owner,
                    owned,
                    cardinality: Cardinality::ZeroOrMore,
                    ..
                } if owner == "Team" && owned == "User"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_parse_generic_array_field() {
        let model = parse(
            r#"
            class Team {
                members: Array<User>;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert_eq!(entity.fields[0].name, "members");
        assert!(matches!(
            &entity.fields[0].type_info,
            TypeInfo::Collection(_)
        ));
    }

    // -- Union type with null/undefined --

    #[test]
    fn test_parse_union_null_field() {
        let model = parse(
            r#"
            class Order {
                user: User | null;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert!(matches!(&entity.fields[0].type_info, TypeInfo::Optional(_)));

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Composition {
                    owner,
                    owned,
                    cardinality: Cardinality::ZeroOrOne,
                    ..
                } if owner == "Order" && owned == "User"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_parse_union_undefined_field() {
        let model = parse(
            r#"
            class Order {
                user: User | undefined;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert!(matches!(&entity.fields[0].type_info, TypeInfo::Optional(_)));
    }

    // -- Composition from direct field type --

    #[test]
    fn test_composition_from_direct_field() {
        let model = parse(
            r#"
            class Order {
                user: User;
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Composition {
                    owner,
                    owned,
                    cardinality: Cardinality::One,
                    ..
                } if owner == "Order" && owned == "User"
            )
        });
        assert!(rel.is_some());
    }

    // -- Visibility modifiers --

    #[test]
    fn test_visibility_defaults_to_public() {
        let model = parse(
            r#"
            class Foo {
                bar: string;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert!(matches!(entity.fields[0].visibility, Visibility::Public));
    }

    // -- Functions --

    #[test]
    fn test_parse_function_with_params_and_return() {
        let model = parse(
            r#"
            function add(a: number, b: number): number {
                return a + b;
            }
            "#,
        );

        assert_eq!(model.functions.len(), 1);
        let func = &model.functions[0];
        assert_eq!(func.name, "add");
        assert_eq!(func.parameters.len(), 2);
        assert_eq!(func.parameters[0].name, "a");
        assert_eq!(func.parameters[1].name, "b");
        assert!(func.return_type.is_some());
    }

    #[test]
    fn test_parse_function_calls() {
        let model = parse(
            r#"
            function doStuff() {
                const x = foo();
                const y = bar.baz(1, 2);
            }
            "#,
        );

        assert_eq!(model.functions.len(), 1);
        let calls = &model.functions[0].calls;
        assert!(calls.len() >= 2);

        let foo_call = calls.iter().find(|c| c.method == "foo").unwrap();
        assert!(foo_call.receiver.is_none());

        let baz_call = calls.iter().find(|c| c.method == "baz").unwrap();
        assert_eq!(baz_call.receiver.as_deref(), Some("bar"));
    }

    #[test]
    fn test_source_file_is_set() {
        let model = parse(
            r#"
            class Foo {}
            function bar() {}
            "#,
        );

        assert_eq!(model.entities[0].source_file, "test.ts");
        assert_eq!(model.functions[0].source_file, "test.ts");
    }

    #[test]
    fn test_no_relationship_for_primitive_types() {
        let model = parse(
            r#"
            class Simple {
                count: number;
                name: string;
            }
            "#,
        );

        assert!(model.relationships.is_empty());
    }

    #[test]
    fn test_optional_field_with_question_mark() {
        let model = parse(
            r#"
            class Config {
                user?: User;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert!(matches!(&entity.fields[0].type_info, TypeInfo::Optional(_)));

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Composition {
                    cardinality: Cardinality::ZeroOrOne,
                    ..
                }
            )
        });
        assert!(rel.is_some());
    }
}
