"""Read-only collection resolution. Action and application edits belong to the caller."""

import hashlib
import json
import re
import time
from copy import deepcopy
from urllib.parse import urlsplit


class ResolutionError(Exception):
    def __init__(self, code, detail=None):
        Exception.__init__(self, code)
        self.code = code
        self.detail = detail


def _stop(code, detail=None):
    raise ResolutionError(code, detail)


def _hash(value):
    return hashlib.sha256(json.dumps(
        value, sort_keys=True, ensure_ascii=True).encode("utf-8")).hexdigest()


def _query(parent, locator):
    if locator.get("kind") == "xpath":
        value = locator["value"]
        if not isinstance(value, str) or "/xbotShadowRoot" in value:
            _stop("UNSUPPORTED_RAW_CONTEXT")
        return parent.find_all_by_xpath(value, timeout=0)
    if locator.get("kind") == "selector" and locator.get("verified") is True:
        return parent.find_all(locator["value"], timeout=0)
    _stop("UNVERIFIED_LOCATOR")


def attribute_locator(attribute, value, version, relative=True):
    """Deterministic XPath quoting for a confirmed DOM attribute/value."""
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.-]*", attribute) or not isinstance(value, str):
        _stop("ATTRIBUTE_LOCATOR_INVALID")
    if "'" not in value:
        literal = "'" + value + "'"
    elif '"' not in value:
        literal = '"' + value + '"'
    else:
        parts = value.split("'")
        literal = "concat(" + ', "\'", '.join("'" + part + "'" for part in parts) + ")"
    return {"kind": "xpath", "version": version,
            "value": (".//" if relative else "//") + "*[@" + attribute + "=" + literal + "]"}


def _relative(locator):
    if locator.get("kind") == "xpath":
        value = locator.get("value")
        if not isinstance(value, str) or not value.startswith("./"):
            _stop("UNSCOPED_RELATIVE_XPATH")
        # Check syntax outside XPath string literals; a quoted "|" is data.
        syntax = []
        quote = None
        for char in value:
            if quote:
                if char == quote:
                    quote = None
            elif char in ("'", '"'):
                quote = char
            else:
                syntax.append(char)
        tokens = "".join(syntax)
        if quote or ".." in tokens or "::" in tokens or "|" in tokens:
            _stop("UNSCOPED_RELATIVE_XPATH")


def _identity(element):
    # Read-only identity for one live DOM generation; never stored as a locator.
    return element.execute_javascript("""function(element,args){
      if(!element.isConnected)return null;
      const parts=[];let n=element;
      while(n&&n.nodeType===1){
        let i=1,p=n.previousElementSibling;
        while(p){i++;p=p.previousElementSibling;}
        parts.unshift(n.localName+':'+i);n=n.parentElement;
      }
      if(element.getRootNode().nodeType!==9) return null;
      return {url:element.ownerDocument.URL,
        generation:element.ownerDocument.defaultView.performance.timeOrigin,
        path:parts.join('/')};
    }""")


