'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { inflateRawSync } = require('node:zlib');
const { crc32, zip, digest } = require('../../scripts/release-artifacts.cjs');
const { matchingNpmVersion, registryVersion, publishNpm, matchingAsset, releaseNotes } = require('../../scripts/release-publish.cjs');

const release = {
  name: '@guanzhu.me/xbot-cli', version: '0.6.1',
  npm: { filename: 'guanzhu.me-xbot-cli-0.6.1.tgz', integrity: 'sha512-example' },
};
const metadata = { name: release.name, version: release.version, dist: { integrity: release.npm.integrity } };

test('GitHub release notes contain only the requested changelog section', () => {
  const changelog = '# Changelog\r\n\r\n## [0.9.2](compare)\r\n\r\n### Added\r\n\r\n- Browser inventory\r\n\r\n## [0.9.1](compare)\r\n\r\n- Previous release\r\n';
  assert.equal(releaseNotes(changelog, '0.9.2'), '### Added\r\n\r\n- Browser inventory');
  assert.equal(releaseNotes(changelog, '0.9.1'), '- Previous release');
  assert.throws(() => releaseNotes(changelog, '0.9'), /Missing changelog section/);
  assert.throws(() => releaseNotes('## [0.9.2](compare)\n\n', '0.9.2'), /Empty changelog section/);
});

test('release ZIP is reproducible and each entry can be extracted with matching CRC', () => {
  const entries = [['xbot-cli.exe', Buffer.from([0, 1, 2, 255])], ['README.md', Buffer.from('中文\n')], ['LICENSE', Buffer.from('MIT\n')]];
  const archive = zip(entries);
  assert.deepEqual(zip(entries), archive);
  let offset = 0;
  for (const [name, data] of entries) {
    assert.equal(archive.readUInt32LE(offset), 0x04034b50);
    assert.equal(archive.readUInt16LE(offset + 12), 33);
    const size = archive.readUInt32LE(offset + 18);
    const nameSize = archive.readUInt16LE(offset + 26);
    assert.equal(archive.subarray(offset + 30, offset + 30 + nameSize).toString(), name);
    const extracted = inflateRawSync(archive.subarray(offset + 30 + nameSize, offset + 30 + nameSize + size));
    assert.deepEqual(extracted, data);
    assert.equal(archive.readUInt32LE(offset + 14), crc32(extracted));
    offset += 30 + nameSize + size;
  }
  assert.equal(archive.readUInt32LE(offset), 0x02014b50);
  assert.equal(archive.readUInt32LE(archive.length - 22), 0x06054b50);
  assert.throws(() => zip([['../escape', Buffer.alloc(0)]]));
});

test('npm retry skips an identical published version and rejects different content', async () => {
  let published = 0;
  await publishNpm(release, { lookup: async () => metadata, publish: () => { published += 1; } });
  assert.equal(published, 0);
  const different = { ...metadata, dist: { integrity: 'sha512-different' } };
  await assert.rejects(publishNpm(release, { lookup: async () => different, publish: () => { published += 1; } }), /different content/);
  assert.equal(published, 0);
  assert.throws(() => matchingNpmVersion({ ...metadata, name: 'unexpected' }, release), /identity differs/);
});

test('npm publish waits for registry propagation and verifies the accepted version', async () => {
  let lookups = 0;
  let published = 0;
  let waits = 0;
  await publishNpm(release, {
    lookup: async () => (++lookups >= 4 ? metadata : null),
    publish: () => { published += 1; },
    sleep: async () => { waits += 1; },
  });
  assert.equal(published, 1);
  assert.equal(waits, 2);
});

test('ambiguous npm response is recovered only when registry integrity matches', async () => {
  let lookups = 0;
  await publishNpm(release, {
    lookup: async () => (++lookups === 1 ? null : metadata),
    publish: () => { throw new Error('connection closed'); },
  });
  await assert.rejects(publishNpm(release, {
    lookup: async () => null,
    publish: () => { throw new Error('rejected'); },
    sleep: async () => {},
  }), /rejected/);
});

test('registry absence is distinguished from authorization and protocol failures', async () => {
  assert.equal(await registryVersion(release, async () => new Response('', { status: 404 })), null);
  await assert.rejects(registryVersion(release, async () => new Response('', { status: 403 })), /403/);
  assert.deepEqual(await registryVersion(release, async (url, options) => {
    assert.match(url, /%40guanzhu.me%2Fxbot-cli\/0.6.1$/);
    assert.equal(options.headers.authorization, undefined);
    return Response.json(metadata);
  }), metadata);
});

test('GitHub retry accepts identical asset bytes and rejects changed bytes', () => {
  const bytes = Buffer.from('tested archive');
  const asset = { name: 'release.tgz', size: bytes.length, sha256: digest(bytes) };
  assert.equal(matchingAsset(asset, bytes, asset), true);
  assert.throws(() => matchingAsset(asset, Buffer.from('changed data'), asset), /different size/);
  assert.throws(() => matchingAsset(asset, Buffer.from('TESTED ARCHIVE'), asset), /different content/);
});
