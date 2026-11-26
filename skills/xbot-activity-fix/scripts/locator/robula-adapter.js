async function resolveRobulaLocator(activePage, refid, library) {
  function describe(target, library) {
    if (!target.isConnected) throw new Error("STALE_REF: node detached");
    const RobulaPlus = new Function(library + "\nreturn RobulaPlus;")();
    const contexts = [];
    let current = target;
    let path = "";
    let shadowDepth = 0;

    function scopeFor(node, root) {
      if (root.nodeType === 9) return { document: root, expected: node };
      if (!root.host || root.mode !== "open") throw new Error("UNSUPPORTED_ROOT");
      const indexes = [];
      for (let element = node; element !== root; element = element.parentNode) {
        indexes.unshift(Array.prototype.indexOf.call(element.parentNode.childNodes, element));
      }
      const document = node.ownerDocument.implementation.createHTMLDocument("");
      for (const child of root.childNodes) document.body.appendChild(document.importNode(child, true));
      let expected = document.body;
      for (const index of indexes) expected = expected.childNodes[index];
      return { document, expected };
    }

    for (;;) {
      const root = current.getRootNode();
      const scope = scopeFor(current, root);
      const nodes = scope.document.body.querySelectorAll("*");
      if (nodes.length > 20000 || [scope.document.body, ...nodes].some(node => node.attributes.length > 12)) {
        throw new Error("ROBULA_SCOPE_LIMIT: evaluation fixture exceeds preflight bound");
      }
      const started = Date.now();
      let evaluations = 0;
      // Only the document facade enforces a budget; upstream transformations stay unchanged.
      const facade = {
        body: scope.document.body,
        evaluate(...args) {
          if (++evaluations > 2500 || Date.now() - started > 1500) {
            throw new Error("ROBULA_BUDGET: XPath evaluation bound exceeded");
          }
          if (args[1] === facade) args[1] = scope.document;
          return scope.document.evaluate(...args);
        },
      };
      const segment = new RobulaPlus().getRobustXPath(scope.expected, facade);
      const result = scope.document.evaluate(segment, scope.document, null, 7, null);
      if (result.snapshotLength !== 1 || result.snapshotItem(0) !== scope.expected) {
        throw new Error("IDENTITY_MISMATCH: upstream output does not uniquely identify target");
      }
      contexts.unshift({ path: segment, evaluations, shadow_scope: Boolean(root.host) });
      path = segment + (path ? "/xbotShadowRoot" + path : "");
      if (!root.host) break;
      if (++shadowDepth > 8) throw new Error("CONTEXT_TOO_DEEP");
      current = root.host;
    }
    return { path, shadow_depth: shadowDepth, identity_verified: true, scopes: contexts };
  }

  const handle = await activePage.locator("aria-ref=" + refid).elementHandle({ timeout: 3000 });
  if (!handle) throw new Error("STALE_REF: take a new snapshot");
  const handles = [handle];
  try {
    const describeSource = describe.toString();
    const evaluate = element => element.evaluate(
      (target, { source, library }) => new Function("return (" + source + ");")()(target, library),
      { source: describeSource, library },
    );
    const target = await evaluate(handle);
    const contexts = [target];
    let frame = await handle.ownerFrame();
    const targetUrl = frame.url();
    while (frame.parentFrame()) {
      if (contexts.length > 16) throw new Error("CONTEXT_TOO_DEEP");
      const frameElement = await frame.frameElement();
      handles.push(frameElement);
      const context = await evaluate(frameElement);
      if (context.shadow_depth) throw new Error("UNSUPPORTED_SHADOW_IFRAME");
      contexts.unshift(context);
      frame = frame.parentFrame();
    }
    const frameDepth = contexts.length - 1;
    const shadowDepth = contexts.reduce((sum, context) => sum + context.shadow_depth, 0);
    return {
      algorithm: "cyluxx/robula-plus",
      revision: "5f33589dadba0f71075294611aa79801da123e40",
      profile: "upstream_defaults",
      refid,
      xpath: contexts.map(context => context.path),
      frame_depth: frameDepth,
      shadow_depth: shadowDepth,
      raw_xpath_compatible: frameDepth === 0 && shadowDepth === 0,
      requires_engine_selector: frameDepth > 0 || shadowDepth > 0,
      identity_verified: true,
      persisted: false,
      target_url: targetUrl,
      contexts,
    };
  } finally {
    for (const item of handles) await item.dispose();
  }
}
