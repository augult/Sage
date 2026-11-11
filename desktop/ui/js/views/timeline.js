// The session timeline, as a conversation: what was asked, what the agent answered, and for each
// turn one quiet line that says what it did. That line unfolds into the tree of the turn's tool
// calls, and each call unfolds into what it predicted and what came back. Nothing is unfolded until
// the user asks, and nothing is tinted: state is said in words.
//
// One element per timeline item, rebuilt only when that item changed; streamed text is patched in
// place. Disclosure state lives in `ctx.expanded`, so a rebuilt item comes back the way the user
// left it.
//
// ctx: { expanded: Map, canAnswer(): boolean, answer(approvalId, allow): Promise, openOutput(handle) }

import { activity, stepVerb } from '../activity.js';
import { h, icon, spinner } from '../dom.js';
import { bytes, duration, firstLine, k, pct, plural } from '../format.js';
import { known, outcomeLook } from './common.js';

// How a step ended, when it was not simply fine.
const STATE_WORDS = { surprise: 'surprise', error: 'failed', skipped: 'skipped', running: 'in background', denied: 'denied', unfinished: 'did not finish' };

const CACHE_WORDS = { first: 'first request', hit: 'cache hit', partial: 'partial cache', miss: 'cache miss', bust: 'cache bust', unknown: 'no usage' };

const RESULT_WORDS = { done: 'Done', yielded: 'Handed back', stopped: 'Stopped', budget: 'Out of budget', error: 'Failed' };

// Characters of a note that still read as one slim line.
const LONG_NOTE = 140;

export function createTimeline(container, ctx) {
  const rendered = [];
  const asides = new Map();

  function build(item) {
    let view;
    try {
      view = item.kind === 'turn' ? turnView(item, ctx) : { el: (ITEMS[item.kind] ?? noteRow)(item, ctx) };
    } catch (problem) {
      view = { el: undrawable(item, problem, ctx) };
    }
    return { ...view, item, rev: item.rev, streamRev: item.streamRev };
  }

  return {
    update(all) {
      const items = gather(all, asides);
      items.forEach((item, i) => {
        const have = rendered[i];
        if (have?.item.key !== item.key) {
          // Something else belongs in this place: drop the stale tail and continue from here.
          for (const stale of rendered.splice(i)) stale.el.remove();
          rendered[i] = build(item);
          container.append(rendered[i].el);
        } else if (have.item !== item || have.rev !== item.rev) {
          // Changed, or the same item from a rebuilt model.
          rendered[i] = build(item);
          have.el.replaceWith(rendered[i].el);
        } else if (have.streamRev !== item.streamRev) {
          have.patch?.();
          have.streamRev = item.streamRev;
        }
      });
      for (const stale of rendered.splice(items.length)) stale.el.remove();
    },
  };
}

/**
 * What arrives outside any turn (the environment snapshot, a note, a lesson) is folded into one
 * line per stretch of it, as a turn's tool calls are. `made` keeps each line's item from one update
 * to the next, so that an unchanged one is not drawn again.
 */
function gather(items, made) {
  const out = [];
  for (const item of items) {
    if (item.kind !== 'env' && item.kind !== 'chip') out.push(item);
    else if (out.at(-1)?.kind === 'aside') out.at(-1).rows.push(item);
    else out.push({ kind: 'aside', key: `${item.key}-aside`, rev: 0, rows: [item] });
  }
  const keys = new Set();
  const kept = out.map((item) => {
    if (item.kind !== 'aside') return item;
    keys.add(item.key);
    const before = made.get(item.key);
    if (before?.rows.length === item.rows.length && before.rows.every((row, i) => row === item.rows[i])) return before;
    made.set(item.key, item);
    return item;
  });
  for (const key of made.keys()) if (!keys.has(key)) made.delete(key);
  return kept;
}

/** Stands in for an item or row that could not be drawn, so that one bad entry costs only itself. */
function undrawable(item, problem, ctx) {
  return noteRow({ key: `${item.key}-undrawable`, label: 'cannot display', text: `${item.kind}: ${problem}` }, ctx);
}

/** The mark of everything that unfolds. It turns as its toggle opens (styles/session.css). */
function chevron() {
  const mark = icon('chevron', 14);
  mark.classList.add('chev');
  return mark;
}

/**
 * Makes `toggle` open and close `body` and returns the wrapper to put in the page. The body slides
 * rather than appears, so it stays in the page while shut: inert, so that nothing in it takes
 * focus. Everything starts shut; the user's choice is remembered under `key`.
 */
function fold(ctx, key, toggle, body) {
  const wrap = h('div.fold', h('div.fold-inner', body));
  const show = (open) => {
    toggle.setAttribute('aria-expanded', String(open));
    wrap.classList.toggle('is-open', open);
    wrap.inert = !open;
  };
  toggle.addEventListener('click', () => {
    const open = toggle.getAttribute('aria-expanded') !== 'true';
    ctx.expanded.set(key, open);
    show(open);
  });
  show(ctx.expanded.get(key) === true);
  return wrap;
}

