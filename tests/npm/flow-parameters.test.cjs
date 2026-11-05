'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { tmpdir } = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');

const root = path.resolve(__dirname, '../..');
const cli = path.join(root, 'native/win32-x64/xbot-cli.exe');
const helper = path.join(root, 'src/parameter_assets/FlowParameters.dll');
const runtime = process.env.XBOT_PARAMETER_TEST_RUNTIME || 'dotnet';
const env = { ...process.env, DOTNET_ROLL_FORWARD: 'Major' };
const probe = spawnSync(runtime, [helper, path.join(tmpdir(), 'xbot-cli-nonexistent-parameter-request.json')], {
  encoding: 'utf8', windowsHide: true, env,
});
const available = !probe.error && probe.stdout?.trim().startsWith('{"ok":false,');
const appId = '12345678-1234-1234-1234-123456789abc';
const types = [
  ...['any', 'str', 'int', 'float', 'bool', 'list', 'dict', 'bytes', 'datetime'].map(id => ({ id, name: id })),
  ...['file', 'directory'].map(id => ({ id, name: id, scope: 'appMainFlowParameter' })),
  { id: 'xbot._web.browser.WebBrowser', name: '网页对象' },
  { id: 'openpyxl.Workbook', name: 'Excel对象' },
  { id: 'list<row>', name: '行列表', base: 'list' },
];

test('embedded parameter adapter matches its source and asset hashes', () => {
  assert.doesNotThrow(() => require('../../scripts/build-flow-parameters.cjs').verify());
});

function invoke(args, input) {
  return spawnSync(cli, args, {
    encoding: 'utf8', input, windowsHide: true,
    env: { ...process.env, XBOT_SHELL_CLI: 'missing-backend-for-offline-parameter-tests.exe',
      XBOT_MCP_ENDPOINT: 'http://127.0.0.1:1/unavailable' },
  });
}

test('extended parameter preview and schema stay offline', () => {
  const result = invoke(['flow', 'params', 'set', 'process2', '--operations', '-',
    '--expect-app', appId, '--dry-run', '--json'], JSON.stringify([
    { op: 'upsert', name: 'items', type: 'list', direction: 'In' },
    { op: 'upsert', name: 'page', type: 'xbot._web.browser.WebBrowser', direction: 'In' },
  ]));
  assert.equal(result.status, 0, result.stderr);
  const preview = JSON.parse(result.stdout).data;
  assert.equal(preview.validation.runtime, 'not_checked');
  assert.equal(preview.arguments.operations[0].type, 'list');
  const schema = JSON.parse(invoke(['schema', 'flow', 'params', 'set']).stdout).data;
  assert.deepEqual(schema.effects, ['write_files', 'save_existing_edits', 'reload_studio_app']);
  assert.equal(schema.dynamic_schema_hint, 'xbot-cli flow params types --json');
  assert.equal(schema.input_shapes.operations.schema.oneOf[0].items.then.properties.type.type, 'string');
  const catalogSchema = JSON.parse(invoke(['schema', 'flow', 'params', 'types']).stdout).data;
  assert.equal(catalogSchema.requires_studio, false);
  assert.match(invoke(['flow', 'params', '--help']).stdout, /types/);
  const rejected = invoke(['flow', 'params', 'set', 'process2', '--operations', '-',
    '--dry-run', '--json'], JSON.stringify([upsert('invalid', 'password')]));
  assert.notEqual(rejected.status, 0);
  assert.match(rejected.stderr, /password/);
  const guide = invoke(['guide', 'flow-parameters']);
  assert.equal(guide.status, 0, guide.stderr);
  assert.match(guide.stdout, /readback_verified/);
});

