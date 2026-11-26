async function guardedBrowser(activePage, request, sources, original) {
  const queryKeys = ["record"];
  function pageIdentity(url) {
    const parsed = new URL(url);
    return { origin: parsed.origin, path: parsed.pathname, hash: parsed.hash,
      query: Object.fromEntries(queryKeys.map(key => [key, parsed.searchParams.get(key)])) };
  }
  function inspect(element) {
    const norm = value => String(value || "").replace(/[ \t\r\n]+/g, " ").trim();
    const attributes = (node, names) => Object.fromEntries(names.map(name => [name, node.getAttribute(name)]));
    const scopeNames = ["data-region", "data-record-id", "data-workflow-key", "data-dialog-id"];
    const hostNames = ["data-host-key", "data-record-id", "data-region"];
    const ancestry = [];
    const hosts = [];
    const blockers = [];
    let current = element;
    for (let depth = 0; current && depth < 64; depth++) {
      const style = current.ownerDocument.defaultView.getComputedStyle(current);
      if (current.hidden || style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse" || Number(style.opacity) === 0) blockers.push("HIDDEN");
      if (current.hasAttribute("inert")) blockers.push("INERT");
      if (current.matches(":disabled") || current.getAttribute("aria-disabled") === "true") blockers.push("DISABLED");
      if (style.pointerEvents === "none") blockers.push("POINTER_EVENTS");
      const scope = attributes(current, scopeNames);
      if (Object.values(scope).some(Boolean)) ancestry.push(scope);
      if (current.parentElement) current = current.parentElement;
      else {
        const root = current.getRootNode();
        if (!root.host) break;
        hosts.unshift({ tag: root.host.localName, attributes: attributes(root.host, hostNames), mode: root.mode });
        current = root.host;
      }
    }
    const document = element.ownerDocument;
    const rect = element.getBoundingClientRect();
    const view = document.defaultView;
    const inViewport = rect.top < view.innerHeight && rect.left < view.innerWidth && rect.bottom > 0 && rect.right > 0;
    const hit = { in_viewport: inViewport, clear: false };
    if (inViewport && rect.width > 0 && rect.height > 0) {
      const x = Math.min(view.innerWidth - 1, Math.max(0, rect.left + rect.width / 2));
      const y = Math.min(view.innerHeight - 1, Math.max(0, rect.top + rect.height / 2));
      hit.point = { x, y };
      const root = element.getRootNode();
      const top = (root.elementFromPoint ? root : document).elementFromPoint(x, y);
      hit.clear = top === element || element.contains(top);
    }
    const root = document.documentElement;
    return {
      connected: element.isConnected, tag: element.localName,
      role: element.getAttribute("role") || (element.localName === "button" ? "button" : element.localName),
      name: norm(element.getAttribute("aria-label") || element.textContent),
      business_key: element.getAttribute("data-business-key"),
      page: attributes(root, ["data-app", "data-page-id", "data-record-id"]),
      page_state: root.getAttribute("data-page-state"),
      ancestry, hosts, blockers: [...new Set(blockers)], hit,
      positive_rect: rect.width > 0 && rect.height > 0,
      time_origin: view.performance.timeOrigin,
    };
  }
  function inspectFrame(element, childPoint) {
    const names = ["data-frame-key", "data-record-id"];
    const attrs = Object.fromEntries(names.map(name => [name, element.getAttribute(name)]));
    const rect = element.getBoundingClientRect();
    const style = element.ownerDocument.defaultView.getComputedStyle(element);
    const root = element.getRootNode();
    let blocked = !rect.width || !rect.height || style.display === "none" || style.visibility === "hidden" || Number(style.opacity) === 0;
    let unsupportedTransform = false;
    for (let node = element; node; node = node.parentElement) {
      const value = node.ownerDocument.defaultView.getComputedStyle(node);
      blocked = blocked || node.hidden || node.hasAttribute("inert") || value.display === "none" || value.visibility === "hidden" || Number(value.opacity) === 0;
      unsupportedTransform = unsupportedTransform || value.transform !== "none" || !["normal", "1"].includes(value.zoom);
    }
    let point = null;
    if (childPoint && !unsupportedTransform) {
      point = { x: rect.left + element.clientLeft + childPoint.x, y: rect.top + element.clientTop + childPoint.y };
      const top = element.ownerDocument.elementFromPoint(point.x, point.y);
      blocked = blocked || top !== element;
    } else blocked = true;
    return { tag: element.localName, attrs, shadow_iframe: Boolean(root.host), blocked, unsupported_transform: unsupportedTransform, point };
  }
  async function read(refid) {
    const handle = await activePage.locator("aria-ref=" + refid).elementHandle({ timeout: 1000 });
    if (!handle) throw new Error("STALE_REF");
    try {
      const value = await handle.evaluate(inspect);
      const frames = [];
      let frame = await handle.ownerFrame();
      let point = value.hit.point;
      while (frame?.parentFrame()) {
        const frameElement = await frame.frameElement();
        try {
          const context = await frameElement.evaluate(inspectFrame, point);
          frames.unshift({ ...context, url: pageIdentity(frame.url()) });
          point = context.point;
        } finally { await frameElement.dispose(); }
        frame = frame.parentFrame();
      }
      const topPage = await activePage.evaluate(() => ({
        state: document.documentElement.getAttribute("data-page-state"),
        identity: Object.fromEntries(["data-app", "data-page-id", "data-record-id"].map(key => [key, document.documentElement.getAttribute(key)])),
      }));
      return { ...value, frames, top_page: topPage, top_url: pageIdentity(activePage.url()) };
    } finally { await handle.dispose(); }
  }
  const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  function differences(value, contract) {
    const errors = [];
    if (!equal(value.top_url, contract.top_url) || !equal(value.top_page.identity, contract.top_page)) errors.push("PAGE_CONTEXT");
    if (!equal(value.page, contract.page)) errors.push("DOCUMENT_CONTEXT");
    if (value.tag !== contract.tag || value.role !== contract.role || value.business_key !== contract.business_key) errors.push("TARGET_IDENTITY");
    if (!equal(value.ancestry, contract.ancestry)) errors.push("RECORD_SCOPE");
    if (!equal(value.hosts, contract.hosts)) errors.push("SHADOW_CONTEXT");
    const frameIdentity = frames => frames.map(({ tag, attrs, url }) => ({ tag, attrs, url }));
    if (!equal(frameIdentity(value.frames), frameIdentity(contract.frames))) errors.push("FRAME_CONTEXT");
    return errors;
  }
  function stamp(value) {
    return {
      top_url: value.top_url, top_page: value.top_page, page: value.page,
      tag: value.tag, role: value.role, name: value.name, business_key: value.business_key,
      ancestry: value.ancestry, hosts: value.hosts, frames: value.frames,
      blockers: value.blockers, hit: value.hit, time_origin: value.time_origin,
    };
  }
  if (request.operation === "capture") {
    const guard = await read(request.refid);
    if (guard.frames.some(frame => frame.shadow_iframe)) return { status: "BLOCKED_UNSUPPORTED_SHADOW_IFRAME" };
    if (guard.top_page.state !== "ready") return { status: "BLOCKED_PAGE_STATE", page_state: guard.top_page.state, persisted: false };
    if (!guard.connected || !guard.positive_rect || guard.blockers.length || guard.frames.some(frame => frame.blocked)) {
      return { status: "BLOCKED_NOT_ACTIONABLE", blockers: guard.blockers, persisted: false };
    }
    const captured = await original(activePage, request, sources);
    captured.contract.guard = {
      ...stamp(guard), top_page: guard.top_page.identity,
      query_keys: queryKeys, allowed_names: [guard.name],
    };
    captured.contract.auto_repair_allowed = captured.contract.auto_repair_allowed && Boolean(
      guard.top_page.identity["data-page-id"] && guard.top_page.identity["data-record-id"] &&
      guard.ancestry.some(scope => scope["data-record-id"]) &&
      guard.frames.every(frame => frame.attrs["data-frame-key"]) &&
      guard.hosts.every(host => host.attributes["data-host-key"])
    );
    return captured;
  }
  const asset = request.asset;
  if (!asset?.contract?.guard) return { status: "REVIEW_REQUIRED_NO_IDENTITY_BASELINE", persisted: false };
  const contract = asset.contract.guard;
  const context = await activePage.evaluate(() => ({
    identity: Object.fromEntries(["data-app", "data-page-id", "data-record-id"].map(key => [key, document.documentElement.getAttribute(key)])),
    state: document.documentElement.getAttribute("data-page-state"),
  }));
  if (!equal(pageIdentity(activePage.url()), contract.top_url) || !equal(context.identity, contract.top_page)) {
    return { status: "BLOCKED_PAGE_CONTEXT", persisted: false };
  }
  if (context.state !== "ready") {
    return { status: context.state === "loading" ? "WAIT_PAGE_LOADING" : "BLOCKED_PAGE_STATE", page_state: context.state, persisted: false };
  }
  const diagnosis = await original(activePage, { operation: "diagnose", asset }, sources);
  if (diagnosis.diagnosis.evidence.some(item => item.error)) {
    return { status: "BLOCKED_LOCATOR_ERROR", diagnosis: diagnosis.diagnosis, persisted: false };
  }
  if (request.operation === "diagnose") return { status: "DIAGNOSED", diagnosis: diagnosis.diagnosis, persisted: false };
  const possible = [];
  const rejected = [];
  for (const refid of request.refids || []) {
    let value;
    try { value = await read(refid); }
    catch (error) { return { status: "BLOCKED_STALE_SNAPSHOT", error: error.message.split("\n")[0], persisted: false }; }
    if (value.frames.some(frame => frame.shadow_iframe)) return { status: "BLOCKED_UNSUPPORTED_SHADOW_IFRAME", persisted: false };
    const errors = differences(value, contract);
    if (errors.length) { rejected.push({ refid, errors }); continue; }
    if (!value.positive_rect || value.blockers.includes("HIDDEN")) { rejected.push({ refid, errors: ["HIDDEN"] }); continue; }
    possible.push({ refid, value });
  }
  const evidence = { diagnosis: diagnosis.diagnosis, rejected, candidate_count: possible.length, persisted: false };
  if (!possible.length) return { status: "BLOCKED_TARGET_MISSING", ...evidence };
  if (possible.length !== 1) return { status: "BLOCKED_AMBIGUOUS", ...evidence };
  const target = possible[0];
  if (target.value.blockers.length || target.value.frames.some(frame => frame.blocked)) {
    return { status: "BLOCKED_NOT_ACTIONABLE", blockers: target.value.blockers, ...evidence };
  }
  if (!target.value.hit.in_viewport) return { status: "REVIEW_REQUIRED_SCROLL_RECHECK", ...evidence };
  if (!target.value.hit.clear) return { status: "BLOCKED_OCCLUDED", ...evidence };
  if (!contract.allowed_names.includes(target.value.name)) return { status: "REVIEW_REQUIRED_ACTION_NAME_CHANGED", ...evidence };
  if (!asset.contract.auto_repair_allowed) return { status: "REVIEW_REQUIRED_WEAK_IDENTITY", ...evidence };
  if (request.operation === "revalidate") {
    const before = request.plan;
    if (!before || before.asset_version !== asset.version || !equal(before.guard_stamp, stamp(target.value))) {
      return { status: "BLOCKED_STALE_PLAN", ...evidence };
    }
  }
  const result = await original(activePage, { operation: "repair", asset, refids: [target.refid] }, sources);
  if (result.status !== "REPAIR_VALIDATED") return { ...result, guard_checked: true };
  const after = await read(target.refid);
  if (!equal(stamp(after), stamp(target.value))) return { status: "BLOCKED_DOM_CHANGED", ...evidence };
  if (result.generated.primary.quality === "structural" || result.generated.primary.rank >= 60) {
    return { status: "REVIEW_REQUIRED_FRAGILE_LOCATOR", ...evidence, generated: result.generated };
  }
  return {
    ...result, status: request.operation === "revalidate" ? "ACTION_REVALIDATED" : "REPAIR_VALIDATED",
    asset_version: asset.version, guard_stamp: stamp(after),
    guard_checked: true, diagnosis: diagnosis.diagnosis,
  };
}
