"use strict";
const {execFile} = require("node:child_process");
const {promisify} = require("node:util");
const {randomUUID} = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const {createFixture} = require("./fixture.cjs");
const run = promisify(execFile);
const executable = path.resolve(process.argv[2] || "target/debug/xbot-cli.exe");
const base = {...process.env};
delete base.XBOT_WEB_PIPE; delete base.XBOT_WEB_TAB_ID;
const a = {...base, XBOT_WEB_SESSION: "test-a-" + randomUUID()};
const b = {...base, XBOT_WEB_SESSION: "test-b-" + randomUUID()};
const report = {executable, cases: []};
async function cli(env, ...args) {
  try {
    const {stdout} = await run(executable, [...args, "--json"], {env, windowsHide: true, maxBuffer: 4 * 1024 * 1024});
    return JSON.parse(stdout).data;
  } catch (error) {
    error.response = JSON.parse(error.stderr || "{}");
    throw error;
  }
}
async function evaluate(env, expression) { return (await cli(env, "web", "eval", "return " + expression + ";")).result; }
function ref(snapshot, label) {
  const id = snapshot.result.split("\n").find(l => l.includes(label))?.match(/\[ref=([^\]]+)\]/)?.[1];
  assert.ok(id, "Missing ref " + label);
  return id;
}
function pass(name) { report.cases.push({name, passed: true}); console.log("PASS " + name); }
async function main() {
  const fixture = await createFixture();
  let openedA, openedB;
  try {
    openedA = await cli(a, "web", "open", fixture.url);
    let id = ref(openedA.snapshot, 'textbox "Input DOM"');
    assert.ok(openedA.snapshot.elements.some(e => e.ref === id && e.role === "textbox"));
    openedB = await cli(b, "web", "open", fixture.url + "?second=1");
    assert.notEqual(openedA.browser_context.tab_id, openedB.browser_context.tab_id);
    const filled = await cli(a, "web", "fill", id, "BOUND_A", "--no-snapshot");
    assert.equal(filled.browser_context.tab_id, openedA.browser_context.tab_id);
    assert.equal(await evaluate(a, 'document.querySelector("#input").value'), "BOUND_A");
    assert.equal(await evaluate(b, 'document.querySelector("#input").value'), "");
    pass("active tab switch does not retarget old refs");

    await evaluate(a, '(()=>{const el=document.querySelector("#input");el._ariaRef={ref:"e999"};el._xbotPreviewAriaRef={ref:"e888",role:"textbox",name:"foreign"};window.__pwIS={foreign:true};return true;})()');
    await cli(a, "web", "fill", id, "MAP_SURVIVED", "--no-snapshot");
    assert.equal(await evaluate(a, 'document.querySelector("#input").value'), "MAP_SURVIVED");
    pass("private node map survives foreign expando and snapshot engine changes");

    await evaluate(a, '(()=>{const el=document.querySelector("#input"),next=el.cloneNode(true);next.value="";el.replaceWith(next);return true;})()');
    await assert.rejects(cli(a, "web", "fill", id, "DO_NOT_WRITE", "--no-snapshot"),
      e => e.response.error?.code === "WEB_REF_STALE");
    await assert.rejects(cli(a, "web", "xpath", id),
      e => e.response.error?.code === "WEB_REF_STALE");
    assert.equal(await evaluate(a, 'document.querySelector("#input").value'), "");
    pass("replaced node is rejected before fill, with explicit stale-ref error");

    let snapshot = await cli(a, "web", "snapshot");
    id = ref(snapshot, 'textbox "Input DOM"');
    await cli(a, "web", "fill", id, "RECOVERED", "--no-snapshot");
    assert.equal(await evaluate(a, 'document.querySelector("#input").value'), "RECOVERED");
    pass("fresh snapshot recovers a replaced textbox");

    await cli(a, "web", "reload", "--no-snapshot");
    await assert.rejects(cli(a, "web", "fill", id, "WRONG_DOCUMENT", "--no-snapshot"),
      e => e.response.error?.code === "WEB_REF_STALE");
    pass("navigation invalidates refs even if numeric ids might be reused");
    snapshot = await cli(a, "web", "snapshot");
    id = ref(snapshot, 'textbox "Input DOM"');
    await cli(a, "web", "fill", id, "NEW_DOCUMENT", "--no-snapshot");
    assert.equal(await evaluate(a, 'document.querySelector("#input").value'), "NEW_DOCUMENT");
    pass("fresh snapshot recovers after navigation");

    await evaluate(a, `(()=>{
      const group=document.createElement("section");group.setAttribute("aria-label","Complete tree");
      for(let i=0;i<80;i++){const b=document.createElement("button");b.textContent="Long item "+i+" "+"A".repeat(160);group.append(b);}
      const mixed=document.createElement("div");mixed.role="checkbox";mixed.setAttribute("aria-checked","mixed");mixed.textContent="Mixed state";group.append(mixed);
      document.body.append(group);return true;
    })()`);
    snapshot = await cli(a, "web", "snapshot");
    const tree = await run(executable, ["web", "snapshot", "--format", "text"], {env:a, windowsHide:true, maxBuffer:4*1024*1024});
    assert.ok(tree.stdout.includes("[ref="));
    assert.ok(!tree.stdout.includes("REF | 类型 | 名称"));
    assert.ok(tree.stdout.includes("├─") || tree.stdout.includes("└─"));
    report.tree_sample = tree.stdout.split(/\r?\n/).slice(0, 70).join("\n");
    assert.ok(tree.stdout.includes("Long item 79 " + "A".repeat(160)));
    assert.ok(tree.stdout.includes('[checked="mixed"]'));
    assert.ok(!tree.stdout.includes("部分显示"));
    const allRefs = [];
    function walk(node, ancestors = []) {
      if (node.ref) allRefs.push(node.ref);
      if (node.ref?.startsWith("f")) {
        const prefix=node.ref.split("e")[0];
        for (const host of node.shadow_hosts || []) {
          if (/^(f\d+)?e\d+$/.test(host)) assert.ok(host.startsWith(prefix), "Shadow host ref must use its frame scope");
        }
      }
      if (node.role === "document") {
        assert.equal(ancestors.at(-1)?.role, "iframe", "Frame document must be nested under its actual owner");
      }
      for (const child of node.children || []) walk(child, [...ancestors, node]);
    }
    walk(snapshot.tree);
    for (const id of allRefs) assert.ok(tree.stdout.includes("[ref=" + id + "]"), "Tree lost " + id);
    assert.ok(snapshot.frames.length >= 2);
    pass("complete tree retains every ref, >60 nodes, long names, mixed states and actual frame hierarchy");
    const limited = await run(executable, ["web", "snapshot", "--role", "textbox", "--limit", "2", "--format", "text"], {env:a, windowsHide:true});
    assert.match(limited.stdout, /筛选树/);
    assert.match(limited.stdout, /部分显示/);
    pass("filtering and line limits are explicit and clearly marked");
    report.passed = true;
  } finally {
    if (openedB) await cli(b, "web", "close").catch(() => {});
    if (openedA) await cli(a, "web", "close").catch(() => {});
    await fixture.close();
    fs.mkdirSync("dist", {recursive:true});
    fs.writeFileSync("dist/preview-sessions.json", JSON.stringify(report, null, 2) + "\n");
  }
}
main().catch(error => {console.error(error.stderr || error.stack); process.exitCode = 1;});
