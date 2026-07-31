export interface SearchInputLatencyReport {
  sampleCount: number;
  p95Ms: number;
  samplesMs: number[];
  phase: string;
}

const MAX_SAMPLES = 200;
const REPORT_EVENT = 'logcrate:search-input-latency';

const samples: number[] = [];

function percentile95(values: number[]): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1);
  return sorted[index] ?? 0;
}

export function recordSearchInputLatency(startedAt: number, phase: string): SearchInputLatencyReport {
  const elapsed = Math.max(0, performance.now() - startedAt);
  samples.push(elapsed);
  if (samples.length > MAX_SAMPLES) samples.shift();
  const report: SearchInputLatencyReport = {
    sampleCount: samples.length,
    p95Ms: percentile95(samples),
    samplesMs: [...samples],
    phase,
  };
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new window.CustomEvent(REPORT_EVENT, { detail: report }));
    (window as Window & { __logcrateSearchInputLatency?: SearchInputLatencyReport }).__logcrateSearchInputLatency = report;
  }
  return report;
}

export function resetSearchInputLatencySamples(): void {
  samples.length = 0;
}

export const searchInputLatencyReportEvent = REPORT_EVENT;
