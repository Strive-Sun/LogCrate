export interface KeywordMatchOptions {
  wholeWord: boolean;
  caseSensitive: boolean;
}

export interface KeywordMatchRange {
  /** JavaScript UTF-16 code unit offset, inclusive. */
  startColumn: number;
  /** JavaScript UTF-16 code unit offset, exclusive. */
  endColumn: number;
}

interface SourceRange {
  startColumn: number;
  endColumn: number;
}

const WORD_CHARACTER = /[\p{Alphabetic}\p{Number}_]/u;

function characterBefore(text: string, column: number): string | undefined {
  if (column <= 0) return undefined;
  const last = text.charCodeAt(column - 1);
  if (last >= 0xdc00 && last <= 0xdfff && column >= 2) {
    const first = text.charCodeAt(column - 2);
    if (first >= 0xd800 && first <= 0xdbff) return text.slice(column - 2, column);
  }
  return text[column - 1];
}

function characterAfter(text: string, column: number): string | undefined {
  if (column >= text.length) return undefined;
  const first = text.charCodeAt(column);
  if (first >= 0xd800 && first <= 0xdbff && column + 1 < text.length) {
    const last = text.charCodeAt(column + 1);
    if (last >= 0xdc00 && last <= 0xdfff) return text.slice(column, column + 2);
  }
  return text[column];
}

function isWholeWord(text: string, startColumn: number, endColumn: number): boolean {
  const before = characterBefore(text, startColumn);
  const after = characterAfter(text, endColumn);
  return (!before || !WORD_CHARACTER.test(before)) && (!after || !WORD_CHARACTER.test(after));
}

function caseSensitiveMatches(text: string, query: string): KeywordMatchRange[] {
  const matches: KeywordMatchRange[] = [];
  let fromColumn = 0;
  while (fromColumn <= text.length - query.length) {
    const startColumn = text.indexOf(query, fromColumn);
    if (startColumn < 0) break;
    const endColumn = startColumn + query.length;
    matches.push({ startColumn, endColumn });
    fromColumn = endColumn;
  }
  return matches;
}

function foldedTextWithSourceRanges(text: string): [string, SourceRange[]] {
  let foldedText = '';
  const sourceRanges: SourceRange[] = [];
  let sourceColumn = 0;
  for (const sourceCharacter of text) {
    const startColumn = sourceColumn;
    sourceColumn += sourceCharacter.length;
    const foldedCharacter = sourceCharacter.toLowerCase();
    foldedText += foldedCharacter;
    for (let index = 0; index < foldedCharacter.length; index += 1) {
      sourceRanges.push({ startColumn, endColumn: sourceColumn });
    }
  }
  return [foldedText, sourceRanges];
}

function caseInsensitiveMatches(text: string, query: string): KeywordMatchRange[] {
  const [foldedText, sourceRanges] = foldedTextWithSourceRanges(text);
  const foldedQuery = query.toLowerCase();
  if (!foldedQuery) return [];

  const matches: KeywordMatchRange[] = [];
  let fromColumn = 0;
  while (fromColumn <= foldedText.length - foldedQuery.length) {
    const foldedStart = foldedText.indexOf(foldedQuery, fromColumn);
    if (foldedStart < 0) break;
    const foldedEnd = foldedStart + foldedQuery.length;
    const sourceStart = sourceRanges[foldedStart];
    const sourceEnd = sourceRanges[foldedEnd - 1];
    const beginsSourceCharacter =
      foldedStart === 0 || sourceRanges[foldedStart - 1].startColumn !== sourceStart.startColumn;
    const endsSourceCharacter =
      foldedEnd === sourceRanges.length ||
      sourceRanges[foldedEnd - 1].startColumn !== sourceRanges[foldedEnd].startColumn;
    if (beginsSourceCharacter && endsSourceCharacter) {
      matches.push({
        startColumn: sourceStart.startColumn,
        endColumn: sourceEnd.endColumn,
      });
    }
    fromColumn = foldedEnd;
  }
  return matches;
}

/** Returns every non-overlapping keyword fragment using JavaScript UTF-16 columns. */
export function findKeywordMatches(
  text: string,
  query: string,
  options: KeywordMatchOptions,
): KeywordMatchRange[] {
  if (!query) return [];
  const matches = options.caseSensitive
    ? caseSensitiveMatches(text, query)
    : caseInsensitiveMatches(text, query);
  return options.wholeWord
    ? matches.filter(({ startColumn, endColumn }) => isWholeWord(text, startColumn, endColumn))
    : matches;
}
