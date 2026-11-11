'use strict';

// "09:30" -> 570
function minutes(clock) {
  const match = /^(\d{1,2}):(\d{2})$/.exec(clock);
  if (!match || Number(match[1]) > 23 || Number(match[2]) > 59) {
    throw new Error(`bad time: ${clock}`);
  }
  return Number(match[1]) * 60 + Number(match[2]);
}

// "09:00-10:30" -> 90
function span(range) {
  const [start, end] = range.split('-');
  const length = minutes(end || '') - minutes(start);
  if (length <= 0) {
    throw new Error(`bad range: ${range}`);
  }
  return length;
}

// 95 -> "1:35"
function hhmm(total) {
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

// Rounds up to a multiple of `step` minutes.
function roundTo(total, step) {
  return step > 0 ? Math.ceil(total / step) * step : total;
}

module.exports = { minutes, span, hhmm, roundTo };
