'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '../../skills');
const names = ['xbot-cli-doc', 'xbot-activity-dev', 'xbot-activity-fix'];

function files(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const target = path.join(directory, entry.name);
    return entry.isDirectory() ? files(target) : [target];
  });
}

test('active Skill directory contains exactly the three installable bundles', () => {
  assert.deepEqual(fs.readdirSync(root).sort(), [...names].sort());
});

test('bundled Skills keep local references and anchors within the installed set', () => {
  const missing = [];
  for (const name of names) {
    for (const file of files(path.join(root, name)).filter(file => file.endsWith('.md'))) {
      const text = fs.readFileSync(file, 'utf8');
      for (const match of text.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
        const link = match[1].replace(/^<|>$/g, '');
        if (/^[a-z][a-z\d+.-]*:/i.test(link)) continue;
        const [local, fragment] = link.split('#');
        const target = local ? path.resolve(path.dirname(file), decodeURIComponent(local)) : file;
        const relative = path.relative(root, target);
        assert.ok(relative && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative),
          `Reference leaves installed Skills: ${file}: ${link}`);
        assert.ok(names.includes(relative.split(path.sep)[0]), `Reference leaves active bundle set: ${link}`);
        if (!fs.existsSync(target) || !fs.statSync(target).isFile()) {
          missing.push(`${file}: ${link}`);
        } else if (fragment) {
          const content = fs.readFileSync(target, 'utf8');
          const anchors = [...content.matchAll(/<a\s+id="([^"]+)"/g)].map(match => match[1]);
          for (const match of content.matchAll(/^#{1,6}\s+(.+)$/gm)) {
            anchors.push(match[1].trim().toLowerCase().replace(/[^\p{L}\p{N}\p{M}_\-\s]/gu, '').replace(/\s/g, '-'));
          }
          assert.ok(anchors.includes(decodeURIComponent(fragment)), `Missing anchor: ${file}: ${link}`);
        }
      }
    }
  }
  assert.deepEqual(missing, [], 'Skill references must ship with the installed set');
});
