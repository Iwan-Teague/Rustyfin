use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct ChannelsToolProvider;

impl ToolProvider for ChannelsToolProvider {
    fn provider_id(&self) -> &'static str {
        "channels"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::ChannelsListUnreadActivity);
        registry.register_tool(self, AssistantToolName::ChannelsGetTranscriptSummary);
        registry.register_tool(self, AssistantToolName::ChannelsListVoiceTranscripts);
        registry.register_tool(self, AssistantToolName::ChannelsReadVoiceTranscript);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_channels_provider_tool(state, context, call).await })
    }
}
