use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use super::context::AssistantContext;
use super::providers::default_tool_providers;
use super::registry::AssistantToolName;
use super::types::{
    AssistantDomainFamily, AssistantToolContextBlock, AssistantToolSpec, ToolAccessMode,
};
use crate::state::AppState;

pub type ToolExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = AssistantToolContextBlock> + Send + 'a>>;

pub trait ToolProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn register(&self, registry: &mut ToolRegistryBuilder);
    fn execute<'a>(
        &'a self,
        state: &'a AppState,
        context: &'a AssistantContext,
        call: &'a super::types::PlannedToolCall,
    ) -> ToolExecutionFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct ToolExecutionProfile {
    pub allowed_tools: HashSet<AssistantToolName>,
    pub read_only: bool,
    pub max_tool_calls: usize,
}

impl ToolExecutionProfile {
    pub fn full_access() -> Self {
        Self {
            allowed_tools: AssistantToolName::all().iter().copied().collect(),
            read_only: false,
            max_tool_calls: usize::MAX,
        }
    }

    pub fn restricted<I>(allowed_tools: I, read_only: bool, max_tool_calls: usize) -> Self
    where
        I: IntoIterator<Item = AssistantToolName>,
    {
        Self {
            allowed_tools: allowed_tools.into_iter().collect(),
            read_only,
            max_tool_calls,
        }
    }

    pub fn denial_reason(
        &self,
        tool: AssistantToolName,
        spec: AssistantToolSpec,
    ) -> Option<String> {
        if !self.allowed_tools.contains(&tool) {
            return Some(format!(
                "{} is not available in this assistant execution profile.",
                spec.name
            ));
        }

        if self.read_only && spec.access_mode != ToolAccessMode::ReadOnly {
            return Some(format!(
                "{} is blocked because this assistant execution profile is read-only.",
                spec.name
            ));
        }

        None
    }
}

impl Default for ToolExecutionProfile {
    fn default() -> Self {
        Self::full_access()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ToolRegistryEntry {
    pub tool: AssistantToolName,
    pub spec: AssistantToolSpec,
    pub provider_id: &'static str,
    pub domain_family: AssistantDomainFamily,
    pub recovery_eligible: bool,
    pub can_parallelize: bool,
    pub ambiguity_prone: bool,
    pub freshness_sensitive: bool,
}

#[derive(Default)]
pub struct ToolRegistryBuilder {
    entries: HashMap<AssistantToolName, ToolRegistryEntry>,
}

impl ToolRegistryBuilder {
    pub fn register_tool<P>(&mut self, provider: &P, tool: AssistantToolName)
    where
        P: ToolProvider + ?Sized,
    {
        let entry = ToolRegistryEntry {
            tool,
            spec: tool.spec(),
            provider_id: provider.provider_id(),
            domain_family: tool.domain_family(),
            recovery_eligible: tool.recovery_eligible(),
            can_parallelize: tool.can_parallelize(),
            ambiguity_prone: tool.ambiguity_prone(),
            freshness_sensitive: tool.freshness_sensitive(),
        };
        if self.entries.insert(tool, entry).is_some() {
            panic!("tool {} was registered more than once", tool.as_str());
        }
    }

    fn build(self, providers: Vec<Arc<dyn ToolProvider>>) -> ToolRegistry {
        let providers = providers
            .into_iter()
            .map(|provider| (provider.provider_id(), provider))
            .collect();
        ToolRegistry {
            entries: self.entries,
            providers,
        }
    }
}

pub struct ToolRegistry {
    entries: HashMap<AssistantToolName, ToolRegistryEntry>,
    providers: HashMap<&'static str, Arc<dyn ToolProvider>>,
}

impl ToolRegistry {
    pub fn from_providers(providers: Vec<Arc<dyn ToolProvider>>) -> Self {
        let mut builder = ToolRegistryBuilder::default();
        for provider in &providers {
            provider.register(&mut builder);
        }
        builder.build(providers)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, tool: AssistantToolName) -> Option<&ToolRegistryEntry> {
        self.entries.get(&tool)
    }

    pub fn provider_id_for_tool(&self, tool: AssistantToolName) -> Option<&'static str> {
        self.entry(tool).map(|entry| entry.provider_id)
    }

    pub fn provider_for_tool(&self, tool: AssistantToolName) -> Option<Arc<dyn ToolProvider>> {
        let provider_id = self.provider_id_for_tool(tool)?;
        self.providers.get(provider_id).cloned()
    }
}

pub fn default_tool_registry() -> &'static ToolRegistry {
    static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let registry = ToolRegistry::from_providers(default_tool_providers());
        assert_eq!(
            registry.len(),
            AssistantToolName::all().len(),
            "default tool registry must cover every assistant tool"
        );
        registry
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{ToolExecutionProfile, ToolRegistry, default_tool_registry};
    use crate::ai_assistant::providers::{
        CalendarToolProvider, DictionaryToolProvider, LibrariesToolProvider, MemoryToolProvider,
    };
    use crate::ai_assistant::registry::AssistantToolName;

