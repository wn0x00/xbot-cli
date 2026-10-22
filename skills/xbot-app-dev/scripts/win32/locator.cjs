const fs = require("node:fs");
const crypto = require("node:crypto");

class LocatorError extends Error {
  constructor(code, detail) {
    super(`${code}: ${JSON.stringify(detail)}`);
    this.code = code;
    this.detail = detail;
  }
}
const stop = (code, detail) => { throw new LocatorError(code, detail); };
const hash = value => crypto.createHash("sha256").update(JSON.stringify(value)).digest("hex");
const flagNames = new Set([
  "disabled", "enabled", "settable", "string", "boolean", "number",
  "selected", "selectable", "checked", "unchecked", "expanded", "collapsed",
  "focused", "focusable", "readonly", "read-only", "password", "protected",
]);

function parseSuffix(text) {
  let flags = "";
  const prefix = /^\(([^)]*)\)(?: |$)/.exec(text);
  if (prefix && prefix[1].split(",").every(flag => flagNames.has(flag.trim()))) {
    flags = prefix[1];
    text = text.slice(prefix[0].length);
  }
  text = text.split(" Secondary Actions:")[0];
  const fields = {};
  const pieces = text.split(/(?:^| )(ID|Value): /);
  const name = pieces.shift();
  for (let i = 0; i < pieces.length; i += 2) {
    if (fields[pieces[i]] !== undefined) stop("SNAPSHOT_FORMAT_AMBIGUOUS");
    fields[pieces[i]] = pieces[i + 1];
  }
  return {flags, name, automation_id:fields.ID || "", value:fields.Value ?? null};
}

function validateSnapshot(snapshot) {
  if (snapshot?.backend !== "sky.window2" || snapshot.schema_version !== 1 ||
      !snapshot.snapshot_id || !snapshot.window?.app || !Number.isInteger(snapshot.window.id) ||
      typeof snapshot.window.title !== "string" || !Array.isArray(snapshot.nodes) ||
      !snapshot.nodes.length) stop("SNAPSHOT_UNSUPPORTED");
  const byIndex = new Map();
  for (const node of snapshot.nodes) {
    if (!Number.isInteger(node.index) || node.index < 0 || byIndex.has(node.index) ||
        typeof node.role !== "string" || !node.role || typeof node.name !== "string" ||
        typeof node.automation_id !== "string" || typeof node.disabled !== "boolean" ||
        !(node.parent === null || Number.isInteger(node.parent))) stop("SNAPSHOT_UNSUPPORTED");
    byIndex.set(node.index, node);
  }
  for (const start of snapshot.nodes) {
    const seen = new Set([start.index]);
    let node = start;
    while (node.parent !== null) {
      node = byIndex.get(node.parent);
      if (!node || seen.has(node.index)) stop("SNAPSHOT_PARENT_INVALID");
      seen.add(node.index);
    }
  }
}

function fromSky(state, observationId) {
  if (!state.window || typeof state.accessibility?.tree !== "string") stop("SNAPSHOT_UNAVAILABLE");
  const nodes = [];
  const stack = [];
  const indexes = new Set();
  let rootDepth = null;
  const tree = state.accessibility.tree.split("\nDocument text:")[0];
  for (const line of tree.split("\n")) {
    if (!line.trim() || line.startsWith("Window:")) continue;
    const match = /^(\t*)(\d+) (\S+)(?: (.*))?$/.exec(line);
    if (!match) stop("SNAPSHOT_FORMAT_UNSUPPORTED", line.slice(0, 80));
    if (rootDepth === null) rootDepth = match[1].length;
    const depth = match[1].length - rootDepth;
    const index = Number(match[2]);
    if (indexes.has(index)) stop("SNAPSHOT_INDEX_COLLISION", index);
    indexes.add(index);
    while (stack.length && stack[stack.length - 1].depth >= depth) stack.pop();
    if (depth !== (stack.length ? stack[stack.length - 1].depth + 1 : 0)) {
      stop("SNAPSHOT_DEPTH_INVALID");
    }
    const suffix = parseSuffix(match[4] || "");
    nodes.push({
      index, role:match[3], ...suffix,
      parent:stack.length ? stack[stack.length - 1].index : null,
      disabled:suffix.flags.split(",").map(s => s.trim()).includes("disabled"),
    });
    stack.push({index, depth});
  }
  if (!nodes.length) stop("SNAPSHOT_EMPTY");
  const snapshot = {
    schema_version:1, backend:"sky.window2", snapshot_id:observationId,
    observed_at:Date.now(), window:state.window, nodes,
    focused_element:state.accessibility.focused_element || null,
    tree_complete:"unverified", atomic:false,
  };
  validateSnapshot(snapshot);
  return snapshot;
}

