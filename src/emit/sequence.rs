use std::collections::{HashMap, HashSet};

use crate::model::{CodeModel, Function};

use super::{DiagramEmitter, MermaidTheme};

/// Mermaid sequenceDiagram reserved keywords that cannot be used as bare participant names.
const RESERVED_KEYWORDS: &[&str] = &[
    "box", "end", "loop", "alt", "else", "opt", "par", "and", "rect", "note",
    "activate", "deactivate", "participant", "actor", "critical", "break",
    "over", "left", "right", "of", "as", "autonumber", "title",
];

pub struct SequenceEmitter;

impl SequenceEmitter {
    /// Returns a safe participant ID. If the name is a reserved keyword, prefix with `p_`.
    fn safe_id(name: &str) -> String {
        let lower = name.to_lowercase();
        if RESERVED_KEYWORDS.contains(&lower.as_str()) {
            format!("p_{}", name)
        } else {
            name.to_string()
        }
    }

    /// Format a call as a message label: `method(arg1, arg2)`
    fn format_message(call: &crate::model::CallExpr) -> String {
        let args_str = call
            .arguments
            .iter()
            .filter(|a| *a != "...")
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}({})", call.method, args_str)
    }

    /// Build the set of names that are "known" in the codebase — entities and functions.
    /// Only calls targeting these names produce meaningful sequence interactions.
    fn build_known_names(model: &CodeModel) -> HashSet<String> {
        let mut known = HashSet::new();
        for entity in &model.entities {
            known.insert(entity.name.clone());
        }
        for func in &model.functions {
            known.insert(func.name.clone());
        }
        known
    }

    /// Emit sequence messages for a function's calls.
    /// Only emits calls where the target is a known entity or function.
    fn emit_calls(
        output: &mut String,
        caller_id: &str,
        func: &Function,
        all_functions: &[Function],
        visited: &mut HashSet<String>,
        id_map: &HashMap<String, String>,
        known: &HashSet<String>,
    ) {
        for call in &func.calls {
            let target_name = call.receiver.as_deref().unwrap_or(&call.method);

            // Only emit calls to known entities/functions in the codebase
            if !known.contains(target_name) && !known.contains(&call.method) {
                continue;
            }

            let target_id = id_map
                .get(target_name)
                .cloned()
                .unwrap_or_else(|| Self::safe_id(target_name));
            let message = Self::format_message(call);

            output.push_str(&format!("    {}->>{}:{}\n", caller_id, target_id, message));

            // If the called method resolves to a known function, recurse into it
            if !visited.contains(&call.method) {
                if let Some(nested) = all_functions
                    .iter()
                    .find(|f| f.name == call.method && !f.calls.is_empty())
                {
                    visited.insert(call.method.clone());
                    output.push_str(&format!("    activate {}\n", target_id));
                    Self::emit_calls(
                        output,
                        &target_id,
                        nested,
                        all_functions,
                        visited,
                        id_map,
                        known,
                    );
                    output.push_str(&format!("    deactivate {}\n", target_id));
                }
            }
        }
    }
}