    #[test]
    fn default_registry_builds_with_all_providers() {
        let registry = default_tool_registry();
        assert_eq!(registry.len(), AssistantToolName::all().len());
        assert!(!registry.is_empty());
    }

    #[test]
    fn tool_lookup_resolves_correct_provider() {
        let registry = default_tool_registry();
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::CalendarGetEventDetails),
            Some("calendar")
        );
        assert_eq!(
            registry
                .provider_id_for_tool(AssistantToolName::DictionaryResolveRelationshipReference),
            Some("dictionary")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::LibrarySearchTitles),
            Some("libraries")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::LibrariesFindDuplicateTitles),
            Some("libraries")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::LibrariesListMissingMetadata),
            Some("libraries")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::DownloadsGetArtifactChecksum),
            Some("downloads")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::DownloadsGetArtifactSource),
            Some("downloads")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::DownloadsGetReleaseNotes),
            Some("downloads")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::DownloadsGetArtifactInstallSteps),
            Some("downloads")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::DownloadsGetArtifactCompatibility),
            Some("downloads")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemorySearchFacts),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemoryListRecentChanges),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemoryListConflictingFacts),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemoryGetEntityProvenance),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemoryListRecentEntities),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemoryGetEntityRelations),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::MemoryGetEntityRelationPath),
            Some("memory")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::LibraryGetItemSourcePaths),
            Some("libraries")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::CalendarGetNextFreeDay),
            Some("calendar")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::CalendarListOverlappingEvents),
            Some("calendar")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::NetworkGetDefaultRoute),
            Some("network")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::NetworkGetHostnameAliases),
            Some("network")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::NetworkGetDnsServers),
            Some("network")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::NetworkGetInterfaceByIp),
            Some("network")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WeatherResolveLocationAlias),
            Some("weather")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WeatherGetForecastForDate),
            Some("weather")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WeatherGetHourlyWindow),
            Some("weather")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WeatherGetRecentHistoryForDate),
            Some("weather")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WebListCuratedSources),
            Some("web")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WebSearchPublicWeb),
            Some("web")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::WebFetchPublicPageSummary),
            Some("web")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetProcessDetail),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetListenerDetail),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetDiskUsageDetail),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetPortConflicts),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetPortConflictDetail),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetFailedUnits),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetFailedUnitDetail),
            Some("system")
        );
        assert_eq!(
            registry.provider_id_for_tool(AssistantToolName::SystemGetMountDetail),
            Some("system")
        );
    }

    #[test]
    fn registry_can_be_built_with_a_small_provider_subset() {
        let registry = ToolRegistry::from_providers(vec![
            Arc::new(CalendarToolProvider) as Arc<dyn super::ToolProvider>,
            Arc::new(DictionaryToolProvider) as Arc<dyn super::ToolProvider>,
            Arc::new(LibrariesToolProvider) as Arc<dyn super::ToolProvider>,
            Arc::new(MemoryToolProvider) as Arc<dyn super::ToolProvider>,
        ]);

        assert!(
            registry
                .entry(AssistantToolName::CalendarListEvents)
                .is_some()
        );
        assert!(
            registry
                .entry(AssistantToolName::LibrarySearchTitles)
                .is_some()
        );
        assert!(
            registry
                .entry(AssistantToolName::DictionarySearchPeople)
                .is_some()
        );
        assert!(
            registry
                .entry(AssistantToolName::WeatherGetCurrent)
                .is_none()
        );
        assert!(
            registry
                .entry(AssistantToolName::MemoryGetEntityRelations)
                .is_some()
        );
        assert!(
            registry
                .entry(AssistantToolName::MemoryGetEntityProvenance)
                .is_some()
        );
    }

    #[test]
    fn registry_entries_include_execution_metadata() {
        let registry = default_tool_registry();
        let weather = registry
            .entry(AssistantToolName::WeatherGetForecast)
            .expect("weather entry");
        assert_eq!(
            weather.domain_family,
            crate::ai_assistant::types::AssistantDomainFamily::Weather
        );
        assert!(weather.recovery_eligible);
        assert!(weather.ambiguity_prone);
        assert!(weather.freshness_sensitive);
    }

    #[test]
    fn write_tools_are_excluded_from_recovery_graph_metadata() {
        let registry = default_tool_registry();
        let create_event = registry
            .entry(AssistantToolName::CalendarCreateEvent)
            .expect("create event entry");
        assert!(!create_event.recovery_eligible);
    }

    #[test]
    fn read_only_profiles_deny_write_tools() {
        let profile =
            ToolExecutionProfile::restricted([AssistantToolName::CalendarCreateEvent], true, 1);
        let reason = profile
            .denial_reason(
                AssistantToolName::CalendarCreateEvent,
                AssistantToolName::CalendarCreateEvent.spec(),
            )
            .expect("write tool should be denied");
        assert!(reason.contains("read-only"));
    }
}
