use std::collections::{BTreeMap, HashSet};

use super::replies::compact_text;
use super::types::{AssistantEvidenceItem, AssistantToolOutcome};

pub fn collect_retained_evidence(
    outcomes: &[AssistantToolOutcome],
    max_items: usize,
) -> Vec<AssistantEvidenceItem> {
    let mut seen = HashSet::<String>::new();
    let mut retained = Vec::new();

    for outcome in outcomes {
        for item in &outcome.evidence_items {
            let key = if item.id.is_empty() {
                format!("{}:{}", item.tool, item.title)
            } else {
                item.id.clone()
            };
            if !seen.insert(key) {
                continue;
            }
            retained.push(AssistantEvidenceItem {
                excerpt: compact_text(&item.excerpt, 220),
                ..item.clone()
            });
            if retained.len() >= max_items {
                return retained;
            }
        }
    }

    retained
}

pub fn conflicting_evidence_count(items: &[AssistantEvidenceItem]) -> u32 {
    let mut seen = BTreeMap::<String, String>::new();
    let mut conflicts = 0u32;
    for item in items {
        let Some(key) = item.conflict_key.clone().or_else(|| {
            (!item.title.trim().is_empty()).then(|| item.title.trim().to_ascii_lowercase())
        }) else {
            continue;
        };
        let value = item.excerpt.trim().to_ascii_lowercase();
        if let Some(previous) = seen.get(&key) {
            if previous != &value {
                conflicts = conflicts.saturating_add(1);
            }
        } else {
            seen.insert(key, value);
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::{collect_retained_evidence, conflicting_evidence_count};
    use crate::ai_assistant::types::{
        AssistantDomainFamily, AssistantEvidenceItem, AssistantToolContextBlock,
        AssistantToolOutcome, AssistantToolOutcomeKind,
    };
    use serde_json::json;

    fn outcome(id: &str, title: &str, excerpt: &str) -> AssistantToolOutcome {
        AssistantToolOutcome {
            tool: "system_get_ai_runtime_summary".to_string(),
            label: title.to_string(),
            domain_family: AssistantDomainFamily::AiRuntime,
            kind: AssistantToolOutcomeKind::Answer,
            confidence: 1.0,
            block: AssistantToolContextBlock {
                tool: "system_get_ai_runtime_summary",
                label: title.to_string(),
                status: "ok",
                data: json!({}),
            },
            evidence_items: vec![AssistantEvidenceItem {
                id: id.to_string(),
                tool: "system_get_ai_runtime_summary".to_string(),
                domain_family: AssistantDomainFamily::AiRuntime,
                title: title.to_string(),
                excerpt: excerpt.to_string(),
                score: 1.0,
                tags: Vec::new(),
                source_chunk_id: None,
                freshness_hint: None,
                conflict_key: None,
            }],
            ambiguity_keys: Vec::new(),
            recovery_hints: Vec::new(),
            args_hash: "hash".to_string(),
            result_signature: "sig".to_string(),
            message: None,
            stale: false,
        }
    }

    #[test]
    fn evidence_collection_dedupes_by_id() {
        let retained = collect_retained_evidence(
            &[
                outcome("same", "Runtime", "First"),
                outcome("same", "Runtime", "Second"),
            ],
            5,
        );
        assert_eq!(retained.len(), 1);
    }

    #[test]
    fn conflict_counter_detects_divergent_values() {
        let count = conflicting_evidence_count(&[
            AssistantEvidenceItem {
                id: "a".to_string(),
                tool: "tool".to_string(),
                domain_family: AssistantDomainFamily::AiRuntime,
                title: "loaded model".to_string(),
                excerpt: "Llama-3.2".to_string(),
                score: 1.0,
                tags: Vec::new(),
                source_chunk_id: None,
                freshness_hint: None,
                conflict_key: Some("loaded_model".to_string()),
            },
            AssistantEvidenceItem {
                id: "b".to_string(),
                tool: "tool".to_string(),
                domain_family: AssistantDomainFamily::AiRuntime,
                title: "loaded model".to_string(),
                excerpt: "Mistral".to_string(),
                score: 1.0,
                tags: Vec::new(),
                source_chunk_id: None,
                freshness_hint: None,
                conflict_key: Some("loaded_model".to_string()),
            },
        ]);
        assert_eq!(count, 1);
    }
}
