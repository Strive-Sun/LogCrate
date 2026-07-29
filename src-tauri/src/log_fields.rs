//! Bounded sampling, layout inference, and user correction for structured log fields.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const QUICK_SAMPLE_MAX_LINES: usize = 256;
pub const QUICK_SAMPLE_MAX_BYTES: usize = 256 * 1024;
pub const BACKGROUND_SAMPLE_MAX_LINES: usize = 10_000;
pub const BACKGROUND_SAMPLE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAIN_LAYOUT_THRESHOLD: f64 = 0.70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[serde(tag = "kind", rename_all = "camelCase")]
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

#[derive(Debug, Clone, PartialEq)]
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
    let spans = bracket_spans(line);
    if spans.len() != 1 || !line.starts_with('[') {
        return None;
    }
    let (content_start, content_end) = spans[0];
    let content = &line[content_start..content_end];
    let first_colon = content.find(':')?;
    let second_relative = content[first_colon + 1..].find(':')?;
    let second_colon = first_colon + 1 + second_relative;
    let timestamp = &content[..first_colon];
    let level = &content[first_colon + 1..second_colon];
    if !looks_like_chromium_time(timestamp) || !looks_like_level(level) {
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
                content_start + first_colon + 1,
                Some(content_start + second_colon),
            ),
            parsed_field(
                line,
                "来源",
                LogFieldType::Discrete,
                content_start + second_colon + 1,
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
    value.len() >= 6
        && value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'/')
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
    matches!(
        value.to_ascii_uppercase().as_str(),
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
    )
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
}
