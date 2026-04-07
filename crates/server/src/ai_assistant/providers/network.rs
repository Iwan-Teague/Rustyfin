use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::provider::{ToolExecutionFuture, ToolProvider, ToolRegistryBuilder};
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::tools;
use crate::ai_assistant::types::PlannedToolCall;
use crate::state::AppState;

#[derive(Debug, Default)]
pub struct NetworkToolProvider;

impl ToolProvider for NetworkToolProvider {
    fn provider_id(&self) -> &'static str {
        "network"
    }

    fn register(&self, registry: &mut ToolRegistryBuilder) {
        registry.register_tool(self, AssistantToolName::NetworkGetTopologySummary);
        registry.register_tool(self, AssistantToolName::NetworkGetInterfaceDetails);
        registry.register_tool(self, AssistantToolName::NetworkGetDefaultRoute);
        registry.register_tool(self, AssistantToolName::NetworkGetHostnameAliases);
        registry.register_tool(self, AssistantToolName::NetworkGetDnsServers);
        registry.register_tool(self, AssistantToolName::NetworkGetRouteTable);
        registry.register_tool(self, AssistantToolName::NetworkGetActiveConnections);
        registry.register_tool(self, AssistantToolName::NetworkGetInterfaceCounters);
        registry.register_tool(self, AssistantToolName::NetworkGetWifiStatus);
        registry.register_tool(self, AssistantToolName::NetworkGetVpnStatus);
    }

    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a PlannedToolCall,
    ) -> ToolExecutionFuture<'a> {
        Box::pin(async move { tools::execute_network_provider_tool(state, context, call).await })
    }
}
