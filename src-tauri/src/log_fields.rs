//! Bounded sampling, layout inference, and user correction for structured log fields.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const QUICK_SAMPLE_MAX_LINES: usize = 256;
pub const QUICK_SAMPLE_MAX_BYTES: usize = 256 * 1024;
pub const BACKGROUND_SAMPLE_MAX_LINES: usize = 10_000;
pub const BACKGROUND_SAMPLE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAIN_LAYOUT_THRESHOLD: f64 = 0.70;
pub const MAX_DISCRETE_VALUES: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SamplingPhase {
    Quick,
    Background,
}

impl SamplingPhase {
    fn limits(self) -> (usize, usize) {
        match self {
            Self::Quick => (QUICK_SAMPLE_MAX_LINES, QUICK_SAMPLE_MAX_BYTES),
            Self::Background => (BACKGROUND_SAMPLE_MAX_LINES, BACKGROUND_SAMPLE_MAX_BYTES),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogFieldType {
    Time,
    Level,
    Discrete,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayoutSource {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldBoundary {
    pub start: usize,
    pub end: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldDefinition {
    pub id: String,
    pub name: String,
    pub field_type: LogFieldType,
    pub boundary: FieldBoundary,
    pub display_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutPattern {
    Bracketed { segment_count: usize },
    Chromium,
    AndroidLogcat,
    ManualColumns,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldLayout {
    pub fields: Vec<LogFieldDefinition>,
    pub pattern: LayoutPattern,
    pub confidence: f64,
    pub source: LayoutSource,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutAnalysis {
    pub layout: Option<LogFieldLayout>,
    pub sampled_non_empty_lines: usize,
    pub sampled_bytes: usize,
    pub main_layout_lines: usize,
    pub unparsed_lines: usize,
}

#[derive(Debug, Clone)]
struct ParsedField {
    name: &'static str,
    field_type: LogFieldType,
    start: usize,
    end: Option<usize>,
    display_width: usize,
}

#[derive(Debug, Clone)]
struct ParsedLine {
    key: String,
    pattern: LayoutPattern,
    fields: Vec<ParsedField>,
}

pub fn sampling_indices(total_lines: usize, phase: SamplingPhase) -> Vec<usize> {
    let (max_lines, _) = phase.limits();
    if total_lines <= max_lines {
        return (0..total_lines).collect();
    }
    if phase == SamplingPhase::Quick {
        return (0..max_lines).collect();
    }
    (0..max_lines)
        .map(|index| index * (total_lines - 1) / (max_lines - 1))
        .collect()
}

pub fn analyze_layout<F>(
    total_lines: usize,
    phase: SamplingPhase,
    mut read_line: F,
) -> LayoutAnalysis
where
    F: FnMut(usize) -> Option<String>,
{
    let (_, max_bytes) = phase.limits();
    let mut sampled_bytes = 0usize;
    let mut sampled_non_empty_lines = 0usize;
    let mut groups = BTreeMap::<String, Vec<ParsedLine>>::new();

    for index in sampling_indices(total_lines, phase) {
        let Some(line) = read_line(index) else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let line_bytes = line.len();
        if line_bytes > max_bytes.saturating_sub(sampled_bytes) {
            continue;
        }
        sampled_bytes += line_bytes;
        sampled_non_empty_lines += 1;
        if let Some(parsed) = parse_line(&line) {
            groups.entry(parsed.key.clone()).or_default().push(parsed);
        }
        if sampled_bytes == max_bytes {
            break;
        }
    }

    let main = groups
        .into_iter()
        .max_by(|(left_key, left), (right_key, right)| {
            left.len()
                .cmp(&right.len())
                .then_with(|| right_key.cmp(left_key))
        })
        .map(|(_, lines)| lines);
    let main_layout_lines = main.as_ref().map_or(0, Vec::len);
    let confidence = if sampled_non_empty_lines == 0 {
        0.0
    } else {
        main_layout_lines as f64 / sampled_non_empty_lines as f64
    };
    let layout = main
        .filter(|_| confidence + f64::EPSILON >= MAIN_LAYOUT_THRESHOLD)
        .map(|lines| build_layout(&lines, confidence));

    LayoutAnalysis {
        layout,
        sampled_non_empty_lines,
        sampled_bytes,
        main_layout_lines,
        unparsed_lines: sampled_non_empty_lines.saturating_sub(main_layout_lines),
    }
}

fn build_layout(lines: &[ParsedLine], confidence: f64) -> LogFieldLayout {
    let first = &lines[0];
    let fields = first
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let mut starts = lines
                .iter()
                .map(|line| line.fields[index].start)
                .collect::<Vec<_>>();
            starts.sort_unstable();
            let ends = lines
                .iter()
                .filter_map(|line| line.fields[index].end)
                .collect::<Vec<_>>();
            let end = if ends.len() == lines.len() {
                let mut sorted = ends;
                sorted.sort_unstable();
                Some(sorted[sorted.len() / 2])
            } else {
                None
            };
            let start = starts[starts.len() / 2];
            let mut display_widths = lines
                .iter()
                .map(|line| line.fields[index].display_width)
                .collect::<Vec<_>>();
            display_widths.sort_unstable();
            LogFieldDefinition {
                id: format!("field-{}", index + 1),
                name: field.name.to_string(),
                field_type: field.field_type,
                boundary: FieldBoundary { start, end },
                display_width: display_widths[display_widths.len() / 2].clamp(4, 80),
            }
        })
        .collect();
    LogFieldLayout {
        fields,
        pattern: first.pattern.clone(),
        confidence,
        source: LayoutSource::Automatic,
    }
}

fn parse_line(line: &str) -> Option<ParsedLine> {
    parse_chromium(line)
        .or_else(|| parse_bracketed(line))
        .or_else(|| parse_android_logcat(line))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LogFieldCondition {
    Discrete {
        field_id: String,
        values: Vec<String>,
    },
    Time {
        field_id: String,
        start: Option<String>,
        end: Option<String>,
    },
    Text {
        field_id: String,
        query: String,
        case_sensitive: bool,
    },
}

impl LogFieldCondition {
    fn field_id(&self) -> &str {
        match self {
            Self::Discrete { field_id, .. }
            | Self::Time { field_id, .. }
            | Self::Text { field_id, .. } => field_id,
        }
    }

    fn is_active(&self) -> bool {
        match self {
            Self::Discrete { values, .. } => !values.is_empty(),
            Self::Time { start, end, .. } => start.is_some() || end.is_some(),
            Self::Text { query, .. } => !query.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldCandidateValue {
    pub value: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldStatistics {
    pub field_id: String,
    pub candidates: Vec<FieldCandidateValue>,
    pub high_cardinality: bool,
    pub min_time: Option<String>,
    pub max_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFieldScanResult {
    /// Zero-based original line indexes, in source order.
    pub matched_lines: Vec<u64>,
    /// Zero-based original line indexes, in source order.
    pub unparsed_lines: Vec<u64>,
    pub statistics: Vec<LogFieldStatistics>,
    pub scanned_lines: u64,
}

#[derive(Debug, Clone)]
struct StatisticsAccumulator {
    values: BTreeMap<String, u64>,
    high_cardinality: bool,
    min_time: Option<String>,
    max_time: Option<String>,
}

impl StatisticsAccumulator {
    fn new() -> Self {
        Self {
            values: BTreeMap::new(),
            high_cardinality: false,
            min_time: None,
            max_time: None,
        }
    }
}

fn extracted_values(layout: &LogFieldLayout, line: &str) -> Option<Vec<String>> {
    if line.trim().is_empty() {
        return None;
    }
    let ranges = if layout.pattern == LayoutPattern::ManualColumns {
        layout
            .fields
            .iter()
            .map(|field| {
                let start = field.boundary.start;
                let end = field.boundary.end.unwrap_or(line.len()).min(line.len());
                (start <= end && line.is_char_boundary(start) && line.is_char_boundary(end))
                    .then_some((start, end))
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        let parsed = parse_line(line)?;
        if parsed.pattern != layout.pattern || parsed.fields.len() != layout.fields.len() {
            return None;
        }
        parsed
            .fields
            .into_iter()
            .map(|field| (field.start, field.end.unwrap_or(line.len()).min(line.len())))
            .collect()
    };
    let values = ranges
        .into_iter()
        .map(|(start, end)| line[start..end].trim().to_string())
        .collect::<Vec<_>>();
    let valid_times = layout
        .fields
        .iter()
        .zip(&values)
        .all(|(field, value)| field.field_type != LogFieldType::Time || looks_like_time(value));
    valid_times.then_some(values)
}

fn contains_text(value: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        value.contains(query)
    } else {
        value.to_lowercase().contains(&query.to_lowercase())
    }
}

fn parse_two_digits(bytes: &[u8], start: usize) -> Option<u8> {
    let tens = *bytes.get(start)?;
    let ones = *bytes.get(start + 1)?;
    (tens.is_ascii_digit() && ones.is_ascii_digit()).then_some((tens - b'0') * 10 + ones - b'0')
}

fn valid_minute_components(month: u8, day: u8, hour: u8, minute: u8) -> bool {
    (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MinuteKey {
    year: Option<u16>,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
}

fn minute_key(value: &str) -> Option<MinuteKey> {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() >= 16
        && matches!(bytes[4], b'-' | b'/')
        && bytes[7] == bytes[4]
        && matches!(bytes[10], b' ' | b'T')
        && bytes[13] == b':'
        && bytes[..4].iter().all(u8::is_ascii_digit)
    {
        let month = parse_two_digits(bytes, 5)?;
        let day = parse_two_digits(bytes, 8)?;
        let hour = parse_two_digits(bytes, 11)?;
        let minute = parse_two_digits(bytes, 14)?;
        let year = value[..4].parse().ok()?;
        return valid_minute_components(month, day, hour, minute).then_some(MinuteKey {
            year: Some(year),
            month,
            day,
            hour,
            minute,
        });
    }
    if bytes.len() >= 11
        && matches!(bytes[2], b'-' | b'/')
        && matches!(bytes[5], b' ' | b'T')
        && bytes[8] == b':'
    {
        let month = parse_two_digits(bytes, 0)?;
        let day = parse_two_digits(bytes, 3)?;
        let hour = parse_two_digits(bytes, 6)?;
        let minute = parse_two_digits(bytes, 9)?;
        return valid_minute_components(month, day, hour, minute).then_some(MinuteKey {
            year: None,
            month,
            day,
            hour,
            minute,
        });
    }
    if bytes.len() >= 9 && bytes[4] == b'/' {
        let month = parse_two_digits(bytes, 0)?;
        let day = parse_two_digits(bytes, 2)?;
        let hour = parse_two_digits(bytes, 5)?;
        let minute = parse_two_digits(bytes, 7)?;
        return valid_minute_components(month, day, hour, minute).then_some(MinuteKey {
            year: None,
            month,
            day,
            hour,
            minute,
        });
    }
    None
}

fn line_matches(
    layout: &LogFieldLayout,
    values: &[String],
    conditions: &[LogFieldCondition],
) -> bool {
    conditions
        .iter()
        .filter(|item| item.is_active())
        .all(|condition| {
            let Some(index) = layout
                .fields
                .iter()
                .position(|field| field.id == condition.field_id())
            else {
                return false;
            };
            let field = &layout.fields[index];
            let value = &values[index];
            match condition {
                LogFieldCondition::Discrete { values, .. } => {
                    if field.field_type == LogFieldType::Level {
                        let canonical = value.to_uppercase();
                        values
                            .iter()
                            .any(|selected| selected.to_uppercase() == canonical)
                    } else {
                        values.iter().any(|selected| selected == value)
                    }
                }
                LogFieldCondition::Time { start, end, .. } => {
                    let Some(value) = minute_key(value) else {
                        return false;
                    };
                    start.as_ref().map_or(
                        true,
                        |start| matches!(minute_key(start), Some(start) if value.year.is_some() == start.year.is_some() && value >= start),
                    ) && end.as_ref().map_or(
                        true,
                        |end| matches!(minute_key(end), Some(end) if value.year.is_some() == end.year.is_some() && value <= end),
                    )
                }
                LogFieldCondition::Text {
                    query,
                    case_sensitive,
                    ..
                } => contains_text(value, query, *case_sensitive),
            }
        })
}

fn update_statistics(
    layout: &LogFieldLayout,
    values: &[String],
    statistics: &mut [StatisticsAccumulator],
) {
    for ((field, value), accumulator) in layout.fields.iter().zip(values).zip(statistics) {
        match field.field_type {
            LogFieldType::Level | LogFieldType::Discrete => {
                if accumulator.high_cardinality {
                    continue;
                }
                let candidate = if field.field_type == LogFieldType::Level {
                    value.to_uppercase()
                } else {
                    value.clone()
                };
                if !accumulator.values.contains_key(&candidate)
                    && accumulator.values.len() == MAX_DISCRETE_VALUES
                {
                    accumulator.values.clear();
                    accumulator.high_cardinality = true;
                } else {
                    *accumulator.values.entry(candidate).or_insert(0) += 1;
                }
            }
            LogFieldType::Time => {
                if accumulator
                    .min_time
                    .as_ref()
                    .map_or(true, |current| value < current)
                {
                    accumulator.min_time = Some(value.clone());
                }
                if accumulator
                    .max_time
                    .as_ref()
                    .map_or(true, |current| value > current)
                {
                    accumulator.max_time = Some(value.clone());
                }
            }
            LogFieldType::Text => {}
        }
    }
}

/// Scan a bounded session source without retaining decoded lines. Returning `None` means the
/// caller cancelled this generation. Reader errors are propagated so callers can fall back to the
/// unfiltered source instead of publishing a partial result as complete.
pub fn scan_field_lines<R, C, P>(
    total_lines: u64,
    layout: &LogFieldLayout,
    conditions: &[LogFieldCondition],
    read_line: R,
    cancelled: C,
    progress: P,
) -> anyhow::Result<Option<LogFieldScanResult>>
where
    R: FnMut(u64) -> anyhow::Result<(String, bool)>,
    C: FnMut() -> bool,
    P: FnMut(u64, &[u64], &[u64]),
{
    extend_field_lines(
        total_lines,
        layout,
        conditions,
        None,
        read_line,
        cancelled,
        progress,
    )
}

pub fn extend_field_lines<R, C, P>(
    total_lines: u64,
    layout: &LogFieldLayout,
    conditions: &[LogFieldCondition],
    initial: Option<LogFieldScanResult>,
    mut read_line: R,
    mut cancelled: C,
    mut progress: P,
) -> anyhow::Result<Option<LogFieldScanResult>>
where
    R: FnMut(u64) -> anyhow::Result<(String, bool)>,
    C: FnMut() -> bool,
    P: FnMut(u64, &[u64], &[u64]),
{
    let start_line = initial.as_ref().map_or(0, |value| value.scanned_lines);
    let mut matched_lines = initial
        .as_ref()
        .map_or_else(Vec::new, |value| value.matched_lines.clone());
    let mut unparsed_lines = initial
        .as_ref()
        .map_or_else(Vec::new, |value| value.unparsed_lines.clone());
    let mut statistics = initial.map_or_else(
        || vec![StatisticsAccumulator::new(); layout.fields.len()],
        |value| {
            value
                .statistics
                .into_iter()
                .map(|field| StatisticsAccumulator {
                    values: field
                        .candidates
                        .into_iter()
                        .map(|candidate| (candidate.value, candidate.count))
                        .collect(),
                    high_cardinality: field.high_cardinality,
                    min_time: field.min_time,
                    max_time: field.max_time,
                })
                .collect()
        },
    );
    if statistics.len() != layout.fields.len() || start_line > total_lines {
        anyhow::bail!("field scan state does not match the current layout");
    }
    for line_index in start_line..total_lines {
        if cancelled() {
            return Ok(None);
        }
        let (line, truncated) = read_line(line_index)?;
        if truncated {
            unparsed_lines.push(line_index);
        } else if let Some(values) = extracted_values(layout, &line) {
            update_statistics(layout, &values, &mut statistics);
            if line_matches(layout, &values, conditions) {
                matched_lines.push(line_index);
            }
        } else {
            unparsed_lines.push(line_index);
        }
        let scanned = line_index + 1;
        if scanned % 256 == 0 || scanned == total_lines {
            progress(scanned, &matched_lines, &unparsed_lines);
        }
    }
    let statistics = layout
        .fields
        .iter()
        .zip(statistics)
        .map(|(field, accumulator)| LogFieldStatistics {
            field_id: field.id.clone(),
            candidates: accumulator
                .values
                .into_iter()
                .map(|(value, count)| FieldCandidateValue { value, count })
                .collect(),
            high_cardinality: accumulator.high_cardinality,
            min_time: accumulator.min_time,
            max_time: accumulator.max_time,
        })
        .collect();
    Ok(Some(LogFieldScanResult {
        matched_lines,
        unparsed_lines,
        statistics,
        scanned_lines: total_lines,
    }))
}

fn parsed_field(
    line: &str,
    name: &'static str,
    field_type: LogFieldType,
    start: usize,
    end: Option<usize>,
) -> ParsedField {
    let value_end = end.unwrap_or(line.len()).min(line.len());
    ParsedField {
        name,
        field_type,
        start,
        end,
        display_width: display_width(&line[start.min(value_end)..value_end]),
    }
}

fn bracket_spans(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'[') {
            break;
        }
        let Some(relative_end) = line[cursor + 1..].find(']') else {
            break;
        };
        let end = cursor + 1 + relative_end;
        spans.push((cursor + 1, end));
        cursor = end + 1;
    }
    spans
}

fn parse_chromium(line: &str) -> Option<ParsedLine> {
    if !line.starts_with('[') {
        return None;
    }
    // Chromium's header is the first bracketed segment. The message may
    // legitimately contain additional bracketed timestamps or metadata.
    let content_start = 1;
    let content_end = line[content_start..].find(']')? + content_start;
    let content = &line[content_start..content_end];
    let first_colon = content.find(':')?;
    let timestamp = &content[..first_colon];
    // Chromium normally uses `LEVEL:source`, but some builds emit
    // `LEVEL 0 source:line`. The level is the first token after the timestamp.
    let level_tail = &content[first_colon + 1..];
    let level_len = level_tail
        .find(|character: char| character == ':' || character.is_ascii_whitespace())
        .unwrap_or(level_tail.len());
    let level = &level_tail[..level_len];
    if !looks_like_chromium_time(timestamp) || !looks_like_level(level) {
        return None;
    }
    let level_start = content_start + first_colon + 1;
    let level_end = level_start + level_len;
    let source_start = if level_tail.as_bytes().get(level_len) == Some(&b':') {
        level_end + 1
    } else {
        level_end + level_tail[level_len..].len() - level_tail[level_len..].trim_start().len()
    };
    if source_start >= content_end {
        return None;
    }
    let body_start = trim_body_start(line, content_end + 1);
    Some(ParsedLine {
        key: "chromium".to_string(),
        pattern: LayoutPattern::Chromium,
        fields: vec![
            parsed_field(
                line,
                "时间",
                LogFieldType::Time,
                content_start,
                Some(content_start + first_colon),
            ),
            parsed_field(
                line,
                "级别",
                LogFieldType::Level,
                level_start,
                Some(level_end),
            ),
            parsed_field(
                line,
                "来源",
                LogFieldType::Discrete,
                source_start,
                Some(content_end),
            ),
            parsed_field(line, "正文", LogFieldType::Text, body_start, None),
        ],
    })
}

fn parse_bracketed(line: &str) -> Option<ParsedLine> {
    let spans = bracket_spans(line);
    if spans.len() < 2 {
        return None;
    }
    let first_value = &line[spans[0].0..spans[0].1];
    if !looks_like_time(first_value) {
        return None;
    }
    let mut fields = Vec::with_capacity(spans.len() + 1);
    for (index, (start, end)) in spans.iter().copied().enumerate() {
        let value = &line[start..end];
        let (name, field_type) = if index == 0 {
            ("时间", LogFieldType::Time)
        } else if index == 1 && looks_like_level(value) {
            ("级别", LogFieldType::Level)
        } else {
            ("字段", LogFieldType::Discrete)
        };
        fields.push(parsed_field(line, name, field_type, start, Some(end)));
    }
    let body_start = trim_body_start(line, spans.last()?.1 + 1);
    fields.push(parsed_field(
        line,
        "正文",
        LogFieldType::Text,
        body_start,
        None,
    ));
    Some(ParsedLine {
        key: format!("bracketed:{}", spans.len()),
        pattern: LayoutPattern::Bracketed {
            segment_count: spans.len(),
        },
        fields,
    })
}

#[derive(Clone, Copy)]
struct TokenSpan<'a> {
    value: &'a str,
    start: usize,
    end: usize,
}

fn token_spans(line: &str) -> Vec<TokenSpan<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        if character.is_whitespace() {
            if let Some(token_start) = start.take() {
                tokens.push(TokenSpan {
                    value: &line[token_start..index],
                    start: token_start,
                    end: index,
                });
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(token_start) = start {
        tokens.push(TokenSpan {
            value: &line[token_start..],
            start: token_start,
            end: line.len(),
        });
    }
    tokens
}

fn parse_android_logcat(line: &str) -> Option<ParsedLine> {
    let tokens = token_spans(line);
    if tokens.len() < 7
        || !looks_like_logcat_date(tokens[0].value)
        || !looks_like_logcat_time(tokens[1].value)
        || !tokens[2].value.bytes().all(|byte| byte.is_ascii_digit())
        || !tokens[3].value.bytes().all(|byte| byte.is_ascii_digit())
        || !looks_like_single_letter_level(tokens[4].value)
    {
        return None;
    }
    let (tag_end, body_start) = if tokens[5].value.ends_with(':') {
        (tokens[5].end - 1, tokens[6].start)
    } else if tokens[6].value == ":" && tokens.len() >= 8 {
        (tokens[5].end, tokens[7].start)
    } else {
        return None;
    };
    Some(ParsedLine {
        key: "android-logcat".to_string(),
        pattern: LayoutPattern::AndroidLogcat,
        fields: vec![
            parsed_field(
                line,
                "时间",
                LogFieldType::Time,
                tokens[0].start,
                Some(tokens[1].end),
            ),
            parsed_field(
                line,
                "PID",
                LogFieldType::Discrete,
                tokens[2].start,
                Some(tokens[2].end),
            ),
            parsed_field(
                line,
                "TID",
                LogFieldType::Discrete,
                tokens[3].start,
                Some(tokens[3].end),
            ),
            parsed_field(
                line,
                "级别",
                LogFieldType::Level,
                tokens[4].start,
                Some(tokens[4].end),
            ),
            parsed_field(
                line,
                "Tag",
                LogFieldType::Discrete,
                tokens[5].start,
                Some(tag_end),
            ),
            parsed_field(line, "正文", LogFieldType::Text, body_start, None),
        ],
    })
}

fn trim_body_start(line: &str, mut start: usize) -> usize {
    let bytes = line.as_bytes();
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if bytes.get(start) == Some(&b'-') {
        start += 1;
        while start < bytes.len() && bytes[start].is_ascii_whitespace() {
            start += 1;
        }
    }
    start
}

fn looks_like_time(value: &str) -> bool {
    value.len() >= 5
        && value.bytes().all(|byte| {
            byte.is_ascii_digit()
                || matches!(byte, b'-' | b'/' | b':' | b'.' | b' ' | b'+' | b'T' | b'Z')
        })
        && (value.contains(':') || value.contains('/') || value.contains('-'))
}

fn looks_like_chromium_time(value: &str) -> bool {
    let Some((date, time)) = value.split_once('/') else {
        return false;
    };
    if date.len() != 4 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let (clock, fraction) = match time.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (time, None),
    };
    clock.len() == 6
        && clock.bytes().all(|byte| byte.is_ascii_digit())
        && match fraction {
            Some(fraction) => {
                !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
            }
            None => true,
        }
}

fn looks_like_logcat_date(value: &str) -> bool {
    value.len() == 5
        && value.as_bytes()[2] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
}

fn looks_like_logcat_time(value: &str) -> bool {
    value.len() >= 8
        && value.contains(':')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b':' | b'.'))
}

fn looks_like_single_letter_level(value: &str) -> bool {
    matches!(value, "V" | "D" | "I" | "W" | "E" | "F" | "A")
}

fn looks_like_level(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "V" | "D"
            | "I"
            | "W"
            | "E"
            | "F"
            | "A"
            | "TRACE"
            | "DEBUG"
            | "INFO"
            | "NOTICE"
            | "WARN"
            | "WARNING"
            | "ERROR"
            | "FATAL"
            | "CRITICAL"
            | "SEVERE"
    ) {
        return true;
    }
    match upper.strip_prefix("VERBOSE") {
        Some(suffix) => suffix.bytes().all(|byte| byte.is_ascii_digit()),
        None => false,
    }
}

pub fn canonical_level_counts<'a, I>(values: I) -> BTreeMap<String, usize>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value.to_uppercase()).or_insert(0) += 1;
    }
    counts
}

pub fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| {
            let code = character as u32;
            if matches!(code, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff) {
                0
            } else if matches!(
                code,
                0x1100..=0x115f
                    | 0x2e80..=0xa4cf
                    | 0xac00..=0xd7a3
                    | 0xf900..=0xfaff
                    | 0xfe10..=0xfe6f
                    | 0xff00..=0xff60
                    | 0x1f300..=0x1faff
            ) {
                2
            } else {
                1
            }
        })
        .sum()
}

pub fn snap_to_char_boundary(value: &str, requested: usize) -> usize {
    let mut boundary = requested.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutEditError {
    InvalidField,
    InvalidBoundary,
    EmptyName,
}

#[derive(Debug, Clone)]
pub struct EditableLayout {
    pub layout: LogFieldLayout,
    frozen: bool,
}

impl EditableLayout {
    pub fn new(layout: LogFieldLayout) -> Self {
        Self {
            layout,
            frozen: false,
        }
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn freeze_for_interaction(&mut self) {
        self.frozen = true;
    }

    pub fn apply_background_layout(&mut self, candidate: LogFieldLayout) -> bool {
        if self.frozen || candidate.confidence <= self.layout.confidence {
            return false;
        }
        self.layout = candidate;
        true
    }

    pub fn drag_boundary(
        &mut self,
        left_index: usize,
        requested: usize,
        sample_line: &str,
    ) -> Result<usize, LayoutEditError> {
        if left_index + 1 >= self.layout.fields.len() {
            return Err(LayoutEditError::InvalidField);
        }
        let boundary = snap_to_char_boundary(sample_line, requested);
        let left_start = self.layout.fields[left_index].boundary.start;
        let right_end = self.layout.fields[left_index + 1]
            .boundary
            .end
            .unwrap_or(sample_line.len());
        if boundary <= left_start || boundary >= right_end {
            return Err(LayoutEditError::InvalidBoundary);
        }
        self.layout.fields[left_index].boundary.end = Some(boundary);
        self.layout.fields[left_index + 1].boundary.start = boundary;
        self.mark_manual();
        Ok(boundary)
    }

    pub fn rename_field(&mut self, index: usize, name: &str) -> Result<(), LayoutEditError> {
        let trimmed = name.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > 128
            || trimmed.chars().any(char::is_control)
        {
            return Err(LayoutEditError::EmptyName);
        }
        let field = self
            .layout
            .fields
            .get_mut(index)
            .ok_or(LayoutEditError::InvalidField)?;
        field.name = trimmed.to_string();
        self.mark_manual();
        Ok(())
    }

    pub fn change_field_type(
        &mut self,
        index: usize,
        field_type: LogFieldType,
    ) -> Result<(), LayoutEditError> {
        let field = self
            .layout
            .fields
            .get_mut(index)
            .ok_or(LayoutEditError::InvalidField)?;
        field.field_type = field_type;
        self.mark_manual();
        Ok(())
    }

    pub fn split_field(
        &mut self,
        index: usize,
        requested: usize,
        sample_line: &str,
    ) -> Result<usize, LayoutEditError> {
        let field = self
            .layout
            .fields
            .get(index)
            .cloned()
            .ok_or(LayoutEditError::InvalidField)?;
        let boundary = snap_to_char_boundary(sample_line, requested);
        let end = field.boundary.end.unwrap_or(sample_line.len());
        if boundary <= field.boundary.start || boundary >= end {
            return Err(LayoutEditError::InvalidBoundary);
        }
        let existing_ids = self
            .layout
            .fields
            .iter()
            .map(|item| item.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut suffix = self.layout.fields.len() + 1;
        let id = loop {
            let candidate = format!("field-{suffix}");
            if !existing_ids.contains(candidate.as_str()) {
                break candidate;
            }
            suffix += 1;
        };
        self.layout.fields[index].boundary.end = Some(boundary);
        self.layout.fields.insert(
            index + 1,
            LogFieldDefinition {
                id,
                name: format!("字段 {suffix}"),
                field_type: field.field_type,
                boundary: FieldBoundary {
                    start: boundary,
                    end: field.boundary.end,
                },
                display_width: end.saturating_sub(boundary),
            },
        );
        self.mark_manual();
        Ok(boundary)
    }

    pub fn merge_with_right(&mut self, left_index: usize) -> Result<(), LayoutEditError> {
        if left_index + 1 >= self.layout.fields.len() {
            return Err(LayoutEditError::InvalidField);
        }
        let right = self.layout.fields.remove(left_index + 1);
        self.layout.fields[left_index].boundary.end = right.boundary.end;
        self.layout.fields[left_index].display_width = right
            .boundary
            .end
            .unwrap_or(self.layout.fields[left_index].boundary.start + 24)
            .saturating_sub(self.layout.fields[left_index].boundary.start);
        self.mark_manual();
        Ok(())
    }

    fn mark_manual(&mut self) {
        self.frozen = true;
        self.layout.pattern = LayoutPattern::ManualColumns;
        self.layout.source = LayoutSource::Manual;
        self.layout.confidence = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze_lines(lines: &[String], phase: SamplingPhase) -> LayoutAnalysis {
        analyze_layout(lines.len(), phase, |index| lines.get(index).cloned())
    }

    #[test]
    fn quick_sampling_is_bounded_and_recognizes_bracketed_fields() {
        let lines = (0..2_000)
            .map(|index| {
                format!(
                    "[2026-06-05 15:39:01.545 +08:00] [info] [Main] [main] [main.js:681] - message {index}"
                )
            })
            .collect::<Vec<_>>();
        let mut reads = 0usize;
        let analysis = analyze_layout(lines.len(), SamplingPhase::Quick, |index| {
            reads += 1;
            lines.get(index).cloned()
        });
        assert_eq!(reads, QUICK_SAMPLE_MAX_LINES);
        assert_eq!(analysis.sampled_non_empty_lines, QUICK_SAMPLE_MAX_LINES);
        assert!(analysis.sampled_bytes <= QUICK_SAMPLE_MAX_BYTES);
        let layout = analysis.layout.unwrap();
        assert_eq!(
            layout.pattern,
            LayoutPattern::Bracketed { segment_count: 5 }
        );
        assert_eq!(layout.fields.len(), 6);
        assert_eq!(layout.fields[0].field_type, LogFieldType::Time);
        assert_eq!(layout.fields[1].field_type, LogFieldType::Level);
        assert_eq!(layout.fields.last().unwrap().field_type, LogFieldType::Text);
    }

    #[test]
    fn recognizes_chromium_and_android_logcat_without_scanning_message_words() {
        let chromium = vec![
            "[0401/172412:ERROR:proxy_service_factory.cc(128)] this is xxx error, please return"
                .to_string();
            8
        ];
        let layout = analyze_lines(&chromium, SamplingPhase::Quick)
            .layout
            .unwrap();
        assert_eq!(layout.pattern, LayoutPattern::Chromium);
        assert_eq!(layout.fields[1].field_type, LogFieldType::Level);
        assert_eq!(layout.fields[2].name, "来源");

        let logcat = vec!["07-14 11:29:29.871 15701 16159 I Ktor : message".to_string(); 8];
        let layout = analyze_lines(&logcat, SamplingPhase::Quick).layout.unwrap();
        assert_eq!(layout.pattern, LayoutPattern::AndroidLogcat);
        assert_eq!(layout.fields.len(), 6);
        assert_eq!(layout.fields[1].name, "PID");
        assert_eq!(layout.fields[3].field_type, LogFieldType::Level);

        let prose = vec!["this is xxx error, please return".to_string(); 8];
        assert!(analyze_lines(&prose, SamplingPhase::Quick).layout.is_none());
    }

    #[test]
    fn recognizes_numbered_chromium_verbose_levels_as_stable_layout() {
        let lines = vec![
            "[0722/134614.337:INFO:missevan_fm_kernel.cpp(121)] MissEvanFM: start",
            "[0722/134614.338:VERBOSE1:vector.cpp(222)] enable AVX2",
            "[0722/134614.401:INFO:httpdns.cc(382)] OnEffectiveConnectionTypeChanged",
            "[0722/134614.529:VERBOSE1:audio_capture.cpp(529)] current friendly name",
            "[0722/134619.355:WARNING:mic_capture.cpp(407)] Reset cur_time_stamp",
            "[0722/134619.387:VERBOSE1:audio_capture.cpp(566)] max block size changed",
            "[0722/134619.877:ERROR:cert_verify_proc_builtin.cc(602)] No net_fetcher",
            "[0722/134645.036:INFO:user_account.cpp(123)] login success",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

        let analysis = analyze_lines(&lines, SamplingPhase::Quick);
        assert_eq!(analysis.main_layout_lines, lines.len());
        assert_eq!(analysis.unparsed_lines, 0);
        let layout = analysis.layout.unwrap();
        assert_eq!(layout.pattern, LayoutPattern::Chromium);
        assert_eq!(layout.fields.len(), 4);

        let values = extracted_values(&layout, &lines[1]).unwrap();
        assert_eq!(values[0], "0722/134614.338");
        assert_eq!(values[1], "VERBOSE1");
        assert_eq!(values[2], "vector.cpp(222)");
        assert_eq!(values[3], "enable AVX2");
    }

    #[test]
    fn ignores_additional_brackets_in_chromium_message_body() {
        let lines = vec![
            "[0615/194037.082:VERBOSE1:bvclive_engine.cpp(143)] [2026-06-15T19:40:37.081 INFO 0 bvclive_api.cc:21 bvclive_live_open] bvclive_version: v1.12.6",
            "[0615/194037.083:INFO:httpdns.cc(382)] [network] OnEffectiveConnectionTypeChanged",
            "[0615/194037.084:WARNING:mic_capture.cpp(202)] [audio] transformer enabled",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

        let analysis = analyze_lines(&lines, SamplingPhase::Quick);
        assert_eq!(analysis.main_layout_lines, lines.len());
        assert_eq!(analysis.unparsed_lines, 0);
        assert_eq!(analysis.layout.unwrap().pattern, LayoutPattern::Chromium);
    }

    #[test]
    fn chromium_metadata_after_level_does_not_pollute_level_candidates() {
        let lines = vec![
            "[0722/135413.464:INFO 0 bvclive_api.cc:31 bvclive_live_open] input".to_string(),
            "[0722/135413.465:ERROR 0 encoder.cc:86 Open] failed".to_string(),
            "[0722/135413.466:ERROR 0 stats_reporter.cc:59 StatsReporter] failed".to_string(),
        ];
        let layout = analyze_layout(lines.len(), SamplingPhase::Quick, |index| {
            lines.get(index).cloned()
        })
        .layout
        .unwrap();
        assert_eq!(layout.pattern, LayoutPattern::Chromium);
        let result = scan_field_lines(
            lines.len() as u64,
            &layout,
            &[],
            |index| Ok((lines[index as usize].clone(), false)),
            || false,
            |_, _, _| {},
        )
        .unwrap()
        .unwrap();
        let candidates = &result.statistics[1].candidates;
        assert!(candidates
            .iter()
            .any(|candidate| candidate.value == "ERROR"));
        assert!(candidates
            .iter()
            .all(|candidate| !candidate.value.contains(' ')));
    }

    #[test]
    fn requires_a_seventy_percent_main_layout_and_reports_mixed_lines_unparsed() {
        let bracket = "[2026-06-05 15:39:01.545 +08:00] [INFO] [Main] - message".to_string();
        let logcat = "07-14 11:29:29.871 15701 16159 I Ktor : message".to_string();
        let mut accepted = vec![bracket.clone(); 7];
        accepted.extend(vec![logcat.clone(); 3]);
        let analysis = analyze_lines(&accepted, SamplingPhase::Quick);
        assert!(analysis.layout.is_some());
        assert_eq!(analysis.main_layout_lines, 7);
        assert_eq!(analysis.unparsed_lines, 3);
        assert!((analysis.layout.unwrap().confidence - 0.7).abs() < f64::EPSILON);

        let mut rejected = vec![bracket; 6];
        rejected.extend(vec![logcat; 4]);
        assert!(analyze_lines(&rejected, SamplingPhase::Quick)
            .layout
            .is_none());
    }

    #[test]
    fn level_values_merge_only_case_variants_and_discrete_values_remain_exact() {
        let counts = canonical_level_counts(["info", "Info", "INFO", "WARN", "WARNING"]);
        assert_eq!(counts.get("INFO"), Some(&3));
        assert_eq!(counts.get("WARN"), Some(&1));
        assert_eq!(counts.get("WARNING"), Some(&1));
        let discrete = ["Main", "main"].into_iter().collect::<BTreeSet<_>>();
        assert_eq!(discrete.len(), 2);
    }

    #[test]
    fn background_sampling_is_even_and_respects_line_and_byte_limits() {
        let total = 100_000usize;
        let indices = sampling_indices(total, SamplingPhase::Background);
        assert_eq!(indices.len(), BACKGROUND_SAMPLE_MAX_LINES);
        assert_eq!(indices[0], 0);
        assert_eq!(*indices.last().unwrap(), total - 1);
        assert!(indices.windows(2).all(|pair| pair[0] < pair[1]));

        let line = "[2026-06-05 15:39:01.545 +08:00] [INFO] [Main] - message";
        let analysis = analyze_layout(total, SamplingPhase::Background, |_| Some(line.to_string()));
        assert_eq!(
            analysis.sampled_non_empty_lines,
            BACKGROUND_SAMPLE_MAX_LINES
        );
        assert!(analysis.sampled_bytes <= BACKGROUND_SAMPLE_MAX_BYTES);
    }

    #[test]
    fn manual_edits_snap_to_utf8_boundaries_and_freeze_background_replacement() {
        let lines =
            vec!["[2026-06-05 15:39:01.545 +08:00] [INFO] [模块😀] - 正文".to_string(); 8];
        let automatic = analyze_lines(&lines, SamplingPhase::Quick).layout.unwrap();
        assert_eq!(automatic.fields[2].display_width, 6);
        let mut editable = EditableLayout::new(automatic.clone());
        let emoji = lines[0].find('😀').unwrap();
        assert_eq!(snap_to_char_boundary(&lines[0], emoji + 2), emoji);
        assert_eq!(display_width("模块😀e\u{301}"), 7);

        editable.rename_field(2, "模块").unwrap();
        assert_eq!(
            editable.rename_field(2, "\n"),
            Err(LayoutEditError::EmptyName)
        );
        editable.change_field_type(2, LogFieldType::Text).unwrap();
        assert!(editable.is_frozen());
        assert_eq!(editable.layout.source, LayoutSource::Manual);
        assert_eq!(editable.layout.pattern, LayoutPattern::ManualColumns);

        let mut better = automatic;
        better.confidence = 1.0;
        assert!(!editable.apply_background_layout(better));
    }

    #[test]
    fn manual_split_drag_and_merge_keep_valid_non_empty_fields() {
        let lines = vec!["[2026-06-05 15:39:01.545 +08:00] [INFO] [Main] - message".to_string(); 8];
        let mut editable =
            EditableLayout::new(analyze_lines(&lines, SamplingPhase::Quick).layout.unwrap());
        let original_len = editable.layout.fields.len();
        let message_index = original_len - 1;
        let message_start = editable.layout.fields[message_index].boundary.start;
        editable
            .split_field(message_index, message_start + 3, &lines[0])
            .unwrap();
        assert_eq!(editable.layout.fields.len(), original_len + 1);
        editable.merge_with_right(message_index).unwrap();
        assert_eq!(editable.layout.fields.len(), original_len);

        let boundary = editable.layout.fields[0].boundary.end.unwrap();
        editable.drag_boundary(0, boundary + 1, &lines[0]).unwrap();
        assert_eq!(editable.layout.fields[0].boundary.end, Some(boundary + 1));
        assert_eq!(
            editable.drag_boundary(0, 0, &lines[0]),
            Err(LayoutEditError::InvalidBoundary)
        );
    }

    #[test]
    fn background_layout_can_improve_only_before_user_interaction() {
        let lines = vec!["[2026-06-05 15:39:01.545 +08:00] [INFO] [Main] - message".to_string(); 8];
        let quick = analyze_lines(&lines, SamplingPhase::Quick).layout.unwrap();
        let mut state = EditableLayout::new(LogFieldLayout {
            confidence: 0.7,
            ..quick.clone()
        });
        let improved = LogFieldLayout {
            confidence: 0.9,
            ..quick
        };
        assert!(state.apply_background_layout(improved.clone()));
        state.freeze_for_interaction();
        assert!(!state.apply_background_layout(LogFieldLayout {
            confidence: 1.0,
            ..improved
        }));
    }

    #[test]
    fn field_scan_applies_same_field_or_cross_field_and_and_closed_time_ranges() {
        let lines = [
            "[2026-06-05 10:00:00] [INFO] [Main] - first".to_string(),
            "[2026-06-05 11:00:00] [warn] [Main] - second".to_string(),
            "[2026-06-05 12:00:00] [ERROR] [Main] - third".to_string(),
            "[2026-06-05 11:30:00] [INFO] [Worker] - fourth".to_string(),
            "    at stack frame".to_string(),
        ];
        let layout = analyze_layout(4, SamplingPhase::Quick, |index| lines.get(index).cloned())
            .layout
            .unwrap();
        let conditions = vec![
            LogFieldCondition::Discrete {
                field_id: "field-2".into(),
                values: vec!["INFO".into(), "WARN".into()],
            },
            LogFieldCondition::Discrete {
                field_id: "field-3".into(),
                values: vec!["Main".into()],
            },
            LogFieldCondition::Time {
                field_id: "field-1".into(),
                start: Some("2026-06-05 10:00:00".into()),
                end: Some("2026-06-05 11:00:00".into()),
            },
        ];
        let result = scan_field_lines(
            lines.len() as u64,
            &layout,
            &conditions,
            |index| Ok((lines[index as usize].clone(), false)),
            || false,
            |_, _, _| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.matched_lines, vec![0, 1]);
        assert_eq!(result.unparsed_lines, vec![4]);
        assert_eq!(result.statistics[1].candidates[0].value, "ERROR");
        assert_eq!(result.statistics[1].candidates[1].value, "INFO");
        assert_eq!(result.statistics[1].candidates[1].count, 2);
        assert_eq!(
            result.statistics[0].min_time.as_deref(),
            Some("2026-06-05 10:00:00")
        );
        assert_eq!(
            result.statistics[0].max_time.as_deref(),
            Some("2026-06-05 12:00:00")
        );
    }

    #[test]
    fn time_conditions_compare_at_minute_precision_without_seconds() {
        let with_year = MinuteKey {
            year: Some(2026),
            month: 7,
            day: 28,
            hour: 17,
            minute: 18,
        };
        let without_year = MinuteKey {
            year: None,
            ..with_year
        };
        assert_eq!(minute_key("2026-07-28 17:18:41.181"), Some(with_year));
        assert_eq!(minute_key("07-28 17:18:59.999"), Some(without_year));
        assert_eq!(minute_key("0728/171859"), Some(without_year));
        assert_eq!(minute_key("2026-07-28 24:00"), None);

        let lines = [
            "[2026-07-28 17:17:59.999] [INFO] - before".to_string(),
            "[2026-07-28 17:18:00.000] [INFO] - first".to_string(),
            "[2026-07-28 17:18:59.999] [INFO] - last".to_string(),
            "[2026-07-28 17:19:00.000] [INFO] - after".to_string(),
        ];
        let layout = analyze_layout(lines.len(), SamplingPhase::Quick, |index| {
            lines.get(index).cloned()
        })
        .layout
        .unwrap();
        let conditions = [LogFieldCondition::Time {
            field_id: "field-1".into(),
            start: Some("2026-07-28 17:18".into()),
            end: Some("2026-07-28 17:18".into()),
        }];
        let result = scan_field_lines(
            lines.len() as u64,
            &layout,
            &conditions,
            |index| Ok((lines[index as usize].clone(), false)),
            || false,
            |_, _, _| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.matched_lines, vec![1, 2]);
    }

    #[test]
    fn high_cardinality_candidates_are_removed_instead_of_publishing_a_partial_list() {
        let lines = (0..=MAX_DISCRETE_VALUES)
            .map(|index| format!("[2026-06-05 10:00:00] [INFO] [Module-{index}] - message"))
            .collect::<Vec<_>>();
        let layout = analyze_layout(lines.len(), SamplingPhase::Quick, |index| {
            lines.get(index).cloned()
        })
        .layout
        .unwrap();
        let result = scan_field_lines(
            lines.len() as u64,
            &layout,
            &[],
            |index| Ok((lines[index as usize].clone(), false)),
            || false,
            |_, _, _| {},
        )
        .unwrap()
        .unwrap();
        assert!(result.statistics[2].high_cardinality);
        assert!(result.statistics[2].candidates.is_empty());
    }

    #[test]
    fn field_scan_cancellation_and_reader_failure_never_return_partial_success() {
        let lines = vec!["[2026-06-05 10:00:00] [INFO] [Main] - message".to_string(); 600];
        let layout = analyze_lines(&lines, SamplingPhase::Quick).layout.unwrap();
        let reads = std::cell::Cell::new(0usize);
        let cancelled = scan_field_lines(
            lines.len() as u64,
            &layout,
            &[],
            |index| {
                reads.set(reads.get() + 1);
                Ok((lines[index as usize].clone(), false))
            },
            || reads.get() >= 10,
            |_, _, _| {},
        )
        .unwrap();
        assert!(cancelled.is_none());

        let failed = scan_field_lines(
            lines.len() as u64,
            &layout,
            &[],
            |index| {
                if index == 4 {
                    anyhow::bail!("injected read failure");
                }
                Ok((lines[index as usize].clone(), false))
            },
            || false,
            |_, _, _| {},
        );
        assert!(failed.is_err());
    }

    #[test]
    fn field_layout_and_conditions_use_the_typescript_camel_case_contract() {
        let pattern = serde_json::to_value(LayoutPattern::Bracketed { segment_count: 3 }).unwrap();
        assert_eq!(pattern["kind"], "bracketed");
        assert_eq!(pattern["segmentCount"], 3);
        let condition = serde_json::to_value(LogFieldCondition::Text {
            field_id: "field-4".into(),
            query: "Error".into(),
            case_sensitive: true,
        })
        .unwrap();
        assert_eq!(condition["kind"], "text");
        assert_eq!(condition["fieldId"], "field-4");
        assert_eq!(condition["caseSensitive"], true);
        let decoded: LogFieldCondition = serde_json::from_value(condition).unwrap();
        assert!(matches!(decoded, LogFieldCondition::Text { .. }));
    }
}
