"use strict";
const {execFile} = require("node:child_process");
const {promisify} = require("node:util");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const {createFixture} = require("./fixture.cjs");
const run = promisify(execFile);
const executable = path.resolve(process.argv[2] || "target/debug/xbot-cli.exe");
const env = {...process.env};
async function cli(...args) {
  const {stdout} = await run(executable, [...args, "--json"], {env, windowsHide: true, maxBuffer: 4 * 1024 * 1024});
  const value = JSON.parse(stdout);
  assert.equal(value.ok, true, stdout);
  return value.data;
}
async function main() {
  const fixture = await createFixture();
  let tab;
  try {
    const opened = await cli("web", "open", fixture.url, "--no-snapshot", "--no-python");
    tab = opened.browser_context.tab_id;
    env.XBOT_WEB_TAB_ID = String(tab);
    env.XBOT_WEB_PIPE = opened.browser_context.pipe;
    const source = fs.readFileSync(path.join(__dirname, "../../src/web_assets/python-locator.js"), "utf8");
    const tests = fs.readFileSync(path.join(__dirname, "locator-cases.js"), "utf8");
    await cli("web", "eval", source + "\nreturn true;");
    const result = await cli("web", "eval", tests + "\nreturn window.__xbotLocatorTestResults;");
    assert.ok(Array.isArray(result.result), JSON.stringify(result));
    for (const c of result.result) console.log("PASS " + c.name + " [" + c.strategy + "]");
    fs.mkdirSync("dist", {recursive: true});
    fs.writeFileSync("dist/preview-locators.json", JSON.stringify({passed: true, cases: result.result}, null, 2) + "\n");
    console.log(result.result.length + " XPath identity/mutation cases passed");
  } finally {
    if (tab) await cli("web", "close").catch(error => console.error("Cleanup: " + error.message));
    await fixture.close();
  }
}
main().catch(error => {console.error(error.stderr || error.stack); process.exitCode = 1;});
