use futures::StreamExt;
use rustfin_ai_agent::{ChatChunk, ChatMessage, LlamaEngine};
use serde::{Deserialize, Serialize};

use super::profiles::{answer_sampling_params, artifact_verifier_sampling_params};
use super::types::{
    AssistantArtifactVerificationDebug, AssistantResponseMode, AssistantToolContextBlock,
};

const MAX_DOCUMENT_VERIFY_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone)]
pub struct DocumentVerificationResult {
    pub content: String,
    pub debug: AssistantArtifactVerificationDebug,
    pub notes_json: String,
}

#[derive(Debug, Clone, Serialize)]
struct DocumentVerificationNotes<'a> {
    status: &'a str,
    attempts: u32,
    revision_count: u32,
    issues: &'a [String],
    sources: Vec<DocumentVerificationSourceNote>,
}

#[derive(Debug, Clone, Serialize)]
struct DocumentVerificationSourceNote {
    tool: String,
    label: String,
    status: String,
}

#[derive(Debug, Default, Deserialize)]
struct DocumentVerificationModelResponse {
    status: String,
    #[serde(default)]
    issues: Vec<String>,
    #[serde(default)]
    revision_instructions: Option<String>,
}

pub async fn verify_or_repair_document(
    engine: &LlamaEngine,
    format_label: &str,
    request_prompt: &str,
    grounding_blocks: &[AssistantToolContextBlock],
    initial_content: String,
) -> Result<DocumentVerificationResult, String> {
    let mut content = initial_content;
    let mut attempts = 0;
    let mut revision_count = 0;
    let mut latest_issues = Vec::new();

    while attempts < MAX_DOCUMENT_VERIFY_ATTEMPTS {
        attempts += 1;
        let mut issues = deterministic_document_issues(format_label, request_prompt, &content);
        let verifier = run_model_verifier(
            engine,
            format_label,
            request_prompt,
            grounding_blocks,
            &content,
        )
        .await?;
        issues.extend(verifier.issues.into_iter());
        normalize_issue_list(&mut issues);

        if verifier.status.eq_ignore_ascii_case("pass") && issues.is_empty() {
            let status = if revision_count > 0 {
                "repaired"
            } else {
                "passed"
            };
            let debug = AssistantArtifactVerificationDebug {
                status: status.to_string(),
                attempts,
                revision_count,
                issues: latest_issues,
            };
            let notes_json = verification_notes_json(
                status,
                attempts,
                revision_count,
                &debug.issues,
                grounding_blocks,
            );
            return Ok(DocumentVerificationResult {
                content,
                debug,
                notes_json,
            });
        }

        latest_issues = issues;
        if attempts >= MAX_DOCUMENT_VERIFY_ATTEMPTS {
            break;
        }

        content = run_document_repair(
            engine,
            format_label,
            request_prompt,
            grounding_blocks,
            &content,
            &latest_issues,
            verifier.revision_instructions.as_deref(),
        )
        .await?;
        revision_count += 1;
    }

    Err(format!(
        "Rustyfin AI could not verify the generated document: {}",
        if latest_issues.is_empty() {
            "the verifier did not approve the draft".to_string()
        } else {
            latest_issues.join("; ")
        }
    ))
}

fn deterministic_document_issues(
    format_label: &str,
    request_prompt: &str,
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    let normalized = content.trim();
    if normalized.is_empty() {
        issues.push("the document body is empty".to_string());
    }
    if normalized.len() < 48 {
        issues.push("the document is too short to satisfy the request".to_string());
    }

    let lower = normalized.to_ascii_lowercase();
    for placeholder in [
        "todo",
        "tbd",
        "lorem ipsum",
        "[insert",
        "placeholder",
        "add details here",
    ] {
        if lower.contains(placeholder) {
            issues.push(format!(
                "the document still contains placeholder text ({placeholder})"
            ));
        }
    }

    if format_label.eq_ignore_ascii_case("markdown") {
        let has_heading = normalized.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('#')
        });
        if !has_heading {
            issues.push("the markdown document is missing headings".to_string());
        }
    }

    if !format_label.eq_ignore_ascii_case("markdown") && normalized.contains("```") {
        issues.push("the plain-text document still contains markdown code fences".to_string());
    }

    if request_prompt.to_ascii_lowercase().contains("checklist")
        && !normalized.contains("- ")
        && !normalized.contains("* ")
    {
        issues.push("the requested checklist structure is missing".to_string());
    }

    issues
}

