# -*- coding: utf-8 -*-
"""
下一轮提示词自动产出器（cdp_send_handoff.py 配套的产出端）。

为什么单独成文件而不是塞进 ai_worker_loop.py：
  ai_worker_loop.py 是产线驱动器，重启频繁、逻辑已长；
  本文件是"交接内容生成器"，逻辑独立、可单独跑、可单独重跑。
  两者通过 _next_prompt.md 这个文件解耦——产线写它，CDP 脚本读它。

产出内容全部来自真数据源（任务板 API + 真 taskboard.md + 磁盘真实行数），
不写死任何状态。每次跑都重新读，改了代码重跑即可刷新。

用法：
  python emit_next_prompt.py                # 生成 _next_prompt.md
  python emit_next_prompt.py --task VE-F0004  # 指定单（默认自动探测下一单）
  python emit_next_prompt.py --print        # 只打到stdout，不落盘
"""

import io
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
REAL_MD = os.path.join(HERE, "VTaskBoard", "taskboard.md")
OUT = os.path.join(HERE, "_next_prompt.md")
API = "http://127.0.0.1:8767"
SVSTAR2 = os.path.join(REPO, "kernel", "varix", "src", "svstar2")
LEDGER = os.path.join(HERE, "_ledger.jsonl")

BOOK_FILES = {
    "VE": os.path.join(REPO, "docs", "Varix", "VE-STAR-II",
                      "VE Varix STAR II · 总纲与施工书.md"),
    "CGPU": os.path.join(REPO, "docs", "Varix", "VE-STAR-II",
                        "CGPU Varix STAR II · 总纲与施工书.md"),
    "CoRun": os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                          "CoRun Varix STAR II · Unxreal.md"),
}

# VE 册 32 域地图（从册内域地图表抄下来，用于把 F 序号翻译成域）
VE_DOMAINS = [
    ("VE-A", 1, 200), ("VE-B", 201, 400), ("VE-C", 401, 600), ("VE-D", 601, 800),
    ("VE-E", 801, 1000), ("VE-F", 1001, 1200), ("VE-G", 1201, 1400),
    ("VE-H", 1401, 1600), ("VE-I", 1601, 1800), ("VE-J", 1801, 2000),
    ("VE-K", 2001, 2200), ("VE-L", 2201, 2400), ("VE-M", 2401, 2600),
    ("VE-N", 2601, 2800), ("VE-O", 2801, 3000), ("VE-P", 3001, 3200),
    ("VE-Q", 3201, 3400), ("VE-R", 3401, 3600), ("VE-S", 3601, 3800),
    ("VE-T", 3801, 4000), ("VE-U", 4001, 4200), ("VE-V", 4201, 4400),
    ("VE-W", 4401, 4600), ("VE-X", 4601, 4800), ("VE-Y", 4801, 5000),
    ("VE-Z", 5001, 5200), ("VE-AA", 5201, 5400), ("VE-AB", 5401, 5600),
    ("VE-AC", 5601, 5800), ("VE-AD", 5801, 6000), ("VE-AE", 6001, 6200),
    ("VE-AF", 6201, 6400),
]


def api(path, body=None, timeout=30):
    """POST/GET 任务板接口。失败如实抛，不静默返回空。"""
    url = API + path
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json; charset=utf-8"
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def read_text(path):
    """读文件；不存在或过大如实返回 None。"""
    if not os.path.exists(path):
        return None
    try:
        if os.path.getsize(path) > 80 * 1024 * 1024:
            return None          # CoRun 册 165MB，不整读
        with io.open(path, encoding="utf-8") as f:
            return f.read()
    except OSError:
        return None


# --------------------------------------------------------------------- #
# 锚点原文抽取
# --------------------------------------------------------------------- #
def extract_anchor(book, task_id):
    """
    从册内抽 `### <task_id> · 标题（目标 NNN 行）` 到下一个 `###` 之间的正文。
    抽不到就如实返回 None——不编造判据。
    """
    path = BOOK_FILES.get(book)
    if not path or not os.path.exists(path):
        return None
    pat = re.compile(r"^###\s+" + re.escape(task_id) + r"\b", re.M)
    anchors = {
        "VE": path,
        "CGPU": path,
        "CoRun": os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                              "CoRun Varix STAR II · Unxreal.md"),
    }
    if book == "CoRun":
        # 165MB 巨册：只流式扫目标锚点段，避免整读
        return _extract_anchor_stream(path, task_id, pat)
    s = read_text(anchors[book])
    if s is None:
        return None
    m = pat.search(s)
    if not m:
        return None
    start = m.start()
    nxt = re.search(r"^###\s+", s[m.end():], re.M)
    end = m.end() + (nxt.start() if nxt else 0)
    return s[start:end].strip()


