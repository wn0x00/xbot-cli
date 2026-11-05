'use strict';

const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { npm } = require('./common.cjs');
const { verify, directory, digest } = require('./release-artifacts.cjs');

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

function matchingNpmVersion(metadata, release) {
  assert.equal(metadata.name, release.name, 'Registry package identity differs');
  assert.equal(metadata.version, release.version, 'Registry version differs');
  assert.equal(metadata.dist?.integrity, release.npm.integrity,
    'Published npm version has different content; refusing to overwrite or continue');
  return true;
}

async function registryVersion(release, request = fetch) {
  const url = `https://registry.npmjs.org/${encodeURIComponent(release.name)}/${encodeURIComponent(release.version)}`;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const response = await request(url, { headers: { accept: 'application/json', 'cache-control': 'no-cache' }, signal: AbortSignal.timeout(30000) });
    if (response.status === 404) return null;
    if (response.ok) return response.json();
    if ((response.status === 429 || response.status >= 500) && attempt < 3) {
      await sleep(2000 * (attempt + 1));
      continue;
    }
    throw new Error(`npm registry returned HTTP ${response.status}`);
  }
}

async function publishNpm(release, dependencies = {}) {
  const lookup = dependencies.lookup || registryVersion;
  const wait = dependencies.sleep || sleep;
  const publish = dependencies.publish || (() => npm([
    'publish', join(directory, release.npm.filename), '--access', 'public', '--ignore-scripts',
    '--provenance', '--registry=https://registry.npmjs.org/',
  ]));
  const existing = await lookup(release);
  if (existing) {
    matchingNpmVersion(existing, release);
    console.log(`${release.name}@${release.version} already published with matching integrity.`);
    return;
  }
  let publishError;
  try { await publish(); } catch (error) { publishError = error; }
  // Accepted versions can take time to appear. Check before retrying any publish.
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const metadata = await lookup(release);
    if (metadata) {
      matchingNpmVersion(metadata, release);
      console.log(`Verified npm registry integrity for ${release.name}@${release.version}.`);
      return;
    }
    if (attempt < 59) await wait(5000);
  }
  throw publishError || new Error('npm accepted the publish, but registry verification timed out. Rerun this job to check before retrying.');
}

function matchingAsset(asset, data, expected) {
  assert.equal(asset.name, expected.name, 'GitHub asset identity differs');
  assert.equal(data.length, expected.size, `Existing ${expected.name} has a different size; refusing to replace it`);
  assert.equal(digest(data), expected.sha256, `Existing ${expected.name} has different content; refusing to replace it`);
  return true;
}

function releaseNotes(changelog, version) {
  const section = changelog.split(/(?=^## \[)/m)
    .find(section => section.startsWith(`## [${version}]`));
  assert.ok(section, `Missing changelog section for ${version}`);
  const notes = section.slice(section.indexOf('\n') + 1).trim();
  assert.ok(notes, `Empty changelog section for ${version}`);
  return notes;
}

async function publishGithub(release) {
  assert.ok(process.env.GITHUB_TOKEN, 'GitHub release job token is required');
  const base = `https://api.github.com/repos/${release.repository}`;
  const headers = {
    authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
    accept: 'application/vnd.github+json',
    'x-github-api-version': '2022-11-28',
    'user-agent': 'xbot-cli-release',
  };
  async function request(url, options = {}) {
    const response = await fetch(url, { ...options, headers: { ...headers, ...options.headers }, signal: AbortSignal.timeout(120000) });
    if (!response.ok && response.status !== 404) throw new Error(`GitHub API returned HTTP ${response.status}`);
    return response;
  }
  const response = await request(`${base}/releases/tags/${encodeURIComponent(release.tag)}`);
  const published = response.status !== 404 ? await response.json() : await (await request(`${base}/releases`, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      tag_name: release.tag, target_commitish: release.commit, name: release.tag,
      body: releaseNotes(readFileSync(join(__dirname, '..', 'CHANGELOG.md'), 'utf8'), release.version),
    }),
  })).json();
  assert.equal(published.tag_name, release.tag, 'GitHub Release tag differs');
  assert.equal(published.draft, false, 'Release is a draft; publish it before uploading release artifacts');
  const assets = await (await request(`${base}/releases/${published.id}/assets?per_page=100`)).json();
  for (const expected of release.assets) {
    const existing = assets.find(asset => asset.name === expected.name);
    if (existing) {
      const downloaded = await request(`${base}/releases/assets/${existing.id}`, { headers: { accept: 'application/octet-stream' } });
      matchingAsset(existing, Buffer.from(await downloaded.arrayBuffer()), expected);
      console.log(`GitHub asset already matches: ${expected.name}`);
      continue;
    }
    const uploaded = await (await request(`https://uploads.github.com/repos/${release.repository}/releases/${published.id}/assets?name=${encodeURIComponent(expected.name)}`, {
      method: 'POST', headers: { 'content-type': 'application/octet-stream' }, body: readFileSync(join(directory, expected.name)),
    })).json();
    const downloaded = await request(`${base}/releases/assets/${uploaded.id}`, { headers: { accept: 'application/octet-stream' } });
    matchingAsset(uploaded, Buffer.from(await downloaded.arrayBuffer()), expected);
    console.log(`Uploaded and verified GitHub asset: ${expected.name}`);
  }
  console.log(`GitHub Release: ${published.html_url}`);
}

if (require.main === module) {
  (async () => {
    const release = verify();
    if (process.argv[2] === 'npm') await publishNpm(release);
    else if (process.argv[2] === 'github') await publishGithub(release);
    else throw new Error('Usage: node scripts/release-publish.cjs npm|github');
  })().catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}

module.exports = { matchingNpmVersion, registryVersion, publishNpm, matchingAsset, releaseNotes };
