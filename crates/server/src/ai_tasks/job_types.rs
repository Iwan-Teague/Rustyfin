use std::path::PathBuf;

use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::memory::build_grounding_chunks_for_turn;
use crate::ai_assistant::orchestrator::plan_tool_calls_with_history;
use crate::ai_assistant::provider::ToolExecutionProfile;
use crate::ai_assistant::replies::grounding_chunks_prompt;
use crate::ai_assistant::tools::{execute_tool_with_profile, source_from_block};
use crate::ai_assistant::types::{
    AssistantChatRequest, AssistantGroundingChunk, AssistantGroundingSource,
    AssistantHistoryMessage, AssistantResponseMode, AssistantToolContextBlock, PlannedToolCall,
};
use crate::auth::AuthUser;
use crate::state::AppState;

use super::types::{AiTaskArtifactFormat, TaskUserContext};

#[derive(Debug, Clone)]
pub struct GroundedTaskExecution {
    pub planned_tools: Vec<PlannedToolCall>,
    pub grounding_blocks: Vec<AssistantToolContextBlock>,
    pub grounding_sources: Vec<AssistantGroundingSource>,
    pub grounding_chunks: Vec<AssistantGroundingChunk>,
}

pub fn task_auth_user(user: &TaskUserContext) -> AuthUser {
    AuthUser {
        user_id: user.user_id.clone(),
        username: user.username.clone(),
        role: user.role.clone(),
    }
}

pub fn assistant_context_for_task(user: &TaskUserContext, task_id: &str) -> AssistantContext {
    let auth_user = task_auth_user(user);
    AssistantContext::new(&auth_user, format!("ai-task-{task_id}"))
}

pub async fn effective_models_for_task(
    state: &AppState,
    requested_model: Option<&str>,
) -> (Option<String>, Option<String>) {
    let engine = state.engine.lock().await;
    let answer_model = engine
        .role_routing
        .iter()
        .find(|decision| decision.role.eq_ignore_ascii_case("answer"))
        .map(|decision| decision.model_name.clone())
        .or_else(|| requested_model.map(str::to_string));
    let planner_model = engine
        .role_routing
        .iter()
        .find(|decision| decision.role.eq_ignore_ascii_case("planner"))
        .map(|decision| decision.model_name.clone())
        .or_else(|| requested_model.map(str::to_string));
    (answer_model, planner_model)
}

pub async fn run_grounded_query(
    state: &AppState,
    task_id: &str,
    user: &TaskUserContext,
    message: &str,
    history: &[AssistantHistoryMessage],
    profile: &ToolExecutionProfile,
) -> GroundedTaskExecution {
    let context = assistant_context_for_task(user, task_id);
    let planned_tools = plan_tool_calls_with_history(message, history)
        .into_iter()
        .filter(|call| profile.denial_reason(call.tool, call.tool.spec()).is_none())
        .take(profile.max_tool_calls)
        .collect::<Vec<_>>();

    let mut grounding_blocks = Vec::with_capacity(planned_tools.len());
    let mut grounding_sources = Vec::with_capacity(planned_tools.len());

    for call in &planned_tools {
        let block = execute_tool_with_profile(state, &context, call, profile).await;
        grounding_sources.push(source_from_block(call.tool, &block));
        grounding_blocks.push(block);
    }

    let grounding_chunks = build_grounding_chunks_for_turn(
        state,
        &context,
        &AssistantChatRequest {
            model: String::new(),
            message: message.to_string(),
            response_mode: AssistantResponseMode::Extended,
            confirmation_token: None,
            history: history.to_vec(),
        },
        &planned_tools,
        &grounding_blocks,
        &grounding_sources,
        history,
    )
    .await;

    GroundedTaskExecution {
        planned_tools,
        grounding_blocks,
        grounding_sources,
        grounding_chunks,
    }
}

pub fn render_document(
    title: &str,
    prompt: &str,
    format: AiTaskArtifactFormat,
    chunks: &[AssistantGroundingChunk],
) -> String {
    match format {
        AiTaskArtifactFormat::Markdown => render_markdown_document(title, prompt, chunks),
        AiTaskArtifactFormat::Text => render_text_document(title, prompt, chunks),
    }
}

