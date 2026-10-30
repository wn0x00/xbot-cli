"""Validate snippets in Python 3.7 and inspect public API calls without an SDK runtime."""
import ast
import json
import sys
import types


class Selector:
    def __init__(self, definition):
        self.definition = definition


selector_module = types.ModuleType("xbot.selector")
selector_module.Selector = Selector
sys.modules["xbot"] = types.ModuleType("xbot")
sys.modules["xbot.selector"] = selector_module


class Element:
    def __init__(self, page, xpath):
        self.page = page
        self.xpath = xpath

    def __getattr__(self, name):
        def record(*args, **kwargs):
            if "--action-fails" in sys.argv:
                raise RuntimeError("TEST_ACTION_FAILURE")
            self.page.calls.append({"method": name, "xpath": self.xpath, "args": args, "kwargs": kwargs})
        return record


class Page:
    def __init__(self):
        self.calls = []

    def find_by_xpath(self, xpath, timeout=20):
        return Element(self, xpath)

    def find(self, selector, timeout=20):
        self.calls.append({"method": "find", "selector": selector.definition, "timeout": timeout})
        if "--find-fails" in sys.argv:
            raise RuntimeError("TEST_FIND_FAILURE")
        return Element(self, selector.definition["xpath"]["segments"])

    def execute_javascript(self, code, argument=None):
        self.calls.append({"method": "execute_javascript", "code": code, "argument": argument})


def main():
    source = sys.stdin.read()
    ast.parse(source)
    page = Page()
    exec(compile(source, "<generated-preview>", "exec"), {"web_page": page, "secret_text": "TEST_SECRET"})
    print(json.dumps({"calls": page.calls}))


if __name__ == "__main__":
    main()
