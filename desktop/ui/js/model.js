// The session view-model: a reducer from engine events to what the timeline and the inspector show.
// Pure data (no DOM, no IPC), so a session can be rebuilt from its stored log at any time.
//
// `items` is the timeline in order. A turn item owns `rows`: its steps, with the chips and approval
// requests that arrived while it ran, in arrival order. `rev` counts structural changes to an item
// and `streamRev` counts streamed text, so the view can tell a re-render from a text patch.
//
// No event is dropped. One this build does not know, or cannot make sense of, becomes a neutral
// chip that names its type, and the events after it are applied as usual. The one exception is what
// a mechanism since retired wrote into older logs (`RETIRED`): that is passed over.

import { duration, effortLabel, firstLine, k } from './format.js';

function createSession(meta) {
  return {
    id: meta.id,
    meta: { autonomy: 'auto', ...meta },
    lastSeq: 0,
    /** Numbers the keys of items and rows. The same log always yields the same keys. */
    serial: 0,
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
    /** Messages the engine answered itself, with no run: `/goal` and `/goal clear`. */
    commands: 0,
    /** Counts changes to anything the inspector shows. */
    sideRev: 0,
  };
}

/** Builds a session from its stored log, taken whole and in order whatever its numbering. */
export function replay(meta, events) {
  const session = createSession(meta);
  for (const event of Array.isArray(events) ? events : []) reduce(session, event);
  return session;
}

/** Marks the end of a log that will get no more events: whatever was in progress was cut off there. */
export function seal(s) {
  closeTurn(s);
  s.running = false;
}

/** Applies one live event. Returns false for one that was already applied (same or older `seq`). */
export function apply(s, ev) {
  if (!ev || !(ev.seq > s.lastSeq)) return false;
  reduce(s, ev);
  return true;
}

/** Event types of mechanisms that are gone (reflex memory, the review pass). Logs still hold them. */
const RETIRED = new Set(['lesson', 'review']);

function reduce(s, ev) {
  if (ev === null || typeof ev !== 'object') ev = { type: 'unreadable', text: str(ev) };
  if (ev.seq > s.lastSeq) s.lastSeq = ev.seq;
  if (RETIRED.has(ev.type)) return;
  const handler = Object.hasOwn(HANDLERS, ev.type) ? HANDLERS[ev.type] : unknown;
  try {
    // A handler that cannot use its event returns false, and the event is shown as it came.
    if (handler(s, ev) === false) unknown(s, ev);
  } catch {
    unknown(s, ev);
  }
}

/** Approval requests still waiting for an answer. */
export function pendingApprovals(s) {
  if (!s.running) return 0;
  let pending = 0;
  for (const { row } of s.approvals.values()) if (row.state === 'pending') pending++;
  return pending;
}

const str = (value) => (value == null ? '' : String(value));
const num = (value) => (Number.isFinite(value) ? value : 0);

/** A tool call's arguments, which arrive as JSON text. Anything but an object is no arguments. */
function parseArguments(text) {
  try {
    const value = JSON.parse(text);
    return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
  } catch {
    return null;
  }
}

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
  cache_busts: 0,
  elapsed_ms: 0,
  model_ms: 0,
  tool_ms: 0,
};

// How each state of a `goal` event reads on the timeline: tone, badge (null: met or open, as the
// event says), and whether the row's text is the goal or the reason given.
const GOAL_LOOKS = {
  set: ['quiet', 'set', 'goal'],
  status: ['quiet', null, 'goal'],
  cleared: ['quiet', 'cleared', 'goal'],
  met: ['good', 'met', 'goal'],
  not_met: ['alert', 'not met', 'reason'],
  unchecked: ['alert', 'not checked', 'reason'],
  open: ['alert', 'left open', 'reason'],
};