def _unique(groups, scope):
    if any(group["locator"].get("kind") != "xpath" for group in groups):
        _stop("ATOMIC_SELECTOR_PROFILE_UNSUPPORTED")
    result = {}
    measured = []
    group_paths = []
    for group in groups:
        paths = []
        for node in group["nodes"]:
            identity = _identity(node)
            if not identity:
                _stop("UNSUPPORTED_DOM_IDENTITY")
            paths.append(identity)
            measured.append((node, identity))
        # One native query cannot contain the same actual node twice.
        if len({_hash(identity) for identity in paths}) != len(paths):
            _stop("DOM_IDENTITY_COLLISION")
        group_paths.append(paths)
    # A path measured at different moments must not erase another live member.
    for node, identity in measured:
        if _identity(node) != identity:
            _stop("DOM_CHANGED_DURING_RESOLUTION")
        result[_hash(identity)] = node
    # Evaluate every version together within one DOM task. Repeated reorderings
    # between per-handle reads cannot hide an extra node by reusing its position.
    atomic_paths = scope.execute_javascript("""function(element,args){
      const locators=JSON.parse(args),doc=element.ownerDocument;
      function identify(node){
        if(!node.isConnected||node.getRootNode().nodeType!==9)return null;
        const parts=[];let n=node;
        while(n&&n.nodeType===1){
          let i=1,p=n.previousElementSibling;
          while(p){i++;p=p.previousElementSibling;}
          parts.unshift(n.localName+':'+i);n=n.parentElement;
        }
        return {url:doc.URL,generation:doc.defaultView.performance.timeOrigin,
          path:parts.join('/')};
      }
      return locators.map(xpath=>{
        const value=doc.evaluate(xpath,element,null,7,null),out=[];
        for(let i=0;i<value.snapshotLength;i++)out.push(identify(value.snapshotItem(i)));
        return out;
      });
    }""", argument=json.dumps([group["locator"]["value"] for group in groups]))
    if atomic_paths != group_paths:
        _stop("DOM_CHANGED_DURING_RESOLUTION")
    return result


def _field(member, field):
    groups = []
    counts = []
    for locator in field["locators"]:
        _relative(locator)
        found = _query(member, locator)
        counts.append({"version": locator["version"], "count": len(found)})
        groups.append({"locator": locator, "nodes": found})
    unique = _unique(groups, member)
    if len(unique) != 1:
        _stop("CHILD_AMBIGUOUS" if unique else "CHILD_MISSING",
              {"field": field["name"], "counts": counts})
    node = next(iter(unique.values()))
    value = (node.get_attribute(field["attribute"]) if field["read"] == "attribute"
             else node.get_text().strip())
    if not isinstance(value, str) or not value.strip():
        _stop("CHILD_VALUE_EMPTY", field["name"])
    if field.get("allowed_values") is not None and value not in field["allowed_values"]:
        _stop("CHILD_VALUE_INVALID", {"field": field["name"], "value": value})
    return value, counts


