use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct LibrariesToolProvider;

impl ToolProvider for LibrariesToolProvider {
    fn provider_id(&self) -> &'static str {
        "libraries"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::LibrariesListAccessible);
        registry.register_tool(self, AssistantToolName::LibrarySearchTitles);
        registry.register_tool(self, AssistantToolName::LibraryGetItemSummary);
        registry.register_tool(self, AssistantToolName::LibrariesGetRecentlyAdded);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_libraries_provider_tool(state, context, call).await })
    }
}
