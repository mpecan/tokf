use tokf_common::config::types::{JsonConfig, JsonExtractRule, JsonFieldExtract};

use super::*;

// --- json_value_to_string ---

#[test]
fn string_value_unquoted() {
    let v = serde_json::json!("hello");
    assert_eq!(json_value_to_string(&v), "hello");
}

#[test]
fn number_value() {
    assert_eq!(json_value_to_string(&serde_json::json!(42)), "42");
    assert_eq!(json_value_to_string(&serde_json::json!(2.71)), "2.71");
}

#[test]
fn bool_value() {
    assert_eq!(json_value_to_string(&serde_json::json!(true)), "true");
    assert_eq!(json_value_to_string(&serde_json::json!(false)), "false");
}

#[test]
fn null_value() {
    assert_eq!(json_value_to_string(&serde_json::json!(null)), "null");
}

#[test]
fn object_value_as_compact_json() {
    let v = serde_json::json!({"a": 1});
    assert_eq!(json_value_to_string(&v), r#"{"a":1}"#);
}

// --- extract_dot_path ---

#[test]
fn simple_dot_path() {
    let v = serde_json::json!({"name": "pod-1"});
    assert_eq!(extract_dot_path(&v, "name"), Some("pod-1".to_string()));
}

#[test]
fn nested_dot_path() {
    let v = serde_json::json!({"metadata": {"name": "my-pod", "namespace": "default"}});
    assert_eq!(
        extract_dot_path(&v, "metadata.name"),
        Some("my-pod".to_string())
    );
}

#[test]
fn deeply_nested_dot_path() {
    let v = serde_json::json!({"a": {"b": {"c": "deep"}}});
    assert_eq!(extract_dot_path(&v, "a.b.c"), Some("deep".to_string()));
}

#[test]
fn missing_dot_path_returns_none() {
    let v = serde_json::json!({"name": "pod-1"});
    assert_eq!(extract_dot_path(&v, "missing"), None);
}

#[test]
fn missing_nested_dot_path_returns_none() {
    let v = serde_json::json!({"metadata": {"name": "pod-1"}});
    assert_eq!(extract_dot_path(&v, "metadata.missing"), None);
}

// --- flatten_object_scalars ---

#[test]
fn flatten_simple_object() {
    let v = serde_json::json!({"name": "pod-1", "status": "Running", "count": 3});
    let item = flatten_object_scalars(&v);
    assert_eq!(item.get("name").unwrap(), "pod-1");
    assert_eq!(item.get("status").unwrap(), "Running");
    assert_eq!(item.get("count").unwrap(), "3");
}

#[test]
fn flatten_non_object_returns_empty() {
    let v = serde_json::json!("not an object");
    let item = flatten_object_scalars(&v);
    assert!(item.is_empty());
}

// --- extract_json: scalar extraction ---

#[test]
fn extract_single_scalar_string() {
    let json = r#"{"version": "1.2.3"}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.version".to_string(),
            as_name: "version".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert_eq!(vars.get("version").unwrap(), "1.2.3");
    assert!(chunks.is_empty());
}

#[test]
fn extract_single_scalar_number() {
    let json = r#"{"count": 42}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.count".to_string(),
            as_name: "total".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    assert_eq!(vars.get("total").unwrap(), "42");
}

#[test]
fn extract_single_scalar_bool() {
    let json = r#"{"ready": true}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.ready".to_string(),
            as_name: "is_ready".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    assert_eq!(vars.get("is_ready").unwrap(), "true");
}

// --- extract_json: array of scalars ---

#[test]
fn extract_array_of_strings() {
    let json = r#"{"names": ["alice", "bob", "charlie"]}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.names[*]".to_string(),
            as_name: "users".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert_eq!(vars.get("users_count").unwrap(), "3");
    let data = chunks.get("users").unwrap();
    if let ChunkData::Flat(items) = data {
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].get("value").unwrap(), "alice");
        assert_eq!(items[1].get("value").unwrap(), "bob");
        assert_eq!(items[2].get("value").unwrap(), "charlie");
    } else {
        panic!("expected Flat chunk data");
    }
}

// --- extract_json: array of objects with fields ---

