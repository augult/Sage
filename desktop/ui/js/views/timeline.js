// The session timeline. One element per timeline item, rebuilt only when that item changed;
// streamed text is patched in place. Disclosure state lives in `ctx.expanded`, so a rebuilt
// item comes back the way the user left it.
//
// ctx: { expanded: Map, canAnswer(): boolean, answer(approvalId, allow): Promise, openOutput(handle) }

import { h, icon, spinner } from '../dom.js';
import { bytes, duration, firstLine, k, pct, plural } from '../format.js';
import { known, outcomeLook, tag } from './common.js';

const STEP_LOOKS = {
  ok: { icon: 'check', tone: 'teal', label: 'ok' },
  surprise: { icon: 'bang', tone: 'amber', label: 'surprise' },
  error: { icon: 'cross', tone: 'red', label: 'error' },
  skipped: { icon: 'dash', tone: 'plain', label: 'skipped' },
  running: { icon: 'clock', tone: 'violet', label: 'still running, as a background job' },
  denied: { icon: 'denied', tone: 'red', label: 'denied' },
  unfinished: { icon: 'dash', tone: 'plain', label: 'did not finish' },
};

// Steps whose observation is worth reading without being asked.
const OPEN_BY_DEFAULT = new Set(['surprise', 'error', 'denied', 'running']);

const CACHE_LOOKS = {
  first: ['first request', 'plain', 'The first request of the session: nothing could have been cached.'],
  hit: ['cache hit', 'teal', 'The provider served the previous prompt from its cache.'],
  partial: ['partial cache', 'amber', 'Only part of the previous prompt was served from cache.'],
  miss: ['cache miss', 'amber', 'The prefix was byte-identical, but the provider reported no cache hit. Not a harness fault.'],
  bust: ['cache bust', 'amber', 'The harness changed what was already sent (old observations trimmed, or a tool added), so the provider cache restarts there.'],
  unknown: ['no usage', 'plain', 'The provider reported no usage for this request.'],
};

const RESULT_WORDS = { done: 'Done', yielded: 'Handed back', stopped: 'Stopped', budget: 'Out of budget', error: 'Failed' };

export function createTimeline(container, ctx) {
  const rendered = [];

  function build(item) {
    const view = item.kind === 'turn' ? turnCard(item, ctx) : { el: (ITEMS[item.kind] ?? chipRow)(item, ctx) };
    return { ...view, item, rev: item.rev, streamRev: item.streamRev };
  }

  return {
    update(items) {
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
          have.patch();
          have.streamRev = item.streamRev;
        }
      });
      for (const stale of rendered.splice(items.length)) stale.el.remove();
    },
  };
}

/** A button that shows or hides `body` and remembers the choice under `key`. */
function disclosure(ctx, key, openByDefault, className, body, ...content) {
  const open = ctx.expanded.get(key) ?? openByDefault;
  body.hidden = !open;
  const toggle = h(
    `button.${className}`,
    {
      type: 'button',
      'aria-expanded': String(open),
      onclick: () => {
        const next = body.hidden;
        ctx.expanded.set(key, next);
        body.hidden = !next;
        toggle.setAttribute('aria-expanded', String(next));
      },
    },
    ...content,
  );
  return toggle;
}

/** Observation text, verbatim. `out:N` handles open the full output; a leading [STATUS] is tinted. */
function observation(text, ctx, tone) {
  const pre = h('pre.block');
  const status = /^\[[A-Za-z]+\]/.exec(text)?.[0] ?? '';
  if (status) pre.append(h(`span.obs-tag.tone-${tone}`, status));
  let from = status.length;
  for (const match of text.matchAll(/\bout:\d+\b/g)) {
    if (match.index < from) continue;
    pre.append(text.slice(from, match.index), h('button.out-link', { type: 'button', title: 'Open the full output', onclick: () => ctx.openOutput(match[0]) }, match[0]));
    from = match.index + match[0].length;
  }
  pre.append(text.slice(from));
  return pre;
}

