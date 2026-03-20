use std::path::Path;

use anyhow::{Context, Result};
use tree_sitter::Node;

use crate::model::{
    CallExpr, Cardinality, CodeModel, Entity, EntityKind, Field, Function, Method, Parameter,
    Relationship, TypeInfo, Visibility,
};

use super::LanguageParser;

pub struct GoParser;

impl LanguageParser for GoParser {
    fn parse_file(&self, path: &Path, source: &str) -> Result<CodeModel> {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
        parser
            .set_language(&language)
            .context("failed to set tree-sitter language to Go")?;

        let tree = parser
            .parse(source, None)
            .context("tree-sitter failed to parse Go source")?;

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
            "type_declaration" => {
                process_type_declaration(&child, source, file, model);
            }
            "function_declaration" => {
                if let Some(func) = parse_function(&child, source, file) {
                    model.functions.push(func);
                }
            }
            "method_declaration" => {
                process_method_declaration(&child, source, file, model);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Type declaration processing
// ---------------------------------------------------------------------------

fn process_type_declaration(node: &Node, source: &str, file: &str, model: &mut CodeModel) {
    // A type_declaration contains one or more type_spec children.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" {
            process_type_spec(&child, source, file, model);
        }
    }
}

fn process_type_spec(node: &Node, source: &str, file: &str, model: &mut CodeModel) {
    let name = match child_by_field(node, "name", source) {
        Some(n) => n,
        None => return,
    };

    let type_node = match node.child_by_field_name("type") {
        Some(n) => n,
        None => return,
    };

    match type_node.kind() {
        "struct_type" => {
            if let Some(entity) = parse_struct(&name, &type_node, source, file) {
                infer_field_relationships(&entity, &mut model.relationships);
                model.entities.push(entity);
            }
        }
        "interface_type" => {
            if let Some(entity) = parse_interface(&name, &type_node, source, file) {
                model.entities.push(entity);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Struct parsing
// ---------------------------------------------------------------------------

fn parse_struct(name: &str, node: &Node, source: &str, file: &str) -> Option<Entity> {
    let mut fields = Vec::new();

    if let Some(field_list) = node.child_by_field_name("body") {
        parse_field_list(&field_list, source, name, &mut fields);
    } else {
        // Some versions: field_declaration_list is a direct child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "field_declaration_list" {
                parse_field_list(&child, source, name, &mut fields);
            }
        }
    }

    Some(Entity {
        name: name.to_string(),
        kind: EntityKind::Struct,
        fields,
        methods: Vec::new(),
        source_file: file.to_string(),
    })
}

fn parse_field_list(
    field_list: &Node,
    source: &str,
    _struct_name: &str,
    fields: &mut Vec<Field>,
) {
    let mut cursor = field_list.walk();
    for child in field_list.children(&mut cursor) {
        if child.kind() == "field_declaration" {
            parse_field_declaration(&child, source, fields);
        }
    }
}

fn parse_field_declaration(node: &Node, source: &str, fields: &mut Vec<Field>) {
    // A field_declaration may have explicit field names or be an embedded type.
    // Collect named fields first.
    let mut names: Vec<String> = Vec::new();
    let mut type_node: Option<Node> = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "field_identifier" => {
                names.push(node_text(&child, source));
            }
            _ if is_type_node(&child) => {
                type_node = Some(child);
            }
            _ => {}
        }
    }

    if let Some(ref tn) = type_node {
        if names.is_empty() {
            // Embedded field: the type itself is the "name".
            let embedded_name = extract_embedded_name(tn, source);
            let type_info = parse_type_node(tn, source);
            let visibility = go_visibility(&embedded_name);
            fields.push(Field {
                name: embedded_name,
                type_info,
                visibility,
            });
        } else {
            let type_info = parse_type_node(tn, source);
            for name in &names {
                let visibility = go_visibility(name);
                fields.push(Field {
                    name: name.clone(),
                    type_info: clone_type_info(&type_info),
                    visibility,
                });
            }
        }
    }
}

/// Extract the base type name from an embedded field node.
/// For `*User` returns "User", for `pkg.User` returns "User", etc.
fn extract_embedded_name(node: &Node, source: &str) -> String {
    match node.kind() {
        "pointer_type" => {
            // *Type -> get inner
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return extract_embedded_name(&child, source);
                }
            }
            node_text(node, source)
        }
        "qualified_type" => {
            // pkg.Type -> take the type part
            if let Some(name_node) = node.child_by_field_name("name") {
                return node_text(&name_node, source);
            }
            node_text(node, source)
        }
        _ => node_text(node, source),
    }
}