#[test]
fn extract_objects_with_fields() {
    let json = r#"{
        "items": [
            {"metadata": {"name": "pod-1"}, "status": {"phase": "Running"}},
            {"metadata": {"name": "pod-2"}, "status": {"phase": "Pending"}}
        ]
    }"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.items[*]".to_string(),
            as_name: "pods".to_string(),
            fields: vec![
                JsonFieldExtract {
                    field: "metadata.name".to_string(),
                    as_name: "name".to_string(),
                },
                JsonFieldExtract {
                    field: "status.phase".to_string(),
                    as_name: "phase".to_string(),
                },
            ],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert_eq!(vars.get("pods_count").unwrap(), "2");
    let data = chunks.get("pods").unwrap();
    if let ChunkData::Flat(items) = data {
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name").unwrap(), "pod-1");
        assert_eq!(items[0].get("phase").unwrap(), "Running");
        assert_eq!(items[1].get("name").unwrap(), "pod-2");
        assert_eq!(items[1].get("phase").unwrap(), "Pending");
    } else {
        panic!("expected Flat chunk data");
    }
}

// --- extract_json: objects without fields (auto-flatten) ---

#[test]
fn extract_objects_without_fields_flattens() {
    let json = r#"{"items": [{"name": "a", "count": 1}, {"name": "b", "count": 2}]}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.items[*]".to_string(),
            as_name: "things".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert_eq!(vars.get("things_count").unwrap(), "2");
    let data = chunks.get("things").unwrap();
    if let ChunkData::Flat(items) = data {
        assert_eq!(items[0].get("name").unwrap(), "a");
        assert_eq!(items[0].get("count").unwrap(), "1");
        assert_eq!(items[1].get("name").unwrap(), "b");
        assert_eq!(items[1].get("count").unwrap(), "2");
    } else {
        panic!("expected Flat chunk data");
    }
}

// --- extract_json: error handling ---

#[test]
fn invalid_json_returns_empty() {
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.foo".to_string(),
            as_name: "foo".to_string(),
            fields: vec![],
        }],
    };
    let (parsed, vars, chunks) = extract_json("not json at all", &config);
    assert!(!parsed, "invalid JSON should return parsed=false");
    assert!(vars.is_empty());
    assert!(chunks.is_empty());
}

#[test]
fn invalid_jsonpath_skips_rule() {
    let json = r#"{"foo": "bar"}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$[invalid!!!".to_string(),
            as_name: "foo".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert!(vars.is_empty());
    assert!(chunks.is_empty());
}

#[test]
fn missing_path_returns_empty() {
    let json = r#"{"foo": "bar"}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.nonexistent".to_string(),
            as_name: "val".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert!(vars.is_empty());
    assert!(chunks.is_empty());
}

// --- extract_json: multiple rules ---

#[test]
fn multiple_extraction_rules() {
    let json = r#"{
        "apiVersion": "v1",
        "items": [
            {"name": "a"},
            {"name": "b"}
        ]
    }"#;
    let config = JsonConfig {
        extract: vec![
            JsonExtractRule {
                path: "$.apiVersion".to_string(),
                as_name: "api".to_string(),
                fields: vec![],
            },
            JsonExtractRule {
                path: "$.items[*]".to_string(),
                as_name: "items".to_string(),
                fields: vec![JsonFieldExtract {
                    field: "name".to_string(),
                    as_name: "name".to_string(),
                }],
            },
        ],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert_eq!(vars.get("api").unwrap(), "v1");
    assert_eq!(vars.get("items_count").unwrap(), "2");
    assert!(chunks.contains_key("items"));
}

// --- extract_dot_path: array index support ---

#[test]
fn dot_path_with_array_index() {
    let v = serde_json::json!({"containers": [{"name": "web"}, {"name": "sidecar"}]});
    assert_eq!(
        extract_dot_path(&v, "containers.0.name"),
        Some("web".to_string())
    );
    assert_eq!(
        extract_dot_path(&v, "containers.1.name"),
        Some("sidecar".to_string())
    );
}

#[test]
fn dot_path_array_index_out_of_bounds() {
    let v = serde_json::json!({"items": ["a"]});
    assert_eq!(extract_dot_path(&v, "items.5"), None);
}

// --- extract_json: single array node (is_array guard) ---

#[test]
fn extract_single_array_value_becomes_chunk() {
    // When JSONPath matches a single node that IS an array, it should
    // be treated as multi-result, not scalar.
    let json = r#"{"tags": ["v1", "v2", "v3"]}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.tags".to_string(),
            as_name: "tags".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    // The single array node triggers process_multi_result
    assert!(vars.contains_key("tags_count"));
}

