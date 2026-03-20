use std::collections::{HashMap, HashSet};

use crate::model::{CodeModel, Function};

use super::{DiagramEmitter, MermaidTheme};

/// Standard library / wrapper types that produce noise rather than meaningful interactions.
/// These are filtered out of sequence diagram output.
const NOISE_TYPES: &[&str] = &[
    "Box", "Some", "None", "Ok", "Err", "Vec", "String", "Arc", "Rc", "Option",
    "Result", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "WalkDir",
    "PathBuf", "Path", "self",
];

/// Mermaid sequenceDiagram reserved keywords that cannot be used as bare participant names.
const RESERVED_KEYWORDS: &[&str] = &[
    "box", "end", "loop", "alt", "else", "opt", "par", "and", "rect", "note",
    "activate", "deactivate", "participant", "actor", "critical", "break",
    "over", "left", "right", "of", "as", "autonumber", "title",
];

pub struct SequenceEmitter;

impl SequenceEmitter {
    /// Returns a safe participant ID for use in arrows/activate/deactivate.
    /// If the name is a reserved keyword, prefix it with `p_`.
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

    /// Returns true if a call target is a noise type that should be skipped.
    fn is_noise(name: &str) -> bool {
        NOISE_TYPES.contains(&name)
    }

    /// Emit sequence messages for a function's calls.
    /// `caller` is the safe participant ID of the calling context.
    fn emit_calls(
        output: &mut String,
        caller_id: &str,
        func: &Function,
        all_functions: &[Function],
        visited: &mut HashSet<String>,
        id_map: &HashMap<String, String>,
    ) {
        for call in &func.calls {
            let target_name = call
                .receiver
                .as_deref()
                .unwrap_or(&call.method);

            // Skip calls to noise types (Box::new, Vec::new, Some(), etc.)
            if Self::is_noise(target_name) {
                continue;
            }

            let target_id = id_map
                .get(target_name)
                .cloned()
                .unwrap_or_else(|| Self::safe_id(target_name));
            let message = Self::format_message(call);

            output.push_str(&format!("    {}->>{}:{}\n", caller_id, target_id, message));

            if !visited.contains(&call.method) {
                if let Some(nested) = all_functions
                    .iter()
                    .find(|f| f.name == call.method && !f.calls.is_empty())
                {
                    visited.insert(call.method.clone());
                    output.push_str(&format!("    activate {}\n", target_id));
                    Self::emit_calls(output, &target_id, nested, all_functions, visited, id_map);
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

        // Only emit public/internal functions that have calls
        let interesting: Vec<&Function> = model
            .functions
            .iter()
            .filter(|f| {
                !f.calls.is_empty()
                    && matches!(
                        f.visibility,
                        crate::model::Visibility::Public | crate::model::Visibility::Internal
                    )
            })
            .collect();

        if interesting.is_empty() {
            return output;
        }

        // Collect all unique participants, skipping noise types
        let mut participants = Vec::new();
        let mut seen = HashSet::new();
        for func in &interesting {
            if seen.insert(func.name.clone()) {
                participants.push(func.name.clone());
            }
            for call in &func.calls {
                let target = call.receiver.as_deref().unwrap_or(&call.method);
                if !Self::is_noise(target) && seen.insert(target.to_string()) {
                    participants.push(target.to_string());
                }
            }
        }

        // Build ID map: original name -> safe mermaid ID
        let mut id_map = HashMap::new();
        for p in &participants {
            let safe = Self::safe_id(p);
            if &safe != p {
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

            let func_id = id_map.get(&func.name).cloned().unwrap_or_else(|| func.name.clone());
            output.push_str(&format!("    Note over {}: {}\n", func_id, func.name));
            Self::emit_calls(&mut output, &func_id, func, &model.functions, &mut visited, &id_map);
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CallExpr, Function, Visibility};

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
    fn test_emit_function_with_receiver_calls() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "process".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![
                    CallExpr {
                        receiver: Some("db".to_string()),
                        method: "connect".to_string(),
                        arguments: vec![],
                    },
                    CallExpr {
                        receiver: Some("db".to_string()),
                        method: "query".to_string(),
                        arguments: vec!["sql".to_string()],
                    },
                ],
                source_file: "process.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.starts_with("sequenceDiagram\n"));
        assert!(result.contains("participant process"));
        assert!(result.contains("participant db"));
        assert!(result.contains("process->>db:connect()"));
        assert!(result.contains("process->>db:query(sql)"));
    }

    #[test]
    fn test_emit_function_with_no_calls_produces_nothing() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "noop".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![],
                source_file: "noop.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert_eq!(result, "sequenceDiagram\n");
    }

    #[test]
    fn test_emit_nested_calls_with_activate() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
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
    fn test_free_function_call_uses_method_as_target() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "main".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![CallExpr {
                    receiver: None,
                    method: "initialize".to_string(),
                    arguments: vec![],
                }],
                source_file: "main.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("main->>initialize:initialize()"));
    }
}
