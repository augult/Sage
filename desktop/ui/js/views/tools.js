// What a row of work unfolds into, the same for every tool: the input the call was made with, then
// its output. The output is the engine's own text, or where it has a shape, that shape: an edit as
// a before and after, a web search as its sources.
//
// ctx: { expanded: Map, openOutput(handle) }

import { toolKind } from '../activity.js';
import { copyButton, h, icon } from '../dom.js';
import { bytes, firstLine, plural } from '../format.js';
import * as prefs from '../prefs.js';

const ICONS = { shell: 'terminal', read: 'file', search: 'search', edit: 'pencil', write: 'filePlus', watch: 'eye', job: 'clock', finish: 'flag', web_search: 'globe', web_fetch: 'globe' };

/** The icon that leads a step's row. */
export const stepIcon = (tool) => ICONS[toolKind(tool)] ?? 'dot';

/** The mark of everything that unfolds. It turns as its toggle opens (styles/session.css). */
export function chevron() {
  const mark = icon('chevron', 14);
  mark.classList.add('chev');
  return mark;
}

/**
 * Makes `toggle` open and close a fold and returns the wrapper to put in the page. `content()`
 * builds what unfolds, the first time it is needed. The fold slides rather than appears, so what
 * is in it stays in the page while shut: inert, so that nothing in it takes focus. Everything
 * starts shut; the user's choice is remembered under `key`. `onToggle(open)` runs on each change.
 */
export function fold(ctx, key, toggle, content, onToggle) {
  const inner = h('div.fold-inner');
  const wrap = h('div.fold', inner);
  let built = false;
  const show = (open) => {
    if (open && !built) {
      built = true;
      inner.append(content());
    }
    toggle.setAttribute('aria-expanded', String(open));
    wrap.classList.toggle('is-open', open);
    wrap.inert = !open;
    onToggle?.(open);
  };
  toggle.addEventListener('click', () => {
    const open = toggle.getAttribute('aria-expanded') !== 'true';
    ctx.expanded.set(key, open);
    show(open);
  });
  show(ctx.expanded.get(key) === true);
  return wrap;
}

/** Text with its `out:N` handles as buttons that open the full output. */
function linked(text, ctx) {
  const parts = [];
  let from = 0;
  for (const match of text.matchAll(/\bout:\d+\b/g)) {
    parts.push(text.slice(from, match.index), h('button.out-link', { type: 'button', onclick: () => ctx.openOutput(match[0]) }, match[0]));
    from = match.index + match[0].length;
  }
  parts.push(text.slice(from));
  return parts;
}

// A line the engine printed with its anchor: optional mark of a match, line number, two-letter hash.
const ANCHORED = /^(\s*(?:>\s)?\s*)(\d+)([a-z]{2})\|(.*)$/;

/**
 * Engine text, verbatim. A leading [STATUS] is set in bold, `out:N` handles open the full output,
 * and the anchors of file lines step back into a gutter.
 */
export function verbatim(text, ctx) {
  const pre = h('pre.block');
  const status = /^\[[A-Za-z]+\]/.exec(text)?.[0] ?? '';
  if (status) pre.append(h('span.obs-tag', status));
  text
    .slice(status.length)
    .split('\n')
    .forEach((line, i) => {
      if (i) pre.append('\n');
      const anchored = ANCHORED.exec(line);
      if (anchored) pre.append(h(`span.gut${anchored[1].includes('>') ? '.is-hit' : ''}`, anchored[1], anchored[2], h('span.gut-hash', anchored[3]), ' '), ...linked(anchored[4], ctx));
      else pre.append(...linked(line, ctx));
    });
  return pre;
}

/** `+2 −1`, the way every count of changed lines is written. */
export function lineStat(added, removed) {
  return h('span.stat-lines', { title: `${plural(added, 'line')} added, ${removed} removed` }, h('span.stat-add', `+${added}`), h('span.stat-del', `−${removed}`));
}

/** What a step's row says beside its words: the lines an edit added and removed. */
export const stepStat = (step) => step.change && lineStat(step.change.added, step.change.removed);

/**
 * The change an edit by exact text or a write describes on its own, for a log from before the
 * engine sent the change itself: the old text out, the new text in, with no line numbers.
 */
function changeFromCall(step) {
  const args = step.args;
  const text = (value) => (typeof value === 'string' && value ? value.replace(/\n$/, '').split('\n') : []);
  const kind = toolKind(step.tool);
  if (!args || step.status === 'error' || step.status === 'denied' || step.status === 'skipped') return null;
  const [removed, added] = kind === 'edit' && typeof args.old === 'string' ? [text(args.old), text(args.new)] : kind === 'write' ? [[], text(args.content)] : [[], []];
  if (!removed.length && !added.length) return null;
  const lines = [...removed.map((line) => `-${line}`), ...added.map((line) => `+${line}`)];
  return { path: String(args.path ?? ''), created: false, added: added.length, removed: removed.length, hunks: [{ old_start: 0, new_start: 0, lines: lines.slice(0, 400) }], truncated: lines.length > 400, unnumbered: true };
}

