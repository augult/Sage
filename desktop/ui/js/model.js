// The session view-model: a reducer from engine events to what the timeline and the inspector show.
// Pure data (no DOM, no IPC), so a session can be rebuilt from its stored log at any time.
//
// `items` is the timeline in order. A turn item owns `rows`: its steps, with the chips and approval
// requests that arrived while it ran, in arrival order. `rev` counts structural changes to an item
// and `streamRev` counts streamed text, so the view can tell a re-render from a text patch.

import { duration, firstLine, k } from './format.js';

export function createSession(meta) {
  return {
    id: meta.id,
    meta: { autonomy: 'auto', ...meta },
    lastSeq: 0,
    /** A run is in progress. Starts from the listing, because a new session has no events yet. */
    running: meta.status === 'running',
    /** Timestamp of the message that started the current run. */
    runStartedAt: meta.created ?? 0,
    /** `{ status, verified }` of the most recent run, once it has ended. */
    outcome: null,
    items: [],
    turns: new Map(),
    /** Number of the turn in progress, if any. */
    openTurn: null,
    approvals: new Map(),
    stats: null,
    watches: new Map(),
    notes: [],
    lessons: [],
    /** Counts changes to anything the inspector shows. */
    sideRev: 0,
  };
}

export function replay(meta, events) {
  const session = createSession(meta);
  for (const event of events) apply(session, event);
  return session;
}

/** Marks the end of a log that will get no more events: whatever was in progress was cut off there. */
export function seal(s) {
  closeTurn(s);
  s.running = false;
}

/** Applies one event. Returns false for one that was already applied (same or older `seq`). */
export function apply(s, ev) {
  if (!ev || !(ev.seq > s.lastSeq)) return false;
  s.lastSeq = ev.seq;
  const handler = Object.hasOwn(HANDLERS, ev.type) ? HANDLERS[ev.type] : unknown;
  handler(s, ev);
  return true;
}

/** Approval requests still waiting for an answer. */
export function pendingApprovals(s) {
  if (!s.running) return 0;
  let pending = 0;
  for (const { row } of s.approvals.values()) if (row.state === 'pending') pending++;
  return pending;
}

const str = (value) => (value == null ? '' : String(value));

// Every field the inspector reads, so a `stats` event that lacks one still renders.
const NO_STATS = {
  turns: 0,
  steps: 0,
  skipped: 0,
  surprises: 0,
  predictions: 0,
  predictions_held: 0,
  prompt_tokens: 0,
  cached_tokens: 0,
  output_tokens: 0,
  reasoning_tokens: 0,
  context_tokens: 0,
  cost: 0,
  priced: false,
  hidden_bytes: 0,
  repairs: 0,
  retries: 0,
  cache_misses: 0,
  elapsed_ms: 0,
  model_ms: 0,
  tool_ms: 0,
};

