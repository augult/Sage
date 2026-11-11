'use strict';

const { span } = require('./time');

const LINE = /^(\d{4}-\d{2}-\d{2})\s+(\S+)\s+(\S+)(?:\s+(.*))?$/;

// One log line -> { date, minutes, project, note }, or null for a blank line or a comment.
function parseLine(line) {
  const text = line.trim();
  if (text === '' || text.startsWith('#')) {
    return null;
  }
  const match = LINE.exec(text);
  if (!match) {
    throw new Error(`bad line: ${text}`);
  }
  return { date: match[1], minutes: span(match[2]), project: match[3], note: match[4] || '' };
}

function parseLog(text) {
  return text.split(/\r?\n/).map(parseLine).filter(Boolean);
}

// "1h30m", "45m", "2h" -> minutes
function parseDuration(text) {
  const match = /^(?:(\d+)h)?(?:(\d+)m)?$/.exec(text.trim());
  if (!match || (match[1] === undefined && match[2] === undefined)) {
    throw new Error(`bad duration: ${text}`);
  }
  return Number(match[1] || 0) * 60 + Number(match[2] || 0);
}

module.exports = { parseLine, parseLog, parseDuration };
