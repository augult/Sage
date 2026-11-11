// The session timeline, as a conversation: what was asked, what the agent said, and between the
// two one grey bar for everything it did. The bar is the one toggle of a stretch of work: every
// tool call the agent ran, with its thinking and the harness's notes between them, across as many
// turns as went by without words. It unfolds into a row per call, and each row into what the call
// was: the command and what it printed, the file as it was read, the edit as a before and after.
// Nothing is unfolded until the user asks, and what they unfolded stays so (`ctx.expanded`).
//
// Each update reads the items into blocks (`gather`) and patches what changed: a block by its key
// and signature, a bar's rows one by one, and only while the bar is open.
//
// ctx: { expanded: Map, canAnswer(): boolean, answer(approvalId, allow): Promise, openOutput(handle) }

import { stepVerb, summarize } from '../activity.js';
import { copyButton, h, icon, spinner } from '../dom.js';
import { duration, firstLine, plural } from '../format.js';
import { known, outcomeLook } from './common.js';
import { prose } from './prose.js';
import { changesCard, chevron, fold, hasDetail, stepDetail, stepIcon, stepStat, verbatim } from './tools.js';

// How a step ended, when it was not simply fine.
const STATE_WORDS = { surprise: 'surprise', error: 'failed', skipped: 'skipped', running: 'in background', denied: 'denied', unfinished: 'did not finish' };

const RESULT_WORDS = { done: 'Done', yielded: 'Handed back', stopped: 'Stopped', budget: 'Out of budget', error: 'Failed' };

// Characters of a note that still read as one slim line.
const LONG_NOTE = 140;

export function createTimeline(container, ctx) {
  const views = [];
  return {
    update(items) {
      const blocks = gather(items, ctx);
      blocks.forEach((block, i) => {
        if (views[i]?.key === block.key) {
          views[i].update(block);
          return;
        }
        // Something else belongs in this place: drop the stale tail and continue from here.
        for (const stale of views.splice(i)) stale.el.remove();
        views[i] = block.kind === 'work' ? workView(block, ctx) : plainView(block, ctx);
        container.append(views[i].el);
      });
      for (const stale of views.splice(blocks.length)) stale.el.remove();
    },
  };
}

const awaitsAnswer = (row, ctx) => row.state === 'pending' && ctx.canAnswer();

// A step with no end in a turn that is over was cut short (stop, or the app closing).
const statusOf = (step, turn) => (step.status === 'active' && !turn.live ? 'unfinished' : step.status);

/**
 * Reads the timeline's items into what the page shows, in order: `user`, `text` (what the agent
 * said), `work` (a stretch of tool calls, thinking and notes, under one bar), `approval` (a request
 * that is waiting, which cannot wait inside a fold) and `result`. Words end a stretch of work;
 * a turn without words continues the one before it.
 */
function gather(items, ctx) {
  const blocks = [];
  let work = null;
  // What the run in progress has edited, for the card under its result.
  let edits = [];
  const into = (first) => {
    if (!work) blocks.push((work = { kind: 'work', key: `work-${first.key}`, rows: [], turns: [], live: null }));
    return work;
  };

  function turn(item) {
    const content = item.content.trim();
    const reasoning = item.reasoning.trim();
    const streaming = item.live && !item.reply;
    const acted = item.steps.size > 0;
    // What the turn opens a bar under, when it is the first thing in one.
    const mark = { key: `${item.key}-mark` };
    let marked = false;
    const put = (rows) => {
      if (!rows.length && marked) return;
      const block = into(marked ? rows[0] : mark);
      // The bar the turn starts in counts the time its reply took.
      if (!marked) block.turns.push(item);
      marked = true;
      block.rows.push(...rows);
    };
    const rows = [];
    const waiting = [];
    for (const row of item.rows) {
      if (row.kind === 'approval' && awaitsAnswer(row, ctx)) waiting.push(row);
      else rows.push(row.kind === 'step' ? { kind: 'step', key: row.key, step: row, turn: item } : row);
      if (row.kind === 'step' && row.change && (row.status === 'ok' || row.status === 'surprise')) edits.push(row.change);
    }
    if (reasoning) put([{ kind: 'thought', key: `${item.key}-thought`, turn: item, text: reasoning, streaming: streaming && !content }]);
    // Words before tool calls introduce them; with no calls, all else came before the words.
    if (rows.length && !(content && acted)) put(rows);
    if (content) {
      work = null;
      blocks.push({ kind: 'text', key: `${item.key}-text`, sig: `${item.rev}:${item.streamRev}`, text: content, streaming });
      if (acted) put(rows);
    } else if (item.live) {
      // A turn that has shown nothing yet still says that it is at work.
      put([]);
    }
    if (item.live && work) work.live = item;
    // An error the provider answered with, while nothing has come of the turn: said in the open,
    // since a bar that reads "Retrying" can go on reading so for minutes.
    const failed = item.rows.findLast((row) => row.retry);
    if (failed && !item.reply) blocks.push({ kind: 'retry', key: `${item.key}-retry`, sig: `${failed.retry.attempt}:${item.live}`, retry: failed.retry, live: item.live });
    for (const row of waiting) blocks.push({ kind: 'approval', key: row.key, sig: row.state, item: row });
  }

  for (const item of items) {
    if (item.kind === 'turn') {
      turn(item);
    } else if (item.kind === 'user') {
      work = null;
      edits = [];
      blocks.push({ kind: 'user', key: item.key, sig: item.rev, item });
    } else if (item.kind === 'result') {
      work = null;
      blocks.push({ kind: 'result', key: item.key, sig: `${item.rev}:${edits.length}`, item, edits });
    } else if (item.kind === 'approval' && awaitsAnswer(item, ctx)) {
      blocks.push({ kind: 'approval', key: item.key, sig: item.state, item });
    } else {
      // The environment snapshot, a note, a lesson, an answered request: rows like any other.
      into(item).rows.push(item);
    }
  }
  return blocks;
}