const ITEMS = {
  user(item) {
    return h('section.user', h('div.label', item.followUp ? 'Follow-up' : 'Task'), h('div.user-text', item.text));
  },

  env(item, ctx) {
    const body = h('pre.block', item.text);
    const lines = item.text.split('\n').length;
    const toggle = disclosure(ctx, item.key, false, 'fold-toggle', body, icon('chevron', 12), 'Environment snapshot', h('span.muted', `· ${plural(lines, 'line')}`));
    return h('section.env', toggle, body);
  },

  result(item) {
    const look = outcomeLook(item.status, item.verified);
    return h(
      `section.result.tone-${look.tone}`,
      h(
        'div.result-head',
        h('span.glyph', icon(look.icon)),
        h('span.result-status', known(RESULT_WORDS, item.status) ?? look.label),
        item.verified ? tag('verified', 'teal', 'A check passed after the last change to the workspace') : tag('unverified', 'amber', 'No check has passed since the last change to the workspace'),
        h('span.result-evidence', item.evidence),
      ),
      item.summary.trim() && !item.repeatsReply && h('div.result-summary', item.summary.trim()),
    );
  },

  approval: approvalRow,
  chip: chipRow,
};

function turnCard(turn, ctx) {
  const stream = h('div.turn-stream', streamBlocks(turn, ctx));
  const rows = turn.rows.map((row) => (row.kind === 'step' ? stepRow(row, ctx, turn) : (ITEMS[row.kind] ?? chipRow)(row, ctx)));
  const queued = turn.live && turn.reply ? turn.reply.calls - turn.steps.size : 0;
  if (queued > 0) rows.push(h('div.queued', `${plural(queued, 'more step')} queued`));
  const el = h(`section.turn${turn.live ? '.is-live' : ''}`, { dataset: { turn: turn.turn } }, turnHead(turn), stream, h('div.script', rows));
  return { el, patch: () => stream.replaceChildren(...streamBlocks(turn, ctx)) };
}

function turnHead(turn) {
  const facts = h('div.turn-facts');
  const reply = turn.reply;
  if (reply) {
    const { usage } = reply;
    facts.append(
      h('span', { title: 'Time the model took for this reply' }, h('b', duration(reply.ms))),
      h('span', { title: 'Prompt tokens, and the share served from the provider cache' }, 'in ', h('b', k(usage.prompt)), ` · ${pct(usage.cached, usage.prompt)} cached`),
      h('span', { title: 'Output tokens, and how many of them were reasoning' }, 'out ', h('b', k(usage.output)), ` · ${k(usage.reasoning)} thinking`),
    );
    if (reply.repairs > 0) facts.append(tag(`${reply.repairs} repaired`, 'amber', 'Malformed tool calls the harness repaired'));
    if (reply.finish === 'length') facts.append(tag('output limit', 'amber', 'The reply hit the output limit; a call cut off by it is not run'));
  } else if (turn.live) {
    facts.append(h('span.thinking', spinner(), 'thinking', h('span', { dataset: { since: turn.startedAt } })));
  }
  if (turn.cache) {
    const { verdict, cached, expected } = turn.cache;
    const [label, tone, meaning] = known(CACHE_LOOKS, verdict) ?? [`cache ${verdict}`, 'plain', ''];
    const numbers = expected > 0 ? ` ${k(cached)} of the ${k(expected)} tokens sent before came from cache.` : '';
    facts.append(tag(label, tone, `${meaning}${numbers}`.trim()));
  }
  return h('header.turn-head', h('span.turn-no', `Turn ${turn.turn}`), tag(`effort ${turn.effort}`, 'plain', 'Reasoning effort for this reply'), facts);
}

/** Reasoning (open while the turn is live, folded to a one-line preview afterwards) and the reply text. */
function streamBlocks(turn, ctx) {
  const streaming = turn.live && !turn.reply;
  const caret = () => streaming && h('span.caret');
  const blocks = [];
  const reasoning = turn.reasoning.trim();
  if (reasoning) {
    const body = h('div.reasoning-body', reasoning, !turn.content && caret());
    const preview = h('span.fold-preview', firstLine(reasoning));
    const toggle = disclosure(ctx, `${turn.key}-reasoning`, turn.live, 'fold-toggle', body, icon('chevron', 12), h('span.label', 'Reasoning'), preview);
    // The preview stands in for the body, so it shows only while the body is hidden.
    const sync = () => (preview.hidden = !body.hidden);
    toggle.addEventListener('click', sync);
    sync();
    blocks.push(h('div.reasoning', toggle, body));
  }
  if (turn.content.trim()) blocks.push(h('div.reply', turn.content.trim(), caret()));
  return blocks;
}

