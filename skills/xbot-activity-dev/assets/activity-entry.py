"""无外部依赖的 CodeFlow 完整示例，可离线导入测试。

接入影刀时复制为 normalize_text.py，声明 In text:str，正式默认值空串；
声明 Out result:str。通过 CLI 写入、回读并从原流程验收。
此模块不注册公开指令，也不在导入时运行业务。
"""


def normalize_text(text):
    """去除首尾空白；保留字符串内容，非法输入明确报错。"""
    if not isinstance(text, str):
        raise ValueError("文本必须为字符串")
    return text.strip()


def main(args):
    if args is None:
        args = {}

    text = args.get("text")
    args["result"] = normalize_text(text)
