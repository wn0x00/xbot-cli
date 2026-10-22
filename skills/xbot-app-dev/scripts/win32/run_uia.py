"""Run a trusted local UIA request; sending requires an explicit request flag."""
import argparse
import json
import os
import sys

sys.dont_write_bytecode = True

from uia_runtime import execute, save_new


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    if os.path.exists(args.out):
        parser.error("--out must be a new path")
    with open(args.request, "r", encoding="utf-8") as stream:
        request = json.load(stream)
    try:
        result = {"ok": True, "data": execute(request)}
    except Exception as error:
        result = {"ok": False, "error": {
            "code": getattr(error, "code", "UIA_PROVIDER_ERROR"),
            "detail": getattr(error, "detail", {}),
            "message": str(error), "retryable": False
        }}
    save_new(args.out, result)
    print(json.dumps(result, ensure_ascii=True))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
