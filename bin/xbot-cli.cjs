#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const { dirname, join } = require('node:path');
const { tmpdir } = require('node:os');
const { mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const pkg = require('../package.json');

const UPDATE_TTL_MS = 24 * 60 * 60 * 1000;
const UPDATE_FAILURE_TTL_MS = 60 * 60 * 1000;
const UPDATE_TIMEOUT_MS = 1000;

// 数字段比较,忽略预发布后缀;返回正数表示 a 比 b 新。
function compareVersions(a, b) {
  const pick = (v) => String(v).split('-')[0].split('.').map((n) => parseInt(n, 10) || 0);
  const pa = pick(a);
  const pb = pick(b);
  for (let i = 0; i < 3; i += 1) {
    if (pa[i] !== pb[i]) return (pa[i] || 0) - (pb[i] || 0);
  }
  return 0;
}

function notifierEnabled() {
  if (process.env.XBOT_NO_UPDATE_NOTIFIER) return false;
  if (process.env.CI) return false;
  if (process.env.XBOT_UPDATE_NOTIFIER === '1') return true;
  return Boolean(process.stderr.isTTY);
}

function cacheFile() {
  const dir = process.env.XBOT_UPDATE_CACHE_DIR
    || join(process.env.LOCALAPPDATA || tmpdir(), 'xbot-cli');
  return join(dir, 'update-check.json');
}

function readCache() {
  try {
    return JSON.parse(readFileSync(cacheFile(), 'utf8'));
  } catch {
    return {};
  }
}

function writeCache(cache) {
  try {
    mkdirSync(dirname(cacheFile()), { recursive: true });
    writeFileSync(cacheFile(), JSON.stringify(cache));
  } catch {
    // 缓存写失败不影响 CLI 本身。
  }
}

async function fetchLatest() {
  const base = (process.env.XBOT_UPDATE_REGISTRY || 'https://registry.npmjs.org').replace(/\/+$/, '');
  const name = pkg.name.startsWith('@') ? `@${pkg.name.slice(1).replace('/', '%2F')}` : pkg.name;
  const response = await fetch(`${base}/${name}/latest`, {
    signal: AbortSignal.timeout(UPDATE_TIMEOUT_MS),
  });
  if (!response.ok) throw new Error(`registry ${response.status}`);
  const body = await response.json();
  if (!body || typeof body.version !== 'string') throw new Error('registry bad payload');
  return body.version;
}

// 命中缓存就直接用;过期则联网刷新,失败退避 1 小时,任何失败都不抛错。
async function resolveLatest(cache, now) {
  if (cache.expiresAt && cache.expiresAt > now && cache.latest) return cache.latest;
  try {
    cache.latest = await fetchLatest();
    cache.expiresAt = now + UPDATE_TTL_MS;
  } catch {
    if (!cache.latest) cache.expiresAt = now + UPDATE_FAILURE_TTL_MS;
  }
  writeCache(cache);
  return cache.latest;
}

function formatNotice(latest) {
  const text = `新版本 xbot-cli ${latest} 可用(当前 ${pkg.version})。运行 npm i -g ${pkg.name} 更新。`;
  if (process.stderr.isTTY) return `\u001b[33m${text}\u001b[0m\n`;
  return `${text}\n`;
}

// 返回要写到 stderr 的提醒文本;不需要提醒时返回空串。
async function updateNotice() {
  if (!notifierEnabled()) return '';
  const now = Date.now();
  const cache = readCache();
  const latest = await resolveLatest(cache, now);
  if (!latest || compareVersions(latest, pkg.version) <= 0) return '';
  if (cache.notifiedFor === latest) return '';
  cache.notifiedFor = latest;
  writeCache(cache);
  return formatNotice(latest);
}

async function main() {
  if (process.platform !== 'win32' || process.arch !== 'x64') {
    process.stderr.write('xbot-cli currently supports Windows x64 with ShadowBot installed.\n');
    process.exit(1);
  }

  // 与原生进程并行发起,最多拖慢退出约 1 秒,失败静默。
  const noticePromise = updateNotice().catch(() => '');

  // Preserve stdin, terminal detection, output streams, cwd and argument boundaries.
  const executable = join(__dirname, '..', 'native', 'win32-x64', 'xbot-cli.exe');
  const result = spawnSync(executable, process.argv.slice(2), {
    stdio: 'inherit',
    shell: false,
    windowsHide: true,
  });
  if (result.error) {
    process.stderr.write(`Unable to start xbot-cli (${result.error.code}). Reinstall the npm package.\n`);
    process.exit(1);
  }
  const notice = await noticePromise;
  if (notice) process.stderr.write(notice);
  process.exit(result.status ?? (result.signal === 'SIGINT' ? 130 : 1));
}

if (require.main === module) {
  main();
}

module.exports = { compareVersions, notifierEnabled, updateNotice, formatNotice };
