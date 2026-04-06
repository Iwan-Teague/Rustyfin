import type {
  CredentialCapturePayload,
  PageContextPayload,
  PendingItemDraft,
  PopupState,
} from './types.js';

export type BackgroundRequest =
  | { type: 'set-server-url'; serverBaseUrl: string }
  | { type: 'save-settings'; settings: Record<string, unknown> }
  | { type: 'pair-device'; pairingInput: string; deviceName: string }
  | { type: 'unlock-vault'; masterPassword: string }
  | { type: 'lock-vault' }
  | { type: 'get-popup-state' }
  | { type: 'ensure-site-permission'; url: string; tabId?: number }
  | { type: 'page-context'; payload: PageContextPayload }
  | { type: 'credential-capture'; payload: CredentialCapturePayload }
  | { type: 'fill-item'; itemId: string; tabId?: number }
  | { type: 'save-pending-item'; tabId?: number; draft?: Partial<PendingItemDraft> }
  | { type: 'dismiss-pending-item'; tabId?: number }
  | { type: 'get-inline-state'; tabId?: number; url: string }
  | { type: 'generate-password'; tabId?: number; url: string; pageKind: string }
  | { type: 'notify-inline-dismissed'; tabId?: number };

export type BackgroundResponse =
  | { ok: true; state?: PopupState; settings?: Record<string, unknown>; granted?: boolean; password?: string }
  | { ok: false; error: string };

export type ContentRequest =
  | {
      type: 'fill-credentials';
      payload: {
        username: string;
        email: string;
        password: string;
      };
    }
  | {
      type: 'show-save-prompt';
      payload: {
        kind: 'save_new' | 'update_existing' | 'add_uri';
        message: string;
      };
    }
  | {
      type: 'dismiss-save-prompt';
    }
  | {
      type: 'generated-password';
      payload: {
        password: string;
      };
    };
