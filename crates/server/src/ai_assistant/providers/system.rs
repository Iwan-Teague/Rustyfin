use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct SystemToolProvider;

impl ToolProvider for SystemToolProvider {
    fn provider_id(&self) -> &'static str {
        "system"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::SystemGetCurrentDateTime);
        registry.register_tool(self, AssistantToolName::SystemGetAiRuntimeSummary);
        registry.register_tool(self, AssistantToolName::SystemGetHostRuntimeSummary);
        registry.register_tool(self, AssistantToolName::SystemGetBackupSummary);
        registry.register_tool(self, AssistantToolName::SystemGetServiceHealth);
        registry.register_tool(self, AssistantToolName::SystemGetTranscodeSummary);
        registry.register_tool(self, AssistantToolName::SystemGetStorageSummary);
        registry.register_tool(self, AssistantToolName::SystemGetRecentErrors);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_system_provider_tool(state, context, call).await })
    }
}
