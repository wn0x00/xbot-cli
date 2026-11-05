'use strict';

const { createHash } = require('node:crypto');
const { readFileSync, writeFileSync, existsSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { root, run } = require('./common.cjs');

const inputs = ['helpers/browser-query/BrowserQuery.csproj', 'helpers/browser-query/Program.cs'];
const assets = ['BrowserQuery.dll', 'BrowserQuery.runtimeconfig.json'];
const directory = join(root, 'src', 'browser_assets');
const manifestPath = join(directory, 'manifest.json');
const sha256 = data => createHash('sha256').update(data).digest('hex');

function sourceHash() {
  return sha256(inputs.map(name => `${name}\0${readFileSync(join(root, name), 'utf8').replace(/\r\n/g, '\n')}`).join('\0'));
}

function verify() {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (manifest.sourceSha256 !== sourceHash()) throw new Error('Browser DLL adapter source changed; rebuild it with node scripts/build-browser-query.cjs <ShadowBot-version-directory>.');
  for (const name of assets) {
    if (manifest.assets[name] !== sha256(readFileSync(join(directory, name)))) throw new Error(`Browser DLL adapter asset is stale or corrupt: ${name}`);
  }
}

function build(installation) {
  if (!installation) throw new Error('Usage: node scripts/build-browser-query.cjs <ShadowBot-version-directory>');
  installation = resolve(installation);
  for (const name of ['ShadowBot.Common.dll', 'ShadowBot.Runtime.dll', 'ShadowBot.UIAutomation.dll']) {
    if (!existsSync(join(installation, name))) throw new Error(`Missing installed assembly: ${name}`);
  }
  run('dotnet', ['build', inputs[0], '-c', 'Release', '--nologo',
    `-p:ShadowBotDirectory=${installation}`, `-p:PathMap=${root}=/build/xbot-cli`, '-o', directory]);
  // Keep text hashes independent of checkout newline handling.
  const config = join(directory, 'BrowserQuery.runtimeconfig.json');
  writeFileSync(config, readFileSync(config, 'utf8').replace(/\r\n/g, '\n'));
  writeFileSync(manifestPath, `${JSON.stringify({
    sourceSha256: sourceHash(),
    assets: Object.fromEntries(assets.map(name => [name, sha256(readFileSync(join(directory, name)))])),
  }, null, 2)}\n`);
  verify();
}

if (require.main === module) build(process.argv[2]);
module.exports = { sourceHash, verify };
