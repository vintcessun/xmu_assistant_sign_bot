# /// script
# requires-python = ">=3.10"
# dependencies = ["opencv-python-headless>=4.9", "numpy>=1.26"]
# ///
"""解一张图里的 TronClass 二维码，按 qr_sign_parse.rs 的规则抠出 42 位 data。

用法:
  uv run agent-browser/qrdecode.py <图片路径>

输出 JSON：{raw, course_id, rollcall_id, data, data_shape_ok, token_ts_utc}
其中 data 就是要拿去 tw 验证的那枚 token。
"""
import json
import re
import sys
import datetime as dt
from urllib.parse import urlparse

import cv2

# 与 src/logic/rollcall/qr_sign_parse.rs 的 TAG_LIST 一致
TAG_LIST = [
    "courseId", "activityId", "activityType", "data", "rollcallId",
    "groupSetId", "accessCode", "action", "enableGroupRollcall",
    "createUser", "joinCourse",
]


def extract_url_tail(s: str) -> str:
    try:
        u = urlparse(s)
        if u.scheme and u.netloc:
            q = ("?" + u.query) if u.query else ""
            return u.path + q
    except Exception:
        pass
    return s


def parse_data(source: str) -> dict:
    if len(source) < 5:
        raise ValueError("data 太短")
    exact = source[5:]  # 跳过前缀（如 /j?p=）
    m = {}
    for e in exact.split("!"):
        if "~" not in e:
            continue
        tag_idx_str, value_raw = e.split("~", 1)
        try:
            tag_idx = int(tag_idx_str)
        except ValueError:
            continue
        tag = TAG_LIST[tag_idx] if 0 <= tag_idx < len(TAG_LIST) else "unknown"
        if value_raw.startswith("\x10"):
            val = str(int(value_raw[1:], 36))
        elif value_raw.startswith("%10"):
            val = str(int(value_raw[3:], 36))
        else:
            val = value_raw
        m[tag] = val
    return m


def main():
    if len(sys.argv) < 2:
        sys.exit("用法: uv run agent-browser/qrdecode.py <图片路径>")
    path = sys.argv[1]
    img = cv2.imread(path)
    if img is None:
        sys.exit(f"读不了图片: {path}")

    detector = cv2.QRCodeDetector()
    raw, points, _ = detector.detectAndDecode(img)
    if not raw:
        # 放大重试一次，密集码有时首次检测不到
        big = cv2.resize(img, None, fx=2.0, fy=2.0, interpolation=cv2.INTER_CUBIC)
        raw, points, _ = detector.detectAndDecode(big)
    if not raw:
        sys.exit("没在图里识别到二维码（换清晰点的截图再试）")

    tail = extract_url_tail(raw)
    m = parse_data(tail)
    data = m.get("data", "")
    shape_ok = bool(re.fullmatch(r"\d{10}[0-9a-fA-F]{32}", data))
    ts_utc = None
    if shape_ok:
        ts_utc = dt.datetime.utcfromtimestamp(int(data[:10])).strftime("%Y-%m-%d %H:%M:%SZ")

    out = {
        "raw": raw,
        "course_id": m.get("courseId"),
        "rollcall_id": m.get("rollcallId"),
        "data": data,
        "data_shape_ok": shape_ok,
        "token_ts_utc": ts_utc,
    }
    print(json.dumps(out, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
