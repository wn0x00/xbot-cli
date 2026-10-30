"use strict";
const {execFile} = require("node:child_process");
const {promisify} = require("node:util");
const {randomUUID} = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const run = promisify(execFile);
const executable = path.resolve(process.argv[2] || "target/debug/xbot-cli.exe");
const env = {...process.env, XBOT_WEB_SESSION:"baidu-" + randomUUID()};
delete env.XBOT_WEB_PIPE; delete env.XBOT_WEB_TAB_ID;
async function cli(...args) {
  const {stdout} = await run(executable,[...args,"--json"],{env,windowsHide:true,maxBuffer:8*1024*1024});
  return JSON.parse(stdout).data;
}
async function main() {
  let opened;
  try {
    opened=await cli("web","open","baidu.com");
    const ref=(await cli("web","eval",`return Array.from(window.__xbotPreviewRefs||[]).find(([ref,el])=>el.matches("#kw,#chat-textarea"))?.[0]||null;`)).result;
    assert.ok(ref,"Baidu search textbox is absent from the open snapshot");
    const filled=await cli("web","fill",ref,"\u5f71\u5200","--no-snapshot");
    assert.equal(filled.browser_context.tab_id,opened.browser_context.tab_id);
    assert.equal((await cli("web","eval",'return document.querySelector("#kw,#chat-textarea").value;')).result,"\u5f71\u5200");
    const xpath=await cli("web","xpath",ref);
    assert.equal(xpath.status,"ready");
    fs.mkdirSync("dist",{recursive:true});
    fs.writeFileSync("dist/preview-baidu.json",JSON.stringify({
      passed:true,executable,ref,tab_id:opened.browser_context.tab_id,
      search_submitted:false,python:filled.python,xpaths:xpath.xpaths
    },null,2)+"\n");
    console.log("PASS Baidu open -> fill "+ref+" -> read back; no search submitted");
    console.log(JSON.stringify(xpath.xpaths));
  } finally {
    if(opened)await cli("web","close").catch(()=>{});
  }
}
main().catch(error=>{console.error(error.stderr||error.stack);process.exitCode=1;});