function fixture(t, kind = 'visual') {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'xbot-cli parameter tests '));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const users = path.join(directory, 'users');
  const packageDir = path.join(users, 'account-a', 'apps', appId, 'xbot_robot');
  const dev = path.join(packageDir, '.dev');
  fs.mkdirSync(dev, { recursive: true });
  const manifest = path.join(packageDir, 'package.json');
  const source = path.join(dev, 'process2.flow.json');
  fs.writeFileSync(manifest, JSON.stringify({
    uuid: appId, startup: 'main', robot_type: 'app',
    flows: [{ filename: 'process2', name: '子流程2', kind: kind === 'visual' ? 'Visual' : 'Code' }],
  }));
  const text = '\uFEFF{\r\n' +
    '  "name": "子流程2",\r\n' +
    `  "kind": "${kind}",\r\n` +
    (kind === 'visual' ? '  "blocks": [{"id": 900719925474099312345, "overflow": 1e400, "negative": -0}],\r\n' : '') +
    '  "parameters": [{"name":"keep","direction":"In","type":"str","kind":"Text","value":"已有值",' +
    '"description":"保留","extra":900719925474099312345}],\r\n' +
    '  "unrelated": {"spelling":1.0000e+007,"text":"中文"}\r\n}\r\n';
  fs.writeFileSync(source, text);
  fs.writeFileSync(path.join(packageDir, 'process2.py'), '# existing user code\n');
  function call(action, request) {
    const input = path.join(directory, 'request.json');
    fs.writeFileSync(input, JSON.stringify({ action, ...request }));
    const result = spawnSync(runtime, [helper, input], { encoding: 'utf8', windowsHide: true, env });
    assert.ifError(result.error);
    const response = JSON.parse(result.stdout);
    assert.equal(response.ok, result.status === 0, result.stderr);
    return response;
  }
  const plan = operations => call('plan', { package_dir: packageDir, app_id: appId,
    flow_id: 'process2', types, operations });
  const apply = data => call('apply', { plan: data });
  return { directory, users, packageDir, manifest, source, plan, apply, call, before: fs.readFileSync(source) };
}

const localTest = (name, fn) => test(name, { skip: !available && 'A .NET runtime is required for the local metadata adapter.' }, fn);
const upsert = (name, type, extra = {}) => ({ op: 'upsert', name, type, direction: 'In', ...extra });

localTest('writes exact type IDs, defaults and uninitialized Out without touching other bytes', t => {
  const f = fixture(t);
  const response = f.plan([
    upsert('items', 'list'), upsert('mapping', 'dict'), upsert('data', 'bytes'),
    upsert('page', 'xbot._web.browser.WebBrowser'), upsert('rows', 'list<row>'),
    upsert('result', 'openpyxl.Workbook', { direction: 'Out' }),
  ]);
  assert.equal(response.ok, true, JSON.stringify(response));
  const p = response.data.parameters;
  assert.equal(p.find(p => p.name === 'items').value, '[]');
  assert.equal(p.find(p => p.name === 'mapping').value, '{}');
  assert.equal(p.find(p => p.name === 'data').value, "b''");
  assert.equal(p.find(p => p.name === 'page').type, 'xbot._web.browser.WebBrowser');
  assert.equal(p.find(p => p.name === 'page').value, 'None');
  assert.equal(p.find(p => p.name === 'rows').value, '[]');
  assert.equal(p.find(p => p.name === 'result').kind, 'Expression');
  assert.equal(p.find(p => p.name === 'result').value, '');
  assert.deepEqual(fs.readFileSync(f.source), f.before, 'Planning must not write.');
  assert.equal(f.apply(response.data).ok, true);
  const after = fs.readFileSync(f.source, 'utf8');
  const before = f.before.toString('utf8');
  assert.equal(after.slice(0, after.indexOf('"parameters"')), before.slice(0, before.indexOf('"parameters"')));
  assert.equal(after.slice(after.indexOf('  "unrelated"')), before.slice(before.indexOf('  "unrelated"')));
  assert.ok(after.includes('900719925474099312345'));
  assert.ok(after.includes('"overflow": 1e400'));
  assert.ok(after.includes('"negative": -0'));
  assert.ok(after.startsWith('\uFEFF'));
  assert.ok(!after.replaceAll('\r\n', '').includes('\n'));
});

localTest('CodeFlow metadata uses the same adapter and never rewrites Python', t => {
  const f = fixture(t, 'code');
  const response = f.plan([upsert('page', 'xbot._web.browser.WebBrowser')]);
  assert.equal(response.ok, true);
  assert.equal(response.data.flow_kind, 'code');
  assert.equal(f.apply(response.data).ok, true);
  assert.equal(fs.readFileSync(path.join(f.packageDir, 'process2.py'), 'utf8'), '# existing user code\n');
});

