use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureCorpusSpec {
    pub suite_name: &'static str,
    pub fixture_file: &'static str,
    pub schema_file: &'static str,
}

const FIXTURE_CORPUS_SPECS: &[FixtureCorpusSpec] = &[
    FixtureCorpusSpec {
        suite_name: "comparison",
        fixture_file: "comparison_cases.jsonl",
        schema_file: "comparison_cases.schema.json",
    },
    FixtureCorpusSpec {
        suite_name: "judge",
        fixture_file: "judge_cases.jsonl",
        schema_file: "judge_cases.schema.json",
    },
    FixtureCorpusSpec {
        suite_name: "planner",
        fixture_file: "planner_cases.jsonl",
        schema_file: "planner_cases.schema.json",
    },
    FixtureCorpusSpec {
        suite_name: "retrieval",
        fixture_file: "retrieval_cases.jsonl",
        schema_file: "retrieval_cases.schema.json",
    },
    FixtureCorpusSpec {
        suite_name: "memory",
        fixture_file: "memory_cases.jsonl",
        schema_file: "memory_cases.schema.json",
    },
    FixtureCorpusSpec {
        suite_name: "execution",
        fixture_file: "execution_cases.jsonl",
        schema_file: "execution_cases.schema.json",
    },
    FixtureCorpusSpec {
        suite_name: "tasks",
        fixture_file: "task_cases.jsonl",
        schema_file: "task_cases.schema.json",
    },
];

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ai")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ai")
        })
}

pub fn fixture_specs_for_mode(mode: &str) -> Result<Vec<FixtureCorpusSpec>> {
    match mode {
        "comparison" | "judge" | "planner" | "retrieval" | "memory" | "execution" | "tasks" => {
            FIXTURE_CORPUS_SPECS
                .iter()
                .copied()
                .find(|spec| spec.suite_name == mode)
                .map(|spec| vec![spec])
                .ok_or_else(|| anyhow::anyhow!("missing fixture corpus spec for mode {mode}"))
        }
        "all" | "default" => Ok(FIXTURE_CORPUS_SPECS
            .iter()
            .copied()
            .filter(|spec| spec.suite_name != "comparison")
            .collect()),
        other => bail!("unknown eval mode: {other}"),
    }
}

pub fn fixture_path(fixtures_dir: &Path, spec: &FixtureCorpusSpec) -> PathBuf {
    fixtures_dir.join(spec.fixture_file)
}

pub fn schema_path(fixtures_dir: &Path, spec: &FixtureCorpusSpec) -> PathBuf {
    fixtures_dir.join(spec.schema_file)
}

pub fn trace_archive_dir(fixtures_dir: &Path) -> PathBuf {
    fixtures_dir.join("trace_archive")
}

pub fn curated_trace_cases_path(fixtures_dir: &Path) -> PathBuf {
    fixtures_dir.join("trace_curated_cases.jsonl")
}

pub fn curated_trace_cases_schema_path(fixtures_dir: &Path) -> PathBuf {
    fixtures_dir.join("trace_curated_cases.schema.json")
}

pub fn load_jsonl<T: DeserializeOwned>(path: &Path, schema_path: &Path) -> Result<Vec<T>> {
    validate_jsonl_schema(path, schema_path)?
        .into_iter()
        .map(|value| {
            serde_json::from_value::<T>(value)
                .with_context(|| format!("failed to deserialize {}", path.display()))
        })
        .collect()
}

pub fn validate_jsonl_schema(path: &Path, schema_path: &Path) -> Result<Vec<Value>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let schema = load_schema(schema_path)?;
    let mut values = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(trimmed).with_context(|| {
            format!(
                "invalid jsonl in {} on line {}",
                path.display(),
                line_index + 1
            )
        })?;
        let mut errors = Vec::new();
        validate_value_against_schema(&value, &schema, "$", &mut errors);
        if !errors.is_empty() {
            bail!(
                "schema validation failed for {} line {}: {}",
                path.display(),
                line_index + 1,
                errors.join("; ")
            );
        }
        values.push(value);
    }

    Ok(values)
}

pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn append_jsonl_row<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let row = serde_json::to_string(value)?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{row}").with_context(|| format!("failed to append {}", path.display()))
}

pub fn fixture_digest(fixtures_dir: &Path, specs: &[FixtureCorpusSpec]) -> Result<String> {
    digest_paths(
        &specs
            .iter()
            .map(|spec| fixture_path(fixtures_dir, spec))
            .collect::<Vec<_>>(),
    )
}

pub fn schema_digest(fixtures_dir: &Path, specs: &[FixtureCorpusSpec]) -> Result<String> {
    digest_paths(
        &specs
            .iter()
            .map(|spec| schema_path(fixtures_dir, spec))
            .collect::<Vec<_>>(),
    )
}

fn load_schema(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read schema {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("invalid schema JSON in {}", path.display()))
}