/// Check if a node looks like a type node (not punctuation, not a field identifier).
fn is_type_node(node: &Node) -> bool {
    is_type_node_kind(node.kind())
}

// ---------------------------------------------------------------------------
// Interface parsing
// ---------------------------------------------------------------------------

fn parse_interface(name: &str, node: &Node, source: &str, file: &str) -> Option<Entity> {
    let mut methods = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // tree-sitter-go v0.25 uses "method_elem" for interface method specs.
        if child.kind() == "method_elem" || child.kind() == "method_spec" {
            if let Some(method) = parse_method_elem(&child, source) {
                methods.push(method);
            }
        }
    }

    Some(Entity {
        name: name.to_string(),
        kind: EntityKind::Interface,
        fields: Vec::new(),
        methods,
        source_file: file.to_string(),
    })
}

/// Parse a method_elem (or method_spec) node from an interface definition.
/// Structure: field_identifier, parameter_list (params), then either
/// a type_identifier (single return) or parameter_list (multiple returns).
fn parse_method_elem(node: &Node, source: &str) -> Option<Method> {
    // The name is a field_identifier child (or accessible via "name" field).
    let name = child_by_field(node, "name", source).or_else(|| {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "field_identifier" {
                return Some(node_text(&child, source));
            }
        }
        None
    })?;

    let visibility = go_visibility(&name);

    // Collect parameter_list nodes. The first is parameters, the second (if it is a
    // parameter_list) is the return type tuple.
    let mut param_lists: Vec<Node> = Vec::new();
    let mut return_type_node: Option<Node> = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "parameter_list" => {
                param_lists.push(child);
            }
            k if is_type_node_kind(k) => {
                return_type_node = Some(child);
            }
            _ => {}
        }
    }

    let mut parameters = Vec::new();
    if let Some(first_params) = param_lists.first() {
        collect_parameters_from_list(first_params, source, &mut parameters);
    }

    // Determine return type.
    let return_type = if param_lists.len() >= 2 {
        // Second parameter_list is the return type (multiple returns).
        let ret_list = &param_lists[1];
        let mut items = Vec::new();
        let mut ret_cursor = ret_list.walk();
        for child in ret_list.children(&mut ret_cursor) {
            if child.kind() == "parameter_declaration" {
                if let Some(type_n) = child.child_by_field_name("type") {
                    items.push(parse_type_node(&type_n, source));
                }
            }
        }
        if items.len() == 1 {
            Some(items.remove(0))
        } else if items.is_empty() {
            None
        } else {
            Some(TypeInfo::Tuple(items))
        }
    } else {
        return_type_node.as_ref().map(|rtn| parse_type_node(rtn, source))
    };

    Some(Method {
        name,
        parameters,
        return_type,
        visibility,
        is_static: false,
        is_abstract: true,
    })
}

fn is_type_node_kind(kind: &str) -> bool {
    matches!(
        kind,
        "type_identifier"
            | "pointer_type"
            | "slice_type"
            | "array_type"
            | "map_type"
            | "channel_type"
            | "function_type"
            | "interface_type"
            | "struct_type"
            | "qualified_type"
            | "generic_type"
    )
}

// ---------------------------------------------------------------------------
// Function parsing
// ---------------------------------------------------------------------------

