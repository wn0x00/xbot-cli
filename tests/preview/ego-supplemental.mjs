// Supplemental source-generator browser checks, not an xbot Bridge/SDK test.
const fs = await import("node:fs/promises");
const {fileURLToPath} = await import("node:url");
const {createFixture} = await import("./fixture.cjs");
const root = fileURLToPath(new URL("../../",import.meta.url)).replace(/\\/g,"/").replace(/\/$/,"");
const report = {bridge_verified:false, sdk_runtime_verified:false, cases:[]};
const task = await taskSpace("xbot-local-merge-20261030");
console.log({spaceId:task.spaceId});
const page = task.page("p1");
const fixture = await createFixture();
let passed = false;
try {
  await page.goto(fixture.url);
  await page.evaluate(await fs.readFile(root + "/src/web_assets/python-locator.js","utf8"));
  await page.evaluate(await fs.readFile(root + "/tests/preview/locator-cases.js","utf8"));
  report.cases.push(...await page.evaluate(() => window.__xbotLocatorTestResults));
  const closedCases = await page.evaluate(() => {
    const cases = [];
    function assert(condition, message) { if (!condition) throw new Error(message); }
    function check(name, node) {
      for (const result of [window.__xbotPythonLocator(node),window.__xbotPythonXPaths([node])]) {
        assert(result.error_code === "WEB_SHADOW_CLOSED", name + ": missing error code");
        assert(result.closed_shadow && !result.steps.length && !result.candidates.length, name + ": leaked paths");
        assert(result.reason.includes("无法生成 XPath"), name + ": missing warning");
      }
      cases.push({name,passed:true});
    }
    const host = document.createElement("lab-closed");
    document.body.append(host);
    const closed = host.attachShadow({mode:"closed"});
    const button = document.createElement("button");
    button.textContent = "Closed button";
    closed.append(button);
    check("closed root blocks XPath and Python locator steps",button);
    const innerHost = document.createElement("lab-inner-open");
    closed.append(innerHost);
    const inner = innerHost.attachShadow({mode:"open"});
    const nested = document.createElement("button");
    nested.textContent = "Nested button";
    inner.append(nested);
    check("open root nested under closed ancestry is blocked",nested);
    const outer = document.createElement("lab-outer-open");
    document.body.append(outer);
    const open = outer.attachShadow({mode:"open"});
    const innerClosedHost = document.createElement("lab-inner-closed");
    open.append(innerClosedHost);
    const innerClosed = innerClosedHost.attachShadow({mode:"closed"});
    const target = document.createElement("button");
    target.textContent = "Inner closed button";
    innerClosed.append(target);
    check("closed root nested under open ancestry is blocked",target);
    const frame = document.createElement("iframe");
    frame.name = "closed-frame-owner";
    closed.append(frame);
    check("iframe owner under closed ancestry has no locator steps",frame);
    const ordinary = document.querySelector("#input");
    assert(window.__xbotPythonXPaths([ordinary]).candidates.length > 0,"ordinary node mislabeled closed");
    cases.push({name:"null shadowRoot alone is not closed evidence",passed:true});
    const mixed = window.__xbotPythonXPaths([button,document.querySelector("#click")]);
    assert(mixed.error_code === "WEB_SHADOW_CLOSED" && mixed.candidates.length === 0,"mixed closed selection leaked XPath");
    cases.push({name:"any closed member blocks a similar-element selection",passed:true});
    const movable = document.createElement("button");
    movable.id = "movable-live-target";
    movable.textContent = "Movable button";
    open.append(movable);
    assert(window.__xbotPythonXPaths([movable]).candidates.length > 0,"open candidate missing");
    inner.append(movable);
    assert(movable.isConnected,"move disconnected target");
    check("same connected node becomes unavailable after moving beneath closed ancestry",movable);
    host.remove(); outer.remove();
    return cases;
  });
  report.cases.push(...closedCases);
  report.passed = true;
  passed = true;
  console.log({passed:true,cases:report.cases.length,bridge_verified:false,sdk_runtime_verified:false});
} catch (error) {
  report.passed = false;
  report.error = String(error.stack || error);
  throw error;
} finally {
  await fixture.close();
  await fs.mkdir(root + "/dist",{recursive:true});
  await fs.writeFile(root + "/dist/preview-selector-closed-source-browser.json",JSON.stringify(report,null,2)+"\n");
}
if (passed) await task.finish({keep:[]});
