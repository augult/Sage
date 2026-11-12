'use strict';

// Scores ttlcache.test.js: it must pass on ttlcache.js and fail on broken copies of it.
// Each mutant is ttlcache.js with one documented rule broken. Solved: the suite passes on the real
// module and fails on at least NEED of the mutants.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const MODULE = 'ttlcache.js';
const TESTS = 'ttlcache.test.js';
const NEED = 11;

// name -> [text in the module, what it becomes]
const MUTANTS = {
  'still live at exactly its time to live': ['return this._now() >= entry.expires;', 'return this._now() > entry.expires;'],
  'get() extends the life of an entry': [
    'this._stats.hits += 1;',
    'this._stats.hits += 1;\n    entry.expires = this._now() + entry.ttl;',
  ],
  'has() counts as use': ['return this._live(key) !== undefined;', 'return this._use(key) !== undefined;'],
  'peek() counts as use': [
    'const entry = this._live(key);\n    return entry === undefined ? undefined : entry.value;',
    'const entry = this._use(key);\n    return entry === undefined ? undefined : entry.value;',
  ],
  'set() of an existing key does not count as use': [
    '    if (previous !== undefined) {\n      this._entries.delete(key);\n    } else if',
    '    if (previous !== undefined) {\n    } else if',
  ],
  'set() of an existing key does not restart its life': [
    'expires: this._now() + ttl });',
    'expires: previous ? previous.expires : this._now() + ttl });',
  ],
  'replacing a key in a full cache evicts': [
    '    if (previous !== undefined) {\n      this._entries.delete(key);\n    } else if (this._entries.size >= this._capacity) {',
    '    if (this._entries.size >= this._capacity) {',
  ],
  'size counts expired entries': ['get size() {\n    this._purge();', 'get size() {'],
  'expired entries take up room': ['    this._purge();\n    const previous', '    const previous'],
  'the ttl given to set() is ignored': [
    'set(key, value, ttl = this._ttl) {',
    'set(key, value, ttl = this._ttl) {\n    ttl = ttl > 0 ? this._ttl : ttl;',
  ],
  'delete() of an expired entry returns true': [
    'const entry = this._live(key);\n    if (entry !== undefined) {\n      this._entries.delete(key);\n    }\n    return entry !== undefined;',
    'return this._entries.delete(key);',
  ],
  'onEvict is not called for expired entries': ["this._onEvict(key, entry.value, 'expired');", ''],
  'stats() hands out the live counters': ['return { ...this._stats };', 'return this._stats;'],
  'keys() lists the most recently used first': ['return [...this._entries.keys()];', 'return [...this._entries.keys()].reverse();'],
};

function runTests(folder) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ['--test', TESTS], { cwd: folder, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', (chunk) => (output += chunk));
    child.stderr.on('data', (chunk) => (output += chunk));
    const timer = setTimeout(() => child.kill('SIGKILL'), 60000);
    child.on('close', (code) => {
      clearTimeout(timer);
      resolve({ passed: code === 0, output });
    });
  });
}

async function survives(name) {
  const [from, to] = MUTANTS[name];
  const source = fs.readFileSync(MODULE, 'utf8');
  if (source.split(from).length !== 2) {
    console.log(`mutant "${name}" does not apply`);
    process.exit(2);
  }
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'ttl-mutant-'));
  for (const entry of fs.readdirSync('.', { withFileTypes: true })) {
    if (entry.isFile() && /\.(js|cjs|mjs|json)$/.test(entry.name) && ![MODULE, 'verify_mutants.js'].includes(entry.name)) {
      fs.copyFileSync(entry.name, path.join(folder, entry.name));
    }
  }
  fs.writeFileSync(path.join(folder, MODULE), source.replace(from, to));
  const { passed } = await runTests(folder);
  fs.rmSync(folder, { recursive: true, force: true });
  return passed;
}

async function main() {
  const names = Object.keys(MUTANTS);
  const fail = (message) => {
    console.log(message);
    console.log(`score: 0/${names.length}`);
    process.exit(1);
  };
  if (!fs.existsSync(TESTS)) fail(`${TESTS} is missing`);
  const real = await runTests('.');
  if (!real.passed) {
    console.log(real.output.split('\n').filter((line) => /✖|Error|expected|actual/.test(line)).slice(0, 8).join('\n'));
    fail(`${TESTS} does not pass on the real ttlcache.js`);
  }
  if (!/\bpass [1-9]/.test(real.output)) fail(`${TESTS} runs no tests`);
  const survived = [];
  const queue = [...names];
  const worker = async () => {
    while (queue.length) {
      const name = queue.shift();
      if (await survives(name)) survived.push(name);
    }
  };
  await Promise.all([worker(), worker(), worker(), worker()]);
  const killed = names.length - survived.length;
  console.log(`the suite catches ${killed} of ${names.length} broken caches (need ${NEED})`);
  if (survived.length) console.log(`not caught: ${survived.join('; ')}`);
  console.log(`score: ${killed}/${names.length}`);
  process.exit(killed >= NEED ? 0 : 1);
}

main();