impl DiagramEmitter for SequenceEmitter {
    fn emit(&self, model: &CodeModel, theme: &MermaidTheme) -> String {
        let mut output = theme.directive();
        output.push_str("sequenceDiagram\n");

        let known = Self::build_known_names(model);

        // Only emit public/internal functions that have calls to known targets
        let interesting: Vec<&Function> = model
            .functions
            .iter()
            .filter(|f| {
                if f.calls.is_empty() {
                    return false;
                }
                if !matches!(
                    f.visibility,
                    crate::model::Visibility::Public | crate::model::Visibility::Internal
                ) {
                    return false;
                }
                // Must have at least one call to a known entity/function
                f.calls.iter().any(|c| {
                    let target = c.receiver.as_deref().unwrap_or(&c.method);
                    known.contains(target) || known.contains(&c.method)
                })
            })
            .collect();

        if interesting.is_empty() {
            return output;
        }

        // Collect participants: only known targets that actually appear in calls
        let mut participants = Vec::new();
        let mut seen = HashSet::new();
        for func in &interesting {
            if seen.insert(func.name.clone()) {
                participants.push(func.name.clone());
            }
            for call in &func.calls {
                let target = call.receiver.as_deref().unwrap_or(&call.method);
                if (known.contains(target) || known.contains(&call.method))
                    && seen.insert(target.to_string())
                {
                    participants.push(target.to_string());
                }
            }
        }

        // Build ID map and emit participant declarations
        let mut id_map = HashMap::new();
        for p in &participants {
            let safe = Self::safe_id(p);
            if safe != *p {
                output.push_str(&format!("    participant {} as {}\n", safe, p));
            } else {
                output.push_str(&format!("    participant {}\n", p));
            }
            id_map.insert(p.clone(), safe);
        }

        // Emit each top-level function's call sequence
        for func in &interesting {
            let mut visited = HashSet::new();
            visited.insert(func.name.clone());

            let func_id = id_map
                .get(&func.name)
                .cloned()
                .unwrap_or_else(|| func.name.clone());
            output.push_str(&format!("    Note over {}: {}\n", func_id, func.name));
            Self::emit_calls(
                &mut output,
                &func_id,
                func,
                &model.functions,
                &mut visited,
                &id_map,
                &known,
            );
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        CallExpr, Entity, EntityKind, Function, Visibility,
    };

    #[test]
    fn test_emit_empty_model() {
        let emitter = SequenceEmitter;
        let model = CodeModel::new();
        assert_eq!(
            emitter.emit(&model, &MermaidTheme::Default),
            "sequenceDiagram\n"
        );
    }

    #[test]
    fn test_only_emits_calls_to_known_entities() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "Database".to_string(),
                kind: EntityKind::Struct,
                fields: vec![],
                methods: vec![],
                source_file: "db.rs".to_string(),
            }],
            functions: vec![Function {
                name: "run".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![
                    // Known entity — should appear
                    CallExpr {
                        receiver: Some("Database".to_string()),
                        method: "connect".to_string(),
                        arguments: vec![],
                    },
                    // Unknown local variable — should be filtered
                    CallExpr {
                        receiver: Some("path".to_string()),
                        method: "is_file".to_string(),
                        arguments: vec![],
                    },
                    // Unknown local — should be filtered
                    CallExpr {
                        receiver: Some("merged".to_string()),
                        method: "merge".to_string(),
                        arguments: vec![],
                    },
                ],
                source_file: "main.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("run->>Database:connect()"));
        assert!(!result.contains("path"));
        assert!(!result.contains("merged"));
    }

    #[test]
    fn test_emits_calls_to_known_functions() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![
                Function {
                    name: "run".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![
                        CallExpr {
                            receiver: None,
                            method: "parse_file".to_string(),
                            arguments: vec![],
                        },
                        // Unknown — filtered
                        CallExpr {
                            receiver: Some("e".to_string()),
                            method: "ok".to_string(),
                            arguments: vec![],
                        },
                    ],
                    source_file: "main.rs".to_string(),
                },
                Function {
                    name: "parse_file".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![],
                    source_file: "parse.rs".to_string(),
                },
            ],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("run->>parse_file:parse_file()"));
        assert!(!result.contains(">>e:"));
    }

    #[test]
    fn test_nested_calls_with_activate() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![
                Entity {
                    name: "App".to_string(),
                    kind: EntityKind::Struct,
                    fields: vec![],
                    methods: vec![],
                    source_file: "app.rs".to_string(),
                },
                Entity {
                    name: "Config".to_string(),
                    kind: EntityKind::Struct,
                    fields: vec![],
                    methods: vec![],
                    source_file: "config.rs".to_string(),
                },
            ],
            functions: vec![
                Function {
                    name: "main".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![CallExpr {
                        receiver: Some("App".to_string()),
                        method: "setup".to_string(),
                        arguments: vec![],
                    }],
                    source_file: "main.rs".to_string(),
                },
                Function {
                    name: "setup".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![CallExpr {
                        receiver: Some("Config".to_string()),
                        method: "load".to_string(),
                        arguments: vec!["path".to_string()],
                    }],
                    source_file: "setup.rs".to_string(),
                },
            ],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("main->>App:setup()"));
        assert!(result.contains("activate App"));
        assert!(result.contains("App->>Config:load(path)"));
        assert!(result.contains("deactivate App"));
    }

    #[test]
    fn test_no_known_targets_produces_empty() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "run".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![CallExpr {
                    receiver: Some("path".to_string()),
                    method: "is_file".to_string(),
                    arguments: vec![],
                }],
                source_file: "main.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        // "run" is known (it's a function), but "path" is not — the call is filtered.
        // However "run" itself has no *emittable* calls so it shouldn't appear either.
        assert_eq!(result, "sequenceDiagram\n");
    }

    #[test]
    fn test_reserved_keyword_escaped() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![Entity {
                name: "end".to_string(),
                kind: EntityKind::Struct,
                fields: vec![],
                methods: vec![],
                source_file: "end.rs".to_string(),
            }],
            functions: vec![Function {
                name: "run".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![CallExpr {
                    receiver: Some("end".to_string()),
                    method: "finish".to_string(),
                    arguments: vec![],
                }],
                source_file: "main.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("participant p_end as end"));
        assert!(result.contains("run->>p_end:finish()"));
    }
}
