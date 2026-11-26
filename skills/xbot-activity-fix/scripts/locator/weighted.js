async function resolveXbotLocator(activePage, refid) {
  if (!/^(?:f\d+)?e\d+$/.test(refid)) {
    throw new Error("INVALID_REF: expected a ref from the current snapshot");
  }

  function describe(target) {
    if (!target.isConnected) throw new Error("STALE_REF: target node is detached");

    const started = Date.now();
    let evaluations = 0;
    let shadowWalks = 0;
    let usedPosition = false;
    const decisions = [];

    const MAX_EVALS = 1500;
    const MAX_MS = 2500;
    const MAX_SHADOW_WALKS = 200000;

    const attributeCosts = {
      "data-testid": 1,
      "data-test-id": 1,
      "data-test": 2,
      "data-qa": 2,
      "data-cy": 2,
      "data-automation-id": 2,
      id: 3,
      name: 5,
      "aria-label": 7,
      title: 9,
      alt: 10,
      placeholder: 11,
      "class-token": 12,
      href: 14,
      src: 14,
      role: 18,
      type: 20,
    };
    const CUSTOM_DATA_COST = 4;

    // ---------- 基础工具 ----------
    function literal(value) {
      if (!value.includes("'")) return "'" + value + "'";
      if (!value.includes('"')) return '"' + value + '"';
      return "concat(" + value.split("'").map(part => "'" + part + "'").join(',"\'",') + ")";
    }
    function nodeTest(node) {
      const name = node.localName;
      return /^[a-z][a-z0-9]*$/.test(name) && node.namespaceURI === "http://www.w3.org/1999/xhtml"
        ? name
        : "*[local-name()=" + literal(name) + "]";
    }
    function tagInfo(node) {
      return { name: node.localName, ns: node.namespaceURI };
    }
    function dynamic(value) {
      if (!value) return false;
      if (/^:r/.test(value)) return true;                              // React useId
      if (/^[0-9a-f]{8}-[0-9a-f-]{27,}$/i.test(value)) return true;    // UUID
      if (/^\d{6,}$/.test(value)) return true;                         // 纯长数字
      if (/^[a-f0-9]{10,}$/i.test(value)) return true;                 // 纯长 hex
      if (/(?:^|[-_])[a-f0-9]{10,}$/i.test(value)) return true;        // 后缀 hash
      if (/(?:^|[-_])\d{6,}$/.test(value)) return true;                // 后缀长数字
      return false;
    }
    function isSessionUrl(value) {
      return /(?:^|[?&])(token|sid|session|sessionid|auth|signature|sig|expires|exp|nonce|state)=/i.test(value);
    }
    function normalizeText(value) {
      return String(value || "").replace(/[ \t\r\n]+/g, " ").trim();
    }
    function classTokens(node) {
      const raw = node.getAttribute("class");
      return raw ? raw.split(/\s+/).filter(Boolean) : [];
    }
    const UTILITY_PREFIX = /^(?:w|h|p|m|px|py|mx|my|pt|pb|pl|pr|mt|mb|ml|mr|text|bg|border|rounded|flex|grid|gap|font|leading|tracking|shadow|opacity|z|top|left|right|bottom|inset|col|row|order|justify|items|self|overflow|cursor|transition|duration|ease|delay|animate|transform|translate|rotate|scale|filter|ring|divide|outline|fill|stroke|select|pointer|resize|scroll|snap|touch|aspect|columns|break|box|clear|float|isolation|object|visibility|whitespace|word|list|indent|align|vertical|table|caption|empty|sr)-/;
    function stableClassToken(token) {
      if (token.length < 3 || token.length > 40) return false;
      if (/[:\[\]/!]/.test(token)) return false;   // Tailwind 变体 / 任意值
      if (UTILITY_PREFIX.test(token)) return false;
      if (dynamic(token)) return false;
      return true;
    }

    // ---------- ShadowRoot 求值器（受限 XPath 子集） ----------
    function matchesTag(el, tag) {
      return el.localName === tag.name && el.namespaceURI === tag.ns;
    }
    function matchesPred(el, pred) {
      if (pred.type === "attr") return el.getAttribute(pred.name) === pred.value;
      if (pred.type === "class") {
        const cls = el.getAttribute("class") || "";
        return (" " + cls.replace(/[ \t\r\n]+/g, " ").trim() + " ").includes(" " + pred.token + " ");
      }
      if (pred.type === "text") return normalizeText(el.textContent) === pred.value;
      if (pred.type === "textContains") return normalizeText(el.textContent).includes(pred.value);
      return false;
    }
    function allDescendants(root) {
      const out = [];
      const stack = [];
      for (const child of Array.from(root.children || [])) stack.push(child);
      while (stack.length) {
        const n = stack.pop();
        out.push(n);
        if (++shadowWalks > MAX_SHADOW_WALKS) throw new Error("LOCATOR_BUDGET: shadow traversal exceeded its bound");
        if (shadowWalks % 128 === 0 && Date.now() - started > MAX_MS) throw new Error("LOCATOR_BUDGET: shadow traversal time exceeded");
        for (const child of Array.from(n.children || [])) stack.push(child);
      }
      return out;
    }
    function evalShadow(root, struct) {
      let current = [root];
      for (const step of struct) {
        const next = [];
        for (const n of current) {
          if (step.axis === "descendant") {
            for (const el of allDescendants(n)) {
              if (matchesTag(el, step.tag) && step.predicates.every(p => matchesPred(el, p))) next.push(el);
            }
          } else { // child
            const kids = Array.from(n.children || []).filter(el =>
              matchesTag(el, step.tag) && step.predicates.every(p => matchesPred(el, p)));
            if (step.index != null) {
              const hit = kids[step.index - 1];
              if (hit) next.push(hit);
            } else {
              for (const k of kids) next.push(k);
            }
          }
        }
        current = Array.from(new Set(next));
        if (!current.length) break;
      }
      return current;
    }

    // ---------- 验证器 ----------
    function makeVerifier(root, expected) {
      const isDoc = root.nodeType === 9;
      return function check(candidate, ignoreBudget) {
        if (!ignoreBudget && (++evaluations > MAX_EVALS || Date.now() - started > MAX_MS)) {
          throw new Error("LOCATOR_BUDGET: candidate search exceeded its bound");
        }
        if (isDoc) {
          const result = root.evaluate(candidate.path, root, null, 7, null);
          return result.snapshotLength === 1 && result.snapshotItem(0) === expected;
        }
        const nodes = evalShadow(root, candidate.struct);
        return nodes.length === 1 && nodes[0] === expected;
      };
    }

    // ---------- 候选生成（结构化 + 字符串） ----------
    function candidates(node) {
      const tag = nodeTest(node);
      const info = tagInfo(node);
      const result = [{
        path: tag, cost: 45, reason: "unique_tag",
        struct: [{ axis: "descendant", tag: info, predicates: [], index: null }],
      }];
      const predicates = [];

      for (const [name, cost] of Object.entries(attributeCosts)) {
        if (name === "class-token") continue;
        const value = node.getAttribute(name);
        if (value === null || value === "" || value.length > 160) continue;
        if (name === "id" && dynamic(value)) continue;
        if ((name === "href" || name === "src") && isSessionUrl(value)) continue;
        predicates.push({
          text: "@" + name + "=" + literal(value), cost, reason: name,
          pred: { type: "attr", name, value },
        });
      }

      for (const attr of Array.from(node.attributes || [])) {
        const name = attr.name;
        if (!name.startsWith("data-") || attributeCosts[name] !== undefined) continue;
        const value = attr.value;
        if (!value || value.length > 160 || dynamic(value)) continue;
        predicates.push({
          text: "@" + name + "=" + literal(value), cost: CUSTOM_DATA_COST, reason: name,
          pred: { type: "attr", name, value },
        });
      }

      for (const token of classTokens(node).filter(stableClassToken).slice(0, 3)) {
        predicates.push({
          text: "contains(concat(' ', normalize-space(@class), ' '), " + literal(" " + token + " ") + ")",
          cost: attributeCosts["class-token"], reason: "class:" + token,
          pred: { type: "class", token },
        });
      }

      const text = normalizeText(node.textContent);
      if (text && text.length <= 80 && !["script", "style", "input", "textarea"].includes(node.localName)) {
        predicates.push({
          text: "normalize-space(.)=" + literal(text), cost: 16 + text.length / 100, reason: "text",
          pred: { type: "text", value: text },
        });
        if (text.length >= 4) {
          const prefix = text.slice(0, Math.min(40, text.length));
          predicates.push({
            text: "contains(normalize-space(.), " + literal(prefix) + ")",
            cost: 30 + prefix.length / 100, reason: "text:contains",
            pred: { type: "textContains", value: prefix },
          });
        }
      }

      predicates.sort((a, b) => a.cost - b.cost);
      for (const item of predicates) {
        result.push({
          path: tag + "[" + item.text + "]", cost: item.cost, reason: item.reason,
          struct: [{ axis: "descendant", tag: info, predicates: [item.pred], index: null }],
        });
      }

      const comboLimit = Math.min(8, predicates.length);
      for (let a = 0; a < comboLimit; a++) {
        for (let b = a + 1; b < comboLimit; b++) {
          result.push({
            path: tag + "[" + predicates[a].text + " and " + predicates[b].text + "]",
            cost: predicates[a].cost + predicates[b].cost + 3,
            reason: predicates[a].reason + "+" + predicates[b].reason,
            struct: [{
              axis: "descendant", tag: info,
              predicates: [predicates[a].pred, predicates[b].pred], index: null,
            }],
          });
        }
      }
      return result.sort((x, y) => x.cost - y.cost);
    }

    // ---------- 单个 scope 内求解 ----------
    function localPath(node, root) {
      const isDoc = root.nodeType === 9;
      const verify = makeVerifier(root, node);
      const own = candidates(node);
      const options = [];

      // 1) 自身候选：//tag[pred]
      for (const item of own) {
        options.push({ ...item, path: "//" + item.path });
      }

      // 2) 祖先锚点短路径：//anchor//item（descendant）与 //anchor/item（child）
      let ancestor = node.parentElement;
      for (let depth = 1; ancestor && ancestor.getRootNode() === root && depth <= 5;
           depth++, ancestor = ancestor.parentElement) {
        if (["html", "body"].includes(ancestor.localName)) continue;
        const anchors = candidates(ancestor)
          .filter(item => !["unique_tag", "text", "text:contains"].includes(item.reason))
          .slice(0, 5);
        for (const anchor of anchors) {
          for (const item of own.slice(0, 8)) {
            const first = item.struct[0];
            options.push({
              path: "//" + anchor.path + "//" + item.path,
              struct: [...anchor.struct, ...item.struct],
              cost: anchor.cost + item.cost + 8 + depth * 2,
              reason: "ancestor(" + anchor.reason + ")/" + item.reason,
            });
            if (depth === 1) options.push({
              path: "//" + anchor.path + "/" + item.path,
              struct: [...anchor.struct, {
                axis: "child", tag: first.tag, predicates: first.predicates, index: null,
              }],
              cost: anchor.cost + item.cost + 6 + depth * 2,
              reason: "ancestor(" + anchor.reason + ")/child/" + item.reason,
            });
          }
        }
      }

      options.sort((a, b) => a.cost - b.cost || a.path.length - b.path.length ||
        (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));

      const seen = new Set();
      let budgetExhausted = false;
      for (const option of options) {
        if (seen.has(option.path)) continue;
        seen.add(option.path);
        try {
          if (verify(option)) {
            decisions.push({ path: option.path, reason: option.reason, cost: option.cost });
            return option.path;
          }
        } catch (error) {
          if (/^LOCATOR_BUDGET/.test(error.message || "")) { budgetExhausted = true; break; }
          throw error;
        }
      }

      // 3) 结构回退（预算耗尽时也走这里，做一次强制验证）
      const steps = [];
      const structSteps = [];
      for (let current = node; current && current !== root; current = current.parentElement) {
        const parent = current.parentNode;
        if (!parent) break;
        const siblings = Array.from(parent.children || []).filter(item =>
          item.localName === current.localName && item.namespaceURI === current.namespaceURI);
        const index = siblings.indexOf(current) + 1;
        steps.unshift(nodeTest(current) + "[" + index + "]");
        structSteps.unshift({ axis: "child", tag: tagInfo(current), predicates: [], index });
      }
      if (!steps.length) throw new Error("IDENTITY_MISMATCH: empty structural path");
      const fallbackPath = (isDoc ? "" : ".") + "/" + steps.join("/");
      const fallbackCandidate = { path: fallbackPath, struct: structSteps };
      if (!verify(fallbackCandidate, true)) {
        throw new Error("IDENTITY_MISMATCH: structural fallback did not resolve the original node");
      }
      usedPosition = true;
      decisions.push({
        path: fallbackPath,
        reason: budgetExhausted ? "structural_fallback(budget)" : "structural_fallback",
        cost: 1000,
      });
      return fallbackPath;
    }

    // ---------- 沿 ShadowRoot 链向上 ----------
    let node = target;
    let path = "";
    let shadowDepth = 0;
    for (;;) {
      const root = node.getRootNode();
      const segment = localPath(node, root);
      path = segment + (path ? "/xbotShadowRoot" + path.replace(/^\.\//, "/") : "");
      if (!root.host) break;
      if (++shadowDepth > 8) throw new Error("CONTEXT_TOO_DEEP: more than 8 shadow roots");
      node = root.host;
    }

    // ---------- 建议 CSS（best-effort） ----------
    let suggestedCss = null;
    if (shadowDepth === 0) {
      const testName = target.hasAttribute("data-testid") ? "data-testid" : "data-test-id";
      const testid = target.getAttribute(testName);
      if (testid && !dynamic(testid)) {
        suggestedCss = "[" + testName + "=" + CSS.escape(testid) + "]";
      } else {
        const id = target.getAttribute("id");
        if (id && !dynamic(id) && /^[A-Za-z_][\w-]*$/.test(id)) suggestedCss = "#" + id;
      }
      if (suggestedCss) {
        const matches = target.getRootNode().querySelectorAll(suggestedCss);
        if (matches.length !== 1 || matches[0] !== target) suggestedCss = null;
      }
    }

    return {
      path,
      shadow_depth: shadowDepth,
      uses_position: usedPosition,
      decisions,
      evaluations,
      identity_verified: true,
      tag: target.localName,
      suggested_css: suggestedCss,
    };
  }

  // ---------- 取句柄 ----------
  let handle;
  try {
    handle = await activePage.locator("aria-ref=" + refid).elementHandle({ timeout: 3000 });
  } catch (error) {
    if (/Timeout \d+ms exceeded/i.test(error.message || "")) {
      throw new Error("STALE_REF: target is missing; take a new snapshot before resolving again");
    }
    throw error;
  }
  if (!handle) throw new Error("STALE_REF: snapshot again before resolving the target");

  const handles = [handle];
  try {
    const target = await handle.evaluate(describe);
    const contexts = [target];
    let frame = await handle.ownerFrame();
    const targetUrl = frame.url();

    while (frame.parentFrame()) {
      if (contexts.length > 16) throw new Error("CONTEXT_TOO_DEEP: more than 16 iframe levels");
      const frameElement = await frame.frameElement();
      handles.push(frameElement);
      const context = await frameElement.evaluate(describe);
      if (context.shadow_depth) {
        throw new Error("UNSUPPORTED_SHADOW_IFRAME: Xbot does not support a frame hosted inside ShadowRoot");
      }
      contexts.unshift(context);
      frame = frame.parentFrame();
    }

    const frameDepth = contexts.length - 1;
    const shadowDepth = contexts.reduce((sum, ctx) => sum + ctx.shadow_depth, 0);

    return {
      algorithm: "weighted_dom_v2",
      refid,
      xpath: contexts.map(ctx => ctx.path),
      frame_depth: frameDepth,
      shadow_depth: shadowDepth,
      raw_xpath_compatible: frameDepth === 0 && shadowDepth === 0,
      requires_engine_selector: frameDepth > 0 || shadowDepth > 0,
      identity_verified: contexts.every(ctx => ctx.identity_verified),
      quality: contexts.some(ctx => ctx.uses_position) ? "structural" : "semantic",
      persisted: false,
      target_url: targetUrl,
      suggested_css: contexts[contexts.length - 1].suggested_css || null,
      diagnostics: {
        evaluations: contexts.reduce((sum, ctx) => sum + ctx.evaluations, 0),
        decisions: contexts.map(ctx => ctx.decisions),
      },
      contexts,
    };
  } finally {
    for (const item of handles) await item.dispose();
  }
}