const HANDLERS = {
  session_start(s, ev) {
    for (const field of ['title', 'cwd', 'provider', 'model', 'effort', 'mode', 'autonomy', 'simulated', 'skills']) {
      if (ev[field] != null) s.meta[field] = ev[field];
    }
    s.sideRev++;
  },

  user(s, ev) {
    closeTurn(s);
    const followUp = s.items.some((item) => item.kind === 'user');
    s.items.push({ kind: 'user', key: key(s, 'user'), rev: 0, text: str(ev.text), files: Array.isArray(ev.files) ? ev.files.map(str) : [], followUp });
    // Skills switched on with this message stay on for the rest of the session.
    if (Array.isArray(ev.skills) && ev.skills.length) s.meta.skills = [...new Set([...(Array.isArray(s.meta.skills) ? s.meta.skills : []), ...ev.skills.map(str)])];
    s.running = true;
    s.runStartedAt = num(ev.ts);
    s.outcome = null;
  },

  /** What the user changed with the message before it: the session's from here on. */
  settings(s, ev) {
    for (const field of ['provider', 'model', 'effort', 'autonomy']) {
      if (ev[field] != null) s.meta[field] = str(ev[field]);
    }
    const now = [ev.model != null && `model ${str(ev.model)}`, ev.effort != null && `effort ${effortLabel(str(ev.effort))}`, ev.autonomy != null && (str(ev.autonomy) === 'ask' ? 'asks first' : 'acts without asking')];
    const row = chip(s, 'quiet', 'now', now.filter(Boolean).join(' · '));
    row.icon = 'sliders';
    place(s, ev, row);
    s.sideRev++;
  },

  env(s, ev) {
    s.items.push({ kind: 'env', key: key(s, 'env'), rev: 0, text: str(ev.text) });
  },

  turn_start(s, ev) {
    closeTurn(s);
    openTurn(s, ev);
  },

  delta(s, ev) {
    const turn = s.turns.get(ev.turn);
    // Streamed text is transient: the `model` event carries the whole reply again.
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
      turn.streamRev++;
      // One row for a run of attempts, saying the latest: with auto-retry a night can hold hundreds.
      if (turn.rows.at(-1)?.retry) turn.rows.pop();
    }
    const row = chip(s, 'alert', 'retry', `attempt ${num(ev.attempt)}: ${str(ev.reason)} — retrying in ${duration(num(ev.wait_ms))}`);
    row.retry = { attempt: num(ev.attempt), reason: str(ev.reason) };
    place(s, ev, row);
  },

  model(s, ev) {
    const turn = turnFor(s, ev);
    if (!turn) return false;
    turn.reasoning = str(ev.reasoning);
    turn.content = str(ev.content);
    const usage = ev.usage && typeof ev.usage === 'object' ? ev.usage : {};
    const calls = Array.isArray(ev.calls) ? ev.calls : [];
    // What each call was asked to do, for the rows that show it: a whole command, an edit's text.
    turn.calls = new Map(calls.map((call) => [str(call?.id), parseArguments(call?.arguments)]));
    turn.reply = {
      ms: num(ev.ms),
      usage: { prompt: num(usage.prompt), cached: num(usage.cached), output: num(usage.output), reasoning: num(usage.reasoning) },
      finish: str(ev.finish),
      repairs: num(ev.repairs),
      calls: calls.length,
    };
    turn.rev++;
  },

  cache(s, ev) {
    const turn = turnFor(s, ev);
    if (!turn) return false;
    turn.cache = { verdict: str(ev.verdict), cached: num(ev.cached), expected: num(ev.expected) };
    turn.rev++;
  },

  step_start(s, ev) {
    const turn = turnFor(s, ev);
    if (!turn) return false;
    startStep(turn, ev);
  },

  step_end(s, ev) {
    const turn = turnFor(s, ev);
    if (!turn) return false;
    const step = turn.steps.get(ev.idx) ?? startStep(turn, ev);
    step.status = str(ev.status);
    step.observation = str(ev.observation);
    step.ms = num(ev.ms);
    step.hidden = num(ev.hidden_bytes);
    step.violations = Array.isArray(ev.violations) ? ev.violations.map(str) : [];
    step.explicit = ev.explicit === true;
    step.change = ev.change && typeof ev.change === 'object' && Array.isArray(ev.change.hunks) ? ev.change : null;
    step.results = Array.isArray(ev.results) ? ev.results : null;
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
    const tone = !detail || ev.change === 'same' ? 'plain' : ev.state === 'pass' ? 'good' : ev.state === 'fail' ? 'alert' : 'plain';
    const text = detail ? detail.replace(/^watch /, '') : `${name}: still passing`;
    place(s, ev, chip(s, tone, 'watch', text, ev.change === 'new' ? 'new' : null));
  },

  note(s, ev) {
    const kind = str(ev.kind);
    s.notes.push({ label: kind || 'note', text: str(ev.text) });
    s.sideRev++;
    // `harness` is the general kind; any other names the mechanism that spoke (attach, gate, …).
    place(s, ev, chip(s, 'quiet', 'note', str(ev.text), kind && kind !== 'harness' ? kind : null));
  },

  /** The goal: set, asked about, cleared, or checked at a `finish`. `meta.goal` is `{ text, met }` or null. */
  goal(s, ev) {
    const state = str(ev.state);
    const look = Object.hasOwn(GOAL_LOOKS, state) ? GOAL_LOOKS[state] : null;
    if (!look) return false;
    const text = str(ev.goal);
    if (state === 'set') s.meta.goal = { text, met: false };
    else if (state === 'met') s.meta.goal = { text, met: true };
    else if (state === 'cleared') s.meta.goal = null;
    // Answered with no run: the message that asked is not on the timeline, this row is.
    if (state === 'status' || state === 'cleared') s.commands++;
    const [tone, badge, says] = look;
    const none = state === 'status' && !text;
    const row = chip(s, tone, 'goal', none ? 'None set' : says === 'reason' ? str(ev.reason) || text : text, none ? null : (badge ?? (ev.met === true ? 'met' : 'open')));
    row.icon = 'flag';
    place(s, ev, row);
    s.sideRev++;
  },

  approval(s, ev) {
    const row = { kind: 'approval', key: key(s, 'approval'), rev: 0, id: ev.id, tool: str(ev.tool), summary: str(ev.summary), reason: str(ev.reason), state: 'pending' };
    s.approvals.set(ev.id, { row, turn: place(s, ev, row) });
  },

  approval_resolved(s, ev) {
    const request = s.approvals.get(ev.id);
    if (!request) return false;
    request.row.state = ev.allow ? 'allowed' : 'denied';
    (request.turn ?? request.row).rev++;
  },

  effort(s, ev) {
    place(s, ev, chip(s, 'quiet', 'effort', `${str(ev.from)} → ${str(ev.to)} · ${str(ev.reason)}`));
  },

  decay(s, ev) {
    place(s, ev, chip(s, 'alert', 'trim', `${num(ev.messages)} old observations left the context (~${k(num(ev.dropped_tokens))} tokens): ${str(ev.reason)}`));
  },

  stats(s, ev) {
    if (!ev.stats || typeof ev.stats !== 'object') return false;
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
    s.items.push({ kind: 'result', key: key(s, 'result'), rev: 0, status: str(ev.status), verified: ev.verified === true, evidence: str(ev.evidence), summary, repeatsReply });
  },

  error(s, ev) {
    place(s, ev, chip(s, 'danger', 'error', str(ev.message)));
  },
};

