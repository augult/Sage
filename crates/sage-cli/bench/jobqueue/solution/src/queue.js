// The queue. README.md has the contract.

import { CancelledError, TaskTimeoutError } from './errors.js';

function positiveInteger(value, name) {
  if (!Number.isInteger(value) || value < 1) throw new RangeError(`${name} must be a positive integer`);
  return value;
}

export class JobQueue {
  constructor({ concurrency = 1 } = {}) {
    this._concurrency = positiveInteger(concurrency, 'concurrency');
    this._running = 0;
    this._waiting = [];
    this._paused = false;
    this._idle = [];
  }

  get concurrency() {
    return this._concurrency;
  }

  set concurrency(value) {
    this._concurrency = positiveInteger(value, 'concurrency');
    this._fill();
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
      // Behind every job of the same or a higher priority.
      let at = this._waiting.length;
      while (at > 0 && this._waiting[at - 1].priority < priority) at -= 1;
      this._waiting.splice(at, 0, job);
      this._fill();
    });
  }

  _fill() {
    while (!this._paused && this._running < this._concurrency && this._waiting.length > 0) {
      const job = this._waiting.shift();
      this._running += 1;
      this._run(job);
    }
    if (this._running === 0 && this._waiting.length === 0) {
      for (const resolve of this._idle.splice(0)) resolve();
    }
  }

  // One attempt: settles with the job's outcome, or with a timeout, whichever is first.
  _attempt(job, attempt) {
    return new Promise((resolve, reject) => {
      const controller = new AbortController();
      let timer;
      if (job.timeout > 0) {
        timer = setTimeout(() => {
          reject(new TaskTimeoutError(job.timeout));
          controller.abort();
        }, job.timeout);
      }
      const settle = (how) => (value) => {
        clearTimeout(timer);
        how(value);
      };
      try {
        Promise.resolve(job.fn({ signal: controller.signal, attempt })).then(settle(resolve), settle(reject));
      } catch (error) {
        settle(reject)(error);
      }
    });
  }

  async _run(job) {
    try {
      for (let attempt = 1; ; attempt += 1) {
        try {
          job.resolve(await this._attempt(job, attempt));
          return;
        } catch (error) {
          if (attempt > job.retries) {
            job.reject(error);
            return;
          }
          if (job.retryDelay > 0) await new Promise((resolve) => setTimeout(resolve, job.retryDelay));
        }
      }
    } finally {
      this._running -= 1;
      this._fill();
    }
  }

  pause() {
    this._paused = true;
  }

  resume() {
    this._paused = false;
    this._fill();
  }

  clear() {
    const dropped = this._waiting.splice(0);
    for (const job of dropped) job.reject(new CancelledError());
    this._fill();
    return dropped.length;
  }

  onIdle() {
    if (this._running === 0 && this._waiting.length === 0) return Promise.resolve();
    return new Promise((resolve) => this._idle.push(resolve));
  }
}
