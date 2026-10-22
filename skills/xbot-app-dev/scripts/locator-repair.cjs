const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");
const { gunzipSync } = require("node:zlib");
const { stripTypeScriptTypes } = require("node:module");
const usage = require("./locator-usage.cjs");

const ROBULA_HASH = "a674d99763c8045456add711d549f53bfe6103f025b338b30e1cec398b2b29d4";
const PROFILE = "data-contract-v1";
const HELP = `Read-only locator helper; no clicks, application edits, selector writes or asset commits.
Usage:
  node locator-repair.cjs capture --profile data-contract-v1 --ref <fresh-ref> --robula-source <index.ts> --out <new.json>
  node locator-repair.cjs diagnose --asset <asset.json> --robula-source <index.ts> --out <new.json>
  node locator-repair.cjs propose --asset <asset.json> --usage-analysis <receipt.json> --robula-source <index.ts> --out <new.json>
  node locator-repair.cjs revalidate --asset <asset.json> --usage-analysis <receipt.json> --plan <plan.json> --robula-source <index.ts> --out <new.json>
Options: --cli <native-executable> --expect-app <app-id> --python <python-executable>
Exit codes: 0 = evidence produced; 2 = blocked/review/wait; 1 = invalid input/tool error.
Propose/revalidate require AI workflow-usage analysis, syntax verification and a fresh workflow read.
Collection usages cannot enter this single-element repair engine.
The data-contract-v1 profile requires explicit page/record/action/frame/host data attributes.
ROBULA+ source is externally supplied and hash-pinned; it is not bundled or downloaded.`;

