'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { join } = require('node:path');
const { tmpdir } = require('node:os');
const { mkdtempSync, rmSync, mkdirSync, writeFileSync, existsSync } = require('node:fs');

const script = join(__dirname, '..', '..', 'scripts', 'preinstall.cjs');

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'xbot-preinstall-'));
  const npmBin = join(root, 'npm');
  const foreign = join(root, 'foreign');
  const other = join(root, 'other');
  for (const dir of [npmBin, foreign, other]) mkdirSync(dir, { recursive: true });
  writeFileSync(join(npmBin, 'xbot-cli.cmd'), '@echo off\r\nnode "%~dp0node_modules\\@guanzhu.me\\xbot-cli\\bin\\xbot-cli.cjs" %*\r\n');
  writeFileSync(join(npmBin, 'xbot.cmd'), '@echo off\r\nnode "%~dp0node_modules\\@guanzhu.me\\xbot-cli\\bin\\xbot-cli.cjs" %*\r\n');
  writeFileSync(join(npmBin, 'xbot'), '#!/bin/sh\nexec node "$(dirname "$0")/node_modules/@guanzhu.me/xbot-cli/bin/xbot-cli.cjs" "$@"\n');
  writeFileSync(join(npmBin, 'unrelated.cmd'), '@echo off\r\necho nothing to do with bots\r\n');
  writeFileSync(join(foreign, 'xbot-cli.exe'), Buffer.from([0x4d, 0x5a, 0x90, 0x00]));
  writeFileSync(join(foreign, 'xbot-cli.cmd'), '@echo off\r\nrem xbot-cli native (Rust single-file).\r\n"%~dp0xbot-cli.exe" %*\r\n');
  writeFileSync(join(foreign, 'xbot-cli.ps1'), 'Write-Output "someone elses script"\r\n');
  writeFileSync(join(other, 'xbot-cli.exe'), Buffer.from([0x4d, 0x5a, 0x90, 0x00]));
  const run = (extra = {}) => spawnSync(process.execPath, [script], {
    env: { ...process.env, XBOT_CLEANUP_NPM_BIN: npmBin, ...extra },
    encoding: 'utf8',
    shell: false,
  });
  return { root, npmBin, foreign, other, run };
}

test('preinstall removes only legacy shims in the npm prefix', (t) => {
  const { root, npmBin, foreign, other, run } = fixture();
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(existsSync(join(npmBin, 'xbot.cmd')), false);
  assert.equal(existsSync(join(npmBin, 'xbot')), false);
  assert.equal(existsSync(join(npmBin, 'xbot-cli.cmd')), true);
  assert.equal(existsSync(join(npmBin, 'unrelated.cmd')), true);
  for (const directory of [foreign, other]) {
    assert.equal(existsSync(join(directory, 'xbot-cli.exe')), true);
    assert.equal(existsSync(join(directory, 'xbot-cli.exe.bak')), false);
  }
  assert.equal(existsSync(join(foreign, 'xbot-cli.cmd')), true);
  assert.equal(existsSync(join(foreign, 'xbot-cli.cmd.bak')), false);
  assert.equal(existsSync(join(foreign, 'xbot-cli.ps1')), true);
  assert.match(result.stdout, /cleanup/);
});

test('preinstall never fails the install and honors the skip switch', (t) => {
  const { root, npmBin, foreign, run } = fixture();
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const skipped = run({ XBOT_CLI_SKIP_CLEANUP: '1' });
  assert.equal(skipped.status, 0);
  assert.equal(existsSync(join(npmBin, 'xbot.cmd')), true);
  assert.equal(existsSync(join(foreign, 'xbot-cli.exe')), true);
  const bare = spawnSync(process.execPath, [script], {
    env: { ...process.env, XBOT_CLEANUP_NPM_BIN: join(root, 'missing') },
    encoding: 'utf8',
    shell: false,
  });
  assert.equal(bare.status, 0);
});
