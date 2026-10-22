'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { gzipSync } = require('node:zlib');
const root = path.resolve(__dirname, '../../skills/xbot-app-dev');
const repair = require(path.join(root, 'scripts/locator-repair.cjs'));
const usage = require(path.join(root, 'scripts/locator-usage.cjs'));
const python = process.env.XBOT_TEST_PYTHON || 'python';
const locator = "//button[@name='approve']";
const asset = {
  app_id: 'test-app', version: 1,
  generated: { primary: { xpath: [locator] }, alternatives: [] },
};
const assetHash = usage.hash(asset);
const prefix = `LOCATOR = ${JSON.stringify(locator)}\n`;
const single = prefix + 'target = browser.find_by_xpath(LOCATOR)\n';
const collection = prefix + 'targets = browser.find_all_by_xpath(LOCATOR)\n';

function capture(text, extra = {}) {
  return usage.makeInput({
    appId: 'test-app', flowId: 'test-flow', locator, asset, assetHash, python,
    source: { kind: 'codeflow', text }, ...extra,
  });
}

function analysis(input, classification = 'single', consumer = 'one') {
  const lines = input.source.text.split('\n');
  return {
    analysis_id: input.analysis_id, classification, consumer,
    callsite_ids: input.observations.calls.map(call => call.id),
    evidence: input.observations.calls.map(call => ({
      callsite_id: call.id, line_start: call.line_start, line_end: call.line_end,
      quote: lines.slice(call.line_start - 1, call.line_end).join('\n'),
    })),
    reasoning: 'Classify the observed query API; no business execution is authorized.',
    uncertainties: [],
  };
}

test('repair helper rejects writes and unverified capture profiles', () => {
  for (const action of ['click', 'commit', 'apply', 'publish']) {
    assert.throws(() => repair.parse([action]), { code: 'BAD_ARGS' });
  }
  assert.throws(() => repair.parse(['capture', '--profile', 'generic', '--ref', 'e1',
    '--robula-source', 'index.ts', '--out', 'result.json']), { code: 'BAD_ARGS' });
  assert.throws(() => repair.parse(['capture', '--profile', 'data-contract-v1', '--ref', '//button',
    '--robula-source', 'index.ts', '--out', 'result.json']), { code: 'BAD_ARGS' });
});

test('only full snapshots expose deduplicated refs in the requested role', () => {
  assert.throws(() => repair.freshRefs({ acc_tree_meta: { kind: 'diff' }, result: '' }),
    { code: 'FULL_SNAPSHOT_REQUIRED' });
  assert.deepEqual(repair.freshRefs({
    acc_tree_meta: { kind: 'full' },
    result: '- button "Save" [e1]\n- textbox "Search" [e2]\n- button "Save" [f1e3]\n- button "Save" [e1]',
  }, 'button'), ['e1', 'f1e3']);
});

test('compressed evidence roundtrips and truncated data is rejected', () => {
  const evidence = { status: 'DIAGNOSED', entries: Array.from({ length: 80 }, (_, i) => ({ id: i })) };
  const compressed = 'gz:' + gzipSync(JSON.stringify(evidence)).toString('base64');
  assert.deepEqual(repair.decode(compressed), evidence);
  assert.throws(() => repair.decode(compressed.slice(0, -24)), { code: 'EVALUATION_RESPONSE_INVALID' });
  assert.throws(() => repair.decode(JSON.stringify(evidence)), { code: 'EVALUATION_RESPONSE_INVALID' });
});

test('altered third-party source is rejected before browser evaluation', () => {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'xbot-robula-gate-'));
  const file = path.join(scratch, 'index.ts');
  try {
    fs.writeFileSync(file, 'export class RobulaPlus { altered = true; }\n');
    assert.throws(() => repair.loadSources(file), { code: 'ROBULA_SOURCE_MISMATCH' });
  } finally {
    fs.unlinkSync(file);
    fs.rmdirSync(scratch);
  }
});