/** Rows of a diff, one under another: old number, new number, sign, text. */
function unifiedRows(change) {
  const rows = [];
  change.hunks.forEach((hunk, at) => {
    if (at) rows.push(h('div.diff-gap', { 'aria-hidden': 'true' }));
    let [before, after] = [hunk.old_start, hunk.new_start];
    for (const line of hunk.lines) {
      const kind = line[0] === '+' ? 'add' : line[0] === '-' ? 'del' : 'ctx';
      rows.push(
        h(
          `div.diff-row.is-${kind}`,
          h('span.diff-n', kind === 'add' || change.unnumbered ? '' : String(before)),
          h('span.diff-n', kind === 'del' || change.unnumbered ? '' : String(after)),
          h('span.diff-sign', kind === 'add' ? '+' : kind === 'del' ? '−' : ''),
          h('span.diff-text', line.slice(1)),
        ),
      );
      if (kind !== 'add') before++;
      if (kind !== 'del') after++;
    }
  });
  return rows;
}

/** Rows of a diff, before on the left and after on the right, a removed line facing what replaced it. */
function splitRows(change) {
  const rows = [];
  const side = (kind, number, text) => [h('span.diff-n', number == null || change.unnumbered ? '' : String(number)), h(`span.diff-text.is-${kind}`, text ?? '')];
  change.hunks.forEach((hunk, at) => {
    if (at) rows.push(h('div.diff-gap', { 'aria-hidden': 'true' }));
    let [before, after] = [hunk.old_start, hunk.new_start];
    let removed = [];
    let added = [];
    const flush = () => {
      for (let i = 0; i < Math.max(removed.length, added.length); i++) {
        const [was, now] = [removed[i], added[i]];
        rows.push(h('div.diff-row', was ? side('del', was[0], was[1]) : side('none'), now ? side('add', now[0], now[1]) : side('none')));
      }
      removed = [];
      added = [];
    };
    for (const line of hunk.lines) {
      const text = line.slice(1);
      if (line[0] === '-') removed.push([before++, text]);
      else if (line[0] === '+') added.push([after++, text]);
      else {
        flush();
        rows.push(h('div.diff-row', side('ctx', before++, text), side('ctx', after++, text)));
      }
    }
    flush();
  });
  return rows;
}

/** One file's change as a before and after, in one column or two. `bare` leaves the file's name out. */
export function diffView(change, { bare = false } = {}) {
  const numbers = change.hunks.map((hunk) => Math.max(hunk.old_start, hunk.new_start) + hunk.lines.length);
  const body = h('div.diff-body', { style: `--digits: ${String(Math.max(0, ...numbers)).length}` });
  const modes = ['unified', 'split'].map((mode) => h('button', { type: 'button', dataset: { mode }, onclick: () => draw(mode, true) }, mode === 'unified' ? 'Unified' : 'Split'));
  const view = h(
    'div.diff',
    h(
      'div.diff-head',
      !bare && h('span.diff-path', change.path),
      change.created && h('span.tag', 'new file'),
      lineStat(change.added, change.removed),
      h('span.diff-modes', { role: 'group', 'aria-label': 'Diff layout' }, modes),
    ),
    body,
    change.truncated && h('div.detail-foot', 'Too long to show whole'),
  );
  function draw(mode, chosen) {
    if (chosen) prefs.set('diff', mode);
    view.classList.toggle('is-split', mode === 'split');
    for (const button of modes) button.setAttribute('aria-pressed', String(button.dataset.mode === mode));
    body.replaceChildren(...(mode === 'split' ? splitRows(change) : unifiedRows(change)));
  }
  draw(prefs.get('diff') === 'split' ? 'split' : 'unified', false);
  return view;
}

const URL_AT = /https?:\/\/[^\s<>()"']+/;
const ITEM_START = /^\s*(?:\d+[.)]|[-*•])\s/;

/**
 * What a web search found: the engine's list when it sends one, else the links in what it printed.
 * There, a result is a run of lines up to a blank one or the next list item; the words before its
 * link (or its first line) are the title, and the rest is the snippet.
 */
function sources(step) {
  const text = (value) => (value == null ? '' : String(value));
  if (step.results) return step.results.map((found) => ({ title: text(found?.title), url: text(found?.url), snippet: text(found?.snippet) })).filter((found) => found.url);
  const entries = [];
  let entry = null;
  for (const line of step.observation.split('\n')) {
    if (!line.trim()) entry = null;
    else if (!entry || ITEM_START.test(line)) entries.push((entry = [line]));
    else entry.push(line);
  }
  const clean = (part) => part.replace(ITEM_START, '').replace(/^[\s)\]:–—-]+|[\s([:–—-]+$/g, '').replace(/[[\]]/g, '').trim();
  return entries.flatMap((lines) => {
    const at = lines.findIndex((line) => URL_AT.test(line));
    if (at === -1) return [];
    const link = URL_AT.exec(lines[at]);
    const before = clean(lines[at].slice(0, link.index));
    const after = clean(lines[at].slice(link.index + link[0].length));
    const rest = lines.filter((_, i) => i !== at).map(clean).filter(Boolean);
    const title = before || rest.shift() || '';
    return [{ title, url: link[0].replace(/[.,;:]+$/, ''), snippet: [after, ...rest].filter(Boolean).join(' ') }];
  });
}

