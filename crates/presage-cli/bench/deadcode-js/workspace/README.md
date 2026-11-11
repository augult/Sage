# tally

Adds up a plain-text time log.

    node bin/tally.js report sample.log --format text
    node bin/tally.js top sample.log --n 2

Formats: `text`, `json`, `csv`. Plugins and their settings are in `tally.config.json`.
As a library: `const { parseLog, summarize, render, roundTo } = require("tally")`.

Tests: `node --test`
