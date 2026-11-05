"""只读核对已知格式的影刀元数据，输出局部静态证据，不操作客户端或运行业务。"""

import argparse
import ast
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import xml.etree.ElementTree as ET


CORE = ("package.json", "prototype.block.json", "__init__.py", "prototype.activity.xml")
STATES = {"active", "deprecated_compatible", "hidden_compatible", "unregistered", "internal"}
MISSING = {"missing": True}
HASH_PATTERN = re.compile(r"[0-9a-f]{64}")


class InputError(ValueError):
    """输入格式或证据不可用时停止当前检查，不猜测配置。"""


def digest(value):
    """只存结构化指纹，不把默认值或源码正文写入报告。"""
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InputError("JSON存在重复键：" + key)
        result[key] = value
    return result


def load_json(content):
    return json.loads(content, object_pairs_hook=unique_object)


def require_list(value, label):
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise InputError(label + "必须为对象列表")
    return value


def unique_index(items, key, label):
    require_list(items, label)
    names = [item.get(key) for item in items]
    if any(not isinstance(name, str) or not name for name in names):
        raise InputError(label + "缺少有效标识：" + key)
    duplicates = [name for name, count in Counter(names).items() if count > 1]
    if duplicates:
        raise InputError(label + "存在重复标识：" + ", ".join(duplicates))
    return dict(zip(names, items))


def safe_path(root, relative):
    """拒绝路径穿越和指向应用外部的链接，不跟随任意清单路径。"""
    if not isinstance(relative, str) or "\\" in relative or ":" in relative:
        raise InputError("不是受支持的应用相对路径")
    parts = PurePosixPath(relative)
    if parts.is_absolute() or not parts.parts or any(part in (".", "..") for part in relative.split("/")):
        raise InputError("应用路径越界或为空：" + relative)
    path = (root / relative).resolve()
    if not path.is_relative_to(root):
        raise InputError("应用路径指向范围外：" + relative)
    return path


def watched_files(root):
    """保护四层文件、根目录Python、流程参数文件和工作区，不扫描虚拟环境。"""
    paths = set(CORE)
    paths.update(path.relative_to(root).as_posix() for path in root.glob("*.py"))
    paths.update(path.relative_to(root).as_posix() for path in (root / ".dev").glob("*.flow.json"))
    if (root / ".dev" / "workspace.state.json").exists():
        paths.add(".dev/workspace.state.json")
    return sorted(paths)


def flatten_hashes(value, pointer=""):
    """记录叶节点及容器类型，避免对象和数组拥有相同路径时漏报。"""
    result = {}
    if isinstance(value, dict) and value:
        result[pointer] = digest({"container_type": "object"})
        for key, child in value.items():
            escaped = key.replace("~", "~0").replace("/", "~1")
            result.update(flatten_hashes(child, pointer + "/" + escaped))
    elif isinstance(value, list) and value:
        result[pointer] = digest({"container_type": "array"})
        for index, child in enumerate(value):
            result.update(flatten_hashes(child, pointer + "/" + str(index)))
    else:
        result[pointer] = digest(value)
    return result


def local_tag(element):
    return element.tag.rsplit("}", 1)[-1]


def python_functions(content):
    """只解析注册文件；普通导入是否可用不在此处执行验证。"""
    tree = ast.parse(content, feature_version=(3, 7))
    compile(tree, "__init__.py", "exec", dont_inherit=True)
    nodes = [node for node in tree.body if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and not node.name.startswith("_")]
    if len({node.name for node in nodes}) != len(nodes):
        raise InputError("Python公开注册存在同名顶层函数")
    return {node.name: node for node in nodes}


def xml_functions(content, namespace):
    if "<!DOCTYPE" in content or "<!ENTITY" in content:
        raise InputError("XML包含当前适配器不接受的实体声明")
    tree = ET.fromstring(content)
    modules = [node for node in tree.iter() if local_tag(node) == "item" and node.get("kind") == "module" and node.get("name") == namespace]
    if len(modules) != 1:
        raise InputError("XML目标命名空间缺失或重复")
    members = [node for node in modules[0] if local_tag(node) == "item" and node.get("kind") == "function"]
    names = [node.get("name") for node in members]
    if len(set(names)) != len(names) or any(not isinstance(name, str) or not name.startswith(namespace + ".") for name in names):
        raise InputError("XML公开成员重复或跨命名空间")
    return {name: node for name, node in zip(names, members)}