/** Engine text, verbatim. `out:N` handles open the full output; a leading [STATUS] is set in bold. */
function verbatim(text, ctx) {
  const pre = h('pre.block');
  const status = /^\[[A-Za-z]+\]/.exec(text)?.[0] ?? '';
  if (status) pre.append(h('span.obs-tag', status));
  let from = status.length;
  for (const match of text.matchAll(/\bout:\d+\b/g)) {
    if (match.index < from) continue;
    pre.append(text.slice(from, match.index), h('button.out-link', { type: 'button', onclick: () => ctx.openOutput(match[0]) }, match[0]));
    from = match.index + match[0].length;
  }
  pre.append(text.slice(from));
  return pre;
}

const ITEMS = {
  user(item) {
    return h('section.user', { 'aria-label': item.followUp ? 'Follow-up' : 'Task' }, item.text);
  },

  /** What came in beside the turns. The snapshot needs no row of its own to open: it is the block. */
  aside(item, ctx) {
    const snapshots = item.rows.filter((row) => row.kind === 'env').length;
    const notes = item.rows.length - snapshots;
    const label = [snapshots > 0 && 'Environment snapshot', notes > 0 && plural(notes, 'note')].filter(Boolean).join(', ');
    const toggle = h('button.activity-toggle', { type: 'button' }, h('span.activity-label', label), chevron());
    const tree = h('div.tree', item.rows.map((row) => (row.kind === 'env' ? h('div.tree-block', verbatim(row.text, ctx)) : noteRow(row, ctx))));
    return h('section.aside', toggle, fold(ctx, item.key, toggle, tree));
  },

  /** How the run ended: its summary as the answer it is, and one line for the outcome. */
  result(item) {
    const look = outcomeLook(item.status, item.verified);
    const summary = item.summary.trim();
    // The evidence says in its own words whether the result was checked; without it, one word does.
    const checked = item.evidence || (!item.verified ? 'unverified' : item.status === 'done' ? 'verified' : '');
    return h(
      'section.result',
      summary && !item.repeatsReply && h('div.reply', summary),
      h('div.result-line', icon(look.icon, 14), h('span.result-status', known(RESULT_WORDS, item.status) ?? look.label), checked && h('span', `· ${checked}`)),
    );
  },

  approval: approvalRow,
};

function turnView(turn, ctx) {
  const el = h(`section.turn${turn.live ? '.is-live' : ''}`, { dataset: { turn: turn.turn } }, turnParts(turn, ctx));
  return { el, patch: () => el.replaceChildren(...turnParts(turn, ctx)) };
}

/**
 * A turn on the page: what the model said, the line for what it did with the tree folded under it,
 * and any approval request that is waiting, which cannot wait inside a fold.
 */
function turnParts(turn, ctx) {
  const streaming = turn.live && !turn.reply;
  const content = turn.content.trim();
  const reply = content && h('div.reply', content, streaming && h('span.caret'));

  const rows = [];
  const cards = [];
  const reasoning = turn.reasoning.trim();
  if (reasoning) {
    const body = h('div.thought', reasoning, streaming && !content && h('span.caret'));
    rows.push(treeRow(ctx, `${turn.key}-reasoning`, [h('span.tree-verb', streaming ? 'Thinking' : 'Thought'), h('span.tree-preview', firstLine(reasoning))], body));
  }
  for (const row of turn.rows) {
    try {
      if (row.kind === 'approval' && awaitsAnswer(row, ctx)) cards.push(approvalRow(row, ctx));
      else rows.push(row.kind === 'step' ? stepRow(row, ctx, turn) : (ITEMS[row.kind] ?? noteRow)(row, ctx));
    } catch (problem) {
      rows.push(undrawable(row, problem, ctx));
    }
  }

  let line = null;
  const said = activity(turn);
  if (said) {
    const queued = turn.live && turn.reply ? turn.reply.calls - turn.steps.size : 0;
    const toggle = h(
      'button.activity-toggle',
      { type: 'button' },
      said.busy && spinner(),
      h('span.activity-label', said.parts.map((part) => (typeof part === 'string' ? part : h('b', part.name)))),
      said.notes.length > 0 && h('span.activity-notes', `· ${said.notes.join(', ')}`),
      said.since != null && h('span.activity-timer', { dataset: { since: said.since } }),
      chevron(),
    );
    const tree = h('div.tree', rows, queued > 0 && h('div.tree-facts', `${plural(queued, 'more step')} queued`), turnFacts(turn));
    line = h('div.activity', toggle, fold(ctx, turn.key, toggle, tree));
  }
  // Words before tool calls introduce them; with no calls, the thinking came before the words.
  const acted = turn.steps.size > 0;
  return [acted && reply, line, !acted && reply, cards].flat().filter(Boolean);
}