def _extract_anchor_stream(path, task_id, pat, window=20000):
    """巨册流式抽锚点：命中后继续读到下一个 ### 为止。"""
    marker = ("### " + task_id).encode("utf-8")
    next_h3 = b"### "
    with open(path, "rb") as f:
        buf = b""
        found = -1
        while True:
            chunk = f.read(1 << 20)
            if not chunk:
                break
            buf += chunk
            if found < 0:
                i = buf.find(marker)
                if i >= 0:
                    found = i
                    # 已读够一段，先试着收口
                    j = buf.find(next_h3, i + len(marker))
                    if j >= 0:
                        return buf[found:j].decode("utf-8", "replace").strip()
            else:
                j = buf.find(next_h3, found + len(marker))
                if j >= 0:
                    return buf[found:j].decode("utf-8", "replace").strip()
            if found >= 0 and len(buf) - found > window:
                return buf[found:found + window].decode("utf-8", "replace").strip()
            if found < 0:
                buf = buf[-len(marker) - 32:]     # 未命中，只留够匹配的尾巴
    return None


# --------------------------------------------------------------------- #
# 任务板真状态
# --------------------------------------------------------------------- #
def md_state_map():
    """读真 taskboard.md，取每单状态/前置/行数/册/域/标题。"""
    states, pres, lines, books, doms, titles = {}, {}, {}, {}, {}, {}
    cur = None
    with io.open(REAL_MD, encoding="utf-8") as f:
        for ln in f:
            if ln.startswith("## ["):
                cur = None
                seg = ln[4:ln.find("]")] if "]" in ln else ""
                if seg:
                    cur = seg
                    states.setdefault(cur, "待领")
                    pres.setdefault(cur, "")
                    lines.setdefault(cur, "")
                    books.setdefault(cur, "")
                    doms.setdefault(cur, "")
                    titles.setdefault(cur, "")
            elif cur:
                s = ln.strip()
                if s.startswith("- 状态:"):
                    states[cur] = s.split(":", 1)[1].strip()
                elif s.startswith("- 前置:"):
                    pres[cur] = s.split(":", 1)[1].strip()
                elif s.startswith("- 行数:"):
                    lines[cur] = s.split(":", 1)[1].strip()
                elif s.startswith("- 册:"):
                    books[cur] = s.split(":", 1)[1].strip()
                elif s.startswith("- 域:"):
                    doms[cur] = s.split(":", 1)[1].strip()
                elif s.startswith("## ") or s.startswith("] "):
                    titles[cur] = s.lstrip("] ").strip()
    return {"状态": states, "前置": pres, "行数": lines,
            "册": books, "域": doms, "标题": titles}


def ledger_done_list():
    """
    台账里已完成的单（真交活记录，跨 exe 重启不丢）。

    同一单可能有多条记录（如VE-F0001 先TS 后迁 Rust 各回写一次），
    这里按 id 去重，保留最后一条——计数必须等于真完成单数，不能虚高。
    """
    latest = {}
    order = []
    if not os.path.exists(LEDGER):
        return []
    with io.open(LEDGER, encoding="utf-8") as f:
        for ln in f:
            ln = ln.strip()
            if not ln:
                continue
            try:
                r = json.loads(ln)
            except ValueError:
                continue
            if r.get("状态") == "已完成" and r.get("id"):
                tid = r["id"]
                if tid not in latest:
                    order.append(tid)
                latest[tid] = (tid, r.get("时间", ""), r.get("领取人", ""))
    return [latest[t] for t in order]


def domain_of(task_id):
    """
    VE-F0123 → ('VE-A', 123, lo, hi)。非VE 册返回 None。

    返回的第三、四项是域的起止序号（供算域内位置），域内位置 =绝对序号 - 起点 + 1。
    """
    m = re.match(r"^VE-F(\d{4})$", task_id or "")
    if not m:
        return None
    n = int(m.group(1))
    for name, lo, hi in VE_DOMAINS:
        if lo <= n <= hi:
            return name, n, lo, hi
    return None


