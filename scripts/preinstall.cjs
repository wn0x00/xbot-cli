#!/usr/bin/env node
'use strict';

// 安装前清理环境,保证 xbot-cli 命令指向本次安装:
// 1. 移除本包历史版本遗留的 xbot shim(0.8.2 起仅保留 xbot-cli 入口);
// 2. PATH 中位于 npm 目录之外的同名 xbot-cli 程序(手工放置的旧版)会截获命令,
//    移为 .bak 而不是删除,可由用户自行恢复或清理。
// 任何失败都不阻断安装;XBOT_CLI_SKIP_CLEANUP=1 完全跳过。
// XBOT_CLEANUP_NPM_BIN / XBOT_CLEANUP_PATH 是测试注入点,正常环境不设置。

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
    // 只处理指向本包的 shim;不相干的同名文件保持原样。
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

function quarantine(file) {
  let target = `${file}.bak`;
  if (fs.existsSync(target)) target = `${file}.bak-${Date.now()}`;
  fs.renameSync(file, target);
  log(`moved conflicting program ${file} -> ${target}`);
}

function quarantineForeign(dirs, ownDir) {
  let moved = 0;
  const own = ownDir && fs.realpathSync.native(ownDir);
  for (const dir of dirs) {
    if (!dir || !fs.existsSync(dir)) continue;
    let real;
    try {
      real = fs.realpathSync.native(dir);
    } catch {
      continue;
    }
    if (own && real === own) continue;
    for (const name of ['xbot-cli.exe', 'xbot-cli.cmd', 'xbot-cli.ps1', 'xbot-cli']) {
      const file = path.join(dir, name);
      if (!fs.existsSync(file)) continue;
      // 脚本类 shim 要求内容指向本包;.exe 按文件名即视为旧版原生程序。
      if (!name.endsWith('.exe') && !referencesPackage(file)) continue;
      try {
        quarantine(file);
        moved += 1;
      } catch (error) {
        log(`cannot move ${file}: ${error.message}`);
      }
    }
  }
  return moved;
}

function main() {
  if (process.env.XBOT_CLI_SKIP_CLEANUP === '1') return;
  if (process.platform !== 'win32') return;
  const dir = binDir();
  if (!dir) return;
  const dirs = (process.env.XBOT_CLEANUP_PATH || process.env.PATH || '').split(path.delimiter);
  const moved = quarantineForeign(dirs, dir);
  const removed = removeLegacyShims(dir);
  if (moved + removed > 0) {
    log(`done: ${moved} conflicting program(s) quarantined, ${removed} legacy shim(s) removed`);
  }
}

try {
  main();
} catch (error) {
  log(`skipped after error: ${error.message}`);
}
