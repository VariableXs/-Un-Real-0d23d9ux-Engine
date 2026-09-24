#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""vx_handoff_proto.py — Windows 侧交接助手协议参考实现（篇 22 协议面的种子）。

职责（刻意窄，篇 2.6 的"接力棒"口径）：
  1. schema 常量表——与内核 handoff::snap 逐字对表（MD2 篇 2.2 是唯一
     权威定义；本文件是助手侧的实现样本，WP-22x 在此基础上长出真助手）。
  2. integrity 封条的正则化口径——写方（助手写 /var-snap/）与读方
     （VARIX 读 /vx-snap/）必须同一套剥除规则，这是 B-206 互通的地基。
  3. 读面校验——先验哈希再解析（Q6）、字段表零差异、上限护栏。
  4. 夹具生成——黄金夹具（VARIX 形态）与助手样本（Windows 形态），
     供内核 Rust 测试逐字节对锁与跨实现互读实证。

零系统写入：本文件不碰真实交接分区——selftest 全程读写临时目录与
仓库内夹具（与 WP-103 selftest 同一纪律）。

用法：
  python vx_handoff_proto.py gen-fixtures <outdir>   # 生成两份夹具
  python vx_handoff_proto.py check <file.json>       # 读面校验一份快照
  python vx_handoff_proto.py selftest                # 八步自检（B-206 宿主侧证据）