def signature_hash(arguments):
    return digest(ast.dump(arguments, include_attributes=False))


def xml_signature(node):
    methods = [child for child in node if local_tag(child) == "item.method"]
    if len(methods) != 1 or not methods[0].text:
        raise InputError("XML成员缺少唯一方法签名：" + str(node.get("name")))
    declaration = ast.parse("def " + methods[0].text.strip() + ":\n    pass\n", feature_version=(3, 7)).body[0]
    if declaration.name != node.get("name").rsplit(".", 1)[-1]:
        raise InputError("XML方法签名与成员绑定不一致")
    return declaration


def parameter_contract(parameters):
    return [{key: item.get(key, MISSING) for key in ("name", "direction", "type", "kind", "value")} for item in parameters]


def block_contract(block):
    result = {key: block.get(key, MISSING) for key in ("name", "function")}
    result.update({direction: [{key: item.get(key, MISSING) for key in ("name", "type", "default", "required")} for item in block.get(direction, [])] for direction in ("inputs", "outputs")})
    return result


def dotted_name(node):
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return dotted_name(node.value) + "." + node.attr
    return ""


def capture(app_dir, expect_app):
    """读取前后对比文件集合和内容；不将此时间窗口冒充事务锁。"""
    root = Path(app_dir).resolve(strict=True)
    if not root.is_dir():
        raise InputError("应用路径不是目录")
    names = watched_files(root)
    payloads = {name: safe_path(root, name).read_bytes() for name in names}
    package = load_json(payloads["package.json"].decode("utf-8-sig"))
    if not isinstance(package, dict) or package.get("uuid") != expect_app:
        raise InputError("应用身份与明确指定的app_id不一致")
    code = package.get("activity_code")
    if not isinstance(code, str) or not code:
        raise InputError("不支持缺少activity_code的应用格式")
    namespace = "xbot_extensions." + code
    prototype = load_json(payloads["prototype.block.json"].decode("utf-8-sig"))
    if not isinstance(prototype, dict):
        raise InputError("指令原型不是对象")
    functions = python_functions(payloads["__init__.py"].decode("utf-8-sig"))
    members = xml_functions(payloads["prototype.activity.xml"].decode("utf-8-sig"), namespace)
    projections = {name: flatten_hashes(value) for name, value in (("package.json", package), ("prototype.block.json", prototype))}
    contracts = {}
    for name, content in payloads.items():
        if name.endswith(".flow.json"):
            flow = load_json(content.decode("utf-8-sig"))
            if not isinstance(flow, dict):
                raise InputError("流程元数据不是对象：" + name)
            parameters = require_list(flow.get("parameters"), name + ".parameters")
            projections[name] = flatten_hashes({"parameters": parameters})
            contracts[name] = digest(parameter_contract(parameters))
    blocks = require_list(prototype.get("blocks"), "blocks")
    local_blocks = [block for block in blocks if isinstance(block.get("name"), str) and block["name"].startswith(namespace + ".")]
    bindings = [block.get("function") for block in local_blocks]
    if any(not isinstance(binding, str) or not binding.startswith(namespace + ".") for binding in bindings) or len(set(bindings)) != len(bindings):
        raise InputError("目标原型存在重复或缺失函数绑定")
    snapshot = {
        "schema_version": 1,
        "app_id": expect_app,
        "namespace": namespace,
        "root": str(root),
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "files": {name: hashlib.sha256(content).hexdigest() for name, content in payloads.items()},
        "json_fields": projections,
        "parameter_contracts": contracts,
        "block_contracts": {block["function"]: digest(block_contract(block)) for block in local_blocks},
        "python_signatures": {namespace + "." + name: signature_hash(node.args) for name, node in functions.items()},
        "xml_signatures": {name: signature_hash(xml_signature(node).args) for name, node in members.items()},
        "consistent_read": True,
    }
    assert_unchanged(root, snapshot)
    return snapshot, payloads, package, prototype, functions, members


def assert_unchanged(root, snapshot):
    if watched_files(root) != sorted(snapshot["files"]):
        raise InputError("读取期间文件集合发生变化，停止使用这份快照")
    for name, expected in snapshot["files"].items():
        if hashlib.sha256(safe_path(root, name).read_bytes()).hexdigest() != expected:
            raise InputError("读取期间文件内容发生变化：" + name)


