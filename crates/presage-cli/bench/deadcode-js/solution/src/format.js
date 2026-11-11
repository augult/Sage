'use strict';

const { hhmm } = require('./time');
const { topProjects } = require('./summary');

function rows(summary) {
  return topProjects(summary, Infinity);
}

function toText(summary) {
  const lines = rows(summary).map(([name, minutes]) => `${name.padEnd(12)}${hhmm(minutes).padStart(6)}`);
  lines.push(`${'total'.padEnd(12)}${hhmm(summary.total).padStart(6)}`);
  lines.push(`${summary.days} days`);
  return lines.join('\n') + '\n';
}

function toJson(summary) {
  return JSON.stringify(summary, null, 2) + '\n';
}

function toCsv(summary) {
  const lines = ['project,minutes'];
  for (const [name, minutes] of rows(summary)) {
    lines.push(`${name},${minutes}`);
  }
  return lines.join('\n') + '\n';
}

const formatters = { text: toText, json: toJson, csv: toCsv };

function render(summary, format) {
  const formatter = formatters[format];
  if (!formatter) {
    throw new Error(`unknown format: ${format}`);
  }
  return formatter(summary);
}

module.exports = { render, formats: Object.keys(formatters) };