/** An event type this build does not know: keep it visible as a neutral chip that names the type. */
function unknown(s, ev) {
  place(s, ev, chip(s, 'plain', str(ev.type) || 'event', compact(ev)));
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

const key = (s, kind) => `${kind}-${s.serial++}`;

function chip(s, tone, label, text, badge = null) {
  return { kind: 'chip', key: key(s, 'chip'), rev: 0, tone, label, text, badge };
}

/**
 * A row goes into the turn in progress when it names that turn or names none; anything else
 * (before the first turn, between runs) goes onto the timeline itself. Returns the turn, if any.
 */
function place(s, ev, row) {
  const open = s.turns.get(s.openTurn);
  const turn = open && (ev.turn == null || ev.turn === open.turn) ? open : null;
  if (turn) {
    turn.rows.push(row);
    turn.rev++;
  } else {
    s.items.push(row);
  }
  return turn;
}

function openTurn(s, ev) {
  const turn = {
    kind: 'turn',
    key: key(s, 'turn'),
    rev: 0,
    streamRev: 0,
    turn: ev.turn,
    effort: str(ev.effort),
    startedAt: num(ev.ts),
    live: true,
    reasoning: '',
    content: '',
    /** Set by the `model` event: `{ ms, usage, finish, repairs, calls }`. */
    reply: null,
    /** Arguments of the reply's tool calls, by call id. */
    calls: new Map(),
    cache: null,
    rows: [],
    steps: new Map(),
  };
  s.turns.set(ev.turn, turn);
  s.openTurn = ev.turn;
  s.items.push(turn);
  return turn;
}

/** The turn an event belongs to. A log that lost its `turn_start` still gets a card for the turn. */
function turnFor(s, ev) {
  const turn = s.turns.get(ev.turn);
  if (turn || !Number.isInteger(ev.turn)) return turn ?? null;
  closeTurn(s);
  return openTurn(s, { turn: ev.turn, ts: ev.ts });
}

function startStep(turn, ev) {
  const step = {
    kind: 'step',
    key: `step-${turn.key}-${ev.idx}`,
    idx: num(ev.idx),
    tool: str(ev.tool),
    summary: str(ev.summary),
    /** What the call was asked to do, as the model wrote it; null when the reply is not at hand. */
    args: turn.calls.get(str(ev.call_id)) ?? null,
    expect: ev.expect == null ? null : str(ev.expect),
    /** `active` until `step_end` supplies the engine's status. */
    status: 'active',
    startedAt: num(ev.ts),
    observation: '',
    ms: 0,
    hidden: 0,
    violations: [],
    explicit: false,
    /** What an edit or a write did to its file: `{ path, created, added, removed, hunks, truncated }`. */
    change: null,
    /** What a web search found, when the engine lists it: `[{ title, url, snippet }]`. */
    results: null,
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