# --------------------------------------------------------------------- #
# 磁盘真实进度
# --------------------------------------------------------------------- #
def svstar2_stats():
    """svstar2 目录真实文件数与纯功能行数（纯功能 = 排除空行与纯注释行）。"""
    if not os.path.isdir(SVSTAR2):
        return {"文件数": 0, "总行": 0, "纯功能行": 0, "文件": []}
    files = sorted(f for f in os.listdir(SVSTAR2) if f.endswith(".rs"))
    pure = 0
    total = 0
    per = []
    for f in files:
        p = os.path.join(SVSTAR2, f)
        with io.open(p, encoding="utf-8") as fh:
            src = fh.read()
        ls = src.split("\n")
        n = 0
        for ln in ls:
            t = ln.strip()
            if not t:
                continue
            if t.startswith("//") or t.startswith("/*") or t.startswith("*"):
                continue
            n += 1
        pure += n
        total += len(ls)
        per.append((f, n, len(ls)))
    return {"文件数": len(files), "总行": total, "纯功能行": pure, "文件": per}


def suggest_module(task_id):
    """
    VE-F0004 → vea04（仓库既有命名：vea01 / vea02 / vea03 / vea04…）。

    规则：以整数解析后按 %02d 格式化。
    不用字符串截取——那是猜；也不用 lstrip("0")——它按字符集剥会把 "0004" 啃成 "4"。
    超过 99 项时 %02d 自然给出三位（vea100），与册内序号一一对应。
    """
    m = re.match(r"^VE-F(\d{4})$", task_id or "")
    if not m:
        return None
    return "vea%02d" % int(m.group(1))


