// What a stretch of work did, as the one line its bar shows: "Read 2 files, ran a command".
// Pure text (no DOM), so it can be checked without a window.

import { plural } from './format.js';

// Per tool: the word that leads its row, the same while it runs, and how one call and several
// calls read in the line. `file` marks the tools whose summary starts with a path.
const TOOLS = {
  shell: { did: 'Ran', doing: 'Running', one: 'ran a command', many: (n) => `ran ${n} commands` },
  read: { did: 'Read', doing: 'Reading', one: 'read a file', many: (n) => `read ${n} files`, file: 'read' },
  search: { did: 'Searched', doing: 'Searching', one: 'searched', many: (n) => `searched ${n} times` },
  edit: { did: 'Edited', doing: 'Editing', one: 'edited a file', many: (n) => `edited ${n} files`, file: 'edited' },
  write: { did: 'Wrote', doing: 'Writing', one: 'wrote a file', many: (n) => `wrote ${n} files`, file: 'wrote' },
  watch: { did: 'Watch', doing: 'Watch', one: 'set a watch', many: (n) => `set ${n} watches` },
  job: { did: 'Job', doing: 'Job', one: 'checked a background job', many: (n) => `checked background jobs ${n} times` },
  finish: { did: 'Finish', doing: 'Finishing' },
  ask: { did: 'Asked', doing: 'Asking', one: 'asked a question', many: (n) => `asked ${n} questions` },
  // Not tools of this engine yet: how a web search and a page fetch read once one has them.
  web_search: { did: 'Searched the web for', doing: 'Searching the web for', one: 'searched the web', many: (n) => `searched the web ${n} times` },
  web_fetch: { did: 'Fetched', doing: 'Fetching', one: 'fetched a page', many: (n) => `fetched ${n} pages` },
};

const WEB = {
  web: 'web_search',
  websearch: 'web_search',
  search_web: 'web_search',
  fetch: 'web_fetch',
  webfetch: 'web_fetch',
  fetch_url: 'web_fetch',
  open_url: 'web_fetch',
};

/** The kind of tool a step ran, with the names a web search or a page fetch may go by folded into one. */
export function toolKind(tool) {
  const name = String(tool ?? '').toLowerCase().replace(/-/g, '_');
  return Object.hasOwn(WEB, name) ? WEB[name] : name;
}

function words(tool) {
  const kind = toolKind(tool);
  return Object.hasOwn(TOOLS, kind) ? TOOLS[kind] : { did: tool || 'Step', doing: tool || 'Step', one: `used ${tool}`, many: (n) => `used ${tool} ${n} times` };
}

/** The word that leads a step's row: "Ran", or "Running" while it runs. */
export const stepVerb = (tool, active) => words(tool)[active ? 'doing' : 'did'];

/** The file a read, edit or write names, without its folder, line range, symbol or anchor. */
function fileName(summary) {
  const path = summary.split(/\s/, 1)[0].replace(/:[\d,-]+$/, '').replace(/\/+$/, '');
  return path.slice(path.lastIndexOf('/') + 1);
}

// What did not go to plan, by step status, in the order the line lists it.
const TROUBLE = [
  ['surprise', (n) => plural(n, 'surprise')],
  ['error', (n) => `${n} failed`],
  ['denied', (n) => `${n} denied`],
  ['skipped', (n) => `${n} skipped`],
  ['unfinished', (n) => `${n} unfinished`],
];

/**
 * The line for a sequence of steps, `[{ tool, summary, status }]` in the order they ran:
 * `{ parts, notes }`. `parts` are strings and `{ name }`s (a file, which the view sets apart) and
 * are empty when no step counts; `notes` (`{ status, text }`) say what went otherwise than planned,
 * since the rows that show it start folded away. A status of `active` is a step still running: it
 * is not trouble.
 */
export function summarize(steps) {
  const groups = new Map();
  for (const step of steps) {
    // A finish that went through is the answer below, not something done on the way.
    if (step.tool === 'finish') continue;
    const kind = toolKind(step.tool);
    if (!groups.has(kind)) groups.set(kind, []);
    groups.get(kind).push(step);
  }
  const parts = [];
  for (const calls of groups.values()) {
    const word = words(calls[0].tool);
    const names = word.file ? [...new Set(calls.map((step) => fileName(step.summary)))] : [];
    if (parts.length) parts.push(', ');
    if (names.length === 1 && names[0]) parts.push(`${word.file} `, { name: names[0] });
    else if (word.file) parts.push(names.length > 1 ? word.many(names.length) : word.one);
    else parts.push(calls.length > 1 ? word.many(calls.length) : word.one);
  }
  if (parts.length) parts[0] = parts[0][0].toUpperCase() + parts[0].slice(1);

  const count = new Map();
  for (const step of steps) count.set(step.status, (count.get(step.status) ?? 0) + 1);
  const notes = TROUBLE.filter(([status]) => count.has(status)).map(([status, say]) => ({ status, text: say(count.get(status)) }));
  return { parts, notes };
}