/** Stands in for a block or row that could not be drawn, so that one bad entry costs only itself. */
const undrawable = (what, problem) => h('div.undrawable', `Cannot display ${what}: ${problem}`);

/** A block that is drawn whole and drawn again when its signature changes. */
function plainView(block, ctx) {
  const draw = (now) => {
    try {
      return PLAIN[now.kind](now, ctx);
    } catch (problem) {
      return undrawable(now.kind, problem);
    }
  };
  const view = {
    key: block.key,
    sig: block.sig,
    el: draw(block),
    update(next) {
      if (next.sig === view.sig) return;
      const el = draw(next);
      view.el.replaceWith(el);
      Object.assign(view, { el, sig: next.sig });
    },
  };
  return view;
}

const PLAIN = {
  /** What was asked: the one thing in the conversation that the user wrote, set to their side. */
  user({ item }) {
    return h('section.msg.user', { 'aria-label': item.followUp ? 'Follow-up' : 'Task' }, h('div.user-text', item.text), h('div.msg-actions', copyButton(() => item.text)));
  },

  text({ text, streaming }) {
    return h('section.msg.agent', prose(text, { caret: streaming }), !streaming && h('div.msg-actions', copyButton(() => text)));
  },

  approval: ({ item }, ctx) => approvalCard(item, ctx),

  /** What the provider answered instead of a reply, and that the request is being made again. */
  retry({ retry, live }) {
    return h(
      'section.notice',
      { role: 'alert' },
      h('div.notice-title', icon('bang', 14), 'Provider error', h('span.notice-state', live ? `retrying · attempt ${retry.attempt}` : plural(retry.attempt, 'attempt'))),
      h('div.notice-text', retry.reason),
    );
  },

  /** How the run ended: its summary as the answer it is, a line for the outcome, and what it edited. */
  result({ item, edits }, ctx) {
    const look = outcomeLook(item.status, item.verified);
    const summary = item.summary.trim();
    // The evidence says in its own words whether the result was checked; without it, one word does.
    const checked = item.evidence || (!item.verified ? 'unverified' : item.status === 'done' ? 'verified' : '');
    return h(
      'section.result',
      summary && !item.repeatsReply && h('div.msg.agent', prose(summary), h('div.msg-actions', copyButton(() => summary))),
      // A stop is the user's own doing: the header says it, and that is enough.
      item.status !== 'stopped' && h('div.result-line', icon(look.icon, 14), h('span.result-status', known(RESULT_WORDS, item.status) ?? look.label), checked && h('span', `· ${checked}`)),
      edits.length > 0 && changesCard(edits, ctx, `${item.key}-edits`),
    );
  },
};

