# -*- coding: utf-8 -*-
"""vxlib —— Windows 域加固包核心库（WP-103 · MD2 篇 3.7 脚本工程纪律）。

从 _attic/vx-stability*.py、vx-vcruntime.py 收编升格。四条纪律在这里兑现：

1. 遗言机制（B-307）：每一步的上下文 + 任何异常写带时间戳的遗言日志后再
   退出，绝不无声死亡（v1 脚本因 U 盘瞬时掉线无声崩溃的教训）。
2. 装载纪律：hive 用 HiveMount 上下文管理器装载——正常路径与异常路径
   都保证 unload（v1 时代残留装载键导致"文件被占用"连环误案）。
3. 写后必读（B-303）：ensure_value 写后重读复核，输出前后对照；
   复核读数是判定的唯一标准，写完不读回等于没写。
4. 幂等可重跑（B-304）：ensure_value 对已就位的项跳过而非报错，
   重复执行无副作用（复检场景必然重跑）。

零文件操作原则（B-302，MD2 3.2）：四板斧本体只准碰注册表；文件操作
（复制/删除/改名）只存在于 harden_vcruntime（有清单+哈希的豁免件）。
审计由 run_all.py 的 selftest 静态扫描执行。

回滚：journal（JSONL）记录每次写动作的 before 值，rollback 反向恢复。
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys
import time
import traceback

# ---------------------------------------------------------------------------
# 遗言机制（B-307）
# ---------------------------------------------------------------------------

class LastWords:
    """带时间戳的遗言日志：正常流是逐条记录，异常时 excepthook 落全量
    traceback。文件每行即时 flush——掉电前的最后几行是排障的命根子。"""

    def __init__(self, path):
        self.path = path
        self.fh = open(path, "w", encoding="utf-8")  # noqa: SIM115 —— 报告文件即产物
        self.step = "(init)"
        self.closed = False
        sys.excepthook = self._hook

    def log(self, msg):
        line = "{} [{}] {}".format(_now(), self.step, msg)
        self.fh.write(line + "\n")
        self.fh.flush()
        return line

    def enter(self, step_name):
        self.step = step_name
        self.log(">> " + step_name)

    def _hook(self, t, v, tb):
        # 遗言本体：时间戳在每行头上，这里补全量异常栈。
        if self.closed:
            print("FATAL (log already closed) {}: {}".format(self.step, v),
                  file=sys.stderr)
            return
        try:
            self.fh.write("FATAL@{} {}: {}\n".format(
                _now(), self.step, "".join(traceback.format_exception(t, v, tb))))
            self.fh.write("=== LAST WORDS ABOVE — 修好后重跑；此日志供 VARIX 侧诊断收取 ===\n")
            self.fh.flush()
        except Exception:
            pass

    def close(self):
        if self.closed:
            return
        self.fh.write("=== CLEAN EXIT {} ===\n".format(_now()))
        self.fh.flush()
        self.fh.close()
        self.closed = True


def _now():
    return datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for blk in iter(lambda: f.read(1 << 20), b""):
            h.update(blk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# 注册表后端抽象：生产走 reg.exe，自测走内存 Fake
# ---------------------------------------------------------------------------

class RegBackend:
    """registry 操作的最小面。键/值一律以完整键字符串表达（HKLM\\X\\...）。"""

    def query(self, key, name):
        raise NotImplementedError

    def set_value(self, key, name, vtype, data):
        raise NotImplementedError

    def delete_value(self, key, name):
        raise NotImplementedError

    def subkeys(self, key):
        return []


class RegCliBackend(RegBackend):
    """reg.exe 封装（生产后端）。90s 超时：U 盘瞬时掉线时 reg 会挂死，
    超时把挂死变成可诊断的失败。"""

    TIMEOUT = 90

    def __init__(self, lw):
        self.lw = lw

    def _run(self, cmd):
        try:
            r = subprocess.run(cmd, capture_output=True, text=True,
                               timeout=self.TIMEOUT)
            return (r.returncode, (r.stdout or "") + (r.stderr or ""))
        except Exception as ex:
            return (124, "EXC {}: {}".format(type(ex).__name__, ex))

    def query(self, key, name):
        rc, out = self._run(["reg", "query", key, "/v", name])
        if rc != 0:
            return None
        for line in out.splitlines():
            # reg query 行形如 "    Name    REG_DWORD    0x0"；值本身可能含
            # 空格（REG_SZ 路径），按前三个空白段切分取余。
            if name in line and "REG_" in line:
                parts = line.split(None, 3)
                return parts[3] if len(parts) > 3 else ""
        return None

    def set_value(self, key, name, vtype, data):
        rc, out = self._run(["reg", "add", key, "/v", name, "/t", vtype,
                             "/d", str(data), "/f"])
        if rc != 0:
            raise RuntimeError("reg add failed: {} {} <- {}: {}".format(
                key, name, data, out.strip()[:120]))
        return True

    def delete_value(self, key, name):
        rc, _ = self._run(["reg", "delete", key, "/v", name, "/f"])
        return rc == 0

    def subkeys(self, key):
        rc, out = self._run(["reg", "query", key])
        if rc != 0:
            return []
        keys = []
        for line in out.splitlines():
            line = line.strip()
            if line.startswith(key.rstrip("\\") + "\\"):
                keys.append(line)
        return keys

    # hive 装载/卸载由 HiveMount 直接用 _run 调 reg load/unload。


class FakeRegBackend(RegBackend):
    """内存后端（selftest 专用）：值存 (vtype, data) 字典；支持两路注入——
    inject_value 直接改值（模拟 Windows 大更新"顺手拆家"），
    fail_on 让下一次指定操作抛异常（模拟瞬时掉线，验遗言）。"""

    def __init__(self):
        self.store = {}          # (key, name) -> (vtype, data)
        self.subkey_set = set()  # key 字符串集合（含隐式父键）
        self.fail_on = None      # ("set"/"query", 片段) -> 下次命中即抛
        self.ops = []            # 操作流水（审计用）

    def _check_fail(self, op, key):
        if self.fail_on and self.fail_on[0] == op and self.fail_on[1] in key:
            self.fail_on = None
            raise OSError("injected fault on {} {}".format(op, key))

    def _touch(self, key):
        # 键与全部父键进入集合，subkeys() 才能枚举。
        parts = key.split("\\")
        for i in range(1, len(parts) + 1):
            self.subkey_set.add("\\".join(parts[:i]))

    def query(self, key, name):
        self._check_fail("query", key)
        self.ops.append(("query", key, name))
        hit = self.store.get((key, name))
        return hit[1] if hit else None

    def set_value(self, key, name, vtype, data):
        self._check_fail("set", key)
        self.ops.append(("set", key, name, str(data)))
        self._touch(key)
        self.store[(key, name)] = (vtype, str(data))

    def delete_value(self, key, name):
        self.ops.append(("delete", key, name))
        self.store.pop((key, name), None)

    def subkeys(self, key):
        prefix = key.rstrip("\\") + "\\"
        out = set()
        for k in self.subkey_set:
            if k.startswith(prefix) and "\\" not in k[len(prefix):]:
                out.add(k)
        return sorted(out)

    def inject_value(self, key, name, data):
        """模拟外部篡改（大更新/优化工具）：直接改值不 journal。"""
        self.store[(key, name)] = ("REG_DWORD", str(data))


class HiveMount:
    """hive 装载纪律的兑现：load → yield → finally unload（三次重试 +
    查询确认）。异常路径也卸载——装载键残留是事故源，不是小瑕疵。

    Fake 后端下 load/unload 是空操作（键名直通），保证自测同一条代码路。"""

    def __init__(self, backend, mount_root, lw, cli=None):
        self.backend = backend
        self.root = mount_root            # 如 HKLM\VXHARD
        self.lw = lw
        self.cli = cli or getattr(backend, "cli", None)
        self._loaded = False

    def load(self, hive_path):
        if isinstance(self.backend, FakeRegBackend):
            self._loaded = True
            self.lw.log("fake mount {} <- {}".format(self.root, hive_path))
            return self.root
        rc, out = RegCliBackend(self.lw)._run(["reg", "load", self.root, hive_path])
        ok = rc == 0
        self.lw.log("reg load {} <- {}: {}".format(
            self.root, hive_path, (out.strip()[:80] or "rc={}".format(rc))))
        if not ok:
            raise RuntimeError("reg load failed: " + out.strip()[:120])
        self._loaded = True
        return self.root

    def unload(self):
        if not self._loaded:
            return True
        if isinstance(self.backend, FakeRegBackend):
            self._loaded = False
            self.lw.log("fake unmount " + self.root)
            return True
        rc = RegCliBackend(self.lw)
        for attempt in range(1, 4):
            _, out = rc._run(["reg", "unload", self.root])
            self.lw.log("unload 尝试{}: {}".format(attempt, out.strip()[:80]))
            _, still = rc._run(["reg", "query", self.root])
            if "ERROR" in still.upper() or "错误" in still:
                self._loaded = False
                self.lw.log("hive 已卸载（查询确认不存在）")
                return True
            time.sleep(2)
        self.lw.log("WARN: unload 三次未确认成功——残留装载键需人工核查")
        return False

    def __enter__(self):
        return self

    def __exit__(self, t, v, tb):
        # 异常路径也卸载（装载纪律第三条：用完必卸，含异常）。
        self.unload()
        return False


# ---------------------------------------------------------------------------
# 幂等 + 写后必读 + journal（回滚账本）
# ---------------------------------------------------------------------------

class Journal:
    """JSONL 账本：每行一次写动作的 (key, name, vtype, before, after)。
    rollback 反向恢复：after 现值若仍等于本次写入值则还原 before；
    before 为 None（新建值）则删除。"""

    def __init__(self, path):
        self.path = path
        self.rows = []

    def add(self, key, name, vtype, before, after):
        self.rows.append({"key": key, "name": name, "type": vtype,
                          "before": before, "after": str(after)})

    def flush(self):
        with open(self.path, "w", encoding="utf-8") as f:
            for row in self.rows:
                f.write(json.dumps(row, ensure_ascii=False) + "\n")

    def rollback(self, backend, lw):
        ok = 0
        for row in reversed(self.rows):
            key, name = row["key"], row["name"]
            if row["before"] is None:
                backend.delete_value(key, name)
                lw.log("rollback: {}\\{} 删除（原不存在）".format(key, name))
            else:
                backend.set_value(key, name, row["type"] or "REG_DWORD",
                                  row["before"])
                now = backend.query(key, name)
                if now != row["before"]:
                    lw.log("rollback MISMATCH: {}\\{} -> {}".format(key, name, now))
                else:
                    ok += 1
                    lw.log("rollback: {}\\{} 还原为 {}".format(key, name, row["before"]))
        return ok


def value_match(vtype, raw, want):
    """跨后端的值匹配口径（单一事实源，recheck 与 selftest 共用）：
    - REG_DWORD：CLI 后端读回形如 "0x50"、Fake 后端存 "80"——按 int(x, 0)
      归一后比较，两边口径差异在这里吸收；
    - 其余（REG_SZ / REG_EXPAND_SZ）：strip 后字符串直比。"""
    if raw is None:
        return False
    if (vtype or "").upper() == "REG_DWORD":
        try:
            return int(str(raw), 0) == int(str(want), 0)
        except ValueError:
            return False
    return str(raw).strip() == str(want).strip()


def ensure_value(backend, lw, journal, key, name, vtype, want,
                 want_text=None, skip_verify=False):
    """幂等写 + 写后必读的单步原语。

    返回 (status, before, after)：status ∈ {ok, skip, fail}。
    - 读旧值；已等于期望 → skip（幂等：复检重跑不重复写）；
    - 否则写 → 立即重读 → 读回值 != 期望 → fail（写后必读红线）。
    - before 记入 journal（回滚账本）；skip 不记（无副作用即无账）。"""

    before = backend.query(key, name)
    display = want_text if want_text is not None else want
    if value_match(vtype, before, want):
        lw.log("  SKIP {}\\{} 已在位 = {}".format(key, name, before))
        return ("skip", before, before)
    backend.set_value(key, name, vtype, want)
    after = backend.query(key, name)
    if not skip_verify and not value_match(vtype, after, want):
        lw.log("  FAIL {}\\{} 写后读回 {} != 期望 {}".format(
            key, name, after, display))
        journal.add(key, name, vtype, before, after)
        return ("fail", before, after)
    lw.log("  OK {}\\{}: {} -> {}".format(key, name, before, after))
    journal.add(key, name, vtype, before, after)
    return ("ok", before, after)
