'use strict';

// Checks a clean-up of tally: the dead code is gone and everything reachable still works.
// Solved: the visible and held-out tests pass, and at least 12 of the 15 dead things are gone.
// A dead file counts as gone when it is deleted or has no code left in it.

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const DEAD_FILES = ['src/legacy/importer.js', 'src/format-old.js'];
const DEAD_NAMES = ['weekNumber', 'parseLegacyLine', 'mergeEntries', 'toHtml', 'escapeHtml', 'defaultConfigPath', 'printBanner'];
const UNUSED_REQUIRES = [
  ['src/parse.js', 'os'],
  ['src/summary.js', 'fs'],
  ['src/format.js', 'path'],
];
// A commented-out block is still there while any of its lines is.
const COMMENTED = {
  'the commented-out byWeek': ['// function byWeek(', '//     const week = weekNumber('],
  'the commented-out --banner': ["// if (rest.includes('--banner'))", '//   printBanner();'],
  'the commented-out width in top': ['// const width = Math.max(', '// return top.map('],
};
const NEED = 12;

function sources(dir, found = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!['node_modules', 'test', '.git'].includes(entry.name)) sources(full, found);
    } else if (entry.name.endsWith('.js') && entry.name !== 'verify_deadcode.js') {
      found.push(full);
    }
  }
  return found;
}

// The code without comments. Rough, but these files keep `//` out of their strings.
function code(text) {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .map((line) => line.replace(/(^|\s)\/\/.*$/, ''))
    .join('\n');
}

const files = sources('.');
const texts = Object.fromEntries(files.map((file) => [file.split(path.sep).join('/'), fs.readFileSync(file, 'utf8')]));
const allCode = Object.values(texts).map(code).join('\n');
const allLines = Object.values(texts).flatMap((text) => text.split('\n').map((line) => line.trim()));

const left = [];
for (const file of DEAD_FILES) {
  const text = texts[file];
  if (text !== undefined && code(text).replace(/['"]use strict['"];?/, '').trim() !== '') left.push(file);
}
for (const name of DEAD_NAMES) {
  if (new RegExp(`\\b${name}\\b`).test(allCode)) left.push(`${name}()`);
}
for (const [file, name] of UNUSED_REQUIRES) {
  const text = texts[file];
  if (text === undefined) continue;
  const body = code(text);
  const required = new RegExp(`require\\(['"](node:)?${name}['"]\\)`).test(body);
  const uses = (body.replace(/require\([^)]*\)/g, '').match(new RegExp(`\\b${name}\\b`, 'g')) || []).length;
  if (required && uses <= 1) left.push(`unused require of ${name} in ${file}`);
}
for (const [label, markers] of Object.entries(COMMENTED)) {
  if (allLines.some((line) => markers.some((marker) => line.startsWith(marker)))) left.push(label);
}

const total = DEAD_FILES.length + DEAD_NAMES.length + UNUSED_REQUIRES.length + Object.keys(COMMENTED).length;
const gone = total - left.length;
const tests = spawnSync(process.execPath, ['--test'], { encoding: 'utf8', timeout: 90000 });
const testsOk = tests.status === 0;
if (!testsOk) {
  const lines = `${tests.stdout}${tests.stderr}`.split('\n').filter((line) => /^\s*(✖|not ok|.*Error|\s+at )/.test(line));
  console.log(lines.slice(0, 8).join('\n'));
}
console.log(`tests ${testsOk ? 'pass' : 'FAIL'}; dead code removed: ${gone}/${total} (need ${NEED})`);
if (left.length) console.log(`still there: ${left.join(', ').slice(0, 600)}`);
console.log(`score: ${gone + (testsOk ? 1 : 0)}/${total + 1}`);
process.exit(testsOk && gone >= NEED ? 0 : 1);