"""

import hashlib
import json
import os
import sys
import tempfile

# ---------------------------------------------------------------------------
# schema 常量表（与 kernel/varix/src/handoff/snap.rs 逐字对表）
# ---------------------------------------------------------------------------

SCHEMA_VERSION = 1

SNAP_DIR_VARIX = "/vx-snap"        # VARIX 写的快照目录（助手只读）
SNAP_DIR_WINDOWS = "/var-snap"     # 助手写的快照目录（VARIX 只读）
SNAP_DIR_DIAG = "/diag"            # 双方可写的诊断包暂存
DRAFT_DIR_VARIX = "/vx-drafts"     # VARIX 侧草稿共享目录
DRAFT_DIR_WINDOWS = "/var-drafts"  # Windows 侧草稿共享目录

CLIP_TEXT_MAX = 256 * 1024  # 剪贴板文本上限（超出截断并置 truncated）
SNAP_LIST_MAX = 256         # 已知清单字段条目上限
SNAP_TEXT_MAX = 1024 * 1024 # 快照全文上限

INTEGRITY_MARK = ',"integrity":"'

# 篇 2.2 字段表（读面校验的对照表；integrity 是封皮不计入 body 校验）
TOP_KEYS = ["schema_version", "created_at", "source_domain", "windows",
            "drafts", "clipboard", "restore_hint"]
WIN_KEYS = ["app_id", "title", "geometry", "workspace", "focused"]
GEO_KEYS = ["x", "y", "w", "h", "output"]
DRAFT_KEYS = ["path", "sha256", "app_id"]
CLIP_KEYS_MIN = ["kind", "content", "truncated"]


class SnapError(Exception):
    """读面失败（人话 reason 必填——优雅放弃不是静默放弃）。"""

    def __init__(self, code, reason):
        super().__init__(reason)
        self.code = code
        self.reason = reason


# ---------------------------------------------------------------------------
# integrity 封条（与 Rust strip_integrity 同一正则化口径）
# ---------------------------------------------------------------------------

def seal(body_text):
    """给不含 integrity 成员的 body 盖封条（返回最终落盘文本）。"""
    if not body_text.endswith("}"):
        raise SnapError("no-integrity", "body 必须以 } 收尾")
    digest = hashlib.sha256(body_text.encode("utf-8")).hexdigest()
    return body_text[:-1] + ',"integrity":"' + digest + '"}'


def strip_seal(text):
    """剥封条：返回 (body, 声明的 hex)。格式不对抛 SnapError。"""
    if not text.endswith('"}'):
        raise SnapError("no-integrity", "快照没有 integrity 封条或封条格式不对")
    idx = text.rfind(INTEGRITY_MARK)
    if idx < 0:
        raise SnapError("no-integrity", "快照没有 integrity 封条")
    hex_and_close = text[idx + len(INTEGRITY_MARK):]
    if len(hex_and_close) != 64 + 2 or not hex_and_close.endswith('"}'):
        raise SnapError("no-integrity", "封条格式不对（应为 64 位十六进制 + \"}）")
    declared = hex_and_close[:64].lower()
    if any(c not in "0123456789abcdef" for c in declared):
        raise SnapError("no-integrity", "封条不是十六进制")
    body = text[:idx] + "}"
    return body, declared


def verify_seal(text):
    """先验哈希——不过不解析（Q6：宁可放弃恢复也不解析半截数据）。"""
    body, declared = strip_seal(text)
    actual = hashlib.sha256(body.encode("utf-8")).hexdigest()
    if actual != declared:
        raise SnapError(
            "integrity-mismatch",
            "快照完整性校验失败（声明 {} 实算 {}）——宁可放弃恢复也不解析半截数据".format(
                declared, actual))
    return body


# ---------------------------------------------------------------------------
# 读面校验（字段表零差异 + 护栏）
# ---------------------------------------------------------------------------

def _hex64(s):
    return isinstance(s, str) and len(s) == 64 and \
        all(c in "0123456789abcdefABCDEF" for c in s)


def _no_dup_pairs(pairs):
    """object_pairs_hook：重复键一律拒绝——写方必须是确定性序列化（与
    内核解析面同一纪律；integrity 剥除规则依赖它）。"""
    seen = {}
    for k, v in pairs:
        if k in seen:
            raise SnapError("schema", '重复键 "{}"——快照写方必须是确定性序列化'.format(k))
        seen[k] = v
    return seen


def validate(body_text):
    """校验 body 并返回 (快照 dict, 降级标记, 注记列表)。"""
    try:
        doc = json.loads(body_text, object_pairs_hook=_no_dup_pairs)
    except json.JSONDecodeError as e:
        raise SnapError("syntax", "JSON 语法错误：{}".format(e))
    notes = []

    version = doc.get("schema_version")
    if not isinstance(version, int) or isinstance(version, bool):
        raise SnapError("schema", "schema_version 必须是整数")
    degraded = version > SCHEMA_VERSION
    if version < SCHEMA_VERSION:
        raise SnapError("schema", "快照 schema 版本 {} 低于最低支持版本".format(version))
    if degraded:
        notes.append("快照 schema 版本 {} 高于本侧支持版本 {}——按能读多少读多少降级".format(
            version, SCHEMA_VERSION))

    created = doc.get("created_at")
    if not isinstance(created, str) or not created.endswith("Z"):
        raise SnapError("schema", "created_at 必须是带时区标记的 UTC 时间戳（…Z）")

    domain = doc.get("source_domain")
    if domain not in ("varix", "windows"):
        raise SnapError("schema", "source_domain {!r} 不在枚举内".format(domain))

    windows = doc.get("windows")
    if not isinstance(windows, list):
        raise SnapError("schema", "windows 必须是数组")
    if len(windows) > SNAP_LIST_MAX:
        raise SnapError("limit", "windows 条目数超出护栏")
    for w in windows:
        if not isinstance(w, dict):
            raise SnapError("schema", "windows[] 必须是对象")
        for k in WIN_KEYS:
            if k not in w and not degraded:
                raise SnapError("schema", "windows[] 缺少必填字段 {}".format(k))
        geo = w.get("geometry", {})
        if not isinstance(geo, dict) or any(k not in geo for k in GEO_KEYS):
            raise SnapError("schema", "geometry 必须是 x/y/w/h/output 的整数组合")
        for k in GEO_KEYS:
            if not isinstance(geo[k], int) or isinstance(geo[k], bool):
                raise SnapError("schema", "geometry.{} 必须是整数".format(k))
        if not isinstance(w.get("workspace", 0), int):
            raise SnapError("schema", "workspace 必须是整数")
        if not isinstance(w.get("focused", False), bool):
            raise SnapError("schema", "focused 必须是布尔")

    drafts = doc.get("drafts")
    if not isinstance(drafts, list):
        raise SnapError("schema", "drafts 必须是数组")
    if len(drafts) > SNAP_LIST_MAX:
        raise SnapError("limit", "drafts 条目数超出护栏")
    for d in drafts:
        if not isinstance(d, dict):
            raise SnapError("schema", "drafts[] 必须是对象")
        for k in DRAFT_KEYS:
            if k not in d and not degraded:
                raise SnapError("schema", "drafts[] 缺少必填字段 {}".format(k))
        if not _hex64(d.get("sha256", "")):
            raise SnapError("schema", "drafts[].sha256 必须是 64 位十六进制")

    clip = doc.get("clipboard")
    if not isinstance(clip, dict):
        raise SnapError("schema", "clipboard 必须是对象")
    for k in CLIP_KEYS_MIN:
        if k not in clip and not degraded:
            raise SnapError("schema", "clipboard 缺少必填字段 {}".format(k))
    if clip.get("kind") not in ("text", "files"):
        raise SnapError("schema", "clipboard.kind {!r} 不在枚举内".format(clip.get("kind")))
    if clip["kind"] == "text":
        if not isinstance(clip.get("content"), str):
            raise SnapError("schema", "text 类 content 必须是字符串")
        if len(clip["content"].encode("utf-8")) > CLIP_TEXT_MAX:
            if not clip.get("truncated"):
                raise SnapError("schema", "text 超 256KB 但未置 truncated 标记")
    else:
        files = clip.get("content")
        if not isinstance(files, list) or any(not isinstance(p, str) for p in files):
            raise SnapError("schema", "files 类 content 必须是路径数组")
        if len(files) > SNAP_LIST_MAX:
            raise SnapError("limit", "files 路径数超出护栏")

    hint = doc.get("restore_hint")
    if not isinstance(hint, str):
        raise SnapError("schema", "restore_hint 必须是字符串")

    if degraded:
        for k in TOP_KEYS:
            if k not in doc:
                notes.append("缺少字段 {!r}（更高版本 schema，按能读多少读多少降级）".format(k))
    return doc, degraded, notes


def read_snapshot(path):
    """读面入口：读文件 → 先验哈希 → 解析校验。失败抛 SnapError（人话）。"""
    with open(path, "rb") as f:
        raw = f.read()
    if len(raw) > SNAP_TEXT_MAX:
        raise SnapError("limit", "快照全文超出护栏")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as e:
        raise SnapError("not-utf8", "快照不是合法 UTF-8：{}".format(e))
    body = verify_seal(text)
    doc, degraded, notes = validate(body)
    return doc, degraded, notes


# ---------------------------------------------------------------------------
# 夹具生成（黄金夹具 = Rust 写方产出的逐字节对锁目标）
# ---------------------------------------------------------------------------

def _varix_canonical():
    """VARIX 规范样本——与 kernel snap.rs tests::canonical 逐字段一致。"""
    return {
        "schema_version": 1,
        "created_at": "2026-09-24T08:30:00Z",
        "source_domain": "varix",
        "windows": [
            {"app_id": "vx.editor", "title": "README.md - Varix Editor",
             "geometry": {"x": 64, "y": 48, "w": 1152, "h": 704, "output": 0},
             "workspace": 1, "focused": True},
            {"app_id": "Wine/default/TotalCommander", "title": "",
             "geometry": {"x": 128, "y": 96, "w": 960, "h": 600, "output": 1},
             "workspace": 2, "focused": False},
        ],
        "drafts": [
            {"path": "/vx-drafts/notes-2026-09-24.md",
             "sha256": "1" * 64, "app_id": "vx.editor"},
        ],
        "clipboard": {"kind": "text", "content": "交接协议测试剪贴板内容",
                      "truncated": False},
        "restore_hint": "回到 VARIX 时恢复 2 个窗口与 1 份草稿",
    }


def _windows_sample():
    """助手写面样本（篇 22.2 形态：临时文件+原子换名+全文件哈希）。"""
    return {
        "schema_version": 1,
        "created_at": "2026-09-24T09:15:00Z",
        "source_domain": "windows",
        "windows": [
            {"app_id": "com.variable.explorer",
             "title": "Q4 Report - Variable Explorer",
             "geometry": {"x": 100, "y": 80, "w": 1200, "h": 800, "output": 0},
             "workspace": 0, "focused": True},
        ],
        "drafts": [
            {"path": "/var-drafts/report-q4.xlsx",
             "sha256": "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
             "app_id": "com.variable.sheets"},
        ],
        "clipboard": {"kind": "files",
                      "content": ["/var-drafts/notes.txt", "/var-drafts/summary.csv"],
                      "truncated": False,
                      "clipboard_skipped_reason": "password-manager-clipboard-skipped"},
        "restore_hint": "Back on Windows: restore 1 window; clipboard carried 2 files; "
                        "password manager copy was skipped",
    }


def serialize_snapshot(doc):
    """写面（篇 22.2：全文件哈希进 integrity）。键序 = schema 冻结键序。"""
    ordered = {k: doc[k] for k in TOP_KEYS}
    body = json.dumps(ordered, ensure_ascii=False, separators=(",", ":"))
    return seal(body)


def gen_fixtures(outdir):
    """生成两份夹具：VARIX 黄金夹具 + Windows 助手样本。"""
    os.makedirs(outdir, exist_ok=True)
    varix_path = os.path.join(outdir, "handoff-varix.json")
    windows_path = os.path.join(outdir, "handoff-windows.json")
    with open(varix_path, "w", encoding="utf-8", newline="") as f:
        f.write(serialize_snapshot(_varix_canonical()))
    with open(windows_path, "w", encoding="utf-8", newline="") as f:
        f.write(serialize_snapshot(_windows_sample()))
    return varix_path, windows_path


# ---------------------------------------------------------------------------
# selftest（B-206 宿主侧证据链；零真实系统接触）
# ---------------------------------------------------------------------------

def _repo_fixtures_dir():
    return os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        "..", "..", "..", "kernel", "varix", "src", "handoff", "fixtures")


def selftest():
    ok = 0
    fails = []

    def check(name, cond, detail=""):
        nonlocal ok
        if cond:
            ok += 1
            print("PASS  {}".format(name))
        else:
            fails.append(name)
            print("FAIL  {} {}".format(name, detail))

    tmp = tempfile.mkdtemp(prefix="vxhandoff-")
    varix_path, windows_path = gen_fixtures(tmp)

    # ① 生成面：两份夹具都是合法快照（先验哈希→解析校验）。
    doc_v, deg_v, notes_v = read_snapshot(varix_path)
    check("① VARIX 夹具生成+读回", doc_v["source_domain"] == "varix" and not deg_v)
    doc_w, deg_w, _ = read_snapshot(windows_path)
    check("② Windows 样本生成+读回", doc_w["source_domain"] == "windows" and not deg_w)

    # ② 互通：助手读 VARIX 黄金夹具（gen 产物）。
    check("③ 助手读 VARIX 快照", len(doc_v["windows"]) == 2 and len(doc_v["drafts"]) == 1)
    # ③ VARIX 读助手样本（内核 Rust 测试跑同一份的读面——此处验字段面）。
    check("④ VARIX 读 Windows 快照",
          doc_w["clipboard"]["kind"] == "files"
          and doc_w["clipboard"]["clipboard_skipped_reason"] == "password-manager-clipboard-skipped")

    # ④ 字段表零差异（B-202 助手侧对表）。
    body_v, _ = strip_seal(open(varix_path, encoding="utf-8").read())
    ordered_keys = list(json.loads(body_v).keys())
    check("⑤ 顶层键序与字段表零差异", ordered_keys == TOP_KEYS, str(ordered_keys))

    # ⑤ 封条自证：body 一个字节都不能动。
    text_v = open(varix_path, encoding="utf-8").read()
    tampered = text_v.replace("README", "RЗADME")  # 外形相似的不同字节
    try:
        verify_seal(tampered)
        tamper_caught = False
    except SnapError:
        tamper_caught = True
    check("⑥ 篡改现行（integrity 自证）", tamper_caught)

    # ⑥ 十次损坏注入，全部优雅放弃（B-207 助手侧对表）。
    good = text_v
    idx = good.rfind(INTEGRITY_MARK)
    corruptions = [
        ("body 翻字节", good.replace("vx.editor", "vx.editur", 1)),
        ("截尾", good[:-10]),
        ("摘除封条", good[:idx] + "}"),
        ("封条作废", good[:idx + len(INTEGRITY_MARK)] + "0" * 64 + '"}'),
        ("封条非十六进制", good[:idx + len(INTEGRITY_MARK)] + "z" * 64 + '"}'),
        ("类型错", good.replace('"schema_version":1', '"schema_version":"1"')),
        ("结构错", good.replace('"windows":[{', '"windows":{')),
        ("枚举错", good.replace('"kind":"text"', '"kind":"weird"')),
        ("整数给浮点", good.replace('"workspace":1', '"workspace":1.5')),
        ("重复键", good.replace('"restore_hint":',
                                '"created_at":"2026-01-01T00:00:00Z","restore_hint":')),
    ]
    caught = 0
    for label, bad in corruptions:
        try:
            body = verify_seal(bad)
            validate(body)
            print("      未拦截: {}".format(label))
        except SnapError:
            caught += 1
        except (ValueError, KeyError):
            caught += 1
    check("⑦ 十组损坏注入全部优雅放弃", caught == len(corruptions),
          "{}/{}".format(caught, len(corruptions)))

    # ⑦ 版本协商（Q18）：更高版本降级读 + 注记。
    v2 = dict(doc_v)
    v2["schema_version"] = 2
    v2["theme"] = "dark"
    v2_body = json.dumps({k: v2[k] for k in
                          ["schema_version", "created_at", "source_domain", "windows",
                           "drafts", "clipboard", "restore_hint", "theme"]},
                         ensure_ascii=False, separators=(",", ":"))
    v2_doc, v2_deg, v2_notes = validate(v2_body)
    check("⑧ 更高版本降级读+注记", v2_deg and v2_doc["windows"] and v2_notes)

    # ⑧ 仓库夹具与生成产物逐字节一致（跨实现契约的锚）。
    repo_dir = os.path.normpath(_repo_fixtures_dir())
    repo_varix = os.path.join(repo_dir, "handoff-varix.json")
    repo_windows = os.path.join(repo_dir, "handoff-windows.json")
    if os.path.exists(repo_varix) and os.path.exists(repo_windows):
        same_v = open(repo_varix, "rb").read() == open(varix_path, "rb").read()
        same_w = open(repo_windows, "rb").read() == open(windows_path, "rb").read()
        check("⑨ 仓库夹具与生成器逐字节一致", same_v and same_w)
    else:
        check("⑨ 仓库夹具与生成器逐字节一致", False, "仓库夹具缺失: " + repo_dir)

    print("")
    print("selftest: {} 项断言, {} 通过{}".format(
        ok + len(fails), ok, "" if not fails else ", 失败: " + ", ".join(fails)))
    if fails:
        sys.exit(2)
    print("CLEAN EXIT")


def main(argv):
    if len(argv) >= 2 and argv[1] == "gen-fixtures" and len(argv) == 3:
        a, b = gen_fixtures(argv[2])
        print(a)
        print(b)
        return 0
    if len(argv) >= 2 and argv[1] == "check" and len(argv) == 3:
        doc, degraded, notes = read_snapshot(argv[2])
        print(json.dumps({"source_domain": doc["source_domain"],
                          "windows": len(doc.get("windows", [])),
                          "drafts": len(doc.get("drafts", [])),
                          "degraded": degraded, "notes": notes},
                         ensure_ascii=False))
        return 0
    if len(argv) == 2 and argv[1] == "selftest":
        selftest()
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
