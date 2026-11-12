import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { test } from 'node:test';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

import { CancelledError, JobQueue, TaskTimeoutError } from '../src/index.js';

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

function gate() {
  let open;
  const promise = new Promise((resolve) => {
    open = resolve;
  });
  return { promise, open };
}

// Fails the test instead of hanging when the queue never settles something.
function within(promise, ms = 3_000, what = 'the promise') {
  let timer;
  const limit = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} did not settle within ${ms} ms`)), ms);
  });
  return Promise.race([promise, limit]).finally(() => clearTimeout(timer));
}

const never = () => new Promise(() => {});

test('concurrency is validated, at construction and when assigned', () => {
  for (const concurrency of [0, -2, 1.5, '2', NaN, Infinity]) {
    assert.throws(() => new JobQueue({ concurrency }), RangeError, `concurrency ${concurrency}`);
  }
  const queue = new JobQueue({ concurrency: 2 });
  assert.throws(() => {
    queue.concurrency = 0;
  }, RangeError);
  assert.equal(queue.concurrency, 2);
  assert.equal(new JobQueue().concurrency, 1);
});

test('1: a job starts before add returns when a slot is free', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  const started = [];
  const first = gate();
  queue.add(() => (started.push('a'), first.promise));
  assert.deepEqual(started, ['a']);
  const b = queue.add(() => (started.push('b'), 'plain value'));
  assert.deepEqual(started, ['a', 'b']);
  assert.equal(await within(b), 'plain value');
  first.open();
  await within(queue.onIdle());
});

test('2: priority first, then the order of adding', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  const started = [];
  const first = gate();
  const jobs = [queue.add(() => first.promise)];
  for (const [name, priority] of [['a', 0], ['b', 5], ['c', 0], ['d', 10], ['e', 5], ['f', -1], ['g', 10]]) {
    jobs.push(queue.add(async () => started.push(name), { priority }));
  }
  jobs.push(queue.add(async () => started.push('h')));
  assert.equal(queue.size, 8);
  first.open();
  await within(Promise.all(jobs));
  assert.deepEqual(started, ['d', 'g', 'b', 'e', 'a', 'c', 'h', 'f']);
});

test('3: a synchronous throw is a rejection and frees the slot', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  let failed;
  assert.doesNotThrow(() => {
    failed = queue.add(() => {
      throw new Error('sync boom');
    });
  });
  const next = queue.add(() => 'next');
  await assert.rejects(within(failed), /sync boom/);
  assert.equal(await within(next), 'next');
  await within(queue.onIdle());
  assert.equal(queue.pending, 0);
});

test('3: many failures do not use up the slots', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  const outcomes = await within(
    Promise.allSettled(
      Array.from({ length: 12 }, (_, i) =>
        queue.add(async () => {
          await sleep(1);
          if (i % 3 !== 2) throw new Error(`job ${i}`);
          return i;
        }),
      ),
    ),
  );
  assert.deepEqual(
    outcomes.map((o) => (o.status === 'fulfilled' ? o.value : o.reason.message)),
    ['job 0', 'job 1', 2, 'job 3', 'job 4', 5, 'job 6', 'job 7', 8, 'job 9', 'job 10', 11],
  );
  await within(queue.onIdle());
  assert.equal(queue.pending, 0);
  assert.equal(queue.size, 0);
});

test('4: fn receives a signal and the number of the attempt', async () => {
  const queue = new JobQueue();
  const seen = await within(queue.add((context) => context));
  assert.ok(seen.signal instanceof AbortSignal);
  assert.equal(seen.signal.aborted, false);
  assert.equal(seen.attempt, 1);
});

test('5: a timeout ends the attempt even if fn never settles', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  let signal;
  const stuck = queue.add(
    (context) => {
      signal = context.signal;
      return never();
    },
    { timeout: 30 },
  );
  const next = queue.add(() => 'next');
  await assert.rejects(within(stuck), (error) => error instanceof TaskTimeoutError && error.name === 'TaskTimeoutError');
  assert.equal(signal.aborted, true);
  assert.equal(await within(next, 3_000, 'the job behind the timed-out one'), 'next');
  await within(queue.onIdle());
});

test('5: a job that settles in time is not timed out, and a late failure is ignored', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  assert.equal(await within(queue.add(async () => (await sleep(5), 'fast'), { timeout: 2_000 })), 'fast');
  const late = queue.add(
    async () => {
      await sleep(60);
      throw new Error('late');
    },
    { timeout: 15 },
  );
  await assert.rejects(within(late), TaskTimeoutError);
  await sleep(90);
  assert.equal(await within(queue.add(() => 'fine')), 'fine');
});

test('5: a job that settles in time leaves no timer behind', () => {
  const script = `
    import { JobQueue } from './src/index.js';
    const queue = new JobQueue({ concurrency: 2 });
    const results = await Promise.allSettled([
      queue.add(async () => 'done', { timeout: 600000 }),
      queue.add(async () => { throw new Error('no') }, { timeout: 600000, retries: 1 }),
    ]);
    console.log(results.map((r) => r.status).join(','));
  `;
  const run = spawnSync(process.execPath, ['--input-type=module', '-e', script], { cwd: ROOT, encoding: 'utf8', timeout: 8_000 });
  assert.equal(run.error, undefined, 'the process is kept alive by a timer');
  assert.equal(run.status, 0, run.stderr);
  assert.equal(run.stdout.trim(), 'fulfilled,rejected');
});

test('6: retries until it works, with the attempt number and a fresh signal', async () => {
  const queue = new JobQueue();
  const attempts = [];
  const signals = new Set();
  const result = await within(
    queue.add(
      async ({ attempt, signal }) => {
        attempts.push(attempt);
        signals.add(signal);
        if (attempt < 3) throw new Error(`fail ${attempt}`);
        return 'third time';
      },
      { retries: 2 },
    ),
  );
  assert.equal(result, 'third time');
  assert.deepEqual(attempts, [1, 2, 3]);
  assert.equal(signals.size, 3);
});

test('6: rejects with the error of the last attempt', async () => {
  const queue = new JobQueue();
  let calls = 0;
  const job = queue.add(
    () => {
      calls += 1;
      throw new Error(`fail ${calls}`);
    },
    { retries: 2 },
  );
  await assert.rejects(within(job), /fail 3/);
  assert.equal(calls, 3);
  calls = 0;
  await assert.rejects(within(queue.add(async () => Promise.reject(new Error(`once ${++calls}`)))), /once 1/);
  assert.equal(calls, 1);
});

test('6: a retry keeps its slot and waits retryDelay', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  const events = [];
  let failedAt;
  let retriedAt;
  const flaky = queue.add(
    async ({ attempt }) => {
      events.push(`flaky ${attempt}`);
      if (attempt === 1) {
        failedAt = Date.now();
        throw new Error('first');
      }
      retriedAt = Date.now();
      return 'ok';
    },
    { retries: 1, retryDelay: 40 },
  );
  const other = queue.add(async () => events.push('other'));
  await sleep(15);
  assert.equal(queue.pending, 1, 'a job waiting for its retry is running');
  assert.equal(queue.size, 1);
  assert.equal(await within(flaky), 'ok');
  await within(other);
  assert.deepEqual(events, ['flaky 1', 'flaky 2', 'other']);
  assert.ok(retriedAt - failedAt >= 35, `retried after ${retriedAt - failedAt} ms`);
});

test('6: a timed-out attempt is retried too', async () => {
  const queue = new JobQueue();
  const signals = [];
  const result = await within(
    queue.add(
      ({ attempt, signal }) => {
        signals.push(signal);
        return attempt === 1 ? never() : 'second';
      },
      { timeout: 20, retries: 1 },
    ),
  );
  assert.equal(result, 'second');
  assert.deepEqual(signals.map((signal) => signal.aborted), [true, false]);
  const always = queue.add(never, { timeout: 10, retries: 2 });
  await assert.rejects(within(always), TaskTimeoutError);
});

test('pending and size', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  const hold = gate();
  const inside = [];
  const jobs = Array.from({ length: 5 }, () =>
    queue.add(async () => {
      inside.push([queue.pending, queue.size]);
      await hold.promise;
    }),
  );
  assert.equal(queue.pending, 2);
  assert.equal(queue.size, 3);
  assert.deepEqual(inside, [[1, 0], [2, 0]]);
  hold.open();
  await within(Promise.all(jobs));
  await within(queue.onIdle());
  assert.deepEqual([queue.pending, queue.size], [0, 0]);
});

test('pause and resume', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  const started = [];
  const hold = gate();
  const running = queue.add(() => (started.push('running'), hold.promise));
  queue.pause();
  assert.equal(queue.isPaused, true);
  const jobs = ['a', 'b', 'c'].map((name) => queue.add(async () => started.push(name)));
  assert.deepEqual(started, ['running']);
  assert.equal(queue.size, 3);
  hold.open();
  await within(running);
  await sleep(10);
  assert.deepEqual(started, ['running'], 'nothing starts while the queue is paused');
  queue.resume();
  assert.equal(queue.isPaused, false);
  assert.deepEqual(started, ['running', 'a', 'b'], 'resume fills every free slot');
  await within(Promise.all(jobs));
  assert.deepEqual(started, ['running', 'a', 'b', 'c']);
});

test('clear rejects the waiting jobs and leaves the running one alone', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  const hold = gate();
  const running = queue.add(() => hold.promise.then(() => 'finished'));
  const waiting = [1, 2, 3].map((n) => queue.add(async () => n));
  const outcomes = Promise.allSettled(waiting);
  assert.equal(queue.clear(), 3);
  assert.equal(queue.size, 0);
  assert.equal(queue.clear(), 0);
  for (const outcome of await within(outcomes, 3_000, 'the cleared jobs')) {
    assert.equal(outcome.status, 'rejected');
    assert.ok(outcome.reason instanceof CancelledError);
  }
  hold.open();
  assert.equal(await within(running), 'finished');
  assert.equal(await within(queue.add(() => 'after')), 'after');
});

test('onIdle waits for the running jobs', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  await within(queue.onIdle(), 1_000, 'onIdle of an idle queue');
  const hold = gate();
  const job = queue.add(() => hold.promise);
  let idle = 0;
  queue.onIdle().then(() => (idle += 1));
  queue.onIdle().then(() => (idle += 1));
  await sleep(15);
  assert.equal(idle, 0, 'a job is still running');
  hold.open();
  await within(job);
  await sleep(5);
  assert.equal(idle, 2);
});

test('a paused queue with waiting jobs is not idle', async () => {
  const queue = new JobQueue();
  queue.pause();
  const job = queue.add(async () => 'done');
  let idle = false;
  const waiting = queue.onIdle().then(() => (idle = true));
  await sleep(15);
  assert.equal(idle, false);
  queue.resume();
  assert.equal(await within(job), 'done');
  await within(waiting, 3_000, 'onIdle after resume');
});

test('onIdle fulfils after a failure, a timeout and a clear', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  const outcomes = Promise.allSettled([
    queue.add(() => Promise.reject(new Error('no'))),
    queue.add(never, { timeout: 10 }),
    queue.add(async () => 'ok'),
  ]);
  await within(queue.onIdle(), 3_000, 'onIdle');
  assert.deepEqual((await outcomes).map((o) => o.status), ['rejected', 'rejected', 'fulfilled']);

  const hold = gate();
  const running = queue.add(() => hold.promise);
  queue.add(async () => 1).catch(() => {});
  const idle = queue.onIdle();
  queue.clear();
  hold.open();
  await within(running);
  await within(idle, 3_000, 'onIdle after clear');
});

test('concurrency can be changed while jobs wait', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  let started = 0;
  let running = 0;
  let most = 0;
  const holds = Array.from({ length: 6 }, gate);
  const jobs = holds.map((hold) =>
    queue.add(async () => {
      started += 1;
      running += 1;
      most = Math.max(most, running);
      await hold.promise;
      running -= 1;
    }),
  );
  assert.equal(started, 1);
  queue.concurrency = 3;
  assert.equal(started, 3, 'raising the limit starts waiting jobs at once');
  queue.concurrency = 1;
  holds[0].open();
  await within(jobs[0]);
  await sleep(5);
  assert.equal(started, 3, 'two jobs are still running, the limit is 1');
  holds[1].open();
  holds[2].open();
  await within(Promise.all([jobs[1], jobs[2]]));
  await sleep(5);
  assert.equal(started, 4);
  holds.forEach((hold) => hold.open());
  await within(Promise.all(jobs));
  assert.equal(most, 3);
});

test('a mixed load never exceeds the limit and always ends', async () => {
  const queue = new JobQueue({ concurrency: 5 });
  let running = 0;
  let most = 0;
  const jobs = Array.from({ length: 150 }, (_, i) =>
    queue.add(
      async ({ attempt }) => {
        running += 1;
        most = Math.max(most, running);
        try {
          await sleep(i % 4);
          if (i % 7 === 0 && attempt === 1) throw new Error('flaky');
          if (i % 11 === 0) throw new Error('broken');
          return i;
        } finally {
          running -= 1;
        }
      },
      { priority: i % 3, retries: 1 },
    ),
  );
  const outcomes = await within(Promise.allSettled(jobs), 10_000, 'the load');
  await within(queue.onIdle());
  assert.equal(most, 5);
  outcomes.forEach((outcome, i) => {
    assert.equal(outcome.status, i % 11 === 0 ? 'rejected' : 'fulfilled', `job ${i}`);
  });
});
