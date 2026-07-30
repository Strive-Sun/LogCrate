import { useState } from 'react';
import type {
  LogFieldCondition,
  LogFieldDefinition,
  LogFieldLayout,
  LogFieldStatistics,
} from '../api';
import { useI18n } from '../i18n/I18nProvider';
import { displayLogMinute, formatPickerMinute, toPickerMinute } from '../util/logFieldDateTime';

interface Props {
  layout: LogFieldLayout | null;
  conditions: LogFieldCondition[];
  statistics: LogFieldStatistics[];
  scrollLeft: number;
  recognizing: boolean;
  onConditionsChange: (conditions: LogFieldCondition[]) => void;
  onLayoutChange: (
    layout: LogFieldLayout,
    trigger: 'boundary' | 'name' | 'type' | 'split' | 'merge',
  ) => void;
}

function conditionFor(conditions: LogFieldCondition[], fieldId: string) {
  return conditions.find((condition) => condition.fieldId === fieldId);
}

function replaceCondition(
  conditions: LogFieldCondition[],
  fieldId: string,
  condition: LogFieldCondition | null,
) {
  const next = conditions.filter((item) => item.fieldId !== fieldId);
  if (condition) next.push(condition);
  return next;
}

function updatedLayout(layout: LogFieldLayout, fields: LogFieldDefinition[]): LogFieldLayout {
  return { ...layout, fields, pattern: { kind: 'manualColumns' }, source: 'manual', confidence: 1 };
}

function fieldNameDisplayWidth(name: string) {
  return Array.from(name).reduce((width, character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return width + (codePoint >= 0x1100 ? 2 : 1);
  }, 0);
}

function commonFieldControlWidth(fields: LogFieldDefinition[]) {
  const fieldWidth = Math.max(4, ...fields.map((field) => field.displayWidth));
  const labelWidth = Math.max(...fields.map((field) => fieldNameDisplayWidth(field.name))) + 2;
  return `max(${fieldWidth}ch, calc(${labelWidth}ch + 16px))`;
}

interface MinuteDateTimePickerProps {
  label: string;
  value?: string;
  placeholder?: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChange: (value?: string) => void;
}

interface PickerMinuteParts {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
}

function parsePickerMinute(value: string): PickerMinuteParts | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  return {
    year: Number(match[1]),
    month: Number(match[2]),
    day: Number(match[3]),
    hour: Number(match[4]),
    minute: Number(match[5]),
  };
}

function currentPickerMinute(): PickerMinuteParts {
  const now = new Date();
  return {
    year: now.getFullYear(),
    month: now.getMonth() + 1,
    day: now.getDate(),
    hour: now.getHours(),
    minute: now.getMinutes(),
  };
}

function pickerMinuteValue(parts: PickerMinuteParts) {
  const pad = (part: number) => part.toString().padStart(2, '0');
  return `${parts.year}-${pad(parts.month)}-${pad(parts.day)}T${pad(parts.hour)}:${pad(parts.minute)}`;
}

function calendarDates(year: number, month: number) {
  const firstWeekday = (new Date(year, month - 1, 1).getDay() + 6) % 7;
  return Array.from({ length: 42 }, (_, index) => {
    const date = new Date(year, month - 1, index - firstWeekday + 1);
    return {
      year: date.getFullYear(),
      month: date.getMonth() + 1,
      day: date.getDate(),
      currentMonth: date.getMonth() === month - 1,
    };
  });
}

function CalendarIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 3v3m10-3v3M4.5 9h15M6 5h12a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z" />
      <path d="M8 13h2m4 0h2m-8 4h2m4 0h2" />
    </svg>
  );
}

