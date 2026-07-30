import type { FileSearchServiceErrorCode } from '../api/types';

const SERVICE_ERROR_CODES = new Set<FileSearchServiceErrorCode>([
  'missing',
  'accessDenied',
  'startFailed',
  'notReady',
  'protocolMismatch',
  'elevationCancelled',
  'repairExecutableMissing',
  'repairFailed',
]);

export class FileSearchServiceRepairError extends Error {
  constructor(
    readonly code: FileSearchServiceErrorCode,
    message: string,
  ) {
    super(message);
    this.name = 'FileSearchServiceRepairError';
  }
}

export function normalizeFileSearchServiceRepairError(
  reason: unknown,
): FileSearchServiceRepairError {
  if (reason instanceof FileSearchServiceRepairError) return reason;
  if (reason && typeof reason === 'object') {
    const raw = reason as { code?: unknown; message?: unknown };
    if (
      typeof raw.code === 'string' &&
      SERVICE_ERROR_CODES.has(raw.code as FileSearchServiceErrorCode)
    ) {
      return new FileSearchServiceRepairError(
        raw.code as FileSearchServiceErrorCode,
        typeof raw.message === 'string' ? raw.message : raw.code,
      );
    }
  }
  const message = reason instanceof Error ? reason.message : String(reason);
  const legacy = /^\[([A-Za-z]+)]\s*(.*)$/.exec(message);
  if (legacy && SERVICE_ERROR_CODES.has(legacy[1] as FileSearchServiceErrorCode)) {
    return new FileSearchServiceRepairError(
      legacy[1] as FileSearchServiceErrorCode,
      legacy[2] || legacy[1],
    );
  }
  return new FileSearchServiceRepairError('repairFailed', message);
}