# One DOM task reads the complete semantic tuple. Parent/index caches avoid
# repeatedly walking thousands of preceding siblings on dense tables.
_BUSINESS_AUDIT = """function(element,args){
  const c=JSON.parse(args),doc=element.ownerDocument;
  const indices=new Map(),indexedParents=new Set(),identities=new Map();
  function fail(status,detail){throw {auditStatus:status,detail};}
  function identify(node){
    if(!node||!node.isConnected||node.getRootNode().nodeType!==9)return null;
    if(identities.has(node))return identities.get(node);
    let parent=node.parentElement;
    if(parent&&!indexedParents.has(parent)){
      let i=0;for(const child of parent.children)indices.set(child,++i);
      indexedParents.add(parent);
    }
    const up=parent?identify(parent).path+'/':'';
    const value={url:doc.URL,generation:doc.defaultView.performance.timeOrigin,
      path:up+node.localName+':'+(parent?indices.get(node):1)};
    identities.set(node,value);return value;
  }
  function query(scope,locator){
    const v=doc.evaluate(locator.value,scope,null,7,null),out=[];
    for(let i=0;i<v.snapshotLength;i++){
      const node=v.snapshotItem(i);
      if(node.nodeType!==1)fail('NON_ELEMENT_CANDIDATE');
      out.push(node);
    }
    return out;
  }
  function visible(node){
    if(!node.isConnected||!node.getClientRects().length)return false;
    for(let n=node;n;n=n.parentElement){
      const s=doc.defaultView.getComputedStyle(n);
      if(n.hidden||s.display==='none'||s.visibility==='hidden'||s.visibility==='collapse'||
         Number(s.opacity)===0)return false;
    }
    return true;
  }
  function disabled(node){
    for(let n=node;n;n=n.parentElement){
      if(n.disabled||n.inert||n.getAttribute('aria-disabled')==='true')return true;
    }
    return false;
  }
  function attrs(node,expected,status){
    for(const [key,value] of Object.entries(expected))
      if(node.getAttribute(key)!==value)fail(status,key);
  }
  try{
    const url=new URL(doc.URL);
    if(url.origin!==c.page.origin||url.pathname!==c.page.path)fail('WRONG_PAGE');
    if(url.search.slice(1)!==(c.page.query||'')||url.hash.slice(1)!==(c.page.fragment||''))
      fail('WRONG_PAGE_PARAMETERS');
    const roots=query(doc,c.page.locator);
    if(roots.length!==1)fail('PAGE_IDENTITY_AMBIGUOUS');
    const root=roots[0];
    attrs(root,c.page.attributes,'PAGE_STATE_OR_IDENTITY');
    const regions=query(doc,c.region.locator).filter(visible);
    if(regions.length!==1)fail(regions.length?'REGION_AMBIGUOUS':'REGION_MISSING');
    if(regions[0]!==element||!root.contains(element))fail('REGION_OUTSIDE_PAGE_GUARD');
    attrs(element,c.region.attributes,'REGION_IDENTITY');
    const groups=c.variants.map(locator=>query(element,locator));
    const union=[...new Set(groups.flat())],members=[],seen=new Set();
    for(const node of union){
      if(node===element||!element.contains(node))fail('MEMBER_OUTSIDE_REGION');
    }
    const membership=new Set(union);
    for(const node of union){
      for(let n=node.parentElement;n&&n!==element;n=n.parentElement)
        if(membership.has(n))fail('NESTED_MEMBERS_UNPROVEN');
    }
    for(const node of union){
      if(!visible(node))continue;
      attrs(node,c.member.attributes,'NEGATIVE_MEMBER_INCLUDED');
      const key=node.getAttribute(c.member.key_attribute);
      if(!key)fail('MEMBER_KEY_MISSING');
      if(seen.has(key))fail('DUPLICATE_BUSINESS_KEY',key);
      seen.add(key);
      if(c.member.require_enabled&&disabled(node))fail('MEMBER_DISABLED',key);
      const fields={},fieldIdentities={};
      for(const field of c.member.fields){
        const children=[...new Set(field.locators.flatMap(locator=>query(node,locator)))];
        if(children.length!==1)fail(children.length?'CHILD_AMBIGUOUS':'CHILD_MISSING',
          {key,field:field.name,count:children.length});
        const child=children[0];
        if(child===node||!node.contains(child))fail('CHILD_OUTSIDE_MEMBER',field.name);
        if(!visible(child))fail('CHILD_NOT_VISIBLE',{key,field:field.name});
        const value=field.read==='attribute'?child.getAttribute(field.attribute):child.innerText.trim();
        if(typeof value!=='string'||!value.trim())fail('CHILD_VALUE_EMPTY',field.name);
        if(field.allowed_values&&!field.allowed_values.includes(value))
          fail('CHILD_VALUE_INVALID',{key,field:field.name,value});
        fields[field.name]=value;
        fieldIdentities[field.name]=identify(child);
        if(field.name===c.member.action_field&&disabled(child))fail('ACTION_DISABLED',key);
      }
      const expected=c.member.expected_by_key[key];
      if(!expected)fail('MEMBERSHIP_MISMATCH',{unexpected:[key]});
      for(const name of Object.keys(expected))
        if(fields[name]!==expected[name])fail('BUSINESS_VALUE_MISMATCH',
          {key,field:name,expected:expected[name],actual:fields[name]});
      members.push({key,identity:identify(node),fields,fieldIdentities});
    }
    const missing=c.coverage.keys.filter(key=>!seen.has(key));
    if(missing.length||seen.size!==c.coverage.keys.length)fail('MEMBERSHIP_MISMATCH',{missing});
    return {status:'BUSINESS_AUDIT_PASSED',region:identify(element),
      groups:groups.map(nodes=>nodes.map(identify)),members,
      excluded_hidden:union.length-members.length};
  }catch(error){
    if(error.auditStatus)return {status:error.auditStatus,detail:error.detail};
    throw error;
  }
}"""


