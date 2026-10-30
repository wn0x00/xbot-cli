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
const env = {...process.env, XBOT_WEB_SESSION:"xpath-" + randomUUID()};
delete env.XBOT_WEB_PIPE; delete env.XBOT_WEB_TAB_ID;
const report = {executable, cases:[]};
async function cli(...args) {
  const {stdout} = await run(executable, [...args, "--json"], {env, windowsHide:true, maxBuffer:8*1024*1024});
  return JSON.parse(stdout).data;
}
async function evaluate(expression) { return (await cli("web", "eval", "return " + expression + ";")).result; }
function ref(snapshot, label) {
  const matches = snapshot.elements.filter(e => e.name === label);
  assert.equal(matches.length, 1, "Ambiguous/missing ref: " + label);
  return matches[0].ref;
}
function pass(name) { report.cases.push({name, passed:true}); console.log("PASS " + name); }
function ready(result) {
  assert.equal(result.status, "ready", JSON.stringify(result));
  assert.ok(result.xpaths.length >= 1 && result.xpaths.length <= 5);
  assert.ok(!("applied" in result), "XPath is read-only, not an applied action");
}
async function verify(result, selectors) {
  return evaluate(`(()=>{
    const paths=${JSON.stringify(result.xpaths)}, selectors=${JSON.stringify(selectors)};
    return paths.map(line=>{
      const segments=line.startsWith("[")?JSON.parse(line):[line];let doc=document;
      for(const segment of segments.slice(0,-1)){
        const nodes=doc.evaluate(segment,doc,null,7,null);
        if(nodes.snapshotLength!==1)throw Error("Ambiguous frame owner");
        doc=nodes.snapshotItem(0).contentDocument;if(!doc)throw Error("Unavailable frame");
      }
      const last=segments.at(-1), expected=selectors.map(s=>{
        let root=doc;for(const host of s.slice(0,-1))root=root.querySelector(host).shadowRoot;
        return root.querySelector(s.at(-1));
      });
      if(expected.some(e=>!e))throw Error("Missing expected node");
      if(last.includes("/xbotShadowRoot"))throw Error("ShadowBot grammar must not be passed to browser XPath verification");
      const r=doc.evaluate(last,doc,null,7,null),nodes=Array.from({length:r.snapshotLength},(_,i)=>r.snapshotItem(i));
      if(!expected.every(e=>nodes.includes(e)))throw Error("XPath identity mismatch");
      return nodes.length;
    });
  })()`);
}
async function main() {
  const fixture = await createFixture();
  let opened;
  try {
    opened = await cli("web","open",fixture.url);
    await evaluate(`(()=>{
      const nav=document.createElement("nav");nav.id="product-nav";
      nav.innerHTML='<a class="catalog-link" href="/alpha">Alpha product</a><a class="catalog-link" href="/beta">Beta product</a><a class="catalog-link" href="/gamma">Gamma product</a>';
      document.body.append(nav);
      const host=document.querySelector("#shadow").shadowRoot;
      host.innerHTML+='<button data-testid="batch-action" aria-label="Batch Alpha">1</button><button data-testid="batch-action" aria-label="Batch Beta">2</button><button>Unsupported shadow text</button>';
      const s=document.createElement("button");s.textContent="100";const t=s.cloneNode();t.textContent="200";document.body.append(s,t);
      const search=document.createElement("input");search.role="searchbox";search.setAttribute("aria-label","Explicit search role");document.body.append(search);
      return true;
    })()`);
    let snapshot = await cli("web","snapshot");
    const input = ref(snapshot,"Input DOM");
    let result = await cli("web","xpath",input);
    ready(result);
    assert.ok((await verify(result,[["#input"]])).every(n=>n===1));
    const text = await run(executable,["web","xpath",input,"--format","text"],{env,windowsHide:true});
    assert.deepEqual(text.stdout.trim().split(/\r?\n/),result.xpaths);
    pass("single-ref alternatives are unique same-node XPath, at most five and one per text line");

    const alpha=ref(snapshot,"Alpha product"), beta=ref(snapshot,"Beta product");
    result=await cli("web","xpath",alpha,beta);ready(result);
    assert.equal(result.kind,"similar");
    assert.ok(result.xpaths.every(p=>!p.includes("|")&&!/\[\d+\]/.test(p)));
    assert.ok((await verify(result,[["#product-nav a[href='/alpha']"],["#product-nav a[href='/beta']"]])).every(n=>n>=2));
    pass("multiple refs produce non-positional collection rules covering selected members");

    for(const ids of [[alpha,input],[input,ref(snapshot,"Check DOM")],
      [input,ref(snapshot,"Explicit search role")],[alpha,alpha]]) {
      result=await cli("web","xpath",...ids);
      assert.equal(result.status,"unavailable");assert.equal(result.xpaths.length,0);assert.ok(result.reason);
    }
    pass("different tags, input types, explicit roles and duplicate refs return clear refusals");

    const plain=snapshot.elements.filter(e=>e.name==="100"||e.name==="200").map(e=>e.ref);
    assert.equal(plain.length,2);
    result=await cli("web","xpath",...plain);
    assert.equal(result.status,"unavailable");
    pass("unrelated weak elements do not become a broad tag-only or union XPath");

    for(const [label,selectors] of [
      ["Input shadow-",[["#shadow","#shadow-input"]]],
      ["Input nested-shadow-",[["#outer","#inner","#nested-shadow-input"]]],
    ]) {
      result=await cli("web","xpath",ref(snapshot,label));ready(result);
      assert.ok(result.xpaths.every(p=>p.startsWith("//")));
      assert.equal(result.syntax,"shadowbot");assert.equal(result.validation,"requires_shadowbot");
      assert.equal(result.runtime_verified,false);
      assert.ok(result.warnings.some(w=>w.includes("影刀语法，请使用影刀验证")));
      assert.ok(result.candidates.every(c=>c.root_scope_verified && c.unique_verified===false));
      assert.ok(result.xpaths.every(p=>p.includes("/xbotShadowRoot//")));
      pass("context path generated with explicit ShadowBot verification requirement: "+label);
    }
    result=await cli("web","xpath",ref(snapshot,"Batch Alpha"),ref(snapshot,"Batch Beta"));ready(result);
    assert.ok(result.candidates.every(c=>c.count===2 && c.validation==="requires_shadowbot"));
    pass("same-ShadowRoot similar-element paths retain scoped DOM evidence, not grammar validation");
    const frameButtons=snapshot.elements.filter(e=>e.name==="Semantic frame-").map(e=>e.ref);
    assert.equal(frameButtons.length,2);
    for(const [refs,depth,selectors] of [
      [[ref(snapshot,"Input frame-")],2,[["#frame-input"]]],
      [[ref(snapshot,"Input nested-")],3,[["#nested-input"]]],
      [[ref(snapshot,"Input frame-shadow-")],2,null],
      [frameButtons,2,[['section[aria-label="frame-primary-area"] button'],['section[aria-label="frame-secondary-area"] button']]]
    ]){
      result=await cli("web","xpath",...refs);
      ready(result);
      assert.equal(result.iframe_context_required,true);
      assert.equal(result.requires_selector,true);
      assert.equal(result.syntax,"shadowbot-selector");
      assert.equal(result.validation,"requires_shadowbot");
      assert.equal(result.runtime_verified,false);
      assert.deepEqual(result.xpath_lists,result.xpaths.map(JSON.parse));
      assert.ok(result.xpath_lists.every(list=>list.length===depth));
      assert.deepEqual(result.candidates.map(c=>c.segments),result.xpath_lists);
      assert.ok(result.warnings.some(w=>w.includes("影刀语法，请使用影刀验证")));
      if(selectors)assert.ok((await verify(result,selectors)).every(n=>n>=refs.length));
      else assert.ok(result.xpath_lists.every(list=>list.at(-1).includes("/xbotShadowRoot")));
      const text=await run(executable,["web","xpath",...refs,"--format","text"],{env,windowsHide:true});
      assert.deepEqual(text.stdout.trim().split(/\r?\n/).slice(result.warnings.length),result.xpaths);
    }
    pass("iframe alternatives are ordered Selector segment lists, including nested frames and shadow targets");

    result=await cli("web","xpath",ref(snapshot,"Unsupported shadow text"));
    ready(result);
    assert.equal(result.validation,"requires_shadowbot");
    assert.ok(result.xpaths.some(p=>p.includes("Unsupported shadow text")));
    const shadowText=await run(executable,["web","xpath",ref(snapshot,"Unsupported shadow text"),"--format","text"],{env,windowsHide:true});
    const shadowLines=shadowText.stdout.trim().split(/\r?\n/);
    assert.match(shadowLines[0],/影刀语法，请使用影刀验证/);
    assert.deepEqual(shadowLines.slice(1),result.xpaths);
    assert.equal(await evaluate("Boolean(window.uiaDispatcher)"),false);
    pass("shadow descendant and text syntax is emitted without running a browser/native Selector validator");
    result=await cli("web","xpath",input,ref(snapshot,"Input frame-"));
    assert.equal(result.status,"unavailable");assert.match(result.reason,/iframe/);
    result=await cli("web","xpath",input,ref(snapshot,"Input shadow-"));
    assert.equal(result.status,"unavailable");assert.match(result.reason,/ShadowRoot/);
    pass("mixed frame and ShadowRoot contexts still return explanations");

    const dry=await cli("web","xpath",input,"--dry-run");
    assert.deepEqual(dry.effects,["read"]);
    pass("dry-run declares only read effects");
    report.passed=true;
  } finally {
    if(opened)await cli("web","close").catch(()=>{});
    await fixture.close();
    fs.mkdirSync("dist",{recursive:true});
    fs.writeFileSync("dist/preview-xpath.json",JSON.stringify(report,null,2)+"\n");
  }
}
main().catch(error=>{console.error(error.stderr||error.stack);process.exitCode=1;});