def validate_policy(policy, root):
    if not isinstance(policy, dict) or policy.get("schema_version") != 1:
        raise InputError("不支持的策略清单版本")
    if not isinstance(policy.get("app_id"), str) or not isinstance(policy.get("namespace"), str):
        raise InputError("策略必须明确指定app_id和namespace")
    entries = unique_index(policy.get("flows"), "filename", "策略流程")
    bindings = set()
    block_names = set()
    for filename, entry in entries.items():
        if "/" in filename or "\\" in filename or filename in (".", ".."):
            raise InputError("策略filename必须是流程文件标识")
        safe_path(root, filename + ".py")
        if entry.get("state") not in STATES:
            raise InputError("未知入口策略：" + filename)
        for field in ("block", "function"):
            if not isinstance(entry.get(field), str) or not entry[field].startswith(policy["namespace"] + "."):
                raise InputError("策略缺少完整本地绑定：" + filename + "." + field)
        if entry["function"] in bindings or entry["block"] in block_names:
            raise InputError("策略中存在重复函数或原型绑定")
        bindings.add(entry["function"])
        block_names.add(entry["block"])
        if "group" not in entry or (entry["group"] is not None and not isinstance(entry["group"], str)):
            raise InputError("策略必须明确指定group，可为null")
    allowed_files = policy.get("allowed_file_changes", [])
    allowed_json = policy.get("allowed_json_changes", {})
    if not isinstance(allowed_files, list) or not isinstance(allowed_json, dict):
        raise InputError("修改许可必须为文件列表及JSON指针映射")
    for name in allowed_files + list(allowed_json):
        safe_path(root, name)
    if any(name in ("package.json", "prototype.block.json") for name in allowed_files):
        raise InputError("package/block修改必须使用JSON指针许可，不能许可整个文件")
    for pointers in allowed_json.values():
        if not isinstance(pointers, list) or any(not isinstance(pointer, str) or not pointer.startswith("/") or "*" in pointer for pointer in pointers):
            raise InputError("JSON许可只接受非空JSON指针，不支持通配符或整个根对象")
    return entries


def validate_snapshot(snapshot, current):
    """核对证据结构和范围；不能由此证明快照来源真实或未经人工改写。"""
    if not isinstance(snapshot, dict) or snapshot.get("schema_version") != 1 or snapshot.get("consistent_read") is not True:
        raise InputError("基线不是受支持的一致读取快照")
    if (snapshot.get("app_id"), snapshot.get("namespace"), snapshot.get("root")) != (current["app_id"], current["namespace"], current["root"]):
        raise InputError("基线应用身份、命名空间或路径不符")
    for field in ("files", "json_fields", "parameter_contracts", "block_contracts", "python_signatures", "xml_signatures"):
        if not isinstance(snapshot.get(field), dict):
            raise InputError("基线缺少字段：" + field)
    if not set(CORE).issubset(snapshot["files"]) or not {"package.json", "prototype.block.json"}.issubset(snapshot["json_fields"]):
        raise InputError("基线缺少四层文件或JSON字段证据")
    for name, value in snapshot["files"].items():
        safe_path(Path(current["root"]), name)
        if not isinstance(value, str) or not HASH_PATTERN.fullmatch(value):
            raise InputError("基线文件指纹无效：" + name)
    for name, fields in snapshot["json_fields"].items():
        if name not in snapshot["files"] or not isinstance(fields, dict) or not fields:
            raise InputError("基线JSON字段证据不完整：" + name)
        for pointer, value in fields.items():
            if not isinstance(pointer, str) or (pointer and not pointer.startswith("/")) or not isinstance(value, str) or not HASH_PATTERN.fullmatch(value):
                raise InputError("基线JSON指纹无效：" + name)
    for field in ("parameter_contracts", "block_contracts", "python_signatures", "xml_signatures"):
        for name, value in snapshot[field].items():
            if not isinstance(value, str) or not HASH_PATTERN.fullmatch(value):
                raise InputError("基线契约指纹无效：" + field)
            if field == "parameter_contracts" and (name not in snapshot["files"] or name not in snapshot["json_fields"]):
                raise InputError("基线流程参数缺少文件或字段证据")
    flow_files = {name for name in snapshot["files"] if name.startswith(".dev/") and name.endswith(".flow.json")}
    if flow_files != set(snapshot["parameter_contracts"]):
        raise InputError("基线流程参数证据覆盖不完整")


