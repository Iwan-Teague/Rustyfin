use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct MemoryToolProvider;

impl ToolProvider for MemoryToolProvider {
    fn provider_id(&self) -> &'static str {
        "memory"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::MemoryListRecentFacts);
        registry.register_tool(self, AssistantToolName::MemoryListRecentEntities);
        registry.register_tool(self, AssistantToolName::MemorySearchFacts);
        registry.register_tool(self, AssistantToolName::MemorySearchEntities);
        registry.register_tool(self, AssistantToolName::MemoryFindExactEntity);
        registry.register_tool(self, AssistantToolName::MemoryGetEntityRelations);
        registry.register_tool(self, AssistantToolName::MemoryGetEntityRelationPath);
        registry.register_tool(self, AssistantToolName::MemoryGetPersonSummary);
        registry.register_tool(self, AssistantToolName::MemoryListRecentChanges);
        registry.register_tool(self, AssistantToolName::MemoryListConflictingFacts);
        registry.register_tool(self, AssistantToolName::MemoryGetEntityProvenance);
        registry.register_tool(self, AssistantToolName::MemoryGetPersonTimeline);
        registry.register_tool(self, AssistantToolName::MemoryGetSourceCitation);
        registry.register_tool(self, AssistantToolName::MemoryGetConflictExplanations);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_memory_provider_tool(state, context, call).await })
    }
}
