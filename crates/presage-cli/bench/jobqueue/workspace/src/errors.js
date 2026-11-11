export class TaskTimeoutError extends Error {
  constructor(ms) {
    super(`the job did not finish within ${ms} ms`);
    this.name = 'TaskTimeoutError';
  }
}

export class CancelledError extends Error {
  constructor() {
    super('the job was removed from the queue before it started');
    this.name = 'CancelledError';
  }
}
