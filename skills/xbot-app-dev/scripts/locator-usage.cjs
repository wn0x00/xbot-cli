const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");

const HELP = `Read-only workflow usage analysis. The Skill's AI writes the analysis; this tool never calls a model.
  node locator-usage.cjs capture --flow <id> --locator <old-xpath> [--asset <runtime.json>] [--callsite py:line:column] --out <new-input.json>
  node locator-usage.cjs validate --input <input.json> --analysis <ai-analysis.json> --out <new-receipt.json>
Options: --cli <native-executable> --expect-app <id> --python <python-executable>
Capture reads status/flow get only. Validate checks source quotes, AST signals and binding.
Single-element handoff additionally requires a matching runtime asset and a fresh live flow read.
Collection classification does not implement collection repair. No browser actions or application edits.`;

function fail(code, message) { throw Object.assign(new Error(message), { code }); }
function hash(value) {
  return crypto.createHash("sha256").update(typeof value === "string" || Buffer.isBuffer(value)
    ? value : JSON.stringify(value)).digest("hex");
}
function readJson(file) { return JSON.parse(fs.readFileSync(path.resolve(file), "utf8")); }
function normalize(source) { return source.replace(/\r\n?/g, "\n"); }
function flowSource(data) {
  if (data?.success === false) fail("FLOW_READ_FAILED", "Workflow read did not succeed");
  if (data?.next_offset != null || data?.has_more === true) fail("FLOW_SOURCE_INCOMPLETE", "Read the complete workflow before analysis");
  if (typeof data?.content === "string") return { kind: "codeflow", text: normalize(data.content) };
  if (Array.isArray(data?.blocks)) return { kind: "visualflow", text: JSON.stringify(data, null, 2) };
  fail("FLOW_SOURCE_UNSUPPORTED", "Expected full CodeFlow content or VisualFlow blocks");
}
function observations(source, locator, callsite, python = "python") {
  if (source.kind !== "codeflow") return { adapter: "unverified-visualflow", calls: [], unresolved: [], complete: false };
  const result = spawnSync(python, ["-I", path.join(__dirname, "locator", "usage-ast.py")], {
    input: JSON.stringify({ source: source.text, locator, callsite }), encoding: "utf8",
    windowsHide: true, shell: false, timeout: 10000, maxBuffer: 2 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) fail("USAGE_AST_UNAVAILABLE", "Python 3.8+ is required for the read-only AST adapter");
  try { return JSON.parse(result.stdout); }
  catch { fail("USAGE_AST_INVALID", "AST adapter returned incomplete evidence"); }
}
function assetLocators(asset) {
  return [asset?.generated?.primary, ...(asset?.generated?.alternatives || [])]
    .filter(item => Array.isArray(item?.xpath) && item.xpath.length === 1).map(item => item.xpath[0]);
}
function makeInput({ appId, flowId, locator, callsite = null, source, asset = null, assetHash = null, python }) {
  for (const value of [appId, flowId, locator]) if (typeof value !== "string" || !value) fail("USAGE_INPUT_INVALID", "Need app, flow and original locator");
  if (asset && (asset.app_id !== appId || !Number.isInteger(asset.version))) fail("USAGE_ASSET_CONTEXT", "Asset application/version mismatch");
  if (callsite && !/^py:\d+:\d+$/.test(callsite)) fail("USAGE_CALLSITE_INVALID", "Use the exact py:line:column ID from failure evidence");
  const input = {
    schema_version: 1, kind: "locator_usage_input", app_id: appId, flow_id: flowId,
    locator, callsite, source, source_sha256: hash(source),
    asset_sha256: assetHash, asset_version: asset?.version ?? null,
    asset_locator_match: asset ? assetLocators(asset).includes(locator) : false,
    observations: observations(source, locator, callsite, python),
  };
  return { ...input, analysis_id: hash(input) };
}
function verifyInput(input, python) {
  if (input?.kind !== "locator_usage_input" || input.schema_version !== 1 ||
      !["codeflow", "visualflow"].includes(input.source?.kind) || typeof input.source.text !== "string") {
    fail("USAGE_INPUT_INVALID", "Need the captured analysis input");
  }
  const { analysis_id: id, ...body } = input;
  if (hash(body) !== id || hash(input.source) !== input.source_sha256) fail("USAGE_INPUT_CHANGED", "Input snapshot or binding changed");
  if (hash(observations(input.source, input.locator, input.callsite, python)) !== hash(input.observations)) {
    fail("USAGE_SIGNALS_CHANGED", "Recomputed syntax evidence differs");
  }
}
function assess(input, analysis, python) {
  verifyInput(input, python);
  const kinds = ["single", "collection", "collection_then_single", "mixed", "unknown"];
  const consumers = ["one", "each", "aggregate", "one_by_business_key", "positional", "mixed", "unknown"];
  if (analysis?.analysis_id !== input.analysis_id || !kinds.includes(analysis.classification) ||
      !consumers.includes(analysis.consumer) || typeof analysis.reasoning !== "string" || !analysis.reasoning.trim() ||
      !Array.isArray(analysis.uncertainties) || !Array.isArray(analysis.evidence) || !Array.isArray(analysis.callsite_ids)) {
    fail("USAGE_ANALYSIS_INVALID", "Need a bound AI classification with reasoning, evidence and uncertainties");
  }
  const calls = input.observations.calls;
  const expectedIds = calls.map(item => item.id).sort();
  if (JSON.stringify([...analysis.callsite_ids].sort()) !== JSON.stringify(expectedIds)) {
    fail("USAGE_CALLSITE_COVERAGE", "Analysis must cover every matching callsite in the captured scope");
  }
  const lines = input.source.text.split("\n");
  for (const evidence of analysis.evidence) {
    if (!Number.isInteger(evidence.line_start) || !Number.isInteger(evidence.line_end) ||
        evidence.line_start < 1 || evidence.line_end < evidence.line_start || evidence.line_end > lines.length ||
        evidence.quote !== lines.slice(evidence.line_start - 1, evidence.line_end).join("\n")) {
      fail("USAGE_EVIDENCE_MISMATCH", "AI evidence must quote exact full source lines");
    }
    if (evidence.callsite_id && !expectedIds.includes(evidence.callsite_id)) fail("USAGE_EVIDENCE_MISMATCH", "Unknown callsite evidence");
  }
  for (const call of calls) {
    if (!analysis.evidence.some(item => item.callsite_id === call.id &&
        item.line_start <= call.line_start && item.line_end >= call.line_end)) {
      fail("USAGE_EVIDENCE_MISSING", "Missing source evidence for " + call.id);
    }
  }
  const modes = [...new Set(calls.map(item => item.query_mode))];
  const certain = !["unknown", "mixed"].includes(analysis.classification);
  if (certain && modes.length && (modes.length !== 1 ||
      modes[0] !== (analysis.classification === "single" ? "single" : "collection"))) {
    fail("USAGE_CLASSIFICATION_CONFLICT", "AI classification contradicts query APIs; collection filtering does not make the query single");
  }
  if (analysis.classification === "single" && analysis.consumer !== "one") fail("USAGE_CLASSIFICATION_CONFLICT", "Single query needs a single-target consumer");
  if (analysis.classification === "collection" && !["each", "aggregate", "unknown"].includes(analysis.consumer)) {
    fail("USAGE_CLASSIFICATION_CONFLICT", "Record collection-to-single consumption separately");
  }
  if (analysis.classification === "collection_then_single" && !["one_by_business_key", "positional"].includes(analysis.consumer)) {
    fail("USAGE_CLASSIFICATION_CONFLICT", "Need the actual selection policy");
  }
  let status = "USAGE_ANALYZED";
  let route = analysis.classification;
  if (analysis.classification === "unknown") status = "REVIEW_REQUIRED_USAGE_UNKNOWN";
  else if (analysis.classification === "mixed" || modes.length > 1) status = "REVIEW_REQUIRED_MIXED_USAGE";
  else if (!input.observations.complete || !calls.length) status = "REVIEW_REQUIRED_USAGE_UNVERIFIED";
  else if (analysis.uncertainties.length) status = "REVIEW_REQUIRED_USAGE_UNCERTAIN";
  else if (analysis.consumer === "positional") status = "REVIEW_REQUIRED_POSITIONAL_SELECTION";
  else if (!input.asset_sha256) status = "REVIEW_REQUIRED_USAGE_UNBOUND";
  else if (!input.asset_locator_match) status = "REVIEW_REQUIRED_LOCATOR_ASSET_BINDING";
  if (status.startsWith("REVIEW_")) route = "review";
  return {
    status, classification: analysis.classification, route,
    single_repair_allowed: status === "USAGE_ANALYZED" && route === "single",
    collection_repair_implemented: false, execution_authorized: false, application_modified: false,
  };
}
function authorize(receipt, asset, assetHash, readFlow, python) {
  if (receipt?.kind !== "locator_usage_receipt") fail("USAGE_ANALYSIS_REQUIRED", "Need a validated AI usage receipt before repair");
  const decision = assess(receipt.input, receipt.analysis, python);
  const input = receipt.input;
  if (asset.app_id !== input.app_id || asset.version !== input.asset_version || assetHash !== input.asset_sha256 ||
      !assetLocators(asset).includes(input.locator)) fail("USAGE_ASSET_CHANGED", "Analysis is bound to another asset, locator or version");
  if (hash(flowSource(readFlow(input.flow_id, input.app_id))) !== input.source_sha256) {
    fail("USAGE_FLOW_CHANGED", "Workflow changed; capture and analyze the current source again");
  }
  if (!decision.single_repair_allowed) {
    fail(decision.status === "USAGE_ANALYZED" ? "COLLECTION_REPAIR_NOT_SUPPORTED" : decision.status,
      "Do not route this usage into the single-element repair engine: " + decision.classification);
  }
  return { analysis_id: input.analysis_id, decision_sha256: hash(receipt.analysis),
    flow_id: input.flow_id, callsite_ids: receipt.analysis.callsite_ids, classification: decision.classification };
}
function callCli(executable, args) {
  const child = spawnSync(executable, args, { encoding: "utf8", windowsHide: true, shell: false,
    timeout: 45000, maxBuffer: 8 * 1024 * 1024 });
  if (child.error) fail("CLI_PROCESS_FAILED", child.error.message);
  let envelope;
  try { envelope = JSON.parse((child.status === 0 ? child.stdout : child.stderr).trim()); }
  catch { fail("CLI_RESPONSE_INVALID", "Need a complete CLI JSON envelope"); }
  if (child.status !== 0 || !envelope.ok) fail(envelope.error?.code || "CLI_FAILED", envelope.error?.message || "CLI failed");
  return envelope.data;
}
function parse(argv) {
  if (!argv.length || argv[0] === "--help") return { help: true };
  const operation = argv[0];
  const allowed = operation === "capture"
    ? ["flow", "locator", "asset", "callsite", "out", "cli", "expect-app", "python"]
    : operation === "validate" ? ["input", "analysis", "out", "python"] : [];
  if (!allowed.length) fail("BAD_ARGS", "Use capture or validate");
  const flags = {};
  for (let i = 1; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    if (!argv[i].startsWith("--") || !allowed.includes(key) || key in flags || !argv[i + 1] || argv[i + 1].startsWith("--")) {
      fail("BAD_ARGS", "Invalid or duplicate flag " + argv[i]);
    }
    flags[key] = argv[i + 1];
  }
  for (const key of operation === "capture" ? ["flow", "locator", "out"] : ["input", "analysis", "out"]) {
    if (!flags[key]) fail("BAD_ARGS", "Missing --" + key);
  }
  return { operation, flags };
}
function run(argv, invoke = callCli) {
  const options = parse(argv);
  if (options.help) { process.stdout.write(HELP + "\n"); return 0; }
  const { operation, flags } = options;
  const output = path.resolve(flags.out);
  if (fs.existsSync(output)) fail("OUTPUT_EXISTS", "Output must be a new file");
  let document;
  if (operation === "capture") {
    const executable = flags.cli || path.join(process.env.APPDATA || "", "npm", "node_modules", "@guanzhu.me", "xbot-cli", "native", "win32-x64", "xbot-cli.exe");
    const status = invoke(executable, ["status", "--json"]);
    const appId = status.app?.app_id;
    if (!appId || !status.studio?.opened || status.console?.has_running_task) fail("CONTEXT_NOT_IDLE", "Need an open idle Studio app");
    if (flags["expect-app"] && flags["expect-app"] !== appId) fail("APP_CONTEXT_CHANGED", "Application changed");
    const raw = flags.asset ? fs.readFileSync(path.resolve(flags.asset)) : null;
    const source = flowSource(invoke(executable, ["flow", "get", flags.flow, "--expect-app", appId, "--json"]));
    document = makeInput({ appId, flowId: flags.flow, locator: flags.locator, callsite: flags.callsite,
      source, asset: raw && JSON.parse(raw.toString("utf8")), assetHash: raw && hash(raw), python: flags.python });
  } else {
    const input = readJson(flags.input);
    const analysis = readJson(flags.analysis);
    document = { schema_version: 1, kind: "locator_usage_receipt", input, analysis,
      decision: assess(input, analysis, flags.python) };
  }
  fs.writeFileSync(output, JSON.stringify(document, null, 2) + "\n", { flag: "wx" });
  const status = document.decision?.status || "USAGE_CONTEXT_CAPTURED";
  process.stdout.write(JSON.stringify({ ok: !status.startsWith("REVIEW_"), status, file: output,
    classification: document.decision?.classification, single_repair_allowed: document.decision?.single_repair_allowed ?? false,
    application_modified: false }) + "\n");
  return status.startsWith("REVIEW_") ? 2 : 0;
}
module.exports = { hash, flowSource, observations, makeInput, assess, authorize, parse, run };
if (require.main === module) {
  try { process.exitCode = run(process.argv.slice(2)); }
  catch (error) {
    process.stderr.write(JSON.stringify({ ok: false, code: error.code || "USAGE_HELPER_FAILED",
      message: error.message, application_modified: false }) + "\n");
    process.exitCode = 1;
  }
}
