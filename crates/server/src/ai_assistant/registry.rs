use serde::{Deserialize, Serialize};

use super::types::{
    AssistantDomainFamily, AssistantToolSpec, ToolAccessMode, ToolConfirmationPolicy, ToolRiskTier,
    ToolRoleRequirement,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantToolName {
    AccountGetProfileSummary,
    CalendarListEvents,
    CalendarGetNextEvent,
    CalendarUpcomingBirthdays,
    CalendarGetEventDetails,
    CalendarCreateEvent,
    CalendarCreateBirthday,
    CalendarDeleteEvent,
    DocumentCreateDownload,
    ChannelsListUnreadActivity,
    ChannelsGetTranscriptSummary,
    DownloadsListAvailableArtifacts,
    NetworkGetTopologySummary,
    LibrariesListAccessible,
    LibrarySearchTitles,
    LibraryGetItemSummary,
    LibrariesGetRecentlyAdded,
    WeatherGetCurrent,
    WeatherGetForecast,
    WeatherGetHistory,
    WebSearchPublicWeb,
    WebFetchPublicPageSummary,
    RoomsListActive,
    RoomsListJoinable,
    RoomsGetRoomSummary,
    SystemGetCurrentDateTime,
    SystemGetAiRuntimeSummary,
    SystemGetHostRuntimeSummary,
    SystemGetBackupSummary,
    SystemGetServiceHealth,
    SystemGetTranscodeSummary,
    SystemGetStorageSummary,
    SystemGetRecentErrors,
    ServersListMinecraftStatus,
    ServersGetMinecraftServerSummary,
}

impl AssistantToolName {
    pub const fn all() -> &'static [Self] {
        &[
            Self::AccountGetProfileSummary,
            Self::CalendarListEvents,
            Self::CalendarGetNextEvent,
            Self::CalendarUpcomingBirthdays,
            Self::CalendarGetEventDetails,
            Self::CalendarCreateEvent,
            Self::CalendarCreateBirthday,
            Self::CalendarDeleteEvent,
            Self::DocumentCreateDownload,
            Self::ChannelsListUnreadActivity,
            Self::ChannelsGetTranscriptSummary,
            Self::DownloadsListAvailableArtifacts,
            Self::NetworkGetTopologySummary,
            Self::LibrariesListAccessible,
            Self::LibrarySearchTitles,
            Self::LibraryGetItemSummary,
            Self::LibrariesGetRecentlyAdded,
            Self::WeatherGetCurrent,
            Self::WeatherGetForecast,
            Self::WeatherGetHistory,
            Self::WebSearchPublicWeb,
            Self::WebFetchPublicPageSummary,
            Self::RoomsListActive,
            Self::RoomsListJoinable,
            Self::RoomsGetRoomSummary,
            Self::SystemGetCurrentDateTime,
            Self::SystemGetAiRuntimeSummary,
            Self::SystemGetHostRuntimeSummary,
            Self::SystemGetBackupSummary,
            Self::SystemGetServiceHealth,
            Self::SystemGetTranscodeSummary,
            Self::SystemGetStorageSummary,
            Self::SystemGetRecentErrors,
            Self::ServersListMinecraftStatus,
            Self::ServersGetMinecraftServerSummary,
        ]
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "account_get_profile_summary" => Some(Self::AccountGetProfileSummary),
            "calendar_list_events" => Some(Self::CalendarListEvents),
            "calendar_get_next_event" => Some(Self::CalendarGetNextEvent),
            "calendar_upcoming_birthdays" => Some(Self::CalendarUpcomingBirthdays),
            "calendar_get_event_details" => Some(Self::CalendarGetEventDetails),
            "calendar_create_event" => Some(Self::CalendarCreateEvent),
            "calendar_create_birthday" => Some(Self::CalendarCreateBirthday),
            "calendar_delete_event" => Some(Self::CalendarDeleteEvent),
            "document_create_download" => Some(Self::DocumentCreateDownload),
            "channels_list_unread_activity" => Some(Self::ChannelsListUnreadActivity),
            "channels_get_transcript_summary" => Some(Self::ChannelsGetTranscriptSummary),
            "downloads_list_available_artifacts" => Some(Self::DownloadsListAvailableArtifacts),
            "network_get_topology_summary" => Some(Self::NetworkGetTopologySummary),
            "libraries_list_accessible" => Some(Self::LibrariesListAccessible),
            "library_search_titles" => Some(Self::LibrarySearchTitles),
            "library_get_item_summary" => Some(Self::LibraryGetItemSummary),
            "libraries_get_recently_added" => Some(Self::LibrariesGetRecentlyAdded),
            "weather_get_current" => Some(Self::WeatherGetCurrent),
            "weather_get_forecast" => Some(Self::WeatherGetForecast),
            "weather_get_history" => Some(Self::WeatherGetHistory),
            "web_search_public_web" => Some(Self::WebSearchPublicWeb),
            "web_fetch_public_page_summary" => Some(Self::WebFetchPublicPageSummary),
            "rooms_list_active" => Some(Self::RoomsListActive),
            "rooms_list_joinable" => Some(Self::RoomsListJoinable),
            "rooms_get_room_summary" => Some(Self::RoomsGetRoomSummary),
            "system_get_current_datetime" => Some(Self::SystemGetCurrentDateTime),
            "system_get_ai_runtime_summary" => Some(Self::SystemGetAiRuntimeSummary),
            "system_get_host_runtime_summary" => Some(Self::SystemGetHostRuntimeSummary),
            "system_get_backup_summary" => Some(Self::SystemGetBackupSummary),
            "system_get_service_health" => Some(Self::SystemGetServiceHealth),
            "system_get_transcode_summary" => Some(Self::SystemGetTranscodeSummary),
            "system_get_storage_summary" => Some(Self::SystemGetStorageSummary),
            "system_get_recent_errors" => Some(Self::SystemGetRecentErrors),
            "servers_list_minecraft_status" => Some(Self::ServersListMinecraftStatus),
            "servers_get_minecraft_server_summary" => Some(Self::ServersGetMinecraftServerSummary),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountGetProfileSummary => "account_get_profile_summary",
            Self::CalendarListEvents => "calendar_list_events",
            Self::CalendarGetNextEvent => "calendar_get_next_event",
            Self::CalendarUpcomingBirthdays => "calendar_upcoming_birthdays",
            Self::CalendarGetEventDetails => "calendar_get_event_details",
            Self::CalendarCreateEvent => "calendar_create_event",
            Self::CalendarCreateBirthday => "calendar_create_birthday",
            Self::CalendarDeleteEvent => "calendar_delete_event",
            Self::DocumentCreateDownload => "document_create_download",
            Self::ChannelsListUnreadActivity => "channels_list_unread_activity",
            Self::ChannelsGetTranscriptSummary => "channels_get_transcript_summary",
            Self::DownloadsListAvailableArtifacts => "downloads_list_available_artifacts",
            Self::NetworkGetTopologySummary => "network_get_topology_summary",
            Self::LibrariesListAccessible => "libraries_list_accessible",
            Self::LibrarySearchTitles => "library_search_titles",
            Self::LibraryGetItemSummary => "library_get_item_summary",
            Self::LibrariesGetRecentlyAdded => "libraries_get_recently_added",
            Self::WeatherGetCurrent => "weather_get_current",
            Self::WeatherGetForecast => "weather_get_forecast",
            Self::WeatherGetHistory => "weather_get_history",
            Self::WebSearchPublicWeb => "web_search_public_web",
            Self::WebFetchPublicPageSummary => "web_fetch_public_page_summary",
            Self::RoomsListActive => "rooms_list_active",
            Self::RoomsListJoinable => "rooms_list_joinable",
            Self::RoomsGetRoomSummary => "rooms_get_room_summary",
            Self::SystemGetCurrentDateTime => "system_get_current_datetime",
            Self::SystemGetAiRuntimeSummary => "system_get_ai_runtime_summary",
            Self::SystemGetHostRuntimeSummary => "system_get_host_runtime_summary",
            Self::SystemGetBackupSummary => "system_get_backup_summary",
            Self::SystemGetServiceHealth => "system_get_service_health",
            Self::SystemGetTranscodeSummary => "system_get_transcode_summary",
            Self::SystemGetStorageSummary => "system_get_storage_summary",
            Self::SystemGetRecentErrors => "system_get_recent_errors",
            Self::ServersListMinecraftStatus => "servers_list_minecraft_status",
            Self::ServersGetMinecraftServerSummary => "servers_get_minecraft_server_summary",
        }
    }

    pub const fn spec(self) -> AssistantToolSpec {
        match self {
            Self::AccountGetProfileSummary => AssistantToolSpec {
                name: "account_get_profile_summary",
                summary: "Summarize the signed-in Rustyfin account and access scope.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 2_000,
                max_result_bytes: 4 * 1024,
            },
            Self::CalendarListEvents => AssistantToolSpec {
                name: "calendar_list_events",
                summary: "List visible upcoming calendar events for a short time window.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarGetNextEvent => AssistantToolSpec {
                name: "calendar_get_next_event",
                summary: "Load the next visible calendar event using deterministic server-side ordering.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarUpcomingBirthdays => AssistantToolSpec {
                name: "calendar_upcoming_birthdays",
                summary: "List visible upcoming birthdays from calendar data and optionally narrow to a named person.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarGetEventDetails => AssistantToolSpec {
                name: "calendar_get_event_details",
                summary: "Load a tighter summary for one visible calendar event.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarCreateEvent => AssistantToolSpec {
                name: "calendar_create_event",
                summary: "Create a one-off calendar event after explicit user confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 5_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarCreateBirthday => AssistantToolSpec {
                name: "calendar_create_birthday",
                summary: "Create a recurring birthday after explicit user confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 5_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarDeleteEvent => AssistantToolSpec {
                name: "calendar_delete_event",
                summary: "Delete a visible calendar event after explicit user confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::High,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 5_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DocumentCreateDownload => AssistantToolSpec {
                name: "document_create_download",
                summary: "Generate a bounded downloadable markdown or text document for the current user after explicit confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::High,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 20_000,
                max_result_bytes: 32 * 1024,
            },
            Self::ChannelsListUnreadActivity => AssistantToolSpec {
                name: "channels_list_unread_activity",
                summary: "Summarize recent visible channel activity; exact unread counts are unavailable.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::ChannelsGetTranscriptSummary => AssistantToolSpec {
                name: "channels_get_transcript_summary",
                summary: "Load the latest accessible completed voice-call transcript summary for a matching channel.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
            },
            Self::DownloadsListAvailableArtifacts => AssistantToolSpec {
                name: "downloads_list_available_artifacts",
                summary: "List authenticated host-published Rustyfin downloads and planned artifacts.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 2_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetTopologySummary => AssistantToolSpec {
                name: "network_get_topology_summary",
                summary: "Summarize host-visible network topology, interface IP addresses, remote-access state, and saved Rustyfin network settings.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::LibrariesListAccessible => AssistantToolSpec {
                name: "libraries_list_accessible",
                summary: "List libraries the current user can access.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::LibrarySearchTitles => AssistantToolSpec {
                name: "library_search_titles",
                summary: "Search accessible library item titles for a user-provided query.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::LibraryGetItemSummary => AssistantToolSpec {
                name: "library_get_item_summary",
                summary: "Load a tighter summary for one accessible library item.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::LibrariesGetRecentlyAdded => AssistantToolSpec {
                name: "libraries_get_recently_added",
                summary: "List recently added accessible library items, optionally narrowed by a title hint.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WeatherGetCurrent => AssistantToolSpec {
                name: "weather_get_current",
                summary: "Fetch current public weather conditions for one location using a fixed weather provider.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WeatherGetForecast => AssistantToolSpec {
                name: "weather_get_forecast",
                summary: "Fetch a short public weather forecast for one location using a fixed weather provider.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WeatherGetHistory => AssistantToolSpec {
                name: "weather_get_history",
                summary: "Fetch recent public weather history for one location using a fixed weather provider.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WebSearchPublicWeb => AssistantToolSpec {
                name: "web_search_public_web",
                summary: "Search a constrained public web source for current public information.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WebFetchPublicPageSummary => AssistantToolSpec {
                name: "web_fetch_public_page_summary",
                summary: "Fetch and summarize one constrained public web page.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::RoomsListActive => AssistantToolSpec {
                name: "rooms_list_active",
                summary: "List currently active public rooms that the user can see.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 2_000,
                max_result_bytes: 8 * 1024,
            },
            Self::RoomsListJoinable => AssistantToolSpec {
                name: "rooms_list_joinable",
                summary: "List rooms the user can join now, including public lobbies and direct invites.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::RoomsGetRoomSummary => AssistantToolSpec {
                name: "rooms_get_room_summary",
                summary: "Load a tighter summary for one active public room.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::SystemGetCurrentDateTime => AssistantToolSpec {
                name: "system_get_current_datetime",
                summary: "Report the current Rustyfin host local date and time for relative-date questions.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 1_000,
                max_result_bytes: 4 * 1024,
            },
            Self::SystemGetAiRuntimeSummary => AssistantToolSpec {
                name: "system_get_ai_runtime_summary",
                summary: "Summarize the current Rustyfin AI runtime, including the loaded model, backend, scheduler, and warm-model pool state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetHostRuntimeSummary => AssistantToolSpec {
                name: "system_get_host_runtime_summary",
                summary: "Summarize current Rustyfin host CPU, memory, load, and runtime counters.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::SystemGetBackupSummary => AssistantToolSpec {
                name: "system_get_backup_summary",
                summary: "Summarize the current Rustyfin backup and restore capability state on this host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::SystemGetServiceHealth => AssistantToolSpec {
                name: "system_get_service_health",
                summary: "Check Rustyfin core and internal agent health endpoints plus service availability state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetTranscodeSummary => AssistantToolSpec {
                name: "system_get_transcode_summary",
                summary: "Summarize active transcoding sessions, failure counters, and hardware acceleration state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::SystemGetStorageSummary => AssistantToolSpec {
                name: "system_get_storage_summary",
                summary: "Summarize Rustyfin storage paths and host free-space availability.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::SystemGetRecentErrors => AssistantToolSpec {
                name: "system_get_recent_errors",
                summary: "Summarize recent Rustyfin failures from job logs and runtime failure counters.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::ServersListMinecraftStatus => AssistantToolSpec {
                name: "servers_list_minecraft_status",
                summary: "List accessible Minecraft server status summaries.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::ServersGetMinecraftServerSummary => AssistantToolSpec {
                name: "servers_get_minecraft_server_summary",
                summary: "Load a tighter summary for one accessible Minecraft server.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
        }
    }

    pub const fn domain_family(self) -> AssistantDomainFamily {
        match self {
            Self::AccountGetProfileSummary => AssistantDomainFamily::Account,
            Self::CalendarListEvents
            | Self::CalendarGetNextEvent
            | Self::CalendarUpcomingBirthdays
            | Self::CalendarGetEventDetails
            | Self::CalendarCreateEvent
            | Self::CalendarCreateBirthday
            | Self::CalendarDeleteEvent => AssistantDomainFamily::Calendar,
            Self::DocumentCreateDownload => AssistantDomainFamily::Documents,
            Self::ChannelsListUnreadActivity => AssistantDomainFamily::Channels,
            Self::ChannelsGetTranscriptSummary => AssistantDomainFamily::Transcript,
            Self::DownloadsListAvailableArtifacts => AssistantDomainFamily::Downloads,
            Self::NetworkGetTopologySummary => AssistantDomainFamily::Network,
            Self::LibrariesListAccessible
            | Self::LibrarySearchTitles
            | Self::LibraryGetItemSummary
            | Self::LibrariesGetRecentlyAdded => AssistantDomainFamily::Library,
            Self::WeatherGetCurrent | Self::WeatherGetForecast | Self::WeatherGetHistory => {
                AssistantDomainFamily::Weather
            }
            Self::WebSearchPublicWeb | Self::WebFetchPublicPageSummary => {
                AssistantDomainFamily::Web
            }
            Self::RoomsListActive | Self::RoomsListJoinable | Self::RoomsGetRoomSummary => {
                AssistantDomainFamily::Rooms
            }
            Self::SystemGetCurrentDateTime
            | Self::SystemGetHostRuntimeSummary
            | Self::SystemGetBackupSummary
            | Self::SystemGetServiceHealth
            | Self::SystemGetTranscodeSummary
            | Self::SystemGetStorageSummary
            | Self::SystemGetRecentErrors => AssistantDomainFamily::System,
            Self::SystemGetAiRuntimeSummary => AssistantDomainFamily::AiRuntime,
            Self::ServersListMinecraftStatus | Self::ServersGetMinecraftServerSummary => {
                AssistantDomainFamily::Servers
            }
        }
    }

    pub const fn recovery_eligible(self) -> bool {
        matches!(self.spec().access_mode, ToolAccessMode::ReadOnly)
            && !matches!(
                self,
                Self::WebSearchPublicWeb | Self::WebFetchPublicPageSummary
            )
    }

    pub const fn can_parallelize(self) -> bool {
        matches!(
            self,
            Self::CalendarListEvents
                | Self::CalendarUpcomingBirthdays
                | Self::DownloadsListAvailableArtifacts
                | Self::LibrariesListAccessible
                | Self::LibrarySearchTitles
                | Self::LibrariesGetRecentlyAdded
                | Self::RoomsListActive
                | Self::RoomsListJoinable
                | Self::ServersListMinecraftStatus
        )
    }

    pub const fn ambiguity_prone(self) -> bool {
        matches!(
            self,
            Self::CalendarListEvents
                | Self::CalendarUpcomingBirthdays
                | Self::CalendarGetEventDetails
                | Self::ChannelsGetTranscriptSummary
                | Self::LibrarySearchTitles
                | Self::LibraryGetItemSummary
                | Self::WeatherGetCurrent
                | Self::WeatherGetForecast
                | Self::WeatherGetHistory
                | Self::RoomsGetRoomSummary
                | Self::ServersGetMinecraftServerSummary
        )
    }

    pub const fn freshness_sensitive(self) -> bool {
        matches!(
            self,
            Self::WeatherGetCurrent
                | Self::WeatherGetForecast
                | Self::WeatherGetHistory
                | Self::NetworkGetTopologySummary
                | Self::RoomsListActive
                | Self::SystemGetAiRuntimeSummary
                | Self::SystemGetHostRuntimeSummary
                | Self::SystemGetServiceHealth
                | Self::SystemGetTranscodeSummary
                | Self::SystemGetStorageSummary
                | Self::SystemGetRecentErrors
                | Self::ServersListMinecraftStatus
                | Self::ServersGetMinecraftServerSummary
        )
    }
}
