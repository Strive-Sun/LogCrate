import { Fragment, useState } from 'react';
import type { ReactNode } from 'react';

interface Props {
  content: string;
}

interface MarkdownBlock {
  kind: 'code' | 'heading' | 'paragraph' | 'unordered-list' | 'ordered-list' | 'rule';
  content?: string;
  items?: string[];
  level?: number;
  language?: string;
}

function isBlockStart(line: string): boolean {
  const trimmed = line.trim();
  return (
    /^```/.test(trimmed) ||
    /^#{1,3}\s+/.test(trimmed) ||
    /^(?:---|\*\*\*)$/.test(trimmed) ||
    /^\*\*[^*]+\*\*$/.test(trimmed) ||
    /^[-*]\s+/.test(trimmed) ||
    /^\d+\.\s+/.test(trimmed)
  );
}

export function parseAiMarkdown(content: string): MarkdownBlock[] {
  const lines = content.replace(/\r\n?/g, '\n').split('\n');
  const blocks: MarkdownBlock[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];
    const trimmed = line.trim();
    if (!trimmed) {
      index += 1;
      continue;
    }

    const fence = trimmed.match(/^```([^`]*)$/);
    if (fence) {
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !/^```\s*$/.test(lines[index].trim())) {
        code.push(lines[index]);
        index += 1;
      }
      if (index < lines.length) index += 1;
      blocks.push({
        kind: 'code',
        content: code.join('\n'),
        language: fence[1].trim(),
      });
      continue;
    }

    const heading = trimmed.match(/^(#{1,3})\s+(.+)$/);
    if (heading) {
      blocks.push({ kind: 'heading', level: heading[1].length, content: heading[2] });
      index += 1;
      continue;
    }

    if (/^(?:---|\*\*\*)$/.test(trimmed)) {
      blocks.push({ kind: 'rule' });
      index += 1;
      continue;
    }

    if (/^\*\*[^*]+\*\*$/.test(trimmed)) {
      blocks.push({ kind: 'paragraph', content: trimmed });
      index += 1;
      continue;
    }

    const unordered = trimmed.match(/^[-*]\s+(.+)$/);
    if (unordered) {
      const items: string[] = [];
      while (index < lines.length) {
        const item = lines[index].trim().match(/^[-*]\s+(.+)$/);
        if (!item) break;
        items.push(item[1]);
        index += 1;
      }
      blocks.push({ kind: 'unordered-list', items });
      continue;
    }

    const ordered = trimmed.match(/^\d+\.\s+(.+)$/);
    if (ordered) {
      const items: string[] = [];
      while (index < lines.length) {
        const item = lines[index].trim().match(/^\d+\.\s+(.+)$/);
        if (!item) break;
        items.push(item[1]);
        index += 1;
      }
      blocks.push({ kind: 'ordered-list', items });
      continue;
    }

    const paragraph = [trimmed];
    index += 1;
    while (index < lines.length && lines[index].trim() && !isBlockStart(lines[index])) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    blocks.push({ kind: 'paragraph', content: paragraph.join(' ') });
  }

  return blocks;
}

function renderInline(text: string): ReactNode {
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).filter(Boolean);
  return parts.map((part, index) => {
    if (part.startsWith('**') && part.endsWith('**')) {
      return <strong key={index}>{part.slice(2, -2)}</strong>;
    }
    if (part.startsWith('`') && part.endsWith('`')) {
      return <code key={index}>{part.slice(1, -1)}</code>;
    }
    return <Fragment key={index}>{part}</Fragment>;
  });
}

async function copyCode(text: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch {
    // Fall back to the synchronous WebView-compatible copy path below.
  }
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.select();
  try {
    document.execCommand('copy');
  } finally {
    textarea.remove();
  }
}

export function AiMessageContent({ content }: Props) {
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);
  const blocks = parseAiMarkdown(content);

  return (
    <div className="ai-markdown">
      {blocks.map((block, index) => {
        if (block.kind === 'heading') {
          const heading = renderInline(block.content ?? '');
          if (block.level === 1) return <h1 key={index}>{heading}</h1>;
          if (block.level === 3) return <h3 key={index}>{heading}</h3>;
          return <h2 key={index}>{heading}</h2>;
        }
        if (block.kind === 'rule') return <hr key={index} />;
        if (block.kind === 'unordered-list' || block.kind === 'ordered-list') {
          const List = block.kind === 'ordered-list' ? 'ol' : 'ul';
          return (
            <List key={index}>
              {(block.items ?? []).map((item, itemIndex) => (
                <li key={itemIndex}>{renderInline(item)}</li>
              ))}
            </List>
          );
        }
        if (block.kind === 'code') {
          return (
            <div className="ai-markdown-code" key={index}>
              <button
                type="button"
                aria-label="复制代码"
                title={copiedIndex === index ? '已复制' : '复制'}
                onClick={() => {
                  void copyCode(block.content ?? '').then(() => {
                    setCopiedIndex(index);
                    window.setTimeout(() => setCopiedIndex(null), 1_500);
                  });
                }}
              >
                {copiedIndex === index ? '✓' : '⧉'}
              </button>
              <pre>
                <code data-language={block.language || undefined}>{block.content}</code>
              </pre>
            </div>
          );
        }
        return <p key={index}>{renderInline(block.content ?? '')}</p>;
      })}
    </div>
  );
}
