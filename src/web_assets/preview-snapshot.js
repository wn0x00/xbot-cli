(function () {
  const engine = window.__xbotPreviewIS;
  const text = engine.ariaSnapshot(document.body, {mode: "ai"});
  const snapshot = engine._lastAriaSnapshotForQuery;
  window.__xbotPreviewRefs = new Map(snapshot.elements);
  window.__xbotPreviewDocumentId = window.__xbotPreviewDocumentId || Date.now().toString(36) + "-" + Math.random().toString(36).slice(2);
  const items = [];
  const refs = new Map(Array.from(snapshot.elements, ([ref, el]) => [el, ref]));
  function visit(node) {
    if (typeof node === "string") return {role: "text", name: node};
    const states = {};
    for (const key of ["active", "checked", "disabled", "expanded", "selected", "pressed"]) {
      if (node[key] !== undefined) states[key] = node[key];
    }
    const packed = {role: node.role, name: node.name || "", ref: node.ref || null,
      level: node.level, children: [], interactive: false, states, props: node.props || {},
      cursor: node.box && node.box.cursor, url: (node.props || {}).url || null};
    if (node.ref) {
      const el = snapshot.elements.get(node.ref);
      packed.shadow_hosts = [];
      packed.shadow_modes = [];
      for (let root = el && el.getRootNode(); root && root.host; root = root.host.getRootNode()) {
        packed.shadow_hosts.unshift(refs.get(root.host) || root.host.localName);
        packed.shadow_modes.unshift(root.mode);
      }
      if (packed.shadow_modes.includes("closed")) {
        packed.xpath_unavailable = "closed-shadow-root";
        packed.warning = "影刀不支持 shadow-root(closed) 内的元素，无法生成 XPath";
      }
      const interactive = /^(button|link|textbox|searchbox|checkbox|radio|combobox|listbox|option|menuitem|menuitemcheckbox|menuitemradio|tab|switch|slider|spinbutton|treeitem)$/.test(node.role) ||
        packed.cursor === "pointer" || Boolean(el && el.isContentEditable);
      items.push({
        ref: node.ref, role: node.role, name: node.name || "", interactive,
        states, url: packed.url,
        shadow_hosts: packed.shadow_hosts, shadow_modes: packed.shadow_modes,
        xpath_unavailable: packed.xpath_unavailable, warning: packed.warning
      });
      packed.interactive = interactive;
      packed.states = items[items.length - 1].states;
    }
    packed.children = (node.children || []).map(visit).filter(Boolean);
    return packed;
  }
  const tree = visit(snapshot.root);
  return {text, elements: items, tree, document_id: window.__xbotPreviewDocumentId,
    url: location.href, ready_state: document.readyState};
})();
