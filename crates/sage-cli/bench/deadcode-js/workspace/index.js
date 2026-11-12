'use strict';

const { parseLog } = require('./src/parse');
const { summarize } = require('./src/summary');
const { render } = require('./src/format');
const { roundTo } = require('./src/time');

module.exports = { parseLog, summarize, render, roundTo };
