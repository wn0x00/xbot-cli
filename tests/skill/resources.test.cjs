'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '../../skills');
const names = ['xbot-app-dev', 'xbot-development-standards'];

function files(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const target = path.join(directory, entry.name);
    return entry.isDirectory() ? files(target) : [target];
  });
}

test('bundled Skills keep local references within the installed pair', () => {
  const missing = [];
  for (const name of names) {
    for (const file of files(path.join(root, name)).filter(file => file.endsWith('.md'))) {
      const text = fs.readFileSync(file, 'utf8');
      for (const match of text.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
        const link = match[1].replace(/^<|>$/g, '');
        if (/^[a-z][a-z\d+.-]*:/i.test(link) || link.startsWith('#')) continue;
        const target = path.resolve(path.dirname(file), decodeURIComponent(link.split('#')[0]));
        const relative = path.relative(root, target);
        assert.ok(relative && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative),
          `Reference leaves installed Skills: ${file}: ${link}`);
        if (!fs.existsSync(target) || !fs.statSync(target).isFile()) missing.push(`${file}: ${link}`);
      }
    }
  }
  assert.deepEqual(missing, [], 'Skill references must ship with the installed pair');
});