def _business_contract(contract):
    member = contract.get("member", {})
    expected = member.get("expected_by_key", {})
    keys = contract.get("coverage", {}).get("keys", [])
    fields = member.get("fields", [])
    names = [field.get("name") for field in fields]
    identity_fields = member.get("identity_fields", [])
    if (not contract.get("coverage", {}).get("source") or not isinstance(expected, dict) or
            set(expected) != set(keys) or not identity_fields or
            len(set(names)) != len(names) or not set(identity_fields).issubset(set(names))):
        _stop("BUSINESS_BASELINE_UNPROVEN")
    for values in expected.values():
        if (not isinstance(values, dict) or set(values) != set(names) or
                any(not isinstance(value, str) or not value.strip() for value in values.values())):
            _stop("BUSINESS_BASELINE_UNPROVEN")
    if (not contract["page"].get("attributes") or not contract["region"].get("attributes") or
            not contract["variants"]):
        _stop("BUSINESS_SCOPE_UNPROVEN")
    locators = [contract["page"]["locator"], contract["region"]["locator"]]
    for variant in contract["variants"]:
        _relative(variant)
        locators.append(variant)
    for field in fields:
        if field.get("read") not in ("text", "attribute") or not field.get("locators"):
            _stop("BUSINESS_FIELD_CONTRACT_INVALID")
        if field["read"] == "attribute" and not field.get("attribute"):
            _stop("BUSINESS_FIELD_CONTRACT_INVALID")
        for locator in field["locators"]:
            _relative(locator)
            locators.append(locator)
    if any(locator.get("kind") != "xpath" or "/xbotShadowRoot" in locator.get("value", "")
           for locator in locators):
        _stop("ATOMIC_SELECTOR_PROFILE_UNSUPPORTED")


def _resolve_business(browser, contract, budget):
    _business_contract(contract)
    regions = [node for node in _query(browser, contract["region"]["locator"]) if node.is_displayed()]
    if len(regions) != 1:
        _stop("REGION_AMBIGUOUS" if regions else "REGION_MISSING")
    region = regions[0]
    argument = json.dumps(contract)
    before = region.execute_javascript(_BUSINESS_AUDIT, argument=argument)
    if before["status"] != "BUSINESS_AUDIT_PASSED":
        _stop(before["status"], before.get("detail"))
    budget()
    handles = {}
    observed = []
    for variant in contract["variants"]:
        ids = []
        for node in _query(region, variant):
            identity = _identity(node)
            if not identity:
                _stop("UNSUPPORTED_DOM_IDENTITY")
            ids.append(identity)
            handles[_hash(identity)] = node
            budget()
        observed.append(ids)
    if observed != before["groups"]:
        _stop("DOM_CHANGED_DURING_RESOLUTION")
    after = region.execute_javascript(_BUSINESS_AUDIT, argument=argument)
    if after != before:
        _stop("BUSINESS_DOM_CHANGED_DURING_RESOLUTION", after.get("status"))
    budget()
    members = {item["key"]: handles[_hash(item["identity"])] for item in after["members"]}
    values = {item["key"]: item["fields"] for item in after["members"]}
    keys = list(contract["coverage"]["keys"])
    selection = contract.get("selection", {"mode": "all"})
    if selection["mode"] == "business_key":
        if selection.get("key") not in members:
            _stop("SELECTED_KEY_MISSING")
        keys = [selection["key"]]
    elif selection["mode"] != "all":
        _stop("SELECTION_UNPROVEN")
    return {"status": "COLLECTION_RESOLVED", "keys": keys, "members": members, "values": values,
            "_business_audit": after,
            "fingerprint": _hash({"contract": contract, "audit": after, "keys": keys}),
            "contract_sha256": _hash(contract),
            "evidence": {"validation_profile": "business", "atomic_semantic_audits": 2,
                         "variants": [{"version": variant["version"], "count": len(ids)}
                                      for variant, ids in zip(contract["variants"], observed)],
                         "excluded_hidden": after["excluded_hidden"],
                         "baseline_source": contract["coverage"]["source"]},
            "application_modified": False, "actions": 0}


