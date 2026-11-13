// Markdown as models write it, read into a small tree. Text in, plain objects out: the view
// (views/prose.js) builds elements from them, so nothing a model wrote is ever parsed as HTML.
//
// Blocks: paragraph, heading, code (fenced), list (nested), quote, rule, table.
// Inline: code, strong, em, strike, link. A newline inside a paragraph stays a line break, the way
// the plain text read before. Text cut off mid-stream still reads: a fence with no end runs to the
// end of the text, and a marker with no partner is ordinary text.

const FENCE = /^( {0,3})(`{3,}|~{3,})\s*([^`]*)$/;
// A closing run of `#` counts only after a space: "C#" keeps its sign.
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const QUOTE = /^ {0,3}> ?/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const DIVIDER = /^\s*\|?\s*:?-+:?\s*(?:\|\s*:?-+:?\s*)*\|?\s*$/;

const lead = (line) => line.length - line.trimStart().length;
const cells = (line) => line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((cell) => cell.trim());

/** Reads `source` into blocks: `{ type, … }`, with inline text as `children`. */
export function parse(source) {
  return blocks(String(source ?? '').replace(/\r\n?/g, '\n').split('\n'));
}

function blocks(lines) {
  const out = [];
  let paragraph = [];
  const flush = () => {
    if (paragraph.length) out.push({ type: 'paragraph', children: inline(paragraph.join('\n')) });
    paragraph = [];
  };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      const [, indent, marks, info] = fence;
      const closes = new RegExp(`^ {0,3}${marks[0]}{${marks.length},}\\s*$`);
      const body = [];
      for (i++; i < lines.length && !closes.test(lines[i]); i++) body.push(lines[i].slice(Math.min(lead(lines[i]), indent.length)));
      i++;
      out.push({ type: 'code', lang: info.trim().split(/\s+/)[0] ?? '', text: body.join('\n') });
    } else if (!line.trim()) {
      flush();
      i++;
    } else if (HEADING.test(line)) {
      flush();
      const [, marks, text] = HEADING.exec(line);
      out.push({ type: 'heading', level: marks.length, children: inline(text) });
      i++;
    } else if (RULE.test(line)) {
      flush();
      out.push({ type: 'rule' });
      i++;
    } else if (QUOTE.test(line)) {
      flush();
      const inner = [];
      for (; i < lines.length && QUOTE.test(lines[i]); i++) inner.push(lines[i].replace(QUOTE, ''));
      out.push({ type: 'quote', children: blocks(inner) });
    } else if (ITEM.test(line)) {
      flush();
      i = list(lines, i, out);
    } else if (line.includes('|') && i + 1 < lines.length && DIVIDER.test(lines[i + 1]) && lines[i + 1].includes('-') && cells(line).length === cells(lines[i + 1]).length) {
      flush();
      i = table(lines, i, out);
    } else {
      paragraph.push(line.trim());
      i++;
    }
  }
  flush();
  return out;
}

/** Reads the list that starts at `lines[start]` into `out`. Returns the line after it. */
function list(lines, start, out) {
  const [, indent, marker] = ITEM.exec(lines[start]);
  const ordered = /\d/.test(marker);
  const items = [];
  let i = start;
  for (;;) {
    // Blank lines between two items do not end the list.
    let next = i;
    while (next < lines.length && !lines[next].trim()) next++;
    const item = next < lines.length ? ITEM.exec(lines[next]) : null;
    if (!item || item[1].length !== indent.length || /\d/.test(item[2]) !== ordered) break;
    const width = item[1].length + item[2].length + 1;
    const body = [item[3]];
    for (i = next + 1; i < lines.length; i++) {
      const line = lines[i];
      if (line.trim()) {
        // Indented under the item: more of it, or a list inside it. Anything else ends the item.
        if (lead(line) <= indent.length) break;
        body.push(line.slice(Math.min(lead(line), width)));
      } else {
        const after = lines.slice(i + 1).find((later) => later.trim());
        if (after === undefined || lead(after) <= indent.length) break;
        body.push('');
      }
    }
    items.push(blocks(body));
  }
  out.push({ type: 'list', ordered, start: ordered ? Number.parseInt(marker, 10) : null, items });
  return i;
}

