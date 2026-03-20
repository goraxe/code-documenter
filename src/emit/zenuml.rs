use crate::model::{CodeModel, Function};

use super::DiagramEmitter;

pub struct ZenumlEmitter;

impl ZenumlEmitter {
    fn emit_function(output: &mut String, func: &Function, all_functions: &[Function]) {
        if func.calls.is_empty() {
            return;
        }

        for call in &func.calls {
            let args_str = call.arguments.join(", ");
            let call_str = match &call.receiver {
                Some(receiver) => format!("{}.{}({})", receiver, call.method, args_str),
                None => format!("{}({})", call.method, args_str),
            };

            // Check if the called method maps to a known function with calls
            let nested_func = all_functions.iter().find(|f| {
                f.name == call.method && !f.calls.is_empty()
            });

            if let Some(nested) = nested_func {
                output.push_str(&format!("    {} {{\n", call_str));
                for nested_call in &nested.calls {
                    let nested_args = nested_call.arguments.join(", ");
                    let nested_call_str = match &nested_call.receiver {
                        Some(receiver) => {
                            format!("        {}.{}({})", receiver, nested_call.method, nested_args)
                        }
                        None => format!("        {}({})", nested_call.method, nested_args),
                    };
                    output.push_str(&nested_call_str);
                    output.push('\n');
                }
                output.push_str("    }\n");
            } else {
                output.push_str(&format!("    {}\n", call_str));
            }
        }
    }
}

impl DiagramEmitter for ZenumlEmitter {
    fn emit(&self, model: &CodeModel) -> String {
        let mut output = String::from("zenuml\n");

        for func in &model.functions {
            Self::emit_function(&mut output, func, &model.functions);
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
        let emitter = ZenumlEmitter;
        let model = CodeModel::new();
        assert_eq!(emitter.emit(&model), "zenuml\n");
    }

    #[test]
    fn test_emit_function_with_calls() {
        let emitter = ZenumlEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "main".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![
                    CallExpr {
                        receiver: None,
                        method: "initialize".to_string(),
                        arguments: vec![],
                    },
                    CallExpr {
                        receiver: None,
                        method: "run".to_string(),
                        arguments: vec!["config".to_string()],
                    },
                ],
                source_file: "main.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model);
        assert!(result.starts_with("zenuml\n"));
        assert!(result.contains("initialize()"));
        assert!(result.contains("run(config)"));
    }

    #[test]
    fn test_emit_function_with_receiver_calls() {
        let emitter = ZenumlEmitter;
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
                        arguments: vec!["sql".to_string(), "params".to_string()],
                    },
                ],
                source_file: "process.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model);
        assert!(result.contains("db.connect()"));
        assert!(result.contains("db.query(sql, params)"));
    }

    #[test]
    fn test_emit_function_with_no_calls_produces_nothing() {
        let emitter = ZenumlEmitter;
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

        let result = emitter.emit(&model);
        assert_eq!(result, "zenuml\n");
    }

    #[test]
    fn test_emit_nested_calls() {
        let emitter = ZenumlEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![
                Function {
                    name: "main".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![CallExpr {
                        receiver: None,
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

        let result = emitter.emit(&model);
        assert!(result.contains("setup() {"));
        assert!(result.contains("Config.load(path)"));
        assert!(result.contains("}"));
    }
}