def resolve_collection(browser, contract, budget_seconds=12):
    """Resolve a complete collection with old/new paths; never click or patch."""
    started = time.monotonic()
    evidence = {"variants": [], "fields": {}, "excluded_hidden": 0}

    def budget():
        if time.monotonic() - started > budget_seconds:
            _stop("RESOLUTION_BUDGET_EXCEEDED")

    try:
        if contract.get("schema_version") != 1:
            _stop("CONTRACT_INVALID")
        if contract.get("classification") not in ("collection", "collection_then_single"):
            _stop("USAGE_REQUIRES_REVIEW")
        if contract.get("uncertainties"):
            _stop("SEMANTIC_CONTRACT_UNCERTAIN")
        if contract.get("merge_policy") != "same_region_union":
            _stop("MERGE_POLICY_UNPROVEN")
        if contract.get("coverage", {}).get("kind") != "exact_keys":
            _stop("COMPLETENESS_UNPROVEN")
        expected = contract["coverage"]["keys"]
        if len(set(expected)) != len(expected) or (not expected and not contract.get("allow_empty")):
            _stop("COVERAGE_INVALID")
        if contract.get("selection", {}).get("mode") == "position":
            _stop("POSITIONAL_SELECTION_UNPROVEN")
        page = urlsplit(browser.get_url())
        guard = contract["page"]
        if (page.scheme + "://" + page.netloc, page.path) != (guard["origin"], guard["path"]):
            _stop("WRONG_PAGE")
        if page.query != guard.get("query", "") or page.fragment != guard.get("fragment", ""):
            _stop("WRONG_PAGE_PARAMETERS")
        if contract.get("validation_profile") == "business":
            return _resolve_business(browser, contract, budget)
        if contract.get("validation_profile") not in (None, "structural"):
            _stop("VALIDATION_PROFILE_UNSUPPORTED")
        roots = _query(browser, guard["locator"])
        if len(roots) != 1:
            _stop("PAGE_IDENTITY_AMBIGUOUS")
        root = roots[0]
        root_id = _identity(root)
        if not root_id:
            _stop("UNSUPPORTED_DOM_IDENTITY")
        for key, value in guard["attributes"].items():
            if root.get_attribute(key) != value:
                _stop("PAGE_STATE_OR_IDENTITY", key)
        budget()
        regions = _query(browser, contract["region"]["locator"])
        regions = [item for item in regions if item.is_displayed()]
        if len(regions) != 1:
            _stop("REGION_AMBIGUOUS" if regions else "REGION_MISSING")
        region = regions[0]
        for key, value in contract["region"]["attributes"].items():
            if region.get_attribute(key) != value:
                _stop("REGION_IDENTITY", key)
        region_id = _identity(region)
        if not region_id:
            _stop("UNSUPPORTED_DOM_IDENTITY")
        if (region_id["url"] != root_id["url"] or
                region_id["generation"] != root_id["generation"]):
            _stop("UNSUPPORTED_CONTEXT_IDENTITY")
        if not (region_id["path"] == root_id["path"] or
                region_id["path"].startswith(root_id["path"] + "/")):
            _stop("REGION_OUTSIDE_PAGE_GUARD")
        groups = []
        for variant in contract["variants"]:
            _relative(variant)
            found = _query(region, variant)
            evidence["variants"].append({"version": variant["version"], "count": len(found)})
            groups.append({"locator": variant, "nodes": found})
            budget()
        members = _unique(groups, region)
        member_ids = [_identity(item) for item in members.values()]
        for identity in member_ids:
            if (not identity or identity["url"] != region_id["url"] or
                    identity["generation"] != region_id["generation"] or
                    not identity["path"].startswith(region_id["path"] + "/")):
                _stop("MEMBER_OUTSIDE_REGION")
            if any(other and other != identity and
                   identity["path"].startswith(other["path"] + "/")
                   for other in member_ids):
                _stop("NESTED_MEMBERS_UNPROVEN")
        by_key = {}
        identities = {}
        values = {}
        for dom_id, member in members.items():
            budget()
            if not member.is_displayed():
                evidence["excluded_hidden"] += 1
                continue
            for key, value in contract["member"]["attributes"].items():
                if member.get_attribute(key) != value:
                    _stop("NEGATIVE_MEMBER_INCLUDED", key)
            business_key = member.get_attribute(contract["member"]["key_attribute"])
            if not business_key:
                _stop("MEMBER_KEY_MISSING")
            if business_key in by_key:
                _stop("DUPLICATE_BUSINESS_KEY", business_key)
            if contract["member"].get("require_enabled") and not member.is_enabled():
                _stop("MEMBER_DISABLED", business_key)
            fields = {}
            evidence["fields"][business_key] = {}
            for field in contract["member"]["fields"]:
                budget()
                fields[field["name"]], counts = _field(member, field)
                evidence["fields"][business_key][field["name"]] = counts
            by_key[business_key] = member
            identities[business_key] = dom_id
            values[business_key] = fields
        if set(by_key) != set(expected):
            _stop("MEMBERSHIP_MISMATCH", {
                "missing": sorted(set(expected) - set(by_key)),
                "unexpected": sorted(set(by_key) - set(expected)),
            })
        selection = contract.get("selection", {"mode": "all"})
        if selection["mode"] == "business_key":
            selected = selection.get("key")
            if selected not in by_key:
                _stop("SELECTED_KEY_MISSING")
            keys = [selected]
        elif selection["mode"] == "all":
            # The contract explicitly supplies semantic order, not query order.
            keys = list(expected)
        else:
            _stop("SELECTION_UNPROVEN")
        budget()
        fingerprint = _hash({"contract": contract, "region": region_id,
                             "members": identities, "values": values, "keys": keys})
        return {"status": "COLLECTION_RESOLVED", "keys": keys, "values": values,
                "fingerprint": fingerprint, "contract_sha256": _hash(contract),
                "evidence": evidence, "members": by_key,
                "application_modified": False, "actions": 0}
    except ResolutionError as error:
        return {"status": error.code, "detail": error.detail, "evidence": evidence,
                "application_modified": False, "actions": 0}


