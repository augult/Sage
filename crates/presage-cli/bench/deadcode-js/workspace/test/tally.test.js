'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { parseLine, parseLog } = require('../src/parse');
const { summarize, topProjects } = require('../src/summary');
const { render } = require('../src/format');

const LOG = `# two days
2024-03-04 09:00-10:30 atlas Spec
2024-03-04 13:00-13:45 beacon

2024-03-05 08:15-09:00 atlas Importer
`;

test('parses a line', () => {
  assert.deepEqual(parseLine('2024-03-04 09:00-10:30 atlas Writing the spec'), {
    date: '2024-03-04',
    minutes: 90,
    project: 'atlas',
    note: 'Writing the spec',
  });
  assert.equal(parseLine('   '), null);
  assert.equal(parseLine('# comment'), null);
  assert.throws(() => parseLine('yesterday atlas'), /bad line/);
  assert.throws(() => parseLine('2024-03-04 10:00-09:00 atlas'), /bad range/);
});

test('summarizes a log', () => {
  const summary = summarize(parseLog(LOG));
  assert.deepEqual(summary, { total: 180, days: 2, byProject: { atlas: 135, beacon: 45 } });
  assert.deepEqual(topProjects(summary, 1), [['atlas', 135]]);
});

test('renders text', () => {
  const text = render(summarize(parseLog(LOG)), 'text');
  assert.equal(text, 'atlas         2:15\nbeacon        0:45\ntotal         3:00\n2 days\n');
  assert.throws(() => render({}, 'xml'), /unknown format/);
});