def compare_snapshot(current, previous, policy, report, stable=False):
    """计划前基线按精确许可比较；稳定基线不使用任何修改许可。"""
    validate_snapshot(previous, current)
    allowed_files = set() if stable else set(policy.get("allowed_file_changes", []))
    allowed_json = {} if stable else policy.get("allowed_json_changes", {})
    for name in sorted(current["files"].keys() | previous["files"].keys()):
        if current["files"].get(name) == previous["files"].get(name):
            continue
        report["changed_files"].append({"path": name, "comparison": "stable" if stable else "before"})
        if stable:
            report["issues"].append({"code": "STABILITY_DRIFT", "severity": "error", "location": name, "message": "稳定快照之后文件发生变化，需复核并重新验收"})
            continue
        before = previous["json_fields"].get(name)
        after = current["json_fields"].get(name)
        if before is not None and after is not None:
            for pointer in sorted(before.keys() | after.keys()):
                if before.get(pointer) != after.get(pointer) and not any(pointer == allowed or pointer.startswith(allowed + "/") for allowed in allowed_json.get(name, [])):
                    report["issues"].append({"code": "UNAPPROVED_JSON_CHANGE", "severity": "error", "location": name + "#" + pointer, "message": "JSON字段变化不在本次精确许可中"})
            if name in ("package.json", "prototype.block.json"):
                continue
        if name not in allowed_files:
            report["issues"].append({"code": "PROTECTED_FILE_CHANGED", "severity": "error", "location": name, "message": "保护文件内容、新增或移除未获本次清单许可"})


def default_value(value, kind):
    """只比较文本或表达式AST，不执行表达式，也不猜测未知编码。"""
    if value == MISSING:
        return MISSING
    if kind == "Text" and isinstance(value, str):
        return ["text", value]
    if kind == "Expression" and isinstance(value, str):
        return ["expression", ast.dump(ast.parse(value, mode="eval", feature_version=(3, 7)), include_attributes=False)]
    raise InputError("默认值种类或表达式无法静态比较")


def prototype_default(item):
    value = item.get("default", MISSING)
    if value == MISSING:
        return MISSING
    if not isinstance(value, str) or ":" not in value:
        raise InputError("原型默认值编码无法静态比较")
    prefix, content = value.split(":", 1)
    if prefix == "10":
        return default_value(content, "Text")
    if prefix in ("11", "13"):
        return default_value(content, "Expression")
    raise InputError("原型默认值编码不在已验证范围")


