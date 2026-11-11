'use strict';

const fs = require('fs');
const path = require('path');
const hooks = require('./hooks');
const { parseLog } = require('./parse');
const { loadConfig, applyPlugins } = require('./config');

function fire(event, ...args) {
  const hook = hooks[`on${event[0].toUpperCase()}${event.slice(1)}`];
  if (hook) {
    hook(...args);
  }
}

function printBanner() {
  process.stdout.write('tally 2\n=======\n');
}

function option(args, name, fallback) {
  const at = args.indexOf(`--${name}`);
  return at >= 0 && at + 1 < args.length ? args[at + 1] : fallback;
}

// tally COMMAND FILE [--format F] [--n N] [--verbose]: returns the exit code.
function main(argv) {
  const [name, file, ...rest] = argv;
  const commandFile = path.join(__dirname, 'commands', `${name}.js`);
  if (!/^[a-z]+$/.test(name || '') || !file || !fs.existsSync(commandFile)) {
    process.stderr.write('usage: tally COMMAND FILE [--format text|json|csv] [--n N] [--verbose]\n');
    return 2;
  }
  const command = require(commandFile);
  const context = { command: name, file, verbose: rest.includes('--verbose'), entries: 0 };
  // if (rest.includes('--banner')) {
  //   printBanner();
  // }
  fire('start', context);
  try {
    const config = loadConfig(process.cwd());
    const entries = applyPlugins(parseLog(fs.readFileSync(file, 'utf8')), config);
    context.entries = entries.length;
    process.stdout.write(command.run(entries, { format: option(rest, 'format', 'text'), n: Number(option(rest, 'n', 3)) }));
  } catch (error) {
    fire('error', context, error);
    return 1;
  }
  fire('finish', context);
  return 0;
}

module.exports = { main };
