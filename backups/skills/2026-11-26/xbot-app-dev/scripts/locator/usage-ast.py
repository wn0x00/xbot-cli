"""Read Python syntax without importing or executing the workflow."""

import ast
import json
import sys


def inspect_usage(source, locator, selected=None):
    tree = ast.parse(source)
    parents = {
        child: parent
        for parent in ast.walk(tree)
        for child in ast.iter_child_nodes(parent)
    }
    assignments = {}
    writes = {}
    parameters = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Name) and isinstance(node.ctx, (ast.Store, ast.Del)):
            writes[node.id] = writes.get(node.id, 0) + 1
        if isinstance(node, ast.arg):
            parameters.add(node.arg)
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            if isinstance(node.targets[0], ast.Name):
                assignments[node.targets[0].id] = node
        if isinstance(node, (ast.Global, ast.Nonlocal)):
            parameters.update(node.names)

    def literal(node, seen=None):
        seen = set() if seen is None else seen
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return node.value
        if not isinstance(node, ast.Name) or node.id in seen:
            return None
        assignment = assignments.get(node.id)
        if (
            assignment is None
            or writes.get(node.id) != 1
            or node.id in parameters
            or not isinstance(parents.get(assignment), (ast.Module, ast.FunctionDef, ast.AsyncFunctionDef))
            or assignment.lineno >= node.lineno
        ):
            return None
        # A local in another function is not a binding for this call.
        def scope(value):
            while value in parents:
                value = parents[value]
                if isinstance(value, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Module, ast.ClassDef)):
                    return value
            return tree
        owner = scope(assignment)
        if owner is not tree and owner is not scope(node):
            return None
        return literal(assignment.value, seen | {node.id})

    calls = []
    unresolved = []
    for node in sorted(
        (value for value in ast.walk(tree) if isinstance(value, ast.Call)),
        key=lambda value: (value.lineno, value.col_offset),
    ):
        method = node.func.attr if isinstance(node.func, ast.Attribute) else (
            node.func.id if isinstance(node.func, ast.Name) else ""
        )
        known = isinstance(node.func, ast.Attribute) and method in ("find_by_xpath", "find_all_by_xpath")
        argument = node.args[0] if node.args else next(
            (item.value for item in node.keywords if item.arg == "xpath"), None
        )
        value = literal(argument)
        call_id = "py:{}:{}".format(node.lineno, node.col_offset)
        if known and value == locator:
            calls.append({
                "id": call_id,
                "api": method,
                "query_mode": "single" if method == "find_by_xpath" else "collection",
                "line_start": node.lineno,
                "line_end": node.end_lineno,
                "quote": ast.get_source_segment(source, node),
            })
        elif known and value is None:
            unresolved.append({"id": call_id, "reason": "DYNAMIC_XPATH_ARGUMENT"})
        elif not known and any(
            literal(item) == locator
            for item in list(node.args) + [item.value for item in node.keywords]
        ):
            unresolved.append({"id": call_id, "reason": "UNVERIFIED_WRAPPER_OR_CONSUMER"})
    if selected:
        calls = [item for item in calls if item["id"] == selected]
        unresolved = [item for item in unresolved if item["id"] == selected]
    return {
        "adapter": "python-ast-v1",
        "calls": calls,
        "unresolved": unresolved,
        "complete": bool(calls) and not unresolved,
    }


if __name__ == "__main__":
    try:
        request = json.load(sys.stdin)
        result = inspect_usage(request["source"], request["locator"], request.get("callsite"))
        print(json.dumps(result, ensure_ascii=True))
    except (SyntaxError, ValueError, KeyError) as error:
        print(json.dumps({
            "adapter": "python-ast-v1", "calls": [], "unresolved": [],
            "complete": False, "error": type(error).__name__ + ": " + str(error),
        }))
