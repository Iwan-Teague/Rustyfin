import { apiJson } from './api';

export type MinecraftServer = {
  id: string;
  display_name: string;
  slug: string;
  description?: string | null;
  owner_user_id: string;
  owner_display_name: string;
  install_mode: string;
  runtime_mode: string;
  desired_state: string;
  observed_state: string;
  health_state: string;
  instance_root: string;
  server_work_dir: string;
  systemd_unit_name: string;
  listen_host: string;
  listen_port: number;
  advertised_host?: string | null;
  advertised_port?: number | null;
  autostart: boolean;
  auto_stop_when_empty: boolean;
  auto_stop_idle_minutes?: number | null;
  current_player_count: number;
  max_player_count?: number | null;
  last_ready_ts?: number | null;
  last_started_ts?: number | null;
  last_stopped_ts?: number | null;
  last_exit_code?: number | null;
  last_error_summary?: string | null;
  created_ts: number;
  updated_ts: number;
  server_distribution: string;
  minecraft_version: string;
  java_path: string;
  world_name: string;
  gamemode: string;
  difficulty: string;
  hardcore: boolean;
  motd: string;
  min_memory_mb: number;
  max_memory_mb: number;
  online_mode: boolean;
  pvp: boolean;
  allow_flight: boolean;
  enable_command_block: boolean;
  white_list_enabled: boolean;
  current_user_role?: string | null;
};

export type MinecraftServerEvent = {
  id: string;
  instance_id: string;
  job_id?: string | null;
  actor_user_id?: string | null;
  level: string;
  event_kind: string;
  message: string;
  created_ts: number;
};

export type ServerLogLine = {
  ts_ms?: number | null;
  priority?: string | null;
  message: string;
};

export type MinecraftServerLogsResponse = {
  unit_name: string;
  lines: ServerLogLine[];
};

export type DiscoveryCandidate = {
  path: string;
  name: string;
  world_name?: string | null;
  motd?: string | null;
  server_properties_present: boolean;
  eula_present: boolean;
  top_level_jars: string[];
  last_modified_ts?: number | null;
};

export type MinecraftDiscoveryScanResponse = {
  roots: string[];
  scanned_root?: string | null;
  candidates: DiscoveryCandidate[];
};

export type MinecraftServerAction = 'start' | 'stop' | 'restart';

export type MinecraftServerActionResponse = {
  job_id: string;
  requested_action: MinecraftServerAction;
  message: string;
  instance: MinecraftServer;
};

export type MinecraftServerOperationResponse = {
  job_id: string;
  message: string;
  instance: MinecraftServer;
};

export type HostDirectoryListEntry = {
  name: string;
  path: string;
};

export type HostDirectoryListResponse = {
  current_path: string;
  parent_path?: string | null;
  roots: string[];
  directories: HostDirectoryListEntry[];
};

export type CreateMinecraftServerPayload = {
  display_name: string;
  description?: string;
  server_distribution: 'vanilla' | 'paper';
  minecraft_version: string;
  world_name: string;
  listen_port: number;
  gamemode: 'survival' | 'creative' | 'adventure' | 'spectator';
  difficulty: 'peaceful' | 'easy' | 'normal' | 'hard';
  hardcore: boolean;
  motd?: string;
  max_player_count: number;
  min_memory_mb: number;
  max_memory_mb: number;
  online_mode: boolean;
  pvp: boolean;
  allow_flight: boolean;
  enable_command_block: boolean;
  white_list_enabled: boolean;
  autostart: boolean;
  eula_accepted: boolean;
};

export function listMinecraftServers() {
  return apiJson<MinecraftServer[]>('/servers/minecraft/instances');
}

export function getMinecraftServer(id: string) {
  return apiJson<MinecraftServer>(`/servers/minecraft/instances/${id}`);
}

export function refreshMinecraftServerStatus(id: string) {
  return apiJson<MinecraftServer>(`/servers/minecraft/instances/${id}/status`);
}

export function listMinecraftServerEvents(id: string, limit = 20) {
  return apiJson<MinecraftServerEvent[]>(
    `/servers/minecraft/instances/${id}/events?limit=${encodeURIComponent(String(limit))}`,
  );
}

export function listMinecraftServerLogs(id: string, limit = 80) {
  return apiJson<MinecraftServerLogsResponse>(
    `/servers/minecraft/instances/${id}/logs?limit=${encodeURIComponent(String(limit))}`,
  );
}

export function createMinecraftServer(payload: CreateMinecraftServerPayload) {
  return apiJson<MinecraftServer>('/servers/minecraft/instances', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function requestMinecraftServerAction(id: string, action: MinecraftServerAction) {
  return apiJson<MinecraftServerActionResponse>(
    `/servers/minecraft/instances/${id}/actions/${action}`,
    {
      method: 'POST',
    },
  );
}

export function provisionMinecraftServer(id: string) {
  return apiJson<MinecraftServerOperationResponse>(`/servers/minecraft/instances/${id}/provision`, {
    method: 'POST',
  });
}

export function importMinecraftServer(id: string, sourcePath: string) {
  return apiJson<MinecraftServerOperationResponse>(`/servers/minecraft/instances/${id}/import`, {
    method: 'POST',
    body: JSON.stringify({ source_path: sourcePath }),
  });
}

export function scanMinecraftDiscoveryCandidates(rootPath?: string, limit = 64) {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  if (rootPath?.trim()) {
    params.set('root_path', rootPath.trim());
  }
  return apiJson<MinecraftDiscoveryScanResponse>(
    `/servers/minecraft/discovery/scan?${params.toString()}`,
  );
}

export function listBackendHostDirectories(path?: string) {
  const suffix = path?.trim()
    ? `?path=${encodeURIComponent(path.trim())}`
    : '';
  return apiJson<HostDirectoryListResponse>(`/system/host-directories${suffix}`);
}
