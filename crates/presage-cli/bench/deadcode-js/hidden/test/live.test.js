'use strict';

// Everything that is reachable must still work after the clean-up.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.resolve(__dirname, '..');
const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'));

function run(args, cwd = ROOT) {
  const result = spawnSync(process.execPath, args, { cwd, encoding: 'utf8' });
  return { code: result.status, out: result.stdout, err: result.stderr };
}

const tally = path.join(ROOT, pkg.bin.tally);

test('the library entry point', () => {
  const api = require(path.join(ROOT, pkg.main));
  assert.deepEqual(Object.keys(api).sort(), ['parseLog', 'render', 'roundTo', 'summarize']);
  assert.equal(api.roundTo(61, 15), 75);
  assert.equal(api.roundTo(60, 15), 60);
  assert.equal(api.roundTo(61, 0), 61);
  const summary = api.summarize(api.parseLog('2024-01-01 09:00-09:20 a\n2024-01-01 10:00-10:05 b x y\n'));
  assert.equal(api.render(summary, 'csv'), 'project,minutes\na,20\nb,5\n');
  assert.deepEqual(JSON.parse(api.render(summary, 'json')), { total: 25, days: 1, byProject: { a: 20, b: 5 } });
});

test('the report command, with the rounding plugin from tally.config.json', () => {
  const text = run([tally, 'report', 'sample.log']);
  assert.equal(text.code, 0, text.err);
  assert.equal(text.out, 'atlas         9:30\ncomet         3:45\nbeacon        2:00\ntotal        15:15\n3 days\n');
  assert.equal(run([tally, 'report', 'sample.log', '--format', 'csv']).out, 'project,minutes\natlas,570\ncomet,225\nbeacon,120\n');
  assert.equal(JSON.parse(run([tally, 'report', 'sample.log', '--format', 'json']).out).total, 915);
});

test('without a config file nothing is rounded', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tally-'));
  fs.copyFileSync(path.join(ROOT, 'sample.log'), path.join(dir, 'sample.log'));
  assert.equal(run([tally, 'report', 'sample.log', '--format', 'csv'], dir).out, 'project,minutes\natlas,560\ncomet,225\nbeacon,110\n');
  fs.rmSync(dir, { recursive: true });
});

test('the top command and the lifecycle hooks', () => {
  const quiet = run([tally, 'top', 'sample.log', '--n', '2']);
  assert.equal(quiet.out, '1. atlas 9:30\n2. comet 3:45\n');
  assert.equal(quiet.err, '');
  const verbose = run([tally, 'top', 'sample.log', '--verbose']);
  assert.equal(verbose.out, '1. atlas 9:30\n2. comet 3:45\n3. beacon 2:00\n');
  assert.equal(verbose.err, 'tally: top sample.log\ntally: 7 entries\n');
});

test('errors', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tally-'));
  fs.writeFileSync(path.join(dir, 'bad.log'), '2024-01-01 09:00-09:20 a\nnonsense\n');
  const bad = run([tally, 'report', 'bad.log'], dir);
  assert.equal(bad.code, 1);
  assert.equal(bad.err, 'tally: bad line: nonsense\n');
  assert.equal(run([tally, 'frobnicate', 'bad.log'], dir).code, 2);
  fs.rmSync(dir, { recursive: true });
});

test('the scripts in package.json', () => {
  for (const name of ['report', 'validate']) {
    const [program, ...args] = pkg.scripts[name].split(' ');
    assert.equal(program, 'node');
    assert.equal(run(args).code, 0, name);
  }
  assert.equal(run(['tools/validate.js', 'sample.log']).out, 'ok\n');
  const strict = run(['tools/validate.js', 'sample.log', '--max', '2h30m']);
  assert.equal(strict.code, 1);
  assert.equal(strict.out, 'sample.log:4: entry of 250 minutes\nsample.log:8: entry of 220 minutes\n2 problems\n');
});
