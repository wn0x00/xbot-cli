(function () {
  const cases = [];
  function assert(value, message) { if (!value) throw new Error(message); }
  function nodes(root, step) {
    if (root.nodeType === 9) {
      const r = root.evaluate(step.xpath, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
      return Array.from({length: r.snapshotLength}, (_, i) => r.snapshotItem(i));
    }
    const norm = v => String(v || "").replace(/[ \t\r\n]+/g, " ").replace(/^ | $/g, "");
    let result = [root];
    for (const p of step.query || [{css: step.css, filters: []}]) {
      result = Array.from(new Set(result.flatMap(r => Array.from(r.querySelectorAll(p.css)).filter(el =>
        p.filters.every(f => {
          if (f.kind === "text") return norm(el.textContent) === f.value;
          if (f.kind === "label") return Array.from(el.labels || []).some(l => norm(l.textContent) === f.value);
          if (f.kind === "has-text") return Array.from(el.querySelectorAll(f.css)).some(n => norm(n.textContent) === f.value);
          if (f.kind === "class-prefix") return Array.from(el.classList).some(c => c.startsWith(f.value));
          if (f.kind === "url-route") return (el.getAttribute(f.name) || "").split("?")[0] === f.value;
          if (f.kind === "has-plan") return Array.from(el.querySelectorAll(f.css)).some(n => nodesForFilter(n, f.filters));
          return false;
        })))));
    }
    return result;
  }
  function nodesForFilter(el, filters) {
    return filters.every(f => {
      if (f.kind === "text") return el.textContent.trim().replace(/[ \t\r\n]+/g, " ") === f.value;
      if (f.kind === "has-plan") return Array.from(el.querySelectorAll(f.css)).some(n => nodesForFilter(n, f.filters));
      return false;
    });
  }
  function test(name, html, select, mutate, check, shadow = false) {
    const host = document.createElement("div");
    document.body.append(host);
    const root = shadow ? host.attachShadow({mode: "open"}) : host;
    root.innerHTML = html;
    try {
      const target = root.querySelector(select);
      const step = window.__xbotPythonLocator(target).steps.slice(-1)[0];
      if (check) check(step);
      const scope = target.getRootNode();
      assert(nodes(scope, step).length === 1 && nodes(scope, step)[0] === target, "Initial identity mismatch");
      if (mutate) {
        mutate(root, target);
        const found = nodes(scope, step);
        assert(found.length === 1 && found[0] === target, "Identity drift after mutation: " + step.xpath);
      }
      cases.push({name, strategy: step.strategy, quality: step.quality, xpath: step.xpath, passed: true});
    } catch (error) { throw new Error(name + ": " + error.message); }
    finally { host.remove(); }
  }
  const semantic = s => {
    assert(s.quality === "semantic", "Expected semantic locator");
    assert(s.unique_verified === true && !s.requires_review, "Missing identity validation");
    assert(!/^\/[^/]/.test(s.xpath), "Unexpected absolute XPath");
  };
  const wrapper = (root, target) => {
    const wrap = document.createElement("div");
    target.replaceWith(wrap); wrap.append(target);
    root.prepend(document.createElement("div"));
  };
  test("stable test attribute", '<button data-testid="save-profile">Save profile</button>', "button",
    wrapper, s => { semantic(s); assert(s.strategy === "data-testid", "Wrong priority"); });
  test("nested button text", '<button><span>Save document</span></button>', "button", wrapper,
    s => { semantic(s); assert(s.xpath === "//button[normalize-space(.)='Save document']", "Not a concise text XPath"); });
  test("link route", '<a href="/products/rpa">RPA product</a>', "a", wrapper, semantic);
  test("unnamed link route", '<a href="/docs/reference"></a>', "a", wrapper,
    s => { semantic(s); assert(s.strategy === "href", "Missing href"); });
  test("duplicate text semantic region",
    '<section data-testid="billing"><button>Save changes</button></section><section data-testid="shipping"><button>Save changes</button></section>',
    "section button", (r, t) => { r.append(r.firstElementChild); wrapper(r.querySelector("section:last-child"), t); },
    s => { semantic(s); assert(s.strategy.startsWith("ancestor:"), "Missing region anchor"); assert(s.xpath.includes("billing"), "Wrong region"); });
  test("duplicate text heading region",
    '<section><h2>Invoice preferences</h2><button>Save preferences</button></section><section><h2>Delivery preferences</h2><button>Save preferences</button></section>',
    "section button", (r, t) => { r.append(r.firstElementChild); wrapper(r.lastElementChild, t); },
    s => { semantic(s); assert(s.xpath.includes("Invoice preferences"), "Missing heading anchor"); });
  test("combined attributes",
    '<input name="recipient" aria-label="Primary recipient"><input name="recipient" aria-label="Other recipient"><input name="other" aria-label="Primary recipient">',
    "input", wrapper, s => { semantic(s); assert(s.strategy === "attribute-pair", "Expected attribute combination"); });
  for (const id of ["react-:r1:", "radix-123456", "el-id-5588-19", "input-1730023456789",
    "ddbc4ff2-a11a-4b99-9e23-91af3c1d0091", "query-a1b2c3d4", ":r123:"]) {
    test("dynamic id " + id, `<button id="${id}">Run workflow</button>`, "button",
      (r, t) => { t.id = "completely-replaced"; wrapper(r, t); },
      s => { semantic(s); assert(!s.xpath.includes(id), "Baked in dynamic id"); });
  }
  test("semantic class token state changes",
    '<a class="download-report active" href="javascript:void(0)"></a>', "a",
    (r, t) => { t.className = "selected download-report compact"; wrapper(r, t); },
    s => { semantic(s); assert(s.strategy === "class-token" && !s.xpath.includes("active"), "State class used"); });
  test("CSS module prefix",
    '<a class="reportDownload___aB8f2" href="javascript:void(0)"></a>', "a",
    (r, t) => { t.className = "expanded reportDownload___X9yZ12"; wrapper(r, t); },
    s => { semantic(s); assert(s.strategy === "class-prefix", "No semantic class prefix"); });
  test("generated class avoided", '<button class="css-a8b9321">Approve request</button>', "button",
    (r, t) => { t.className = "css-b9c842a"; wrapper(r, t); },
    s => { semantic(s); assert(!s.xpath.includes("css-"), "Hashed class used"); });
  test("tracking parameters change",
    '<a href="/guide/workflow?utm_source=old&utm_campaign=test"></a>', "a",
    (r, t) => { t.setAttribute("href", "/guide/workflow?utm_source=new&utm_campaign=next"); wrapper(r, t); },
    s => { semantic(s); assert(s.strategy === "href-route" && !s.xpath.includes("old"), "Tracking query frozen"); });
  test("business query preserved",
    '<a href="/orders?id=123&utm_source=old"></a><a href="/orders?id=124&utm_source=old"></a>', "a",
    wrapper, s => { semantic(s); assert(s.xpath.includes("id=123"), "Business query dropped"); });
  test("hash route preserved", '<a href="/app#/orders/123"></a><a href="/app#/orders/124"></a>',
    "a", wrapper, s => { semantic(s); assert(s.xpath.includes("#/orders/123"), "Hash route dropped"); });
  test("session URL not persisted", '<a href="/download?token=short-lived">Download invoice</a>', "a",
    (r, t) => { t.setAttribute("href", "/download?token=replaced"); wrapper(r, t); },
    s => { semantic(s); assert(!s.xpath.includes("short-lived"), "Session token persisted"); });
  test("explicit label with dynamic id",
    '<label for=":r17:">Account email</label><input id=":r17:">', "input",
    (r, t) => { t.id = ":r88:"; r.querySelector("label").htmlFor = t.id; wrapper(r, t); },
    s => { semantic(s); assert(s.strategy === "label" && !s.xpath.includes(":r17:"), "Label contract not used"); });
  test("wrapped label", '<label>Newsletter opt in <input type="checkbox"></label>', "input",
    wrapper, s => { semantic(s); assert(s.strategy === "label", "Wrapped label not used"); });
  test("quotes and unicode", '<button>He said &quot;it&#39;s ready&quot; 中文</button>', "button",
    wrapper, s => { semantic(s); assert(s.xpath.includes("concat("), "Missing XPath quote escaping"); });
  test("XPath whitespace", '<button> \t Confirm \n  changes \r </button>', "button",
    (r, t) => { t.textContent = "\nConfirm  changes\t"; wrapper(r, t); }, semantic);
  test("nonbreaking spaces preserved", '<button>&nbsp;Confirm details&nbsp;</button>', "button", wrapper, semantic);
  test("namespace SVG", '<svg xmlns="http://www.w3.org/2000/svg"><path data-testid="workflow-icon"></path></svg>',
    "path", wrapper, s => { semantic(s); assert(s.xpath.includes("namespace-uri()"), "Missing SVG namespace"); });
  test("positional fallback is not reusable",
    '<div><span></span><span></span></div><div><span></span><span></span></div>', "div span", null,
    s => assert(s.requires_review && s.quality === "structural", "Positional fallback promoted to stable"));
  test("generic root is not a semantic identity",
    '<div id="root"><b></b><b></b></div>', "b", null,
    s => assert(s.requires_review && !s.xpath.includes("@id='root'"), "Generic root promoted"));
  for (const [name, html, select, mutation] of [
    ["text", '<button><span>Shadow approval</span></button>', "button", wrapper],
    ["region", '<section aria-label="Billing panel"><button>Shadow save</button></section><section aria-label="Shipping panel"><button>Shadow save</button></section>', "section button",
      (r, t) => wrapper(r.firstElementChild, t)],
    ["label", '<label for=":r9:">Shadow account email</label><input id=":r9:">', "input",
      (r, t) => { t.id = ":r12:"; r.querySelector("label").htmlFor = t.id; wrapper(r, t); }],
    ["class-prefix", '<button class="approveRequest___aB4c56"></button>', "button",
      (r, t) => { t.className = "approveRequest___bC5d67"; wrapper(r, t); }],
    ["heading", '<section><h2>Shadow invoice</h2><button>Shadow preferences</button></section><section><h2>Shadow delivery</h2><button>Shadow preferences</button></section>', "section button",
      (r, t) => wrapper(r.firstElementChild, t)]
  ]) test("open shadow " + name, html, select, mutation, semantic, true);
  // Large repeated regions still use an explicit record key, not a row index.
  test("dense repeated records", Array.from({length: 300}, (_, i) =>
    `<article data-record-id="order-${i}"><button>Inspect order</button></article>`).join(""),
    '[data-record-id="order-148"] button', (r, t) => { r.prepend(t.parentElement); wrapper(t.parentElement, t); },
    s => { semantic(s); assert(s.xpath.includes("order-148"), "Record key lost"); });
  window.__xbotLocatorTestResults = cases;
})();
