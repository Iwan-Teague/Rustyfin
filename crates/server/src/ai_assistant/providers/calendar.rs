use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct CalendarToolProvider;

impl ToolProvider for CalendarToolProvider {
    fn provider_id(&self) -> &'static str {
        "calendar"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::CalendarListEvents);
        registry.register_tool(self, AssistantToolName::CalendarGetNextEvent);
        registry.register_tool(self, AssistantToolName::CalendarUpcomingBirthdays);
        registry.register_tool(self, AssistantToolName::CalendarGetEventDetails);
        registry.register_tool(self, AssistantToolName::CalendarCreateEvent);
        registry.register_tool(self, AssistantToolName::CalendarCreateBirthday);
        registry.register_tool(self, AssistantToolName::CalendarDeleteEvent);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_calendar_provider_tool(state, context, call).await })
    }
}
