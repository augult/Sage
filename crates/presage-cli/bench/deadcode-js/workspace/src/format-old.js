'use strict';

// The fixed-width report of tally 1.

function pad(text, width) {
  return String(text) + ' '.repeat(Math.max(0, width - String(text).length));
}

function formatOld(summary) {
  return Object.keys(summary.byProject)
    .map((name) => pad(name, 20) + (summary.byProject[name] / 60).toFixed(2))
    .join('\n');
}

module.exports = { formatOld };