fn parse_function(node: &Node, source: &str, file: &str) -> Option<Function> {
    let name = child_by_field(node, "name", source)?;
    let visibility = go_visibility(&name);
    let parameters = parse_parameters(node, source);
    let return_type = parse_result(node, source);

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
// Method declaration processing
// ---------------------------------------------------------------------------

fn process_method_declaration(node: &Node, source: &str, file: &str, model: &mut CodeModel) {
    let name = match child_by_field(node, "name", source) {
        Some(n) => n,
        None => return,
    };

    let receiver_type = match extract_receiver_type(node, source) {
        Some(t) => t,
        None => return,
    };

    let visibility = go_visibility(&name);
    let parameters = parse_method_parameters(node, source);
    let return_type = parse_result(node, source);

    let mut calls = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        collect_call_exprs(&body, source, &mut calls);
    }

    let method = Method {
        name: name.clone(),
        parameters,
        return_type,
        visibility,
        is_static: false,
        is_abstract: false,
    };

    // Also record method calls as a function for cross-reference.
    if !calls.is_empty() {
        model.functions.push(Function {
            name: format!("{}.{}", receiver_type, name),
            parameters: Vec::new(),
            return_type: None,
            visibility: go_visibility(&name),
            calls,
            source_file: file.to_string(),
        });
    }

    attach_method_to_entity(model, &receiver_type, method, file);
}

fn extract_receiver_type(node: &Node, source: &str) -> Option<String> {
    // method_declaration has a "receiver" field which is a parameter_list.
    let receiver = node.child_by_field_name("receiver")?;
    let mut cursor = receiver.walk();
    for child in receiver.children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            // The type of the receiver parameter, e.g. `*User` or `User`.
            if let Some(type_node) = child.child_by_field_name("type") {
                return Some(extract_base_type_name(&type_node, source));
            }
        }
    }
    None
}

/// Extract the base type name from a receiver type, stripping pointers.
fn extract_base_type_name(node: &Node, source: &str) -> String {
    match node.kind() {
        "pointer_type" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return extract_base_type_name(&child, source);
                }
            }
            node_text(node, source)
        }
        _ => node_text(node, source),
    }
}

fn attach_method_to_entity(model: &mut CodeModel, type_name: &str, method: Method, file: &str) {
    if let Some(entity) = model.entities.iter_mut().find(|e| e.name == type_name) {
        entity.methods.push(method);
    } else {
        // Create a new struct entity for methods without a prior struct definition.
        let entity = Entity {
            name: type_name.to_string(),
            kind: EntityKind::Struct,
            fields: Vec::new(),
            methods: vec![method],
            source_file: file.to_string(),
        };
        model.entities.push(entity);
    }
}

// ---------------------------------------------------------------------------
// Parameter parsing
// ---------------------------------------------------------------------------

fn parse_parameters(node: &Node, source: &str) -> Vec<Parameter> {
    let mut params = Vec::new();
    if let Some(param_list) = node.child_by_field_name("parameters") {
        collect_parameters_from_list(&param_list, source, &mut params);
    }
    params
}

/// For method declarations, skip the receiver and parse the second parameter_list.
fn parse_method_parameters(node: &Node, source: &str) -> Vec<Parameter> {
    let mut params = Vec::new();
    if let Some(param_list) = node.child_by_field_name("parameters") {
        collect_parameters_from_list(&param_list, source, &mut params);
    }
    params
}

fn collect_parameters_from_list(param_list: &Node, source: &str, params: &mut Vec<Parameter>) {
    let mut cursor = param_list.walk();
    for child in param_list.children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            parse_parameter_declaration(&child, source, params);
        }
    }
}

