"use strict";
const {execFile,spawnSync}=require("node:child_process");
const {promisify}=require("node:util");
const {randomUUID}=require("node:crypto");
const fs=require("node:fs"),path=require("node:path"),assert=require("node:assert/strict");
const {createFixture}=require("./fixture.cjs");
const run=promisify(execFile);
const executable=path.resolve(process.argv[2]||"target/debug/xbot-cli.exe");
const env={...process.env,XBOT_WEB_SESSION:"boundaries-"+randomUUID()};
delete env.XBOT_WEB_PIPE;delete env.XBOT_WEB_TAB_ID;
const report={executable,cases:[],shadowbot_runtime_verified:false};
async function cli(...args){
  try{
    const r=await run(executable,[...args,"--json"],{env,windowsHide:true,maxBuffer:8*1024*1024});
    return JSON.parse(r.stdout).data;
  }catch(e){e.response=JSON.parse(e.stderr||"{}");throw e;}
}
async function evaluate(code){return (await cli("web","eval","return "+code+";")).result;}
async function waitClick(id,count){
  let events;
  for(let attempt=0;attempt<20;attempt++){
    events=await evaluate("window.__boundaryEvents");
    if(events.some(e=>e.event==="click"&&e.id===id&&e.count===count))return;
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert.fail("Click receipt missing: "+id+" "+JSON.stringify(events));
}
function ref(snapshot,name){
  const matches=snapshot.elements.filter(e=>e.name===name);
  assert.equal(matches.length,1,"Missing/ambiguous "+name);return matches[0].ref;
}
function pass(name){report.cases.push({name,passed:true});console.log("PASS "+name);}
function inspectSelector(action,method){
  const python=process.env.PREVIEW_PYTHON||"D:/Program Files/ShadowBot/shadowbot-6.3.39/python/python.exe";
  const inspected=spawnSync(python,[path.join(__dirname,"inspect_python.py")],{input:action.python.code,
    encoding:"utf8",windowsHide:true,env:{...process.env,PYTHONIOENCODING:"utf-8"}});
  assert.equal(inspected.status,0,inspected.stderr);
  const calls=JSON.parse(inspected.stdout).calls;
  assert.equal(calls[0].method,"find");
  assert.deepEqual(calls[0].selector.xpath.segments,action.python.locator.selector_segments);
  assert.equal(calls[1].method,method);
  assert.equal(action.python.runtime_verified,false);
  assert.ok(!action.python.code.includes("execute_javascript"));
}
async function main(){
  const fixture=await createFixture({boundaries:true});let opened;
  try{
    opened=await cli("web","open",fixture.url);
    let snapshot=opened.snapshot;
    report.frames=snapshot.frames.map(f=>({prefix:f.prefix,frame_id:f.frame_id,target_id:f.target_id,
      parent_id:f.parent_id,owner_ref:f.owner_ref,url:f.url}));
    assert.deepEqual(snapshot.frame_warnings,[]);
    assert.ok(snapshot.frames.length>=9,JSON.stringify(report.frames));
    const owners=new Set();
    function walk(node,parent){
      if(node.role==="document"){
        assert.equal(parent?.role,"iframe");owners.add(parent.ref);
      }
      for(const child of node.children||[])walk(child,node);
    }
    walk(snapshot.tree);
    for(const f of snapshot.frames)assert.ok(owners.has(f.owner_ref),"Lost frame owner "+f.prefix);
    pass("OOPIF, nested cross-origin, shadow-to-iframe and duplicate URLs attach to actual owners");

    for(const prefix of ["cross-","cross-nested-","cross-shadow-","return-","shadow-frame-","shadow-cross-"]){
      const id=ref(snapshot,"Input "+prefix),value="FILLED_"+prefix;
      const filled=await cli("web","fill",id,value,"--no-snapshot");
      assert.equal(filled.result,value);
      assert.ok((await evaluate("window.__boundaryEvents")).some(e=>e.event==="input"&&e.id===prefix+"input"&&e.value===value));
      const result=await cli("web","xpath",id);
      assert.equal(result.status,"ready",JSON.stringify(result));
      assert.equal(result.iframe_context_required,true);
      assert.equal(result.requires_selector,true);
      assert.equal(result.runtime_verified,false);
      assert.equal(result.validation,"requires_shadowbot");
      assert.equal(result.syntax,"shadowbot-selector");
      assert.ok(result.xpath_lists.length>0&&result.xpath_lists.length<=5);
      assert.deepEqual(result.xpath_lists,result.xpaths.map(p=>JSON.parse(p)));
      assert.ok(result.xpath_lists.every(list=>list.length===filled.python.locator.selector_segments.length));
      assert.ok(result.xpath_lists.every(list=>list.at(-1).includes(prefix+"input")||list.at(-1).includes("Input "+prefix)));
      inspectSelector(filled,"input");
      assert.ok(!filled.python.locator.shadowbot_xpath);
      pass("fill and ordered Selector XPath segments preserve real frame scope: "+prefix);
    }
    const clicked=await cli("web","click",ref(snapshot,"Click cross-"),"--no-snapshot");
    assert.equal(clicked.python.status,"ready");
    inspectSelector(clicked,"click");
    assert.ok(!clicked.python.locator.shadowbot_xpath);
    await waitClick("cross-click",1);
    pass("cross-origin trusted click executes once and emits native Selector code, not a page-JS bypass");
    for(const prefix of ["cross-nested-","return-","shadow-cross-","cross-shadow-"]){
      await cli("web","click",ref(snapshot,"Click "+prefix),"--no-snapshot");
      await waitClick(prefix+"click",1);
    }
    pass("trusted mouse coordinates cross nested renderer and open-shadow frame boundaries");
    const shadowClicked=await cli("web","click",ref(snapshot,"Click shadow-frame-"),"--no-snapshot");
    assert.equal(shadowClicked.python.status,"ready");
    assert.ok(shadowClicked.python.locator.shadow_frame);
    assert.ok((await evaluate("window.__boundaryEvents")).some(e=>e.event==="click"&&e.id==="shadow-frame-click"&&e.count===1));
    inspectSelector(shadowClicked,"click");
    assert.ok(shadowClicked.python.locator.selector_segments[0].includes("/xbotShadowRoot"));
    assert.ok(!(await evaluate("window.__boundaryEvents")).some(e=>e.event==="click"&&e.id==="shadow-frame-click"&&e.count===2));
    pass("shadow-to-iframe boundaries stay within owner segments; Python generation never replays the click");

    const duplicateFrames=snapshot.frames.filter(f=>f.url.endsWith("/duplicate"));
    assert.equal(duplicateFrames.length,2);
    assert.notEqual(duplicateFrames[0].frame_id,duplicateFrames[1].frame_id);
    for(const [i,f] of duplicateFrames.entries()){
      const id=snapshot.elements.find(e=>e.frame===f.prefix&&e.name==="Input duplicate-").ref;
      await cli("web","fill",id,"DUP_"+i,"--no-snapshot");
    }
    const events=(await evaluate("window.__boundaryEvents")).filter(e=>e.id==="duplicate-input"&&e.event==="input");
    assert.equal(events.length,2);assert.notEqual(events[0].direct,events[1].direct);
    const ids=duplicateFrames.map(f=>snapshot.elements.find(e=>e.frame===f.prefix&&e.name==="Input duplicate-").ref);
    const mixed=await cli("web","xpath",...ids);
    assert.equal(mixed.status,"unavailable");assert.match(mixed.reason,/iframe/);
    pass("identical iframe URLs and colliding local refs cannot retarget each other");
    const before=ref(snapshot,"Input cross-");
    snapshot=await cli("web","snapshot");
    assert.equal(ref(snapshot,"Input cross-"),before,"Stable document should retain frame prefix");
    pass("target detach/reattach across CLI processes preserves live-document refs");

    await evaluate(`(()=>{document.querySelector("#cross-frame").src="http://localhost:${fixture.ports[1]}/cross?navigation=1";return true;})()`);
    await new Promise(resolve=>setTimeout(resolve,300));
    await assert.rejects(cli("web","fill",before,"DO_NOT_WRITE","--no-snapshot"),
      e=>e.response?.error?.code==="WEB_REF_STALE");
    snapshot=await cli("web","snapshot");
    assert.notEqual(ref(snapshot,"Input cross-"),before);
    await cli("web","fill",ref(snapshot,"Input cross-"),"RECOVERED_CROSS","--no-snapshot");
    pass("iframe-only navigation invalidates old refs; fresh snapshot recovers");
    const launched=await cli("web","click",ref(snapshot,"Open login popup"));
    assert.equal(launched.applied,true);
    snapshot=launched.snapshot;
    if(!snapshot.elements.some(e=>e.name==="Input popup-")){
      await new Promise(resolve=>setTimeout(resolve,300));
      snapshot=await cli("web","snapshot");
    }
    const popup=ref(snapshot,"Input popup-");
    const popupFrame=snapshot.frames.find(f=>f.url.endsWith("/popup"));
    assert.ok(popupFrame.target_id && popupFrame.owner_ref);
    const popupPath=await cli("web","xpath",popup);
    assert.equal(popupPath.iframe_context_required,true);
    assert.equal(popupPath.status,"ready");
    assert.ok(popupPath.xpath_lists.every(list=>list.length===2));
    assert.ok(popupPath.xpath_lists.some(list=>list.at(-1).includes("popup-input")||list.at(-1).includes("Input popup-")));
    await cli("web","fill",popup,"DISPOSABLE_INPUT","--no-snapshot");
    assert.ok((await evaluate("window.__boundaryEvents")).some(e=>e.id==="popup-input"&&e.value==="DISPOSABLE_INPUT"));
    pass("click-created cross-origin login popup is captured and addressable without replaying the opening click");
    report.passed=true;
  }finally{
    if(opened)await cli("web","close").catch(()=>{});
    await fixture.close();
    fs.mkdirSync("dist",{recursive:true});
    fs.writeFileSync("dist/preview-boundaries.json",JSON.stringify(report,null,2)+"\n");
  }
}
main().catch(e=>{console.error(e.stderr||e.stack);process.exitCode=1;});