const HANDLERS = {
  session_start(s, ev) {
    for (const field of ['title', 'cwd', 'provider', 'model', 'effort', 'mode', 'autonomy', 'simulated']) {
      if (ev[field] != null) s.meta[field] = ev[field];
    }
    s.sideRev++;
  },

  user(s, ev) {
    closeTurn(s);
    const followUp = s.items.some((item) => item.kind === 'user');
    s.items.push({ kind: 'user', key: `user-${ev.seq}`, rev: 0, text: str(ev.text), followUp });
    s.running = true;
    s.runStartedAt = ev.ts;
    s.outcome = null;
  },

  env(s, ev) {
    s.items.push({ kind: 'env', key: `env-${ev.seq}`, rev: 0, text: str(ev.text) });
  },

  turn_start(s, ev) {
    closeTurn(s);
    const turn = {
      kind: 'turn',
      key: `turn-${ev.seq}`,
      rev: 0,
      streamRev: 0,
      turn: ev.turn,
      effort: str(ev.effort),
      startedAt: ev.ts,
      live: true,
      reasoning: '',
      content: '',
      /** Set by the `model` event: `{ ms, usage, finish, repairs, calls }`. */
      reply: null,
      cache: null,
      rows: [],
      steps: new Map(),
    };
    s.turns.set(ev.turn, turn);
    s.openTurn = ev.turn;
    s.items.push(turn);
  },

  delta(s, ev) {
    const turn = s.turns.get(ev.turn);
    if (!turn || (ev.channel !== 'reasoning' && ev.channel !== 'content')) return;
    turn[ev.channel] += str(ev.text);
    turn.streamRev++;
  },

  retry(s, ev) {
    const turn = s.turns.get(ev.turn);
    if (turn) {
      // The provider call starts over, so whatever it streamed for this turn is void.
      turn.reasoning = '';
      turn.content = '';
    }
    place(s, ev, chip(ev, 'amber', 'retry', `attempt ${ev.attempt}: ${str(ev.reason)} — retrying in ${duration(ev.wait_ms ?? 0)}`));
  },

  model(s, ev) {
    const turn = s.turns.get(ev.turn);
    if (!turn) return;
    turn.reasoning = str(ev.reasoning);
    turn.content = str(ev.content);
    turn.reply = {
      ms: ev.ms ?? 0,
      usage: { prompt: 0, cached: 0, output: 0, reasoning: 0, ...ev.usage },
      finish: str(ev.finish),
      repairs: ev.repairs ?? 0,
      calls: Array.isArray(ev.calls) ? ev.calls.length : 0,
    };
    turn.rev++;
  },

  cache(s, ev) {
    const turn = s.turns.get(ev.turn);
    if (!turn) return;
    turn.cache = { verdict: str(ev.verdict), cached: ev.cached ?? 0, expected: ev.expected ?? 0 };
    turn.rev++;
  },

  step_start(s, ev) {
    const turn = s.turns.get(ev.turn);
    if (turn) startStep(turn, ev);
  },

  step_end(s, ev) {
    const turn = s.turns.get(ev.turn);
    if (!turn) return;
    const step = turn.steps.get(ev.idx) ?? startStep(turn, ev);
    step.status = str(ev.status);
    step.observation = str(ev.observation);
    step.ms = ev.ms ?? 0;
    step.hidden = ev.hidden_bytes ?? 0;
    step.violations = Array.isArray(ev.violations) ? ev.violations.map(str) : [];
    step.explicit = ev.explicit === true;
    // No event announces a watch being switched off; its own step is the only trace.
    if (step.tool === 'watch' && step.status === 'ok' && step.summary.endsWith(' off')) {
      s.watches.delete(step.summary.slice(0, -' off'.length));
      s.sideRev++;
    }
    turn.rev++;
  },

  watch(s, ev) {
    const name = str(ev.name);
    const detail = str(ev.detail);
    s.watches.set(name, { name, cmd: str(ev.cmd), state: str(ev.state), detail: firstLine(detail) });
    s.sideRev++;
    // An empty detail is the engine's "nothing changed, still passing".
    const tone = !detail ? 'plain' : ev.state === 'pass' ? 'teal' : ev.state === 'fail' ? 'amber' : 'plain';
    place(s, ev, chip(ev, tone, 'watch', detail ? detail.replace(/^watch /, '') : `${name}: still passing`));
  },

  note(s, ev) {
    s.notes.push({ label: str(ev.kind) || 'note', text: str(ev.text) });
    s.sideRev++;
    place(s, ev, chip(ev, 'violet', 'note', str(ev.text)));
  },

  lesson(s, ev) {
    const label = ev.learned ? 'learned' : 'recalled';
    s.lessons.push({ label, text: str(ev.text) });
    s.sideRev++;
    place(s, ev, chip(ev, 'teal', label, str(ev.text)));
  },

  approval(s, ev) {
    const row = { kind: 'approval', key: `approval-${ev.seq}`, rev: 0, id: ev.id, tool: str(ev.tool), summary: str(ev.summary), reason: str(ev.reason), state: 'pending' };
    s.approvals.set(ev.id, { row, turn: place(s, ev, row) });
  },

  approval_resolved(s, ev) {
    const request = s.approvals.get(ev.id);
    if (!request) return;
    request.row.state = ev.allow ? 'allowed' : 'denied';
    (request.turn ?? request.row).rev++;
  },

  effort(s, ev) {
    place(s, ev, chip(ev, 'violet', 'effort', `${str(ev.from)} → ${str(ev.to)} · ${str(ev.reason)}`));
  },

  decay(s, ev) {
    place(s, ev, chip(ev, 'amber', 'trim', `${ev.messages ?? 0} old observations left the context (~${k(ev.dropped_tokens ?? 0)} tokens): ${str(ev.reason)}`));
  },

  stats(s, ev) {
    if (!ev.stats || typeof ev.stats !== 'object') return;
    s.stats = { ...NO_STATS, ...ev.stats };
    s.sideRev++;
  },

  finish(s, ev) {
    closeTurn(s);
    s.running = false;
    s.outcome = { status: str(ev.status), verified: ev.verified === true };
    const summary = str(ev.summary);
    // A run that hands back in prose reports that same prose as its summary.
    const lastTurn = s.items.findLast((item) => item.kind === 'turn');
    const repeatsReply = summary.trim() !== '' && lastTurn?.content.trim() === summary.trim();
    s.items.push({ kind: 'result', key: `result-${ev.seq}`, rev: 0, status: str(ev.status), verified: ev.verified === true, evidence: str(ev.evidence), summary, repeatsReply });
  },

  error(s, ev) {
    place(s, ev, chip(ev, 'red', 'error', str(ev.message)));
  },
};

