'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { createServer } = require('node:http');
const { join } = require('node:path');
const { tmpdir } = require('node:os');
const { mkdtempSync, rmSync } = require('node:fs');
const { version } = require('../../package.json');
const { compareVersions, notifierEnabled, updateNotice } = require('../../bin/xbot-cli.cjs');
const launcher = join(__dirname, '..', '..', 'bin', 'xbot-cli.cjs');

test('compareVersions orders release versions and ignores pre-release suffix', () => {
  assert.ok(compareVersions('0.8.0', '0.7.2') > 0);
  assert.ok(compareVersions('0.7.2', '0.8.0') < 0);
  assert.ok(compareVersions('1.0.0', '0.99.9') > 0);
  assert.equal(compareVersions('0.7.2', '0.7.2'), 0);
  assert.ok(compareVersions('0.8.0-beta.1', '0.7.2') > 0);
  assert.equal(compareVersions('0.7.2', version), 0);
});

test('notifier is opt-in under piped stderr and honors env switches', () => {
  assert.equal(notifierEnabled(), false); // node:test 的 stderr 不是 TTY
  process.env.CI = 'true';
  assert.equal(notifierEnabled(), false); // CI 环境默认关闭
  delete process.env.CI;
  process.env.XBOT_UPDATE_NOTIFIER = '1';
  assert.equal(notifierEnabled(), true);
  process.env.CI = 'true';
  assert.equal(notifierEnabled(), true); // 显式开关优先于 CI
  delete process.env.CI;
  process.env.XBOT_NO_UPDATE_NOTIFIER = '1';
  assert.equal(notifierEnabled(), false);
  delete process.env.XBOT_NO_UPDATE_NOTIFIER;
  delete process.env.XBOT_UPDATE_NOTIFIER;
});

test('update notice prints once per version over the launcher', async () => {
  const registry = createServer((req, res) => {
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ version: '999.0.0' }));
  });
  await new Promise((resolve) => registry.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${registry.address().port}`;

  const work = mkdtempSync(join(tmpdir(), 'xbot-update-test-'));
  const env = {
    ...process.env,
    XBOT_UPDATE_NOTIFIER: '1',
    XBOT_UPDATE_REGISTRY: base,
    XBOT_UPDATE_CACHE_DIR: join(work, 'cache'),
  };
  const run = () => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [launcher, '--version'], {
      cwd: work, env, shell: false,
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('error', reject);
    child.on('close', (status) => resolve({ status, stdout, stderr }));
  });

  try {
    const first = await run();
    assert.equal(first.stdout.trim(), `xbot-cli ${version}`);
    assert.match(first.stderr, /新版本 xbot-cli 999\.0\.0 可用/);
    assert.match(first.stderr, new RegExp(`npm i -g .* 更新`));

    const second = await run();
    assert.equal(second.stdout.trim(), `xbot-cli ${version}`);
    assert.equal(second.stderr, '');
  } finally {
    registry.close();
    rmSync(work, { recursive: true, force: true });
  }
});

test('updateNotice stays silent when registry is unreachable', async () => {
  const work = mkdtempSync(join(tmpdir(), 'xbot-update-test-'));
  process.env.XBOT_UPDATE_NOTIFIER = '1';
  process.env.XBOT_UPDATE_REGISTRY = 'http://127.0.0.1:1';
  process.env.XBOT_UPDATE_CACHE_DIR = join(work, 'cache');
  try {
    assert.equal(await updateNotice(), '');
  } finally {
    delete process.env.XBOT_UPDATE_NOTIFIER;
    delete process.env.XBOT_UPDATE_REGISTRY;
    delete process.env.XBOT_UPDATE_CACHE_DIR;
    rmSync(work, { recursive: true, force: true });
  }
});
