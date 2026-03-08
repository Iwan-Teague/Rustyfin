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
  world_name: string;
  gamemode: string;
  difficulty: string;
  max_memory_mb: number;
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

export function listMinecraftServerEvents(id: string, limit = 20) {
  return apiJson<MinecraftServerEvent[]>(
    `/servers/minecraft/instances/${id}/events?limit=${encodeURIComponent(String(limit))}`,
  );
}

export function createMinecraftServer(payload: CreateMinecraftServerPayload) {
  return apiJson<MinecraftServer>('/servers/minecraft/instances', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}
