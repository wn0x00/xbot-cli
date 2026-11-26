async function hybridBrowser(activePage, request, sources) {
  const weighted = new Function("return (" + sources.weighted + ");")();
  const robula = new Function("return (" + sources.robulaAdapter + ");")();

  function profile(element) {
    const norm = value => String(value || "").replace(/[ \t\r\n]+/g, " ").trim();
    const regions = [];
    const hosts = [];
    let current = element;
    for (let depth = 0; current && depth < 64; depth++) {
      if (current !== element && current.hasAttribute("data-region")) regions.push(current.getAttribute("data-region"));
      if (current.parentElement) current = current.parentElement;
      else {
        const root = current.getRootNode();
        if (!root.host) break;
        hosts.unshift(root.host.localName);
        current = root.host;
      }
    }
    const view = element.ownerDocument.defaultView;
    const style = view.getComputedStyle(element);
    const rect = element.getBoundingClientRect();
    const visible = style.visibility !== "hidden" && style.display !== "none" && rect.width > 0 && rect.height > 0;
    return {
      tag: element.localName,
      role: element.getAttribute("role") || (element.localName === "button" ? "button" : element.localName === "input" ? "textbox" : element.localName),
      name: norm(element.getAttribute("aria-label") || element.textContent),
      business_key: element.getAttribute("data-business-key"),
      regions, hosts, visible,
      enabled: !element.matches(":disabled") && element.getAttribute("aria-disabled") !== "true",
      origin: view.location.origin,
      path: view.location.pathname,
      time_origin: view.performance.timeOrigin,
    };
  }
  async function getProfile(handle) {
    const value = await handle.evaluate(profile);
    const frames = [];
    let frame = await handle.ownerFrame();
    while (frame) {
      frames.unshift(await frame.evaluate(() => ({ origin: location.origin, time_origin: performance.timeOrigin })));
      frame = frame.parentFrame();
    }
    return { ...value, frames, top_url: activePage.url() };
  }
  function identityErrors(value, contract) {
    const errors = [];
    const top = new URL(value.top_url);
    if (top.origin !== contract.top_origin || top.pathname !== contract.top_path) errors.push("PAGE_CONTEXT");
    if (value.tag !== contract.tag || value.role !== contract.role) errors.push("ROLE");
    if (contract.business_key) {
      if (value.business_key !== contract.business_key) errors.push("BUSINESS_KEY");
    } else if (value.name !== contract.name) errors.push("NAME");
    if (JSON.stringify(value.regions) !== JSON.stringify(contract.regions)) errors.push("REGION");
    if (JSON.stringify(value.hosts) !== JSON.stringify(contract.hosts)) errors.push("SHADOW_CONTEXT");
    if (JSON.stringify(value.frames.map(frame => frame.origin)) !== JSON.stringify(contract.frame_origins)) errors.push("FRAME_CONTEXT");
    return errors;
  }
  async function describeRef(refid) {
    const handle = await activePage.locator("aria-ref=" + refid).elementHandle({ timeout: 2500 });
    if (!handle) throw new Error("STALE_REF");
    try { return await getProfile(handle); } finally { await handle.dispose(); }
  }
  async function query(xpath) {
    let roots = [await activePage.evaluateHandle(() => document)];
    const allocated = [...roots];
    try {
      for (let frameIndex = 0; frameIndex < xpath.length; frameIndex++) {
        const parts = xpath[frameIndex].split("/xbotShadowRoot");
        for (let shadowIndex = 0; shadowIndex < parts.length; shadowIndex++) {
          let expression = parts[shadowIndex];
          if (shadowIndex > 0) expression = expression.startsWith("//") ? "." + expression : "./" + expression.replace(/^\/+/, "");
          const matches = [];
          for (const root of roots) {
            const array = await root.evaluateHandle((root, expression) => {
              const document = root.nodeType === 9 ? root : root.ownerDocument;
              // XPath cannot query ShadowRoot. A detached element preserves relative
              // element paths; map matched clones back by child-node indices.
              let context = root;
              if (root.nodeType === 11) {
                context = document.createElement("div");
                for (const child of root.childNodes) context.appendChild(child.cloneNode(true));
              }
              const result = document.evaluate(expression, context, null, 7, null);
              const nodes = [];
              for (let i = 0; i < result.snapshotLength; i++) {
                let node = result.snapshotItem(i);
                if (root.nodeType === 11) {
                  const indexes = [];
                  for (let current = node; current !== context; current = current.parentNode) {
                    if (!current?.parentNode) throw new Error("SHADOW_QUERY_ESCAPED_SCOPE");
                    indexes.unshift(Array.prototype.indexOf.call(current.parentNode.childNodes, current));
                  }
                  node = root;
                  for (const index of indexes) node = node.childNodes[index];
                }
                if (node?.nodeType === 1 && !nodes.includes(node)) nodes.push(node);
              }
              if (nodes.length > 100) throw new Error("MATCH_LIMIT");
              return nodes;
            }, expression);
            allocated.push(array);
            const length = await array.evaluate(nodes => nodes.length);
            for (let i = 0; i < length; i++) {
              const item = await array.evaluateHandle((nodes, index) => nodes[index], i);
              allocated.push(item);
              matches.push(item);
            }
          }
          roots = matches;
          if (shadowIndex < parts.length - 1) {
            roots = [];
            for (const item of matches) {
              const shadow = await item.evaluateHandle(element => element.shadowRoot);
              allocated.push(shadow);
              if (await shadow.evaluate(root => Boolean(root))) roots.push(shadow);
            }
          }
        }
        if (frameIndex < xpath.length - 1) {
          const frames = [];
          for (const item of roots) {
            const element = item.asElement();
            const frame = element && await element.contentFrame();
            if (!frame) continue;
            const root = await frame.evaluateHandle(() => document);
            allocated.push(root);
            frames.push(root);
          }
          roots = frames;
        }
      }
      return { handles: roots, dispose: async () => { for (const item of allocated.reverse()) await item.dispose(); } };
    } catch (error) {
      for (const item of allocated.reverse()) await item.dispose();
      throw error;
    }
  }
  async function observations(locators, contract) {
    const all = [];
    const owned = [];
    const evidence = [];
    try {
      for (const item of locators) {
        try {
          const result = await query(item.xpath);
          owned.push(result);
          const entries = [];
          for (const handle of result.handles) {
            const value = await getProfile(handle);
            const errors = identityErrors(value, contract);
            entries.push({ business_key: value.business_key, visible: value.visible, enabled: value.enabled, identity_errors: errors });
            if (!errors.length && value.visible && value.enabled) {
              let duplicate = false;
              for (const other of all) {
                if (await handle.evaluate((node, other) => node === other, other)) { duplicate = true; break; }
              }
              if (!duplicate) all.push(handle);
            }
          }
          evidence.push({ source: item.source, xpath: item.xpath, count: result.handles.length, entries });
        } catch (error) { evidence.push({ source: item.source, xpath: item.xpath, error: error.message }); }
      }
      return { evidence, unique_identity_count: all.length };
    } finally { for (const result of owned) await result.dispose(); }
  }
  async function generate(refid, contract) {
    const before = await describeRef(refid);
    if (contract && identityErrors(before, contract).length) throw new Error("REF_IDENTITY_MISMATCH");
    const candidates = [];
    const errors = [];
    for (const [name, run] of [
      ["weighted_v2", () => weighted(activePage, refid)],
      ["robula_plus", () => robula(activePage, refid, sources.robulaLibrary)],
    ]) {
      try {
        const result = await run();
        const quality = result.quality || (result.xpath.some(part => /\[\d+\]/.test(part)) ? "structural" : "semantic");
        const dynamicId = result.xpath.some(part => /@id=['"][^'"]*(?:\d{6,}|:r|[a-f0-9]{10,})/.test(part));
        const generic = result.xpath.some(part => /^\/\/[a-z-]+$/.test(part));
        const rank = quality === "structural" ? 90 : dynamicId ? 80 : generic ? 60 : name === "weighted_v2" ? 10 : 20;
        candidates.push({ source: name, xpath: result.xpath, rank, quality,
          raw_xpath_compatible: result.raw_xpath_compatible, requires_engine_selector: result.requires_engine_selector });
      } catch (error) { errors.push({ source: name, error: error.message.split("\n")[0].slice(0, 300) }); }
    }
    const after = await describeRef(refid);
    if (JSON.stringify(before) !== JSON.stringify(after)) throw new Error("DOCUMENT_CHANGED_DURING_GENERATION");
    const verified = [];
    const expectedHandle = await activePage.locator("aria-ref=" + refid).elementHandle();
    try {
      for (const candidate of candidates) {
        const result = await query(candidate.xpath);
        try {
          if (result.handles.length !== 1 || !await expectedHandle.evaluate((node, other) => node === other, result.handles[0])) {
            errors.push({ source: candidate.source, error: "FINAL_IDENTITY_MISMATCH", xpath: candidate.xpath });
          } else verified.push(candidate);
        } finally { await result.dispose(); }
      }
    } finally { await expectedHandle.dispose(); }
    if (!verified.length) throw new Error("NO_VERIFIED_LOCATOR: " + JSON.stringify(errors));
    verified.sort((a, b) => a.rank - b.rank || JSON.stringify(a.xpath).length - JSON.stringify(b.xpath).length);
    return { primary: verified[0], alternatives: verified.slice(1), errors, profile: after };
  }

  if (request.operation === "capture") {
    const generated = await generate(request.refid);
    const value = generated.profile;
    const top = new URL(value.top_url);
    const contract = {
      tag: value.tag, role: value.role, name: value.name, business_key: value.business_key,
      regions: value.regions, hosts: value.hosts, frame_origins: value.frames.map(frame => frame.origin),
      top_origin: top.origin, top_path: top.pathname,
      auto_repair_allowed: Boolean(value.business_key && value.regions.length),
    };
    return { status: "CAPTURED", contract, generated };
  }
  const contract = request.asset.contract;
  const page = new URL(activePage.url());
  if (page.origin !== contract.top_origin || page.pathname !== contract.top_path) {
    return { status: "BLOCKED_PAGE_CONTEXT", changed: false };
  }
  const old = [request.asset.generated.primary, ...request.asset.generated.alternatives];
  const diagnosis = await observations(old, contract);
  if (request.operation === "diagnose") return { status: "DIAGNOSED", diagnosis };

  const possible = [];
  for (const refid of request.refids) {
    const value = await describeRef(refid);
    if (!identityErrors(value, contract).length) possible.push({ refid, profile: value });
  }
  const visible = possible.filter(item => item.profile.visible);
  if (!visible.length) return { status: "BLOCKED_TARGET_MISSING", diagnosis, possible, changed: false };
  if (visible.length !== 1) return { status: "BLOCKED_AMBIGUOUS", diagnosis, possible, changed: false };
  if (!visible[0].profile.enabled) return { status: "BLOCKED_DISABLED", diagnosis, possible, changed: false };
  if (!contract.auto_repair_allowed) return { status: "REVIEW_REQUIRED_WEAK_IDENTITY", diagnosis, possible, changed: false };
  const generated = await generate(visible[0].refid, contract);
  return {
    status: "REPAIR_VALIDATED", diagnosis, selected_ref: visible[0].refid, generated,
    identity_evidence: "pinned business key + region + tag/role + frame origins + shadow hosts; unique visible enabled target",
    changed: JSON.stringify(generated.primary.xpath) !== JSON.stringify(request.asset.generated.primary.xpath),
    persisted: false,
  };
}
