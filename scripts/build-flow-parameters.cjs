'use strict';

const { createHash } = require('node:crypto');
const { readFileSync, writeFileSync } = require('node:fs');
const { join } = require('node:path');
const { root, run } = require('./common.cjs');

const inputs = ['helpers/flow-parameters/FlowParameters.csproj', 'helpers/flow-parameters/Program.cs'];
const assets = ['FlowParameters.dll', 'FlowParameters.runtimeconfig.json'];
const directory = join(root, 'src', 'parameter_assets');
const manifestPath = join(directory, 'manifest.json');
const sha256 = data => createHash('sha256').update(data).digest('hex');
const sourceHash = () => sha256(inputs.map(name =>
  `${name}\0${readFileSync(join(root, name), 'utf8').replace(/\r\n/g, '\n')}`).join('\0'));

function verify() {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (manifest.sourceSha256 !== sourceHash()) throw new Error('Rebuild the parameter adapter: node scripts/build-flow-parameters.cjs');
  for (const name of assets) {
    if (manifest.assets[name] !== sha256(readFileSync(join(directory, name)))) throw new Error(`Stale parameter adapter: ${name}`);
  }
}

function build() {
  run('dotnet', ['build', inputs[0], '-c', 'Release', '--nologo',
    `-p:PathMap=${root}=/build/xbot-cli`, '-o', directory]);
  const config = join(directory, 'FlowParameters.runtimeconfig.json');
  writeFileSync(config, readFileSync(config, 'utf8').replace(/\r\n/g, '\n'));
  writeFileSync(manifestPath, JSON.stringify({
    sourceSha256: sourceHash(),
    assets: Object.fromEntries(assets.map(name => [name, sha256(readFileSync(join(directory, name)))])),
  }, null, 2) + '\n');
  verify();
}

if (require.main === module) build();
module.exports = { verify };
