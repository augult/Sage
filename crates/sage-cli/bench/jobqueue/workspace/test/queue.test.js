import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as sleep } from 'node:timers/promises';

import { JobQueue } from '../src/index.js';

test('runs jobs and returns their results', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  const results = await Promise.all([1, 2, 3, 4].map((n) => queue.add(async () => n * 10)));
  assert.deepEqual(results, [10, 20, 30, 40]);
});

test('never runs more than `concurrency` jobs at once', async () => {
  const queue = new JobQueue({ concurrency: 2 });
  let running = 0;
  let most = 0;
  const job = async () => {
    running += 1;
    most = Math.max(most, running);
    await sleep(5);
    running -= 1;
  };
  await Promise.all(Array.from({ length: 8 }, () => queue.add(job)));
  assert.equal(most, 2);
});

test('a failing job gives its slot back', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  const failed = queue.add(async () => {
    throw new Error('boom');
  });
  const next = queue.add(async () => 'still running');
  await assert.rejects(failed, /boom/);
  assert.equal(await next, 'still running');
});

test('a job that takes too long is timed out', async () => {
  const queue = new JobQueue({ concurrency: 1 });
  await assert.rejects(queue.add(() => sleep(200), { timeout: 20 }), { name: 'TaskTimeoutError' });
});