def revalidate_collection(browser, contract, previous, budget_seconds=12):
    """Requery every dependency immediately before the caller's authorized action."""
    if contract.get("validation_profile") == "business":
        # Check old native handles where the SDK preserves them. Some engines
        # re-resolve a handle by locator, so this is not a stable node-id proof.
        for node in previous.get("members", {}).values():
            if not _identity(node):
                return {"status": "STALE_RESOLUTION", "application_modified": False, "actions": 0}
    current = resolve_collection(browser, contract, budget_seconds)
    if current["status"] != "COLLECTION_RESOLVED":
        return current
    if previous.get("status") != "COLLECTION_RESOLVED" or (
            previous.get("fingerprint") != current["fingerprint"]):
        return {"status": "STALE_RESOLUTION", "application_modified": False, "actions": 0}
    return current


def validate_business_action(browser, contract, previous, business_key, budget_seconds=12):
    """Read-only preflight for one authorized action after caller prepares viewport."""
    if (contract.get("validation_profile") != "business" or
            business_key not in previous.get("keys", [])):
        return {"status": "BUSINESS_ACTION_UNPROVEN", "application_modified": False, "actions": 0}
    action = contract.get("member", {}).get("action", {})
    if (action.get("kind") != "click" or
            (action.get("tag") not in ("button", "a", "input") and
             action.get("role") not in ("button", "link"))):
        return {"status": "BUSINESS_ACTION_CONTRACT_UNPROVEN", "application_modified": False, "actions": 0}
    if action.get("prior_outcome") not in ("not_attempted", "confirmed_not_applied"):
        return {"status": "ACTION_OUTCOME_UNPROVEN", "application_modified": False, "actions": 0}
    current = revalidate_collection(browser, contract, previous, budget_seconds)
    if current["status"] != "COLLECTION_RESOLVED":
        return current
    name = contract["member"].get("action_field")
    fields = [field for field in contract["member"]["fields"] if field["name"] == name]
    if len(fields) != 1:
        return {"status": "BUSINESS_ACTION_UNPROVEN", "application_modified": False, "actions": 0}
    member = current["members"][business_key]
    field = fields[0]
    targets = _unique([{"locator": locator, "nodes": _query(member, locator)}
                       for locator in field["locators"]], member)
    if len(targets) != 1:
        return {"status": "CHILD_AMBIGUOUS", "application_modified": False, "actions": 0}
    target = next(iter(targets.values()))
    status = target.execute_javascript("""function(element,args){
      const spec=JSON.parse(args),c=spec.contract,doc=element.ownerDocument;
      const scopes=doc.evaluate(c.region.locator.value,doc,null,7,null);
      let region=null;
      for(let i=0;i<scopes.snapshotLength;i++){
        const node=scopes.snapshotItem(i);
        if(node.contains(element)){
          if(region)return 'ACTION_REGION_AMBIGUOUS';
          region=node;
        }
      }
      if(!region)return 'ACTION_OUTSIDE_REGION';
      const audit=BUSINESS_AUDIT_FUNCTION(region,JSON.stringify(c));
      if(audit.status!=='BUSINESS_AUDIT_PASSED')return audit.status;
      function stable(value){
        if(Array.isArray(value))return value.map(stable);
        if(value&&typeof value==='object')return Object.fromEntries(
          Object.keys(value).sort().map(key=>[key,stable(value[key])]));
        return value;
      }
      if(JSON.stringify(stable(audit))!==JSON.stringify(stable(spec.audit)))return 'STALE_ACTION_TARGET';
      const owners=new Set();
      for(const variant of c.variants){
        const rows=doc.evaluate(variant.value,region,null,7,null);
        for(let i=0;i<rows.snapshotLength;i++){
          const row=rows.snapshotItem(i);
          if(row.getAttribute(c.member.key_attribute)===spec.key&&row.contains(element))owners.add(row);
        }
      }
      if(owners.size!==1)return 'ACTION_OWNER_AMBIGUOUS';
      const owner=[...owners][0],field=c.member.fields.find(item=>item.name===c.member.action_field);
      const targets=new Set();
      for(const locator of field.locators){
        const nodes=doc.evaluate(locator.value,owner,null,7,null);
        for(let i=0;i<nodes.snapshotLength;i++)targets.add(nodes.snapshotItem(i));
      }
      if(targets.size!==1||!targets.has(element))return 'ACTION_IDENTITY_CHANGED';
      if(!element.isConnected)return 'STALE_ACTION_TARGET';
      const action=c.member.action;
      if(action.tag&&element.localName!==action.tag)return 'ACTION_KIND_MISMATCH';
      if(action.role&&element.getAttribute('role')!==action.role)return 'ACTION_KIND_MISMATCH';
      for(const [name,value] of Object.entries(action.attributes||{}))
        if(element.getAttribute(name)!==value)return 'ACTION_KIND_MISMATCH';
      for(let n=element;n;n=n.parentElement){
        const s=getComputedStyle(n);
        if(n.disabled||n.inert||n.getAttribute('aria-disabled')==='true')return 'ACTION_DISABLED';
        if(n.hidden||s.display==='none'||s.visibility!=='visible'||Number(s.opacity)===0)
          return 'ACTION_NOT_VISIBLE';
        if(s.pointerEvents==='none')return 'ACTION_POINTER_BLOCKED';
      }
      const r=element.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
      if(!r.width||!r.height||x<0||y<0||x>=innerWidth||y>=innerHeight)
        return 'ACTION_VIEWPORT_UNPROVEN';
      const hit=document.elementFromPoint(x,y);
      return hit&&(hit===element||element.contains(hit))?'BUSINESS_ACTION_READY':'ACTION_OCCLUDED';
    }""".replace("BUSINESS_AUDIT_FUNCTION", "(" + _BUSINESS_AUDIT + ")"),
        argument=json.dumps({"contract": contract, "audit": current["_business_audit"], "key": business_key}))
    return {"status": status, "target": target if status == "BUSINESS_ACTION_READY" else None,
            "key": business_key, "fingerprint": current["fingerprint"],
            "application_modified": False, "actions": 0}


