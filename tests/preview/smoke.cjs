"use strict";
const {spawn, spawnSync} = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const {createFixture} = require("./fixture.cjs");
const executable = path.resolve(process.argv[2] || "target/debug/xbot-cli.exe");
const report = {executable, cases:[], sdk_runtime_verified:false};
const python = process.env.PREVIEW_PYTHON || "D:/Program Files/ShadowBot/shadowbot-6.3.39/python/python.exe";
function inspectPython(code, ...args) {
  const result = spawnSync(python,[path.join(__dirname,"inspect_python.py"),...args],{input:code,encoding:"utf8",windowsHide:true,env:{...process.env,PYTHONIOENCODING:"utf-8"}});
  assert.equal(result.status,0,result.stderr);
  return JSON.parse(result.stdout);
}
function inspectSelector(action, method) {
  const inspected=inspectPython(action.python.code);
  const find=inspected.calls[0];
  assert.equal(find.method,"find");
  assert.equal(find.timeout,20);
  assert.equal(find.selector.type,"simple");
  assert.equal(find.selector.xpath.enable,"true");
  assert.deepEqual(find.selector.xpath.segments,action.python.locator.selector_segments);
  assert.equal(inspected.calls[1].method,method);
  assert.ok(!action.python.code.includes("execute_javascript"));
  assert.equal(action.python.runtime_verified,false);
  return inspected.calls[1];
}
async function cli(...args) {
  return new Promise((resolve,reject) => {
    const child = spawn(executable, [...args, "--json"], {windowsHide:true});
    let stdout="",stderr="";
    child.stdout.on("data",d => stdout+=d);
    child.stderr.on("data",d => stderr+=d);
    child.on("error",reject);
    child.on("exit",code => {
      let value;
      try {value=JSON.parse(code === 0 ? stdout : stderr);} catch {return reject(Error(stdout+stderr));}
      if (code !== 0 || !value.ok) return reject(Object.assign(Error(JSON.stringify(value)),{response:value}));
      resolve(value.data);
    });
  });
}
function ref(tree, label) {
  const line = tree.split("\n").find(l => l.includes(label));
  const found = line?.match(/\[ref=([^\]]+)\]/)?.[1];
  assert.ok(found, "Missing ref for "+label+"\n"+tree);
  return found;
}
async function evaluate(code) {return (await cli("web","eval","return "+code+";")).result;}
async function main() {
  const fixture = await createFixture();
  let opened=false, tab;
  try {
    report.url = fixture.url;
    console.log("FIXTURE "+fixture.url);
    const data = await cli("web","open",fixture.url);
    opened=true; tab=data.browser_context.tab_id;
    process.env.XBOT_WEB_TAB_ID=String(tab);
    process.env.XBOT_WEB_PIPE=data.browser_context.pipe;
    console.log("OPENED "+tab);
    report.initial_response=data;
    assert.equal(data.python.status,"ready");
    assert.match(data.python.code,/mode="edge"|mode="chrome"/);
    assert.ok(Array.isArray(data.snapshot.frames));
    report.initial_snapshot = data.snapshot.result;
    report.frame_warnings = data.snapshot.frame_warnings;
    async function test(label, command, args, validate, expected="ready") {
      // Each action obtains a fresh snapshot. Refs are not guessed.
      const snapshot=await cli("web","snapshot");
      const id=ref(snapshot.result,label);
      const action=await cli("web",command,id,...args,"--no-snapshot");
      assert.equal(action.python.status,expected,JSON.stringify(action.python));
      if (validate) await validate(action);
      report.cases.push({label,command,ref:id,python:action.python,status:"passed"});
      console.log("PASS "+label+" "+command+" "+id+" "+expected);
    }
    for (const [label,query] of [
      ['button "Click DOM"', 'document.querySelector("#click")'],
      ['button "Click shadow-"', 'document.querySelector("#shadow").shadowRoot.querySelector("button")'],
      ['button "Click nested-shadow-"', 'document.querySelector("#outer").shadowRoot.querySelector("x-panel").shadowRoot.querySelector("button")'],
      ['button "Click frame-"', 'document.querySelector("#same-frame").contentDocument.querySelector("#frame-click")'],
      ['button "Click frame-shadow-"', 'document.querySelector("#same-frame").contentDocument.querySelector("x-panel").shadowRoot.querySelector("button")'],
      ['button "Click nested-"', 'document.querySelector("#same-frame").contentDocument.querySelector("#nested-frame").contentDocument.querySelector("#nested-click")'],
      ['button "Quote target"', 'document.querySelectorAll("section button")[1]'],
    ]) {
      await test(label,"click",[],async action => {
        assert.equal(await evaluate(`Number(${query}.dataset.count)`),1);
        if (action.python.locator.steps.length===1) assert.match(action.python.code,/find_by_xpath/);
        else if(action.python.locator.steps.some(s=>s.kind==="frame")) {
          inspectSelector(action,"click");
          action.python.selector_inspected=true;
        } else {
          assert.match(action.python.code,/execute_javascript/);
          const inspected=inspectPython(action.python.code);
          const call=inspected.calls[0];
          assert.equal(call.method,"execute_javascript");
          // Replay the emitted JS adapter only on this disposable test page.
          await evaluate(`(${call.code})(null, ${JSON.stringify(call.argument)})`);
          assert.equal(await evaluate(`Number(${query}.dataset.count)`),2);
          action.python.adapter_replay_verified=true;
        }
      });
    }
    for (const [label, query] of [
      ['button "Semantic shadow-"', 'document.querySelector("#shadow").shadowRoot.querySelector(\'section[aria-label="shadow-primary-area"] button\')'],
      ['button "Semantic nested-shadow-"', 'document.querySelector("#outer").shadowRoot.querySelector("x-panel").shadowRoot.querySelector(\'section[aria-label="nested-shadow-primary-area"] button\')'],
      ['button "Semantic frame-"', 'document.querySelector("#same-frame").contentDocument.querySelector(\'section[aria-label="frame-primary-area"] button\')'],
      ['button "Semantic frame-shadow-"', 'document.querySelector("#same-frame").contentDocument.querySelector("x-panel").shadowRoot.querySelector(\'section[aria-label="frame-shadow-primary-area"] button\')']
    ]) {
      await test(label, "click", [], async action => {
        const step = action.python.locator.steps.at(-1);
        assert.equal(step.strategy, "ancestor:aria-label/text");
        assert.equal(step.quality, "semantic");
        assert.equal(await evaluate(`Number(${query}.dataset.count)`), 1);
        await evaluate(`(()=>{const b=${query};const w=b.ownerDocument.createElement("div");b.replaceWith(w);w.append(b);return true;})()`);
        const call = inspectPython(action.python.code).calls[0];
        if(action.python.locator.steps.some(s=>s.kind==="frame")){
          inspectSelector(action,"click");
          assert.equal(await evaluate(`Number(${query}.dataset.count)`),1);
          action.python.selector_inspected_after_mutation=true;
          return;
        }
        assert.equal(call.method, "execute_javascript");
        await evaluate(`(${call.code})(null, ${JSON.stringify(call.argument)})`);
        assert.equal(await evaluate(`Number(${query}.dataset.count)`), 2);
        action.python.mutation_replay_verified = true;
      });
    }
    for (const [label,query] of [
      ['textbox "Input DOM"', 'document.querySelector("#input")'],
      ['textbox "Input shadow-"', 'document.querySelector("#shadow").shadowRoot.querySelector("input")'],
      ['textbox "Input frame-"', 'document.querySelector("#same-frame").contentDocument.querySelector("#frame-input")'],
      ['textbox "Input frame-shadow-"', 'document.querySelector("#same-frame").contentDocument.querySelector("x-panel").shadowRoot.querySelector("input")']
    ]) {
      await test(label,"fill",['quotes "\' 中文\nline'],async action => {
        assert.equal(await evaluate(`${query}.value`),'quotes "\' 中文line');
        const call=action.python.locator.steps.some(s=>s.kind==="frame") ?
          inspectSelector(action,"input") : inspectPython(action.python.code).calls[0];
        if(call.method==="execute_javascript") {
          await evaluate(`(${call.code})(null, ${JSON.stringify(call.argument)})`);
          assert.equal(await evaluate(`${query}.value`),'quotes "\' 中文line');
        } else {
          assert.equal(call.method,"input");
          assert.equal(call.args[0],'quotes "\' 中文\nline');
        }
      });
    }
    await test('textbox "Password DOM"',"fill",["PREVIEW_SECRET_123"],async action => {
      assert.ok(!JSON.stringify(action.python).includes("PREVIEW_SECRET_123"));
      assert.match(action.python.code,/secret_text/);
      inspectPython(action.python.code);
    });
    await test('combobox "Select DOM"',"select",["b"],async action => {
      assert.match(action.python.code,/select_by_index\(1\)/);
      assert.equal(await evaluate('document.querySelector("#select").value'),"b");
    });
    await test('checkbox "Check DOM"',"check",[],async () => {
      assert.equal(await evaluate('document.querySelector("#check").checked'),true);
    });
    await test('checkbox "Check DOM"',"uncheck",[],async () => {
      assert.equal(await evaluate('document.querySelector("#check").checked'),false);
    });
    await test('combobox "Select frame-"',"select",["b"],async action => {
      const call=inspectSelector(action,"select_by_index");
      assert.deepEqual(call.args,[1]);
      assert.equal(await evaluate('document.querySelector("#same-frame").contentDocument.querySelector("#frame-select").value'),"b");
    });
    await test('checkbox "Check frame-"',"check",[],async action => {
      const call=inspectSelector(action,"check");
      assert.equal(call.kwargs.mode,"check");
    });
    await test('checkbox "Check frame-"',"uncheck",[],async action => {
      const call=inspectSelector(action,"check");
      assert.equal(call.kwargs.mode,"uncheck");
    });
    await test('textbox "Password frame-"',"fill",["PREVIEW_IFRAME_SECRET"],async action => {
      assert.ok(!JSON.stringify(action.python).includes("PREVIEW_IFRAME_SECRET"));
      assert.equal(inspectSelector(action,"input").args[0],"TEST_SECRET");
      const failed=inspectPython(action.python.code,"--find-fails");
      assert.deepEqual(failed.calls.map(c=>c.method),["find"]);
      const actionFails=spawnSync(python,[path.join(__dirname,"inspect_python.py"),"--action-fails"],{
        input:action.python.code,encoding:"utf8",windowsHide:true,env:{...process.env,PYTHONIOENCODING:"utf-8"}});
      assert.notEqual(actionFails.status,0);
      assert.match(actionFails.stderr,/TEST_ACTION_FAILURE/);
    });
    // Successful pre-action capture must survive a node disappearing on click.
    await evaluate(`(()=>{const b=document.createElement("button");b.id="vanish";b.textContent="Vanish target";b.onclick=()=>{window.__vanished=(window.__vanished||0)+1;b.remove();};document.body.prepend(b);return true;})()`);
    await test('button "Vanish target"',"click",[],async action => {
      assert.match(action.python.code,/vanish/);
      assert.equal(await evaluate("window.__vanished"),1);
      assert.equal(await evaluate('Boolean(document.querySelector("#vanish"))'),false);
    });
    // A successful action must not promote a positional locator to reusable Python.
    await evaluate(`(()=>{const box=document.createElement("div");box.innerHTML='<button onclick="window.__weakClicked=(window.__weakClicked||0)+1"></button><button></button>';document.body.prepend(box);window.__weakContainer=box;return true;})()`);
    await test('button [ref=', "click", [], async action => {
      assert.equal(await evaluate("window.__weakClicked"), 1);
      assert.equal(action.python.code, null);
      assert.equal(action.python.locator.steps.at(-1).requires_review, true);
      assert.match(action.python.reason, /语义锚点/);
    }, "unavailable");
    await evaluate('(()=>{window.__weakContainer.remove();return true;})()');
    // An occluded iframe must not receive an accidental click.
    const frameRef=ref((await cli("web","snapshot")).result,'button "Click frame-"');
    await evaluate(`(()=>{const d=document.createElement("div");d.id="overlay";d.style.cssText="position:fixed;inset:0;z-index:2147483647;background:transparent";document.body.append(d);return true;})()`);
    await assert.rejects(cli("web","click",frameRef,"--no-snapshot"),error => error.response?.error?.code==="WEB_FRAME_OBSCURED");
    await evaluate('(()=>{document.querySelector("#overlay").remove();return true;})()');
    assert.equal(await evaluate('Number(document.querySelector("#same-frame").contentDocument.querySelector("#frame-click").dataset.count)'),1);
    report.occlusion_guard=true;
    // Transform detection must fail before dispatch.
    await evaluate('(()=>{document.querySelector("#same-frame").style.transform="scale(.9)";return true;})()');
    await assert.rejects(cli("web","click",frameRef,"--no-snapshot"),error => error.response?.error?.code==="WEB_FRAME_UNAVAILABLE");
    await evaluate('(()=>{document.querySelector("#same-frame").style.transform="";return true;})()');
    report.transform_guard=true;
    const snapshot=await cli("web","snapshot");
    if (snapshot.result.includes('button "Click cross-"')) {
      await test('button "Click cross-"',"click",[],action=>inspectSelector(action,"click"));
    } else {
      assert.ok((snapshot.frame_warnings||[]).some(w=>w.includes("OOPIF")));
      report.cross_origin="not accessible through current Bridge/CDP target; see frame_warnings";
    }
    await cli("web","goto",fixture.url+"?navigation=1","--no-snapshot");
    const reload=await cli("web","reload","--no-snapshot");
    assert.equal(reload.python.code,"web_page.reload()");
    report.page_operations=["open","active","goto","reload","close"];
    const active=await cli("web","active","--no-snapshot","--no-python");
    assert.equal(active.python,undefined);
    report.no_python=true;
    report.passed=true;
  } catch(error) {
    report.error=error.stack;
    throw error;
  } finally {
    if (opened) {
      // Refuse to close another tab if the human changed focus during the test.
      const current=await cli("web","active","--no-snapshot","--no-python").catch(()=>null);
      if (current?.browser_context?.tab_id===tab) await cli("web","close");
      else report.cleanup="Test tab was not active; no unrelated tab closed";
    }
    await fixture.close();
    fs.mkdirSync("dist",{recursive:true});
    fs.writeFileSync("dist/preview-smoke.json",JSON.stringify(report,null,2)+"\n");
  }
}
main().catch(error => {console.error(error.stack);process.exitCode=1;});
