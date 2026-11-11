'use strict';

function onStart(context) {
  if (context.verbose) {
    process.stderr.write(`tally: ${context.command} ${context.file}\n`);
  }
}

function onFinish(context) {
  if (context.verbose) {
    process.stderr.write(`tally: ${context.entries} entries\n`);
  }
}

function onError(context, error) {
  process.stderr.write(`tally: ${error.message}\n`);
}

module.exports = { onStart, onFinish, onError };
