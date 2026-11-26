"""离线验证元数据检查器；只在临时目录生成样本，不连接影刀或运行业务源码。"""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import check_metadata as checker


APP_ID = "offline-test-app"
NAMESPACE = "xbot_extensions.activity_test"
REGISTERED = {"active", "deprecated_compatible", "hidden_compatible"}


class MetadataChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / ".dev").mkdir()
        self.policy = {
            "schema_version": 1,
            "app_id": APP_ID,
            "namespace": NAMESPACE,
            "flows": [],
            "allowed_file_changes": [],
            "allowed_json_changes": {},
        }
        self.package = {"uuid": APP_ID, "activity_code": "activity_test", "flows": [], "flow_groups": [{"name": "账户"}], "unknown_field": {"keep": "original"}}
        self.prototype = {"blocks": []}
        self.parameters = {}
        self.configure()

    def write(self, name, value):
        path = self.root / name
        path.write_text(value if isinstance(value, str) else json.dumps(value, ensure_ascii=False), encoding="utf-8")

    def read(self, name):
        return (self.root / name).read_text(encoding="utf-8")

    def configure(self, state="active"):
        registered = state in REGISTERED
        filename = "read_account"
        function = NAMESPACE + "." + filename
        block = NAMESPACE + ".读取账户"
        self.policy["flows"] = [{"filename": filename, "state": state, "function": function, "block": block, "group": "账户"}]
        self.package["flows"] = [{"name": "读取账户", "filename": filename, "opened": registered, "groupName": "账户"}]
        self.prototype["blocks"] = [{
            "name": block,
            "function": function,
            "hidden": not registered,
            "comment": "读取 %account_name%，输出 %records%",
            "inputs": [
                {"name": "account_name", "label": "账户名称", "type": "str", "default": "10:", "required": False},
                {"name": "include_disabled", "label": "包含禁用项", "type": "bool", "default": "13:False", "required": False},
            ],
            "outputs": [{"name": "records", "label": "账户列表", "type": "list"}],
        }]
        self.parameters = {
            "parameters": [
                {"name": "account_name", "direction": "In", "type": "str", "kind": "Text", "value": ""},
                {"name": "include_disabled", "direction": "In", "type": "bool", "kind": "Expression", "value": "False"},
                {"name": "records", "direction": "Out", "type": "list", "kind": "Expression", "value": "[]"},
            ],
            "blocks": [],
        }
        self.flush()
        imports = "from . import package\nimport xbot_visual\n"
        registration = (
            "\ndef read_account(account_name, include_disabled):\n"
            "    outputs = ['records']\n"
            "    inputs = {'account_name': account_name, 'include_disabled': include_disabled}\n"
            "    return xbot_visual.process.run(process='" + function + "', package=__name__, inputs=inputs, outputs=outputs)\n"
        )
        self.write("__init__.py", imports + registration if registered else imports)
        member = "<item kind='function' name='" + function + "'><item.method>read_account(account_name, include_disabled)</item.method></item>"
        self.write("prototype.activity.xml", "<root><item kind='module' name='" + NAMESPACE + "'>" + (member if registered else "") + "</item></root>")
        self.write("read_account.py", "raise RuntimeError('业务源码不得执行')\n")
        self.write(".dev/workspace.state.json", {"openedFlows": ["read_account"]})

    def flush(self):
        self.write("package.json", self.package)
        self.write("prototype.block.json", self.prototype)
        self.write(".dev/read_account.flow.json", self.parameters)

    def snapshot(self):
        return checker.capture(self.root, APP_ID)[0]

    def audit(self, baseline=None, stable=None, phase="offline"):
        return checker.audit(self.root, self.policy, baseline, stable, phase)

    def codes(self, report):
        return {issue["code"] for issue in report["issues"]}

    def errors(self, report):
        return [issue for issue in report["issues"] if issue["severity"] == "error"]

    def test_all_states_keep_distinct_registration_semantics(self):
        for state in checker.STATES:
            with self.subTest(state=state):
                self.configure(state)
                report = self.audit(self.snapshot())
                self.assertEqual(self.errors(report), [])
                self.assertFalse(report["acceptance_complete"])
                if state == "hidden_compatible":
                    self.assertIn("HIDDEN_COMPAT_UNVERIFIED", self.codes(report))
                elif state == "unregistered":
                    self.assertIn("RETIREMENT_EVIDENCE_REQUIRED", self.codes(report))
                else:
                    self.assertEqual(report["status"], "no_findings_in_checked_rules")

    def test_deprecated_entry_cannot_lose_registration(self):
        self.configure("deprecated_compatible")
        baseline = self.snapshot()
        self.configure("internal")
        self.policy["flows"][0]["state"] = "deprecated_compatible"
        codes = self.codes(self.audit(baseline))
        self.assertTrue({"PYTHON_SET", "XML_SET", "OPENED_STATE", "HIDDEN_STATE", "LEGACY_CONTRACT_CHANGED"}.issubset(codes))

    def test_old_public_entry_cannot_be_relabeled_internal(self):
        baseline = self.snapshot()
        self.configure("internal")
        self.assertIn("INTERNAL_WAS_PUBLIC", self.codes(self.audit(baseline)))

    def test_hidden_compatibility_cannot_close_registration(self):
        self.configure("hidden_compatible")
        self.package["flows"][0]["opened"] = False
        self.flush()
        self.assertIn("COMPATIBILITY_REGISTRATION_LOST", self.codes(self.audit(self.snapshot())))

    def test_retirement_requires_evidence_even_with_valid_files(self):
        self.configure("unregistered")
        report = self.audit(self.snapshot())
        self.assertEqual(report["status"], "needs_review")
        self.assertEqual(report["manual_evidence"]["retirement_authorization"], "not_verified")

    def test_legacy_contract_stays_protected_despite_change_permissions(self):
        self.configure("deprecated_compatible")
        baseline = self.snapshot()
        self.parameters["parameters"][0]["value"] = "changed"
        self.prototype["blocks"][0]["inputs"][0]["default"] = "10:changed"
        self.flush()
        self.policy["allowed_file_changes"] = [".dev/read_account.flow.json"]
        self.policy["allowed_json_changes"] = {".dev/read_account.flow.json": ["/parameters/0/value"], "prototype.block.json": ["/blocks/0/inputs/0/default"]}
        report = self.audit(baseline)
        self.assertIn("LEGACY_CONTRACT_CHANGED", self.codes(report))
        self.assertNotIn("UNAPPROVED_JSON_CHANGE", self.codes(report))

    def test_legacy_cosmetic_label_change_does_not_change_contract(self):
        self.configure("deprecated_compatible")
        baseline = self.snapshot()
        self.prototype["blocks"][0]["inputs"][0]["label"] = "旧版账户名称"
        self.flush()
        self.policy["allowed_json_changes"] = {"prototype.block.json": ["/blocks/0/inputs/0/label"]}
        self.assertEqual(self.audit(baseline)["issues"], [])

    def test_legacy_block_identity_cannot_change_under_permission(self):
        self.configure("deprecated_compatible")
        baseline = self.snapshot()
        renamed = NAMESPACE + ".改名后的入口"
        self.policy["flows"][0]["block"] = renamed
        self.prototype["blocks"][0]["name"] = renamed
        self.flush()
        self.policy["allowed_json_changes"] = {"prototype.block.json": ["/blocks/0/name"]}
        self.assertIn("LEGACY_CONTRACT_CHANGED", self.codes(self.audit(baseline)))

    def test_equal_collection_counts_do_not_mask_wrong_member(self):
        self.write("__init__.py", self.read("__init__.py").replace("def read_account(", "def wrong_account("))
        report = self.audit(self.snapshot())
        self.assertIn("PYTHON_SET", self.codes(report))

    def test_policy_must_cover_all_flows(self):
        self.policy["flows"] = []
        self.assertIn("POLICY_COVERAGE", self.codes(self.audit(self.snapshot())))

    def test_duplicate_flow_names_fail_closed(self):
        self.package["flows"].append(dict(self.package["flows"][0], filename="another"))
        self.flush()
        with self.assertRaises(checker.InputError):
            self.audit()

    def test_duplicate_parameter_names_across_directions_fail_closed(self):
        self.parameters["parameters"][2]["name"] = "account_name"
        self.flush()
        with self.assertRaises(checker.InputError):
            self.audit()

    def test_duplicate_block_binding_fails_closed(self):
        self.prototype["blocks"].append(dict(self.prototype["blocks"][0], name=NAMESPACE + ".重复"))
        self.flush()
        with self.assertRaises(checker.InputError):
            self.snapshot()

    def test_foreign_block_cannot_silently_bind_local_function(self):
        self.prototype["blocks"].append({"name": "xbot_extensions.other.command", "function": NAMESPACE + ".read_account"})
        self.flush()
        self.assertIn("CROSS_NAMESPACE_BINDING", self.codes(self.audit(self.snapshot())))

    def test_local_block_cannot_bind_foreign_function(self):
        self.prototype["blocks"][0]["function"] = "xbot_extensions.other.read_account"
        self.flush()
        with self.assertRaises(checker.InputError):
            self.snapshot()

    def test_wrong_function_alias_is_reported(self):
        self.policy["flows"][0]["function"] = NAMESPACE + ".another"
        self.prototype["blocks"][0]["function"] = NAMESPACE + ".another"
        self.flush()
        self.assertIn("FUNCTION_BINDING", self.codes(self.audit(self.snapshot())))

    def test_dangling_group_is_reported(self):
        self.package["flow_groups"] = []
        self.flush()
        self.assertIn("GROUP_REFERENCE", self.codes(self.audit(self.snapshot())))

    def test_duplicate_group_is_rejected(self):
        self.package["flow_groups"].append({"name": "账户"})
        self.flush()
        with self.assertRaises(checker.InputError):
            self.audit()

    def test_parameter_names_types_and_directions(self):
        cases = [
            ("PARAMETER_SET", lambda: self.prototype["blocks"][0]["inputs"].pop()),
            ("PARAMETER_TYPE", lambda: self.prototype["blocks"][0]["inputs"][0].update(type="int")),
            ("PARAMETER_DIRECTION", lambda: self.parameters["parameters"][0].update(direction="InOut")),
        ]
        for code, change in cases:
            with self.subTest(code=code):
                self.configure()
                change()
                self.flush()
                self.assertIn(code, self.codes(self.audit(self.snapshot())))

    def test_ui_order_need_not_match_call_order(self):
        self.prototype["blocks"][0]["inputs"].reverse()
        self.flush()
        self.assertEqual(self.audit(self.snapshot())["issues"], [])

    def test_python_call_order_must_match_flow_order(self):
        self.parameters["parameters"][:2] = self.parameters["parameters"][1::-1]
        self.flush()
        self.assertIn("PYTHON_SIGNATURE", self.codes(self.audit(self.snapshot())))

    def test_missing_types_do_not_match_each_other(self):
        del self.parameters["parameters"][0]["type"]
        del self.prototype["blocks"][0]["inputs"][0]["type"]
        self.flush()
        self.assertIn("PARAMETER_TYPE", self.codes(self.audit(self.snapshot())))

    def test_default_false_zero_none_empty_and_missing_are_distinct(self):
        for value in ("13:0", "13:None", "10:", checker.MISSING):
            with self.subTest(value=value):
                target = self.prototype["blocks"][0]["inputs"][1]
                if value == checker.MISSING:
                    target.pop("default", None)
                else:
                    target["default"] = value
                self.flush()
                self.assertIn("DEFAULT_MISMATCH", self.codes(self.audit(self.snapshot())))

    def test_unknown_default_encoding_requires_review(self):
        self.prototype["blocks"][0]["inputs"][0]["default"] = "99:unknown"
        self.flush()
        self.assertIn("DEFAULT_UNVERIFIED", self.codes(self.audit(self.snapshot())))

    def test_default_expressions_are_not_executed(self):
        sentinel = self.root / "executed.txt"
        expression = "__import__('pathlib').Path(" + repr(str(sentinel)) + ").write_text('bad')"
        self.parameters["parameters"][0].update(kind="Expression", value=expression)
        self.prototype["blocks"][0]["inputs"][0]["default"] = "11:" + expression
        self.flush()
        self.assertEqual(self.audit(self.snapshot())["issues"], [])
        self.assertFalse(sentinel.exists())

    def test_signature_and_outputs_and_input_mapping(self):
        cases = [
            ("PYTHON_SIGNATURE", "account_name, include_disabled):", "account_name):"),
            ("PYTHON_OUTPUTS", "outputs = ['records']", "outputs = []"),
            ("PYTHON_INPUTS", "'account_name': account_name", "'account_name': include_disabled"),
            ("PYTHON_FORWARDING", "outputs=outputs", "outputs=[]"),
            ("PYTHON_TARGET", "process='" + NAMESPACE + ".read_account'", "process='" + NAMESPACE + ".wrong'"),
            ("PYTHON_TARGET_UNVERIFIED", "xbot_visual.process.run(", "unrelated_function("),
        ]
        for code, before, after in cases:
            with self.subTest(code=code):
                self.configure()
                self.write("__init__.py", self.read("__init__.py").replace(before, after))
                self.assertIn(code, self.codes(self.audit(self.snapshot())))

    def test_unknown_registration_style_is_not_certified(self):
        self.write("__init__.py", self.read("__init__.py") + "\nfrom .helpers import run_command\n")
        self.assertIn("REGISTRATION_FORMAT_UNVERIFIED", self.codes(self.audit(self.snapshot())))

    def test_xml_signature_mismatch_is_reported(self):
        self.write("prototype.activity.xml", self.read("prototype.activity.xml").replace("account_name, include_disabled", "account_name"))
        self.assertIn("XML_SIGNATURE", self.codes(self.audit(self.snapshot())))

    def test_xml_namespaces_are_supported(self):
        content = self.read("prototype.activity.xml").replace("<root>", "<root xmlns='urn:offline:test'>")
        self.write("prototype.activity.xml", content)
        self.assertEqual(self.audit(self.snapshot())["issues"], [])

    def test_duplicate_xml_member_is_rejected(self):
        content = self.read("prototype.activity.xml")
        self.write("prototype.activity.xml", content.replace("</item></root>", content[content.index("<item kind='function'"):content.index("</item></root>")] + "</item></root>"))
        with self.assertRaises(checker.InputError):
            self.snapshot()

    def test_xml_entity_declaration_is_rejected(self):
        self.write("prototype.activity.xml", "<!DOCTYPE root [<!ENTITY sample 'value'>]>" + self.read("prototype.activity.xml"))
        with self.assertRaises(checker.InputError):
            self.snapshot()

    def test_missing_source_and_parameter_file_are_not_silent(self):
        (self.root / "read_account.py").unlink()
        (self.root / ".dev/read_account.flow.json").unlink()
        codes = self.codes(self.audit(self.snapshot()))
        self.assertTrue({"FLOW_SOURCE_UNAVAILABLE", "PARAMETERS_UNAVAILABLE"}.issubset(codes))

    def test_unknown_json_fields_are_protected(self):
        baseline = self.snapshot()
        self.package["unknown_field"]["keep"] = "changed"
        self.flush()
        self.assertIn("UNAPPROVED_JSON_CHANGE", self.codes(self.audit(baseline)))

    def test_json_pointer_permission_does_not_cover_siblings(self):
        baseline = self.snapshot()
        self.package["unknown_field"].update(keep="changed", sibling="unexpected")
        self.flush()
        self.policy["allowed_json_changes"] = {"package.json": ["/unknown_field/keep"]}
        report = self.audit(baseline)
        locations = [item["location"] for item in report["issues"] if item["code"] == "UNAPPROVED_JSON_CHANGE"]
        self.assertEqual(locations, ["package.json#/unknown_field/sibling"])

    def test_json_object_array_type_change_cannot_hide_behind_equal_leaf_paths(self):
        self.package["unknown_field"] = {"0": "same"}
        self.flush()
        baseline = self.snapshot()
        self.package["unknown_field"] = ["same"]
        self.flush()
        report = self.audit(baseline)
        self.assertIn("UNAPPROVED_JSON_CHANGE", self.codes(report))
        self.assertTrue(any(item["location"] == "package.json#/unknown_field" for item in report["issues"]))

    def test_json_formatting_only_does_not_create_semantic_drift(self):
        baseline = self.snapshot()
        self.write("package.json", json.dumps(self.package, indent=2))
        self.assertEqual(self.audit(baseline)["issues"], [])

    def test_source_and_workspace_remain_protected(self):
        for name in ("read_account.py", ".dev/workspace.state.json"):
            with self.subTest(name=name):
                self.configure()
                baseline = self.snapshot()
                self.write(name, "changed")
                self.assertIn("PROTECTED_FILE_CHANGED", self.codes(self.audit(baseline)))

    def test_flow_body_changes_require_whole_file_permission(self):
        baseline = self.snapshot()
        self.parameters["blocks"] = [{"id": "different"}]
        self.flush()
        self.assertIn("PROTECTED_FILE_CHANGED", self.codes(self.audit(baseline)))

    def test_stable_comparison_ignores_all_change_permissions(self):
        baseline = self.snapshot()
        self.write("read_account.py", "changed")
        self.policy["allowed_file_changes"] = ["read_account.py"]
        self.assertEqual(self.errors(self.audit(baseline)), [])
        self.assertIn("STABILITY_DRIFT", self.codes(self.audit(baseline, baseline)))

    def test_absent_baseline_and_phase_label_do_not_imply_acceptance(self):
        report = self.audit(phase="after_reopen")
        self.assertIn("BASELINE_NOT_PROVIDED", self.codes(report))
        self.assertEqual(set(report["manual_evidence"].values()), {"not_verified"})
        self.assertFalse(report["acceptance_complete"])

    def test_reopen_phase_requires_stability_comparison(self):
        report = self.audit(self.snapshot(), phase="after_reopen")
        self.assertIn("STABLE_SNAPSHOT_NOT_PROVIDED", self.codes(report))
        self.assertEqual(report["status"], "needs_review")

    def test_incomplete_or_corrupt_baselines_are_rejected(self):
        for field in ("files", "json_fields", "parameter_contracts"):
            with self.subTest(field=field):
                baseline = self.snapshot()
                baseline[field] = {}
                with self.assertRaises(checker.InputError):
                    self.audit(baseline)
        baseline = self.snapshot()
        baseline["files"]["package.json"] = "not-a-hash"
        with self.assertRaises(checker.InputError):
            self.audit(baseline)

    def test_other_app_and_other_root_snapshots_are_rejected(self):
        for field in ("app_id", "namespace", "root"):
            with self.subTest(field=field):
                baseline = self.snapshot()
                baseline[field] = "different"
                with self.assertRaises(checker.InputError):
                    self.audit(baseline)

    def test_app_identity_is_required(self):
        with self.assertRaises(checker.InputError):
            checker.capture(self.root, "wrong-app")

    def test_duplicate_json_keys_are_rejected(self):
        self.write("package.json", '{"uuid":"first","uuid":"second"}')
        with self.assertRaises(checker.InputError):
            self.snapshot()

    def test_paths_cannot_escape_app(self):
        for name in ("../outside", "/outside", "C:/outside", "folder\\file", "./file", ""):
            with self.subTest(name=name):
                with self.assertRaises(checker.InputError):
                    checker.safe_path(self.root, name)

    def test_broad_json_file_permission_is_rejected(self):
        self.policy["allowed_file_changes"] = ["package.json"]
        with self.assertRaises(checker.InputError):
            self.audit()

    def test_concurrent_content_changes_are_detected(self):
        original = checker.watched_files
        counter = [0]

        def changing_files(root):
            counter[0] += 1
            if counter[0] == 2:
                self.write("read_account.py", "changed during read")
            return original(root)

        with patch.object(checker, "watched_files", side_effect=changing_files):
            with self.assertRaises(checker.InputError):
                self.snapshot()

    def test_concurrent_file_addition_is_detected(self):
        original = checker.watched_files
        counter = [0]

        def changing_files(root):
            counter[0] += 1
            if counter[0] == 2:
                self.write("new_module.py", "new")
            return original(root)

        with patch.object(checker, "watched_files", side_effect=changing_files):
            with self.assertRaises(checker.InputError):
                self.snapshot()

    def test_source_is_not_imported_and_app_files_are_not_written(self):
        sentinel = self.root / "executed.txt"
        self.write("__init__.py", "open(" + repr(str(sentinel)) + ", 'w').write('bad')\n" + self.read("__init__.py"))
        before = {path.relative_to(self.root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in self.root.rglob("*") if path.is_file()}
        self.assertEqual(self.audit(self.snapshot())["issues"], [])
        after = {path.relative_to(self.root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in self.root.rglob("*") if path.is_file()}
        self.assertEqual(before, after)
        self.assertFalse(sentinel.exists())

    def test_unknown_action_placeholders_are_reported(self):
        self.prototype["blocks"][0]["comment"] = "读取 %missing_name%"
        self.flush()
        self.assertIn("ACTION_PLACEHOLDER", self.codes(self.audit(self.snapshot())))

    def test_cli_snapshot_and_three_exit_statuses(self):
        command = [sys.executable, "-B", "-X", "utf8", str(Path(checker.__file__).resolve())]
        snapshot = subprocess.run(command + ["snapshot", "--app-dir", str(self.root), "--expect-app", APP_ID], capture_output=True, text=True, encoding="utf-8", check=False)
        self.assertEqual(snapshot.returncode, 0, snapshot.stderr)
        baseline_path = self.root / "before.json"
        baseline_path.write_text(snapshot.stdout, encoding="utf-8")
        policy_path = self.root / "policy.json"
        policy_path.write_text(json.dumps(self.policy), encoding="utf-8")
        arguments = ["check", "--app-dir", str(self.root), "--policy", str(policy_path)]
        for expected, suffix in ((0, ["--baseline", str(baseline_path)]), (2, [])):
            result = subprocess.run(command + arguments + suffix, capture_output=True, text=True, encoding="utf-8", check=False)
            self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
            self.assertFalse(json.loads(result.stdout)["acceptance_complete"])
        self.package["flows"][0]["opened"] = False
        self.flush()
        result = subprocess.run(command + arguments, capture_output=True, text=True, encoding="utf-8", check=False)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "issues_found")


if __name__ == "__main__":
    unittest.main()