# --------------------------------------------------------------------- #
# 主产出
# --------------------------------------------------------------------- #
def build(task_id=None, book=None):
    sm = md_state_map()
    states, pres, lns, books, doms = (sm["状态"], sm["前置"], sm["行数"],
                                      sm["册"], sm["域"])
    api_sum = {}
    try:
        api_sum = api("/api/summary")
    except Exception as e:                      # noqa: BLE001
        api_sum = {"错误": str(e)}

    by_status = api_sum.get("by_status") or {}
    by_book = api_sum.get("by_book") or {}

    # 定下一单：优先显式指定，否则从软件领一张再退回。
    #
    # ★ 这段曾出过一次真缺陷：release 的返回值被丢弃，若release 失败（网络抖、
    #   服务正忙），探测用的单就永久卡在「已领」状态——下次别的 AI 领单会被它挡住。
    #   现在用 try/finally 保证必退，且退不掉时**如实报错并回显卡住的单号**，
    #   不静默继续（异常零静默）。
    probe_failed = None
    if task_id is None:
        claimed_id = None
        try:
            r = api("/api/claim", {"worker": "__emit_probe__"})
            if r.get("ok"):
                claimed_id = r["task"]["id"]
                task_id = claimed_id
        except Exception as e:                      # noqa: BLE001
            return {"ok": False, "错误": "任务板不可达：%s" % e}
        finally:
            if claimed_id:
                try:
                    rr = api("/api/release", {"id": claimed_id})
                    if not rr.get("ok"):
                        probe_failed = "退还失败：%s" % rr.get("err", rr)
                except Exception as e:              # noqa: BLE001
                    probe_failed = "退还请求异常：%s" % e
    if task_id is None:
        return {"ok": False, "错误": "无可领单，交接内容无法生成"}

    book = book or books.get(task_id, "")
    title = doms.get(task_id, "") and doms.get(task_id) or ""
    # 标题从 md 首行抽
    mt = re.search(r"^##\s*\[" + re.escape(task_id) + r"\]\s*(.+)$",
                   read_text(REAL_MD) or "", re.M)
    title = mt.group(1).strip() if mt else title
    dom = domain_of(task_id)
    # domain_of 给的是**全书绝对序号**（如F0201=201），域内位置要减掉域起点。
    # 不减就会算出「第 201 项 / 域内共 200」这种自相矛盾的显示。
    if dom:
        dom_name, abs_no, lo, hi = dom
        dom_label = "%s（域内第 %d 项 / 本域 %d 项，F%04d–F%04d）" % (
            dom_name, abs_no - lo + 1, hi - lo + 1, lo, hi)
    else:
        dom_label = doms.get(task_id) or "—"
    anchor = extract_anchor(book, task_id) or "（锚点抽取失败——按任务板验收字段施工，勿猜判据）"
    anchor_short = anchor[:1800] + ("\n…（已截断，全文见书路径锚点）" if len(anchor) > 1800 else "")
    done_list = ledger_done_list()
    st = svstar2_stats()
    mod = suggest_module(task_id)
    target = lns.get(task_id, "") or "见锚点"

    md = []
    A = md.append
    A("# 下一轮施工提示词（自动生成，勿手改）\n")
    A("> 由 `VarixTaskOps/emit_next_prompt.py` 于 %s 生成。" % time.strftime("%Y-%m-%d %H:%M:%S"))
    A("> 配套发送器：`_attic/2026-10-04-ve-产线/cdp/cdp_send_handoff.mjs`")
    A("> 数据全部来自真任务板 API + 真 `taskboard.md` + 磁盘真实行数，无写死状态。\n")

    A("## 你要做的事（这一轮）\n")
    A("在 **Agent 模式** 下施工任务 **%s · %s**。\n" % (task_id, title))
    A("- 册：%s　域：%s" % (book or "—", dom_label))
    A("- 纯功能行数目标：**%s 行**（低于目标 90%% 回炉）" % target)
    A("- 前置：%s（前置未完成不许领，本单已由软件校验解锁）" % (pres.get(task_id) or "无"))
    A("- 建议落点：`kernel/varix/src/svstar2/%s.rs`" % (mod or "veaNN_xxx"))
    A("- 自检函数：`run_%s_checks()`，登记进 `svstar2/mod.rs` 的 `blocks` 数组\n" % (mod or "veaNN"))

    A("## 锚点原文（判据逐条落实，不得增减）\n")
    A("```\n%s\n```\n" % anchor_short)

    A("## 落位铁律（三册全是 Rust，写在内核树，不在前端）\n")
    A("权威依据 `docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md:36`：")
    A("「全部功能针对 VARIX Rust 内核编写；内核态只留合成裁决与安全，VE 全部活在用户态服务」。\n")
    A("- **用户态服务** = 不在内核态特权上下文跑，**不是**「能用前端语言写」")
    A("- 内核树是 no_std Rust（`[dependencies]` 为空、`[[bin]]` 带 `required-features=[\"kernel-image\"]`）")
    A("- TS 编译出 JS 必须靠 V8/JSC，内核态无堆无GC，且 GC 停顿与内核延迟硬要求冲突")
    A("- 任务板 `vtaskboard.py:2213` 的 `BOOK_LANG={\"VE\":\"TypeScript\"}` 是**显示用启发式**，不决定落位")
    A("- `svstar2/mod.rs` 有断言 `vea_lives_in_kernel_not_frontend` 守住这条线\n")

    A("## 当前真实进度（自动实测，勿信记忆）\n")
    A("| 册 | 总数 | 已完成 | 待领 |")
    A("|---|---|---|---|")
    for b in ("VE", "CGPU", "CoRun"):
        d = by_book.get(b) or {}
        A("| %s | %s | %s | %s |" % (b, d.get("总", "—"), d.get("已完成", "—"), d.get("待领", "—")))
    A("")
    A("- `svstar2/` 现有 **%d 个 .rs 文件，共 %d 行（纯功能 %d 行）**" % (st["文件数"], st["总行"], st["纯功能行"]))
    if st["文件"]:
        A("- 明细：")
        for f, p, t in st["文件"]:
            A("  - `%s`　纯功能 %d / 总 %d" % (f, p, t))
    A("- 台账已交活：%s" % (("、".join("%s(%s)" % (i, w or "—") for i, _, w in done_list)) or "无"))
    A("- 队列总览：%s\n" % (json.dumps(by_status, ensure_ascii=False)))

    A("## 门禁\n")
    A("```bash")
    A("cd kernel && cargo test --lib svstar2      # 本域，应全绿零 error")
    A("cd kernel && cargo test --lib              # 全量，约 150 秒")
    A("```\n")
    A("基线：`svstar2` 32/32 绿；全量 9408 通过 / 2 失败。那 2 个是 `stareco` 既存缺陷")
    A("（报 `\"bad grant id\"`，`stareco/deep/f144e.rs:229`），已用 `git stash` 验证非新引入。\n")
    A("定位红项用 `CheckSet::red_items()`——仓库自带面，见 `vea01_index.rs` 的 `vea01_checks_all_green`。\n")

    A("## 循环步骤\n")
    A("```bash")
    A("cd VarixTaskOps")
    A("python ai_worker_loop.py --claim --worker AI-VE      # 领单（三道闸：全局锁/前置复核/施工锁）")
    A("python ai_worker_loop.py --done %s --result \"<摘要>\"" % task_id)
    A("python emit_next_prompt.py                           # 生成下一轮提示词")
    A("node ../_attic/2026-10-04-ve-产线/cdp/cdp_send_handoff.mjs   # 自动发到下一轮（需 9222 开放）")
    A("```\n")

    A("## 纪律（凌驾任务目标）\n")
    A("1. **源码只增不减不移动**——永不删除/覆盖/清空/gitignore 源码。推前必数文件数与行数，数不对不推")
    A("   （事故先例：`f9f4ef21` 单提交把 4153 个 `.rs` 从 main 整体抹掉）")
    A("2. **过程产物全归 `_attic/<日期-主题>/`**——测试脚本/日志/截图/临时探针，原位清零，")
    A("   归档是移动不是复制，`_attic/` 不入库")
    A("3. **只推纯功能代码**——测试脚本与测试截图一律不推 GitHub")
    A("4. **只add 显式路径**，绝不 `git add .`（`VTaskBoard/ui.html` 与 `vtaskboard.py` 有并行会话在途，别误算进 commit）")
    A("5. **异常零静默**——任何失败显性化（发生了什么/为什么/下一步怎么办），拒绝必带修正建议")
    A("6. **完成度铁律**——不半途交付、不留占位符、不写 TODO；交付前逐条核对原始需求")
    A("7. **修测试不迁就实现，修实现不迁就测试**——两者错都认，但要先归因是哪一边错")

    A("\n---\n")
    A("**已完成的单**（完成即解锁后继；无单可领时结束并汇报战果）\n")
    for tid, when, who in done_list:
        A("- %s　%s　%s" % (tid, when, who))
    if not done_list:
        A("- （无）")

    text = "\n".join(md) + "\n"
    meta = {
        "ok": True,
        "任务": task_id,
        "标题": title,
        "册": book,
        "域": dom_label,
        "行数目标": target,
        "落点": "kernel/varix/src/svstar2/%s.rs" % (mod or "veaNN_xxx"),
        "锚点是否抓到": not anchor.startswith("（锚点抽取失败"),
        "锚点长度": len(anchor),
        "队列": by_status,
        "svstar2": {"文件数": st["文件数"], "总行": st["总行"], "纯功能行": st["纯功能行"]},
        "已完成单数": len(done_list),
        "字数": len(text),
    }
    if probe_failed:
        # 探测领单退不回去 ⇒ 该单会卡在「已领」挡住后续 AI。必须显性化。
        meta["ok"] = False
        meta["错误"] = ("探测领单 %s 未成功退还，它现在卡在「已领」状态，"
                        "会挡住后续 AI 领单。请人工处理："
                        "python ai_worker_loop.py --release %s" % (task_id, task_id))
        meta["探测问题"] = probe_failed
    return meta, text



def main():
    task_id = None
    book = None
    do_print = False
    a = sys.argv[1:]
    i = 0
    while i < len(a):
        if a[i] == "--task" and i + 1 < len(a):
            task_id = a[i + 1]
            i += 2
        elif a[i] == "--book" and i + 1 < len(a):
            book = a[i + 1]
            i += 2
        elif a[i] == "--print":
            do_print = True
            i += 1
        else:
            i += 1

    res = build(task_id, book)
    if isinstance(res, tuple):
        meta, text = res
    else:
        # build() 早退（不可达 / 无单可领）——只回元数据
        print(json.dumps(res, ensure_ascii=False, indent=2))
        return 1

    if do_print:
        print(json.dumps(meta, ensure_ascii=False, indent=2))
        return 0

    with io.open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
        f.flush()
        os.fsync(f.fileno())          # 落盘即生效，避免下个对话读到半截

    meta["已写入"] = OUT
    print(json.dumps(meta, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
