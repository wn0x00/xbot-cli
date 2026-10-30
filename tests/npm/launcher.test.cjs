'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { join } = require('node:path');
const { tmpdir } = require('node:os');
const { version } = require('../../package.json');
const launcher = join(__dirname, '..', '..', 'bin', 'xbot-cli.cjs');
const { nativeEnvironment } = require(launcher);

test('npm launcher identifies itself without overriding explicit browser context', () => {
  const original = Object.freeze({
    XBOT_CLI_LAUNCHER_PID: 'old-launcher',
    XBOT_WEB_SESSION: 'isolated-workflow',
    XBOT_WEB_PIPE: 'explicit-pipe',
    XBOT_WEB_TAB_ID: '123',
    PATH: 'unchanged',
  });
  assert.deepEqual(nativeEnvironment(original, 456), {
    ...original, XBOT_CLI_LAUNCHER_PID: '456',
  });
  assert.equal(original.XBOT_CLI_LAUNCHER_PID, 'old-launcher');
  assert.equal(nativeEnvironment().XBOT_CLI_LAUNCHER_PID, String(process.pid));
});

function invoke(args, input) {
  const result = spawnSync(process.execPath, [launcher, ...args], {
    cwd: tmpdir(), input, encoding: 'utf8', shell: false,
    env: { ...process.env, XBOT_SHELL_CLI: 'xbot-cli-nonexistent-shell-for-test.exe' },
  });
  assert.ifError(result.error);
  return result;
}

test('npm launcher keeps help and version offline', () => {
  for (const args of [[], ['app'], ['flow', 'code'], ['--help']]) {
    const result = invoke(args);
    assert.equal(result.status, 0);
    assert.equal(result.stderr, '');
    assert.match(result.stdout, /Usage:/);
  }
  assert.equal(invoke(['--version']).stdout.trim(), `xbot-cli ${version}`);
});

test('npm launcher preserves structured error stream and native exit status', () => {
  const result = invoke(['app', 'open', '--json']);
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
  assert.equal(JSON.parse(result.stderr).error.code, 'BAD_ARGS');
});

test('npm launcher keeps stdin and shell metacharacters as literal data', () => {
  const content = 'print("中文 & | %PATH% $()")\n';
  const result = invoke(['flow', 'code', 'write', 'main', '--file', '-', '--dry-run', '--json'], content);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, '');
  const data = JSON.parse(result.stdout).data;
  assert.equal(data.dry_run, true);
  assert.ok(JSON.stringify(data).includes('中文 & | %PATH% $()'));
});