function stepRow(step, ctx, turn) {
  // A step with no end in a turn that is over was cut short (stop, or the app closing).
  const status = step.status === 'active' && !turn.live ? 'unfinished' : step.status;
  const active = status === 'active';
  const look = active ? { tone: 'violet', label: 'running' } : known(STEP_LOOKS, status) ?? { icon: 'dot', tone: 'plain', label: status };
  const line = [
    h('span.step-no', String(step.idx + 1)),
    h('span.step-tool', step.tool),
    h('span.step-main', h('span.step-summary', step.summary), status === 'skipped' && tag('skipped'), expectChip(step, status)),
    active ? h('span.step-took', { dataset: { since: step.startedAt } }) : step.ms >= 100 && h('span.step-took', duration(step.ms)),
  ];
  const row = h(
    `div.row.step.is-${status.replace(/[^a-z0-9-]/gi, '')}`,
    { dataset: { step: `${turn.turn}.${step.idx + 1}` } },
    h(`span.step-glyph.tone-${look.tone}`, { title: look.label, role: 'img', 'aria-label': look.label }, active ? spinner() : icon(look.icon, 12)),
  );
  if (status === 'skipped') {
    row.append(h('div.step-line', line), h('div.step-note', step.observation.replace(/^\[skipped\]\s*/, '')));
  } else if (!step.observation) {
    row.append(h('div.step-line', line));
  } else {
    // Violations the observation's first line does not already state.
    const unstated = step.violations.filter((violation) => !firstLine(step.observation).includes(violation));
    const detail = h(
      'div.step-detail',
      unstated.map((violation) => h('div.step-violation', violation)),
      observation(step.observation, ctx, look.tone),
      step.hidden > 0 && h('div.step-foot', `${bytes(step.hidden)} of output stayed out of the context.`),
    );
    row.append(disclosure(ctx, step.key, OPEN_BY_DEFAULT.has(status), 'step-line', detail, line), detail);
  }
  return row;
}

/** The prediction for a step. It turns teal when it held and amber when reality contradicted it. */
function expectChip(step, status) {
  if (step.expect == null) {
    return status === 'surprise' ? h('span.expect.is-violated', { title: 'Not what the harness expects of this step' }, h('span.expect-text', 'surprise')) : null;
  }
  const verdict = status === 'ok' && step.explicit ? 'held' : status === 'surprise' ? 'violated' : null;
  const meaning =
    verdict === 'held'
      ? 'Reality matched the prediction, so the model got a receipt instead of the output.'
      : verdict === 'violated'
        ? 'Reality contradicted the prediction. The script stopped here.'
        : 'What the model predicted this step would do.';
  return h(
    `span.expect${verdict ? `.is-${verdict}` : ''}`,
    { title: meaning },
    h('span.expect-text', `expect ${step.expect}`),
    verdict && h('span.expect-verdict', icon(verdict === 'held' ? 'check' : 'bang', 12), verdict),
  );
}

/** A slim one-line note. Text that runs over several lines unfolds on demand. */
function chipRow(chip, ctx) {
  const [head, ...rest] = chip.text.split('\n');
  const content = [h('span.chip-label', chip.label), h('span.chip-text', head)];
  const row = h(`div.row.chip.tone-${chip.tone}`, h('span.chip-dot'));
  if (rest.length) {
    const body = h('pre.block', rest.join('\n'));
    const more = icon('chevron', 12);
    more.classList.add('chip-more');
    row.append(disclosure(ctx, chip.key, false, 'chip-line', body, content, more), body);
  } else {
    row.append(h('div.chip-line', content));
  }
  return row;
}

/** A prominent card while the request waits; a chip once it is answered or can no longer be. */
function approvalRow(row, ctx) {
  if (row.state !== 'pending' || !ctx.canAnswer()) {
    const [tone, verdict] = { allowed: ['teal', 'allowed'], denied: ['red', 'denied'] }[row.state] ?? ['plain', 'never answered'];
    return chipRow({ key: row.key, tone, label: 'approval', text: `${verdict} · ${row.tool} ${row.summary}` }, ctx);
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
    'div.row.approval',
    { dataset: { approval: row.id } },
    h(
      'div.approval-card',
      h('div.approval-title', icon('bang'), 'Approval needed'),
      h('div.approval-what', h('b', row.tool), row.summary),
      h('div.approval-why', row.reason),
      h('div.approval-actions', error, buttons),
    ),
  );
}
