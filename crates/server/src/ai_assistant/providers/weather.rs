use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct WeatherToolProvider;

impl ToolProvider for WeatherToolProvider {
    fn provider_id(&self) -> &'static str {
        "weather"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::WeatherGetCurrent);
        registry.register_tool(self, AssistantToolName::WeatherGetForecast);
        registry.register_tool(self, AssistantToolName::WeatherGetHistory);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_weather_provider_tool(state, context, call).await })
    }
}
