/// The complete parsed representation of a codebase.
pub struct CodeModel {
    pub entities: Vec<Entity>,
    pub functions: Vec<Function>,
    pub relationships: Vec<Relationship>,
}

pub struct Entity {
    pub name: String,
    pub kind: EntityKind,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub source_file: String,
}

pub enum EntityKind {
    Struct,
    Enum,
    Interface,
    Class,
    Trait,
    TypeAlias,
}

pub struct Field {
    pub name: String,
    pub type_info: TypeInfo,
    pub visibility: Visibility,
}

pub enum TypeInfo {
    Simple(String),
    Generic { base: String, params: Vec<TypeInfo> },
    Optional(Box<TypeInfo>),
    Collection(Box<TypeInfo>),
    Tuple(Vec<TypeInfo>),
    Reference(Box<TypeInfo>),
}

pub struct Method {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<TypeInfo>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
}

pub struct Parameter {
    pub name: String,
    pub type_info: TypeInfo,
}

pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<TypeInfo>,
    pub visibility: Visibility,
    pub calls: Vec<CallExpr>,
    pub source_file: String,
}

pub struct CallExpr {
    pub receiver: Option<String>,
    pub method: String,
    pub arguments: Vec<String>,
}

pub enum Relationship {
    Inheritance {
        child: String,
        parent: String,
    },
    Implementation {
        implementor: String,
        interface: String,
    },
    Composition {
        owner: String,
        owned: String,
        field_name: String,
        cardinality: Cardinality,
    },
    Aggregation {
        from: String,
        to: String,
        field_name: String,
        cardinality: Cardinality,
    },
    Association {
        from: String,
        to: String,
        label: String,
    },
}

pub enum Cardinality {
    One,
    ZeroOrOne,
    OneOrMore,
    ZeroOrMore,
}

impl CodeModel {
    /// Creates an empty CodeModel.
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            functions: Vec::new(),
            relationships: Vec::new(),
        }
    }

    /// Merges entities, functions, and relationships from another model into this one.
    pub fn merge(&mut self, other: CodeModel) {
        self.entities.extend(other.entities);
        self.functions.extend(other.functions);
        self.relationships.extend(other.relationships);
    }
}

impl Default for CodeModel {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeInfo {
    /// Returns a human-readable type name suitable for Mermaid output.
    ///
    /// Examples:
    /// - `Simple("User")` -> `"User"`
    /// - `Collection(Simple("User"))` -> `"List~User~"`
    /// - `Optional(Simple("User"))` -> `"User"`
    /// - `Generic{base: "HashMap", params: [Simple("String"), Simple("User")]}` -> `"HashMap~String, User~"`
    /// - `Tuple(vec![Simple("i32"), Simple("String")])` -> `"(i32, String)"`
    /// - `Reference(Simple("User"))` -> `"User"`
    pub fn display_name(&self) -> String {
        match self {
            TypeInfo::Simple(name) => name.clone(),
            TypeInfo::Generic { base, params } => {
                let param_names: Vec<String> = params.iter().map(|p| p.display_name()).collect();
                format!("{}~{}~", base, param_names.join(", "))
            }
            TypeInfo::Optional(inner) => inner.display_name(),
            TypeInfo::Collection(inner) => {
                format!("List~{}~", inner.display_name())
            }
            TypeInfo::Tuple(items) => {
                let item_names: Vec<String> = items.iter().map(|i| i.display_name()).collect();
                format!("({})", item_names.join(", "))
            }
            TypeInfo::Reference(inner) => inner.display_name(),
        }
    }
}

impl Visibility {
    /// Returns the Mermaid class diagram visibility prefix.
    ///
    /// - Public -> `"+"`
    /// - Private -> `"-"`
    /// - Protected -> `"#"`
    /// - Internal -> `"~"`
    pub fn mermaid_prefix(&self) -> &str {
        match self {
            Visibility::Public => "+",
            Visibility::Private => "-",
            Visibility::Protected => "#",
            Visibility::Internal => "~",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_model_new_is_empty() {
        let model = CodeModel::new();
        assert!(model.entities.is_empty());
        assert!(model.functions.is_empty());
        assert!(model.relationships.is_empty());
    }

    #[test]
    fn test_code_model_merge() {
        let mut model_a = CodeModel::new();
        model_a.entities.push(Entity {
            name: "Foo".to_string(),
            kind: EntityKind::Struct,
            fields: vec![],
            methods: vec![],
            source_file: "a.rs".to_string(),
        });

        let mut model_b = CodeModel::new();
        model_b.entities.push(Entity {
            name: "Bar".to_string(),
            kind: EntityKind::Struct,
            fields: vec![],
            methods: vec![],
            source_file: "b.rs".to_string(),
        });
        model_b.functions.push(Function {
            name: "baz".to_string(),
            parameters: vec![],
            return_type: None,
            visibility: Visibility::Public,
            calls: vec![],
            source_file: "b.rs".to_string(),
        });

        model_a.merge(model_b);
        assert_eq!(model_a.entities.len(), 2);
        assert_eq!(model_a.functions.len(), 1);
    }

    #[test]
    fn test_type_info_display_name_simple() {
        let t = TypeInfo::Simple("User".to_string());
        assert_eq!(t.display_name(), "User");
    }

    #[test]
    fn test_type_info_display_name_collection() {
        let t = TypeInfo::Collection(Box::new(TypeInfo::Simple("User".to_string())));
        assert_eq!(t.display_name(), "List~User~");
    }

    #[test]
    fn test_type_info_display_name_optional() {
        let t = TypeInfo::Optional(Box::new(TypeInfo::Simple("User".to_string())));
        assert_eq!(t.display_name(), "User");
    }

    #[test]
    fn test_type_info_display_name_generic() {
        let t = TypeInfo::Generic {
            base: "HashMap".to_string(),
            params: vec![
                TypeInfo::Simple("String".to_string()),
                TypeInfo::Simple("User".to_string()),
            ],
        };
        assert_eq!(t.display_name(), "HashMap~String, User~");
    }

    #[test]
    fn test_type_info_display_name_tuple() {
        let t = TypeInfo::Tuple(vec![
            TypeInfo::Simple("i32".to_string()),
            TypeInfo::Simple("String".to_string()),
        ]);
        assert_eq!(t.display_name(), "(i32, String)");
    }

    #[test]
    fn test_type_info_display_name_reference() {
        let t = TypeInfo::Reference(Box::new(TypeInfo::Simple("User".to_string())));
        assert_eq!(t.display_name(), "User");
    }

    #[test]
    fn test_visibility_mermaid_prefix() {
        assert_eq!(Visibility::Public.mermaid_prefix(), "+");
        assert_eq!(Visibility::Private.mermaid_prefix(), "-");
        assert_eq!(Visibility::Protected.mermaid_prefix(), "#");
        assert_eq!(Visibility::Internal.mermaid_prefix(), "~");
    }
}