def audit(app_dir, policy, baseline=None, stable_snapshot=None, phase="offline"):
    root = Path(app_dir).resolve(strict=True)
    entries = validate_policy(policy, root)
    snapshot, payloads, package, prototype, functions, members = capture(root, policy["app_id"])
    if snapshot["namespace"] != policy["namespace"]:
        raise InputError("策略命名空间与当前应用不一致")
    if baseline is not None:
        validate_snapshot(baseline, snapshot)
    if stable_snapshot is not None:
        validate_snapshot(stable_snapshot, snapshot)
    report = {
        "schema_version": 1,
        "app_id": policy["app_id"],
        "root": str(root),
        "requested_phase": phase,
        "acceptance_complete": False,
        "issues": [],
        "changed_files": [],
        "checks": {},
        "manual_evidence": {name: "not_verified" for name in ("editor_closed", "real_save", "full_reopen", "old_consumers", "hidden_visibility", "retirement_authorization", "dependency_imports", "registration_unrelated_content", "ui_rendering", "runtime_business", "published_consumers")},
    }

    def finding(code, location, message, severity="error"):
        report["issues"].append({"code": code, "severity": severity, "location": location, "message": message})

    flows = unique_index(package.get("flows"), "filename", "应用流程")
    unique_index(package["flows"], "name", "应用流程显示名称")
    groups = unique_index(package.get("flow_groups"), "name", "流程分组")
    blocks = unique_index(require_list(prototype.get("blocks"), "blocks"), "name", "指令原型")
    local_blocks = {name: block for name, block in blocks.items() if name.startswith(policy["namespace"] + ".")}
    for name, block in blocks.items():
        function = block.get("function")
        if name not in local_blocks and isinstance(function, str) and function.startswith(policy["namespace"] + "."):
            finding("CROSS_NAMESPACE_BINDING", name, "其他命名空间的原型绑定了本地函数，需先消除歧义")
    registration_tree = ast.parse(payloads["__init__.py"].decode("utf-8-sig"), feature_version=(3, 7))
    for node in registration_tree.body:
        if isinstance(node, (ast.Assign, ast.AnnAssign, ast.ClassDef)) or (isinstance(node, ast.ImportFrom) and not (node.module is None and all(alias.name == "package" for alias in node.names))):
            finding("REGISTRATION_FORMAT_UNVERIFIED", "__init__.py", "存在当前适配器未覆盖的别名、类或导入式导出，需复核公开集合", "warning")
            break
    if set(flows) != set(entries):
        finding("POLICY_COVERAGE", "flows", "策略必须覆盖应用全部流程，不允许靠遗漏名单自动注销")
    if set(local_blocks) != {entry["block"] for entry in entries.values()}:
        finding("BLOCK_SET", "prototype.block.json", "目标原型精确集合与策略不一致")
    expected_registered = {entry["function"] for entry in entries.values() if entry["state"] in ("active", "deprecated_compatible", "hidden_compatible")}
    python_registered = {policy["namespace"] + "." + name for name in functions}
    if python_registered != expected_registered:
        finding("PYTHON_SET", "__init__.py", "Python公开函数精确集合与注册策略不一致；导入式导出需另行适配")
    if set(members) != expected_registered:
        finding("XML_SET", "prototype.activity.xml", "XML公开成员精确集合与注册策略不一致")
    report["checks"]["collections"] = {"expected_registered": sorted(expected_registered), "python_registered": sorted(python_registered), "xml_registered": sorted(members), "visible_blocks": sorted(name for name, block in local_blocks.items() if block.get("hidden") is False)}
    for filename, entry in entries.items():
        flow = flows.get(filename)
        block = blocks.get(entry["block"])
        if flow is None or block is None:
            continue
        state = entry["state"]
        registered = entry["function"] in expected_registered
        if filename + ".py" not in payloads:
            finding("FLOW_SOURCE_UNAVAILABLE", filename, "缺少当前适配器要求的根目录Python流程源码")
        if state == "unregistered":
            finding("RETIREMENT_EVIDENCE_REQUIRED", filename, "注销授权与消费者迁移/退役证据需要单独核验", "warning")
        if baseline is not None and state == "internal" and entry["function"] in baseline.get("python_signatures", {}):
            finding("INTERNAL_WAS_PUBLIC", filename, "原公开入口不能改标内部模块绕过注销策略")
        if state == "hidden_compatible":
            finding("HIDDEN_COMPAT_UNVERIFIED", filename, "当前适配器不认证独立隐藏能力；必须保留注册并按版本另行核验", "warning")
            if flow.get("opened") is not True:
                finding("COMPATIBILITY_REGISTRATION_LOST", filename, "仅隐藏兼容不能通过关闭注册开关实现")
        elif type(flow.get("opened")) is not bool or flow["opened"] != registered:
            finding("OPENED_STATE", filename, "opened与注册策略不符")
        if state != "hidden_compatible" and (type(block.get("hidden")) is not bool or block["hidden"] != (not registered)):
            finding("HIDDEN_STATE", entry["block"], "hidden与当前已验证展示策略不符")
        if block.get("function") != entry["function"] or entry["function"] != policy["namespace"] + "." + filename:
            finding("FUNCTION_BINDING", filename, "真实函数绑定与明确filename不一致，当前适配器不猜测别名")
        if flow.get("groupName") != entry["group"] or (entry["group"] is not None and entry["group"] not in groups):
            finding("GROUP_REFERENCE", filename, "分组不符或引用尚未创建的组")
        metadata_name = ".dev/" + filename + ".flow.json"
        if metadata_name not in payloads:
            finding("PARAMETERS_UNAVAILABLE", filename, "缺少流程In/Out文件，参数检查未完成", "warning")
            continue
        parameters = load_json(payloads[metadata_name].decode("utf-8-sig"))["parameters"]
        unique_index(parameters, "name", filename + ".parameters")
        if any(item.get("direction") not in ("In", "Out") for item in parameters):
            finding("PARAMETER_DIRECTION", filename, "存在当前适配器不支持的参数方向")
        directions = {}
        for direction, field in (("In", "inputs"), ("Out", "outputs")):
            declared = [item for item in parameters if item.get("direction") == direction]
            expected = unique_index(declared, "name", filename + "." + direction)
            actual = unique_index(block.get(field), "name", entry["block"] + "." + field)
            directions[direction] = list(expected)
            if set(expected) != set(actual):
                finding("PARAMETER_SET", filename + "." + direction, "流程与原型参数名称或方向不一致；面板顺序不强制等于调用顺序")
            for name in expected.keys() & actual.keys():
                source, target = expected[name], actual[name]
                if not isinstance(source.get("type"), str) or not source["type"] or source.get("type") != target.get("type"):
                    finding("PARAMETER_TYPE", filename + "." + name, "流程与原型参数类型不一致")
                if direction == "In":
                    try:
                        if default_value(source.get("value", MISSING), source.get("kind")) != prototype_default(target):
                            finding("DEFAULT_MISMATCH", filename + "." + name, "流程与原型默认值不一致；报告不包含其敏感内容")
                    except (InputError, SyntaxError, ValueError, TypeError):
                        finding("DEFAULT_UNVERIFIED", filename + "." + name, "默认值不可安全静态比较，需按原表达式人工核对", "warning")
                if registered and (not isinstance(target.get("label"), str) or not target["label"].strip()):
                    finding("UI_TEXT_UNVERIFIED", filename + "." + name, "参数label缺失，中文展示需复核", "warning")
        comment = block.get("comment")
        if isinstance(comment, str):
            for placeholder in re.findall(r"%([^%\r\n]+)%", comment):
                if placeholder not in directions["In"] + directions["Out"]:
                    finding("ACTION_PLACEHOLDER", entry["block"], "动作描述引用了未声明的参数：" + placeholder)
        if registered:
            function = functions.get(filename)
            member = members.get(entry["function"])
            if function is not None:
                positional = function.args.posonlyargs + function.args.args
                if isinstance(function, ast.AsyncFunctionDef) or [arg.arg for arg in positional] != directions["In"] or function.args.vararg or function.args.kwarg or function.args.kwonlyargs:
                    finding("PYTHON_SIGNATURE", filename, "Python公开签名与流程输入不一致")
                output_nodes = [node.value for node in function.body if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "outputs" for target in node.targets)]
                if len(output_nodes) == 1 and isinstance(output_nodes[0], (ast.List, ast.Tuple)) and all(isinstance(item, ast.Constant) and isinstance(item.value, str) for item in output_nodes[0].elts):
                    if [item.value for item in output_nodes[0].elts] != directions["Out"]:
                        finding("PYTHON_OUTPUTS", filename, "Python注册的输出声明与原流程不一致")
                else:
                    finding("PYTHON_OUTPUTS_UNVERIFIED", filename, "无法静态确定注册输出列表，需人工核对", "warning")
                input_nodes = [node.value for node in function.body if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "inputs" for target in node.targets)]
                if len(input_nodes) == 1 and isinstance(input_nodes[0], ast.Dict) and all(isinstance(key, ast.Constant) and isinstance(key.value, str) and isinstance(value, ast.Name) for key, value in zip(input_nodes[0].keys, input_nodes[0].values)):
                    pairs = [(key.value, value.id) for key, value in zip(input_nodes[0].keys, input_nodes[0].values)]
                    if pairs != [(name, name) for name in directions["In"]]:
                        finding("PYTHON_INPUTS", filename, "Python注册输入映射与流程输入不一致")
                else:
                    finding("PYTHON_INPUTS_UNVERIFIED", filename, "无法静态确定注册输入映射，需人工核对", "warning")
                calls = [node for node in ast.walk(function) if isinstance(node, ast.Call)]
                runs = [call for call in calls if dotted_name(call.func) == "xbot_visual.process.run"]
                targets = [keyword.value.value for call in runs for keyword in call.keywords if keyword.arg == "process" and isinstance(keyword.value, ast.Constant) and isinstance(keyword.value.value, str)]
                if len(runs) != 1 or len(targets) != 1:
                    finding("PYTHON_TARGET_UNVERIFIED", filename, "未发现可直接核对的process绑定，需适配当前注册形式", "warning")
                elif any(target != entry["function"] for target in targets):
                    finding("PYTHON_TARGET", filename, "注册包装函数调用了其他流程绑定")
                for call in runs:
                    keywords = {keyword.arg: keyword.value for keyword in call.keywords}
                    if any(not isinstance(keywords.get(name), ast.Name) or keywords[name].id != name for name in ("inputs", "outputs")):
                        finding("PYTHON_FORWARDING", filename, "run未传入已声明的inputs/outputs变量")
                if member is not None and signature_hash(function.args) != signature_hash(xml_signature(member).args):
                    finding("XML_SIGNATURE", filename, "Python与XML公开签名不一致")
        if baseline is not None and state in ("deprecated_compatible", "hidden_compatible"):
            for key, identity in (("parameter_contracts", metadata_name), ("block_contracts", entry["function"]), ("python_signatures", entry["function"]), ("xml_signatures", entry["function"])):
                if baseline.get(key, {}).get(identity) != snapshot[key].get(identity):
                    finding("LEGACY_CONTRACT_CHANGED", filename, "兼容入口的原契约指纹变化或缺少旧证据：" + key)
    if baseline is None:
        finding("BASELINE_NOT_PROVIDED", "snapshot", "未提供修改前基线，不能验证保护文件及原契约未变化", "warning")
    else:
        compare_snapshot(snapshot, baseline, policy, report)
    if stable_snapshot is not None:
        compare_snapshot(snapshot, stable_snapshot, policy, report, stable=True)
    elif phase == "after_reopen":
        finding("STABLE_SNAPSHOT_NOT_PROVIDED", "snapshot", "重开阶段缺少真实保存后的稳定快照，不能比较持久化漂移", "warning")
    report["checks"]["stable_snapshot"] = "compared" if stable_snapshot is not None else "not_provided"
    report["checks"]["read_window"] = "no_change_observed"
    report["scope"] = ["已知package/block/flow.json格式及顶层函数式Python注册、XML函数成员", "保护根目录*.py、.dev/*.flow.json、工作区状态及四层文件；不递归保护依赖、资源或虚拟环境", "JSON未知字段按基线及精确许可比较；允许整文件改动的Python/XML无关内容仍须人工复核", "仅比较流程/原型的输入默认值、静态签名和已识别包装结构，不执行表达式、导入依赖、验证SDK、控制流或业务行为", "基线来源、阶段标签及文件稳定不能证明设计器真实关闭、保存、重开、兼容运行或发布"]
    assert_unchanged(root, snapshot)
    errors = any(item["severity"] == "error" for item in report["issues"])
    report["status"] = "issues_found" if errors else "needs_review" if report["issues"] else "no_findings_in_checked_rules"
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("snapshot", "check"))
    parser.add_argument("--app-dir", required=True, type=Path)
    parser.add_argument("--expect-app", help="snapshot必须明确指定应用uuid")
    parser.add_argument("--policy", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--stable-snapshot", type=Path)
    parser.add_argument("--phase", choices=("offline", "after_save", "after_reopen"), default="offline")
    args = parser.parse_args()
    try:
        if args.operation == "snapshot":
            if not args.expect_app:
                raise InputError("snapshot必须提供--expect-app")
            result = capture(args.app_dir, args.expect_app)[0]
            code = 0
        else:
            if args.policy is None:
                raise InputError("check必须提供完整策略--policy")
            policy = load_json(args.policy.read_text(encoding="utf-8-sig"))
            baseline = load_json(args.baseline.read_text(encoding="utf-8-sig")) if args.baseline else None
            stable = load_json(args.stable_snapshot.read_text(encoding="utf-8-sig")) if args.stable_snapshot else None
            result = audit(args.app_dir, policy, baseline, stable, args.phase)
            code = 1 if result["status"] == "issues_found" else 2 if result["status"] == "needs_review" else 0
    except (OSError, ValueError, TypeError, SyntaxError, KeyError, AttributeError, ET.ParseError) as exc:
        result = {"status": "issues_found", "acceptance_complete": False, "issues": [{"code": "INPUT_OR_SNAPSHOT_UNAVAILABLE", "severity": "error", "message": str(exc)}]}
        code = 1
    print(json.dumps(result, ensure_ascii=False, indent=2))
    raise SystemExit(code)


if __name__ == "__main__":
    main()
