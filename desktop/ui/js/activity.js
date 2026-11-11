// What a turn did, as the one line the timeline shows for it: "Read 2 files, ran a command".
// Pure text (no DOM), so it can be checked without a window.

import { duration, plural } from './format.js';

// Per tool: the word that leads its row in the tree, the same while it runs, and how one call and
// several calls read in a turn's line. `file` marks the tools whose summary starts with a path.
const TOOLS = {
  shell: { did: 'Ran', doing: 'Running', one: 'ran a command', many: (n) => `ran ${n} commands` },
  read: { did: 'Read', doing: 'Reading', one: 'read a file', many: (n) => `read ${n} files`, file: 'read' },
  search: { did: 'Searched', doing: 'Searching', one: 'searched', many: (n) => `searched ${n} times` },
  edit: { did: 'Edited', doing: 'Editing', one: 'edited a file', many: (n) => `edited ${n} files`, file: 'edited' },
  write: { did: 'Wrote', doing: 'Writing', one: 'wrote a file', many: (n) => `wrote ${n} files`, file: 'wrote' },
  watch: { did: 'Watch', doing: 'Watch', one: 'set a watch', many: (n) => `set ${n} watches` },
  job: { did: 'Job', doing: 'Job', one: 'checked a background job', many: (n) => `checked background jobs ${n} times` },
  finish: { did: 'Finish', doing: 'Finishing' },
};

const words = (tool) => (Object.hasOwn(TOOLS, tool) ? TOOLS[tool] : { did: tool || 'Step', doing: tool || 'Step', one: `used ${tool}`, many: (n) => `used ${tool} ${n} times` });

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
 * The line for a turn: `{ busy, since, parts, notes }`, or null for a turn with nothing to unfold.
 * `parts` are strings and `{ name }`s (a file or a command, which the view sets apart); `notes`
 * say what went otherwise than planned, since the tree that shows it starts folded. While the turn
 * runs the line says what is happening now, and `since` is when that began.
 */
export function activity(turn) {
  const steps = turn.rows.filter((row) => row.kind === 'step');
  if (turn.live) {
    const running = steps.findLast((step) => step.status === 'active');
    if (running) return { busy: true, since: running.startedAt, parts: [`${stepVerb(running.tool, true)} `, { name: running.summary }], notes: [] };
    if (!turn.reply) return { busy: true, since: turn.startedAt, parts: ['Thinking'], notes: [] };
  }

  const groups = new Map();
  for (const step of steps) {
    // A finish that went through is the answer below, not something the turn did on the way.
    if (step.tool === 'finish') continue;
    if (!groups.has(step.tool)) groups.set(step.tool, []);
    groups.get(step.tool).push(step);
  }
  const parts = [];
  for (const [tool, calls] of groups) {
    const word = words(tool);
    const names = word.file ? [...new Set(calls.map((step) => fileName(step.summary)))] : [];
    if (parts.length) parts.push(', ');
    if (names.length === 1 && names[0]) parts.push(`${word.file} `, { name: names[0] });
    else if (word.file) parts.push(names.length > 1 ? word.many(names.length) : word.one);
    else parts.push(calls.length > 1 ? word.many(calls.length) : word.one);
  }
  if (!parts.length) {
    if (turn.reasoning.trim()) parts.push(turn.reply ? `thought for ${duration(turn.reply.ms)}` : 'thought');
    else if (steps.length) parts.push('finished');
    else if (turn.rows.length) parts.push(turn.reply ? `replied in ${duration(turn.reply.ms)}` : 'no reply');
    else if (turn.live) parts.push('working');
    else return null;
  }
  parts[0] = parts[0][0].toUpperCase() + parts[0].slice(1);

  const count = new Map();
  for (const step of steps) {
    // A step with no end in a turn that is over was cut short (stop, or the app closing).
    const status = step.status === 'active' ? 'unfinished' : step.status;
    count.set(status, (count.get(status) ?? 0) + 1);
  }
  const notes = TROUBLE.filter(([status]) => count.has(status)).map(([status, say]) => say(count.get(status)));
  return { busy: turn.live, since: null, parts, notes };
}
