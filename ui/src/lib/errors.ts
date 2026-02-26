export type AppClientError = {
  message: string;
  code?: string;
  status?: number;
  details?: unknown;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function toClientError(error: unknown): AppClientError {
  if (error instanceof Error) {
    const withFields = error as Error & {
      code?: unknown;
      status?: unknown;
      details?: unknown;
    };
    return {
      message: withFields.message || 'Unexpected error',
      code: typeof withFields.code === 'string' ? withFields.code : undefined,
      status: typeof withFields.status === 'number' ? withFields.status : undefined,
      details: withFields.details,
    };
  }

  if (isRecord(error)) {
    const message =
      typeof error.message === 'string' && error.message.trim().length > 0
        ? error.message
        : 'Unexpected error';
    return {
      message,
      code: typeof error.code === 'string' ? error.code : undefined,
      status: typeof error.status === 'number' ? error.status : undefined,
      details: error.details,
    };
  }

  if (typeof error === 'string' && error.trim().length > 0) {
    return { message: error };
  }

  return { message: 'Unexpected error' };
}

export function clientErrorMessage(error: unknown, fallback: string): string {
  const message = toClientError(error).message?.trim();
  return message && message.length > 0 ? message : fallback;
}
