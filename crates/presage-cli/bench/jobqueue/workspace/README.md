# job-queue

Runs async jobs with a limit on how many run at once. Used by the importer to talk to the
partner API without tripping its rate limiter. No dependencies; Node 20 or later.

```js
import { JobQueue } from './src/index.js';

const queue = new JobQueue({ concurrency: 4 });

const invoice = await queue.add(() => fetchInvoice(id));
const report = await queue.add(({ signal, attempt }) => buildReport(id, { signal }), {
  priority: 10,
  timeout: 30_000,
  retries: 2,
});

await queue.onIdle();
```

    npm test        # node --test, with a time limit per test

## Contract

### `new JobQueue({ concurrency = 1 })`

`concurrency` is a positive integer, otherwise `RangeError`. `queue.concurrency` reads it, and
assigning to it changes it later (same validation; a rejected value changes nothing): raising it
starts waiting jobs at once, lowering it lets the running jobs finish.

### `queue.add(fn, { priority = 0, timeout = 0, retries = 0, retryDelay = 0 } = {})`

Queues the job `fn` and returns a promise for its outcome: it fulfils with what `fn` returned
(or the promise it returned fulfilled with) and rejects with what it threw (or rejected with).
`add` itself never throws for a failing job: a `fn` that throws synchronously is a rejection like
any other.

1. **Limit.** Never more than `concurrency` jobs are running. A job starts as soon as there is a
   free slot: if there is one when `add` is called, `fn` is called before `add` returns.
2. **Order.** Waiting jobs start by priority, highest first; jobs of the same priority start in
   the order they were added.
3. **Slots always come back.** However a job ends (fulfilled, rejected, thrown, timed out), its
   slot is free again and the next waiting job starts. One job's failure never affects another.
4. **`fn` receives `{ signal, attempt }`**: an `AbortSignal` of this attempt, and the number of the
   attempt, starting at 1.
5. **Timeout.** With `timeout` greater than 0, an attempt that has not settled after that many
   milliseconds fails with a `TaskTimeoutError`. The attempt's `signal` is aborted, and the queue
   does not wait for `fn` any longer: the attempt is over at that moment, whatever `fn` does
   afterwards. An attempt that settles in time leaves no timer behind.
6. **Retries.** With `retries: n`, a job whose attempt fails (an error or a timeout) is tried
   again, up to `n` more times. It keeps its slot in between: the next attempt starts
   `retryDelay` milliseconds after the failure, without going back into the queue, and gets a
   fresh `signal`. The promise that `add` returned settles with the outcome of the last attempt
   only.

### The rest

- `queue.pending` is the number of running jobs (a job waiting for its retry is running);
  `queue.size` the number of waiting ones.
- `queue.pause()` stops the queue from starting jobs; the running ones carry on. `add` still
  accepts jobs. `queue.resume()` starts as many waiting jobs as there are free slots.
  `queue.isPaused` tells which state it is in.
- `queue.clear()` drops all waiting jobs, rejecting each one's promise with a `CancelledError`, and
  returns how many there were. Running jobs are not affected.
- `queue.onIdle()` returns a promise that fulfils once no job is running and none is waiting.
  If that is already so, it fulfils right away. A paused queue with waiting jobs is not idle.

`TaskTimeoutError` and `CancelledError` are exported from `src/index.js`.
