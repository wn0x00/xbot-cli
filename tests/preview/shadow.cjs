"use strict";
const {execFile,spawnSync} = require("node:child_process");
const {promisify} = require("node:util");
const {randomUUID} = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const {createFixture} = require("./fixture.cjs");
const run = promisify(execFile);
const executable = path.resolve(process.argv[2] || "target/debug/xbot-cli.exe");
const env = {...process.env, XBOT_WEB_SESSION:"shadow-" + randomUUID()};
delete env.XBOT_WEB_PIPE; delete env.XBOT_WEB_TAB_ID;
const report = {cases:[]};
async function cli(...args) {
  const {stdout} = await run(executable,[...args,"--json"],{env,windowsHide:true,maxBuffer:8*1024*1024});
  return JSON.parse(stdout).data;
}
async function evaluate(code) { return (await cli("web","eval",code)).result; }
function pass(name) { report.cases.push({name,passed:true}); console.log("PASS "+name); }
async function main() {
  const fixture = await createFixture(); let opened;
  try {
    opened = await cli("web","open",fixture.url);
    await evaluate(`const root=document.createElement("lab-comments");
      document.body.append(root);const comments=root.attachShadow({mode:"open"});
      comments.innerHTML='<lab-header></lab-header><lab-thread></lab-thread><lab-thread></lab-thread>';
      const header=comments.querySelector("lab-header").attachShadow({mode:"open"});
      for(const label of ["Hot posts","New posts"]){
        const host=document.createElement("lab-text-button");host.textContent=label;header.append(host);
        host.attachShadow({mode:"open"}).innerHTML='<button><slot></slot></button>';
      }
      comments.querySelectorAll("lab-thread").forEach((thread,i)=>{
        const r=thread.attachShadow({mode:"open"});r.innerHTML='<lab-reply></lab-reply>';
        const reply=r.querySelector("lab-reply").attachShadow({mode:"open"});
        reply.innerHTML='<div><a data-user-id="'+(i+10)+'" href="/user/'+(i+10)+'">Author</a><lab-actions></lab-actions></div>';
        const actions=reply.querySelector("lab-actions").attachShadow({mode:"open"});
        actions.innerHTML='<button>Reply</button><lab-icon icon="BDC/hand_thumbsup_line/1"></lab-icon>';
        actions.querySelector("lab-icon").attachShadow({mode:"open"}).innerHTML='<svg id="icon" xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20"/></svg>';
      });
      const peer=document.createElement("lab-peer");document.body.append(peer);
      peer.attachShadow({mode:"open"}).innerHTML='<div><a data-customer-id="alpha">Customer</a><button onclick="this.dataset.count=String(Number(this.dataset.count||0)+1)">Save peer</button></div><div><a data-customer-id="beta">Customer</a><button>Save peer</button></div>';
      return true;`);
    const snapshot=await cli("web","snapshot");
    const hot=snapshot.elements.find(e=>e.name==="Hot posts" && e.role==="button");
    const fresh=snapshot.elements.find(e=>e.name==="New posts" && e.role==="button");
    assert.ok(hot && fresh);
    for(const el of [hot,fresh]){
      const result=await cli("web","xpath",el.ref);
      assert.equal(result.status,"ready");assert.equal(result.validation,"requires_shadowbot");
      assert.ok(result.xpaths.every(p=>p.includes(el.name) && !/\[\d+\]/.test(p)), JSON.stringify(result));
      assert.ok(result.candidates.every(c=>c.count===1 && c.dom_identity_verified && !c.unique_verified));
    }
    pass("attribute-less nested custom hosts and slotted buttons generate single-element ShadowBot strings");
    let result=await cli("web","xpath",hot.ref,fresh.ref);
    assert.equal(result.status,"ready");assert.ok(result.candidates.every(c=>c.count===2));
    pass("same-type buttons across sibling open ShadowRoots generate a shared component XPath");
    const evidence=await evaluate(`const root=document.querySelector("lab-comments").shadowRoot;
      const thread=root.querySelector("lab-thread");
      const reply=thread.shadowRoot.querySelector("lab-reply").shadowRoot;
      const actions=reply.querySelector("lab-actions");
      const icon=actions.shadowRoot.querySelector("lab-icon");
      const svg=icon.shadowRoot.querySelector("svg");
      return {reply:window.__xbotPythonXPaths([actions.shadowRoot.querySelector("button")]),
        icon:window.__xbotPythonXPaths([svg])};`);
    for(const r of [evidence.reply,evidence.icon]){
      assert.ok(r.candidates.length>0);
      assert.ok(r.candidates.every(c=>c.count===1 && c.xpath.includes("data-user-id")));
    }
    pass("repeated host chains are disambiguated by a root-local sibling business key");
    const changed=await evaluate(`const root=document.querySelector("lab-comments").shadowRoot;
      const thread=root.querySelector("lab-thread");
      const reply=thread.shadowRoot.querySelector("lab-reply").shadowRoot;
      const icon=reply.querySelector("lab-actions").shadowRoot.querySelector("lab-icon");
      icon.setAttribute("icon","BDC/hand_thumbsup_fill/1");
      return window.__xbotPythonXPaths([icon.shadowRoot.querySelector("svg")]);`);
    assert.ok(changed.candidates.every(c=>!c.xpath.includes("thumbsup_fill") && !c.xpath.includes("thumbsup_line")));
    pass("icon line/fill state is not frozen into a supposedly stable selector");
    const ambiguous=await evaluate(`const root=document.querySelector("lab-comments").shadowRoot;
      let svg;for(const thread of root.querySelectorAll("lab-thread")){
        const reply=thread.shadowRoot.querySelector("lab-reply").shadowRoot;
        reply.querySelector("a").setAttribute("data-user-id","same-author");
        reply.querySelector("a").setAttribute("href","/user/same-author");
        svg=reply.querySelector("lab-actions").shadowRoot.querySelector("lab-icon").shadowRoot.querySelector("svg");
      }return window.__xbotPythonXPaths([svg]);`);
    assert.ok(ambiguous.candidates.length>0);
    assert.ok(ambiguous.candidates.every(c=>c.count===2 && c.requires_review));
    pass("indistinguishable icons retain ambiguity counts and require review, not positional fallback");
    const ids=await evaluate(`return Array.from(window.__xbotPreviewRefs.entries())
      .filter(([id,n])=>n.localName==="svg" && n.getRootNode().host?.localName==="lab-icon").map(([id])=>id);`);
    assert.equal(ids.length,2);
    result=await cli("web","xpath",ids[0]);
    assert.equal(result.status,"needs_review");assert.equal(result.requires_review,true);
    const text=await run(executable,["web","xpath",ids[0],"--format","text"],{env,windowsHide:true});
    assert.match(text.stdout,/未唯一定位/);assert.match(text.stdout,/影刀语法，请使用影刀验证/);
    assert.ok(result.xpaths.every(p=>!p.startsWith("[")));
    pass("CLI text and JSON expose ambiguity and never serialize a shadow XPath as a segment array");
    const peerRef=snapshot.elements.filter(e=>e.name==="Save peer" && e.role==="button")[0].ref;
    const clicked=await cli("web","click",peerRef,"--no-snapshot");
    assert.equal(clicked.python.status,"ready");
    assert.ok(clicked.python.locator.steps.at(-1).query.some(p=>p.filters.some(f=>f.kind==="has-plan")));
    const python=process.env.PREVIEW_PYTHON||"D:/Program Files/ShadowBot/shadowbot-6.3.39/python/python.exe";
    const inspected=spawnSync(python,[path.join(__dirname,"inspect_python.py")],{
      input:clicked.python.code,encoding:"utf8",windowsHide:true,env:{...process.env,PYTHONIOENCODING:"utf-8"}});
    assert.equal(inspected.status,0,inspected.stderr);
    const adapter=JSON.parse(inspected.stdout).calls[0];
    assert.equal(adapter.method,"execute_javascript");
    await evaluate(`(${adapter.code})(null,${JSON.stringify(adapter.argument)});return true;`);
    assert.equal(await evaluate('return document.querySelector("lab-peer").shadowRoot.querySelector("button").dataset.count;'),"2");
    pass("root-local sibling-key query filters replay consistently in generated JavaScript adapters");
    // Keep actual nodes from open roots, then move them under a closed boundary.
    // This exercises real, still-connected snapshot refs without inventing ref IDs.
    const closedSetup=await evaluate(`const closedHost=document.createElement("lab-closed");
      document.body.append(closedHost);
      const closed=closedHost.attachShadow({mode:"closed"});
      const plain=document.createElement("button");plain.textContent="Closed target";closed.append(plain);
      const openHost=document.createElement("lab-open-inside-closed");closed.append(openHost);
      const inner=openHost.attachShadow({mode:"open"});
      const innerButton=document.createElement("button");innerButton.textContent="Nested closed target";inner.append(innerButton);
      const openOuter=document.createElement("lab-open-outer");document.body.append(openOuter);
      const outer=openOuter.attachShadow({mode:"open"});
      const innerClosedHost=document.createElement("lab-inner-closed");outer.append(innerClosedHost);
      const innerClosed=innerClosedHost.attachShadow({mode:"closed"});
      const closedInnerButton=document.createElement("button");closedInnerButton.textContent="Closed inside open";innerClosed.append(closedInnerButton);
      window.__closedLab={closed,inner,plain,innerButton,closedInnerButton};
      return [plain,innerButton,closedInnerButton].map(el=>({
        xpath:window.__xbotPythonXPaths([el]),locator:window.__xbotPythonLocator(el)
      }));`);
    for(const item of closedSetup){
      for(const blocked of [item.xpath,item.locator]){
        assert.equal(blocked.error_code,"WEB_SHADOW_CLOSED");
        assert.equal(blocked.closed_shadow,true);
        assert.deepEqual(blocked.candidates,[]);
        assert.deepEqual(blocked.steps,[]);
        assert.match(blocked.reason,/不支持 shadow-root\(closed\)/);
        assert.match(blocked.reason,/无法生成 XPath/);
        assert.ok(!JSON.stringify(blocked).includes('"xpath":'));
      }
    }
    pass("closed, open-inside-closed and closed-inside-open nodes are blocked before path generation");
    const mixedClosed=await evaluate(`return window.__xbotPythonXPaths([
      window.__closedLab.plain,document.querySelector("lab-peer").shadowRoot.querySelector("button")]);`);
    assert.equal(mixedClosed.error_code,"WEB_SHADOW_CLOSED");
    assert.deepEqual(mixedClosed.candidates,[]);
    const ordinary=await evaluate(`const b=document.createElement("button");b.id="ordinary-shadow-null";
      b.textContent="Ordinary button";document.body.append(b);
      return window.__xbotPythonXPaths([b]);`);
    assert.ok(ordinary.candidates.length>0);
    pass("any closed member blocks a selection; shadowRoot=null on an ordinary element is not misclassified");
    await evaluate(`const el=window.__xbotPreviewRefs.get(${JSON.stringify(hot.ref)});
      window.__closedLab.inner.append(el);return el.isConnected;`);
    result=await cli("web","xpath",hot.ref);
    assert.equal(result.status,"unavailable");
    assert.equal(result.error_code,"WEB_SHADOW_CLOSED");
    assert.deepEqual(result.xpaths,[]);
    assert.deepEqual(result.candidates,[]);
    assert.ok(!result.local_xpaths && !result.xpath_lists);
    const closedText=await run(executable,["web","xpath",hot.ref,"--format","text"],{env,windowsHide:true});
    assert.match(closedText.stdout,/不支持 shadow-root\(closed\)/);
    assert.match(closedText.stdout,/无法生成 XPath/);
    assert.ok(!closedText.stdout.includes("/xbotShadowRoot"));
    pass("CLI refuses a live ref moved beneath closed ancestry with a prominent warning and no XPath leakage");
    report.passed=true;
  } finally {
    if(opened)await cli("web","close").catch(()=>{});
    await fixture.close();
    fs.mkdirSync("dist",{recursive:true});
    fs.writeFileSync("dist/preview-shadow.json",JSON.stringify(report,null,2)+"\n");
  }
}
main().catch(e=>{console.error(e.stderr||e.stack);process.exitCode=1;});
