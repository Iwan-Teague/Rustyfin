use serde::{Deserialize, Serialize};

use super::types::{
    AssistantDomainFamily, AssistantToolSpec, ToolAccessMode, ToolConfirmationPolicy, ToolRiskTier,
    ToolRoleRequirement,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantToolName {
    AccountGetProfileSummary,
    DictionaryGetAccountIdentity,
    DictionaryListVisibleWorkspaces,
    DictionaryBrowseWorkspacePeople,
    DictionarySearchPeople,
    DictionaryGetPersonBundle,
    DictionaryResolveRelationshipReference,
    MemoryListRecentFacts,
    MemoryListRecentEntities,
    MemorySearchFacts,
    MemorySearchEntities,
    MemoryFindExactEntity,
    MemoryGetEntityRelations,
    MemoryGetEntityRelationPath,
    MemoryListRecentChanges,
    MemoryListConflictingFacts,
    MemoryGetEntityProvenance,
    MemoryGetPersonSummary,
    MemoryGetPersonTimeline,
    MemoryGetSourceCitation,
    MemoryGetConflictExplanations,
    CalendarListEvents,
    CalendarGetNextEvent,
    CalendarListDateConflicts,
    CalendarListFreeDays,
    CalendarGetNextFreeDay,
    CalendarGetNextEventTiming,
    CalendarCountEvents,
    CalendarListBusyDays,
    CalendarListOverlappingEvents,
    CalendarUpcomingBirthdays,
    CalendarGetEventDetails,
    CalendarGetEventByExactDateAndTitle,
    CalendarGetEventSeriesSummary,
    CalendarGetNextFreeSlot,
    CalendarListBusySlots,
    CalendarCreateEvent,
    CalendarCreateBirthday,
    CalendarDeleteEvent,
    DocumentCreateDownload,
    ConversationsArchiveSelection,
    ConversationsDeleteSelection,
    ConversationsMoveToGroupSelection,
    ChannelsListUnreadActivity,
    ChannelsGetTranscriptSummary,
    DownloadsListAvailableArtifacts,
    DownloadsGetArtifactDetails,
    DownloadsGetArtifactChecksum,
    DownloadsGetArtifactInstallSteps,
    DownloadsGetArtifactCompatibility,
    DownloadsGetLatestForPlatform,
    DownloadsGetArtifactPlatformMatrix,
    DownloadsGetArtifactSigningInfo,
    DownloadsGetArtifactSource,
    DownloadsGetReleaseNotes,
    NetworkGetTopologySummary,
    NetworkGetInterfaceDetails,
    NetworkGetInterfaceByIp,
    NetworkGetDefaultRoute,
    NetworkGetHostnameAliases,
    NetworkGetDnsServers,
    NetworkGetRouteToDestination,
    NetworkGetActiveConnectionDetail,
    NetworkGetRouteTable,
    NetworkGetActiveConnections,
    NetworkGetInterfaceCounters,
    NetworkGetWifiStatus,
    NetworkGetVpnStatus,
    LibrariesListAccessible,
    LibrariesGetLibrarySummary,
    LibrarySearchTitles,
    LibraryGetItemSummary,
    LibraryGetItemMediaDetails,
    LibraryGetItemSourcePaths,
    LibraryGetItemExternalIds,
    LibraryGetItemPlayHistory,
    LibrariesGetRecentlyAdded,
    LibrariesFindDuplicateTitles,
    LibrariesListMissingMetadata,
    WeatherGetCurrent,
    WeatherGetForecast,
    WeatherGetHistory,
    WeatherResolveLocationAlias,
    WeatherGetForecastForDate,
    WeatherGetHourlyWindow,
    WeatherGetRecentHistoryForDate,
    WebListCuratedSources,
    WebSearchPublicWeb,
    WebFetchPublicPageSummary,
    WebFetchSourceWithCitation,
    RoomsListActive,
    RoomsListJoinable,
    RoomsGetRoomSummary,
    SystemGetCurrentDateTime,
    SystemGetAiRuntimeSummary,
    SystemGetHostRuntimeSummary,
    SystemGetBackupSummary,
    SystemGetServiceHealth,
    SystemGetServiceDetail,
    SystemGetServiceLogs,
    SystemGetServiceDependencies,
    SystemGetTranscodeSummary,
    SystemGetStorageSummary,
    SystemGetStoragePathDetail,
    SystemGetMountDetail,
    SystemGetRecentErrors,
    SystemGetKernelInfo,
    SystemGetCpuTopology,
    SystemGetTemperatureSensors,
    SystemGetBlockDeviceInventory,
    SystemGetFilesystemTable,
    SystemGetGpuInventory,
    SystemGetPciDevices,
    SystemGetUsbDevices,
    SystemGetBootLogSummary,
    SystemGetJournalSummary,
    SystemGetProcessDetail,
    SystemGetListenerDetail,
    SystemGetDiskUsageDetail,
    SystemGetPortConflicts,
    SystemGetPortConflictDetail,
    SystemGetFailedUnits,
    SystemGetFailedUnitDetail,
    SystemGetFailedServiceLogs,
    SystemGetProcessTreeDetail,
    AiListBackgroundJobs,
    AiGetJobStatus,
    AiGetToolRegistry,
    AiGetGroundingSummary,
    AiGetLastToolFailureReason,
    ServersListMinecraftStatus,
    ServersGetMinecraftServerSummary,
}

impl AssistantToolName {
    pub const fn all() -> &'static [Self] {
        &[
            Self::AccountGetProfileSummary,
            Self::DictionaryGetAccountIdentity,
            Self::DictionaryListVisibleWorkspaces,
            Self::DictionaryBrowseWorkspacePeople,
            Self::DictionarySearchPeople,
            Self::DictionaryGetPersonBundle,
            Self::DictionaryResolveRelationshipReference,
            Self::MemoryListRecentFacts,
            Self::MemoryListRecentEntities,
            Self::MemorySearchFacts,
            Self::MemorySearchEntities,
            Self::MemoryFindExactEntity,
            Self::MemoryGetEntityRelations,
            Self::MemoryGetEntityRelationPath,
            Self::MemoryListRecentChanges,
            Self::MemoryListConflictingFacts,
            Self::MemoryGetEntityProvenance,
            Self::MemoryGetPersonSummary,
            Self::MemoryGetPersonTimeline,
            Self::MemoryGetSourceCitation,
            Self::MemoryGetConflictExplanations,
            Self::CalendarListEvents,
            Self::CalendarGetNextEvent,
            Self::CalendarListDateConflicts,
            Self::CalendarListFreeDays,
            Self::CalendarGetNextFreeDay,
            Self::CalendarGetNextEventTiming,
            Self::CalendarCountEvents,
            Self::CalendarListBusyDays,
            Self::CalendarListOverlappingEvents,
            Self::CalendarUpcomingBirthdays,
            Self::CalendarGetEventDetails,
            Self::CalendarGetEventByExactDateAndTitle,
            Self::CalendarGetEventSeriesSummary,
            Self::CalendarGetNextFreeSlot,
            Self::CalendarListBusySlots,
            Self::CalendarCreateEvent,
            Self::CalendarCreateBirthday,
            Self::CalendarDeleteEvent,
            Self::DocumentCreateDownload,
            Self::ConversationsArchiveSelection,
            Self::ConversationsDeleteSelection,
            Self::ConversationsMoveToGroupSelection,
            Self::ChannelsListUnreadActivity,
            Self::ChannelsGetTranscriptSummary,
            Self::DownloadsListAvailableArtifacts,
            Self::DownloadsGetArtifactDetails,
            Self::DownloadsGetArtifactChecksum,
            Self::DownloadsGetArtifactInstallSteps,
            Self::DownloadsGetArtifactCompatibility,
            Self::DownloadsGetLatestForPlatform,
            Self::DownloadsGetArtifactPlatformMatrix,
            Self::DownloadsGetArtifactSigningInfo,
            Self::DownloadsGetArtifactSource,
            Self::DownloadsGetReleaseNotes,
            Self::NetworkGetTopologySummary,
            Self::NetworkGetInterfaceDetails,
            Self::NetworkGetInterfaceByIp,
            Self::NetworkGetDefaultRoute,
            Self::NetworkGetHostnameAliases,
            Self::NetworkGetDnsServers,
            Self::NetworkGetRouteToDestination,
            Self::NetworkGetActiveConnectionDetail,
            Self::NetworkGetRouteTable,
            Self::NetworkGetActiveConnections,
            Self::NetworkGetInterfaceCounters,
            Self::NetworkGetWifiStatus,
            Self::NetworkGetVpnStatus,
            Self::LibrariesListAccessible,
            Self::LibrariesGetLibrarySummary,
            Self::LibrarySearchTitles,
            Self::LibraryGetItemSummary,
            Self::LibraryGetItemMediaDetails,
            Self::LibraryGetItemSourcePaths,
            Self::LibraryGetItemExternalIds,
            Self::LibraryGetItemPlayHistory,
            Self::LibrariesGetRecentlyAdded,
            Self::LibrariesFindDuplicateTitles,
            Self::LibrariesListMissingMetadata,
            Self::WeatherGetCurrent,
            Self::WeatherGetForecast,
            Self::WeatherGetHistory,
            Self::WeatherResolveLocationAlias,
            Self::WeatherGetForecastForDate,
            Self::WeatherGetHourlyWindow,
            Self::WeatherGetRecentHistoryForDate,
            Self::WebListCuratedSources,
            Self::WebSearchPublicWeb,
            Self::WebFetchPublicPageSummary,
            Self::WebFetchSourceWithCitation,
            Self::RoomsListActive,
            Self::RoomsListJoinable,
            Self::RoomsGetRoomSummary,
            Self::SystemGetCurrentDateTime,
            Self::SystemGetAiRuntimeSummary,
            Self::SystemGetHostRuntimeSummary,
            Self::SystemGetBackupSummary,
            Self::SystemGetServiceHealth,
            Self::SystemGetServiceDetail,
            Self::SystemGetServiceLogs,
            Self::SystemGetServiceDependencies,
            Self::SystemGetTranscodeSummary,
            Self::SystemGetStorageSummary,
            Self::SystemGetStoragePathDetail,
            Self::SystemGetMountDetail,
            Self::SystemGetRecentErrors,
            Self::SystemGetKernelInfo,
            Self::SystemGetCpuTopology,
            Self::SystemGetTemperatureSensors,
            Self::SystemGetBlockDeviceInventory,
            Self::SystemGetFilesystemTable,
            Self::SystemGetGpuInventory,
            Self::SystemGetPciDevices,
            Self::SystemGetUsbDevices,
            Self::SystemGetBootLogSummary,
            Self::SystemGetJournalSummary,
            Self::SystemGetProcessDetail,
            Self::SystemGetListenerDetail,
            Self::SystemGetDiskUsageDetail,
            Self::SystemGetPortConflicts,
            Self::SystemGetPortConflictDetail,
            Self::SystemGetFailedUnits,
            Self::SystemGetFailedUnitDetail,
            Self::SystemGetFailedServiceLogs,
            Self::SystemGetProcessTreeDetail,
            Self::AiListBackgroundJobs,
            Self::AiGetJobStatus,
            Self::AiGetToolRegistry,
            Self::AiGetGroundingSummary,
            Self::AiGetLastToolFailureReason,
            Self::ServersListMinecraftStatus,
            Self::ServersGetMinecraftServerSummary,
        ]
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "account_get_profile_summary" => Some(Self::AccountGetProfileSummary),
            "dictionary_get_account_identity" => Some(Self::DictionaryGetAccountIdentity),
            "dictionary_list_visible_workspaces" => Some(Self::DictionaryListVisibleWorkspaces),
            "dictionary_browse_workspace_people" => Some(Self::DictionaryBrowseWorkspacePeople),
            "dictionary_search_people" => Some(Self::DictionarySearchPeople),
            "dictionary_get_person_bundle" => Some(Self::DictionaryGetPersonBundle),
            "dictionary_resolve_relationship_reference" => {
                Some(Self::DictionaryResolveRelationshipReference)
            }
            "memory_list_recent_facts" => Some(Self::MemoryListRecentFacts),
            "memory_list_recent_entities" => Some(Self::MemoryListRecentEntities),
            "memory_search_facts" => Some(Self::MemorySearchFacts),
            "memory_search_entities" => Some(Self::MemorySearchEntities),
            "memory_find_exact_entity" => Some(Self::MemoryFindExactEntity),
            "memory_get_entity_relations" => Some(Self::MemoryGetEntityRelations),
            "memory_get_entity_relation_path" => Some(Self::MemoryGetEntityRelationPath),
            "memory_list_recent_changes" => Some(Self::MemoryListRecentChanges),
            "memory_list_conflicting_facts" => Some(Self::MemoryListConflictingFacts),
            "memory_get_entity_provenance" => Some(Self::MemoryGetEntityProvenance),
            "memory_get_person_summary" => Some(Self::MemoryGetPersonSummary),
            "memory_get_person_timeline" => Some(Self::MemoryGetPersonTimeline),
            "memory_get_source_citation" => Some(Self::MemoryGetSourceCitation),
            "memory_get_conflict_explanations" => Some(Self::MemoryGetConflictExplanations),
            "calendar_list_events" => Some(Self::CalendarListEvents),
            "calendar_get_next_event" => Some(Self::CalendarGetNextEvent),
            "calendar_list_date_conflicts" => Some(Self::CalendarListDateConflicts),
            "calendar_list_free_days" => Some(Self::CalendarListFreeDays),
            "calendar_get_next_free_day" => Some(Self::CalendarGetNextFreeDay),
            "calendar_get_next_event_timing" => Some(Self::CalendarGetNextEventTiming),
            "calendar_count_events" => Some(Self::CalendarCountEvents),
            "calendar_list_busy_days" => Some(Self::CalendarListBusyDays),
            "calendar_list_overlapping_events" => Some(Self::CalendarListOverlappingEvents),
            "calendar_upcoming_birthdays" => Some(Self::CalendarUpcomingBirthdays),
            "calendar_get_event_details" => Some(Self::CalendarGetEventDetails),
            "calendar_get_event_by_exact_date_and_title" => {
                Some(Self::CalendarGetEventByExactDateAndTitle)
            }
            "calendar_get_event_series_summary" => Some(Self::CalendarGetEventSeriesSummary),
            "calendar_get_next_free_slot" => Some(Self::CalendarGetNextFreeSlot),
            "calendar_list_busy_slots" => Some(Self::CalendarListBusySlots),
            "calendar_create_event" => Some(Self::CalendarCreateEvent),
            "calendar_create_birthday" => Some(Self::CalendarCreateBirthday),
            "calendar_delete_event" => Some(Self::CalendarDeleteEvent),
            "document_create_download" => Some(Self::DocumentCreateDownload),
            "conversations_archive_selection" => Some(Self::ConversationsArchiveSelection),
            "conversations_delete_selection" => Some(Self::ConversationsDeleteSelection),
            "conversations_move_to_group_selection" => {
                Some(Self::ConversationsMoveToGroupSelection)
            }
            "channels_list_unread_activity" => Some(Self::ChannelsListUnreadActivity),
            "channels_get_transcript_summary" => Some(Self::ChannelsGetTranscriptSummary),
            "downloads_list_available_artifacts" => Some(Self::DownloadsListAvailableArtifacts),
            "downloads_get_artifact_details" => Some(Self::DownloadsGetArtifactDetails),
            "downloads_get_artifact_checksum" => Some(Self::DownloadsGetArtifactChecksum),
            "downloads_get_artifact_install_steps" => Some(Self::DownloadsGetArtifactInstallSteps),
            "downloads_get_artifact_compatibility" => Some(Self::DownloadsGetArtifactCompatibility),
            "downloads_get_latest_for_platform" => Some(Self::DownloadsGetLatestForPlatform),
            "downloads_get_artifact_platform_matrix" => {
                Some(Self::DownloadsGetArtifactPlatformMatrix)
            }
            "downloads_get_artifact_signing_info" => Some(Self::DownloadsGetArtifactSigningInfo),
            "downloads_get_artifact_source" => Some(Self::DownloadsGetArtifactSource),
            "downloads_get_release_notes" => Some(Self::DownloadsGetReleaseNotes),
            "network_get_topology_summary" => Some(Self::NetworkGetTopologySummary),
            "network_get_interface_details" => Some(Self::NetworkGetInterfaceDetails),
            "network_get_interface_by_ip" => Some(Self::NetworkGetInterfaceByIp),
            "network_get_default_route" => Some(Self::NetworkGetDefaultRoute),
            "network_get_hostname_aliases" => Some(Self::NetworkGetHostnameAliases),
            "network_get_dns_servers" => Some(Self::NetworkGetDnsServers),
            "network_get_route_to_destination" => Some(Self::NetworkGetRouteToDestination),
            "network_get_active_connection_detail" => Some(Self::NetworkGetActiveConnectionDetail),
            "network_get_route_table" => Some(Self::NetworkGetRouteTable),
            "network_get_active_connections" => Some(Self::NetworkGetActiveConnections),
            "network_get_interface_counters" => Some(Self::NetworkGetInterfaceCounters),
            "network_get_wifi_status" => Some(Self::NetworkGetWifiStatus),
            "network_get_vpn_status" => Some(Self::NetworkGetVpnStatus),
            "libraries_list_accessible" => Some(Self::LibrariesListAccessible),
            "libraries_get_library_summary" => Some(Self::LibrariesGetLibrarySummary),
            "library_search_titles" => Some(Self::LibrarySearchTitles),
            "library_get_item_summary" => Some(Self::LibraryGetItemSummary),
            "library_get_item_media_details" => Some(Self::LibraryGetItemMediaDetails),
            "library_get_item_source_paths" => Some(Self::LibraryGetItemSourcePaths),
            "library_get_item_external_ids" => Some(Self::LibraryGetItemExternalIds),
            "library_get_item_play_history" => Some(Self::LibraryGetItemPlayHistory),
            "libraries_get_recently_added" => Some(Self::LibrariesGetRecentlyAdded),
            "libraries_find_duplicate_titles" => Some(Self::LibrariesFindDuplicateTitles),
            "libraries_list_missing_metadata" => Some(Self::LibrariesListMissingMetadata),
            "weather_get_current" => Some(Self::WeatherGetCurrent),
            "weather_get_forecast" => Some(Self::WeatherGetForecast),
            "weather_get_history" => Some(Self::WeatherGetHistory),
            "weather_resolve_location_alias" => Some(Self::WeatherResolveLocationAlias),
            "weather_get_forecast_for_date" => Some(Self::WeatherGetForecastForDate),
            "weather_get_hourly_window" => Some(Self::WeatherGetHourlyWindow),
            "weather_get_recent_history_for_date" => Some(Self::WeatherGetRecentHistoryForDate),
            "web_list_curated_sources" => Some(Self::WebListCuratedSources),
            "web_search_public_web" => Some(Self::WebSearchPublicWeb),
            "web_fetch_public_page_summary" => Some(Self::WebFetchPublicPageSummary),
            "web_fetch_source_with_citation" => Some(Self::WebFetchSourceWithCitation),
            "rooms_list_active" => Some(Self::RoomsListActive),
            "rooms_list_joinable" => Some(Self::RoomsListJoinable),
            "rooms_get_room_summary" => Some(Self::RoomsGetRoomSummary),
            "system_get_current_datetime" => Some(Self::SystemGetCurrentDateTime),
            "system_get_ai_runtime_summary" => Some(Self::SystemGetAiRuntimeSummary),
            "system_get_host_runtime_summary" => Some(Self::SystemGetHostRuntimeSummary),
            "system_get_backup_summary" => Some(Self::SystemGetBackupSummary),
            "system_get_service_health" => Some(Self::SystemGetServiceHealth),
            "system_get_service_detail" => Some(Self::SystemGetServiceDetail),
            "system_get_service_logs" => Some(Self::SystemGetServiceLogs),
            "system_get_service_dependencies" => Some(Self::SystemGetServiceDependencies),
            "system_get_transcode_summary" => Some(Self::SystemGetTranscodeSummary),
            "system_get_storage_summary" => Some(Self::SystemGetStorageSummary),
            "system_get_storage_path_detail" => Some(Self::SystemGetStoragePathDetail),
            "system_get_mount_detail" => Some(Self::SystemGetMountDetail),
            "system_get_recent_errors" => Some(Self::SystemGetRecentErrors),
            "system_get_kernel_info" => Some(Self::SystemGetKernelInfo),
            "system_get_cpu_topology" => Some(Self::SystemGetCpuTopology),
            "system_get_temperature_sensors" => Some(Self::SystemGetTemperatureSensors),
            "system_get_block_device_inventory" => Some(Self::SystemGetBlockDeviceInventory),
            "system_get_filesystem_table" => Some(Self::SystemGetFilesystemTable),
            "system_get_gpu_inventory" => Some(Self::SystemGetGpuInventory),
            "system_get_pci_devices" => Some(Self::SystemGetPciDevices),
            "system_get_usb_devices" => Some(Self::SystemGetUsbDevices),
            "system_get_boot_log_summary" => Some(Self::SystemGetBootLogSummary),
            "system_get_journal_summary" => Some(Self::SystemGetJournalSummary),
            "system_get_process_detail" => Some(Self::SystemGetProcessDetail),
            "system_get_listener_detail" => Some(Self::SystemGetListenerDetail),
            "system_get_disk_usage_detail" => Some(Self::SystemGetDiskUsageDetail),
            "system_get_port_conflicts" => Some(Self::SystemGetPortConflicts),
            "system_get_port_conflict_detail" => Some(Self::SystemGetPortConflictDetail),
            "system_get_failed_units" => Some(Self::SystemGetFailedUnits),
            "system_get_failed_unit_detail" => Some(Self::SystemGetFailedUnitDetail),
            "system_get_failed_service_logs" => Some(Self::SystemGetFailedServiceLogs),
            "system_get_process_tree_detail" => Some(Self::SystemGetProcessTreeDetail),
            "ai_list_background_jobs" => Some(Self::AiListBackgroundJobs),
            "ai_get_job_status" => Some(Self::AiGetJobStatus),
            "ai_get_tool_registry" => Some(Self::AiGetToolRegistry),
            "ai_get_grounding_summary" => Some(Self::AiGetGroundingSummary),
            "ai_get_last_tool_failure_reason" => Some(Self::AiGetLastToolFailureReason),
            "servers_list_minecraft_status" => Some(Self::ServersListMinecraftStatus),
            "servers_get_minecraft_server_summary" => Some(Self::ServersGetMinecraftServerSummary),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountGetProfileSummary => "account_get_profile_summary",
            Self::DictionaryGetAccountIdentity => "dictionary_get_account_identity",
            Self::DictionaryListVisibleWorkspaces => "dictionary_list_visible_workspaces",
            Self::DictionaryBrowseWorkspacePeople => "dictionary_browse_workspace_people",
            Self::DictionarySearchPeople => "dictionary_search_people",
            Self::DictionaryGetPersonBundle => "dictionary_get_person_bundle",
            Self::DictionaryResolveRelationshipReference => {
                "dictionary_resolve_relationship_reference"
            }
            Self::MemoryListRecentFacts => "memory_list_recent_facts",
            Self::MemoryListRecentEntities => "memory_list_recent_entities",
            Self::MemorySearchFacts => "memory_search_facts",
            Self::MemorySearchEntities => "memory_search_entities",
            Self::MemoryFindExactEntity => "memory_find_exact_entity",
            Self::MemoryGetEntityRelations => "memory_get_entity_relations",
            Self::MemoryGetEntityRelationPath => "memory_get_entity_relation_path",
            Self::MemoryListRecentChanges => "memory_list_recent_changes",
            Self::MemoryListConflictingFacts => "memory_list_conflicting_facts",
            Self::MemoryGetEntityProvenance => "memory_get_entity_provenance",
            Self::MemoryGetPersonSummary => "memory_get_person_summary",
            Self::MemoryGetPersonTimeline => "memory_get_person_timeline",
            Self::MemoryGetSourceCitation => "memory_get_source_citation",
            Self::MemoryGetConflictExplanations => "memory_get_conflict_explanations",
            Self::CalendarListEvents => "calendar_list_events",
            Self::CalendarGetNextEvent => "calendar_get_next_event",
            Self::CalendarListDateConflicts => "calendar_list_date_conflicts",
            Self::CalendarListFreeDays => "calendar_list_free_days",
            Self::CalendarGetNextFreeDay => "calendar_get_next_free_day",
            Self::CalendarGetNextEventTiming => "calendar_get_next_event_timing",
            Self::CalendarCountEvents => "calendar_count_events",
            Self::CalendarListBusyDays => "calendar_list_busy_days",
            Self::CalendarListOverlappingEvents => "calendar_list_overlapping_events",
            Self::CalendarUpcomingBirthdays => "calendar_upcoming_birthdays",
            Self::CalendarGetEventDetails => "calendar_get_event_details",
            Self::CalendarGetEventByExactDateAndTitle => {
                "calendar_get_event_by_exact_date_and_title"
            }
            Self::CalendarGetEventSeriesSummary => "calendar_get_event_series_summary",
            Self::CalendarGetNextFreeSlot => "calendar_get_next_free_slot",
            Self::CalendarListBusySlots => "calendar_list_busy_slots",
            Self::CalendarCreateEvent => "calendar_create_event",
            Self::CalendarCreateBirthday => "calendar_create_birthday",
            Self::CalendarDeleteEvent => "calendar_delete_event",
            Self::DocumentCreateDownload => "document_create_download",
            Self::ConversationsArchiveSelection => "conversations_archive_selection",
            Self::ConversationsDeleteSelection => "conversations_delete_selection",
            Self::ConversationsMoveToGroupSelection => "conversations_move_to_group_selection",
            Self::ChannelsListUnreadActivity => "channels_list_unread_activity",
            Self::ChannelsGetTranscriptSummary => "channels_get_transcript_summary",
            Self::DownloadsListAvailableArtifacts => "downloads_list_available_artifacts",
            Self::DownloadsGetArtifactDetails => "downloads_get_artifact_details",
            Self::DownloadsGetArtifactChecksum => "downloads_get_artifact_checksum",
            Self::DownloadsGetArtifactInstallSteps => "downloads_get_artifact_install_steps",
            Self::DownloadsGetArtifactCompatibility => "downloads_get_artifact_compatibility",
            Self::DownloadsGetLatestForPlatform => "downloads_get_latest_for_platform",
            Self::DownloadsGetArtifactPlatformMatrix => "downloads_get_artifact_platform_matrix",
            Self::DownloadsGetArtifactSigningInfo => "downloads_get_artifact_signing_info",
            Self::DownloadsGetArtifactSource => "downloads_get_artifact_source",
            Self::DownloadsGetReleaseNotes => "downloads_get_release_notes",
            Self::NetworkGetTopologySummary => "network_get_topology_summary",
            Self::NetworkGetInterfaceDetails => "network_get_interface_details",
            Self::NetworkGetInterfaceByIp => "network_get_interface_by_ip",
            Self::NetworkGetDefaultRoute => "network_get_default_route",
            Self::NetworkGetHostnameAliases => "network_get_hostname_aliases",
            Self::NetworkGetDnsServers => "network_get_dns_servers",
            Self::NetworkGetRouteToDestination => "network_get_route_to_destination",
            Self::NetworkGetActiveConnectionDetail => "network_get_active_connection_detail",
            Self::NetworkGetRouteTable => "network_get_route_table",
            Self::NetworkGetActiveConnections => "network_get_active_connections",
            Self::NetworkGetInterfaceCounters => "network_get_interface_counters",
            Self::NetworkGetWifiStatus => "network_get_wifi_status",
            Self::NetworkGetVpnStatus => "network_get_vpn_status",
            Self::LibrariesListAccessible => "libraries_list_accessible",
            Self::LibrariesGetLibrarySummary => "libraries_get_library_summary",
            Self::LibrarySearchTitles => "library_search_titles",
            Self::LibraryGetItemSummary => "library_get_item_summary",
            Self::LibraryGetItemMediaDetails => "library_get_item_media_details",
            Self::LibraryGetItemSourcePaths => "library_get_item_source_paths",
            Self::LibraryGetItemExternalIds => "library_get_item_external_ids",
            Self::LibraryGetItemPlayHistory => "library_get_item_play_history",
            Self::LibrariesGetRecentlyAdded => "libraries_get_recently_added",
            Self::LibrariesFindDuplicateTitles => "libraries_find_duplicate_titles",
            Self::LibrariesListMissingMetadata => "libraries_list_missing_metadata",
            Self::WeatherGetCurrent => "weather_get_current",
            Self::WeatherGetForecast => "weather_get_forecast",
            Self::WeatherGetHistory => "weather_get_history",
            Self::WeatherResolveLocationAlias => "weather_resolve_location_alias",
            Self::WeatherGetForecastForDate => "weather_get_forecast_for_date",
            Self::WeatherGetHourlyWindow => "weather_get_hourly_window",
            Self::WeatherGetRecentHistoryForDate => "weather_get_recent_history_for_date",
            Self::WebListCuratedSources => "web_list_curated_sources",
            Self::WebSearchPublicWeb => "web_search_public_web",
            Self::WebFetchPublicPageSummary => "web_fetch_public_page_summary",
            Self::WebFetchSourceWithCitation => "web_fetch_source_with_citation",
            Self::RoomsListActive => "rooms_list_active",
            Self::RoomsListJoinable => "rooms_list_joinable",
            Self::RoomsGetRoomSummary => "rooms_get_room_summary",
            Self::SystemGetCurrentDateTime => "system_get_current_datetime",
            Self::SystemGetAiRuntimeSummary => "system_get_ai_runtime_summary",
            Self::SystemGetHostRuntimeSummary => "system_get_host_runtime_summary",
            Self::SystemGetBackupSummary => "system_get_backup_summary",
            Self::SystemGetServiceHealth => "system_get_service_health",
            Self::SystemGetServiceDetail => "system_get_service_detail",
            Self::SystemGetServiceLogs => "system_get_service_logs",
            Self::SystemGetServiceDependencies => "system_get_service_dependencies",
            Self::SystemGetTranscodeSummary => "system_get_transcode_summary",
            Self::SystemGetStorageSummary => "system_get_storage_summary",
            Self::SystemGetStoragePathDetail => "system_get_storage_path_detail",
            Self::SystemGetMountDetail => "system_get_mount_detail",
            Self::SystemGetRecentErrors => "system_get_recent_errors",
            Self::SystemGetKernelInfo => "system_get_kernel_info",
            Self::SystemGetCpuTopology => "system_get_cpu_topology",
            Self::SystemGetTemperatureSensors => "system_get_temperature_sensors",
            Self::SystemGetBlockDeviceInventory => "system_get_block_device_inventory",
            Self::SystemGetFilesystemTable => "system_get_filesystem_table",
            Self::SystemGetGpuInventory => "system_get_gpu_inventory",
            Self::SystemGetPciDevices => "system_get_pci_devices",
            Self::SystemGetUsbDevices => "system_get_usb_devices",
            Self::SystemGetBootLogSummary => "system_get_boot_log_summary",
            Self::SystemGetJournalSummary => "system_get_journal_summary",
            Self::SystemGetProcessDetail => "system_get_process_detail",
            Self::SystemGetListenerDetail => "system_get_listener_detail",
            Self::SystemGetDiskUsageDetail => "system_get_disk_usage_detail",
            Self::SystemGetPortConflicts => "system_get_port_conflicts",
            Self::SystemGetPortConflictDetail => "system_get_port_conflict_detail",
            Self::SystemGetFailedUnits => "system_get_failed_units",
            Self::SystemGetFailedUnitDetail => "system_get_failed_unit_detail",
            Self::SystemGetFailedServiceLogs => "system_get_failed_service_logs",
            Self::SystemGetProcessTreeDetail => "system_get_process_tree_detail",
            Self::AiListBackgroundJobs => "ai_list_background_jobs",
            Self::AiGetJobStatus => "ai_get_job_status",
            Self::AiGetToolRegistry => "ai_get_tool_registry",
            Self::AiGetGroundingSummary => "ai_get_grounding_summary",
            Self::AiGetLastToolFailureReason => "ai_get_last_tool_failure_reason",
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
            Self::DictionaryGetAccountIdentity => AssistantToolSpec {
                name: "dictionary_get_account_identity",
                summary: "Load the signed-in user's linked Human Dictionary identity and default workspaces.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DictionaryListVisibleWorkspaces => AssistantToolSpec {
                name: "dictionary_list_visible_workspaces",
                summary: "List the signed-in user's visible Human Dictionary workspaces.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DictionaryBrowseWorkspacePeople => AssistantToolSpec {
                name: "dictionary_browse_workspace_people",
                summary: "Browse or search visible Human Dictionary people inside one accessible workspace.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::DictionarySearchPeople => AssistantToolSpec {
                name: "dictionary_search_people",
                summary: "Search visible Human Dictionary people inside one accessible workspace.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::DictionaryGetPersonBundle => AssistantToolSpec {
                name: "dictionary_get_person_bundle",
                summary: "Load one visible Human Dictionary person's facts, relations, and document in one workspace.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 14 * 1024,
            },
            Self::DictionaryResolveRelationshipReference => AssistantToolSpec {
                name: "dictionary_resolve_relationship_reference",
                summary: "Resolve relationship-relative Human Dictionary references like my mother, my brother, or my co-workers.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 14 * 1024,
            },
            Self::AiListBackgroundJobs => AssistantToolSpec {
                name: "ai_list_background_jobs",
                summary: "List recent Rustyfin background jobs and their live state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::AiGetJobStatus => AssistantToolSpec {
                name: "ai_get_job_status",
                summary: "Summarize the current Rustyfin background job counters and AI runtime activity.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::AiGetToolRegistry => AssistantToolSpec {
                name: "ai_get_tool_registry",
                summary: "List the registered Rustyfin AI tools and their execution metadata.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 16 * 1024,
            },
            Self::AiGetGroundingSummary => AssistantToolSpec {
                name: "ai_get_grounding_summary",
                summary: "Summarize the current Rustyfin AI runtime grounding and scheduler state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 16 * 1024,
            },
            Self::AiGetLastToolFailureReason => AssistantToolSpec {
                name: "ai_get_last_tool_failure_reason",
                summary: "Summarize the most recent Rustyfin AI execution failure, if one exists.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemoryGetPersonSummary => AssistantToolSpec {
                name: "memory_get_person_summary",
                summary: "Load a concise summary for one stored person or profile entity.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::MemoryGetPersonTimeline => AssistantToolSpec {
                name: "memory_get_person_timeline",
                summary: "Load a bounded chronological timeline for one stored person or profile entity.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemoryGetSourceCitation => AssistantToolSpec {
                name: "memory_get_source_citation",
                summary: "Resolve one stored entity or source chunk and show its recorded citation.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemoryGetConflictExplanations => AssistantToolSpec {
                name: "memory_get_conflict_explanations",
                summary: "Explain conflicting stored memory facts with their competing values and timestamps.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemoryListRecentFacts => AssistantToolSpec {
                name: "memory_list_recent_facts",
                summary: "List the signed-in user's recent stored AI memory facts.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::MemoryListRecentEntities => AssistantToolSpec {
                name: "memory_list_recent_entities",
                summary: "List the signed-in user's recent stored entities and people.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::MemorySearchFacts => AssistantToolSpec {
                name: "memory_search_facts",
                summary: "Search the signed-in user's stored AI memory facts for a query.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::MemoryFindExactEntity => AssistantToolSpec {
                name: "memory_find_exact_entity",
                summary: "Find the signed-in user's exact stored entity match for a query.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::MemoryGetEntityRelations => AssistantToolSpec {
                name: "memory_get_entity_relations",
                summary: "Load the stored entity graph relations for a specific subject.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemoryGetEntityRelationPath => AssistantToolSpec {
                name: "memory_get_entity_relation_path",
                summary: "Trace a bounded relation path between two stored entities.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 14 * 1024,
            },
            Self::MemoryListRecentChanges => AssistantToolSpec {
                name: "memory_list_recent_changes",
                summary: "List the signed-in user's most recent stored memory changes.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::MemoryListConflictingFacts => AssistantToolSpec {
                name: "memory_list_conflicting_facts",
                summary: "Surface stored memory facts that disagree with each other.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemoryGetEntityProvenance => AssistantToolSpec {
                name: "memory_get_entity_provenance",
                summary: "Resolve a stored entity and show its recorded source provenance.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::MemorySearchEntities => AssistantToolSpec {
                name: "memory_search_entities",
                summary: "Search the signed-in user's stored entity graph for people or groups.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
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
            Self::CalendarListDateConflicts => AssistantToolSpec {
                name: "calendar_list_date_conflicts",
                summary: "List visible calendar dates in a window that contain more than one event.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::CalendarListFreeDays => AssistantToolSpec {
                name: "calendar_list_free_days",
                summary: "List visible calendar dates in a window that have no events.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::CalendarGetNextFreeDay => AssistantToolSpec {
                name: "calendar_get_next_free_day",
                summary: "Report the next visible free calendar day in a bounded window.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarGetEventByExactDateAndTitle => AssistantToolSpec {
                name: "calendar_get_event_by_exact_date_and_title",
                summary: "Load one visible calendar event by exact date and title.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarGetEventSeriesSummary => AssistantToolSpec {
                name: "calendar_get_event_series_summary",
                summary: "Summarize a visible recurring calendar event series.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::CalendarGetNextFreeSlot => AssistantToolSpec {
                name: "calendar_get_next_free_slot",
                summary: "Report the next visible free calendar slot in a bounded window.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarListBusySlots => AssistantToolSpec {
                name: "calendar_list_busy_slots",
                summary: "List visible occupied calendar slots in a bounded window.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::CalendarGetNextEventTiming => AssistantToolSpec {
                name: "calendar_get_next_event_timing",
                summary: "Report how long until the next visible calendar event.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarCountEvents => AssistantToolSpec {
                name: "calendar_count_events",
                summary: "Count visible calendar events in a window and summarize how many days are occupied.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::CalendarListBusyDays => AssistantToolSpec {
                name: "calendar_list_busy_days",
                summary: "List visible calendar days in a window that have one or more events.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 12 * 1024,
            },
            Self::CalendarListOverlappingEvents => AssistantToolSpec {
                name: "calendar_list_overlapping_events",
                summary: "List visible calendar dates where multiple events overlap.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 12 * 1024,
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
            Self::ConversationsArchiveSelection => AssistantToolSpec {
                name: "conversations_archive_selection",
                summary: "Archive selected AI conversations for the signed-in user after explicit confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
            },
            Self::ConversationsDeleteSelection => AssistantToolSpec {
                name: "conversations_delete_selection",
                summary: "Permanently delete selected AI conversations for the signed-in user after explicit confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::Critical,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
            },
            Self::ConversationsMoveToGroupSelection => AssistantToolSpec {
                name: "conversations_move_to_group_selection",
                summary: "Move selected AI conversations into a named group for the signed-in user after explicit confirmation.",
                access_mode: ToolAccessMode::Write,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::ExplicitUserConfirm,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
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
            Self::DownloadsGetArtifactDetails => AssistantToolSpec {
                name: "downloads_get_artifact_details",
                summary: "Load exact metadata for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::DownloadsGetArtifactSource => AssistantToolSpec {
                name: "downloads_get_artifact_source",
                summary: "Load the source URL or package path for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::DownloadsGetReleaseNotes => AssistantToolSpec {
                name: "downloads_get_release_notes",
                summary: "Load the release-note detail text for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::DownloadsGetArtifactChecksum => AssistantToolSpec {
                name: "downloads_get_artifact_checksum",
                summary: "Return only the checksum for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DownloadsGetArtifactInstallSteps => AssistantToolSpec {
                name: "downloads_get_artifact_install_steps",
                summary: "Return the install steps for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DownloadsGetArtifactCompatibility => AssistantToolSpec {
                name: "downloads_get_artifact_compatibility",
                summary: "Return platform and architecture compatibility for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DownloadsGetLatestForPlatform => AssistantToolSpec {
                name: "downloads_get_latest_for_platform",
                summary: "Return the most recent available download artifact for a platform or platform query.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::DownloadsGetArtifactPlatformMatrix => AssistantToolSpec {
                name: "downloads_get_artifact_platform_matrix",
                summary: "Summarize available download artifact platform and architecture coverage.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 10 * 1024,
            },
            Self::DownloadsGetArtifactSigningInfo => AssistantToolSpec {
                name: "downloads_get_artifact_signing_info",
                summary: "Return checksum and signature details for one authenticated host-published download artifact.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
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
            Self::NetworkGetInterfaceDetails => AssistantToolSpec {
                name: "network_get_interface_details",
                summary: "Describe one host network interface and its addresses.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetInterfaceByIp => AssistantToolSpec {
                name: "network_get_interface_by_ip",
                summary: "Resolve one host network interface from an exact IP address.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetDefaultRoute => AssistantToolSpec {
                name: "network_get_default_route",
                summary: "Summarize the host default route, gateway, interface, and source address.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetHostnameAliases => AssistantToolSpec {
                name: "network_get_hostname_aliases",
                summary: "List hostname aliases and host naming entries visible to the Rustyfin host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetDnsServers => AssistantToolSpec {
                name: "network_get_dns_servers",
                summary: "List DNS servers and resolvers visible to the Rustyfin host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetRouteToDestination => AssistantToolSpec {
                name: "network_get_route_to_destination",
                summary: "Resolve the host route used for one exact destination address or hostname.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::NetworkGetActiveConnectionDetail => AssistantToolSpec {
                name: "network_get_active_connection_detail",
                summary: "Load a bounded detail view for one active host connection or listener.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::NetworkGetRouteTable => AssistantToolSpec {
                name: "network_get_route_table",
                summary: "Summarize the host route table and default routing entries.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::NetworkGetActiveConnections => AssistantToolSpec {
                name: "network_get_active_connections",
                summary: "List active host socket connections and listeners.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::NetworkGetInterfaceCounters => AssistantToolSpec {
                name: "network_get_interface_counters",
                summary: "Summarize host interface traffic counters and link state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::NetworkGetWifiStatus => AssistantToolSpec {
                name: "network_get_wifi_status",
                summary: "Summarize wireless interfaces and Wi-Fi status visible to the host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::NetworkGetVpnStatus => AssistantToolSpec {
                name: "network_get_vpn_status",
                summary: "Summarize tunnel, VPN, and WireGuard interface state visible to the host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
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
            Self::LibrariesGetLibrarySummary => AssistantToolSpec {
                name: "libraries_get_library_summary",
                summary: "Load exact metadata, paths, item count, and settings for one accessible library.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
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
            Self::LibraryGetItemMediaDetails => AssistantToolSpec {
                name: "library_get_item_media_details",
                summary: "Resolve one accessible library item and return artwork and media-path details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::LibraryGetItemSourcePaths => AssistantToolSpec {
                name: "library_get_item_source_paths",
                summary: "Resolve one accessible library item and return source-path details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::LibraryGetItemExternalIds => AssistantToolSpec {
                name: "library_get_item_external_ids",
                summary: "Resolve one accessible library item and return external provider identifiers.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::LibraryGetItemPlayHistory => AssistantToolSpec {
                name: "library_get_item_play_history",
                summary: "Resolve one accessible library item and return the current user's playback state.",
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
            Self::LibrariesFindDuplicateTitles => AssistantToolSpec {
                name: "libraries_find_duplicate_titles",
                summary: "List duplicate library titles across accessible libraries.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 10 * 1024,
            },
            Self::LibrariesListMissingMetadata => AssistantToolSpec {
                name: "libraries_list_missing_metadata",
                summary: "List accessible library items with missing core metadata.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 10 * 1024,
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
            Self::WeatherResolveLocationAlias => AssistantToolSpec {
                name: "weather_resolve_location_alias",
                summary: "Resolve one public location name to a canonical location and timezone.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WeatherGetForecastForDate => AssistantToolSpec {
                name: "weather_get_forecast_for_date",
                summary: "Fetch a date-specific public weather forecast for one location.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WeatherGetHourlyWindow => AssistantToolSpec {
                name: "weather_get_hourly_window",
                summary: "Fetch a date-specific public hourly weather window for one location.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 12 * 1024,
            },
            Self::WeatherGetRecentHistoryForDate => AssistantToolSpec {
                name: "weather_get_recent_history_for_date",
                summary: "Fetch a date-specific public weather history window for one location.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AnyAuthenticatedUser,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WebListCuratedSources => AssistantToolSpec {
                name: "web_list_curated_sources",
                summary: "List the curated public web source categories and their allowed domains.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Low,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 2_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WebSearchPublicWeb => AssistantToolSpec {
                name: "web_search_public_web",
                summary: "Search a constrained public web source for current public information, optionally within a curated category.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WebFetchPublicPageSummary => AssistantToolSpec {
                name: "web_fetch_public_page_summary",
                summary: "Fetch and summarize one constrained public web page, optionally within a curated category.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 8 * 1024,
            },
            Self::WebFetchSourceWithCitation => AssistantToolSpec {
                name: "web_fetch_source_with_citation",
                summary: "Fetch and summarize one constrained public web page with a compact source citation.",
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
            Self::SystemGetServiceDetail => AssistantToolSpec {
                name: "system_get_service_detail",
                summary: "Load one Rustyfin service health component by name or alias.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 8 * 1024,
            },
            Self::SystemGetServiceLogs => AssistantToolSpec {
                name: "system_get_service_logs",
                summary: "Load recent logs for one Rustyfin service or systemd unit.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetServiceDependencies => AssistantToolSpec {
                name: "system_get_service_dependencies",
                summary: "Load the dependency tree for one Rustyfin service or systemd unit.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
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
            Self::SystemGetStoragePathDetail => AssistantToolSpec {
                name: "system_get_storage_path_detail",
                summary: "Resolve one storage path or mount and return exact usage and mount details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 10 * 1024,
            },
            Self::SystemGetMountDetail => AssistantToolSpec {
                name: "system_get_mount_detail",
                summary: "Resolve one storage mount point or filesystem and return exact mount usage details.",
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
            Self::SystemGetKernelInfo => AssistantToolSpec {
                name: "system_get_kernel_info",
                summary: "Report the host kernel, operating system, and runtime base platform details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 3_000,
                max_result_bytes: 8 * 1024,
            },
            Self::SystemGetCpuTopology => AssistantToolSpec {
                name: "system_get_cpu_topology",
                summary: "Summarize the host CPU topology, sockets, cores, and threads.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetTemperatureSensors => AssistantToolSpec {
                name: "system_get_temperature_sensors",
                summary: "Report hardware temperature sensors exposed by the host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetBlockDeviceInventory => AssistantToolSpec {
                name: "system_get_block_device_inventory",
                summary: "List the host block devices and their mount state.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
            },
            Self::SystemGetFilesystemTable => AssistantToolSpec {
                name: "system_get_filesystem_table",
                summary: "Report mounted filesystems and their usage details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
            },
            Self::SystemGetGpuInventory => AssistantToolSpec {
                name: "system_get_gpu_inventory",
                summary: "Report GPU and graphics adapter inventory visible to the host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 16 * 1024,
            },
            Self::SystemGetPciDevices => AssistantToolSpec {
                name: "system_get_pci_devices",
                summary: "List host PCI devices and their device IDs.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 16 * 1024,
            },
            Self::SystemGetUsbDevices => AssistantToolSpec {
                name: "system_get_usb_devices",
                summary: "List host USB devices and their device IDs.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 4_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetBootLogSummary => AssistantToolSpec {
                name: "system_get_boot_log_summary",
                summary: "Summarize the current boot journal and early boot log entries.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 16 * 1024,
            },
            Self::SystemGetJournalSummary => AssistantToolSpec {
                name: "system_get_journal_summary",
                summary: "Summarize current boot warning and error journal entries.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 16 * 1024,
            },
            Self::SystemGetProcessDetail => AssistantToolSpec {
                name: "system_get_process_detail",
                summary: "Resolve one host process and return exact process details and command line context.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetListenerDetail => AssistantToolSpec {
                name: "system_get_listener_detail",
                summary: "Resolve one host listener or socket and return exact bound address and process context.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetDiskUsageDetail => AssistantToolSpec {
                name: "system_get_disk_usage_detail",
                summary: "Resolve one path or mount and return exact disk usage details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetPortConflicts => AssistantToolSpec {
                name: "system_get_port_conflicts",
                summary: "Summarize listening ports, sockets, and process conflicts on the host.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetPortConflictDetail => AssistantToolSpec {
                name: "system_get_port_conflict_detail",
                summary: "Resolve one specific host port or socket conflict and return exact listener details.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetFailedUnits => AssistantToolSpec {
                name: "system_get_failed_units",
                summary: "Summarize failed systemd units and their latest log context.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetFailedUnitDetail => AssistantToolSpec {
                name: "system_get_failed_unit_detail",
                summary: "Inspect one failed systemd unit and return exact status and recent log context.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetFailedServiceLogs => AssistantToolSpec {
                name: "system_get_failed_service_logs",
                summary: "Load recent logs for one failed service or systemd unit.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 6_000,
                max_result_bytes: 12 * 1024,
            },
            Self::SystemGetProcessTreeDetail => AssistantToolSpec {
                name: "system_get_process_tree_detail",
                summary: "Load a bounded process tree for one process, pid, or command line.",
                access_mode: ToolAccessMode::ReadOnly,
                risk_tier: ToolRiskTier::Moderate,
                required_role: ToolRoleRequirement::AdminOnly,
                confirmation: ToolConfirmationPolicy::None,
                timeout_ms: 5_000,
                max_result_bytes: 12 * 1024,
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
            Self::DictionaryGetAccountIdentity
            | Self::DictionaryListVisibleWorkspaces
            | Self::DictionaryBrowseWorkspacePeople
            | Self::DictionarySearchPeople
            | Self::DictionaryGetPersonBundle
            | Self::DictionaryResolveRelationshipReference => AssistantDomainFamily::Dictionary,
            Self::MemoryListRecentFacts
            | Self::MemoryListRecentEntities
            | Self::MemorySearchFacts
            | Self::MemorySearchEntities
            | Self::MemoryGetPersonSummary
            | Self::MemoryFindExactEntity
            | Self::MemoryGetEntityRelationPath
            | Self::MemoryGetEntityRelations
            | Self::MemoryListRecentChanges
            | Self::MemoryListConflictingFacts
            | Self::MemoryGetEntityProvenance
            | Self::MemoryGetPersonTimeline
            | Self::MemoryGetSourceCitation
            | Self::MemoryGetConflictExplanations => AssistantDomainFamily::Memory,
            Self::CalendarListEvents
            | Self::CalendarGetNextEvent
            | Self::CalendarListDateConflicts
            | Self::CalendarListFreeDays
            | Self::CalendarGetNextFreeDay
            | Self::CalendarGetEventByExactDateAndTitle
            | Self::CalendarGetEventSeriesSummary
            | Self::CalendarGetNextFreeSlot
            | Self::CalendarListBusySlots
            | Self::CalendarGetNextEventTiming
            | Self::CalendarCountEvents
            | Self::CalendarListBusyDays
            | Self::CalendarListOverlappingEvents
            | Self::CalendarUpcomingBirthdays
            | Self::CalendarGetEventDetails
            | Self::CalendarCreateEvent
            | Self::CalendarCreateBirthday
            | Self::CalendarDeleteEvent => AssistantDomainFamily::Calendar,
            Self::DocumentCreateDownload => AssistantDomainFamily::Documents,
            Self::ConversationsArchiveSelection
            | Self::ConversationsDeleteSelection
            | Self::ConversationsMoveToGroupSelection => AssistantDomainFamily::Conversations,
            Self::ChannelsListUnreadActivity => AssistantDomainFamily::Channels,
            Self::ChannelsGetTranscriptSummary => AssistantDomainFamily::Transcript,
            Self::DownloadsListAvailableArtifacts
            | Self::DownloadsGetArtifactDetails
            | Self::DownloadsGetArtifactSource
            | Self::DownloadsGetReleaseNotes
            | Self::DownloadsGetArtifactChecksum
            | Self::DownloadsGetArtifactInstallSteps
            | Self::DownloadsGetArtifactCompatibility
            | Self::DownloadsGetLatestForPlatform
            | Self::DownloadsGetArtifactPlatformMatrix
            | Self::DownloadsGetArtifactSigningInfo => AssistantDomainFamily::Downloads,
            Self::NetworkGetTopologySummary
            | Self::NetworkGetInterfaceDetails
            | Self::NetworkGetInterfaceByIp
            | Self::NetworkGetDefaultRoute
            | Self::NetworkGetHostnameAliases
            | Self::NetworkGetDnsServers
            | Self::NetworkGetRouteToDestination
            | Self::NetworkGetActiveConnectionDetail
            | Self::NetworkGetRouteTable
            | Self::NetworkGetActiveConnections
            | Self::NetworkGetInterfaceCounters
            | Self::NetworkGetWifiStatus
            | Self::NetworkGetVpnStatus => AssistantDomainFamily::Network,
            Self::LibrariesListAccessible
            | Self::LibrariesGetLibrarySummary
            | Self::LibrarySearchTitles
            | Self::LibraryGetItemSummary
            | Self::LibraryGetItemMediaDetails
            | Self::LibraryGetItemSourcePaths
            | Self::LibraryGetItemExternalIds
            | Self::LibraryGetItemPlayHistory
            | Self::LibrariesGetRecentlyAdded
            | Self::LibrariesFindDuplicateTitles
            | Self::LibrariesListMissingMetadata => AssistantDomainFamily::Library,
            Self::WeatherGetCurrent
            | Self::WeatherGetForecast
            | Self::WeatherGetHistory
            | Self::WeatherResolveLocationAlias
            | Self::WeatherGetForecastForDate
            | Self::WeatherGetHourlyWindow
            | Self::WeatherGetRecentHistoryForDate => AssistantDomainFamily::Weather,
            Self::WebListCuratedSources
            | Self::WebSearchPublicWeb
            | Self::WebFetchPublicPageSummary
            | Self::WebFetchSourceWithCitation => AssistantDomainFamily::Web,
            Self::RoomsListActive | Self::RoomsListJoinable | Self::RoomsGetRoomSummary => {
                AssistantDomainFamily::Rooms
            }
            Self::SystemGetCurrentDateTime
            | Self::SystemGetHostRuntimeSummary
            | Self::SystemGetBackupSummary
            | Self::SystemGetServiceHealth
            | Self::SystemGetServiceDetail
            | Self::SystemGetServiceLogs
            | Self::SystemGetServiceDependencies
            | Self::SystemGetTranscodeSummary
            | Self::SystemGetStorageSummary
            | Self::SystemGetStoragePathDetail
            | Self::SystemGetMountDetail
            | Self::SystemGetRecentErrors
            | Self::SystemGetKernelInfo
            | Self::SystemGetCpuTopology
            | Self::SystemGetTemperatureSensors
            | Self::SystemGetBlockDeviceInventory
            | Self::SystemGetFilesystemTable
            | Self::SystemGetGpuInventory
            | Self::SystemGetPciDevices
            | Self::SystemGetUsbDevices
            | Self::SystemGetBootLogSummary
            | Self::SystemGetJournalSummary
            | Self::SystemGetProcessDetail
            | Self::SystemGetListenerDetail
            | Self::SystemGetDiskUsageDetail
            | Self::SystemGetPortConflicts
            | Self::SystemGetPortConflictDetail
            | Self::SystemGetFailedUnits
            | Self::SystemGetFailedUnitDetail
            | Self::SystemGetFailedServiceLogs
            | Self::SystemGetProcessTreeDetail => AssistantDomainFamily::System,
            Self::SystemGetAiRuntimeSummary
            | Self::AiListBackgroundJobs
            | Self::AiGetJobStatus
            | Self::AiGetToolRegistry
            | Self::AiGetGroundingSummary
            | Self::AiGetLastToolFailureReason => AssistantDomainFamily::AiRuntime,
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
                | Self::DictionaryListVisibleWorkspaces
                | Self::DictionaryBrowseWorkspacePeople
                | Self::DictionarySearchPeople
                | Self::DictionaryGetPersonBundle
                | Self::DictionaryResolveRelationshipReference
                | Self::CalendarListDateConflicts
                | Self::CalendarListFreeDays
                | Self::CalendarGetNextFreeDay
                | Self::CalendarGetEventByExactDateAndTitle
                | Self::CalendarGetEventSeriesSummary
                | Self::CalendarGetNextFreeSlot
                | Self::CalendarListBusySlots
                | Self::CalendarCountEvents
                | Self::CalendarListBusyDays
                | Self::CalendarListOverlappingEvents
                | Self::CalendarUpcomingBirthdays
                | Self::DownloadsListAvailableArtifacts
                | Self::DownloadsGetArtifactDetails
                | Self::DownloadsGetArtifactSource
                | Self::DownloadsGetReleaseNotes
                | Self::DownloadsGetArtifactChecksum
                | Self::DownloadsGetArtifactInstallSteps
                | Self::DownloadsGetArtifactCompatibility
                | Self::NetworkGetInterfaceDetails
                | Self::NetworkGetInterfaceByIp
                | Self::NetworkGetDefaultRoute
                | Self::NetworkGetHostnameAliases
                | Self::NetworkGetDnsServers
                | Self::NetworkGetRouteTable
                | Self::NetworkGetActiveConnections
                | Self::NetworkGetInterfaceCounters
                | Self::NetworkGetWifiStatus
                | Self::NetworkGetVpnStatus
                | Self::LibrariesListAccessible
                | Self::LibrariesGetLibrarySummary
                | Self::LibrarySearchTitles
                | Self::LibraryGetItemSummary
                | Self::LibraryGetItemMediaDetails
                | Self::LibraryGetItemSourcePaths
                | Self::LibrariesGetRecentlyAdded
                | Self::LibrariesFindDuplicateTitles
                | Self::LibrariesListMissingMetadata
                | Self::MemoryListRecentChanges
                | Self::MemoryListConflictingFacts
                | Self::MemoryGetEntityProvenance
                | Self::MemoryGetPersonSummary
                | Self::WebListCuratedSources
                | Self::WeatherResolveLocationAlias
                | Self::WeatherGetForecastForDate
                | Self::WeatherGetHourlyWindow
                | Self::WeatherGetRecentHistoryForDate
                | Self::RoomsListActive
                | Self::RoomsListJoinable
                | Self::ServersListMinecraftStatus
                | Self::SystemGetMountDetail
                | Self::SystemGetProcessDetail
                | Self::SystemGetListenerDetail
                | Self::SystemGetDiskUsageDetail
                | Self::SystemGetPortConflicts
                | Self::SystemGetPortConflictDetail
                | Self::SystemGetFailedUnits
                | Self::SystemGetFailedUnitDetail
                | Self::AiListBackgroundJobs
                | Self::AiGetJobStatus
                | Self::AiGetToolRegistry
                | Self::AiGetGroundingSummary
                | Self::AiGetLastToolFailureReason
        )
    }

    pub const fn ambiguity_prone(self) -> bool {
        matches!(
            self,
            Self::CalendarListEvents
                | Self::DictionaryListVisibleWorkspaces
                | Self::DictionaryBrowseWorkspacePeople
                | Self::DictionarySearchPeople
                | Self::DictionaryGetPersonBundle
                | Self::DictionaryResolveRelationshipReference
                | Self::CalendarUpcomingBirthdays
                | Self::CalendarGetEventDetails
                | Self::CalendarGetEventByExactDateAndTitle
                | Self::CalendarGetEventSeriesSummary
                | Self::CalendarGetNextFreeSlot
                | Self::CalendarListBusySlots
                | Self::CalendarListDateConflicts
                | Self::CalendarListFreeDays
                | Self::CalendarGetNextFreeDay
                | Self::CalendarGetNextEventTiming
                | Self::CalendarCountEvents
                | Self::CalendarListBusyDays
                | Self::CalendarListOverlappingEvents
                | Self::ChannelsGetTranscriptSummary
                | Self::DownloadsGetArtifactDetails
                | Self::DownloadsGetArtifactSource
                | Self::DownloadsGetReleaseNotes
                | Self::DownloadsGetArtifactChecksum
                | Self::DownloadsGetArtifactInstallSteps
                | Self::DownloadsGetArtifactCompatibility
                | Self::NetworkGetInterfaceDetails
                | Self::NetworkGetInterfaceByIp
                | Self::NetworkGetDefaultRoute
                | Self::NetworkGetHostnameAliases
                | Self::NetworkGetDnsServers
                | Self::LibrariesGetLibrarySummary
                | Self::LibrarySearchTitles
                | Self::LibraryGetItemSummary
                | Self::LibraryGetItemMediaDetails
                | Self::LibraryGetItemSourcePaths
                | Self::LibrariesFindDuplicateTitles
                | Self::WeatherGetCurrent
                | Self::WeatherGetForecast
                | Self::WeatherGetHistory
                | Self::WeatherResolveLocationAlias
                | Self::WeatherGetForecastForDate
                | Self::WeatherGetHourlyWindow
                | Self::WeatherGetRecentHistoryForDate
                | Self::RoomsGetRoomSummary
                | Self::MemoryGetEntityProvenance
                | Self::MemoryGetPersonSummary
                | Self::SystemGetServiceDetail
                | Self::SystemGetStorageSummary
                | Self::SystemGetStoragePathDetail
                | Self::SystemGetMountDetail
                | Self::SystemGetProcessDetail
                | Self::SystemGetListenerDetail
                | Self::SystemGetDiskUsageDetail
                | Self::SystemGetPortConflicts
                | Self::SystemGetPortConflictDetail
                | Self::SystemGetFailedUnits
                | Self::SystemGetFailedUnitDetail
                | Self::AiListBackgroundJobs
                | Self::AiGetJobStatus
                | Self::AiGetToolRegistry
                | Self::AiGetGroundingSummary
                | Self::AiGetLastToolFailureReason
                | Self::SystemGetKernelInfo
                | Self::SystemGetCpuTopology
                | Self::SystemGetTemperatureSensors
                | Self::SystemGetBlockDeviceInventory
                | Self::SystemGetFilesystemTable
                | Self::SystemGetGpuInventory
                | Self::SystemGetPciDevices
                | Self::SystemGetUsbDevices
                | Self::SystemGetBootLogSummary
                | Self::SystemGetJournalSummary
                | Self::ServersGetMinecraftServerSummary
        )
    }

    pub const fn freshness_sensitive(self) -> bool {
        matches!(
            self,
            Self::WeatherGetCurrent
                | Self::WeatherGetForecast
                | Self::WeatherGetHistory
                | Self::DictionaryResolveRelationshipReference
                | Self::NetworkGetTopologySummary
                | Self::NetworkGetInterfaceDetails
                | Self::NetworkGetInterfaceByIp
                | Self::NetworkGetDefaultRoute
                | Self::NetworkGetHostnameAliases
                | Self::NetworkGetDnsServers
                | Self::NetworkGetRouteTable
                | Self::NetworkGetActiveConnections
                | Self::NetworkGetInterfaceCounters
                | Self::NetworkGetWifiStatus
                | Self::NetworkGetVpnStatus
                | Self::CalendarListDateConflicts
                | Self::CalendarListFreeDays
                | Self::CalendarGetNextFreeDay
                | Self::CalendarGetNextEventTiming
                | Self::CalendarCountEvents
                | Self::CalendarListBusyDays
                | Self::CalendarListOverlappingEvents
                | Self::MemoryListRecentChanges
                | Self::MemoryListConflictingFacts
                | Self::WebSearchPublicWeb
                | Self::WebFetchPublicPageSummary
                | Self::DownloadsGetArtifactDetails
                | Self::DownloadsGetArtifactSource
                | Self::DownloadsGetReleaseNotes
                | Self::DownloadsGetArtifactChecksum
                | Self::DownloadsGetArtifactInstallSteps
                | Self::DownloadsGetArtifactCompatibility
                | Self::LibrariesGetLibrarySummary
                | Self::LibrariesFindDuplicateTitles
                | Self::LibrariesListMissingMetadata
                | Self::SystemGetServiceDetail
                | Self::RoomsListActive
                | Self::SystemGetAiRuntimeSummary
                | Self::SystemGetHostRuntimeSummary
                | Self::SystemGetServiceHealth
                | Self::SystemGetTranscodeSummary
                | Self::SystemGetStorageSummary
                | Self::SystemGetStoragePathDetail
                | Self::SystemGetMountDetail
                | Self::SystemGetProcessDetail
                | Self::SystemGetListenerDetail
                | Self::SystemGetDiskUsageDetail
                | Self::SystemGetRecentErrors
                | Self::SystemGetKernelInfo
                | Self::SystemGetCpuTopology
                | Self::SystemGetTemperatureSensors
                | Self::SystemGetBlockDeviceInventory
                | Self::SystemGetFilesystemTable
                | Self::SystemGetGpuInventory
                | Self::SystemGetPciDevices
                | Self::SystemGetUsbDevices
                | Self::SystemGetBootLogSummary
                | Self::SystemGetJournalSummary
                | Self::SystemGetPortConflicts
                | Self::SystemGetPortConflictDetail
                | Self::SystemGetFailedUnits
                | Self::SystemGetFailedUnitDetail
                | Self::ServersListMinecraftStatus
                | Self::ServersGetMinecraftServerSummary
        )
    }
}