localTest('same-contract updates preserve omitted values and unknown fields; no-op bytes remain identical', t => {
  const f = fixture(t);
  const noOp = f.plan([upsert('keep', 'str'), { op: 'remove', name: 'missing' }]);
  assert.equal(noOp.ok, true);
  assert.equal(noOp.data.changed, false);
  assert.deepEqual(Buffer.from(noOp.data.content_base64, 'base64'), f.before);
  const update = f.plan([upsert('keep', 'str', { description: '更新说明' })]);
  assert.equal(update.data.parameters[0].value, '已有值');
  assert.equal(f.apply(update.data).ok, true);
  assert.match(fs.readFileSync(f.source, 'utf8'), /"extra": 900719925474099312345/);
});

localTest('unchanged Out preserves client null or absent initializers byte for byte', t => {
  const f = fixture(t);
  const model = JSON.parse(f.before.toString('utf8').replace(/^\uFEFF/, ''));
  model.parameters = [
    { name: 'result', type: 'list', direction: 'Out', kind: 'Expression', value: null },
    { name: 'other', type: 'str', direction: 'Out' },
  ];
  fs.writeFileSync(f.source, JSON.stringify(model));
  const before = fs.readFileSync(f.source);
  const plan = f.plan(model.parameters.map(({ name, type, direction }) => upsert(name, type, { direction })));
  assert.equal(plan.ok, true);
  assert.equal(plan.data.changed, false);
  assert.deepEqual(Buffer.from(plan.data.content_base64, 'base64'), before);
  const description = f.plan([upsert('result', 'list', { direction: 'Out', description: '仅改说明' })]);
  assert.equal(description.ok, true);
  const after = JSON.parse(Buffer.from(description.data.content_base64, 'base64'));
  assert.equal(after.parameters[0].value, null);
  assert.equal(Object.hasOwn(after.parameters[1], 'value'), false);
});

localTest('type and direction changes reset stale initializers; expressions are never evaluated', t => {
  const f = fixture(t);
  const response = f.plan([upsert('keep', 'list'), upsert('out', 'str', { direction: 'Out' }),
    upsert('expression', 'any', { kind: 'Expression', value: '__import__("os").system("not executed")' })]);
  assert.equal(response.ok, true);
  assert.equal(response.data.parameters[0].value, '[]');
  assert.equal(response.data.parameters[0].kind, 'Expression');
  assert.equal(response.data.parameters[1].value, '');
  assert.equal(response.data.parameters[2].value, '__import__("os").system("not executed")');
  assert.deepEqual(fs.readFileSync(f.source), f.before);
});

localTest('invalid batches never write, and ambiguous any initializers require a kind', t => {
  const f = fixture(t);
  for (const operations of [
    [{ op: 'remove', name: 'keep' }, upsert('bad', 'invented.type')],
    [upsert('bad', 'password')],
    [upsert('bad', 'any', { value: '123' })],
    [upsert('bad', 'list', { kind: 'Expression', value: '' })],
    [{ op: 'remove', name: 'keep', type: 'str' }],
    [upsert('bad', 'str', { unknown: true })],
  ]) {
    assert.equal(f.plan(operations).ok, false);
    assert.deepEqual(fs.readFileSync(f.source), f.before);
  }
});

localTest('validates Python names, normalized collisions, and preserves Chinese contracts', t => {
  const f = fixture(t);
  for (const name of ['class', '1name', 'x;print(1)', 'ｋｅｅｐ', 'bad/name']) {
    assert.equal(f.plan([upsert(name, 'str')]).ok, false, name);
  }
  assert.equal(f.plan([upsert('中文参数', 'str')]).ok, true);
  const model = JSON.parse(f.before.toString('utf8').replace(/^\uFEFF/, ''));
  model.parameters.push({ ...model.parameters[0], name: 'ｋｅｅｐ' });
  fs.writeFileSync(f.source, JSON.stringify(model));
  assert.equal(f.plan([upsert('other', 'str')]).ok, false);
});

