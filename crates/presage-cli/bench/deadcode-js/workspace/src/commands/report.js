'use strict';

const { summarize } = require('../summary');
const { render } = require('../format');

// tally report FILE [--format F]
function run(entries, options) {
  return render(summarize(entries), options.format);
}

module.exports = { run };
