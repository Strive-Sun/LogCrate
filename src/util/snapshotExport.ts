import type { SnapshotExportResult } from '../api/types';

export async function exportSnapshotAfterSelection(
  selectDestination: () => Promise<string | null>,
  exportTo: (destination: string) => Promise<SnapshotExportResult>,
): Promise<SnapshotExportResult | null> {
  const destination = await selectDestination();
  if (!destination) return null;
  return exportTo(destination);
}
