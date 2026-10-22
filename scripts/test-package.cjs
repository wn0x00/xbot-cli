'use strict';

const assert = require('node:assert/strict');
const { mkdirSync, mkdtempSync, existsSync, readFileSync, readdirSync, rmSync } = require('node:fs');
const { join, resolve, relative } = require('node:path');
const { tmpdir } = require('node:os');
const { createHash } = require('node:crypto');
const { spawnSync } = require('node:child_process');
const { root, manifest, run, npm } = require('./common.cjs');

run(process.execPath, [join(__dirname, 'build.cjs')]);
const dist = join(root, 'dist');
mkdirSync(dist, { recursive: true });
const packed = JSON.parse(npm(['pack', '--ignore-scripts', '--json', '--pack-destination', dist], {
  encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
}).stdout)[0];
const expected = ['LICENSE', 'README.md', 'bin/xbot-cli.cjs', 'native/win32-x64/xbot-cli.exe', 'package.json'].sort();
assert.deepEqual(packed.files.map(file => file.path).sort(), expected, 'Unexpected npm package content');
const tarball = join(dist, packed.filename);
const scratch = mkdtempSync(join(tmpdir(), 'xbot-cli package test '));
const prefix = join(scratch, 'install prefix');
const workdir = join(scratch, 'user workspace');
mkdirSync(workdir, { recursive: true });

try {
  // Local tarball, no dependencies, and lifecycle scripts disabled: no compiler or download.
  npm(['install', '--global', '--prefix', prefix, '--ignore-scripts', '--offline', '--no-audit', '--no-fund', tarball]);
  const launcher = join(prefix, 'node_modules', ...manifest.name.split('/'), 'bin', 'xbot-cli.cjs');
  for (const command of ['xbot', 'xbot-cli']) {
    const shim = join(prefix, `${command}.cmd`);
    assert.ok(existsSync(shim), `npm must create ${command}.cmd`);
    function invokeShim(args) {
      const quotedArgs = args.map(arg => `'${arg.replaceAll("'", "''")}'`).join(' ');
      return run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
        `& '${shim.replaceAll("'", "''")}' ${quotedArgs}; exit $LASTEXITCODE`], {
        cwd: workdir, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
        env: { ...process.env, XBOT_SHELL_CLI: join(scratch, 'missing-backend.exe') },
      });
    }
    const version = invokeShim(['--version']);
    assert.equal(version.stdout.trim(), `xbot-cli ${manifest.version}`);
    assert.equal(version.stderr, '');
    const versionCommand = invokeShim(['version']);
    assert.equal(versionCommand.stdout, version.stdout, `${command} version must match --version`);
    assert.equal(versionCommand.stderr, '');
    const versionJson = invokeShim(['version', '--json']);
    assert.deepEqual(JSON.parse(versionJson.stdout), {
      data: { name: 'xbot-cli', version: manifest.version }, ok: true,
    });
    assert.equal(versionJson.stderr, '');
    for (const args of [[], ['--help'], ['app'], ['web'], ['browser']]) {
      const help = invokeShim(args);
      assert.match(help.stdout, /Usage:/, `${command} ${args.join(' ')} must show help`);
      assert.equal(help.stderr, '');
    }
    const webPreview = invokeShim(['web', 'open', 'example.com', '--dry-run', '--json']);
    const webData = JSON.parse(webPreview.stdout).data;
    assert.equal(webData.dry_run, true);
    assert.equal(webData.arguments.url, 'https://example.com/');
    assert.equal(webData.snapshot_after, true);
    assert.equal(webPreview.stderr, '');
  }
  function invoke(args, input) {
    const result = spawnSync(process.execPath, [launcher, ...args], {
      cwd: workdir, input, encoding: 'utf8', shell: false,
      env: { ...process.env, XBOT_SHELL_CLI: join(scratch, 'missing-backend.exe') },
    });
    assert.ifError(result.error);
    return result;
  }
  assert.equal(invoke(['--version']).stdout.trim(), `xbot-cli ${manifest.version}`);
  for (const args of [[], ['app']]) {
    const result = invoke(args);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Usage:/);
  }
  const error = invoke(['app', 'open', '--json']);
  assert.equal(error.status, 2);
  assert.equal(error.stdout, '');
  assert.equal(JSON.parse(error.stderr).error.code, 'BAD_ARGS');
  const discovery = invoke(['schema', 'flow', 'code', 'write']);
  assert.equal(JSON.parse(discovery.stdout).data.name, 'flow code write');
  const preview = invoke(['flow', 'code', 'write', 'main', '--file', '-', '--dry-run', '--json'], 'print("npm stdin")\n');
  assert.equal(preview.status, 0, preview.stderr);
  assert.equal(JSON.parse(preview.stdout).data.dry_run, true);
  const skill = invoke(['install', '--skills', '--json']);
  assert.equal(skill.status, 0, skill.stderr);
  const skillRoot = join(workdir, '.agents', 'skills', 'xbot-app-dev');
  const sourceSkillRoot = join(root, 'skills', 'xbot-app-dev');
  function skillFiles(directory, prefix = '') {
    return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
      const path = prefix ? `${prefix}/${entry.name}` : entry.name;
      return entry.isDirectory() ? skillFiles(join(directory, entry.name), path) : [path];
    }).sort();
  }
  const expectedSkillFiles = skillFiles(sourceSkillRoot);
  assert.deepEqual(skillFiles(skillRoot), expectedSkillFiles);
  assert.deepEqual(JSON.parse(skill.stdout).data.files.map(path =>
    relative(skillRoot, path).replaceAll('\\', '/')).sort(), expectedSkillFiles);
  for (const file of expectedSkillFiles) {
    assert.deepEqual(readFileSync(join(skillRoot, file)), readFileSync(join(sourceSkillRoot, file)),
      `Installed Skill resource differs: ${file}`);
  }
  assert.match(invoke(['guide', 'cli-contract']).stdout, /^# /);
  for (const [topic, resource] of [
    ['automation-routing', 'references/automation-routing.md'],
    ['web-module', 'modules/web.md'],
    ['win-design', 'modules/win.md'],
    ['win-repair', 'references/win-repair.md'],
    ['locator-repair', 'references/locator-repair.md'],
    ['locator-usage', 'references/locator-usage.md'],
    ['collection-repair', 'references/collection-repair.md'],
    ['skill-updates', 'UPDATE-RECORD.md'],
  ]) {
    const result = invoke(['guide', topic, '--json']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(result.stdout).data.markdown,
      readFileSync(join(sourceSkillRoot, resource), 'utf8'), `Stale embedded guide: ${topic}`);
  }
  const nativeSchema = invoke(['schema', 'win32', 'snapshot']);
  assert.equal(nativeSchema.status, 0, nativeSchema.stderr);
  const nativeContract = JSON.parse(nativeSchema.stdout).data;
  assert.equal(nativeContract.context, 'windows_uia');
  assert.equal(nativeContract.requires_studio, false);
  assert.deepEqual(nativeContract.effects, ['read']);
  const digest = createHash('sha256').update(readFileSync(tarball)).digest('hex');
  console.log(`Package installation verified: ${relative(root, tarball)}`);
  console.log(`SHA256 ${digest}`);
} finally {
  const safeRoot = resolve(tmpdir());
  const target = resolve(scratch);
  if (!target.startsWith(`${safeRoot}\\`) || !target.includes('xbot-cli package test ')) {
    throw new Error(`Refusing to remove unexpected test directory: ${target}`);
  }
  rmSync(target, { recursive: true, force: true });
}