fn parse_parameter_declaration(node: &Node, source: &str, params: &mut Vec<Parameter>) {
    let type_node = match node.child_by_field_name("type") {
        Some(n) => n,
        None => return,
    };
    let type_info = parse_type_node(&type_node, source);

    // Collect parameter names (Go allows multiple names per declaration).
    let mut names: Vec<String> = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            names.push(node_text(&child, source));
        }
    }

    if names.is_empty() {
        // Unnamed parameter — use empty string.
        params.push(Parameter {
            name: String::new(),
            type_info,
        });
    } else {
        for name in names {
            params.push(Parameter {
                name,
                type_info: clone_type_info(&type_info),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Return type (result) parsing
// ---------------------------------------------------------------------------

fn parse_result(node: &Node, source: &str) -> Option<TypeInfo> {
    let result_node = node.child_by_field_name("result")?;
    match result_node.kind() {
        "parameter_list" => {
            // Multiple return values -> Tuple
            let mut items = Vec::new();
            let mut cursor = result_node.walk();
            for child in result_node.children(&mut cursor) {
                if child.kind() == "parameter_declaration" {
                    if let Some(type_n) = child.child_by_field_name("type") {
                        items.push(parse_type_node(&type_n, source));
                    }
                }
            }
            if items.len() == 1 {
                Some(items.remove(0))
            } else if items.is_empty() {
                None
            } else {
                Some(TypeInfo::Tuple(items))
            }
        }
        _ => {
            // Single return type.
            Some(parse_type_node(&result_node, source))
        }
    }
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
        "selector_expression" => {
            // e.g., `receiver.Method(args)`
            let receiver = func_node
                .child_by_field_name("operand")
                .map(|n| node_text(&n, source));
            let method = func_node
                .child_by_field_name("field")
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
            // Skip punctuation and delimiters.
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
        "type_identifier" => TypeInfo::Simple(node_text(node, source)),
        "qualified_type" => TypeInfo::Simple(node_text(node, source)),
        "pointer_type" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    return TypeInfo::Reference(Box::new(parse_type_node(&child, source)));
                }
            }
            TypeInfo::Simple(node_text(node, source))
        }
        "slice_type" => {
            if let Some(elem) = node.child_by_field_name("element") {
                TypeInfo::Collection(Box::new(parse_type_node(&elem, source)))
            } else {
                TypeInfo::Simple(node_text(node, source))
            }
        }
        "array_type" => {
            if let Some(elem) = node.child_by_field_name("element") {
                TypeInfo::Collection(Box::new(parse_type_node(&elem, source)))
            } else {
                TypeInfo::Simple(node_text(node, source))
            }
        }
        "map_type" => {
            let key = node
                .child_by_field_name("key")
                .map(|n| parse_type_node(&n, source));
            let value = node
                .child_by_field_name("value")
                .map(|n| parse_type_node(&n, source));
            match (key, value) {
                (Some(k), Some(v)) => TypeInfo::Generic {
                    base: "map".to_string(),
                    params: vec![k, v],
                },
                _ => TypeInfo::Simple(node_text(node, source)),
            }
        }
        "interface_type" => TypeInfo::Simple("interface{}".to_string()),
        "channel_type" => TypeInfo::Simple(node_text(node, source)),
        "function_type" => TypeInfo::Simple(node_text(node, source)),
        _ => TypeInfo::Simple(node_text(node, source)),
    }
}

// ---------------------------------------------------------------------------
// Relationship inference
// ---------------------------------------------------------------------------

fn infer_field_relationships(entity: &Entity, relationships: &mut Vec<Relationship>) {
    let owner = &entity.name;
    for field in &entity.fields {
        // Check for embedded field (field name matches a type name and there's no explicit
        // separate type — embedded fields use the type name as the field name).
        if is_embedded_field(field) {
            let parent_name = extract_type_name_from_info(&field.type_info);
            if let Some(parent) = parent_name {
                relationships.push(Relationship::Inheritance {
                    child: owner.to_string(),
                    parent,
                });
                continue;
            }
        }
        infer_from_type(owner, &field.name, &field.type_info, relationships);
    }
}

/// Determine if a field is an embedded struct field.
/// In Go, embedded fields have the type name as the field name (e.g., field `User` with type `User`).
fn is_embedded_field(field: &Field) -> bool {
    match &field.type_info {
        TypeInfo::Simple(type_name) => field.name == *type_name,
        TypeInfo::Reference(inner) => {
            // *User embedded
            if let TypeInfo::Simple(type_name) = inner.as_ref() {
                field.name == *type_name
            } else {
                false
            }
        }
        _ => false,
    }
}

fn extract_type_name_from_info(type_info: &TypeInfo) -> Option<String> {
    match type_info {
        TypeInfo::Simple(name) if is_user_type(name) => Some(name.clone()),
        TypeInfo::Reference(inner) => extract_type_name_from_info(inner),
        _ => None,
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
        TypeInfo::Reference(inner) => {
            // Pointer field → Aggregation
            if let Some(name) = extract_user_type_name(inner) {
                relationships.push(Relationship::Aggregation {
                    from: owner.to_string(),
                    to: name,
                    field_name: field_name.to_string(),
                    cardinality: Cardinality::ZeroOrOne,
                });
            }
        }
        TypeInfo::Collection(inner) => {
            // Slice/array field
            match inner.as_ref() {
                TypeInfo::Reference(ptr_inner) => {
                    // []*Type → Aggregation with ZeroOrMore
                    if let Some(name) = extract_user_type_name(ptr_inner) {
                        relationships.push(Relationship::Aggregation {
                            from: owner.to_string(),
                            to: name,
                            field_name: field_name.to_string(),
                            cardinality: Cardinality::ZeroOrMore,
                        });
                    }
                }
                _ => {
                    // []Type → Composition with ZeroOrMore
                    if let Some(name) = extract_user_type_name(inner) {
                        relationships.push(Relationship::Composition {
                            owner: owner.to_string(),
                            owned: name,
                            field_name: field_name.to_string(),
                            cardinality: Cardinality::ZeroOrMore,
                        });
                    }
                }
            }
        }
        TypeInfo::Generic { base, .. } if base == "map" => {
            // Skip map types — too complex for relationship inference.
        }
        _ => {}
    }
}

/// Check if a type name looks like a user-defined type (starts with uppercase).
fn is_user_type(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_uppercase() {
        return false;
    }
    // Exclude well-known Go stdlib types that are not domain types.
    !matches!(name, "String" | "Error")
}

fn extract_user_type_name(type_info: &TypeInfo) -> Option<String> {
    match type_info {
        TypeInfo::Simple(name) if is_user_type(name) => Some(name.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Visibility
// ---------------------------------------------------------------------------

fn go_visibility(name: &str) -> Visibility {
    if name.is_empty() {
        return Visibility::Internal;
    }
    let first = name.chars().next().unwrap();
    if first.is_uppercase() {
        Visibility::Public
    } else {
        Visibility::Internal
    }
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

// ---------------------------------------------------------------------------
// TypeInfo Clone implementation (needed for multi-name field declarations)
// ---------------------------------------------------------------------------

fn clone_type_info(ti: &TypeInfo) -> TypeInfo {
    match ti {
        TypeInfo::Simple(s) => TypeInfo::Simple(s.clone()),
        TypeInfo::Generic { base, params } => TypeInfo::Generic {
            base: base.clone(),
            params: params.iter().map(clone_type_info).collect(),
        },
        TypeInfo::Optional(inner) => TypeInfo::Optional(Box::new(clone_type_info(inner))),
        TypeInfo::Collection(inner) => TypeInfo::Collection(Box::new(clone_type_info(inner))),
        TypeInfo::Tuple(items) => TypeInfo::Tuple(items.iter().map(clone_type_info).collect()),
        TypeInfo::Reference(inner) => TypeInfo::Reference(Box::new(clone_type_info(inner))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> CodeModel {
        let parser = GoParser;
        parser
            .parse_file(Path::new("test.go"), source)
            .expect("parse failed")
    }

    // -- Struct parsing --

    #[test]
    fn test_parse_struct_with_fields() {
        let model = parse(
            r#"
package main

type User struct {
    Name  string
    age   int
    Email string
}
"#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "User");
        assert!(matches!(entity.kind, EntityKind::Struct));
        assert_eq!(entity.fields.len(), 3);

        assert_eq!(entity.fields[0].name, "Name");
        assert!(matches!(entity.fields[0].visibility, Visibility::Public));

        assert_eq!(entity.fields[1].name, "age");
        assert!(matches!(entity.fields[1].visibility, Visibility::Internal));

        assert_eq!(entity.fields[2].name, "Email");
        assert!(matches!(entity.fields[2].visibility, Visibility::Public));
    }

    #[test]
    fn test_parse_struct_field_types() {
        let model = parse(
            r#"
package main

type Container struct {
    items  []Item
    ptr    *Node
    lookup map[string]Value
    name   string
}
"#,
        );

        let entity = &model.entities[0];
        assert_eq!(entity.fields.len(), 4);

        // []Item -> Collection
        assert!(matches!(
            entity.fields[0].type_info,
            TypeInfo::Collection(_)
        ));

        // *Node -> Reference (pointer)
        assert!(matches!(
            entity.fields[1].type_info,
            TypeInfo::Reference(_)
        ));

        // map[string]Value -> Generic with base "map"
        assert!(matches!(
            entity.fields[2].type_info,
            TypeInfo::Generic { ref base, .. } if base == "map"
        ));
    }

    // -- Interface parsing --

    #[test]
    fn test_parse_interface() {
        let model = parse(
            r#"
package main

type Reader interface {
    Read(p []byte) (int, error)
    Close() error
}
"#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "Reader");
        assert!(matches!(entity.kind, EntityKind::Interface));
        assert_eq!(entity.methods.len(), 2);

        let read = &entity.methods[0];
        assert_eq!(read.name, "Read");
        assert!(read.is_abstract);
        assert!(!read.is_static);
        assert!(read.return_type.is_some());

        let close = &entity.methods[1];
        assert_eq!(close.name, "Close");
        assert!(close.is_abstract);
    }

    // -- Function parsing --

    #[test]
    fn test_parse_function() {
        let model = parse(
            r#"
package main

func Add(a int, b int) int {
    return a + b
}
"#,
        );

        assert_eq!(model.functions.len(), 1);
        let func = &model.functions[0];
        assert_eq!(func.name, "Add");
        assert!(matches!(func.visibility, Visibility::Public));
        assert_eq!(func.parameters.len(), 2);
        assert_eq!(func.parameters[0].name, "a");
        assert_eq!(func.parameters[1].name, "b");
        assert!(func.return_type.is_some());
    }

    #[test]
    fn test_parse_private_function() {
        let model = parse(
            r#"
package main

func helper() {
}
"#,
        );

        assert_eq!(model.functions.len(), 1);
        assert_eq!(model.functions[0].name, "helper");
        assert!(matches!(
            model.functions[0].visibility,
            Visibility::Internal
        ));
    }

    #[test]
    fn test_parse_function_calls() {
        let model = parse(
            r#"
package main

func doStuff() {
    x := foo()
    y := bar.Baz(1, 2)
}
"#,
        );

        assert_eq!(model.functions.len(), 1);
        let calls = &model.functions[0].calls;
        assert!(calls.len() >= 2);

        let foo_call = calls.iter().find(|c| c.method == "foo").unwrap();
        assert!(foo_call.receiver.is_none());

        let baz_call = calls.iter().find(|c| c.method == "Baz").unwrap();
        assert_eq!(baz_call.receiver.as_deref(), Some("bar"));
    }

    // -- Method parsing --

    #[test]
    fn test_parse_method_declaration() {
        let model = parse(
            r#"
package main

type User struct {
    Name string
}

func (u *User) GetName() string {
    return u.Name
}

func (u User) String() string {
    return u.Name
}
"#,
        );

        let user = model.entities.iter().find(|e| e.name == "User").unwrap();
        assert_eq!(user.methods.len(), 2);

        let get_name = &user.methods[0];
        assert_eq!(get_name.name, "GetName");
        assert!(matches!(get_name.visibility, Visibility::Public));
        assert!(!get_name.is_static);
        assert!(get_name.return_type.is_some());
    }

    // -- Embedded struct (inheritance) --

    #[test]
    fn test_embedded_struct_creates_inheritance() {
        let model = parse(
            r#"
package main

type Base struct {
    ID int
}

type Derived struct {
    Base
    Name string
}
"#,
        );

        let derived = model.entities.iter().find(|e| e.name == "Derived").unwrap();
        // The embedded field should appear as a field named "Base"
        let base_field = derived.fields.iter().find(|f| f.name == "Base");
        assert!(base_field.is_some());

        // Should have an Inheritance relationship
        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Inheritance {
                    child,
                    parent,
                } if child == "Derived" && parent == "Base"
            )
        });
        assert!(rel.is_some(), "expected Inheritance relationship for embedded struct");
    }

    // -- Pointer fields (aggregation) --

    #[test]
    fn test_pointer_field_creates_aggregation() {
        let model = parse(
            r#"
package main

type Order struct {
    user *User
}
"#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Aggregation {
                    from,
                    to,
                    cardinality: Cardinality::ZeroOrOne,
                    ..
                } if from == "Order" && to == "User"
            )
        });
        assert!(rel.is_some(), "expected Aggregation for pointer field");
    }

    // -- Slice fields (composition with ZeroOrMore) --

    #[test]
    fn test_slice_field_creates_composition() {
        let model = parse(
            r#"
package main

type Team struct {
    members []User
}
"#,
        );

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
        assert!(rel.is_some(), "expected Composition with ZeroOrMore for slice field");
    }

    // -- Slice of pointers (aggregation with ZeroOrMore) --

    #[test]
    fn test_slice_of_pointers_creates_aggregation() {
        let model = parse(
            r#"
package main

type Team struct {
    members []*User
}
"#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Aggregation {
                    from,
                    to,
                    cardinality: Cardinality::ZeroOrMore,
                    ..
                } if from == "Team" && to == "User"
            )
        });
        assert!(rel.is_some(), "expected Aggregation with ZeroOrMore for []*User");
    }

    // -- Plain user type field (composition with One) --

    #[test]
    fn test_plain_user_type_creates_composition_one() {
        let model = parse(
            r#"
package main

type Order struct {
    item Item
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
                } if owner == "Order" && owned == "Item"
            )
        });
        assert!(rel.is_some(), "expected Composition with One for plain user type");
    }

    // -- Map fields are skipped --

    #[test]
    fn test_map_field_no_relationship() {
        let model = parse(
            r#"
package main

type Cache struct {
    data map[string]User
}
"#,
        );

        // Map fields should not produce relationships
        let rels: Vec<_> = model
            .relationships
            .iter()
            .filter(|r| match r {
                Relationship::Composition { field_name, .. }
                | Relationship::Aggregation { field_name, .. } => field_name == "data",
                _ => false,
            })
            .collect();
        assert!(rels.is_empty(), "map fields should not create relationships");
    }

    // -- Visibility --

    #[test]
    fn test_visibility_based_on_capitalization() {
        let model = parse(
            r#"
package main

type Config struct {
    Host     string
    port     int
    Database string
    timeout  int
}
"#,
        );

        let entity = &model.entities[0];
        assert!(matches!(entity.fields[0].visibility, Visibility::Public));
        assert!(matches!(entity.fields[1].visibility, Visibility::Internal));
        assert!(matches!(entity.fields[2].visibility, Visibility::Public));
        assert!(matches!(entity.fields[3].visibility, Visibility::Internal));
    }

    // -- Source file is set --

    #[test]
    fn test_source_file_is_set() {
        let model = parse(
            r#"
package main

type Foo struct {}

func bar() {}
"#,
        );

        assert_eq!(model.entities[0].source_file, "test.go");
        assert_eq!(model.functions[0].source_file, "test.go");
    }

    // -- Multiple return values --

    #[test]
    fn test_function_multiple_return_values() {
        let model = parse(
            r#"
package main

func Divide(a int, b int) (int, error) {
    return a / b, nil
}
"#,
        );

        let func = &model.functions[0];
        assert!(matches!(func.return_type, Some(TypeInfo::Tuple(ref items)) if items.len() == 2));
    }

    // -- Interface method visibility --

    #[test]
    fn test_interface_method_visibility() {
        let model = parse(
            r#"
package main

type Service interface {
    Start() error
    stop()
}
"#,
        );

        let entity = &model.entities[0];
        assert!(matches!(entity.methods[0].visibility, Visibility::Public));
        assert!(matches!(entity.methods[1].visibility, Visibility::Internal));
    }
}
