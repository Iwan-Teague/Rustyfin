use super::types::WorkerResult;

pub fn merge_worker_results(objective: &str, worker_results: &[WorkerResult]) -> String {
    let mut lines = vec![
        "# Deep Research Report".to_string(),
        String::new(),
        format!("Objective: {objective}"),
        String::new(),
        "## Findings".to_string(),
    ];
    for worker in worker_results {
        lines.push(format!("### {}", worker.objective));
        lines.push(worker.summary.clone());
        for finding in &worker.findings {
            lines.push(format!(
                "- {} [{}]",
                finding.claim,
                finding.evidence_refs.join(", ")
            ));
        }
        lines.push(String::new());
    }
    lines.push("## Sources".to_string());
    let mut seen = std::collections::BTreeSet::new();
    for worker in worker_results {
        for finding in &worker.findings {
            for evidence_ref in &finding.evidence_refs {
                if seen.insert(evidence_ref.clone()) {
                    lines.push(format!("- {evidence_ref}"));
                }
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::merge_worker_results;
    use crate::ai_tasks::types::{WorkerFinding, WorkerResult};

    #[test]
    fn merge_combines_multiple_worker_results() {
        let report = merge_worker_results(
            "Investigate Rustyfin state",
            &[
                WorkerResult {
                    worker_profile: "grounding_worker".to_string(),
                    objective: "Health".to_string(),
                    findings: vec![WorkerFinding {
                        claim: "API is healthy".to_string(),
                        evidence_refs: vec!["chunk-1".to_string()],
                        confidence: 0.9,
                        open_questions: Vec::new(),
                    }],
                    summary: "Health summary".to_string(),
                },
                WorkerResult {
                    worker_profile: "grounding_worker".to_string(),
                    objective: "Storage".to_string(),
                    findings: vec![WorkerFinding {
                        claim: "Storage is available".to_string(),
                        evidence_refs: vec!["chunk-2".to_string()],
                        confidence: 0.8,
                        open_questions: Vec::new(),
                    }],
                    summary: "Storage summary".to_string(),
                },
            ],
        );
        assert!(report.contains("Health"));
        assert!(report.contains("Storage"));
    }
}
