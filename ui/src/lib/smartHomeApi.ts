'use client';

import { apiJson } from './api';

export type SmartDeviceType = 'camera' | 'light' | 'door_lock' | 'alarm' | 'generic';

export interface SmartDevice {
  id: string;
  name: string;
  device_type: SmartDeviceType;
  room?: string | null;
  status: string;
  battery_level?: number | null;
  last_seen_ts?: number | null;
}

export interface SmartHomeSummary {
  available: boolean;
  provider?: string | null;
  devices: SmartDevice[];
}

export async function getSmartHomeState(): Promise<SmartHomeSummary> {
  return apiJson<SmartHomeSummary>('/system/smart-home');
}
