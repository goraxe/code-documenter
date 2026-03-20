use std::path::Path;

use anyhow::{Context, Result};
use tree_sitter::Node;

use crate::model::{
    CallExpr, Cardinality, CodeModel, Entity, EntityKind, Field, Function, Method, Parameter,
    Relationship, TypeInfo, Visibility,
};

use super::LanguageParser;

pub struct RustParser;

impl LanguageParser for RustParser {
    fn parse_file(&self, path: &Path, source: &str) -> Result<CodeModel> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .context("failed to set tree-sitter language to Rust")?;

        let tree = parser
            .parse(source, None)
            .context("tree-sitter failed to parse Rust source")?;

        let root = tree.root_node();
        let file_name = path.display().to_string();

        let mut model = CodeModel::new();

        // First pass: collect entities (structs, enums, traits) and free functions.
        collect_top_level(&root, source, &file_name, &mut model);

        // Second pass: process impl blocks to attach methods and create relationships.
        process_impl_blocks(&root, source, &file_name, &mut model);

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
            "struct_item" => {
                if let Some(entity) = parse_struct(&child, source, file) {
                    // Infer relationships from fields.
                    infer_field_relationships(&entity, &mut model.relationships);
                    model.entities.push(entity);
                }
            }
            "enum_item" => {
                if let Some(entity) = parse_enum(&child, source, file) {
                    model.entities.push(entity);
                }
            }
            "trait_item" => {
                if let Some(entity) = parse_trait(&child, source, file) {
                    model.entities.push(entity);
                }
            }
            "function_item" => {
                if let Some(func) = parse_function(&child, source, file) {
                    model.functions.push(func);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Struct parsing
// ---------------------------------------------------------------------------

fn parse_struct(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;
    let mut fields = Vec::new();

    if let Some(field_list) = node.child_by_field_name("body") {
        let mut cursor = field_list.walk();
        for child in field_list.children(&mut cursor) {
            if child.kind() == "field_declaration" {
                if let Some(field) = parse_field_declaration(&child, source) {
                    fields.push(field);
                }
            }
        }
    }

    Some(Entity {
        name,
        kind: EntityKind::Struct,
        fields,
        methods: Vec::new(),
        source_file: file.to_string(),
    })
}

fn parse_field_declaration(node: &Node, source: &str) -> Option<Field> {
    let name = child_by_field(node, "name", source)?;
    let visibility = extract_visibility(node, source);
    let type_node = node.child_by_field_name("type")?;
    let type_info = parse_type_node(&type_node, source);

    Some(Field {
        name,
        type_info,
        visibility,
    })
}

// ---------------------------------------------------------------------------
// Enum parsing
// ---------------------------------------------------------------------------

fn parse_enum(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;
    let mut fields = Vec::new();

    if let Some(variant_list) = node.child_by_field_name("body") {
        let mut cursor = variant_list.walk();
        for child in variant_list.children(&mut cursor) {
            if child.kind() == "enum_variant" {
                if let Some(variant_name) = child_by_field(&child, "name", source) {
                    fields.push(Field {
                        name: variant_name,
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
// Trait parsing
// ---------------------------------------------------------------------------

fn parse_trait(node: &Node, source: &str, file: &str) -> Option<Entity> {
    let name = child_by_field(node, "name", source)?;
    let mut methods = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_signature_item" => {
                    if let Some(method) = parse_method_signature(&child, source, true) {
                        methods.push(method);
                    }
                }
                "function_item" => {
                    if let Some(method) = parse_method_from_function(&child, source) {
                        methods.push(method);
                    }
                }
                _ => {}
            }
        }
    }

    Some(Entity {
        name,
        kind: EntityKind::Trait,
        fields: Vec::new(),
        methods,
        source_file: file.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Impl block processing
// ---------------------------------------------------------------------------

fn process_impl_blocks(root: &Node, source: &str, file: &str, model: &mut CodeModel) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "impl_item" {
            process_impl_item(&child, source, file, model);
        }
    }
}

fn process_impl_item(node: &Node, source: &str, file: &str, model: &mut CodeModel) {
    // Determine the struct/type being implemented.
    let type_node = match node.child_by_field_name("type") {
        Some(n) => n,
        None => return,
    };
    let type_name = node_text(&type_node, source);

    // Determine if this is a trait impl: `impl Trait for Type`.
    let trait_name = node
        .child_by_field_name("trait")
        .map(|n| node_text(&n, source));

    // If it's a trait impl, record a Relationship::Implementation.
    if let Some(ref trait_n) = trait_name {
        model.relationships.push(Relationship::Implementation {
            implementor: type_name.clone(),
            interface: trait_n.clone(),
        });
    }

    // Collect methods from the body.
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for child in body.children(&mut body_cursor) {
            if child.kind() == "function_item" {
                if let Some(method) = parse_method_from_function(&child, source) {
                    // Attach to the entity if it exists.
                    attach_method_to_entity(model, &type_name, method, file);
                }
            }
        }
    }
}

fn attach_method_to_entity(model: &mut CodeModel, type_name: &str, method: Method, file: &str) {
    // Try to find an existing entity with the given name.
    if let Some(entity) = model.entities.iter_mut().find(|e| e.name == type_name) {
        entity.methods.push(method);
    } else {
        // Create a new struct entity for impl blocks without a prior struct definition.
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
// Method / function parsing
// ---------------------------------------------------------------------------

fn parse_method_signature(node: &Node, source: &str, is_abstract: bool) -> Option<Method> {
    let name = child_by_field(node, "name", source)?;
    let visibility = extract_visibility(node, source);
    let parameters = parse_parameters(node, source);
    let return_type = parse_return_type(node, source);
    let is_static = !has_self_parameter(node, source);

    Some(Method {
        name,
        parameters,
        return_type,
        visibility,
        is_static,
        is_abstract,
    })
}

fn parse_method_from_function(node: &Node, source: &str) -> Option<Method> {
    let name = child_by_field(node, "name", source)?;
    let visibility = extract_visibility(node, source);
    let parameters = parse_parameters(node, source);
    let return_type = parse_return_type(node, source);
    let is_static = !has_self_parameter(node, source);

    Some(Method {
        name,
        parameters,
        return_type,
        visibility,
        is_static,
        is_abstract: false,
    })
}

fn parse_function(node: &Node, source: &str, file: &str) -> Option<Function> {
    let name = child_by_field(node, "name", source)?;
    let visibility = extract_visibility(node, source);
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

fn parse_parameters(node: &Node, source: &str) -> Vec<Parameter> {
    let mut params = Vec::new();
    if let Some(param_list) = node.child_by_field_name("parameters") {
        let mut cursor = param_list.walk();
        for child in param_list.children(&mut cursor) {
            if child.kind() == "parameter" {
                if let Some(param) = parse_single_parameter(&child, source) {
                    params.push(param);
                }
            }
            // Skip self_parameter nodes -- they are not regular parameters.
        }
    }
    params
}

fn parse_single_parameter(node: &Node, source: &str) -> Option<Parameter> {
    let name = child_by_field(node, "pattern", source)?;
    let type_node = node.child_by_field_name("type")?;
    let type_info = parse_type_node(&type_node, source);
    Some(Parameter { name, type_info })
}

fn parse_return_type(node: &Node, source: &str) -> Option<TypeInfo> {
    let ret = node.child_by_field_name("return_type")?;
    // The return_type field points to the type node directly.
    Some(parse_type_node(&ret, source))
}

fn has_self_parameter(node: &Node, source: &str) -> bool {
    if let Some(param_list) = node.child_by_field_name("parameters") {
        let mut cursor = param_list.walk();
        for child in param_list.children(&mut cursor) {
            match child.kind() {
                "self_parameter" => return true,
                _ => {
                    let text = node_text(&child, source);
                    if text == "self" || text == "&self" || text == "&mut self" {
                        return true;
                    }
                }
            }
        }
    }
    false
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

/// Returns true if `name` looks like a valid identifier for ZenUML output
/// (alphanumeric + underscores, no operators, no complex expressions).
fn is_valid_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
}

fn parse_call_expr(node: &Node, source: &str) -> Option<CallExpr> {
    let func_node = node.child_by_field_name("function")?;

    match func_node.kind() {
        "field_expression" => {
            // e.g., `receiver.method(args)`
            let receiver_node = func_node.child_by_field_name("value")?;
            let method_node = func_node.child_by_field_name("field")?;
            let receiver = node_text(&receiver_node, source);
            let method = node_text(&method_node, source);

            // Only keep if receiver is a simple identifier (not a complex expression)
            if !is_valid_identifier(&method) {
                return None;
            }
            // Simplify chained receivers like `self.field` to just the last part
            let receiver = if let Some((_prefix, last)) = receiver.rsplit_once('.') {
                last.to_string()
            } else {
                receiver
            };
            if !is_valid_identifier(&receiver) {
                return None;
            }

            Some(CallExpr {
                receiver: Some(receiver),
                method,
                arguments: extract_argument_names(node, source),
            })
        }
        "scoped_identifier" => {
            let text = node_text(&func_node, source);
            // e.g., `Foo::bar` -> receiver=Foo, method=bar
            let (recv, method) = text.rsplit_once("::")?;
            if !is_valid_identifier(recv) || !is_valid_identifier(method) {
                return None;
            }
            Some(CallExpr {
                receiver: Some(recv.to_string()),
                method: method.to_string(),
                arguments: extract_argument_names(node, source),
            })
        }
        "identifier" => {
            let name = node_text(&func_node, source);
            if !is_valid_identifier(&name) {
                return None;
            }
            Some(CallExpr {
                receiver: None,
                method: name,
                arguments: extract_argument_names(node, source),
            })
        }
        // Skip anything else (closures, complex expressions, macro-like calls)
        _ => None,
    }
}

/// Extract simplified argument names — only keep simple identifiers, skip complex expressions.
fn extract_argument_names(node: &Node, source: &str) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(arg_list) = node.child_by_field_name("arguments") {
        let mut cursor = arg_list.walk();
        for child in arg_list.children(&mut cursor) {
            match child.kind() {
                "(" | ")" | "," => {}
                "identifier" => args.push(node_text(&child, source)),
                "reference_expression" => {
                    // &foo -> just "foo"
                    if let Some(inner) = child.child_by_field_name("value") {
                        if inner.kind() == "identifier" {
                            args.push(node_text(&inner, source));
                        }
                    }
                }
                // For complex args, just use a placeholder
                _ => args.push("...".to_string()),
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
        "type_identifier" | "primitive_type" => TypeInfo::Simple(node_text(node, source)),
        "scoped_type_identifier" => TypeInfo::Simple(node_text(node, source)),
        "generic_type" => parse_generic_type(node, source),
        "reference_type" => {
            if let Some(inner) = node.child_by_field_name("type") {
                TypeInfo::Reference(Box::new(parse_type_node(&inner, source)))
            } else {
                TypeInfo::Simple(node_text(node, source))
            }
        }
        "tuple_type" => {
            let mut items = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    items.push(parse_type_node(&child, source));
                }
            }
            TypeInfo::Tuple(items)
        }
        _ => TypeInfo::Simple(node_text(node, source)),
    }
}

fn parse_generic_type(node: &Node, source: &str) -> TypeInfo {
    let base = node
        .child_by_field_name("type")
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

    match base.as_str() {
        "Option" if params.len() == 1 => TypeInfo::Optional(Box::new(params.remove(0))),
        "Vec" if params.len() == 1 => TypeInfo::Collection(Box::new(params.remove(0))),
        _ => TypeInfo::Generic { base, params },
    }
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
                    cardinality: Cardinality::OneOrMore,
                });
            }
        }
        TypeInfo::Reference(inner) => {
            if let Some(name) = extract_user_type_name(inner) {
                relationships.push(Relationship::Aggregation {
                    from: owner.to_string(),
                    to: name,
                    field_name: field_name.to_string(),
                    cardinality: Cardinality::One,
                });
            }
        }
        TypeInfo::Generic { base, params } => {
            // Arc<T>, Rc<T> -> Aggregation
            // Box<T> -> Composition (owned)
            match base.as_str() {
                "Arc" | "Rc" => {
                    if let Some(inner) = params.first() {
                        if let Some(name) = extract_user_type_name(inner) {
                            relationships.push(Relationship::Aggregation {
                                from: owner.to_string(),
                                to: name,
                                field_name: field_name.to_string(),
                                cardinality: Cardinality::One,
                            });
                        }
                    }
                }
                "Box" => {
                    if let Some(inner) = params.first() {
                        if let Some(name) = extract_user_type_name(inner) {
                            relationships.push(Relationship::Composition {
                                owner: owner.to_string(),
                                owned: name,
                                field_name: field_name.to_string(),
                                cardinality: Cardinality::One,
                            });
                        }
                    }
                }
                _ => {
                    // For other generics, look at params for user types.
                    for param in params {
                        infer_from_type(owner, field_name, param, relationships);
                    }
                }
            }
        }
        TypeInfo::Tuple(items) => {
            for item in items {
                infer_from_type(owner, field_name, item, relationships);
            }
        }
    }
}

/// Check if a type name looks like a user-defined type (starts with uppercase,
/// and is not a well-known primitive).
fn is_user_type(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_uppercase() {
        return false;
    }
    // Exclude well-known stdlib types that are not domain types.
    !matches!(
        name,
        "String"
            | "PathBuf"
            | "OsString"
            | "CString"
            | "bool"
            | "char"
            | "Self"
            | "Result"
            | "Error"
    )
}

fn extract_user_type_name(type_info: &TypeInfo) -> Option<String> {
    match type_info {
        TypeInfo::Simple(name) if is_user_type(name) => Some(name.clone()),
        _ => None,
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

fn extract_visibility(node: &Node, source: &str) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(&child, source);
            if text.starts_with("pub") {
                return Visibility::Public;
            }
        }
    }
    Visibility::Private
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> CodeModel {
        let parser = RustParser;
        parser
            .parse_file(Path::new("test.rs"), source)
            .expect("parse failed")
    }

    // -- Struct parsing --

    #[test]
    fn test_parse_struct_with_fields() {
        let model = parse(
            r#"
            pub struct User {
                pub name: String,
                age: u32,
                pub email: Option<String>,
            }
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "User");
        assert!(matches!(entity.kind, EntityKind::Struct));
        assert_eq!(entity.fields.len(), 3);

        assert_eq!(entity.fields[0].name, "name");
        assert!(matches!(entity.fields[0].visibility, Visibility::Public));
        assert!(matches!(entity.fields[0].type_info, TypeInfo::Simple(ref s) if s == "String"));

        assert_eq!(entity.fields[1].name, "age");
        assert!(matches!(entity.fields[1].visibility, Visibility::Private));

        assert_eq!(entity.fields[2].name, "email");
        assert!(matches!(entity.fields[2].type_info, TypeInfo::Optional(_)));
    }

    #[test]
    fn test_parse_struct_vec_field() {
        let model = parse(
            r#"
            struct Team {
                members: Vec<User>,
            }
            "#,
        );

        let entity = &model.entities[0];
        assert_eq!(entity.fields[0].name, "members");
        assert!(matches!(
            entity.fields[0].type_info,
            TypeInfo::Collection(_)
        ));
    }

    #[test]
    fn test_parse_struct_reference_field() {
        let model = parse(
            r#"
            struct Borrowed {
                data: &User,
            }
            "#,
        );

        let entity = &model.entities[0];
        assert!(matches!(entity.fields[0].type_info, TypeInfo::Reference(_)));
    }

    // -- Enum parsing --

    #[test]
    fn test_parse_enum_with_variants() {
        let model = parse(
            r#"
            pub enum Color {
                Red,
                Green,
                Blue,
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

    // -- Trait parsing --

    #[test]
    fn test_parse_trait_with_methods() {
        let model = parse(
            r#"
            pub trait Drawable {
                fn draw(&self);
                fn resize(&mut self, width: u32, height: u32) -> bool;
            }
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "Drawable");
        assert!(matches!(entity.kind, EntityKind::Trait));
        assert_eq!(entity.methods.len(), 2);

        let draw = &entity.methods[0];
        assert_eq!(draw.name, "draw");
        assert!(!draw.is_static);
        assert!(draw.is_abstract);
        assert!(draw.return_type.is_none());

        let resize = &entity.methods[1];
        assert_eq!(resize.name, "resize");
        assert!(!resize.is_static);
        assert_eq!(resize.parameters.len(), 2);
        assert!(resize.return_type.is_some());
    }

    // -- Impl block --

    #[test]
    fn test_parse_impl_block_attaches_methods() {
        let model = parse(
            r#"
            struct Foo {
                x: i32,
            }

            impl Foo {
                pub fn new(x: i32) -> Foo {
                    Foo { x }
                }

                pub fn get_x(&self) -> i32 {
                    self.x
                }
            }
            "#,
        );

        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "Foo");
        assert_eq!(entity.methods.len(), 2);

        let new_method = &entity.methods[0];
        assert_eq!(new_method.name, "new");
        assert!(new_method.is_static);
        assert!(matches!(new_method.visibility, Visibility::Public));

        let get_x = &entity.methods[1];
        assert_eq!(get_x.name, "get_x");
        assert!(!get_x.is_static);
    }

    // -- Impl Trait for Struct --

    #[test]
    fn test_parse_impl_trait_for_struct() {
        let model = parse(
            r#"
            struct Circle {
                radius: f64,
            }

            trait Shape {
                fn area(&self) -> f64;
            }

            impl Shape for Circle {
                fn area(&self) -> f64 {
                    3.14 * self.radius * self.radius
                }
            }
            "#,
        );

        // Should have Circle and Shape entities.
        assert_eq!(model.entities.len(), 2);

        // Circle should have the area method attached.
        let circle = model.entities.iter().find(|e| e.name == "Circle").unwrap();
        assert_eq!(circle.methods.len(), 1);
        assert_eq!(circle.methods[0].name, "area");

        // Should have an Implementation relationship.
        let impl_rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Implementation {
                    implementor,
                    interface,
                } if implementor == "Circle" && interface == "Shape"
            )
        });
        assert!(impl_rel.is_some());
    }

    // -- Free functions --

    #[test]
    fn test_parse_free_function() {
        let model = parse(
            r#"
            pub fn add(a: i32, b: i32) -> i32 {
                a + b
            }
            "#,
        );

        assert_eq!(model.functions.len(), 1);
        let func = &model.functions[0];
        assert_eq!(func.name, "add");
        assert!(matches!(func.visibility, Visibility::Public));
        assert_eq!(func.parameters.len(), 2);
        assert_eq!(func.parameters[0].name, "a");
        assert_eq!(func.parameters[1].name, "b");
        assert!(func.return_type.is_some());
    }

    // -- Call expressions --

    #[test]
    fn test_parse_function_calls() {
        let model = parse(
            r#"
            fn do_stuff() {
                let x = foo();
                let y = bar.baz(1, 2);
                let z = Qux::create();
            }
            "#,
        );

        assert_eq!(model.functions.len(), 1);
        let calls = &model.functions[0].calls;
        assert!(calls.len() >= 3);

        let foo_call = calls.iter().find(|c| c.method == "foo").unwrap();
        assert!(foo_call.receiver.is_none());

        let baz_call = calls.iter().find(|c| c.method == "baz").unwrap();
        assert_eq!(baz_call.receiver.as_deref(), Some("bar"));

        let create_call = calls.iter().find(|c| c.method == "create").unwrap();
        assert_eq!(create_call.receiver.as_deref(), Some("Qux"));
    }

    // -- Relationship inference --

    #[test]
    fn test_composition_from_direct_field() {
        let model = parse(
            r#"
            struct Order {
                user: User,
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

    #[test]
    fn test_composition_zero_or_one_from_option() {
        let model = parse(
            r#"
            struct Order {
                user: Option<User>,
            }
            "#,
        );

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
    fn test_composition_one_or_more_from_vec() {
        let model = parse(
            r#"
            struct Order {
                items: Vec<Item>,
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Composition {
                    owner,
                    owned,
                    cardinality: Cardinality::OneOrMore,
                    ..
                } if owner == "Order" && owned == "Item"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_aggregation_from_reference() {
        let model = parse(
            r#"
            struct Borrowed {
                data: &User,
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Aggregation {
                    from,
                    to,
                    cardinality: Cardinality::One,
                    ..
                } if from == "Borrowed" && to == "User"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_aggregation_from_arc() {
        let model = parse(
            r#"
            struct Shared {
                data: Arc<User>,
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Aggregation {
                    from,
                    to,
                    cardinality: Cardinality::One,
                    ..
                } if from == "Shared" && to == "User"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_composition_from_box() {
        let model = parse(
            r#"
            struct Container {
                inner: Box<Widget>,
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
                } if owner == "Container" && owned == "Widget"
            )
        });
        assert!(rel.is_some());
    }

    #[test]
    fn test_parse_generic_type_field() {
        let model = parse(
            r#"
            struct Config {
                settings: HashMap<String, Value>,
            }
            "#,
        );

        let entity = &model.entities[0];
        let field = &entity.fields[0];
        assert!(matches!(
            &field.type_info,
            TypeInfo::Generic { base, params } if base == "HashMap" && params.len() == 2
        ));
    }

    #[test]
    fn test_no_relationship_for_primitive_types() {
        let model = parse(
            r#"
            struct Simple {
                count: i32,
                name: String,
            }
            "#,
        );

        assert!(model.relationships.is_empty());
    }

    #[test]
    fn test_parse_tuple_type() {
        let model = parse(
            r#"
            struct Pair {
                coords: (i32, i32),
            }
            "#,
        );

        let field = &model.entities[0].fields[0];
        assert!(matches!(&field.type_info, TypeInfo::Tuple(items) if items.len() == 2));
    }

    #[test]
    fn test_source_file_is_set() {
        let model = parse(
            r#"
            struct Foo {}
            fn bar() {}
            "#,
        );

        assert_eq!(model.entities[0].source_file, "test.rs");
        assert_eq!(model.functions[0].source_file, "test.rs");
    }

    #[test]
    fn test_trait_static_method() {
        let model = parse(
            r#"
            trait Factory {
                fn create() -> Self;
            }
            "#,
        );

        let entity = &model.entities[0];
        assert_eq!(entity.methods.len(), 1);
        assert!(entity.methods[0].is_static);
    }

    #[test]
    fn test_aggregation_from_rc() {
        let model = parse(
            r#"
            struct Shared {
                data: Rc<User>,
            }
            "#,
        );

        let rel = model.relationships.iter().find(|r| {
            matches!(
                r,
                Relationship::Aggregation {
                    from,
                    to,
                    cardinality: Cardinality::One,
                    ..
                } if from == "Shared" && to == "User"
            )
        });
        assert!(rel.is_some());
    }
}
