'use strict';

// Checks a log before it is committed: every line parses, and no entry is longer than --max (default 8h).
//   node tools/validate.js FILE [--max 6h30m]

const fs = require('fs');
const { parseLine, parseDuration } = require('../src/parse');

const [file, flag, value] = process.argv.slice(2);
const limit = parseDuration(flag === '--max' ? value : '8h');
let problems = 0;

fs.readFileSync(file, 'utf8').split(/\r?\n/).forEach((line, index) => {
  try {
    const entry = parseLine(line);
    if (entry && entry.minutes > limit) {
      throw new Error(`entry of ${entry.minutes} minutes`);
    }
  } catch (error) {
    problems += 1;
    console.log(`${file}:${index + 1}: ${error.message}`);
  }
});

console.log(problems === 0 ? "ok" : `${problems} problems`);
process.exitCode = problems === 0 ? 0 : 1;