function sourceList(found) {
  return h(
    'ol.sources',
    found.map((source) => {
      let site = source.url;
      let path = '';
      try {
        const url = new URL(source.url);
        site = url.hostname.replace(/^www\./, '');
        path = decodeURIComponent(url.pathname).replace(/\/$/, '');
      } catch {
        // Not a URL this build can take apart: shown as it came.
      }
      return h(
        'li.source',
        h('span.source-site', { 'aria-hidden': 'true' }, site.charAt(0).toUpperCase()),
        h('div.source-body', h('div.source-title', source.title || site), h('div.source-url', { title: source.url }, site, path && h('span', path)), source.snippet && h('p.source-snippet', source.snippet)),
        copyButton(() => source.url, 'Copy link'),
      );
    }),
  );
}

/** The prediction a step made, and whether it held. */
function prediction(step, status) {
  const verdict = status === 'ok' && step.explicit ? 'held' : status === 'surprise' ? 'did not hold' : null;
  return h('div.predict', h('span.predict-key', 'Predicted'), h('span.predict-what', step.expect), verdict && h('span.predict-verdict', verdict));
}

/** The arguments a step was called with, when the reply that made the call is known. */
const hasInput = (step) => Boolean(step.args) && Object.keys(step.args).length > 0;

/** A step has something to unfold. */
export function hasDetail(step) {
  return Boolean(step.observation || step.expect != null || step.change || hasInput(step));
}

/** A part of a detail under its name. `action` sits at the end of the name's line. */
const part = (name, body, action) => h('div.part', h('div.part-head', h('span.part-name', name), action), body);

/**
 * What unfolds under a step, the same for every tool: the input it was called with, as the model
 * wrote it, then the output. Where the output has a shape (an edit is a before and after, a search
 * is its sources) it is set that way, with the engine's own text folded under it.
 */
export function stepDetail(step, status, ctx) {
  const kind = toolKind(step.tool);
  const skipped = status === 'skipped';
  const observation = skipped ? step.observation.replace(/^\[skipped\]\s*/, '') : step.observation;
  // Violations the observation's first line does not already state.
  const unstated = step.violations.filter((violation) => !firstLine(step.observation).includes(violation));
  const change = kind === 'edit' || kind === 'write' ? step.change ?? changeFromCall(step) : null;
  const found = kind === 'web_search' && !skipped ? sources(step) : [];

  const parts = [];
  if (hasInput(step)) {
    const input = JSON.stringify(step.args, null, 2);
    parts.push(part('Input', h('pre.block.is-input', input), copyButton(() => input, 'Copy input')));
  }
  if (step.expect != null) parts.push(prediction(step, status));
  parts.push(...unstated.map((violation) => h('div.detail-line', violation)));

  const shaped = change ? diffView(change) : found.length ? sourceList(found) : null;
  if (shaped) {
    parts.push(part('Output', shaped));
    if (observation) {
      const toggle = h('button.sub-toggle', { type: 'button' }, chevron(), 'Raw output');
      parts.push(h('div.sub', toggle, fold(ctx, `${step.key}-observation`, toggle, () => verbatim(observation, ctx))));
    }
  } else if (observation) {
    parts.push(part('Output', skipped ? h('div.detail-line', observation) : verbatim(observation, ctx)));
  }
  if (step.hidden > 0) parts.push(h('div.detail-foot', `${bytes(step.hidden)} kept out of context`));
  return h('div.row-detail', parts);
}

/** The files a run edited, each with its lines added and removed, unfolding into the edits themselves. */
export function changesCard(changes, ctx, key) {
  const files = new Map();
  for (const change of changes) {
    const file = files.get(change.path) ?? { path: change.path, created: false, added: 0, removed: 0, parts: [] };
    file.created ||= change.created;
    file.added += change.added;
    file.removed += change.removed;
    file.parts.push(change);
    files.set(change.path, file);
  }
  const all = [...files.values()];
  const sum = (field) => all.reduce((total, file) => total + file[field], 0);
  return h(
    'div.changes',
    h('div.changes-head', h('span.changes-title', `Edited ${plural(all.length, 'file')}`), lineStat(sum('added'), sum('removed'))),
    all.map((file) => {
      const toggle = h(
        'button.row-line',
        { type: 'button' },
        h('span.row-icon', icon(file.created ? 'filePlus' : 'pencil', 14)),
        h('span.row-what.is-path', file.path),
        file.created && h('span.row-state.is-quiet', 'new'),
        lineStat(file.added, file.removed),
        chevron(),
      );
      const edits = () => h('div.row-detail', file.parts.map((part) => diffView(part, { bare: true })));
      return h('div.row', toggle, fold(ctx, `${key}-${file.path}`, toggle, edits));
    }),
  );
}
