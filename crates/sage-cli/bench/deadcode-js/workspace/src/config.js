'use strict';

const fs = require('fs');
const path = require('path');

const FILE = 'tally.config.json';

function defaultConfigPath() {
  return path.join(process.env.HOME || '.', '.tallyrc');
}

// The settings in tally.config.json in `dir`, with its plugins loaded. No file: no plugins.
function loadConfig(dir) {
  const file = path.join(dir, FILE);
  if (!fs.existsSync(file)) {
    return { plugins: [] };
  }
  const config = JSON.parse(fs.readFileSync(file, 'utf8'));
  const plugins = (config.plugins || []).map((entry) => require(path.resolve(dir, entry)));
  return { ...config, plugins };
}

// Runs the entries through every plugin in turn.
function applyPlugins(entries, config) {
  return config.plugins.reduce((current, plugin) => plugin.transform(current, config), entries);
}

module.exports = { loadConfig, applyPlugins };