function fail(code, message) {
  throw Object.assign(new Error(message), { code });
}
function parse(argv) {
  if (argv.length === 0 || argv[0] === "--help") return { help: true };
  const operation = argv[0];
  if (!["capture", "diagnose", "propose", "revalidate"].includes(operation)) fail("BAD_ARGS", "Unsupported operation");
  const flags = {};
  const allowed = new Set(["profile", "ref", "asset", "plan", "robula-source", "out", "cli", "expect-app", "usage-analysis", "python"]);
  for (let i = 1; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    if (!argv[i].startsWith("--") || !allowed.has(key) || key in flags || !argv[i + 1] || argv[i + 1].startsWith("--")) {
      fail("BAD_ARGS", "Invalid or duplicate option: " + argv[i]);
    }
    flags[key] = argv[i + 1];
  }
  for (const key of ["robula-source", "out"]) if (!flags[key]) fail("BAD_ARGS", "Missing --" + key);
  if (operation === "capture") {
    if (flags.profile !== PROFILE || !/^(?:f\d+)?e\d+$/.test(flags.ref || "") || flags.asset || flags.plan) {
      fail("BAD_ARGS", "Capture needs --profile data-contract-v1 and a fresh --ref");
    }
  } else {
    if (!flags.asset || flags.ref || flags.profile) fail("BAD_ARGS", "Use --asset for an existing runtime capture");
    if (operation === "revalidate" ? !flags.plan : Boolean(flags.plan)) fail("BAD_ARGS", "Only revalidate accepts and requires --plan");
  }
  if (["propose", "revalidate"].includes(operation)) {
    if (!flags["usage-analysis"]) fail("USAGE_ANALYSIS_REQUIRED", "Analyze workflow usage before proposing a repair");
  } else if (flags["usage-analysis"]) fail("BAD_ARGS", "Usage receipts apply only to propose/revalidate");
  return { operation, flags };
}
function loadJson(file) {
  try { return JSON.parse(fs.readFileSync(path.resolve(file), "utf8")); }
  catch (error) { fail("INPUT_JSON_INVALID", error.message); }
}
function loadSources(file) {
  const bytes = fs.readFileSync(path.resolve(file));
  if (crypto.createHash("sha256").update(bytes).digest("hex") !== ROBULA_HASH) {
    fail("ROBULA_SOURCE_MISMATCH", "ROBULA+ source does not match the evaluated fixed revision");
  }
  if (typeof stripTypeScriptTypes !== "function") fail("NODE_TYPESCRIPT_UNAVAILABLE", "This helper requires Node stripTypeScriptTypes support");
  // Isolate Node's experimental parser warning so stderr stays a JSON error channel.
  const stripped = spawnSync(process.execPath, ["--no-warnings", "-e",
    "const fs=require('node:fs'),m=require('node:module');process.stdout.write(m.stripTypeScriptTypes(fs.readFileSync(0,'utf8'),{mode:'strip'}));"], {
    input: bytes, encoding: "utf8", windowsHide: true, timeout: 10000, maxBuffer: 2 * 1024 * 1024,
  });
  if (stripped.error || stripped.status !== 0) fail("ROBULA_SOURCE_PARSE_FAILED", "Unable to strip the pinned TypeScript source");
  const directory = path.join(__dirname, "locator");
  const text = name => fs.readFileSync(path.join(directory, name), "utf8");
  const weighted = text("weighted.js");
  const robulaAdapter = text("robula-adapter.js");
  const original = text("hybrid-core.js")
    .replace('const weighted = new Function("return (" + sources.weighted + ");")();', "const weighted = (" + weighted + ");")
    .replace('const robula = new Function("return (" + sources.robulaAdapter + ");")();', "const robula = (" + robulaAdapter + ");");
  if (original.includes('const weighted = new Function') || original.includes('const robula = new Function')) {
    fail("GENERATOR_ASSEMBLY_FAILED", "Unexpected generator wrapper");
  }
  return {
    core: text("guarded-core.js"), original,
    sources: {
      weighted, robulaAdapter,
      robulaLibrary: stripped.stdout.replace(/^export /gm, ""),
    },
  };
}
function callCli(executable, args, input) {
  const result = spawnSync(executable, args, {
    input: input === undefined ? undefined : JSON.stringify(input),
    encoding: "utf8", windowsHide: true, shell: false, timeout: 45000, maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) fail("CLI_PROCESS_FAILED", result.error.message);
  let response;
  try { response = JSON.parse((result.status === 0 ? result.stdout : result.stderr).trim()); }
  catch { fail("CLI_RESPONSE_INVALID", "CLI did not return a complete JSON envelope"); }
  if (result.status !== 0 || !response.ok) fail(response.error?.code || "CLI_FAILED", response.error?.message || "CLI failed");
  return response.data;
}
function decode(value) {
  try {
    if (typeof value !== "string") return value;
    if (!value.startsWith("gz:")) fail("EVALUATION_RESPONSE_INVALID", "Missing compressed response envelope");
    return JSON.parse(gunzipSync(Buffer.from(value.slice(3), "base64"), { maxOutputLength: 8 * 1024 * 1024 }).toString("utf8"));
  } catch { fail("EVALUATION_RESPONSE_INVALID", "Truncated, invalid or oversized evaluation result; no repair was adopted"); }
}
function buildCode(request, bundle) {
  return `const value=await (${bundle.core})(activePage,${JSON.stringify(request)},${JSON.stringify(bundle.sources)},(${bundle.original}));
const stream=new Blob([JSON.stringify(value)]).stream().pipeThrough(new CompressionStream('gzip'));
const bytes=new Uint8Array(await new Response(stream).arrayBuffer());
return 'gz:'+btoa(String.fromCharCode(...bytes));`;
}
function freshRefs(snapshot, role) {
  if (snapshot.acc_tree_meta?.kind !== "full" || typeof snapshot.result !== "string") fail("FULL_SNAPSHOT_REQUIRED", "Need a complete current snapshot");
  return [...new Set(snapshot.result.split("\n").filter(line => !role || line.includes("- " + role + " "))
    .map(line => line.match(/\[((?:f\d+)?e\d+)\]/)?.[1]).filter(Boolean))];
}
function run(argv, invoke = callCli) {
  const options = parse(argv);
  if (options.help) { process.stdout.write(HELP + "\n"); return 0; }
  const { operation, flags } = options;
  const output = path.resolve(flags.out);
  if (fs.existsSync(output)) fail("OUTPUT_EXISTS", "Output must be a new file");
  if (!fs.statSync(path.dirname(output)).isDirectory()) fail("OUTPUT_DIRECTORY_MISSING", "Output parent is not a directory");
  const asset = flags.asset ? loadJson(flags.asset) : null;
  if (asset && (asset.profile_id !== PROFILE || !asset.contract?.guard || !asset.generated?.primary || !Number.isInteger(asset.version) || !asset.app_id)) {
    fail("ASSET_INVALID", "Need a runtime asset produced by this helper; do not pass a design template");
  }
  const planDocument = flags.plan ? loadJson(flags.plan) : null;
  const plan = planDocument?.result;
  if (operation === "revalidate" && (!plan || plan.status !== "REPAIR_VALIDATED" || planDocument.app_id !== asset.app_id)) {
    fail("PLAN_INVALID", "Need this helper's successful proposal for the same application");
  }
  const executable = flags.cli || path.join(process.env.APPDATA || "", "npm", "node_modules", "@guanzhu.me", "xbot-cli", "native", "win32-x64", "xbot-cli.exe");
  const status = invoke(executable, ["status", "--json"]);
  const appId = status.app?.app_id;
  if (!appId || !status.studio?.opened || status.console?.has_running_task) fail("CONTEXT_NOT_IDLE", "Need an open idle Studio application");
  if ((flags["expect-app"] && flags["expect-app"] !== appId) || (asset && asset.app_id !== appId)) fail("APP_CONTEXT_CHANGED", "Current app differs from expected application or asset");
  let usageEvidence;
  if (flags["usage-analysis"]) {
    usageEvidence = usage.authorize(loadJson(flags["usage-analysis"]), asset, usage.hash(fs.readFileSync(path.resolve(flags.asset))),
      (flowId, expectedApp) => invoke(executable, ["flow", "get", flowId, "--expect-app", expectedApp, "--json"]), flags.python);
    if (operation === "revalidate" && JSON.stringify(plan.usage_analysis) !== JSON.stringify(usageEvidence)) {
      fail("USAGE_PLAN_CHANGED", "Proposal and execution preflight must use the same workflow usage analysis");
    }
  }
  const bundle = loadSources(flags["robula-source"]);
  const snapshot = invoke(executable, ["web", "snapshot", "--json"]);
  const refs = freshRefs(snapshot, asset?.contract.role);
  if (operation === "capture" && !freshRefs(snapshot).includes(flags.ref)) fail("REF_NOT_IN_FRESH_SNAPSHOT", "Capture ref is absent from the latest full snapshot");
  const request = { operation: operation === "propose" ? "repair" : operation, refid: flags.ref, asset, plan, refids: refs };
  const response = invoke(executable, ["tool", "browser", "call", "web_evaluate_playwright", "-", "--json"], {
    code: buildCode(request, bundle),
    explanation: "只读运行强身份校验、ROBULA+ 和加权生成器，输出定位修复证据；不点击、不写入元素库或流程。",
  });
  const result = decode(response.evalute_result);
  if (!result || typeof result.status !== "string") fail("EVALUATION_RESPONSE_INVALID", "Missing structured status");
  if (usageEvidence) result.usage_analysis = usageEvidence;
  let document;
  if (operation === "capture" && result.status === "CAPTURED") {
    const guard = result.contract.guard;
    if (guard.top_page["data-page-id"] === null || guard.top_page["data-record-id"] === null) fail("PROFILE_NOT_SUPPORTED", "Page does not expose the selected data identity contract");
    document = { ...result, schema_version: 1, profile_id: PROFILE, app_id: appId, version: 1, history: [], captured_at: new Date().toISOString() };
  } else {
    document = { schema_version: 1, profile_id: PROFILE, app_id: appId, asset_version: asset?.version,
      snapshot_meta: snapshot.acc_tree_meta, generated_at: new Date().toISOString(), result };
  }
  fs.writeFileSync(output, JSON.stringify(document, null, 2) + "\n", { flag: "wx" });
  process.stdout.write(JSON.stringify({ ok: !/^(?:BLOCKED|REVIEW_REQUIRED|WAIT)_/.test(result.status),
    status: result.status, file: output, app_id: appId, persisted: false, application_modified: false }) + "\n");
  return /^(?:BLOCKED|REVIEW_REQUIRED|WAIT)_/.test(result.status) ? 2 : 0;
}

module.exports = { parse, decode, freshRefs, loadSources, buildCode, run, ROBULA_HASH };
if (require.main === module) {
  try { process.exitCode = run(process.argv.slice(2)); }
  catch (error) {
    process.stderr.write(JSON.stringify({ ok: false, code: error.code || "HELPER_FAILED", message: error.message, application_modified: false }) + "\n");
    process.exitCode = 1;
  }
}
