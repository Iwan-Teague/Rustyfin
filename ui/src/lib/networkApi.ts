'use client';

import { apiJson } from './api';

export interface NetworkAddressSummary {
  family: string;
  address: string;
  scope?: string | null;
}

export interface NetworkNodeSummary {
  name: string;
  status: string;
  is_loopback: boolean;
  addresses: NetworkAddressSummary[];
}

export interface NetworkTopologySnapshot {
  available: boolean;
  reason?: string | null;
  host_label?: string | null;
  public_host?: string | null;
  remote_access_enabled: boolean;
  trusted_proxy_count: number;
  trusted_proxies?: string[] | null;
  online_node_count: number;
  offline_node_count: number;
  loopback_node_count: number;
  nodes: NetworkNodeSummary[];
}

export async function getNetworkTopology(): Promise<NetworkTopologySnapshot> {
  return apiJson<NetworkTopologySnapshot>('/system/network');
}
