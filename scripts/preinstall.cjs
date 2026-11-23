#!/usr/bin/env node
'use strict';

// Only remove this package's legacy shims inside the current npm prefix.
// PATH entries outside that directory may belong to other tools and stay untouched.
// Failures do not block installation; XBOT_CLI_SKIP_CLEANUP=1 skips cleanup.
// XBOT_CLEANUP_NPM_BIN is a test injection point and is not set in normal installs.

const fs = require('node:fs');
const path = require('node:path');

function log(message) {
  console.log(`[xbot-cli cleanup] ${message}`);
}

function binDir() {
  return process.env.XBOT_CLEANUP_NPM_BIN || process.env.npm_config_prefix || null;
}

function referencesPackage(file) {
  try {
    return fs.readFileSync(file, 'utf8').includes('xbot-cli');
  } catch {
    return false;
  }
}

function removeLegacyShims(dir) {
  let removed = 0;
  for (const name of ['xbot', 'xbot.cmd', 'xbot.ps1']) {
    const file = path.join(dir, name);
    if (!fs.existsSync(file) || !referencesPackage(file)) continue;
    try {
      fs.rmSync(file, { force: true });
      removed += 1;
      log(`removed legacy shim ${file}`);
    } catch (error) {
      log(`cannot remove ${file}: ${error.message}`);
    }
  }
  return removed;
}

function main() {
  if (process.env.XBOT_CLI_SKIP_CLEANUP === '1') return;
  if (process.platform !== 'win32') return;
  const dir = binDir();
  if (!dir) return;
  const removed = removeLegacyShims(dir);
  if (removed > 0) log(`done: ${removed} legacy shim(s) removed`);
}

try {
  main();
} catch (error) {
  log(`skipped after error: ${error.message}`);
}