fn render_markdown_document(
    title: &str,
    prompt: &str,
    chunks: &[AssistantGroundingChunk],
) -> String {
    let mut lines = vec![
        format!("# {title}"),
        String::new(),
        format!("Request: {prompt}"),
        String::new(),
        "## Findings".to_string(),
    ];

    if chunks.is_empty() {
        lines.push("- No grounded sources were available for this request.".to_string());
    } else {
        for chunk in chunks {
            lines.push(format!(
                "- [{}] {}: {}",
                chunk.id, chunk.title, chunk.excerpt
            ));
        }
    }

    lines.push(String::new());
    lines.push("## Sources".to_string());
    if chunks.is_empty() {
        lines.push("- None".to_string());
    } else {
        for chunk in chunks {
            lines.push(format!(
                "- [{}] {} ({})",
                chunk.id, chunk.title, chunk.source_kind
            ));
        }
    }

    lines.join("\n")
}

fn render_text_document(title: &str, prompt: &str, chunks: &[AssistantGroundingChunk]) -> String {
    let mut sections = vec![
        title.to_string(),
        "=".repeat(title.len()),
        String::new(),
        format!("Request: {prompt}"),
        String::new(),
        "Findings:".to_string(),
    ];

    if chunks.is_empty() {
        sections.push("- No grounded sources were available for this request.".to_string());
    } else {
        for chunk in chunks {
            sections.push(format!(
                "- [{}] {}: {}",
                chunk.id, chunk.title, chunk.excerpt
            ));
        }
    }

    sections.push(String::new());
    sections.push("Sources:".to_string());
    if chunks.is_empty() {
        sections.push("- None".to_string());
    } else {
        for chunk in chunks {
            sections.push(format!(
                "- [{}] {} ({})",
                chunk.id, chunk.title, chunk.source_kind
            ));
        }
    }

    sections.join("\n")
}

pub fn verify_document(
    title: &str,
    prompt: &str,
    format: AiTaskArtifactFormat,
    draft: &str,
    chunks: &[AssistantGroundingChunk],
) -> (String, Vec<String>) {
    let mut revised = draft.trim().to_string();
    let mut issues = Vec::new();
    if revised.is_empty() {
        issues.push("draft was empty".to_string());
        revised = render_document(title, prompt, format, chunks);
    }
    if !revised.contains("Sources") {
        issues.push("draft was missing a sources section".to_string());
        revised = render_document(title, prompt, format, chunks);
    }
    (revised, issues)
}

pub fn default_file_name(stem: &str, format: AiTaskArtifactFormat) -> String {
    let normalized = stem
        .trim()
        .to_ascii_lowercase()
        .replace(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'), "-");
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        format!("task-output.{}", format.file_extension())
    } else if normalized.ends_with(&format!(".{}", format.file_extension())) {
        normalized.to_string()
    } else {
        format!("{normalized}.{}", format.file_extension())
    }
}

pub fn split_objective_into_questions(objective: &str, max_workers: usize) -> Vec<String> {
    let mut parts = objective
        .split(['?', ';'])
        .flat_map(|segment| segment.split(" and "))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    parts.dedup();
    if parts.is_empty() {
        parts.push(objective.trim().to_string());
    }
    parts.truncate(max_workers.clamp(1, 4));
    parts
}

pub fn persist_task_artifact_file(
    state: &AppState,
    task_id: &str,
    file_name: &str,
    content: &str,
) -> Result<(PathBuf, i64), String> {
    let artifact_dir = state.cache_dir.join("ai_tasks").join(task_id);
    std::fs::create_dir_all(&artifact_dir)
        .map_err(|e| format!("failed to create ai task artifact directory: {e}"))?;
    let path = artifact_dir.join(file_name);
    std::fs::write(&path, content)
        .map_err(|e| format!("failed to write ai task artifact file: {e}"))?;
    let size_bytes = content.len() as i64;
    Ok((path, size_bytes))
}

pub fn compact_grounding_summary(chunks: &[AssistantGroundingChunk]) -> String {
    if chunks.is_empty() {
        "No grounded source chunks were available.".to_string()
    } else {
        grounding_chunks_prompt(chunks)
    }
}
