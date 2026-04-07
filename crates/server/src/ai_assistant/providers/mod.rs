mod account;
mod ai_runtime;
mod calendar;
mod channels;
mod conversations;
mod dictionary;
mod documents;
mod downloads;
mod libraries;
mod memory;
mod network;
mod rooms;
mod servers;
mod system;
mod weather;
mod web;

use std::sync::Arc;

pub use account::AccountToolProvider;
pub use ai_runtime::AiRuntimeToolProvider;
pub use calendar::CalendarToolProvider;
pub use channels::ChannelsToolProvider;
pub use conversations::ConversationsToolProvider;
pub use dictionary::DictionaryToolProvider;
pub use documents::DocumentsToolProvider;
pub use downloads::DownloadsToolProvider;
pub use libraries::LibrariesToolProvider;
pub use memory::MemoryToolProvider;
pub use network::NetworkToolProvider;
pub use rooms::RoomsToolProvider;
pub use servers::ServersToolProvider;
pub use system::SystemToolProvider;
pub use weather::WeatherToolProvider;
pub use web::WebToolProvider;

use super::provider::ToolProvider;

pub fn default_tool_providers() -> Vec<Arc<dyn ToolProvider>> {
    vec![
        Arc::new(AccountToolProvider),
        Arc::new(CalendarToolProvider),
        Arc::new(ChannelsToolProvider),
        Arc::new(ConversationsToolProvider),
        Arc::new(DictionaryToolProvider),
        Arc::new(DocumentsToolProvider),
        Arc::new(DownloadsToolProvider),
        Arc::new(LibrariesToolProvider),
        Arc::new(MemoryToolProvider),
        Arc::new(NetworkToolProvider),
        Arc::new(RoomsToolProvider),
        Arc::new(ServersToolProvider),
        Arc::new(SystemToolProvider),
        Arc::new(AiRuntimeToolProvider),
        Arc::new(WeatherToolProvider),
        Arc::new(WebToolProvider),
    ]
}
