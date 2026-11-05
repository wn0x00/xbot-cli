'use strict';

const { copyFileSync, mkdirSync } = require('node:fs');
const { dirname, join } = require('node:path');
const { homedir } = require('node:os');
const { root, target, nativePath, run, checkVersions } = require('./common.cjs');

if (process.platform !== 'win32' || process.arch !== 'x64') {
  throw new Error('Build the npm package on Windows x64 with the MSVC Rust toolchain.');
}
checkVersions();
require('./build-browser-query.cjs').verify();
require('./build-flow-parameters.cjs').verify();
// Use deterministic release flags and avoid embedding the maintainer's local paths
// in dependency panic messages. Encoded flags preserve paths containing spaces.
const flags = [
  '-C', 'target-feature=+crt-static',
  `--remap-path-prefix=${homedir()}=/build/home`,
  `--remap-path-prefix=${process.env.CARGO_HOME || join(homedir(), '.cargo')}=/build/cargo`,
  `--remap-path-prefix=${root}=/build/xbot-cli`,
];
run('cargo', ['build', '--release', '--locked', '--target', target], {
  env: { ...process.env, CARGO_ENCODED_RUSTFLAGS: flags.join('\x1f') },
});
const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1'], {
  encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
}).stdout);
mkdirSync(dirname(nativePath), { recursive: true });
copyFileSync(join(metadata.target_directory, target, 'release', 'xbot-cli.exe'), nativePath);
require('./check-package.cjs');
console.log(`Built ${target} npm executable relative to ${root}`);
