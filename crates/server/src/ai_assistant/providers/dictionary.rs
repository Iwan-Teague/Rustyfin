use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct DictionaryToolProvider;

impl ToolProvider for DictionaryToolProvider {
    fn provider_id(&self) -> &'static str {
        "dictionary"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::DictionaryGetAccountIdentity);
        registry.register_tool(self, AssistantToolName::DictionarySearchPeople);
        registry.register_tool(self, AssistantToolName::DictionaryGetPersonBundle);
        registry.register_tool(
            self,
            AssistantToolName::DictionaryResolveRelationshipReference,
        );
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_dictionary_provider_tool(state, context, call).await })
    }
}
