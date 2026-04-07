use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct DownloadsToolProvider;

impl ToolProvider for DownloadsToolProvider {
    fn provider_id(&self) -> &'static str {
        "downloads"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::DownloadsListAvailableArtifacts);
        registry.register_tool(self, AssistantToolName::DownloadsGetArtifactDetails);
        registry.register_tool(self, AssistantToolName::DownloadsGetArtifactSource);
        registry.register_tool(self, AssistantToolName::DownloadsGetReleaseNotes);
        registry.register_tool(self, AssistantToolName::DownloadsGetArtifactChecksum);
        registry.register_tool(self, AssistantToolName::DownloadsGetArtifactInstallSteps);
        registry.register_tool(self, AssistantToolName::DownloadsGetArtifactCompatibility);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_downloads_provider_tool(state, context, call).await })
    }
}