// --- extract_json: single object without fields (auto-flatten) ---

#[test]
fn extract_single_object_without_fields_flattens() {
    let json = r#"{"metadata": {"name": "my-pod", "namespace": "default"}}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.metadata".to_string(),
            as_name: "meta".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    // Single non-array object → scalar (compact JSON)
    assert!(vars.contains_key("meta"));
}

// --- extract_json: null value extraction ---

#[test]
fn extract_null_value() {
    let json = r#"{"value": null}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.value".to_string(),
            as_name: "val".to_string(),
            fields: vec![],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    assert_eq!(vars.get("val").unwrap(), "null");
}

// --- extract_json: root-level array ---

#[test]
fn extract_from_root_level_array() {
    let json = r#"[{"name": "a"}, {"name": "b"}]"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$[*]".to_string(),
            as_name: "items".to_string(),
            fields: vec![JsonFieldExtract {
                field: "name".to_string(),
                as_name: "name".to_string(),
            }],
        }],
    };
    let (_, vars, chunks) = extract_json(json, &config);
    assert_eq!(vars.get("items_count").unwrap(), "2");
    let data = chunks.get("items").unwrap();
    if let ChunkData::Flat(items) = data {
        assert_eq!(items[0].get("name").unwrap(), "a");
        assert_eq!(items[1].get("name").unwrap(), "b");
    } else {
        panic!("expected Flat chunk data");
    }
}

// --- extract_json: empty array produces _count = "0" ---

#[test]
fn empty_array_with_fields_produces_zero_count() {
    let json = r#"{"items": []}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.items[*]".to_string(),
            as_name: "pods".to_string(),
            fields: vec![JsonFieldExtract {
                field: "name".to_string(),
                as_name: "name".to_string(),
            }],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    assert_eq!(vars.get("pods_count").unwrap(), "0");
}

#[test]
fn missing_path_with_fields_produces_zero_count() {
    let json = r#"{"other": "value"}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.items[*]".to_string(),
            as_name: "pods".to_string(),
            fields: vec![JsonFieldExtract {
                field: "name".to_string(),
                as_name: "name".to_string(),
            }],
        }],
    };
    let (_, vars, _) = extract_json(json, &config);
    assert_eq!(vars.get("pods_count").unwrap(), "0");
}

// --- TOML deserialization round-trip ---

#[test]
fn json_config_deserializes_from_toml() {
    let toml_str = r#"
command = "kubectl get pods -o json"

[json]

[[json.extract]]
path = "$.items[*]"
as = "pods"

[[json.extract.fields]]
field = "metadata.name"
as = "name"

[[json.extract.fields]]
field = "status.phase"
as = "phase"

[on_success]
output = "Pods ({pods_count}):\n{pods | each: \"  {name}: {phase}\" | join: \"\\n\"}"
"#;
    let config: tokf_common::config::types::FilterConfig = toml::from_str(toml_str).unwrap();
    let json_config = config.json.unwrap();
    assert_eq!(json_config.extract.len(), 1);
    assert_eq!(json_config.extract[0].path, "$.items[*]");
    assert_eq!(json_config.extract[0].as_name, "pods");
    assert_eq!(json_config.extract[0].fields.len(), 2);
    assert_eq!(json_config.extract[0].fields[0].field, "metadata.name");
    assert_eq!(json_config.extract[0].fields[0].as_name, "name");
}

// --- extract_json: field with missing sub-path defaults to empty ---

#[test]
fn missing_field_subpath_defaults_to_empty() {
    let json = r#"{"items": [{"name": "a"}]}"#;
    let config = JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.items[*]".to_string(),
            as_name: "items".to_string(),
            fields: vec![
                JsonFieldExtract {
                    field: "name".to_string(),
                    as_name: "name".to_string(),
                },
                JsonFieldExtract {
                    field: "missing.field".to_string(),
                    as_name: "extra".to_string(),
                },
            ],
        }],
    };
    let (_, _, chunks) = extract_json(json, &config);
    let data = chunks.get("items").unwrap();
    if let ChunkData::Flat(items) = data {
        assert_eq!(items[0].get("name").unwrap(), "a");
        assert_eq!(items[0].get("extra").unwrap(), "");
    } else {
        panic!("expected Flat chunk data");
    }
}