/** Reads the table whose header is `lines[start]` into `out`. Returns the line after it. */
function table(lines, start, out) {
  const align = cells(lines[start + 1]).map((cell) => (cell.endsWith(':') ? (cell.startsWith(':') ? 'center' : 'right') : null));
  const rows = [];
  let i = start + 2;
  for (; i < lines.length && lines[i].trim() && lines[i].includes('|'); i++) rows.push(cells(lines[i]).map(inline));
  out.push({ type: 'table', align, head: cells(lines[start]).map(inline), rows });
  return i;
}

const LETTER = /[\p{L}\p{N}]/u;
const BARE_URL = /^https?:\/\/[^\s<>()[\]`]*[^\s<>()[\]`.,;:!?'"*_]/;

/**
 * Where the emphasis opened at `start` with `mark` closes, or -1 when it is not emphasis: a mark
 * must hug its text on both sides, and an underscore inside a word is part of the word.
 */
function closing(text, start, mark) {
  const from = start + mark.length;
  if (from >= text.length || /\s/.test(text[from])) return -1;
  const inWord = mark[0] === '_';
  if (inWord && start > 0 && LETTER.test(text[start - 1])) return -1;
  for (let end = text.indexOf(mark, from + 1); end !== -1; end = text.indexOf(mark, end + mark.length)) {
    // One star of a pair is the pair's.
    if (mark.length === 1 && (text[end + 1] === mark || text[end - 1] === mark)) continue;
    if (/\s/.test(text[end - 1])) continue;
    const after = text[end + mark.length];
    if (inWord && after !== undefined && LETTER.test(after)) continue;
    return end;
  }
  return -1;
}

/** Reads a run of text into inline nodes: `{ type: 'text' | 'code', text }` or `{ type, children }`. */
export function inline(text) {
  const out = [];
  let plain = '';
  const push = (node) => {
    if (plain) out.push({ type: 'text', text: plain });
    plain = '';
    if (node) out.push(node);
  };
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '\\' && i + 1 < text.length && /[\\`*_{}[\]()#+\-.!~|>]/.test(text[i + 1])) {
      plain += text[i + 1];
      i += 2;
      continue;
    }
    if (ch === '`') {
      const marks = /^`+/.exec(text.slice(i))[0];
      const end = text.indexOf(marks, i + marks.length);
      if (end !== -1) {
        push({ type: 'code', text: text.slice(i + marks.length, end).replace(/^ (.+) $/s, '$1') });
        i = end + marks.length;
        continue;
      }
    }
    const pair = ch === '*' || ch === '_' || ch === '~' ? text.slice(i, i + 2) : '';
    const mark = pair === '**' || pair === '__' || pair === '~~' ? pair : ch === '*' || ch === '_' ? ch : '';
    if (mark) {
      const end = closing(text, i, mark);
      if (end !== -1) {
        push({ type: mark === '~~' ? 'strike' : mark.length === 2 ? 'strong' : 'em', children: inline(text.slice(i + mark.length, end)) });
        i = end + mark.length;
      } else {
        plain += mark;
        i += mark.length;
      }
      continue;
    }
    if (ch === '[') {
      const middle = text.indexOf('](', i);
      const end = middle === -1 ? -1 : text.indexOf(')', middle + 2);
      if (end !== -1 && !text.slice(i + 1, middle).includes('[')) {
        push({ type: 'link', href: text.slice(middle + 2, end).trim().split(/\s+/)[0] ?? '', children: inline(text.slice(i + 1, middle)) });
        i = end + 1;
        continue;
      }
    }
    if (ch === 'h' || ch === '<') {
      const open = ch === '<' ? 1 : 0;
      const url = (i === 0 || open || /[\s(]/.test(text[i - 1])) && BARE_URL.exec(text.slice(i + open))?.[0];
      if (url && (!open || text[i + 1 + url.length] === '>')) {
        push({ type: 'link', href: url, children: [{ type: 'text', text: url }] });
        i += url.length + 2 * open;
        continue;
      }
    }
    plain += ch;
    i++;
  }
  push(null);
  return out;
}
