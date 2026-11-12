'use strict';

// Reads the semicolon-separated export of the old desktop timer: DATE;HOURS;PROJECT

const fs = require('fs');

function importLegacy(file) {
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      const [date, hours, project] = line.split(';');
      return { date, minutes: Math.round(Number(hours) * 60), project, note: '' };
    });
}

module.exports = { importLegacy };