function ancestors(snapshot, node) {
  const byIndex = new Map(snapshot.nodes.map(item => [item.index, item]));
  const result = [];
  const seen = new Set([node.index]);
  while (node.parent !== null) {
    node = byIndex.get(node.parent);
    if (!node || seen.has(node.index)) stop("SNAPSHOT_PARENT_INVALID");
    seen.add(node.index);
    result.push(node);
  }
  return result;
}

function matches(snapshot, node, selector) {
  const allowed = new Set(["role", "name", "automation_id", "ancestor_ids"]);
  if (!selector || Object.keys(selector).some(key => !allowed.has(key)) ||
      typeof selector.role !== "string" || !selector.role ||
      !(selector.name || selector.automation_id) ||
      ["name", "automation_id"].some(key => selector[key] !== undefined && typeof selector[key] !== "string") ||
      (selector.ancestor_ids !== undefined && (!Array.isArray(selector.ancestor_ids) ||
       selector.ancestor_ids.some(id => typeof id !== "string" || !id)))) stop("SELECTOR_UNPROVEN");
  if (node.role !== selector.role) return false;
  for (const key of ["name", "automation_id"]) {
    if (selector[key] !== undefined && node[key] !== selector[key]) return false;
  }
  const ids = ancestors(snapshot, node).map(item => item.automation_id).filter(Boolean);
  let position = 0;
  for (const id of selector.ancestor_ids || []) {
    const found = ids.indexOf(id, position);
    if (found < 0) return false;
    position = found + 1;
  }
  return true;
}

function check(snapshot, asset) {
  validateSnapshot(snapshot);
  if (asset?.backend !== "sky.window2" || asset.schema_version !== 1 || asset.usage !== "single" ||
      !asset.baseline?.snapshot_sha256 || !asset.baseline?.identity_verified ||
      !asset.target?.purpose || !asset.scope || !Array.isArray(asset.variants) ||
      !asset.variants.length) stop("IDENTITY_BASELINE_REQUIRED");
  if (snapshot.window.app !== asset.scope.app || snapshot.window.id !== asset.scope.window_id ||
      snapshot.window.title !== asset.scope.title) stop("WINDOW_SCOPE_MISMATCH");
  if (asset.scope.conversation) {
    const titles = snapshot.nodes.filter(node => node.automation_id.endsWith(".current_chat_name_label"));
    if (titles.length !== 1 || titles[0].name !== asset.scope.conversation) stop("CONVERSATION_MISMATCH");
  }
  if (asset.uncertainties?.length) stop("IDENTITY_UNCERTAIN", asset.uncertainties);
}

function capture(snapshot, index, purpose, conversation = null) {
  validateSnapshot(snapshot);
  if (typeof purpose !== "string" || !purpose) stop("PURPOSE_REQUIRED");
  const node = snapshot.nodes.find(item => item.index === index);
  if (!node || (!node.name && !node.automation_id)) stop("TARGET_MISSING");
  const ancestorIds = ancestors(snapshot, node).map(item => item.automation_id).filter(Boolean);
  const identity = {
    role:node.role, name:node.name,
    ...(node.automation_id ? {automation_id:node.automation_id} : {}),
    ancestor_ids:ancestorIds,
  };
  if (snapshot.nodes.filter(item => matches(snapshot, item, identity)).length !== 1) stop("TARGET_AMBIGUOUS");
  return {
    schema_version:1, backend:"sky.window2", usage:"single", asset_id:purpose,
    scope:{app:snapshot.window.app,window_id:snapshot.window.id,title:snapshot.window.title,conversation},
    target:{purpose,identity}, uncertainties:[],
    baseline:{snapshot_id:snapshot.snapshot_id,snapshot_sha256:hash(snapshot),identity_verified:true},
    variants:[{id:"original",selector:identity,source_snapshot:snapshot.snapshot_id}],
    persisted_to_shadowbot_selector:false,
  };
}