fn digest_paths(paths: &[PathBuf]) -> Result<String> {
    let mut hasher = Sha256::new();
    for path in paths {
        let content =
            std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(&content);
        hasher.update([0xff]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn validate_value_against_schema(
    value: &Value,
    schema: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    let Some(schema_obj) = schema.as_object() else {
        return;
    };

    if let Some(expected_type) = schema_obj.get("type").and_then(Value::as_str) {
        let type_matches = match expected_type {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !type_matches {
            errors.push(format!("{path}: expected type {expected_type}"));
            return;
        }
    }

    if let Some(enum_values) = schema_obj.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|candidate| candidate == value) {
            errors.push(format!("{path}: value is not in enum"));
        }
    }

    if let Some(text) = value.as_str() {
        if let Some(min_length) = schema_obj.get("minLength").and_then(Value::as_u64) {
            if text.chars().count() < min_length as usize {
                errors.push(format!(
                    "{path}: string shorter than minLength {min_length}"
                ));
            }
        }
        if let Some(max_length) = schema_obj.get("maxLength").and_then(Value::as_u64) {
            if text.chars().count() > max_length as usize {
                errors.push(format!("{path}: string longer than maxLength {max_length}"));
            }
        }
    }

    if let Some(array) = value.as_array() {
        if let Some(min_items) = schema_obj.get("minItems").and_then(Value::as_u64) {
            if array.len() < min_items as usize {
                errors.push(format!("{path}: array shorter than minItems {min_items}"));
            }
        }
        if let Some(max_items) = schema_obj.get("maxItems").and_then(Value::as_u64) {
            if array.len() > max_items as usize {
                errors.push(format!("{path}: array longer than maxItems {max_items}"));
            }
        }
        if let Some(item_schema) = schema_obj.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate_value_against_schema(
                    item,
                    item_schema,
                    &format!("{path}[{index}]"),
                    errors,
                );
            }
        }
    }

    if let Some(object) = value.as_object() {
        if let Some(required) = schema_obj.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    errors.push(format!("{path}: missing required property {key}"));
                }
            }
        }

        let additional_allowed = schema_obj
            .get("additionalProperties")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let properties = schema_obj.get("properties").and_then(Value::as_object);
        if !additional_allowed {
            for key in object.keys() {
                let known = properties
                    .map(|props| props.contains_key(key))
                    .unwrap_or(false);
                if !known {
                    errors.push(format!("{path}: unexpected property {key}"));
                }
            }
        }
        if let Some(properties) = properties {
            for (key, prop_schema) in properties {
                if let Some(child) = object.get(key) {
                    validate_value_against_schema(
                        child,
                        prop_schema,
                        &format!("{path}.{key}"),
                        errors,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        FIXTURE_CORPUS_SPECS, fixture_path, fixtures_dir, load_jsonl, schema_path,
        validate_jsonl_schema,
    };

    #[test]
    fn all_fixture_files_validate_against_their_schemas() {
        let dir = fixtures_dir();
        for spec in FIXTURE_CORPUS_SPECS {
            let values =
                validate_jsonl_schema(&fixture_path(&dir, spec), &schema_path(&dir, spec)).unwrap();
            assert!(
                !values.is_empty(),
                "{} should contain at least one fixture row",
                spec.fixture_file
            );
        }
    }

    #[test]
    fn invalid_json_line_fails_fast() {
        let dir = tempdir().unwrap();
        let fixture_path = dir.path().join("cases.jsonl");
        let schema_path = dir.path().join("cases.schema.json");
        std::fs::write(&fixture_path, "{\"name\":\"ok\"}\n{").unwrap();
        std::fs::write(
            &schema_path,
            r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#,
        )
        .unwrap();

        let error = validate_jsonl_schema(&fixture_path, &schema_path).unwrap_err();
        assert!(error.to_string().contains("invalid jsonl"));
    }

    #[test]
    fn schema_validation_rejects_missing_required_properties() {
        let dir = tempdir().unwrap();
        let fixture_path = dir.path().join("cases.jsonl");
        let schema_path = dir.path().join("cases.schema.json");
        std::fs::write(&fixture_path, "{\"missing\":\"name\"}\n").unwrap();
        std::fs::write(
            &schema_path,
            r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":true}"#,
        )
        .unwrap();

        let error = validate_jsonl_schema(&fixture_path, &schema_path).unwrap_err();
        assert!(error.to_string().contains("missing required property name"));
    }

    #[test]
    fn load_jsonl_deserializes_after_schema_validation() {
        let dir = fixtures_dir();
        let spec = FIXTURE_CORPUS_SPECS
            .iter()
            .find(|spec| spec.suite_name == "planner")
            .unwrap();
        let rows =
            load_jsonl::<serde_json::Value>(&fixture_path(&dir, spec), &schema_path(&dir, spec))
                .unwrap();
        assert!(!rows.is_empty());
    }
}
