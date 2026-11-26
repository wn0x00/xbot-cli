"""离线测试同目录 activity-entry.py；直接运行本文件时才执行测试。

应用内复用时复制为 test_normalize_text.py，正式模块为 normalize_text.py；
使用相对导入，显式运行所需测试，不注册公开指令或接入正常主流程。
本例只处理文本，无真实业务写动作；离线通过不代表客户端验收完成。
"""

import unittest

if __package__:
    import importlib

    package = importlib.import_module(__package__)
    missing_binding = object()
    previous_binding = vars(package).get("normalize_text", missing_binding)
    try:
        # 显式载入业务模块，并保留包上同名的公开包装函数。
        activity_module = importlib.import_module(".normalize_text", __package__)
    finally:
        if previous_binding is not missing_binding:
            setattr(package, "normalize_text", previous_binding)
else:
    import importlib.util
    from pathlib import Path

    source_path = Path(__file__).with_name("activity-entry.py")
    if not source_path.is_file():
        source_path = Path(__file__).with_name("normalize_text.py")
    source_spec = importlib.util.spec_from_file_location("activity_example", str(source_path))
    activity_module = importlib.util.module_from_spec(source_spec)
    source_spec.loader.exec_module(activity_module)


class NormalizeTextTests(unittest.TestCase):
    def test_normal_and_empty_text(self):
        for text, expected in [(" 示例 ", "示例"), ("", ""), (" \t\n", ""), ("甲 乙", "甲 乙")]:
            with self.subTest(text=text):
                self.assertEqual(activity_module.normalize_text(text), expected)

    def test_invalid_values_are_rejected(self):
        for text in (None, False, 0, [], {}):
            with self.subTest(text=text):
                with self.assertRaisesRegex(ValueError, "文本必须为字符串"):
                    activity_module.normalize_text(text)

    def test_main_writes_out_on_same_args(self):
        args = {"text": " 示例 ", "caller_context": {"keep": True}}
        alias = args
        caller_context = args["caller_context"]
        self.assertIsNone(activity_module.main(args))
        self.assertIs(args["caller_context"], caller_context)
        self.assertEqual(alias["result"], "示例")
        self.assertEqual(args["text"], " 示例 ")

    def test_main_consumes_caller_value(self):
        for text in ("", " 甲 ", "乙"):
            args = {"text": text}
            activity_module.main(args)
            self.assertEqual(args["result"], text.strip())

    def test_missing_input_does_not_get_sample_default(self):
        args = {}
        with self.assertRaises(ValueError):
            activity_module.main(args)
        self.assertEqual(args, {})
        with self.assertRaises(ValueError):
            activity_module.main(None)

    def test_invalid_input_does_not_overwrite_existing_out(self):
        args = {"text": None, "result": "保留原结果"}
        with self.assertRaises(ValueError):
            activity_module.main(args)
        self.assertEqual(args, {"text": None, "result": "保留原结果"})

    def test_main_calls_the_same_business_function(self):
        from unittest.mock import patch

        args = {"text": "调用方输入"}
        with patch.object(activity_module, "normalize_text", return_value="已核验输出") as business:
            activity_module.main(args)
        business.assert_called_once_with("调用方输入")
        self.assertEqual(args["result"], "已核验输出")


if __name__ == "__main__":
    unittest.main()