localTest('declared main-flow scopes remain visible without rejecting verified subflow contracts', t => {
  const f = fixture(t);
  for (const type of ['file', 'directory']) {
    const result = f.plan([upsert('new_param', type)]);
    assert.equal(result.ok, true);
    assert.equal(result.data.warnings.length, 1);
  }
  const manifest = JSON.parse(fs.readFileSync(f.manifest));
  manifest.startup = 'process2';
  fs.writeFileSync(f.manifest, JSON.stringify(manifest));
  const response = f.plan(['file', 'directory'].map(type => upsert(type + '_value', type)));
  assert.equal(response.ok, true);
  assert.equal(response.data.warnings.length, 0);
  assert.equal(f.apply(response.data).ok, true);
  manifest.startup = 'main';
  fs.writeFileSync(f.manifest, JSON.stringify(manifest));
  const legacy = f.plan([upsert('file_value', 'file', { description: '保持既有契约' })]);
  assert.equal(legacy.ok, true);
  assert.equal(legacy.data.warnings.length, 1);
});

localTest('source or manifest changes after planning prevent atomic replacement', t => {
  const f = fixture(t);
  const first = f.plan([upsert('items', 'list')]).data;
  fs.appendFileSync(f.source, ' ');
  assert.equal(f.apply(first).error.code, 'PARAMETER_CONFLICT');
  assert.deepEqual(fs.readFileSync(f.source), Buffer.concat([f.before, Buffer.from(' ')]));
  const second = f.plan([upsert('items', 'list')]).data;
  fs.appendFileSync(f.manifest, '\n');
  const conflict = f.apply(second);
  assert.equal(conflict.ok, false);
  assert.equal(conflict.data.written, false);
});

localTest('apply rejects plans that alter non-parameter data even with a valid content hash', t => {
  const f = fixture(t);
  const plan = f.plan([upsert('items', 'list')]).data;
  const tampered = Buffer.from(Buffer.from(plan.content_base64, 'base64').toString('utf8').replace('"子流程2"', '"changed"'));
  plan.content_base64 = tampered.toString('base64');
  plan.after_sha256 = createHash('sha256').update(tampered).digest('hex');
  assert.equal(f.apply(plan).ok, false);
  assert.deepEqual(fs.readFileSync(f.source), f.before);
});

localTest('context rejects account ambiguity, View copies and unsafe flow paths', t => {
  const f = fixture(t);
  const request = { users_root: f.users, app_id: appId, flow_id: 'process2' };
  assert.equal(f.call('context', request).ok, true);
  const other = path.join(f.users, 'account-b', 'apps', appId, 'xbot_robot');
  fs.mkdirSync(other, { recursive: true });
  fs.copyFileSync(f.manifest, path.join(other, 'package.json'));
  assert.equal(f.call('context', request).error.code, 'PARAMETER_APP_AMBIGUOUS');
  fs.rmSync(path.join(f.users, 'account-b'), { recursive: true });
  fs.mkdirSync(path.dirname(f.packageDir) + '_temp');
  assert.equal(f.call('context', request).error.code, 'VIEW_MODE_READONLY');
  assert.equal(f.plan([upsert('safe', 'str')]).ok, true);
  assert.equal(f.call('plan', { package_dir: f.packageDir, app_id: appId,
    flow_id: '../process2', types, operations: [upsert('safe', 'str')] }).ok, false);
});

localTest('existing unknown types can be preserved or removed, but are not silently invented', t => {
  const f = fixture(t);
  const model = JSON.parse(f.before.toString('utf8').replace(/^\uFEFF/, ''));
  model.parameters[0].type = 'plugin.custom.Type';
  fs.writeFileSync(f.source, JSON.stringify(model));
  assert.equal(f.plan([upsert('keep', 'plugin.custom.Type')]).ok, true);
  assert.equal(f.plan([upsert('new_name', 'plugin.custom.Type')]).ok, false);
  assert.equal(f.plan([{ op: 'remove', name: 'keep' }]).ok, true);
});