for (const [name, text, modes, complete] of [
  ['single lookup', single, ['single'], true],
  ['collection lookup', collection, ['collection'], true],
  ['comment is not a call', '# browser.find_all_by_xpath(LOCATOR)\n' + single, ['single'], true],
  ['reassignment is unresolved', single + 'LOCATOR = "//other"\n', [], false],
  ['method alias is unresolved', prefix + 'lookup = browser.find_by_xpath\ntarget = lookup(LOCATOR)\n', [], false],
  ['dynamic argument is unresolved', prefix + 'target = browser.find_by_xpath(args["xpath"])\n', [], false],
]) {
  test('Python AST: ' + name, () => {
    const input = capture(text);
    assert.deepEqual(input.observations.calls.map(call => call.query_mode), modes);
    assert.equal(input.observations.complete, complete);
  });
}

test('collection filtered to one cannot enter the single-element repairer', () => {
  const input = capture(collection + 'selected = [item for item in targets if item.get_text() == key]\n');
  assert.throws(() => usage.assess(input, analysis(input), python), { code: 'USAGE_CLASSIFICATION_CONFLICT' });
  const result = analysis(input, 'collection_then_single', 'one_by_business_key');
  const decision = usage.assess(input, result, python);
  assert.equal(decision.status, 'USAGE_ANALYZED');
  assert.equal(decision.single_repair_allowed, false);
  assert.throws(() => usage.authorize({
    kind: 'locator_usage_receipt', input, analysis: result, decision,
  }, asset, assetHash, () => ({ content: input.source.text }), python),
  { code: 'COLLECTION_REPAIR_NOT_SUPPORTED' });
});

test('mixed callsites cannot be hidden by omitting collection evidence', () => {
  const input = capture(single + 'targets = browser.find_all_by_xpath(LOCATOR)\n');
  const report = analysis(input, 'mixed', 'mixed');
  assert.equal(usage.assess(input, report, python).status, 'REVIEW_REQUIRED_MIXED_USAGE');
  report.callsite_ids.pop();
  assert.throws(() => usage.assess(input, report, python), { code: 'USAGE_CALLSITE_COVERAGE' });
});

test('fabricated source quotes and changed syntax observations are rejected', () => {
  const input = capture(single);
  const report = analysis(input);
  report.evidence[0].quote = 'target = browser.find_all_by_xpath(LOCATOR)';
  assert.throws(() => usage.assess(input, report, python), { code: 'USAGE_EVIDENCE_MISMATCH' });
  const modified = capture(collection);
  modified.observations.calls[0].query_mode = 'single';
  const { analysis_id, ...body } = modified;
  modified.analysis_id = usage.hash(body);
  assert.throws(() => usage.assess(modified, analysis(modified), python), { code: 'USAGE_SIGNALS_CHANGED' });
});

test('missing historical identity binding and uncertainty stay review-only', () => {
  const unbound = capture(single, { asset: null, assetHash: null });
  assert.equal(usage.assess(unbound, analysis(unbound), python).single_repair_allowed, false);
  const input = capture(single);
  const report = analysis(input);
  report.uncertainties.push('Receiver and business identity require confirmation.');
  assert.equal(usage.assess(input, report, python).status, 'REVIEW_REQUIRED_USAGE_UNCERTAIN');
});

test('handoff rereads source and rejects changed workflow or asset', () => {
  const input = capture(single);
  const report = analysis(input);
  const receipt = { kind: 'locator_usage_receipt', input, analysis: report };
  assert.equal(usage.authorize(receipt, asset, assetHash, () => ({ content: single }), python).classification,
    'single');
  assert.throws(() => usage.authorize(receipt, asset, assetHash,
    () => ({ content: collection }), python), { code: 'USAGE_FLOW_CHANGED' });
  assert.throws(() => usage.authorize(receipt, { ...asset, version: 2 }, assetHash,
    () => ({ content: single }), python), { code: 'USAGE_ASSET_CHANGED' });
});

test('missing usage receipt fails before any CLI call', () => {
  let calls = 0;
  assert.throws(() => repair.run(['propose', '--asset', 'asset.json',
    '--robula-source', 'index.ts', '--out', 'result.json'], () => { calls++; }),
  { code: 'USAGE_ANALYSIS_REQUIRED' });
  assert.equal(calls, 0);
});