def public_result(result):
    return {key: value for key, value in result.items()
            if key not in ("members", "target", "_business_audit")}


def screen_collection_candidates(browser, baseline, candidates, budget_seconds=30):
    """Validate proposals against an unchanged baseline, never rank by resemblance."""
    if baseline.get("validation_profile") != "business" or not candidates:
        return {"status": "BUSINESS_BASELINE_UNPROVEN", "actions": 0, "application_modified": False}
    started = time.monotonic()
    evidence = []
    accepted = {}
    validated = []
    for candidate in candidates:
        remaining = budget_seconds - (time.monotonic() - started)
        if remaining <= 0:
            return {"status": "CANDIDATE_BUDGET_EXCEEDED", "candidates": evidence,
                    "actions": 0, "application_modified": False}
        # Candidate proposals may change location, never identity or completeness.
        contract = deepcopy(baseline)
        contract["region"]["locator"] = deepcopy(candidate["region_locator"])
        result = resolve_collection(browser, contract, remaining)
        evidence.append({"name": candidate["name"], "status": result["status"],
                         "detail": result.get("detail")})
        if result["status"] == "COLLECTION_RESOLVED":
            audit = result["_business_audit"]
            identity = _hash({"region": audit["region"], "members": audit["members"]})
            accepted.setdefault(identity, []).append(candidate["name"])
            validated.append((contract, result))
    if len(accepted) == 1:
        remaining = budget_seconds - (time.monotonic() - started)
        if remaining <= 0:
            return {"status": "CANDIDATE_BUDGET_EXCEEDED", "candidates": evidence,
                    "actions": 0, "application_modified": False}
        contract, previous = validated[0]
        current = revalidate_collection(browser, contract, previous, remaining)
        if current["status"] != "COLLECTION_RESOLVED":
            return {"status": "CANDIDATE_STALE", "detail": current["status"], "candidates": evidence,
                    "actions": 0, "application_modified": False}
    status = ("CANDIDATE_VALIDATED" if len(accepted) == 1 else
              "CANDIDATES_AMBIGUOUS" if accepted else "NO_VALID_CANDIDATE")
    return {"status": status, "equivalent_candidates": list(accepted.values()),
            "candidates": evidence, "actions": 0, "application_modified": False}


def main(args):
    """Reusable CodeFlow: In page_url, contract_json; Out result_json."""
    import xbot.web
    from xbot import print
    if args is None:
        args = {}
    contract = json.loads(args["contract_json"])
    browser = xbot.web.create(args["page_url"], mode="edge", load_timeout=15)
    try:
        result = resolve_collection(browser, contract)
        args["result_json"] = json.dumps(public_result(result), ensure_ascii=True)
        print(args["result_json"])
    finally:
        browser.close()
