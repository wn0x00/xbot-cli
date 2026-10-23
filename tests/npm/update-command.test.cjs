'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { createServer } = require('node:http');
const { join } = require('node:path');
const { tmpdir } = require('node:os');
const { mkdtempSync, rmSync, writeFileSync, readFileSync, existsSync } = require('node:fs');
const { name, version } = require('../../package.json');
const launcher = join(__dirname, '..', '..', 'bin', 'xbot-cli.cjs');

// 本地假 registry;version 为 null 时直接不响应(模拟网络失败)。
async function fakeRegistry(latest) {
  const server = createServer((req, res) => {
    if (latest === null) {
      req.socket.destroy();
      return;
    }
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ version: latest }));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { server, base: `http://127.0.0.1:${server.address().port}` };
}

function setup(latest) {
  return fakeRegistry(latest).then(({ server, base }) => {
    const work = mkdtempSync(join(tmpdir(), 'xbot-update-cmd-'));
    const marker = join(work, 'npm-argv.json');
    const stub = join(work, 'npm-stub.cjs');
    writeFileSync(stub, [
      "'use strict';",
      "require('node:fs').writeFileSync(process.env.XBOT_TEST_NPM_MARKER, JSON.stringify(process.argv.slice(2)));",
      "process.exit(Number(process.env.XBOT_TEST_NPM_EXIT || 0));",
      '',
    ].join('\n'));
    const env = {
      ...process.env,
      XBOT_UPDATE_REGISTRY: base,
      XBOT_UPDATE_CACHE_DIR: join(work, 'cache'),
      XBOT_UPDATE_NPM: `"${process.execPath}" "${stub}"`,
      XBOT_TEST_NPM_MARKER: marker,
    };
    // 必须异步启动:假 registry 运行在本进程内,spawnSync 会阻塞事件循环使其无法响应。
    const run = (args, extra = {}) => new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [launcher, 'update', ...args], {
        cwd: work, env: { ...env, ...extra }, shell: false,
      });
      let stdout = '';
      let stderr = '';
      child.stdout.on('data', (chunk) => { stdout += chunk; });
      child.stderr.on('data', (chunk) => { stderr += chunk; });
      child.on('error', reject);
      child.on('close', (status) => resolve({ status, stdout, stderr }));
    });
    const cleanup = () => {
      server.close();
      rmSync(work, { recursive: true, force: true });
    };
    return { run, marker, cleanup };
  });
}

test('update reports already latest without invoking npm', async () => {
  const { run, marker, cleanup } = await setup(version);
  try {
    const result = await run([]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /已是最新版本/);
    assert.equal(existsSync(marker), false);
  } finally {
    cleanup();
  }
});

test('update --check reports a newer version without invoking npm', async () => {
  const { run, marker, cleanup } = await setup('999.0.0');
  try {
    const result = await run(['--check']);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /新版本 xbot-cli 999\.0\.0 可用/);
    assert.match(result.stdout, /xbot-cli update/);
    assert.equal(existsSync(marker), false);
  } finally {
    cleanup();
  }
});

test('update installs the fetched version through npm', async () => {
  const { run, marker, cleanup } = await setup('999.0.0');
  try {
    const result = await run([]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /正在升级 xbot-cli .* -> 999\.0\.0/);
    assert.match(result.stdout, /已升级到 999\.0\.0/);
    assert.deepEqual(JSON.parse(readFileSync(marker, 'utf8')), ['install', '-g', `${name}@999.0.0`]);
  } finally {
    cleanup();
  }
});

test('update propagates npm failure and exit code', async () => {
  const { run, cleanup } = await setup('999.0.0');
  try {
    const result = await run([], { XBOT_TEST_NPM_EXIT: '3' });
    assert.equal(result.status, 3);
    assert.match(result.stderr, /升级失败/);
    assert.match(result.stderr, /npm i -g/);
  } finally {
    cleanup();
  }
});

test('update rejects unknown arguments', async () => {
  const { run, cleanup } = await setup(version);
  try {
    const result = await run(['--bogus']);
    assert.equal(result.status, 2);
    assert.match(result.stderr, /用法: xbot-cli update/);
  } finally {
    cleanup();
  }
});

test('update fails cleanly when the registry is unreachable', async () => {
  const { run, cleanup } = await setup(null);
  try {
    const result = await run([]);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /无法查询最新版本/);
  } finally {
    cleanup();
  }
});