/** What a bar says: what is happening while its last turn runs, what was done once it has. */
function headline(block) {
  const turn = block.live;
  if (turn) {
    const running = [...turn.steps.values()].findLast((step) => step.status === 'active');
    if (running) return { busy: true, since: running.startedAt, parts: [`${stepVerb(running.tool, true)} `, { name: running.summary }], notes: [], meta: '' };
    if (!turn.reply) return { busy: true, since: turn.startedAt, parts: [turn.rows.some((row) => row.retry) ? 'Retrying' : 'Thinking'], notes: [], meta: '' };
  }
  const count = (kind) => block.rows.filter((row) => row.kind === kind).length;
  const steps = block.rows.filter((row) => row.kind === 'step').map((row) => ({ tool: row.step.tool, summary: row.step.summary, status: statusOf(row.step, row.turn), ms: row.step.ms }));
  const { parts, notes } = summarize(steps);
  const spent = block.turns.reduce((ms, each) => ms + (each.reply?.ms ?? 0), 0) + steps.reduce((ms, step) => ms + step.ms, 0);
  let meta = [plural(steps.length, 'step'), spent >= 100 && duration(spent)].filter(Boolean).join(' · ');
  if (!steps.length) {
    const asides = [count('env') > 0 && 'Environment snapshot', count('chip') + count('approval') > 0 && plural(count('chip') + count('approval'), 'note')].filter(Boolean);
    meta = '';
    if (count('thought')) parts.push(spent >= 100 ? `Thought for ${duration(spent)}` : 'Thought');
    else if (asides.length) parts.push(asides.join(', '));
    else parts.push(turn ? 'Working' : 'No reply');
  } else if (!parts.length) {
    parts.push('Finished');
  }
  return { busy: Boolean(turn), since: null, parts, notes, meta };
}

/** A stretch of work: the bar, and under it the rows, drawn when the bar is first opened. */
function workView(block, ctx) {
  const busy = spinner();
  const label = h('span.work-label');
  const notes = h('span.work-notes');
  const meta = h('span.work-meta');
  const toggle = h('button.work-toggle', { type: 'button' }, chevron(), busy, label, notes, meta);
  const list = h('div.work-rows');
  const el = h('section.work', h('div.work-bar', toggle));
  let rows = [];
  let said = '';
  let open = false;

  function head() {
    const line = headline(block);
    const now = JSON.stringify(line);
    if (now === said) return;
    said = now;
    busy.hidden = !line.busy;
    el.classList.toggle('is-live', line.busy);
    label.replaceChildren(...line.parts.map((part) => (typeof part === 'string' ? part : h('b', part.name))));
    // Failures in red, here too: the rows that show them start shut.
    notes.replaceChildren(...line.notes.flatMap((note, i) => [i ? ', ' : null, h(`span${note.status === 'error' ? '.is-failed' : ''}`, note.text)].filter(Boolean)));
    notes.hidden = !line.notes.length;
    meta.replaceChildren(line.since != null ? h('span', { dataset: { since: line.since } }) : line.meta);
    meta.hidden = line.since == null && !line.meta;
  }

  function drawRow(row) {
    try {
      return ROWS[row.kind](row, ctx);
    } catch (problem) {
      return undrawable(row.kind, problem);
    }
  }

  function syncRows() {
    if (!open) return;
    block.rows.forEach((row, i) => {
      const sig = rowSig(row);
      const have = rows[i];
      if (have?.key === row.key && have.sig === sig) return;
      const fresh = { key: row.key, sig, el: drawRow(row) };
      if (have?.key === row.key) {
        have.el.replaceWith(fresh.el);
      } else {
        for (const stale of rows.splice(i)) stale.el.remove();
        list.append(fresh.el);
      }
      rows[i] = fresh;
    });
    for (const stale of rows.splice(block.rows.length)) stale.el.remove();
  }

  const body = fold(
    ctx,
    block.key,
    toggle,
    () => list,
    (now) => {
      open = now;
      el.classList.toggle('is-open', now);
      syncRows();
    },
  );
  el.append(body);
  head();

  return {
    key: block.key,
    el,
    update(next) {
      block = next;
      head();
      syncRows();
    },
  };
}

/** Everything a row shows that can change, as one string: the row is drawn again when it differs. */
function rowSig(row) {
  switch (row.kind) {
    case 'step':
      return `${row.step.status}|${row.step.ms}|${row.step.observation.length}|${row.turn.live}`;
    case 'thought':
      return `${row.text.length}|${row.streaming}`;
    case 'approval':
      return row.state;
    default:
      return '';
  }
}