function resolve(snapshot, asset) {
  check(snapshot, asset);
  const candidates = new Map();
  const evidence = [];
  for (const variant of asset.variants) {
    if (variant.enabled === false) continue;
    const raw = snapshot.nodes.filter(node => matches(snapshot, node, variant.selector));
    const valid = raw.filter(node => matches(snapshot, node, asset.target.identity) && !node.disabled);
    evidence.push({variant:variant.id,raw:raw.length,valid:valid.length});
    if (raw.length !== valid.length) stop("VARIANT_IDENTITY_MISMATCH", evidence);
    for (const node of valid) candidates.set(node.index, node);
  }
  if (!candidates.size) stop("ELEMENT_NOT_FOUND", evidence);
  if (candidates.size !== 1) stop("ELEMENT_AMBIGUOUS", evidence);
  const target = [...candidates.values()][0];
  return {
    status:"SNAPSHOT_TARGET_RESOLVED", snapshot_id:snapshot.snapshot_id,
    index:target.index, target:{role:target.role,name:target.name,automation_id:target.automation_id},
    variants:evidence, actions:0, live_target_verified:false, asset_sha256:hash(asset),
  };
}

function propose(snapshot, asset) {
  check(snapshot, asset);
  try { return {status:"EXISTING_VARIANT_VALID",result:resolve(snapshot,asset),asset}; }
  catch (error) { if (error.code !== "ELEMENT_NOT_FOUND") throw error; }
  const found = snapshot.nodes.filter(node => matches(snapshot,node,asset.target.identity) && !node.disabled);
  if (found.length !== 1) stop(found.length ? "REPAIR_AMBIGUOUS" : "REPAIR_IDENTITY_NOT_FOUND",found.length);
  const target = found[0];
  const selector = {
    role:target.role,
    ...(target.automation_id ? {automation_id:target.automation_id} : {name:target.name}),
    ancestor_ids:asset.target.identity.ancestor_ids,
  };
  const repaired = JSON.parse(JSON.stringify(asset));
  const variantId = `repair-${hash(selector).slice(0,12)}`;
  if (!repaired.variants.some(variant => variant.id === variantId)) {
    repaired.variants.push({id:variantId,selector,source_snapshot:snapshot.snapshot_id});
  }
  return {
    status:"REPAIR_PROPOSED", asset:repaired, result:resolve(snapshot,repaired),
    original_variants_retained:true, actions:0, application_modified:false,
    boundary:"Observed single control only; fresh UI observation and business checks are still required.",
  };
}

function redact(snapshot, permittedNames = []) {
  const allowed = new Set(permittedNames);
  const result = JSON.parse(JSON.stringify(snapshot));
  result.focused_element = null;
  for (const node of result.nodes) {
    if (node.name && !allowed.has(node.name)) node.name = "<redacted>";
    node.value = node.value === null || node.value === undefined ? null : "<redacted>";
    for (const prefix of ["session_item_", "search_item_"]) {
      if (node.automation_id.startsWith(prefix) &&
          !allowed.has(node.automation_id.slice(prefix.length))) node.automation_id = "<redacted>";
    }
  }
  return result;
}

module.exports = {LocatorError,fromSky,capture,resolve,propose,redact,hash};

if (require.main === module) {
  let outputFile;
  try {
    const [mode, ...argv] = process.argv.slice(2);
    if (!["query","repair"].includes(mode)) stop("BAD_ARGS","query|repair --snapshot FILE --asset FILE [--out FILE]");
    const options = {};
    for (let i=0;i<argv.length;i+=2) {
      if (!["--snapshot","--asset","--out"].includes(argv[i]) || !argv[i+1] || options[argv[i]]) stop("BAD_ARGS");
      options[argv[i]]=argv[i+1];
    }
    if (!options["--snapshot"] || !options["--asset"]) stop("BAD_ARGS");
    outputFile=options["--out"];
    const snapshot=JSON.parse(fs.readFileSync(options["--snapshot"],"utf8"));
    const asset=JSON.parse(fs.readFileSync(options["--asset"],"utf8"));
    const data=mode === "query" ? resolve(snapshot,asset) : propose(snapshot,asset);
    const result={ok:true,data};
    if (outputFile) fs.writeFileSync(outputFile,JSON.stringify(result,null,2),{flag:"wx"});
    console.log(JSON.stringify(result));
  } catch (error) {
    const result={ok:false,error:{code:error.code || "INTERNAL",message:error.message,detail:error.detail},actions:0};
    if (outputFile && !fs.existsSync(outputFile)) fs.writeFileSync(outputFile,JSON.stringify(result,null,2),{flag:"wx"});
    console.error(JSON.stringify(result));
    process.exitCode=1;
  }
}