async fn run_model_verifier(
    engine: &LlamaEngine,
    format_label: &str,
    request_prompt: &str,
    grounding_blocks: &[AssistantToolContextBlock],
    content: &str,
) -> Result<DocumentVerificationModelResponse, String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You verify grounded Rustyfin downloadable documents. Return JSON only with keys status, issues, revision_instructions. status must be one of pass, revise, fail. Report only unsupported grounded claims, missing requested sections, obvious formatting problems, or mismatches with the user request.".to_string(),
        },
        ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Grounding for verification:\n{}",
                serde_json::to_string(grounding_blocks).unwrap_or_else(|_| "[]".to_string())
            ),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Requested {} document:\n{}\n\nGenerated document draft:\n{}",
                format_label,
                request_prompt.trim(),
                content.trim()
            ),
        },
    ];

    let raw = collect_response_text(engine, messages, artifact_verifier_sampling_params()).await?;
    let json_text = extract_json_object(&raw)
        .ok_or_else(|| "document verifier returned invalid JSON".to_string())?;
    serde_json::from_str::<DocumentVerificationModelResponse>(json_text)
        .map_err(|error| format!("document verifier returned malformed JSON: {error}"))
}

async fn run_document_repair(
    engine: &LlamaEngine,
    format_label: &str,
    request_prompt: &str,
    grounding_blocks: &[AssistantToolContextBlock],
    content: &str,
    issues: &[String],
    revision_instructions: Option<&str>,
) -> Result<String, String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: format!(
                "You repair grounded Rustyfin downloadable documents. Return only the repaired {} document body with no commentary and no code fences.",
                format_label
            ),
        },
        ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Authoritative Rustyfin grounding:\n{}",
                serde_json::to_string(grounding_blocks).unwrap_or_else(|_| "[]".to_string())
            ),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Original request:\n{}\n\nCurrent draft:\n{}\n\nRepair the draft so it satisfies the request and fix these issues:\n- {}\n{}",
                request_prompt.trim(),
                content.trim(),
                issues.join("\n- "),
                revision_instructions
                    .map(|value| format!("\nExtra verifier guidance:\n{value}"))
                    .unwrap_or_default()
            ),
        },
    ];

    let mut sampling = answer_sampling_params(AssistantResponseMode::Extended);
    sampling.max_tokens = sampling.max_tokens.min(2048);
    let repaired = collect_response_text(engine, messages, sampling).await?;
    let trimmed = repaired.trim();
    if trimmed.is_empty() {
        return Err("document repair generated an empty document".to_string());
    }
    if trimmed.len() > 64_000 {
        return Err("the repaired document exceeded the maximum allowed size".to_string());
    }
    Ok(trimmed.replace("\r\n", "\n"))
}

async fn collect_response_text(
    engine: &LlamaEngine,
    messages: Vec<ChatMessage>,
    sampling: rustfin_ai_agent::SamplingParams,
) -> Result<String, String> {
    let stream = engine.chat_stream(messages, sampling);
    futures::pin_mut!(stream);

    let mut output = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk.map_err(|error| format!("document verification failed: {error}"))? {
            ChatChunk::Token(text) => output.push_str(&text),
            ChatChunk::Stats { .. } | ChatChunk::Done => {}
        }
    }
    Ok(output)
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then_some(&raw[start..=end])
}

fn normalize_issue_list(issues: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    issues.retain_mut(|issue| {
        *issue = issue.trim().trim_end_matches('.').to_string();
        !issue.is_empty() && seen.insert(issue.to_ascii_lowercase())
    });
}

fn verification_notes_json(
    status: &str,
    attempts: u32,
    revision_count: u32,
    issues: &[String],
    grounding_blocks: &[AssistantToolContextBlock],
) -> String {
    let notes = DocumentVerificationNotes {
        status,
        attempts,
        revision_count,
        issues,
        sources: grounding_blocks
            .iter()
            .map(|block| DocumentVerificationSourceNote {
                tool: block.tool.to_string(),
                label: block.label.clone(),
                status: block.status.to_string(),
            })
            .collect(),
    };
    serde_json::to_string(&notes).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::{deterministic_document_issues, extract_json_object};

    #[test]
    fn deterministic_document_issues_flags_placeholders_and_missing_headings() {
        let issues = deterministic_document_issues(
            "markdown",
            "Write a checklist for the deployment.",
            "TODO\nThis is placeholder text.",
        );
        assert!(issues.iter().any(|issue| issue.contains("placeholder")));
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("missing headings"))
        );
        assert!(issues.iter().any(|issue| issue.contains("checklist")));
    }

    #[test]
    fn extract_json_object_finds_embedded_json() {
        assert_eq!(
            extract_json_object("prefix {\"status\":\"pass\"} suffix"),
            Some("{\"status\":\"pass\"}")
        );
    }
}