/** One row. With `detail` it is a toggle that unfolds what `detail()` builds; without, a plain line. */
function row(ctx, key, classes, cells, detail, attributes = {}) {
  const spec = `div.row${classes}`;
  if (!detail) return h(spec, attributes, h('div.row-line', cells, h('span.chev-slot')));
  const toggle = h('button.row-line', { type: 'button' }, cells, chevron());
  return h(spec, attributes, toggle, fold(ctx, key, toggle, detail));
}

/**
 * A quiet row for what happened beside the steps: a watch change, a note, a lesson, a retry.
 * One that is long or runs over several lines shows its first line and unfolds the rest.
 */
function noteRow(note, ctx) {
  const head = firstLine(note.text);
  const label = note.label.charAt(0).toUpperCase() + note.label.slice(1);
  const long = head.length < note.text.length || head.length > LONG_NOTE;
  const cells = [h('span.row-icon', icon(note.icon ?? 'note', 14)), h('span.row-verb', label), h('span.row-preview', head), note.badge && h('span.row-state.is-quiet', note.badge)];
  return row(ctx, note.key, `.is-note${note.tone === 'danger' ? '.tone-danger' : ''}`, cells, long ? () => h('div.row-detail', verbatim(note.text, ctx)) : null);
}

const ROWS = {
  thought(item, ctx) {
    const cells = [h('span.row-icon', icon('spark', 14)), h('span.row-verb', item.streaming ? 'Thinking' : 'Thought'), h('span.row-preview', firstLine(item.text))];
    return row(ctx, item.key, '.is-thought', cells, () => h('div.row-detail', prose(item.text, { caret: item.streaming })));
  },

  step({ step, turn }, ctx) {
    const status = statusOf(step, turn);
    const active = status === 'active';
    const held = status === 'ok' && step.explicit;
    const state = held ? 'held' : active || status === 'ok' ? null : known(STATE_WORDS, status) ?? status;
    const cells = [
      h('span.row-icon', icon(stepIcon(step.tool), 14)),
      h('span.row-verb', stepVerb(step.tool, active)),
      h('span.row-what', step.summary),
      state && h(`span.row-state${held ? '.is-quiet' : ''}`, state),
      stepStat(step),
      active ? h('span.row-took', spinner(), h('span', { dataset: { since: step.startedAt } })) : step.ms >= 100 && h('span.row-took', duration(step.ms)),
    ];
    const detail = hasDetail(step) ? () => stepDetail(step, status, ctx) : null;
    return row(ctx, step.key, `.is-${status.replace(/[^a-z0-9-]/gi, '')}`, cells, detail, { dataset: { step: `${turn.turn}.${step.idx + 1}` } });
  },

  chip: noteRow,

  /** The snapshot of the workspace that went out with the task. */
  env(item, ctx) {
    const cells = [h('span.row-icon', icon('layers', 14)), h('span.row-verb', 'Environment'), h('span.row-preview', firstLine(item.text))];
    return row(ctx, item.key, '.is-note', cells, () => h('div.row-detail', verbatim(item.text, ctx)));
  },

  /** A request that was answered, or can no longer be. It sits under the step it was about, so the reason is all it needs to say. */
  approval(item, ctx) {
    const verdict = known({ allowed: 'allowed', denied: 'denied' }, item.state) ?? 'never answered';
    return noteRow({ key: item.key, label: 'approval', icon: 'bang', badge: verdict, text: item.reason || `${item.tool} ${item.summary}` }, ctx);
  },
};

/** A request that is waiting for its answer: the one thing in a run that cannot wait inside a fold. */
function approvalCard(item, ctx) {
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const answer = async (allow) => {
    for (const button of buttons) button.disabled = true;
    try {
      await ctx.answer(item.id, allow);
    } catch (problem) {
      error.textContent = String(problem);
      error.hidden = false;
      for (const button of buttons) button.disabled = false;
    }
  };
  const buttons = [
    h('button.btn.approval-deny', { type: 'button', onclick: () => answer(false) }, 'Deny'),
    h('button.btn.primary.approval-allow', { type: 'button', onclick: () => answer(true) }, 'Allow'),
  ];
  return h(
    'div.approval',
    { dataset: { approval: item.id } },
    h('div.approval-title', icon('bang'), 'Approval needed'),
    h('div.approval-what', h('b', item.tool), item.summary),
    item.reason && h('div.approval-why', item.reason),
    h('div.approval-actions', error, buttons),
  );
}
