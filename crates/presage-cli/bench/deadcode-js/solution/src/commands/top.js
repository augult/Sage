'use strict';

const { summarize, topProjects } = require('../summary');
const { hhmm } = require('../time');

// tally top FILE [--n N]
function run(entries, options) {
  const top = topProjects(summarize(entries), options.n);
  return top.map(([name, minutes], index) => `${index + 1}. ${name} ${hhmm(minutes)}\n`).join('');
}

module.exports = { run };