function MinuteDateTimePicker({
  label,
  value,
  placeholder,
  open,
  onOpenChange,
  onChange,
}: MinuteDateTimePickerProps) {
  const { locale, t } = useI18n();
  const sample = value ?? placeholder;
  const pickerValue = toPickerMinute(value ?? placeholder);
  const displayValue = displayLogMinute(value);
  const selected = parsePickerMinute(pickerValue) ?? currentPickerMinute();
  const [viewMonth, setViewMonth] = useState(() => ({
    year: selected.year,
    month: selected.month,
  }));
  const localeName = locale === 'zh-CN' ? 'zh-CN' : 'en-US';
  const days = calendarDates(viewMonth.year, viewMonth.month);
  const today = currentPickerMinute();
  const monthLabel = new Intl.DateTimeFormat(localeName, {
    year: 'numeric',
    month: 'long',
  }).format(new Date(viewMonth.year, viewMonth.month - 1, 1));
  const weekdays = Array.from({ length: 7 }, (_, index) =>
    new Intl.DateTimeFormat(localeName, { weekday: 'short' }).format(new Date(2024, 0, index + 1)),
  );

  const commit = (next: PickerMinuteParts) => {
    const formatted = formatPickerMinute(pickerMinuteValue(next), sample);
    if (formatted) onChange(formatted);
  };

  const changeMonth = (offset: number) => {
    const date = new Date(viewMonth.year, viewMonth.month - 1 + offset, 1);
    setViewMonth({ year: date.getFullYear(), month: date.getMonth() + 1 });
  };

  const openCalendar = () => {
    setViewMonth({ year: selected.year, month: selected.month });
    onOpenChange(!open);
  };

  return (
    <div
      className={'log-field-datetime' + (open ? ' is-open' : '')}
      role="group"
      aria-label={label}
    >
      <div className="log-field-datetime-row">
        <button
          type="button"
          className="log-field-datetime-trigger"
          aria-haspopup="dialog"
          aria-expanded={open}
          aria-label={label}
          onClick={openCalendar}
        >
          <span className="log-field-datetime-icon">
            <CalendarIcon />
          </span>
          <span className="log-field-datetime-copy">
            <span className="log-field-datetime-label">{label}</span>
            <strong>{displayValue ?? t('fields.chooseDateTime')}</strong>
          </span>
          <span className="log-field-datetime-chevron" aria-hidden="true">
            {open ? '▴' : '▾'}
          </span>
        </button>
        {value && (
          <button
            type="button"
            className="log-field-datetime-clear"
            aria-label={t('fields.clearBoundary', { label })}
            onClick={() => onChange(undefined)}
          >
            ×
          </button>
        )}
      </div>
      {!value && placeholder && (
        <small className="log-field-datetime-hint">{displayLogMinute(placeholder)}</small>
      )}
      {open && (
        <div
          className="log-field-calendar"
          role="dialog"
          aria-label={t('fields.calendarFor', { label })}
        >
          <div className="log-field-calendar-head">
            <button
              type="button"
              className="log-field-calendar-nav"
              aria-label={t('fields.previousMonth')}
              onClick={() => changeMonth(-1)}
            >
              ‹
            </button>
            <strong>{monthLabel}</strong>
            <button
              type="button"
              className="log-field-calendar-nav"
              aria-label={t('fields.nextMonth')}
              onClick={() => changeMonth(1)}
            >
              ›
            </button>
          </div>
          <div className="log-field-calendar-weekdays" aria-hidden="true">
            {weekdays.map((weekday) => (
              <span key={weekday}>{weekday}</span>
            ))}
          </div>
          <div className="log-field-calendar-grid" role="grid">
            {days.map((date) => {
              const isSelected =
                selected.year === date.year &&
                selected.month === date.month &&
                selected.day === date.day;
              const isToday =
                today.year === date.year && today.month === date.month && today.day === date.day;
              const dateValue = new Date(date.year, date.month - 1, date.day);
              return (
                <button
                  type="button"
                  className={
                    'log-field-calendar-day' +
                    (date.currentMonth ? '' : ' is-adjacent') +
                    (isSelected ? ' is-selected' : '') +
                    (isToday ? ' is-today' : '')
                  }
                  aria-label={new Intl.DateTimeFormat(localeName, { dateStyle: 'long' }).format(
                    dateValue,
                  )}
                  aria-selected={isSelected}
                  role="gridcell"
                  key={`${date.year}-${date.month}-${date.day}`}
                  onClick={() => {
                    setViewMonth({ year: date.year, month: date.month });
                    commit({ ...selected, year: date.year, month: date.month, day: date.day });
                  }}
                >
                  {date.day}
                </button>
              );
            })}
          </div>
          <div className="log-field-calendar-time">
            <span>{t('fields.timeOfDay')}</span>
            <label>
              <select
                aria-label={t('fields.hour')}
                value={selected.hour}
                onChange={(event) =>
                  commit({ ...selected, hour: Number(event.currentTarget.value) })
                }
              >
                {Array.from({ length: 24 }, (_, hour) => (
                  <option value={hour} key={hour}>
                    {hour.toString().padStart(2, '0')}
                  </option>
                ))}
              </select>
              <span>{t('fields.hourUnit')}</span>
            </label>
            <span className="log-field-calendar-time-separator">:</span>
            <label>
              <select
                aria-label={t('fields.minute')}
                value={selected.minute}
                onChange={(event) =>
                  commit({ ...selected, minute: Number(event.currentTarget.value) })
                }
              >
                {Array.from({ length: 60 }, (_, minute) => (
                  <option value={minute} key={minute}>
                    {minute.toString().padStart(2, '0')}
                  </option>
                ))}
              </select>
              <span>{t('fields.minuteUnit')}</span>
            </label>
          </div>
          <div className="log-field-calendar-footer">
            <button
              type="button"
              onClick={() => {
                setViewMonth({ year: today.year, month: today.month });
                commit(today);
              }}
            >
              {t('fields.today')}
            </button>
            {value && (
              <button type="button" onClick={() => onChange(undefined)}>
                {t('fields.clearBoundaryShort')}
              </button>
            )}
            <button type="button" className="is-primary" onClick={() => onOpenChange(false)}>
              {t('fields.done')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export function LogFieldFilterBar({
  layout,
  conditions,
  statistics,
  scrollLeft,
  recognizing,
  onConditionsChange,
  onLayoutChange,
}: Props) {
  const { t } = useI18n();
  const [openField, setOpenField] = useState<string | null>(null);
  const [openTimeBoundary, setOpenTimeBoundary] = useState<string | null>(null);
  const [dragBoundary, setDragBoundary] = useState<number | null>(null);
  if (!layout) {
    return (
      <div className="log-field-bar is-empty" role="status">
        {t(recognizing ? 'fields.recognizing' : 'fields.unstable')}
      </div>
    );
  }

  const fieldControlWidth = commonFieldControlWidth(layout.fields);

  const rename = (field: LogFieldDefinition) => {
    const name = window.prompt(t('fields.renamePrompt'), field.name)?.trim();
    if (!name) return;
    onLayoutChange(
      updatedLayout(
        layout,
        layout.fields.map((item) => (item.id === field.id ? { ...item, name } : item)),
      ),
      'name',
    );
  };

  const split = (field: LogFieldDefinition) => {
    const visualEnd = field.boundary.end ?? field.boundary.start + field.displayWidth;
    if (visualEnd - field.boundary.start < 2) return;
    const splitAt = Math.floor((field.boundary.start + visualEnd) / 2);
    const index = layout.fields.findIndex((item) => item.id === field.id);
    const left = { ...field, boundary: { ...field.boundary, end: splitAt } };
    const right: LogFieldDefinition = {
      ...field,
      id: `${field.id}-split-${Date.now()}`,
      name: t('fields.newField'),
      boundary: { start: splitAt, end: field.boundary.end },
      displayWidth: Math.max(4, field.displayWidth - Math.floor(field.displayWidth / 2)),
    };
    left.displayWidth = Math.max(4, field.displayWidth - right.displayWidth);
    const fields = [...layout.fields];
    fields.splice(index, 1, left, right);
    onLayoutChange(updatedLayout(layout, fields), 'split');
  };

  const mergeRight = (field: LogFieldDefinition) => {
    const index = layout.fields.findIndex((item) => item.id === field.id);
    const right = layout.fields[index + 1];
    if (!right) return;
    const fields = [...layout.fields];
    fields.splice(index, 2, {
      ...field,
      boundary: { start: field.boundary.start, end: right.boundary.end },
      displayWidth: field.displayWidth + right.displayWidth,
    });
    onLayoutChange(updatedLayout(layout, fields), 'merge');
  };

  const resizeBoundary = (field: LogFieldDefinition, index: number, requested: number) => {
    const right = layout.fields[index + 1];
    if (!right || field.boundary.end === null) return;
    const maximum = right.boundary.end === null ? field.boundary.end + 80 : right.boundary.end - 1;
    const boundary = Math.max(field.boundary.start + 1, Math.min(maximum, requested));
    if (boundary === field.boundary.end) return;
    const fields = layout.fields.map((item, itemIndex) =>
      itemIndex === index
        ? {
            ...item,
            boundary: { ...item.boundary, end: boundary },
            displayWidth: Math.max(4, boundary - item.boundary.start),
          }
        : itemIndex === index + 1
          ? {
              ...item,
              boundary: { ...item.boundary, start: boundary },
              displayWidth: Math.max(
                4,
                (item.boundary.end ?? boundary + item.displayWidth) - boundary,
              ),
            }
          : item,
    );
    onLayoutChange(updatedLayout(layout, fields), 'boundary');
  };

  return (
    <div
      className="log-field-bar"
      aria-label={t('fields.bar')}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          setOpenTimeBoundary(null);
          setOpenField(null);
        }
      }}
    >
      {dragBoundary !== null && (
        <span
          className="log-field-drag-guide"
          style={{
            left: `calc(var(--log-gutter-width) + ${dragBoundary}ch - ${scrollLeft}px)`,
          }}
        />
      )}
      <div className="log-field-track">
        {layout.fields.map((field) => {
          const condition = conditionFor(conditions, field.id);
          const stats = statistics.find((item) => item.fieldId === field.id);
          const active = Boolean(condition);
          return (
            <div
              className={'log-field' + (active ? ' active' : '')}
              style={{
                width: fieldControlWidth,
                flexBasis: fieldControlWidth,
                flexGrow: 0,
                flexShrink: 1,
                minWidth: 0,
              }}
              key={field.id}
            >
              <button
                type="button"
                className="log-field-button"
                title={field.name}
                aria-expanded={openField === field.id}
                onDoubleClick={() => rename(field)}
                onClick={() => {
                  setOpenTimeBoundary(null);
                  setOpenField(openField === field.id ? null : field.id);
                }}
              >
                {field.name}
                {condition?.kind === 'discrete' ? `: ${condition.values.length}` : ''} ▾
              </button>
              {openField === field.id && (
                <div
                  className={'log-field-popover' + (field.fieldType === 'time' ? ' is-time' : '')}
                >
                  {field.fieldType === 'time' && (
                    <>
                      <MinuteDateTimePicker
                        label={t('fields.start')}
                        value={condition?.kind === 'time' ? condition.start : undefined}
                        placeholder={stats?.minTime}
                        open={openTimeBoundary === `${field.id}:start`}
                        onOpenChange={(open) =>
                          setOpenTimeBoundary(open ? `${field.id}:start` : null)
                        }
                        onChange={(start) => {
                          const end = condition?.kind === 'time' ? condition.end : undefined;
                          onConditionsChange(
                            replaceCondition(
                              conditions,
                              field.id,
                              start || end ? { kind: 'time', fieldId: field.id, start, end } : null,
                            ),
                          );
                        }}
                      />
                      <MinuteDateTimePicker
                        label={t('fields.end')}
                        value={condition?.kind === 'time' ? condition.end : undefined}
                        placeholder={stats?.maxTime}
                        open={openTimeBoundary === `${field.id}:end`}
                        onOpenChange={(open) =>
                          setOpenTimeBoundary(open ? `${field.id}:end` : null)
                        }
                        onChange={(end) => {
                          const start = condition?.kind === 'time' ? condition.start : undefined;
                          onConditionsChange(
                            replaceCondition(
                              conditions,
                              field.id,
                              start || end ? { kind: 'time', fieldId: field.id, start, end } : null,
                            ),
                          );
                        }}
                      />
                    </>
                  )}
                  {(field.fieldType === 'level' ||
                    (field.fieldType === 'discrete' && !stats?.highCardinality)) &&
                    stats?.candidates.map((candidate) => {
                      const selected =
                        condition?.kind === 'discrete' &&
                        condition.values.includes(candidate.value);
                      return (
                        <label className="log-field-choice" key={candidate.value}>
                          <input
                            type="checkbox"
                            checked={selected}
                            onChange={() => {
                              const previous =
                                condition?.kind === 'discrete' ? condition.values : [];
                              const values = selected
                                ? previous.filter((value) => value !== candidate.value)
                                : [...previous, candidate.value];
                              onConditionsChange(
                                replaceCondition(
                                  conditions,
                                  field.id,
                                  values.length
                                    ? { kind: 'discrete', fieldId: field.id, values }
                                    : null,
                                ),
                              );
                            }}
                          />
                          <span>{candidate.value}</span>
                          <small>{candidate.count}</small>
                        </label>
                      );
                    })}
                  {(field.fieldType === 'text' || stats?.highCardinality) && (
                    <>
                      <label>
                        {t('fields.contains')}
                        <input
                          value={condition?.kind === 'text' ? condition.query : ''}
                          onInput={(event) => {
                            const query = event.currentTarget.value;
                            onConditionsChange(
                              replaceCondition(
                                conditions,
                                field.id,
                                query
                                  ? {
                                      kind: 'text',
                                      fieldId: field.id,
                                      query,
                                      caseSensitive:
                                        condition?.kind === 'text' && condition.caseSensitive,
                                    }
                                  : null,
                              ),
                            );
                          }}
                        />
                      </label>
                      <label className="log-field-inline-option">
                        <input
                          type="checkbox"
                          checked={condition?.kind === 'text' && condition.caseSensitive}
                          disabled={condition?.kind !== 'text'}
                          onChange={(event) => {
                            if (condition?.kind !== 'text') return;
                            onConditionsChange(
                              replaceCondition(conditions, field.id, {
                                ...condition,
                                caseSensitive: event.target.checked,
                              }),
                            );
                          }}
                        />
                        {t('fields.caseSensitive')}
                      </label>
                    </>
                  )}
                  <div className="log-field-actions">
                    <button type="button" onClick={() => rename(field)}>
                      {t('fields.rename')}
                    </button>
                    <select
                      aria-label={t('fields.type')}
                      value={field.fieldType}
                      onChange={(event) => {
                        const fields = layout.fields.map((item) =>
                          item.id === field.id
                            ? {
                                ...item,
                                fieldType: event.target.value as LogFieldDefinition['fieldType'],
                              }
                            : item,
                        );
                        onLayoutChange(updatedLayout(layout, fields), 'type');
                      }}
                    >
                      {(['time', 'level', 'discrete', 'text'] as const).map((type) => (
                        <option key={type} value={type}>
                          {t(`fields.type.${type}`)}
                        </option>
                      ))}
                    </select>
                    <button type="button" onClick={() => split(field)}>
                      {t('fields.split')}
                    </button>
                    <button
                      type="button"
                      disabled={field === layout.fields.at(-1)}
                      onClick={() => mergeRight(field)}
                    >
                      {t('fields.mergeRight')}
                    </button>
                  </div>
                </div>
              )}
              {field !== layout.fields.at(-1) && field.boundary.end !== null && (
                <button
                  type="button"
                  className="log-field-resizer"
                  aria-label={t('fields.resize', { name: field.name })}
                  aria-valuenow={field.boundary.end}
                  onKeyDown={(event) => {
                    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
                    event.preventDefault();
                    resizeBoundary(
                      field,
                      layout.fields.findIndex((item) => item.id === field.id),
                      field.boundary.end! + (event.key === 'ArrowLeft' ? -1 : 1),
                    );
                  }}
                  onPointerDown={(event) => {
                    event.preventDefault();
                    const startX = event.clientX;
                    const index = layout.fields.findIndex((item) => item.id === field.id);
                    const original = field.boundary.end!;
                    const requestedBoundary = (clientX: number) =>
                      original + Math.round((clientX - startX) / 7.2);
                    const move = (pointerEvent: PointerEvent) => {
                      setDragBoundary(requestedBoundary(pointerEvent.clientX));
                    };
                    const cleanup = () => {
                      window.removeEventListener('pointermove', move);
                      window.removeEventListener('pointerup', finish);
                      window.removeEventListener('pointercancel', cancel);
                      setDragBoundary(null);
                    };
                    const finish = (pointerEvent: PointerEvent) => {
                      cleanup();
                      resizeBoundary(field, index, requestedBoundary(pointerEvent.clientX));
                    };
                    const cancel = () => cleanup();
                    setDragBoundary(original);
                    window.addEventListener('pointermove', move);
                    window.addEventListener('pointerup', finish, { once: true });
                    window.addEventListener('pointercancel', cancel, { once: true });
                  }}
                />
              )}
            </div>
          );
        })}
      </div>
      {layout.confidence === 0 && (
        <span className="log-field-warning" role="status">
          {t('fields.unstable')}
        </span>
      )}
    </div>
  );
}
