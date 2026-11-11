// The queue. README.md has the contract.

import { TaskTimeoutError } from './errors.js';

export class JobQueue {
  constructor({ concurrency = 1 } = {}) {
    this._concurrency = concurrency;
    this._running = 0;
    this._waiting = [];
    this._paused = false;
    this._idle = [];
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

  add(fn, { priority = 0, timeout = 0 } = {}) {
    return new Promise((resolve, reject) => {
      const job = { fn, priority, timeout, resolve, reject };
      if (priority > 0) {
        this._waiting.unshift(job);
      } else {
        this._waiting.push(job);
      }
      this._next();
    });
  }

  _next() {
    if (this._paused || this._running >= this._concurrency) return;
    const job = this._waiting.shift();
    if (!job) {
      for (const resolve of this._idle.splice(0)) resolve();
      return;
    }
    this._running += 1;
    let outcome = job.fn({});
    if (job.timeout > 0) {
      const timer = new Promise((_, reject) => {
        setTimeout(() => reject(new TaskTimeoutError(job.timeout)), job.timeout);
      });
      outcome = Promise.race([outcome, timer]);
    }
    Promise.resolve(outcome)
      .then((value) => {
        this._running -= 1;
        job.resolve(value);
        this._next();
      })
      .catch(job.reject);
  }

  pause() {
    this._paused = true;
  }

  resume() {
    this._paused = false;
    this._next();
  }

  clear() {
    const dropped = this._waiting.length;
    this._waiting = [];
    return dropped;
  }

  onIdle() {
    if (this._waiting.length === 0) return Promise.resolve();
    return new Promise((resolve) => this._idle.push(resolve));
  }
}
