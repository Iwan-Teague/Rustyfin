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
        registry.register_tool(self, AssistantToolName::SystemGetServiceDetail);
        registry.register_tool(self, AssistantToolName::SystemGetServiceLogs);
        registry.register_tool(self, AssistantToolName::SystemGetServiceDependencies);
        registry.register_tool(self, AssistantToolName::SystemGetTranscodeSummary);
        registry.register_tool(self, AssistantToolName::SystemGetStorageSummary);
        registry.register_tool(self, AssistantToolName::SystemGetStoragePathDetail);
        registry.register_tool(self, AssistantToolName::SystemGetMountDetail);
        registry.register_tool(self, AssistantToolName::SystemGetRecentErrors);
        registry.register_tool(self, AssistantToolName::SystemGetKernelInfo);
        registry.register_tool(self, AssistantToolName::SystemGetCpuTopology);
        registry.register_tool(self, AssistantToolName::SystemGetTemperatureSensors);
        registry.register_tool(self, AssistantToolName::SystemGetBlockDeviceInventory);
        registry.register_tool(self, AssistantToolName::SystemGetFilesystemTable);
        registry.register_tool(self, AssistantToolName::SystemGetGpuInventory);
        registry.register_tool(self, AssistantToolName::SystemGetPciDevices);
        registry.register_tool(self, AssistantToolName::SystemGetUsbDevices);
        registry.register_tool(self, AssistantToolName::SystemGetBootLogSummary);
        registry.register_tool(self, AssistantToolName::SystemGetJournalSummary);
        registry.register_tool(self, AssistantToolName::SystemGetProcessDetail);
        registry.register_tool(self, AssistantToolName::SystemGetListenerDetail);
        registry.register_tool(self, AssistantToolName::SystemGetDiskUsageDetail);
        registry.register_tool(self, AssistantToolName::SystemGetPortConflicts);
        registry.register_tool(self, AssistantToolName::SystemGetPortConflictDetail);
        registry.register_tool(self, AssistantToolName::SystemGetFailedUnits);
        registry.register_tool(self, AssistantToolName::SystemGetFailedUnitDetail);
        registry.register_tool(self, AssistantToolName::SystemGetFailedServiceLogs);
        registry.register_tool(self, AssistantToolName::SystemGetProcessTreeDetail);
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
