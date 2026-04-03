use super::types::WorkerResult;

#[derive(Debug, Clone)]
pub struct ResearchVerification {
    pub issues: Vec<String>,
    pub revised_report: Option<String>,
}

pub fn verify_research_report(
    merged_report: &str,
    worker_results: &[WorkerResult],
) -> ResearchVerification {
    let mut issues = Vec::new();
    for worker in worker_results {
        for finding in &worker.findings {
            if finding.evidence_refs.is_empty() {
                issues.push(format!(
                    "unsupported claim in {}: {}",
                    worker.objective, finding.claim
                ));
            }
        }
    }

    let revised_report = if issues.is_empty() {
        None
    } else {
        Some(format!(
            "{merged_report}\n\n## Verification Notes\n- {}",
            issues.join("\n- ")
        ))
    };

    ResearchVerification {
        issues,
        revised_report,
    }
}

#[cfg(test)]
mod tests {
    use super::verify_research_report;
    use crate::ai_tasks::types::{WorkerFinding, WorkerResult};

    #[test]
    fn verifier_rejects_unsupported_claims() {
        let verification = verify_research_report(
            "Draft",
            &[WorkerResult {
                worker_profile: "grounding_worker".to_string(),
                objective: "Health".to_string(),
                findings: vec![WorkerFinding {
                    claim: "Unknown".to_string(),
                    evidence_refs: Vec::new(),
                    confidence: 0.2,
                    open_questions: Vec::new(),
                }],
                summary: "Summary".to_string(),
            }],
        );
        assert_eq!(verification.issues.len(), 1);
        assert!(verification.revised_report.is_some());
    }
}