/** The numbers of a turn, at the foot of its tree. */
function turnFacts(turn) {
  const facts = [`Turn ${turn.turn}`];
  const reply = turn.reply;
  if (reply) {
    const { usage } = reply;
    facts.push(duration(reply.ms), `in ${k(usage.prompt)}, ${pct(usage.cached, usage.prompt)} cached`, `out ${k(usage.output)}, ${k(usage.reasoning)} thinking`);
    if (reply.repairs > 0) facts.push(`${reply.repairs} repaired`);
    if (reply.finish === 'length') facts.push('output limit');
  } else if (!turn.live) {
    facts.push('no reply');
  }
  if (turn.cache) {
    const { verdict, cached, expected } = turn.cache;
    facts.push(h('span', { title: expected > 0 ? `${k(cached)} of ${k(expected)} cached` : null }, known(CACHE_WORDS, verdict) ?? `cache ${verdict}`));
  }
  if (turn.effort) facts.push(`effort ${turn.effort}`);
  return h('div.tree-facts', facts.flatMap((fact, i) => (i ? [' · ', fact] : [fact])));
}

/** One row of a tree. With `detail` it is a toggle that unfolds it; without, a plain line. */
function treeRow(ctx, key, line, detail, options = {}) {
  const { state, ...attributes } = options;
  const spec = `div.tree-row${state ? `.is-${state.replace(/[^a-z0-9-]/gi, '')}` : ''}`;
  if (!detail) return h(spec, attributes, h('div.tree-line', h('span.chev-slot'), line));
  const toggle = h('button.tree-line', { type: 'button' }, chevron(), line);
  return h(spec, attributes, toggle, fold(ctx, key, toggle, detail));
}

function stepRow(step, ctx, turn) {
  // A step with no end in a turn that is over was cut short (stop, or the app closing).
  const status = step.status === 'active' && !turn.live ? 'unfinished' : step.status;
  const active = status === 'active';
  const state = active || status === 'ok' ? null : known(STATE_WORDS, status) ?? status;
  const line = [
    h('span.tree-verb', stepVerb(step.tool, active)),
    h('span.tree-what', step.summary),
    state && h('span.tree-state', state),
    active ? h('span.tree-took', spinner(), h('span', { dataset: { since: step.startedAt } })) : step.ms >= 100 && h('span.tree-took', duration(step.ms)),
  ];
  return treeRow(ctx, step.key, line, stepDetail(step, status, ctx), { state: status, dataset: { step: `${turn.turn}.${step.idx + 1}` } });
}

/** What unfolds under a step: its prediction and whether it held, then what came back. */
function stepDetail(step, status, ctx) {
  const skipped = status === 'skipped';
  const observation = skipped ? step.observation.replace(/^\[skipped\]\s*/, '') : step.observation;
  if (!observation && step.expect == null) return null;
  const verdict = status === 'ok' && step.explicit ? 'held' : status === 'surprise' ? 'did not hold' : null;
  // Violations the observation's first line does not already state.
  const unstated = step.violations.filter((violation) => !firstLine(step.observation).includes(violation));
  return h(
    'div.tree-detail',
    step.expect != null && h('div.detail-line', h('span.detail-key', 'Expected'), h('span.detail-mono', step.expect), verdict && h('span.detail-verdict', verdict)),
    unstated.map((violation) => h('div.detail-line', violation)),
    observation && (skipped ? h('div.detail-line', observation) : verbatim(observation, ctx)),
    step.hidden > 0 && h('div.detail-foot', `${bytes(step.hidden)} kept out of context`),
  );
}

/**
 * A quiet line for what happened beside the steps: a watch change, a note, a lesson, a retry.
 * One that is long or runs over several lines shows its first line and unfolds the rest.
 */
function noteRow(note, ctx) {
  const head = firstLine(note.text);
  const label = note.label.charAt(0).toUpperCase() + note.label.slice(1);
  const line = [h('span.tree-verb', label), note.badge && h('span.tree-state', note.badge), h('span.tree-preview', head)];
  const long = head.length < note.text.length || head.length > LONG_NOTE;
  return treeRow(ctx, note.key, line, long ? h('div.tree-detail', verbatim(note.text, ctx)) : null, { state: 'note' });
}

const awaitsAnswer = (row, ctx) => row.state === 'pending' && ctx.canAnswer();

/** A card while the request waits; a quiet line once it is answered or can no longer be. */
function approvalRow(row, ctx) {
  if (!awaitsAnswer(row, ctx)) {
    const verdict = known({ allowed: 'allowed', denied: 'denied' }, row.state) ?? 'never answered';
    // The line sits under the step it was about, so the reason is all it needs to say.
    return noteRow({ key: row.key, label: 'approval', badge: verdict, text: row.reason || `${row.tool} ${row.summary}` }, ctx);
  }
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const answer = async (allow) => {
    for (const button of buttons) button.disabled = true;
    try {
      await ctx.answer(row.id, allow);
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
    { dataset: { approval: row.id } },
    h('div.approval-title', icon('bang'), 'Approval needed'),
    h('div.approval-what', h('b', row.tool), row.summary),
    h('div.approval-why', row.reason),
    h('div.approval-actions', error, buttons),
  );
}
