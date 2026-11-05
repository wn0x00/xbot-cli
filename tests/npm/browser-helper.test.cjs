'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { sourceHash, verify } = require('../../scripts/build-browser-query.cjs');

test('embedded browser-query helper matches its source and generated asset hashes', () => {
  assert.match(sourceHash(), /^[a-f0-9]{64}$/);
  assert.doesNotThrow(verify);
  const bytes = readFileSync(join(__dirname, '../../src/browser_assets/BrowserQuery.dll'));
  assert.equal(bytes.toString('ascii', 0, 2), 'MZ');
  assert.ok(bytes.includes(Buffer.from('PluginManager')));
  assert.ok(bytes.includes(Buffer.from('IsInstalledExtension')));
  assert.equal(bytes.includes(Buffer.from('C:\\Users\\yingdao')), false);
});
