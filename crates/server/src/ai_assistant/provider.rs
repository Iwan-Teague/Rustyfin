use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use super::context::AssistantContext;
use super::providers::default_tool_providers;
use super::registry::AssistantToolName;
use super::types::{AssistantToolContextBlock, AssistantToolSpec, ToolAccessMode};
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
    use crate::ai_assistant::providers::{CalendarToolProvider, LibrariesToolProvider};
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
            registry.provider_id_for_tool(AssistantToolName::LibrarySearchTitles),
            Some("libraries")
        );
    }

    #[test]
    fn registry_can_be_built_with_a_small_provider_subset() {
        let registry = ToolRegistry::from_providers(vec![
            Arc::new(CalendarToolProvider) as Arc<dyn super::ToolProvider>,
            Arc::new(LibrariesToolProvider) as Arc<dyn super::ToolProvider>,
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
                .entry(AssistantToolName::WeatherGetCurrent)
                .is_none()
        );
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
