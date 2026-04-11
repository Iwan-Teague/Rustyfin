use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Result, bail};
use chrono::Utc;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::ai_audit::{
    AiAssistantAuditEventResponse, AiGroundingChunk, AiGroundingCitation, AiGroundingVisibility,
};
use crate::ai_turn_journal::AiTurnJournalSummary;

use super::corpus;

pub const TRACE_INGEST_SOURCE_VERSION: &str = "rustyfin_ai_trace_ingest_v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceSourceType {
    ProductionTrace,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceDifficulty {
    Easy,
    Medium,
    Hard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceSensitivity {
    Low,
    Medium,
    High,
    Restricted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceAccessBoundary {
    Public,
    Workspace,
    Private,
    AdminOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceReviewStatus {
    Draft,
    Approved,
    Rejected,
    Deprecated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceRedactionState {
    IdentifiersHashed,
    RedactedAndHashed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceCalibrationQueueStatus {
    NotRequired,
    Queued,
    InReview,
    Resolved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvalTraceCalibrationQueueReason {
    ReleaseRelevant,
    HighSensitivity,
    RestrictedSensitivity,
    FailedTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalTraceConversationTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalTraceExpectedContract {
    pub hard_gates: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub must_include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub must_not_include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_words: Option<u64>,
    pub requires_citation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalTraceReference {
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalTraceSource {
    pub kind: EvalTraceSourceType,
    pub version: String,
    pub trace_id: String,
    pub raw_archive_id: String,
    pub audit_event_id: String,
    pub turn_journal_id: String,
    pub source_user_id_hash: String,
    pub source_username_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalTraceMetadata {
    pub source_type: EvalTraceSourceType,
    pub difficulty: EvalTraceDifficulty,
    pub sensitivity: EvalTraceSensitivity,
    pub access_boundary: EvalTraceAccessBoundary,
    pub expected_answer_shape: String,
    pub review_status: EvalTraceReviewStatus,
    pub redaction_state: EvalTraceRedactionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalTraceReviewerDecision {
    pub reviewer: String,
    pub decision: EvalTraceReviewStatus,
    pub notes: String,
    pub decided_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalTraceConsensusLabel {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub reviewers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalTraceCalibrationQueue {
    pub status: EvalTraceCalibrationQueueStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<EvalTraceCalibrationQueueReason>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_reviewers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewer_decisions: Vec<EvalTraceReviewerDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consensus_label: Option<EvalTraceConsensusLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalTraceReview {
    pub status: EvalTraceReviewStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<String>,
    pub release_relevant: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_ts: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<EvalTraceCalibrationQueue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalCuratedTraceCase {
    pub id: String,
    pub prompt: String,
    pub domain: String,
    pub intent: String,
    pub mode: String,
    pub observed_response: String,
    pub expected: EvalTraceExpectedContract,
    pub source: EvalTraceSource,
    pub metadata: EvalTraceMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<EvalTraceReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversation_history: Vec<EvalTraceConversationTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intermediate_events: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_model_response: Option<String>,
    pub review: EvalTraceReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalTraceArchiveRecord {
    pub archive_id: String,
    pub trace_id: String,
    pub imported_ts: i64,
    pub audit_event: AiAssistantAuditEventResponse,
    pub turn_journal: AiTurnJournalSummary,
    pub assistant_response: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversation_history: Vec<EvalTraceConversationTurn>,
}

#[derive(Debug, Clone)]
pub struct EvalTraceImportSource {
    pub audit_event: AiAssistantAuditEventResponse,
    pub turn_journal: AiTurnJournalSummary,
    pub assistant_response: String,
    pub conversation_history: Vec<EvalTraceConversationTurn>,
    pub imported_ts: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct EvalTraceCurationRequest {
    pub case_id: String,
    pub domain: String,
    pub intent: String,
    pub difficulty: EvalTraceDifficulty,
    pub expected_answer_shape: String,
    pub hard_gates: Vec<String>,
    pub must_include: Vec<String>,
    pub must_not_include: Vec<String>,
    pub max_words: Option<u64>,
    pub requires_citation: bool,
    pub release_relevant: bool,
    pub reference_answer: Option<String>,
    pub reference_notes: Option<String>,
    pub baseline_model_response: Option<String>,
    pub assigned_reviewers: Vec<String>,
}

#[derive(Debug, Clone)]
struct RedactionContext {
    exact_values: Vec<ExactSensitiveValue>,
}

#[derive(Debug, Clone)]
struct ExactSensitiveValue {
    label: &'static str,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SanitizationOutcome {
    content_redacted: bool,
}

impl SanitizationOutcome {
    fn combine(self, other: Self) -> Self {
        Self {
            content_redacted: self.content_redacted || other.content_redacted,
        }
    }
}

pub fn archive_trace_source(source: EvalTraceImportSource) -> EvalTraceArchiveRecord {
    EvalTraceArchiveRecord {
        archive_id: format!("trace-archive-{}", source.audit_event.trace_id),
        trace_id: source.audit_event.trace_id.clone(),
        imported_ts: source.imported_ts.unwrap_or_else(|| Utc::now().timestamp()),
        audit_event: source.audit_event,
        turn_journal: source.turn_journal,
        assistant_response: source.assistant_response,
        conversation_history: source.conversation_history,
    }
}

pub fn curate_trace_archive_record(
    archive: &EvalTraceArchiveRecord,
    request: &EvalTraceCurationRequest,
) -> Result<EvalCuratedTraceCase> {
    let mode = normalize_mode(&archive.turn_journal.response_mode)?;
    if request.hard_gates.is_empty() {
        bail!("trace curation requires at least one hard gate");
    }

    let redaction_context = build_redaction_context(archive);
    let access_boundary = derive_access_boundary(
        &archive.audit_event.grounding_chunks,
        &archive.audit_event.user_role,
    );
    let sensitivity = derive_sensitivity(access_boundary);

    let (prompt, prompt_outcome) =
        sanitize_text(&archive.turn_journal.request_message, &redaction_context);
    let (observed_response, response_outcome) =
        sanitize_text(&archive.assistant_response, &redaction_context);
    let (conversation_history, history_outcome) =
        sanitize_history(&archive.conversation_history, &redaction_context);
    let (intermediate_events, events_outcome) =
        build_intermediate_events(archive, &redaction_context)?;
    let (reference, reference_outcome) = sanitize_reference(
        request.reference_answer.as_deref(),
        request.reference_notes.as_deref(),
        &redaction_context,
    );
    let (baseline_model_response, baseline_outcome) = request
        .baseline_model_response
        .as_deref()
        .map(|value| sanitize_text(value, &redaction_context))
        .map(|(value, outcome)| (Some(value), outcome))
        .unwrap_or((
            None,
            SanitizationOutcome {
                content_redacted: false,
            },
        ));

    let content_outcome = prompt_outcome
        .combine(response_outcome)
        .combine(history_outcome)
        .combine(events_outcome)
        .combine(reference_outcome)
        .combine(baseline_outcome);
    let redaction_state = if content_outcome.content_redacted {
        EvalTraceRedactionState::RedactedAndHashed
    } else {
        EvalTraceRedactionState::IdentifiersHashed
    };

    let calibration = build_initial_calibration_queue(
        archive,
        request.release_relevant,
        sensitivity,
        request.assigned_reviewers.clone(),
    );

    Ok(EvalCuratedTraceCase {
        id: request.case_id.clone(),
        prompt,
        domain: request.domain.clone(),
        intent: request.intent.clone(),
        mode,
        observed_response,
        expected: EvalTraceExpectedContract {
            hard_gates: request.hard_gates.clone(),
            must_include: request.must_include.clone(),
            must_not_include: request.must_not_include.clone(),
            max_words: request.max_words,
            requires_citation: request.requires_citation,
        },
        source: EvalTraceSource {
            kind: EvalTraceSourceType::ProductionTrace,
            version: TRACE_INGEST_SOURCE_VERSION.to_string(),
            trace_id: archive.trace_id.clone(),
            raw_archive_id: archive.archive_id.clone(),
            audit_event_id: archive.audit_event.id.clone(),
            turn_journal_id: archive.turn_journal.id.clone(),
            source_user_id_hash: hash_identifier(&archive.audit_event.user_id),
            source_username_hash: hash_identifier(&archive.audit_event.username),
        },
        metadata: EvalTraceMetadata {
            source_type: EvalTraceSourceType::ProductionTrace,
            difficulty: request.difficulty,
            sensitivity,
            access_boundary,
            expected_answer_shape: request.expected_answer_shape.clone(),
            review_status: EvalTraceReviewStatus::Draft,
            redaction_state,
        },
        reference,
        conversation_history,
        intermediate_events,
        baseline_model_response,
        review: EvalTraceReview {
            status: EvalTraceReviewStatus::Draft,
            reviewer: None,
            review_notes: None,
            release_relevant: request.release_relevant,
            approved_ts: None,
            calibration,
        },
    })
}

pub fn approve_curated_trace_case(
    case: &mut EvalCuratedTraceCase,
    reviewer: &str,
    notes: Option<&str>,
    approved_ts: Option<i64>,
) {
    case.review.status = EvalTraceReviewStatus::Approved;
    case.review.reviewer = Some(reviewer.to_string());
    case.review.review_notes = notes.map(str::to_string);
    case.review.approved_ts = Some(approved_ts.unwrap_or_else(|| Utc::now().timestamp()));
    case.metadata.review_status = EvalTraceReviewStatus::Approved;
}

pub fn queue_trace_calibration_review(
    case: &mut EvalCuratedTraceCase,
    reasons: Vec<EvalTraceCalibrationQueueReason>,
    assigned_reviewers: Vec<String>,
) {
    case.review.calibration = Some(EvalTraceCalibrationQueue {
        status: EvalTraceCalibrationQueueStatus::Queued,
        reasons,
        assigned_reviewers,
        reviewer_decisions: Vec::new(),
        consensus_label: None,
    });
}

pub fn resolve_trace_calibration_consensus(
    case: &mut EvalCuratedTraceCase,
    label: EvalTraceConsensusLabel,
    reviewer_decisions: Vec<EvalTraceReviewerDecision>,
) {
    let mut queue = case
        .review
        .calibration
        .clone()
        .unwrap_or(EvalTraceCalibrationQueue {
            status: EvalTraceCalibrationQueueStatus::NotRequired,
            reasons: Vec::new(),
            assigned_reviewers: Vec::new(),
            reviewer_decisions: Vec::new(),
            consensus_label: None,
        });
    queue.status = EvalTraceCalibrationQueueStatus::Resolved;
    queue.reviewer_decisions = reviewer_decisions;
    queue.consensus_label = Some(label);
    case.review.calibration = Some(queue);
}

pub fn assert_trace_case_release_ready(case: &EvalCuratedTraceCase) -> Result<()> {
    if case.metadata.source_type != EvalTraceSourceType::ProductionTrace {
        return Ok(());
    }
    if case.metadata.review_status != case.review.status {
        bail!("metadata.review_status and review.status must agree");
    }
    if case.review.status != EvalTraceReviewStatus::Approved {
        bail!("production trace case is not approved");
    }
    if case.review.release_relevant {
        let Some(calibration) = case.review.calibration.as_ref() else {
            bail!("release-relevant trace case requires calibration queue metadata");
        };
        if calibration.status != EvalTraceCalibrationQueueStatus::Resolved {
            bail!("release-relevant trace case has unresolved calibration queue");
        }
        if calibration.consensus_label.is_none() {
            bail!("resolved calibration queue is missing consensus label");
        }
    }
    Ok(())
}

pub fn write_trace_archive_record(
    fixtures_dir: &Path,
    archive: &EvalTraceArchiveRecord,
) -> Result<PathBuf> {
    let path = corpus::trace_archive_dir(fixtures_dir).join(format!("{}.json", archive.trace_id));
    corpus::write_json_pretty(&path, archive)?;
    Ok(path)
}

pub fn load_trace_archive_record(
    fixtures_dir: &Path,
    trace_id: &str,
) -> Result<EvalTraceArchiveRecord> {
    let path = corpus::trace_archive_dir(fixtures_dir).join(format!("{trace_id}.json"));
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn append_curated_trace_case(
    fixtures_dir: &Path,
    case: &EvalCuratedTraceCase,
) -> Result<PathBuf> {
    let path = corpus::curated_trace_cases_path(fixtures_dir);
    corpus::append_jsonl_row(&path, case)?;
    Ok(path)
}

pub fn load_curated_trace_cases(fixtures_dir: &Path) -> Result<Vec<EvalCuratedTraceCase>> {
    corpus::load_jsonl(
        &corpus::curated_trace_cases_path(fixtures_dir),
        &corpus::curated_trace_cases_schema_path(fixtures_dir),
    )
}

fn build_redaction_context(archive: &EvalTraceArchiveRecord) -> RedactionContext {
    let mut exact_values = vec![
        ExactSensitiveValue {
            label: "user_id",
            value: archive.audit_event.user_id.clone(),
        },
        ExactSensitiveValue {
            label: "username",
            value: archive.audit_event.username.clone(),
        },
    ];
    for chunk in &archive.audit_event.grounding_chunks {
        if let Some(owner_user_id) = chunk.owner_user_id.as_ref() {
            exact_values.push(ExactSensitiveValue {
                label: "owner_user_id",
                value: owner_user_id.clone(),
            });
        }
        if let Some(source_id) = chunk.source_id.as_ref() {
            exact_values.push(ExactSensitiveValue {
                label: "source_id",
                value: source_id.clone(),
            });
        }
        if let Some(source_sub_id) = chunk.source_sub_id.as_ref() {
            exact_values.push(ExactSensitiveValue {
                label: "source_sub_id",
                value: source_sub_id.clone(),
            });
        }
        if let Some(citation) = chunk.citation.as_ref() {
            exact_values.push(ExactSensitiveValue {
                label: "citation_source_id",
                value: citation.source_id.clone(),
            });
            if let Some(source_sub_id) = citation.source_sub_id.as_ref() {
                exact_values.push(ExactSensitiveValue {
                    label: "citation_source_sub_id",
                    value: source_sub_id.clone(),
                });
            }
        }
    }
    exact_values.retain(|value| !value.value.trim().is_empty());
    RedactionContext { exact_values }
}

fn derive_access_boundary(chunks: &[AiGroundingChunk], user_role: &str) -> EvalTraceAccessBoundary {
    let highest_visibility = chunks
        .iter()
        .map(|chunk| chunk.visibility)
        .max_by_key(|visibility| match visibility {
            AiGroundingVisibility::Shared => 0,
            AiGroundingVisibility::User => 1,
            AiGroundingVisibility::Admin => 2,
        });

    match highest_visibility {
        Some(AiGroundingVisibility::Admin) => EvalTraceAccessBoundary::AdminOnly,
        Some(AiGroundingVisibility::User) => EvalTraceAccessBoundary::Private,
        Some(AiGroundingVisibility::Shared) => EvalTraceAccessBoundary::Workspace,
        None if user_role.eq_ignore_ascii_case("admin") => EvalTraceAccessBoundary::AdminOnly,
        None => EvalTraceAccessBoundary::Private,
    }
}

fn derive_sensitivity(access_boundary: EvalTraceAccessBoundary) -> EvalTraceSensitivity {
    match access_boundary {
        EvalTraceAccessBoundary::Public => EvalTraceSensitivity::Low,
        EvalTraceAccessBoundary::Workspace => EvalTraceSensitivity::Medium,
        EvalTraceAccessBoundary::Private => EvalTraceSensitivity::High,
        EvalTraceAccessBoundary::AdminOnly => EvalTraceSensitivity::Restricted,
    }
}

fn build_initial_calibration_queue(
    archive: &EvalTraceArchiveRecord,
    release_relevant: bool,
    sensitivity: EvalTraceSensitivity,
    assigned_reviewers: Vec<String>,
) -> Option<EvalTraceCalibrationQueue> {
    let mut reasons = Vec::new();
    if release_relevant {
        reasons.push(EvalTraceCalibrationQueueReason::ReleaseRelevant);
    }
    match sensitivity {
        EvalTraceSensitivity::High => {
            reasons.push(EvalTraceCalibrationQueueReason::HighSensitivity)
        }
        EvalTraceSensitivity::Restricted => {
            reasons.push(EvalTraceCalibrationQueueReason::RestrictedSensitivity)
        }
        EvalTraceSensitivity::Low | EvalTraceSensitivity::Medium => {}
    }
    if archive.audit_event.error_message.is_some()
        || !archive
            .turn_journal
            .status
            .eq_ignore_ascii_case("completed")
    {
        reasons.push(EvalTraceCalibrationQueueReason::FailedTrace);
    }
    reasons.sort();
    reasons.dedup();

    if reasons.is_empty() {
        None
    } else {
        Some(EvalTraceCalibrationQueue {
            status: EvalTraceCalibrationQueueStatus::Queued,
            reasons,
            assigned_reviewers,
            reviewer_decisions: Vec::new(),
            consensus_label: None,
        })
    }
}

fn build_intermediate_events(
    archive: &EvalTraceArchiveRecord,
    context: &RedactionContext,
) -> Result<(Vec<Value>, SanitizationOutcome)> {
    let mut outcome = SanitizationOutcome {
        content_redacted: false,
    };
    let planner = sanitize_json_value(archive.audit_event.planner.clone(), context, &mut outcome);
    let executed_tools = sanitize_json_value(
        serde_json::to_value(&archive.audit_event.executed_tools)?,
        context,
        &mut outcome,
    );
    let grounding_sources = sanitize_json_value(
        serde_json::to_value(&archive.audit_event.grounding_sources)?,
        context,
        &mut outcome,
    );
    let grounding_chunks = sanitize_json_value(
        serde_json::to_value(
            &archive
                .audit_event
                .grounding_chunks
                .iter()
                .map(|chunk| redact_grounding_chunk(chunk, context, &mut outcome))
                .collect::<Vec<_>>(),
        )?,
        context,
        &mut outcome,
    );
    let turn_journal = sanitize_json_value(
        serde_json::to_value(&archive.turn_journal)?,
        context,
        &mut outcome,
    );

    Ok((
        vec![
            json!({"kind": "planner", "value": planner}),
            json!({"kind": "executed_tools", "value": executed_tools}),
            json!({"kind": "grounding_sources", "value": grounding_sources}),
            json!({"kind": "grounding_chunks", "value": grounding_chunks}),
            json!({"kind": "turn_journal", "value": turn_journal}),
        ],
        outcome,
    ))
}

fn sanitize_history(
    turns: &[EvalTraceConversationTurn],
    context: &RedactionContext,
) -> (Vec<EvalTraceConversationTurn>, SanitizationOutcome) {
    let mut outcome = SanitizationOutcome {
        content_redacted: false,
    };
    let turns = turns
        .iter()
        .map(|turn| {
            let (content, turn_outcome) = sanitize_text(&turn.content, context);
            outcome = outcome.combine(turn_outcome);
            EvalTraceConversationTurn {
                role: turn.role.clone(),
                content,
            }
        })
        .collect::<Vec<_>>();
    (turns, outcome)
}

fn sanitize_reference(
    answer: Option<&str>,
    notes: Option<&str>,
    context: &RedactionContext,
) -> (Option<EvalTraceReference>, SanitizationOutcome) {
    let mut outcome = SanitizationOutcome {
        content_redacted: false,
    };
    let Some(answer) = answer else {
        return (None, outcome);
    };
    let (answer, answer_outcome) = sanitize_text(answer, context);
    outcome = outcome.combine(answer_outcome);
    let notes = notes.map(|value| {
        let (sanitized, notes_outcome) = sanitize_text(value, context);
        outcome = outcome.combine(notes_outcome);
        sanitized
    });

    (Some(EvalTraceReference { answer, notes }), outcome)
}

fn redact_grounding_chunk(
    chunk: &AiGroundingChunk,
    context: &RedactionContext,
    outcome: &mut SanitizationOutcome,
) -> AiGroundingChunk {
    let (title, title_outcome) = sanitize_text(&chunk.title, context);
    *outcome = outcome.combine(title_outcome);
    let (excerpt, excerpt_outcome) = sanitize_text(&chunk.excerpt, context);
    *outcome = outcome.combine(excerpt_outcome);

    AiGroundingChunk {
        id: chunk.id.clone(),
        source_kind: chunk.source_kind.clone(),
        title,
        excerpt,
        score: chunk.score,
        visibility: chunk.visibility,
        topic_key: chunk.topic_key.clone(),
        owner_user_id: chunk
            .owner_user_id
            .as_ref()
            .map(|value| hash_identifier(value)),
        source_id: chunk.source_id.as_ref().map(|value| hash_identifier(value)),
        source_sub_id: chunk
            .source_sub_id
            .as_ref()
            .map(|value| hash_identifier(value)),
        citation: chunk
            .citation
            .as_ref()
            .map(|citation| redact_citation(citation, context, outcome)),
    }
}

fn redact_citation(
    citation: &AiGroundingCitation,
    context: &RedactionContext,
    outcome: &mut SanitizationOutcome,
) -> AiGroundingCitation {
    let (label, label_outcome) = citation
        .label
        .as_deref()
        .map(|value| sanitize_text(value, context))
        .unwrap_or((
            String::new(),
            SanitizationOutcome {
                content_redacted: false,
            },
        ));
    *outcome = outcome.combine(label_outcome);
    let (excerpt, excerpt_outcome) = citation
        .excerpt
        .as_deref()
        .map(|value| sanitize_text(value, context))
        .unwrap_or((
            String::new(),
            SanitizationOutcome {
                content_redacted: false,
            },
        ));
    *outcome = outcome.combine(excerpt_outcome);

    AiGroundingCitation {
        citation_id: citation.citation_id.clone(),
        source_kind: citation.source_kind.clone(),
        source_id: hash_identifier(&citation.source_id),
        source_sub_id: citation
            .source_sub_id
            .as_ref()
            .map(|value| hash_identifier(value)),
        label: citation
            .label
            .as_ref()
            .map(|_| label)
            .filter(|value| !value.is_empty()),
        excerpt: citation
            .excerpt
            .as_ref()
            .map(|_| excerpt)
            .filter(|value| !value.is_empty()),
        started_ts_ms: citation.started_ts_ms,
        ended_ts_ms: citation.ended_ts_ms,
        url: citation.url.as_ref().map(|value| {
            let (sanitized, url_outcome) = sanitize_text(value, context);
            *outcome = outcome.combine(url_outcome);
            sanitized
        }),
    }
}

fn sanitize_json_value(
    value: Value,
    context: &RedactionContext,
    outcome: &mut SanitizationOutcome,
) -> Value {
    match value {
        Value::String(text) => {
            let (sanitized, sanitized_outcome) = sanitize_text(&text, context);
            *outcome = outcome.combine(sanitized_outcome);
            Value::String(sanitized)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| sanitize_json_value(item, context, outcome))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, sanitize_json_value(value, context, outcome)))
                .collect(),
        ),
        other => other,
    }
}

fn sanitize_text(text: &str, context: &RedactionContext) -> (String, SanitizationOutcome) {
    let mut value = text.to_string();
    let mut changed = false;

    for exact in &context.exact_values {
        if exact.value.is_empty() || !value.contains(&exact.value) {
            continue;
        }
        value = value.replace(
            &exact.value,
            &format!("<hashed:{}:{}>", exact.label, hash_identifier(&exact.value)),
        );
        changed = true;
    }

    for (regex, label) in redaction_patterns() {
        let next = regex
            .replace_all(&value, |captures: &Captures<'_>| {
                changed = true;
                format!(
                    "<redacted:{}:{}>",
                    label,
                    hash_identifier(captures.get(0).map(|m| m.as_str()).unwrap_or_default())
                )
            })
            .into_owned();
        value = next;
    }

    (
        value,
        SanitizationOutcome {
            content_redacted: changed,
        },
    )
}

fn redaction_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b")
                    .expect("email regex"),
                "email",
            ),
            (
                Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._\-=/+]+\b").expect("bearer regex"),
                "bearer_token",
            ),
            (
                Regex::new(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b")
                    .expect("jwt regex"),
                "jwt",
            ),
            (
                Regex::new(r"https?://[^\s)]+").expect("url regex"),
                "url",
            ),
            (
                Regex::new(
                    r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}\b",
                )
                .expect("uuid regex"),
                "uuid",
            ),
        ]
    })
}

fn normalize_mode(mode: &str) -> Result<String> {
    let normalized = mode.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "instant" | "thinking" | "extended" => Ok(normalized),
        other => bail!("unsupported trace mode: {other}"),
    }
}

fn hash_identifier(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        EvalCuratedTraceCase, EvalTraceAccessBoundary, EvalTraceCalibrationQueueReason,
        EvalTraceCalibrationQueueStatus, EvalTraceConsensusLabel, EvalTraceConversationTurn,
        EvalTraceCurationRequest, EvalTraceDifficulty, EvalTraceImportSource,
        EvalTraceReviewStatus, append_curated_trace_case, approve_curated_trace_case,
        archive_trace_source, assert_trace_case_release_ready, curate_trace_archive_record,
        load_curated_trace_cases, load_trace_archive_record, queue_trace_calibration_review,
        resolve_trace_calibration_consensus, write_trace_archive_record,
    };
    use crate::ai_audit::{
        AiAssistantAuditEventResponse, AiAssistantAuditGroundingSource,
        AiAssistantAuditToolExecution, AiGroundingChunk, AiGroundingCitation,
        AiGroundingVisibility,
    };
    use crate::ai_turn_journal::AiTurnJournalSummary;

    fn sample_source() -> EvalTraceImportSource {
        EvalTraceImportSource {
            audit_event: AiAssistantAuditEventResponse {
                id: "audit-1".to_string(),
                trace_id: "trace-123".to_string(),
                user_id: "user-42".to_string(),
                username: "alice@example.com".to_string(),
                user_role: "user".to_string(),
                model_name: "fixture-model".to_string(),
                message_preview: "help".to_string(),
                history_len: 1,
                response_kind: "completed".to_string(),
                planner: json!({"query":"alice@example.com", "token":"Bearer secret-token-123"}),
                model_routing: Vec::new(),
                planned_tools: vec!["dictionary_list_visible_workspaces".to_string()],
                executed_tools: vec![AiAssistantAuditToolExecution {
                    tool: "dictionary_list_visible_workspaces".to_string(),
                    input_summary: "query=alice@example.com".to_string(),
                    status: "ok".to_string(),
                    label: "Visible workspaces".to_string(),
                    result_count: Some(2),
                }],
                grounding_chunks: vec![AiGroundingChunk {
                    id: "chunk-1".to_string(),
                    source_kind: "dictionary".to_string(),
                    title: "Rachel alice@example.com".to_string(),
                    excerpt: "Contact alice@example.com at https://private.example/path?token=abc"
                        .to_string(),
                    score: 0.9,
                    visibility: AiGroundingVisibility::User,
                    topic_key: Some("person.rachel".to_string()),
                    owner_user_id: Some("user-42".to_string()),
                    source_id: Some("person-123".to_string()),
                    source_sub_id: Some("fact-456".to_string()),
                    citation: Some(AiGroundingCitation {
                        citation_id: "citation-1".to_string(),
                        source_kind: "dictionary".to_string(),
                        source_id: "person-123".to_string(),
                        source_sub_id: Some("fact-456".to_string()),
                        label: Some("alice@example.com".to_string()),
                        excerpt: Some("Bearer secret-token-123".to_string()),
                        started_ts_ms: None,
                        ended_ts_ms: None,
                        url: Some("https://private.example/path?token=abc".to_string()),
                    }),
                }],
                grounding_sources: vec![AiAssistantAuditGroundingSource {
                    tool: "dictionary_list_visible_workspaces".to_string(),
                    label: "https://private.example/path?token=abc".to_string(),
                    access_mode: "read".to_string(),
                    risk_tier: "low".to_string(),
                    status: "ok".to_string(),
                }],
                error_message: None,
                created_ts: 123,
            },
            turn_journal: AiTurnJournalSummary {
                id: "journal-1".to_string(),
                user_id: "user-42".to_string(),
                conversation_id: Some("conversation-1".to_string()),
                request_turn_id: Some("turn-1".to_string()),
                request_turn_index: Some(1),
                trace_id: "trace-123".to_string(),
                request_message: "Message alice@example.com with Bearer secret-token-123"
                    .to_string(),
                model_name: "fixture-model".to_string(),
                response_mode: "thinking".to_string(),
                planner_mode: Some("structured".to_string()),
                status: "completed".to_string(),
                current_phase: "completed".to_string(),
                history_len: 1,
                planner_debug: Default::default(),
                prompt_debug: None,
                stats: None,
                overload_reason: None,
                error_message: None,
                compact_boundary_count: 0,
                artifact_verification: None,
                created_ts: 123,
                updated_ts: 124,
                finished_ts: Some(124),
            },
            assistant_response:
                "I found Rachel. Her contact is alice@example.com and the token is Bearer secret-token-123.".to_string(),
            conversation_history: vec![EvalTraceConversationTurn {
                role: "user".to_string(),
                content: "Earlier message from alice@example.com".to_string(),
            }],
            imported_ts: Some(555),
        }
    }

    fn curation_request() -> EvalTraceCurationRequest {
        EvalTraceCurationRequest {
            case_id: "trace-case-1".to_string(),
            domain: "dictionary".to_string(),
            intent: "person_lookup".to_string(),
            difficulty: EvalTraceDifficulty::Medium,
            expected_answer_shape: "direct_short_answer".to_string(),
            hard_gates: vec![
                "acl_privacy_boundary".to_string(),
                "no_raw_json".to_string(),
            ],
            must_include: vec!["Rachel".to_string()],
            must_not_include: vec!["raw_json".to_string()],
            max_words: Some(40),
            requires_citation: false,
            release_relevant: true,
            reference_answer: Some("Rachel is visible in the dictionary.".to_string()),
            reference_notes: Some("Do not leak private contact info.".to_string()),
            baseline_model_response: Some("Rachel appears in the visible dictionary.".to_string()),
            assigned_reviewers: vec!["reviewer-1".to_string(), "reviewer-2".to_string()],
        }
    }

    #[test]
    fn curated_case_redacts_private_material_and_hashes_provenance() {
        let archive = archive_trace_source(sample_source());
        let curated = curate_trace_archive_record(&archive, &curation_request()).unwrap();

        assert_eq!(
            curated.metadata.access_boundary,
            EvalTraceAccessBoundary::Private
        );
        assert_eq!(curated.metadata.review_status, EvalTraceReviewStatus::Draft);
        assert!(curated.prompt.contains("<hashed:username:"));
        assert!(curated.prompt.contains("<redacted:bearer_token:"));
        assert!(!curated.prompt.contains("alice@example.com"));
        assert!(!curated.observed_response.contains("alice@example.com"));
        assert!(
            !serde_json::to_string(&curated.intermediate_events)
                .unwrap()
                .contains("secret-token-123")
        );
        assert_ne!(curated.source.source_user_id_hash, "user-42");
        assert_ne!(curated.source.source_username_hash, "alice@example.com");
        assert!(matches!(
            curated
                .review
                .calibration
                .as_ref()
                .map(|queue| queue.status),
            Some(EvalTraceCalibrationQueueStatus::Queued)
        ));
    }

    #[test]
    fn release_ready_requires_approval_and_resolved_calibration() {
        let archive = archive_trace_source(sample_source());
        let mut curated = curate_trace_archive_record(&archive, &curation_request()).unwrap();

        let error = assert_trace_case_release_ready(&curated).unwrap_err();
        assert!(error.to_string().contains("not approved"));

        approve_curated_trace_case(&mut curated, "reviewer-1", Some("looks good"), Some(777));
        let error = assert_trace_case_release_ready(&curated).unwrap_err();
        assert!(error.to_string().contains("unresolved calibration queue"));

        resolve_trace_calibration_consensus(
            &mut curated,
            EvalTraceConsensusLabel {
                label: "pass".to_string(),
                score: Some(0.91),
                reviewers: vec!["reviewer-1".to_string(), "reviewer-2".to_string()],
                notes: Some("Consensus reached.".to_string()),
            },
            vec![],
        );
        assert_trace_case_release_ready(&curated).unwrap();
    }

    #[test]
    fn raw_archive_and_curated_outputs_are_kept_separate() {
        let dir = tempdir().unwrap();
        let archive = archive_trace_source(sample_source());
        let curated = curate_trace_archive_record(&archive, &curation_request()).unwrap();

        let raw_path = write_trace_archive_record(dir.path(), &archive).unwrap();
        let curated_path = append_curated_trace_case(dir.path(), &curated).unwrap();
        let loaded_archive = load_trace_archive_record(dir.path(), &archive.trace_id).unwrap();
        assert_ne!(raw_path, curated_path);
        assert!(raw_path.to_string_lossy().contains("trace_archive"));
        assert!(curated_path.ends_with("trace_curated_cases.jsonl"));

        let raw_content = std::fs::read_to_string(&raw_path).unwrap();
        let curated_content = std::fs::read_to_string(&curated_path).unwrap();
        assert!(raw_content.contains("alice@example.com"));
        assert!(!curated_content.contains("alice@example.com"));
        assert_eq!(loaded_archive.trace_id, "trace-123");
        assert_eq!(loaded_archive.audit_event.username, "alice@example.com");
    }

    #[test]
    fn curated_rows_validate_against_schema_and_draft_cases_are_not_release_ready() {
        let dir = tempdir().unwrap();
        let fixture_dir = dir.path();
        let schema_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/ai/trace_curated_cases.schema.json");
        let schema_dst = fixture_dir.join("trace_curated_cases.schema.json");
        std::fs::copy(&schema_src, &schema_dst).unwrap();

        let archive = archive_trace_source(sample_source());
        let curated = curate_trace_archive_record(&archive, &curation_request()).unwrap();
        append_curated_trace_case(fixture_dir, &curated).unwrap();

        let loaded: Vec<EvalCuratedTraceCase> = load_curated_trace_cases(fixture_dir).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(assert_trace_case_release_ready(&loaded[0]).is_err());
    }

    #[test]
    fn manual_calibration_queue_can_be_reopened_and_resolved() {
        let archive = archive_trace_source(sample_source());
        let mut curated = curate_trace_archive_record(&archive, &curation_request()).unwrap();
        queue_trace_calibration_review(
            &mut curated,
            vec![EvalTraceCalibrationQueueReason::FailedTrace],
            vec!["reviewer-3".to_string()],
        );
        assert_eq!(
            curated
                .review
                .calibration
                .as_ref()
                .map(|queue| queue.status),
            Some(EvalTraceCalibrationQueueStatus::Queued)
        );
        resolve_trace_calibration_consensus(
            &mut curated,
            EvalTraceConsensusLabel {
                label: "needs_revision".to_string(),
                score: None,
                reviewers: vec!["reviewer-3".to_string()],
                notes: None,
            },
            vec![],
        );
        assert_eq!(
            curated
                .review
                .calibration
                .as_ref()
                .and_then(|queue| queue.consensus_label.as_ref())
                .map(|label| label.label.as_str()),
            Some("needs_revision")
        );
    }
}
