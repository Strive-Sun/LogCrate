export type LogMinuteFormat =
  | { kind: 'yearSeparated'; dateSeparator: '-' | '/'; dateTimeSeparator: ' ' | 'T' }
  | { kind: 'monthDay'; dateSeparator: '-' | '/'; dateTimeSeparator: ' ' | 'T' }
  | { kind: 'chromium' };

interface MinuteParts {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  format: LogMinuteFormat;
}

function validParts(parts: Omit<MinuteParts, 'format'>) {
  const value = new Date(parts.year, parts.month - 1, parts.day, parts.hour, parts.minute);
  return (
    value.getFullYear() === parts.year &&
    value.getMonth() === parts.month - 1 &&
    value.getDate() === parts.day &&
    value.getHours() === parts.hour &&
    value.getMinutes() === parts.minute
  );
}

function pad(value: number) {
  return value.toString().padStart(2, '0');
}

export function parseLogMinute(value: string, referenceYear = new Date().getFullYear()) {
  const trimmed = value.trim();
  const yearSeparated = /^(\d{4})([-/])(\d{2})\2(\d{2})([ T])(\d{2}):(\d{2})/.exec(trimmed);
  if (yearSeparated) {
    const parts: MinuteParts = {
      year: Number(yearSeparated[1]),
      month: Number(yearSeparated[3]),
      day: Number(yearSeparated[4]),
      hour: Number(yearSeparated[6]),
      minute: Number(yearSeparated[7]),
      format: {
        kind: 'yearSeparated',
        dateSeparator: yearSeparated[2] as '-' | '/',
        dateTimeSeparator: yearSeparated[5] as ' ' | 'T',
      },
    };
    return validParts(parts) ? parts : null;
  }

  const monthDay = /^(\d{2})([-/])(\d{2})([ T])(\d{2}):(\d{2})/.exec(trimmed);
  if (monthDay) {
    const parts: MinuteParts = {
      year: referenceYear,
      month: Number(monthDay[1]),
      day: Number(monthDay[3]),
      hour: Number(monthDay[5]),
      minute: Number(monthDay[6]),
      format: {
        kind: 'monthDay',
        dateSeparator: monthDay[2] as '-' | '/',
        dateTimeSeparator: monthDay[4] as ' ' | 'T',
      },
    };
    return validParts(parts) ? parts : null;
  }

  const chromium = /^(\d{2})(\d{2})\/(\d{2})(\d{2})/.exec(trimmed);
  if (chromium) {
    const parts: MinuteParts = {
      year: referenceYear,
      month: Number(chromium[1]),
      day: Number(chromium[2]),
      hour: Number(chromium[3]),
      minute: Number(chromium[4]),
      format: { kind: 'chromium' },
    };
    return validParts(parts) ? parts : null;
  }
  return null;
}

export function toPickerMinute(value?: string, referenceYear?: number) {
  if (!value) return '';
  const parts = parseLogMinute(value, referenceYear);
  if (!parts) return '';
  return `${parts.year}-${pad(parts.month)}-${pad(parts.day)}T${pad(parts.hour)}:${pad(parts.minute)}`;
}

export function formatPickerMinute(value: string, sample?: string) {
  const selected = parseLogMinute(value);
  if (!selected) return undefined;
  const sampleFormat = sample ? parseLogMinute(sample, selected.year)?.format : undefined;
  const format = sampleFormat ?? {
    kind: 'yearSeparated',
    dateSeparator: '-',
    dateTimeSeparator: ' ',
  };
  const month = pad(selected.month);
  const day = pad(selected.day);
  const hour = pad(selected.hour);
  const minute = pad(selected.minute);
  if (format.kind === 'chromium') return `${month}${day}/${hour}${minute}`;
  const dateTime = `${hour}:${minute}`;
  if (format.kind === 'monthDay') {
    return `${month}${format.dateSeparator}${day}${format.dateTimeSeparator}${dateTime}`;
  }
  return `${selected.year}${format.dateSeparator}${month}${format.dateSeparator}${day}${format.dateTimeSeparator}${dateTime}`;
}

export function displayLogMinute(value?: string) {
  if (!value) return undefined;
  const picker = toPickerMinute(value);
  return picker ? formatPickerMinute(picker, value) : undefined;
}
