THOUGHT = r'''The contract in README.md is far larger than the implementation: no concurrency validation or setter, priority ordering is a single unshift, `{signal, attempt}` is passed as `{}`, retries/retryDelay are ignored, a rejection never decrements `_running` and never calls `_next` (so slots leak and `onIdle` never settles), and a timeout leaves the job counted as running. I will rewrite the scheduler around one attempt loop that owns the slot, clears its timer, aborts its own signal, and either retries in place or releases the slot, then verify the rules the watched test misses (ordering, both hint forms, retries, cancellation, pause/idle) with a temporary script before finishing.'''
CONTENT = ""
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "src/queue.js",
            "content": r'''// The queue. README.md has the contract.

import { CancelledError, TaskTimeoutError } from './errors.js';

function checkedConcurrency(value) {
  if (!Number.isInteger(value) || value < 1) {
    throw new RangeError(`concurrency must be a positive integer, got ${value}`);
  }
  return value;
}

export class JobQueue {
  constructor({ concurrency = 1 } = {}) {
    this._concurrency = checkedConcurrency(concurrency);
    this._running = 0;
    this._waiting = [];
    this._paused = false;
    this._idle = [];
    this._draining = false;
    this._drainAgain = false;
  }

  get concurrency() {
    return this._concurrency;
  }

  set concurrency(value) {
    this._concurrency = checkedConcurrency(value);
    this._next();
  }

  get pending() {
    return this._running;
  }

  get size() {
    return this._waiting.length;
  }

  get isPaused() {
    return this._paused;
  }

  add(fn, { priority = 0, timeout = 0, retries = 0, retryDelay = 0 } = {}) {
    return new Promise((resolve, reject) => {
      const job = {
        fn,
        priority,
        timeout,
        retries,
        retryDelay,
        attempt: 0,
        resolve,
        reject,
      };
      // Highest priority first; equal priorities keep their arrival order.
      let index = this._waiting.findIndex((other) => other.priority < priority);
      if (index === -1) index = this._waiting.length;
      this._waiting.splice(index, 0, job);
      this._next();
    });
  }

  _next() {
    // Re-entrant calls (a job settling synchronously) only flag the outer loop,
    // so a long run of synchronous jobs cannot grow the stack.
    if (this._draining) {
      this._drainAgain = true;
      return;
    }
    this._draining = true;
    try {
      do {
        this._drainAgain = false;
        while (!this._paused && this._running < this._concurrency) {
          const job = this._waiting.shift();
          if (!job) break;
          this._start(job);
        }
      } while (this._drainAgain);
    } finally {
      this._draining = false;
    }
    this._settleIdle();
  }

  _start(job) {
    this._running += 1;
    this._attempt(job);
  }

  _attempt(job) {
    job.attempt += 1;
    const controller = new AbortController();
    let settled = false;
    let timer = null;

    const finish = (outcome) => {
      if (settled) return;
      settled = true;
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      this._endAttempt(job, outcome);
    };

    if (job.timeout > 0) {
      timer = setTimeout(() => {
        settled = true;
        timer = null;
        controller.abort();
        this._endAttempt(job, { ok: false, error: new TaskTimeoutError(job.timeout) });
      }, job.timeout);
    }

    let outcome;
    try {
      outcome = job.fn({ signal: controller.signal, attempt: job.attempt });
    } catch (error) {
      finish({ ok: false, error });
      return;
    }
    Promise.resolve(outcome).then(
      (value) => finish({ ok: true, value }),
      (error) => finish({ ok: false, error }),
    );
  }

  _endAttempt(job, outcome) {
    if (!outcome.ok && job.attempt <= job.retries) {
      // The job keeps its slot while it waits to be tried again.
      setTimeout(() => this._attempt(job), job.retryDelay > 0 ? job.retryDelay : 0);
      return;
    }
    this._running -= 1;
    if (outcome.ok) {
      job.resolve(outcome.value);
    } else {
      job.reject(outcome.error);
    }
    this._next();
  }

  _settleIdle() {
    if (this._running > 0 || this._waiting.length > 0) return;
    for (const resolve of this._idle.splice(0)) resolve();
  }

  pause() {
    this._paused = true;
  }

  resume() {
    this._paused = false;
    this._next();
  }

  clear() {
    const dropped = this._waiting;
    this._waiting = [];
    for (const job of dropped) job.reject(new CancelledError());
    this._settleIdle();
    return dropped.length;
  }

  onIdle() {
    if (this._running === 0 && this._waiting.length === 0) return Promise.resolve();
    return new Promise((resolve) => this._idle.push(resolve));
  }
}
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''cat > /tmp/check-jobqueue.mjs <<'CHECK'
import assert from 'node:assert/strict';

const { JobQueue, TaskTimeoutError, CancelledError } = await import(
  process.cwd() + '/src/index.js'
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// constructor and setter validation
assert.throws(() => new JobQueue({ concurrency: 0 }), RangeError);
assert.throws(() => new JobQueue({ concurrency: 2.5 }), RangeError);
{
  const q = new JobQueue({ concurrency: 3 });
  assert.equal(q.concurrency, 3);
  assert.throws(() => { q.concurrency = 0; }, RangeError);
  assert.equal(q.concurrency, 3);
}

// starts at once, sync throw becomes a rejection
{
  const q = new JobQueue();
  let started = false;
  const p = q.add(() => { started = true; return 7; });
  assert.equal(started, true);
  assert.equal(await p, 7);
  await assert.rejects(q.add(() => { throw new Error('sync'); }), /sync/);
  assert.equal(q.pending, 0);
}

// priority order, FIFO within a priority, negative priorities allowed
{
  const q = new JobQueue({ concurrency: 1 });
  const order = [];
  let release;
  const gate = new Promise((r) => { release = r; });
  const block = q.add(async () => { order.push('block'); await gate; });
  const low1 = q.add(() => { order.push('low1'); });
  const low2 = q.add(() => { order.push('low2'); });
  const high = q.add(() => { order.push('high1'); }, { priority: 5 });
  const high2 = q.add(() => { order.push('high2'); }, { priority: 5 });
  const neg = q.add(() => { order.push('neg'); }, { priority: -2 });
  assert.equal(q.size, 5);
  release();
  await Promise.all([block, low1, low2, high, high2, neg]);
  assert.deepEqual(order, ['block', 'high1', 'high2', 'low1', 'low2', 'neg']);
}

// raising concurrency starts waiting jobs, lowering lets running ones finish
{
  const q = new JobQueue({ concurrency: 1 });
  const gates = [];
  const started = [];
  const mk = (name) => () => {
    started.push(name);
    return new Promise((r) => gates.push(r));
  };
  const a = q.add(mk('a'));
  const b = q.add(mk('b'));
  const c = q.add(mk('c'));
  assert.deepEqual(started, ['a']);
  q.concurrency = 3;
  assert.deepEqual(started, ['a', 'b', 'c']);
  q.concurrency = 1;
  gates.forEach((r) => r());
  await Promise.all([a, b, c]);
  assert.equal(q.pending, 0);
}

// attempts are numbered from 1, each gets a fresh signal, retries reuse the slot
{
  const q = new JobQueue();
  const attempts = [];
  const signals = [];
  const stamps = [];
  const value = await q.add(({ signal, attempt }) => {
    attempts.push(attempt);
    signals.push(signal);
    stamps.push(Date.now());
    if (attempt < 3) throw new Error('nope ' + attempt);
    return 'done';
  }, { retries: 2, retryDelay: 30 });
  assert.equal(value, 'done');
  assert.deepEqual(attempts, [1, 2, 3]);
  assert.equal(new Set(signals).size, 3);
  assert.ok(signals.every((s) => s instanceof AbortSignal));
  assert.ok(stamps[1] - stamps[0] >= 25, `retryDelay was ${stamps[1] - stamps[0]}ms`);
  assert.ok(stamps[2] - stamps[1] >= 25, `retryDelay was ${stamps[2] - stamps[1]}ms`);
}

// promise settles with the last attempt only, even if that is a failure
{
  const q = new JobQueue();
  let seen = 0;
  await assert.rejects(
    q.add(({ attempt }) => { seen = attempt; throw new Error('fail ' + attempt); }, { retries: 3 }),
    /fail 4/,
  );
  assert.equal(seen, 4);
  await assert.rejects(q.add(() => { throw new Error('none'); }, { retries: 0 }), /none/);
}

// a retrying job still counts as pending and not waiting
{
  const q = new JobQueue();
  let firstFailed;
  const failed = new Promise((r) => { firstFailed = r; });
  const p = q.add(async ({ attempt }) => {
    if (attempt === 1) { firstFailed(); throw new Error('once'); }
    return 'ok';
  }, { retries: 1, retryDelay: 40 });
  await failed;
  await sleep(5);
  assert.equal(q.pending, 1);
  assert.equal(q.size, 0);
  assert.equal(await p, 'ok');
  assert.equal(q.pending, 0);
}

// timeout aborts the attempt, frees the slot at once, and does not wait for fn
{
  const q = new JobQueue({ concurrency: 1 });
  let aborted = false;
  const started = Date.now();
  await assert.rejects(
    q.add(({ signal }) => {
      signal.addEventListener('abort', () => { aborted = true; });
      return new Promise(() => {});
    }, { timeout: 30 }),
    (err) => err instanceof TaskTimeoutError && err.name === 'TaskTimeoutError',
  );
  assert.ok(Date.now() - started < 500);
  assert.equal(aborted, true);
  assert.equal(q.pending, 0);
  assert.equal(await q.add(() => 'next'), 'next');
}

// a timed-out attempt can be retried with a fresh signal
{
  const q = new JobQueue();
  const signals = [];
  const value = await q.add(({ signal, attempt }) => {
    signals.push(signal);
    if (attempt === 1) return sleep(200);
    return 'after timeout';
  }, { timeout: 30, retries: 1 });
  assert.equal(value, 'after timeout');
  assert.equal(signals.length, 2);
  assert.equal(signals[0].aborted, true);
  assert.equal(signals[1].aborted, false);
  assert.notEqual(signals[0], signals[1]);
}

// a job that settles in time leaves no timer behind
{
  const q = new JobQueue();
  await sleep(20);
  const timers = () => process.getActiveResourcesInfo().filter((r) => r === 'Timeout').length;
  const before = timers();
  await q.add(() => 'fast', { timeout: 60000 });
  assert.equal(timers(), before);
}

// clear drops only waiting jobs and rejects them with CancelledError
{
  const q = new JobQueue({ concurrency: 1 });
  let release;
  const gate = new Promise((r) => { release = r; });
  const running = q.add(async () => { await gate; return 'ran'; });
  const w1 = q.add(() => 'w1');
  const w2 = q.add(() => 'w2');
  assert.equal(q.size, 2);
  assert.equal(q.clear(), 2);
  assert.equal(q.size, 0);
  await assert.rejects(w1, (e) => e instanceof CancelledError && e.name === 'CancelledError');
  await assert.rejects(w2, (e) => e.name === 'CancelledError');
  release();
  assert.equal(await running, 'ran');
  assert.equal(q.clear(), 0);
}

// pause, resume, isPaused and onIdle
{
  const q = new JobQueue({ concurrency: 1 });
  q.pause();
  const ran = [];
  const p = q.add(() => { ran.push(1); return 'p'; });
  assert.deepEqual(ran, []);
  assert.equal(q.isPaused, true);
  let idle = false;
  const idleP = q.onIdle().then(() => { idle = true; });
  await sleep(10);
  assert.equal(idle, false);
  q.resume();
  assert.equal(q.isPaused, false);
  assert.equal(await p, 'p');
  await idleP;
  assert.equal(idle, true);
  assert.equal(await Promise.race([q.onIdle().then(() => 'idle'), sleep(100).then(() => 'timeout')]), 'idle');
}

// onIdle waits for a job inside its retry delay
{
  const q = new JobQueue();
  const p = q.add(({ attempt }) => {
    if (attempt < 2) throw new Error('again');
    return 'ok';
  }, { retries: 1, retryDelay: 30 });
  let idle = false;
  q.onIdle().then(() => { idle = true; });
  await sleep(10);
  assert.equal(idle, false);
  assert.equal(await p, 'ok');
  await q.onIdle();
  assert.equal(idle, true);
}

// a long run of synchronously settling jobs must not overflow the stack
{
  const q = new JobQueue({ concurrency: 4 });
  const jobs = [];
  for (let i = 0; i < 3000; i += 1) {
    jobs.push(q.add(() => {
      if (i % 2 === 1) throw new Error('odd ' + i);
      return i;
    }));
  }
  const settled = await Promise.allSettled(jobs);
  assert.equal(settled.filter((r) => r.status === 'fulfilled').length, 1500);
  assert.equal(settled.filter((r) => r.status === 'rejected').length, 1500);
  assert.equal(q.pending, 0);
  assert.equal(q.size, 0);
}

console.log('all contract checks passed');
CHECK
node /tmp/check-jobqueue.mjs; status=$?; rm -f /tmp/check-jobqueue.mjs; exit $status''',
            "expect_exit": "0",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Rewrote `src/queue.js` to meet the README contract: concurrency validation with a working setter, priority/FIFO ordering (including negative priorities), per-attempt `{signal, attempt}` from 1, timeout that aborts the signal and frees the slot immediately, in-place retries with `retryDelay`, slots released on every outcome, `clear()` rejecting with `CancelledError`, pause/resume, and `onIdle()` that also waits out retry delays. A temporary script checked the rules the watched test misses (ordering, setter, retries, timeout abort, idle, 3000 sync jobs) and passed; it was removed afterwards.",
            "unverified": "",
        },
    },
]
