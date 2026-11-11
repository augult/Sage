'use strict';

const { roundTo } = require('../src/time');

// Rounds every entry up to the `roundTo` setting, in minutes.
function transform(entries, config) {
  const step = Number(config.roundTo) || 0;
  return entries.map((entry) => ({ ...entry, minutes: roundTo(entry.minutes, step) }));
}

module.exports = { transform };
