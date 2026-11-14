// Sessions the UI has opened. Each keeps its view-model for the life of the window; a session the
// engine still runs also keeps exactly one subscription, so nothing is missed while it is off screen.
// A session from an earlier launch is read from its log. One that can be continued (`resumable`) is
// subscribed to once a message has brought it back: `resume`.

import { api } from './api.js';
import { apply, replay, seal } from './model.js';

const entries = new Map();
const listeners = new Set();

/** `listener(entry)` runs after every change to any opened session. Returns the unsubscribe function. */
export function onChange(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const get = (id) => entries.get(id);

/** A session that was renamed is shown under its new title wherever it is open. */
export function retitle(id, title) {
  const entry = entries.get(id);
  if (!entry) return;
  entry.meta = { ...entry.meta, title };
  if (entry.model) {
    entry.model.meta = { ...entry.model.meta, title };
    // The pane beside the conversation names the session: it is drawn again.
    entry.model.sideRev++;
  }
  notify(entry);
}

/** A session that was deleted is let go of: what is still on its way from it has nowhere to arrive. */
export function forget(id) {
  entries.delete(id);
}

/** Returns the entry for a session, loading it on first use. `entry.model` is null until loaded. */
export function open(meta) {
  let entry = entries.get(meta.id);
  if (!entry) {
    const stored = meta.status === 'closed' || meta.status === 'resumable';
    entry = { id: meta.id, meta, live: !stored, resumable: meta.status === 'resumable', model: null, error: null, subscribed: false, syncing: false, queue: [], lastLive: 0 };
    entries.set(meta.id, entry);
    sync(entry);
  }
  return entry;
}

/**
 * (Re)builds the view-model from the stored events. The first call for a live session also
 * subscribes; later calls repair a feed that lagged. Live events wait in the queue meanwhile and
 * are applied afterwards, where `apply` drops the ones the stored log already covered.
 */
async function sync(entry) {
  if (entry.syncing) return;
  entry.syncing = true;
  try {
    const stored = entry.live && !entry.subscribed ? await api.subscribe(entry.id, (event) => receive(entry, event)) : await api.sessionEvents(entry.id);
    entry.subscribed = entry.live;
    entry.model = replay(entry.meta, stored);
    if (!entry.live) seal(entry.model);
    entry.error = null;
  } catch (error) {
    entry.error = String(error);
  }
  entry.syncing = false;
  entry.lastLive = 0;
  for (const event of entry.queue.splice(0)) receive(entry, event);
  notify(entry);
}

export const reload = (entry) => sync(entry);

/**
 * A stored session that was sent a message is the engine's to run from now on: subscribe, and
 * rebuild the view from its log, which has grown by the resumption and the message. The rebuild
 * is from the whole log, so nothing shows twice.
 */
export function resume(entry) {
  if (entry.live) return;
  entry.live = true;
  entry.resumable = false;
  entry.meta = { ...entry.meta, status: 'idle', why_closed: null };
  entry.subscribed = false;
  sync(entry);
}

function receive(entry, event) {
  if (entry.syncing) {
    entry.queue.push(event);
    return;
  }
  if (!entry.model) return;
  // The live feed numbers every event consecutively, so a jump means the feed lagged and events
  // were dropped. The stored log has them: rebuild from it.
  const lagged = entry.lastLive > 0 && event.seq > entry.lastLive + 1;
  entry.lastLive = event.seq;
  if (lagged) {
    entry.queue.push(event);
    sync(entry);
  } else if (apply(entry.model, event)) {
    notify(entry);
  }
}

function notify(entry) {
  for (const listener of listeners) listener(entry);
}
