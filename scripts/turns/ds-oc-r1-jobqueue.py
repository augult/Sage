THOUGHT = r'''I'll rewrite queue.js to the full contract, then run the provided tests plus a temporary script exercising ordering, retries, timeouts, cancellation, pause/resume, dynamic concurrency, and onIdle.'''

CONTENT = r'''Now I'll rewrite the queue to the whole contract.'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "src/queue.js", "content": r'''// The queue. README.md has the contract.

import { CancelledError, TaskTimeoutError } from './errors.js';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function checkConcurrency(value) {
  if (!Number.isInteger(value) || value < 1) {
    throw new RangeError('concurrency must be a positive integer');
  }
  return value;
}

export class JobQueue {
  constructor({ concurrency = 1 } = {}) {
    this._concurrency = checkConcurrency(concurrency);
    this._running = 0;
    this._waiting = [];
    this._paused = false;
    this._idle = [];
  }

  get concurrency() {
    return this._concurrency;
  }

  set concurrency(value) {
    this._concurrency = checkConcurrency(value);
    this._start();
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
      const job = { fn, priority, timeout, retries, retryDelay, resolve, reject };
      // Keep the list ordered by descending priority, FIFO within a priority.
      const index = this._waiting.findIndex((waiting) => waiting.priority < priority);
      if (index === -1) this._waiting.push(job);
      else this._waiting.splice(index, 0, job);
      this._start();
    });
  }

  _start() {
    if (this._paused) return;
    while (this._running < this._concurrency && this._waiting.length > 0) {
      const job = this._waiting.shift();
      this._running += 1;
      this._run(job);
    }
    this._checkIdle();
  }

  _run(job) {
    const release = () => {
      this._running -= 1;
      this._start();
    };
    this._attempt(job).then(release, release);
  }

  async _attempt(job) {
    for (let attempt = 1; ; attempt += 1) {
      const controller = new AbortController();
      let timer = null;
      try {
        let call;
        try {
          call = job.fn({ signal: controller.signal, attempt });
        } catch (error) {
          call = Promise.reject(error);
        }

        let outcome;
        if (job.timeout > 0) {
          const expired = new Promise((_, reject) => {
            timer = setTimeout(() => reject(new TaskTimeoutError(job.timeout)), job.timeout);
          });
          outcome = await Promise.race([call, expired]);
        } else {
          outcome = await call;
        }

        if (timer) clearTimeout(timer);
        job.resolve(outcome);
        return;
      } catch (error) {
        if (timer) clearTimeout(timer);
        controller.abort(error);
        if (attempt <= job.retries) {
          if (job.retryDelay > 0) await sleep(job.retryDelay);
          continue; // keeps its slot; it is not requeued
        }
        job.reject(error);
        return;
      }
    }
  }

  pause() {
    this._paused = true;
  }

  resume() {
    this._paused = false;
    this._start();
  }

  clear() {
    const dropped = this._waiting.splice(0);
    for (const job of dropped) job.reject(new CancelledError());
    this._checkIdle();
    return dropped.length;
  }

  onIdle() {
    if (this._running === 0 && this._waiting.length === 0) return Promise.resolve();
    return new Promise((resolve) => this._idle.push(resolve));
  }

  _checkIdle() {
    if (this._running === 0 && this._waiting.length === 0) {
      for (const resolve of this._idle.splice(0)) resolve();
    }
  }
}
'''}},
    {"name": "bash", "arguments": {"command": "cat > verify.tmp.mjs <<'EOF'\nimport assert from 'node:assert/strict';\nimport { JobQueue, CancelledError, TaskTimeoutError } from './src/index.js';\nimport { setTimeout as sleep } from 'node:timers/promises';\n\n// concurrency validation\nassert.throws(() => new JobQueue({ concurrency: 0 }), RangeError);\nassert.throws(() => new JobQueue({ concurrency: 1.5 }), RangeError);\nconst q0 = new JobQueue();\nassert.equal(q0.concurrency, 1);\nassert.throws(() => { q0.concurrency = 0; }, RangeError);\nassert.equal(q0.concurrency, 1);\nq0.concurrency = 3;\nassert.equal(q0.concurrency, 3);\n\n// synchronous start when a slot is free\nconst q1 = new JobQueue({ concurrency: 1 });\nlet started = false;\nconst p1 = q1.add(() => { started = true; return 'a'; });\nassert.equal(started, true, 'fn must be called before add returns');\nassert.equal(await p1, 'a');\nassert.equal(q1.pending, 0);\n\n// ordering by priority then FIFO, and slot always returns after rejections\nconst q2 = new JobQueue({ concurrency: 1 });\nconst order = [];\nconst gate = sleep(20);\nconst first = q2.add(async () => { await gate; order.push('first'); });\nconst low = q2.add(() => order.push('low'));\nconst highA = q2.add(() => order.push('highA'), { priority: 5 });\nconst highB = q2.add(() => order.push('highB'), { priority: 5 });\nconst thrown = q2.add(() => { throw new Error('sync boom'); });\nawait assert.rejects(thrown, /sync boom/);\nawait Promise.all([first, low, highA, highB]);\nassert.deepEqual(order, ['first', 'highA', 'highB', 'low']);\n\n// { signal, attempt } and retries; last error only\nconst q3 = new JobQueue({ concurrency: 1 });\nconst seen = [];\nconst failures = [];\nconst p3 = q3.add(({ signal, attempt }) => {\n  seen.push(attempt);\n  assert.ok(signal instanceof AbortSignal);\n  throw new Error('fail ' + attempt);\n}, { retries: 2 });\nawait assert.rejects(p3, /fail 3/);\nassert.deepEqual(seen, [1, 2, 3]);\nassert.equal(q3.pending, 0);\n\n// retry delay and retrying job holds its slot, onIdle waits\nt0 = Date.now();\nconst p4 = q3.add(() => 'ok', { retries: 0 });\nassert.equal(await p4, 'ok');\nconst q4 = new JobQueue({ concurrency: 1 });\nconst starts = [];\nconst p5 = q4.add(({ attempt }) => { starts.push(Date.now()); if (attempt < 2) throw new Error('x'); return 'done'; }, { retries: 2, retryDelay: 60 });\nassert.equal(q4.pending, 1);\nawait q4.onIdle();\nassert.equal(await p5, 'done');\nassert.equal(starts.length, 2);\nassert.ok(starts[1] - starts[0] >= 50, 'retry delay respected');\n\n// timeout aborts the signal and frees the slot without waiting\nconst q5 = new JobQueue({ concurrency: 1 });\nlet aborted = false;\nlet sawSignal;\nconst p6 = q5.add(({ signal }) => { sawSignal = signal; signal.addEventListener('abort', () => { aborted = true; }); return sleep(500); }, { timeout: 20 });\nconst next = q5.add(() => 'recovered');\nawait assert.rejects(p6, TaskTimeoutError);\nassert.equal(aborted, true);\nassert.equal(await next, 'recovered');\n\n// clear rejects waiting jobs, keeps running ones\nconst q6 = new JobQueue({ concurrency: 1 });\nconst running = q6.add(() => sleep(30).then(() => 'ran'));\nconst a = q6.add(() => 'a');\nconst b = q6.add(() => 'b');\nassert.equal(q6.clear(), 2);\nawait assert.rejects(a, CancelledError);\nawait assert.rejects(b, CancelledError);\nassert.equal(await running, 'ran');\nassert.equal(q6.pending, 0);\n\n// pause / resume / size / isPaused\nconst q7 = new JobQueue({ concurrency: 1 });\nq7.pause();\nassert.equal(q7.isPaused, true);\nlet ran = 0;\nconst p7 = q7.add(() => { ran += 1; });\nconst p8 = q7.add(() => { ran += 1; });\nassert.equal(ran, 0);\nassert.equal(q7.size, 2);\nlet idle = false;\nq7.onIdle().then(() => { idle = true; });\nawait sleep(10);\nassert.equal(idle, false);\nq7.resume();\nawait Promise.all([p7, p8]);\nassert.equal(ran, 2);\n\n// raising concurrency starts waiting jobs\nconst q8 = new JobQueue({ concurrency: 1 });\nlet active = 0;\nlet peak = 0;\nconst job = async () => { active += 1; peak = Math.max(peak, active); await sleep(10); active -= 1; };\nconst all = [1, 2, 3, 4].map(() => q8.add(job));\nawait sleep(5);\nassert.equal(peak, 1);\nq8.concurrency = 4;\nawait Promise.all(all);\nassert.equal(peak, 4);\n\nconsole.log('all contract checks passed');\nEOF\nnode verify.tmp.mjs && npm test; status=$?; rm -f verify.tmp.mjs; exit $status", "timeout": 120, "workdir": "."}},
]
