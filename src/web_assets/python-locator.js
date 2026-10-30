(function () {
  function literal(value) {
    if (!value.includes("'")) return "'" + value + "'";
    if (!value.includes('"')) return '"' + value + '"';
    return "concat(" + value.split("'").map((v) => "'" + v + "'").join(', "\'", ') + ")";
  }
  const HTML = "http://www.w3.org/1999/xhtml";
  const generic = /^(root|app|main|content|container|wrapper|page|layout|__next|__nuxt)$/i;
  const weakClass = /^(active|selected|hover|focus|disabled|enabled|open|closed|show|hide|current|is-.+|has-.+|row|col|column|grid|flex|block|hidden|layout|container|wrapper|content|main|section|item|cell|btn|button|link)$/i;
  const frameworkId = /^(?:[:]|react-|headlessui-|radix-|ember\d|el-id-|mui-\d|rc_select_)/i;
  function normalize(value) { return String(value || "").replace(/[ \t\r\n]+/g, " ").replace(/^ | $/g, ""); }
  function useful(value) { return Boolean(value && normalize(value) && value.length <= 160); }
  function generated(value) {
    return /^[0-9a-f]{8}-[0-9a-f-]{27,}$/i.test(value) ||
      /(?:^|[-_])[0-9a-f]{8,}(?:$|[-_])/i.test(value) ||
      /(?:^|[-_])\d{3,}$/.test(value) ||
      /[-_](?=[a-z0-9]{6,}$)(?=[a-z0-9]*\d)(?=[a-z0-9]*[a-z])[a-z0-9]+$/i.test(value) ||
      /^\d+$/.test(value);
  }
  function stableClass(value) {
    return useful(value) && !weakClass.test(value) && !generated(value) &&
      !/^(?:css|sc|jss|jsx)-|^(?:el|van|ivu|arco|semi|mui|ant)-|^(?:p|m|w|h|gap|text|bg|border|rounded|items|justify)-|___|^[a-zA-Z]{0,3}\d{3,}/.test(value);
  }
  function tagPath(el) {
    if (el.namespaceURI === HTML && /^[a-z][a-z0-9-]*$/.test(el.localName)) return el.localName;
    return "*[local-name()=" + literal(el.localName) + " and namespace-uri()=" + literal(el.namespaceURI || "") + "]";
  }
  function attrCss(name, value, operator = "=") {
    return "[" + CSS.escape(name) + operator + JSON.stringify(value) + "]";
  }
  function semanticText(value) {
    return useful(value) && value.length <= 80 && !/^\W*[\d\s.,:/%+-]+\W*$/.test(value) &&
      !/\d{4}[-/]\d{1,2}[-/]\d{1,2}|\b\d{1,2}:\d{2}\b|(?:^|[ \t])\d+[ \t]*$/.test(value);
  }
  function terms(el, ancestor) {
    const result = [], tag = CSS.escape(el.localName);
    function add(xpath, css, score, strategy, filters = [], warnings = [], quality = "semantic") {
      result.push({xpath, css: tag + css, score, strategy, filters, warnings, quality});
    }
    const attributes = [
      ["data-testid", 130], ["data-test", 128], ["data-automation-id", 128],
      ["data-qa", 126], ["data-cy", 126], ["data-business-key", 124],
      ["data-record-id", 122], ["data-id", 116], ["id", 120],
      ["name", 105], ["aria-label", 103], ["placeholder", 98],
      ["title", 96], ["alt", 94], ["for", 87], ["role", 42], ["type", 36]
    ];
    for (const a of Array.from(el.attributes)) {
      if (/^data-(?:[a-z][a-z0-9-]*-)?(?:id|key|code|uid|slug)$/.test(a.name) &&
          !/session|token|nonce|track|spm/i.test(a.name) && !attributes.some(([name]) => name === a.name)) {
        attributes.push([a.name, 112]);
      }
    }
    if (el.localName.includes("-")) attributes.push(["icon", 95]);
    for (const [name, score] of attributes) {
      const value = el.getAttribute(name);
      if (!useful(value)) continue;
      if (name === "icon" && /(?:line|fill)\/\d+$/.test(value)) {
        const prefix = value.replace(/(?:line|fill)\/\d+$/, "");
        add("starts-with(@icon," + literal(prefix) + ")", attrCss(name, prefix, "^="),
          85, "icon-prefix", [], ["icon-variant"]);
        continue;
      }
      if (name === "id") {
        if (generic.test(value) || frameworkId.test(value)) continue;
        if (generated(value)) {
          const match = value.match(/^([A-Za-z][A-Za-z_-]{3,}[-_])(?:\d{3,}|[A-Za-z0-9]{6,})$/);
          if (match && !generic.test(match[1].slice(0, -1))) {
            add("starts-with(@id," + literal(match[1]) + ")", attrCss(name, match[1], "^="),
              65, "id-prefix", [], ["id-prefix"]);
          }
          continue;
        }
      } else if (name === "for" && (generated(value) || frameworkId.test(value))) continue;
      const quality = name === "role" || name === "type" ? "weak" : "semantic";
      add("@" + name + "=" + literal(value), attrCss(name, value), score, name, [], [], quality);
    }
    for (const name of ["href", "src"]) {
      const value = el.getAttribute(name);
      if (!useful(value) || !/^(?:https?:\/\/|\/|\.{1,2}\/)/i.test(value)) continue;
      let url;
      try { url = new URL(value, el.ownerDocument.baseURI); } catch { continue; }
      if (!/^https?:$/.test(url.protocol)) continue;
      if (Array.from(url.searchParams.keys()).some(key => /^(?:token|access_token|auth|signature|session|sessionid|nonce|expires|timestamp)$/i.test(key))) continue;
      const trackingOnly = url.search && !url.hash && Array.from(url.searchParams.keys()).every(
        key => /^(?:utm_.+|gclid|fbclid|msclkid|spm)$/i.test(key));
      if (trackingOnly) {
        const prefix = value.slice(0, value.indexOf("?"));
        add("substring-before(concat(@" + name + ",'?'),'?')=" + literal(prefix),
          "[" + name + "]", 84, name + "-route",
          [{kind: "url-route", name, value: prefix}], ["tracking-query"]);
      } else {
        add("@" + name + "=" + literal(value), attrCss(name, value), url.search || url.hash ? 81 : 91,
          name, [], url.search || url.hash ? ["business-url"] : []);
      }
    }
    for (const token of Array.from(el.classList || []).slice(0, 24)) {
      if (stableClass(token)) {
        add("contains(concat(' ',normalize-space(@class),' ')," + literal(" " + token + " ") + ")",
          "." + CSS.escape(token), 68, "class-token", [], ["class"]);
      } else {
        const match = token.match(/^([A-Za-z][A-Za-z_-]{3,})___[A-Za-z0-9_-]{4,}$/);
        if (match && stableClass(match[1])) {
          const prefix = match[1] + "___";
          add("contains(concat(' ',normalize-space(@class))," + literal(" " + prefix) + ")",
            "", 60, "class-prefix", [{kind: "class-prefix", value: prefix}], ["class-prefix"]);
        }
      }
    }
    const text = normalize(el.textContent);
    if (semanticText(text) && !el.matches("input,textarea,select,script,style") &&
        (!el.children.length || el.localName.includes("-") ||
         el.matches("a,button,label,p,h1,h2,h3,h4,h5,h6,[role='button'],[role='tab'],[role='link']"))) {
      add("normalize-space(.)=" + literal(text), "", 93, "text", [{kind: "text", value: text}], ["text"]);
    }
    if (el.labels) {
      for (const label of Array.from(el.labels).slice(0, 3)) {
        const value = normalize(label.textContent);
        if (!semanticText(value)) continue;
        add("(@id=//label[normalize-space(.)=" + literal(value) + "]/@for or ancestor::label[normalize-space(.)=" + literal(value) + "])",
          "", 100, "label", [{kind: "label", value}], ["text"]);
      }
    }
    if (ancestor && el.matches("section,article,form,fieldset,div,nav,aside,[role='dialog'],[role='region']")) {
      for (const heading of Array.from(el.querySelectorAll("h1,h2,h3,h4,h5,h6,legend")).slice(0, 3)) {
        const value = normalize(heading.textContent), css = heading.localName;
        if (!semanticText(value)) continue;
        add(".//" + css + "[normalize-space(.)=" + literal(value) + "]", "", 88,
          "heading", [{kind: "has-text", css, value}], ["text"]);
      }
      // A sibling business key can distinguish otherwise identical component
      // roots, without positional indices or predicates crossing ShadowRoots.
      const keys = [];
      for (const child of Array.from(el.querySelectorAll("*")).slice(0, 64)) {
        if (child.matches("script,style")) continue;
        for (const t of terms(child, false).filter(t => t.quality === "semantic" &&
            /^(?:data-.+|href|aria-label|text)$/.test(t.strategy)).slice(0, 2)) {
          keys.push({child, t});
        }
      }
      keys.sort((a, b) => b.t.score - a.t.score);
      for (const {child, t} of keys.slice(0, 6)) {
        add(".//" + tagPath(child) + "[" + t.xpath + "]", "", t.score - 18,
          "descendant:" + t.strategy, [{kind: "has-plan", css: t.css, filters: t.filters}], t.warnings);
      }
    }
    result.sort((a, b) => b.score - a.score || a.xpath.length - b.xpath.length);
    // Pairs disambiguate repeated names/labels without freezing layout or input values.
    const simple = result.filter(t => !t.filters.length && t.quality === "semantic").slice(0, 8);
    for (let i = 0; i < simple.length; i++) {
      for (let j = i + 1; j < simple.length; j++) {
        const a = simple[i], b = simple[j];
        result.push({xpath: a.xpath + " and " + b.xpath, css: a.css + b.css.slice(tag.length),
          score: (a.score + b.score) / 2 - 5, strategy: "attribute-pair",
          filters: [], warnings: a.warnings.concat(b.warnings), quality: "semantic"});
      }
    }
    const scoped = el.localName.includes("-") ||
      (el.getRootNode().host && el.getRootNode().querySelectorAll(tag).length === 1);
    result.push({xpath: "", css: tag, score: el.localName.includes("-") ? 90 : scoped ? 45 : 10,
      strategy: scoped ? "component-tag" : "tag-only", filters: [],
      warnings: scoped ? ["component-structure"] : [], quality: scoped ? "semantic" : "weak"});
    return result.sort((a, b) => b.score - a.score || a.xpath.length - b.xpath.length).slice(0, 52);
  }
  function accepts(el, filters) {
    return filters.every(f => {
      if (f.kind === "text") return normalize(el.textContent) === f.value;
      if (f.kind === "label") return Array.from(el.labels || []).some(l => normalize(l.textContent) === f.value);
      if (f.kind === "has-text") return Array.from(el.querySelectorAll(f.css)).some(n => normalize(n.textContent) === f.value);
      if (f.kind === "class-prefix") return Array.from(el.classList).some(c => c.startsWith(f.value));
      if (f.kind === "url-route") return (el.getAttribute(f.name) || "").split("?")[0] === f.value;
      if (f.kind === "has-plan") return Array.from(el.querySelectorAll(f.css)).some(n => accepts(n, f.filters));
      return false;
    });
  }
  function query(root, plan) {
    let roots = [root];
    for (const part of plan) {
      roots = Array.from(new Set(roots.flatMap(r => Array.from(r.querySelectorAll(part.css))
        .filter(n => accepts(n, part.filters)))));
      if (roots.length > 5000) throw new Error("Locator query budget exceeded");
    }
    return roots;
  }
  function locate(el, alternatives = false) {
    const root = el.getRootNode(), candidates = [], seen = new Set();
    let checked = 0;
    function add(nodes, selected, strategy, score) {
      const xpath = "//" + selected.map((t, i) => tagPath(nodes[i]) + (t.xpath ? "[" + t.xpath + "]" : "")).join("//");
      if (seen.has(xpath)) return;
      seen.add(xpath);
      candidates.push({
        xpath, css: selected.map(t => t.css).join(" "),
        query: selected.map(t => ({css: t.css, filters: t.filters})),
        strategy, quality: selected.some(t => t.quality === "weak") ? "weak" : "semantic",
        warnings: Array.from(new Set(selected.flatMap(t => t.warnings))),
        score: score - xpath.length * 0.015
      });
    }
    function verified(candidate) {
      if (++checked > 280) return false;
      try {
        const matches = query(root, candidate.query);
        if (matches.length !== 1 || matches[0] !== el) return false;
        if (root.nodeType === 9) {
          const result = el.ownerDocument.evaluate(candidate.xpath, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
          if (result.snapshotLength !== 1 || result.snapshotItem(0) !== el) return false;
        }
        candidate.score = Math.round(candidate.score * 10) / 10;
        candidate.unique_verified = true;
        candidate.requires_review = candidate.quality !== "semantic";
        return true;
      } catch { return false; }
    }
    const target = terms(el, false);
    for (const t of target) add([el], [t], t.strategy, t.score);
    let parent = el.parentElement;
    for (let depth = 1; parent && depth <= 8; parent = parent.parentElement, depth++) {
      if (parent.matches("html,body")) break;
      const anchors = terms(parent, true).filter(t => t.quality === "semantic").slice(0, 5);
      for (const a of anchors) {
        for (const t of target.filter(t => t.quality === "semantic").slice(0, 12)) {
          add([parent, el], [a, t], "ancestor:" + a.strategy + "/" + t.strategy,
            (a.score + t.score) / 2 - 17 - depth * 0.6);
        }
      }
    }
    candidates.sort((a, b) => b.score - a.score || a.xpath.length - b.xpath.length);
    const valid = [];
    for (const candidate of candidates) {
      if (verified(candidate)) {
        if (!alternatives) return candidate;
        if (candidate.quality === "semantic") valid.push(candidate);
        if (valid.length === 5) return valid;
      }
      if (checked >= 280) break;
    }
    if (alternatives) return valid;
    // Diagnostic only: positional paths never become ready-to-run Python.
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && parts.length < 64) {
      const siblings = Array.from(node.parentNode.children).filter(
        n => n.localName === node.localName && n.namespaceURI === node.namespaceURI);
      const index = siblings.indexOf(node) + 1;
      parts.unshift({xpath: tagPath(node) + "[" + index + "]",
        css: CSS.escape(node.localName) + ":nth-of-type(" + index + ")"});
      node = node.parentElement;
    }
    const fallback = {xpath: "/" + parts.map(p => p.xpath).join("/"),
      css: parts.map(p => p.css).join(" > "), strategy: "positional-fallback",
      quality: "structural", requires_review: true, unique_verified: true, warnings: ["structural"]};
    const matches = root.querySelectorAll(fallback.css);
    if (node || matches.length !== 1 || matches[0] !== el) throw new Error("Cannot generate a unique locator for the observed element");
    if (root.nodeType === 9) {
      const r = el.ownerDocument.evaluate(fallback.xpath, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
      if (r.snapshotLength !== 1 || r.snapshotItem(0) !== el) throw new Error("XPath identity check failed");
    }
    return fallback;
  }
  function path(el) {
    const step = Object.assign({kind: "element"}, locate(el));
    const root = el.getRootNode();
    if (!root.host) return [step];
    const hosts = path(root.host);
    hosts[hosts.length - 1].kind = "shadow";
    return hosts.concat(step);
  }
  function similar(elements) {
    const first = elements[0], root = first.getRootNode();
    const shared = terms(first, false).filter(t => {
      try {
        return t.strategy !== "label" && query(root, [{css: t.css, filters: t.filters}])
          .filter(n => elements.includes(n)).length === elements.length;
      } catch { return false; }
    });
    const candidates = [], seen = new Set();
    function add(anchor, a, t, depth) {
      const xpath = (anchor ? "//" + tagPath(anchor) + "[" + a.xpath + "]//" : "//") +
        tagPath(first) + (t.xpath ? "[" + t.xpath + "]" : "");
      if (seen.has(xpath)) return;
      seen.add(xpath);
      const plan = (anchor ? [{css: a.css, filters: a.filters}] : []).concat({css: t.css, filters: t.filters});
      try {
        const found = query(root, plan);
        if (!elements.every(e => found.includes(e)) || found.length < elements.length) return;
        if (root.nodeType === 9) {
          const r = first.ownerDocument.evaluate(xpath, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
          const nodes = Array.from({length: r.snapshotLength}, (_, i) => r.snapshotItem(i));
          if (nodes.length !== found.length || !elements.every(e => nodes.includes(e))) return;
        }
        const score = (anchor ? (a.score + t.score) / 2 : t.score) -
          Math.max(0, found.length - elements.length) * 2 - depth - xpath.length * 0.015;
        candidates.push({xpath, query: plan, count: found.length, score, strategy: anchor ? "similar:region/" + t.strategy : "similar:" + t.strategy});
      } catch { /* An unusable candidate never becomes an output path. */ }
    }
    for (const t of shared.filter(t => t.quality === "semantic").slice(0, 20)) add(null, null, t, 0);
    let ancestor = first.parentElement, depth = 0;
    while (ancestor && !elements.every(e => ancestor.contains(e))) ancestor = ancestor.parentElement;
    for (; ancestor && depth < 8; ancestor = ancestor.parentElement, depth++) {
      if (ancestor.matches("html,body")) break;
      for (const a of terms(ancestor, true).filter(t => t.quality === "semantic").slice(0, 5)) {
        for (const t of shared.slice(0, 8)) add(ancestor, a, t, depth);
      }
    }
    return candidates.sort((a, b) => b.score - a.score || a.xpath.length - b.xpath.length).slice(0, 5);
  }
  function shadowPaths(elements) {
    const chains = elements.map(el => {
      const nodes = [el];
      while (nodes[0].getRootNode().host && nodes.length < 33) nodes.unshift(nodes[0].getRootNode().host);
      return nodes;
    });
    const chain = chains[0];
    if (chain.length > 32 || chains.some(c => c.length !== chain.length)) return [];
    const ids = new WeakMap(); let serial = 0;
    const stateId = nodes => nodes.map(n => {
      if (!ids.has(n)) ids.set(n, ++serial);
      return ids.get(n);
    }).sort((a, b) => a - b).join(",");
    let beam = [{xpath: "", roots: [chain[0].getRootNode()], score: 0, warnings: [], steps: []}];
    for (let index = 0; index < chain.length; index++) {
      const node = chain[index], last = index === chain.length - 1;
      const sameRoot = elements.every(e => e.getRootNode() === elements[0].getRootNode());
      let options = last && elements.length > 1 && sameRoot ? similar(elements) :
        locate(node, true).concat(terms(node, false).filter(t => t.quality === "semantic").map(t => ({
          xpath: "//" + tagPath(node) + (t.xpath ? "[" + t.xpath + "]" : ""),
          query: [{css: t.css, filters: t.filters}], score: t.score, strategy: t.strategy, warnings: t.warnings
        })));
      options = options.filter((o, i, all) => o.query && all.findIndex(p => p.xpath === o.xpath) === i).slice(0, 24);
      const next = [];
      for (const prior of beam) {
        for (const option of options) {
          let found;
          try { found = Array.from(new Set(prior.roots.flatMap(r => query(r, option.query)))); } catch { continue; }
          if (!chains.every(c => found.includes(c[index])) || found.length > 5000) continue;
          if (last && !elements.every(e => found.includes(e))) continue;
          const roots = last ? [] : found.map(n => n.shadowRoot).filter(Boolean);
          if (!last && !roots.length) continue;
          const xpath = prior.xpath + (index ? "/xbotShadowRoot" : "") + option.xpath;
          next.push({xpath, roots, score: prior.score + option.score - Math.max(0, found.length - 1) * 3,
            state: stateId(found),
            count: found.length, strategy: "shadow-chain:" + option.strategy,
            warnings: [...prior.warnings, ...(option.warnings || [])],
            steps: prior.steps.concat({xpath: option.xpath, query: option.query, count: found.length})});
        }
      }
      const seen = new Set();
      const states = new Map();
      beam = next.sort((a, b) => b.score - a.score || a.xpath.length - b.xpath.length)
        .filter(p => {
          if (seen.has(p.xpath) || (states.get(p.state) || 0) >= 5) return false;
          seen.add(p.xpath); states.set(p.state, (states.get(p.state) || 0) + 1); return true;
        }).slice(0, 48);
      if (!beam.length) return [];
    }
    // Check identity using root-scoped DOM queries only, never evaluate the
    // composed ShadowBot expression in a browser or call a native validator.
    const bestCount = Math.min(...beam.map(c => c.count));
    return beam.filter(c => elements.length > 1 || c.count === bestCount)
      .sort((a, b) => b.score - a.score)
      .slice(0, 5).map(({roots, state, ...tail}) => Object.assign(tail, {
      root_scope_verified: true, dom_identity_verified: true, unique_verified: false,
      requires_review: elements.length === 1 && tail.count !== 1,
      syntax: "shadowbot", validation: "requires_shadowbot", runtime_verified: false,
      warnings: Array.from(new Set([...tail.warnings, "shadowbot-syntax-unverified"]))
    }));
  }
  function closedShadow(el) {
    for (let root = el && el.getRootNode(); root && root.host; root = root.host.getRootNode()) {
      if (root.mode === "closed") return true;
    }
    return false;
  }
  function closedResult() {
    const reason = "【不支持 shadow-root(closed)】影刀无法使用 closed ShadowRoot 内的元素，无法生成 XPath 或等价 Python；请由页面提供 open ShadowRoot 或外部可操作元素。";
    return {unsupported: "closed-shadow-root", error_code: "WEB_SHADOW_CLOSED",
      closed_shadow: true, shadow_mode: "closed", candidates: [], steps: [],
      reason, warnings: [reason], runtime_verified: false};
  }
  window.__xbotPythonXPaths = function (elements) {
    if (elements.some(closedShadow)) return closedResult();
    const first = elements[0];
    if (new Set(elements).size !== elements.length) return {candidates: [], reason: "请传入不同元素的 ref，不能重复选择同一个元素"};
    const type = el => JSON.stringify([el.namespaceURI, el.localName,
      el.localName === "input" ? el.type : "", el.getAttribute("role") || ""]);
    if (elements.some(el => type(el) !== type(first))) return {candidates: [], reason: "元素类型不一致，无法生成相似元素 XPath"};
    if (elements.some(el => el.getRootNode() !== first.getRootNode())) {
      const topology = el => {
        const tags = [];
        while (el) { tags.unshift(el.namespaceURI + ":" + el.localName); el = el.getRootNode().host; }
        return JSON.stringify(tags);
      };
      if (!first.getRootNode().host || elements.some(el => !el.getRootNode().host || topology(el) !== topology(first))) {
        return {candidates: [], reason: "元素不在兼容的文档或 ShadowRoot 组件链，无法生成相似元素 XPath"};
      }
    }
    const shadow = Boolean(first.getRootNode().host);
    const candidates = shadow ? shadowPaths(elements) : elements.length === 1 ? locate(first, true) : similar(elements);
    return {candidates, shadow, reason: candidates.length ? null : shadow ?
      "当前 ShadowRoot 链仍无法用语义属性、文字或组件作用域区分目标；未使用位置索引，请选择带业务标识的元素" :
      "没有找到可验证的语义 XPath；请补充稳定属性或选择同一区域的同类元素"};
  };
  window.__xbotPythonLocator = function (el) {
    if (closedShadow(el)) return closedResult();
    const steps = path(el);
    const sensitive = el.matches('input[type="password"],input[autocomplete*="cc-"],input[autocomplete="one-time-code"]') ||
      /password|passwd|secret|token|api.?key|credit.?card/i.test([el.id, el.name, el.getAttribute("aria-label")].join(" "));
    return {
      steps, sensitive, origin: location.origin, url: location.href,
      tag: el.localName, input_type: el.type || null,
      multiple: Boolean(el.multiple),
      options: el.options ? Array.from(el.options).map((o, index) => ({value: o.value, index})) : []
    };
  };
})();
