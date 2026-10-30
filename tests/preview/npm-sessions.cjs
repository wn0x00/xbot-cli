"use strict";
const {execFile} = require("node:child_process");
const {promisify} = require("node:util");
const {randomUUID} = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const assert = require("node:assert/strict");
const {createFixture} = require("./fixture.cjs");
const run = promisify(execFile);
const launcher = path.resolve(__dirname,"../../bin/xbot-cli.cjs");
const scratch = fs.mkdtempSync(path.join(os.tmpdir(),"xbot-npm-web-session-"));
const a = {...process.env,LOCALAPPDATA:scratch,XBOT_NO_UPDATE_NOTIFIER:"1"};
delete a.XBOT_WEB_PIPE; delete a.XBOT_WEB_TAB_ID; delete a.XBOT_WEB_SESSION;
const b = {...a,XBOT_WEB_SESSION:"second-"+randomUUID()};
const report = {launcher,cases:[],sdk_runtime_verified:false};
async function cli(env,...args) {
  const {stdout} = await run(process.execPath,[launcher,...args,"--json"],{
    env,windowsHide:true,maxBuffer:8*1024*1024
  });
  return JSON.parse(stdout).data;
}
function pass(name) { report.cases.push({name,passed:true}); console.log("PASS "+name); }
async function main() {
  const fixture = await createFixture();
  let openedA,openedB;
  try {
    openedA = await cli(a,"web","open",fixture.url);
    const heading = await run(process.execPath,[launcher,"web","active","--no-snapshot","--format","text"],{
      env:a,windowsHide:true,maxBuffer:1024*1024
    });
    assert.ok(heading.stdout.includes("影刀 Python："));
    assert.ok(!heading.stdout.includes("独立预览版"));
    pass("npm command renders exactly the integrated Python heading");
    openedB = await cli(b,"web","open",fixture.url+"?npm-second=1");
    assert.notEqual(openedA.browser_context.tab_id,openedB.browser_context.tab_id);
    const snapshot = await cli(a,"web","snapshot");
    assert.equal(snapshot.browser_context.tab_id,openedA.browser_context.tab_id);
    const id = snapshot.elements.find(e=>e.name==="Input DOM" && e.role==="textbox")?.ref;
    assert.ok(id);
    const filled = await cli(a,"web","fill",id,"NPM_BOUND_ORIGINAL","--no-snapshot");
    assert.equal(filled.browser_context.tab_id,openedA.browser_context.tab_id);
    assert.equal(filled.python.status,"ready");
    pass("successive short-lived npm launchers retain the original caller's page");
    const valueA = await cli(a,"web","eval",'return document.querySelector("#input").value;');
    const valueB = await cli(b,"web","eval",'return document.querySelector("#input").value;');
    assert.equal(valueA.result,"NPM_BOUND_ORIGINAL");
    assert.equal(valueB.result,"");
    pass("active-tab changes cannot redirect npm ref input into another session");
    const records = fs.readdirSync(path.join(scratch,"xbot-cli","web-sessions"));
    assert.equal(records.length,2);
    assert.ok(records.some(name=>name.startsWith("p"+process.pid+"-")));
    assert.ok(!fs.existsSync(path.join(scratch,"xbot-cli-python-preview")));
    pass("integrated cache uses the calling process identity and stable namespace");
    report.passed = true;
  } finally {
    if(openedB)await cli(b,"web","close").catch(()=>{});
    if(openedA)await cli(a,"web","close").catch(()=>{});
    await fixture.close();
    fs.mkdirSync("dist",{recursive:true});
    fs.writeFileSync("dist/web-local-npm-sessions.json",JSON.stringify(report,null,2)+"\n");
    const resolved = fs.realpathSync(scratch);
    const temporary = fs.realpathSync(os.tmpdir());
    assert.equal(path.dirname(resolved),temporary);
    assert.ok(path.basename(resolved).startsWith("xbot-npm-web-session-"));
    fs.rmSync(resolved,{recursive:true,force:true});
  }
}
main().catch(error=>{console.error(error.stderr||error.stack);process.exitCode=1;});