/** An event type this build does not know: keep it visible as a generic chip. */
function unknown(s, ev) {
  place(s, ev, chip(ev, 'plain', str(ev.type) || 'event', compact(ev)));
}

const TEXT_FIELDS = ['text', 'message', 'summary', 'detail', 'reason', 'name'];
const ENVELOPE = new Set(['seq', 'ts', 'type', 'turn']);

/** One line for an unknown event: its main text field if it has one, else its scalar fields. */
function compact(ev) {
  const text = TEXT_FIELDS.map((field) => ev[field]).find((value) => typeof value === 'string' && value.trim());
  if (text) return text;
  return Object.entries(ev)
    .filter(([name, value]) => !ENVELOPE.has(name) && ['string', 'number', 'boolean'].includes(typeof value))
    .map(([name, value]) => `${name} ${value}`)
    .join(' · ');
}

function chip(ev, tone, label, text) {
  return { kind: 'chip', key: `chip-${ev.seq}`, rev: 0, tone, label, text };
}

/** Rows belong to the turn they name, else to the turn in progress, else to the timeline itself. */
function place(s, ev, row) {
  const turn = s.turns.get(ev.turn ?? s.openTurn);
  if (turn) {
    turn.rows.push(row);
    turn.rev++;
  } else {
    s.items.push(row);
  }
  return turn ?? null;
}

function startStep(turn, ev) {
  const step = {
    kind: 'step',
    key: `step-${turn.turn}-${ev.idx}`,
    idx: ev.idx,
    tool: str(ev.tool),
    summary: str(ev.summary),
    expect: ev.expect == null ? null : str(ev.expect),
    /** `active` until `step_end` supplies the engine's status. */
    status: 'active',
    startedAt: ev.ts,
    observation: '',
    ms: 0,
    hidden: 0,
    violations: [],
    explicit: false,
  };
  turn.steps.set(ev.idx, step);
  turn.rows.push(step);
  turn.rev++;
  return step;
}

function closeTurn(s) {
  const turn = s.turns.get(s.openTurn);
  if (turn) {
    turn.live = false;
    turn.rev++;
  }
  s.openTurn = null;
}
