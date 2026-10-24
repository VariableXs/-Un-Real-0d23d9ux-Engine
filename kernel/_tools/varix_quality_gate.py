#!/usr/bin/env python3
"""Varix 内核质量门禁（AI-3 · 质量门禁官交付物）

锚点：docs/Varix/7AI分工总案.md 第二节 AI-3 任务书 · 第三节铁律 1/6
对应正本记忆：D:\\2\\14\\Un real memory\\MEMORY.md

本脚本只做「静态文本 + 编译门禁」两件事，**不修改任何源码**，全部只读。
退出码 0 表示全绿；非 0 表示有硬门禁未过。

四道门（G1~G4）：

  G1  内核编译门禁  cargo kcheck（--target x86_64-unknown-none --features kernel-image）
  G1b 宿主构建门禁  cargo check --workspace --all-targets（判据侧，与内核路径分离）
  G2  panic 面     内核路径（实现层，非判据层）unwrap/expect/panic!/todo!/unimplemented! 计数
  G3  no_std 三件套  宏 `use alloc::vec;` / 类型 `use alloc::vec::Vec;` /
                   `use alloc::string::{String, ToString};` 缺项检测
  G4  编译图纯净度  src/ 下混入的「非内核源码树」（自带 Cargo.toml）检测

★ G1 与 G1b 必须分列的理由（AI-3 实测）：
  项目 lib.rs 是 `#![cfg_attr(not(test), no_std)]`，宿主测试构建走 std 路径，
  **看不到**真内核 target 的编译错误。实测两者曾严重背离：
  宿主 all-targets = 0 error，内核 kcheck = 317 errors（E0004 等）。
  只跑宿主构建 ⇒ 371 万行代码的核心交付物（内核 ELF）零校验。
  故本门禁默认以 G1（kcheck）为准，宿主口径单列 G1b 供参考。

用法：
    python varix_quality_gate.py                 # 全跑（G1+G1b+G2~G4）
    python varix_quality_gate.py --no-host        # 只跑内核 kcheck + 静态门禁
    python varix_quality_gate.py --no-cargo       # 只跑 G2~G4（秒级，可高频复跑）
    python varix_quality_gate.py --json          # 机读输出
    python varix_quality_gate.py --baseline <f> # 与基线文件对比，只报增量

设计约束（遵守总案第四节「共享文件只 append」与 Karpathy「Surgical Changes」）：
  - 本文件是**新增**独立文件，不修改任何既有源码。
  - 全部判定为只读扫描，零副作用，可随时复跑。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

if sys.platform == "win32":
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

# --------------------------------------------------------------------------
# 项目常量
# --------------------------------------------------------------------------

# 判据层文件特征：判据是测试性质的，unwrap/expect 在其中属可议，需人工裁决而非硬门禁
CHECK_FILE_RE = re.compile(r"(_checks\.rs$|_test\.rs$|^tests/|(^|/)robust\.rs$|(^|/)checks\.rs$)")

# 内核 panic 面模式：铁律 1「内核路径禁 unwrap/expect」
# 2026-10-23 纠偏：旧式裸名匹配 `(unwrap|expect)\s*\(` 方向反了——
#   ① Rust 中 Option/Result 的 unwrap/expect 只能以方法（`.unwrap(`）或限定路径
#      （`Option::expect(`）调用；裸名 `expect(` 必然是本文件自由函数/参数调用，
#      与 panic 面无关（实例：unx0490_rndpat.rs 的 `expect: impl Fn` 参数被误判）。
#   ② 旧正则负向前瞻排除 `.` 前缀，反而把最高频的违规形态 `.unwrap(`/`.expect(`
#      全部漏掉（实测漏报 9,921 处 / 非判据实现层 3,198 处）。
# 现按真实 panic 能力匹配：方法形态 + 限定路径形态 + 四个硬宏。
PANIC_RE = re.compile(
    r"\.\s*unwrap\s*\("              # 方法形态：x.unwrap(（无参形态必为 Option/Result 或同名无参自定义）
    r"|\.\s*expect\s*\(\s*\""        # 标准 expect：Result::expect 必带字符串消息；
    r"|::(?:unwrap|expect)\s*\("     # 限定路径：Option::unwrap( / Result::expect(
    r"|panic!\s*\(|todo!\s*\(|unimplemented!\s*\("
)
# 注：解析器型自定义 `expect(term, level, sink)` 首参非字符串字面量（vec21_parser
# 实测 31 处 self.expect(Term::…)），与 Result::expect 签名区分，不计 panic 面。

# G2 显式豁免标记：设计意图型 panic（表级缺陷守门等 fail-fast 场景），
# 在命中行或其紧邻上一行注释里写 G2-ALLOW 并说明理由，则不计入 panic 面。
# 与整文件白名单相对：标记跟着命中行走，文件重构后标记不会错位。
G2_ALLOW_RE = re.compile(r"G2-ALLOW")

# 定义侧误报：`fn unwrap(` / `fn expect(` / `pub fn unwrap(` 等自造方法名
DEF_RE = re.compile(
    r"\bfn\s+(unwrap|expect)\s*[\(<]|"
    r"\b(fn|let|const)\s+\w+.*\b(unwrap|expect)\s*(::|<)"
)

# no_std 三件套（铁律 6）：alloc 根下没有 String/Vec/ToString 这三个名字，
# 必须从 alloc 子模块分别引入；且 `vec` 宏与 `Vec` 类型是两套东西。
#
# 导入形态实测有 5 类（`grep "^use alloc::" | sort | uniq -c` 全量普查结果）：
#   use alloc::vec;                                    456  （宏）
#   use alloc::vec::Vec;                               2771  （类型）
#   use alloc::string::String;                         2004  （单名）
#   use alloc::string::{String, ToString};              530  （组合）
#   use alloc::string::ToString;                         76  （单名）
#   use alloc::vec::{self, Vec};                          2  （带 self）
#   use alloc::{vec, vec::Vec, string::{String, ToString}, format};  12+13+9+5（多行嵌套）
# 只认单行形态会把 2004+76 处单名导入与 39 处多行嵌套全判成缺项 ⇒ 巨量误报。
# 故统一走「展平 use 声明」：把每个 use 语句（含跨行）抽出正文再做子路径匹配。
# 2026-10-22 晚修正：lib.rs 为 `pub extern crate alloc;`，`use crate::alloc::...`
# 与 `use alloc::...` 同为合法绑定（实测 400+ 文件用前缀形态），旧正则漏认
# 导致整批误报为缺项。归一化时剥掉 `crate::` 前缀，两种形态等价。
USE_STMT_RE = re.compile(r"\buse\s+((?:crate::)?alloc(?::[^;]*?)?)\s*;", re.S)


def _collect_alloc_uses(text: str) -> str:
    """抽出全文所有 `use alloc::...;` 声明并**展开花括号**，返回归一化的路径集合。

    必须展开花括号：`use alloc::string::{String, ToString};` 展平后应同时含
    `alloc::string::String` 与 `alloc::string::ToString`，否则组合形态
    （实测 530 处）会被判成缺项。同样 `use alloc::{vec, vec::Vec, format};`
    需拆成三个独立路径。
    """
    paths: list[str] = []
    for m in USE_STMT_RE.finditer(text):
        body = re.sub(r"\s+", "", m.group(1))
        if body.startswith("crate::"):
            body = body[len("crate::"):]
        paths.extend(_expand_use_path(body))
    return " ".join(paths)


def _expand_use_path(body: str) -> list[str]:
    """把 `a::{b::{c,d}, e}` 递归展开为 ['a::b::c','a::b::d','a::e']。

    实现要点：每次递归都必须消费一层花括号，故以 `_consume_brace` 返回的
    (prefix, inner, rest) 驱动，避免「已展开路径仍含 `::{`」造成的死循环。
    """
    if "::{" not in body:
        return [body] if body and not body.endswith("}") else []

    parsed = _consume_brace(body)
    if parsed is None:
        return [body]
    prefix, inner = parsed

    items = _split_top_level(inner)
    out: list[str] = []
    for it in items:
        it = it.strip()
        if not it:
            continue
        if it == "self":
            # `use alloc::vec::{self, Vec};` —— self 让 vec 本身可达
            out.append(prefix)
            continue
        out.extend(_expand_use_path(f"{prefix}::{it}"))
    return out


def _consume_brace(body: str) -> tuple[str, str] | None:
    """取 body 中第一个 `::{...}`，返回 (前缀, 花括号内内容)。"""
    idx = body.find("::{")
    if idx == -1:
        return None
    prefix = body[:idx]
    start = idx + 3  # 跳过 `::{`
    depth = 1
    i = start
    while i < len(body):
        ch = body[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return prefix, body[start:i]
        i += 1
    return None


def _split_top_level(inner: str) -> list[str]:
    """按顶层逗号切分（嵌套花括号内的逗号不切）。"""
    items: list[str] = []
    depth = 0
    cur: list[str] = []
    for ch in inner:
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        if ch == "," and depth == 0:
            items.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    if cur:
        items.append("".join(cur))
    return items


def _alloc_has(alloc_uses: str, path: str, name: str | None = None) -> bool:
    """判断展平后的 alloc 导入集合里是否存在指定路径。"""
    target = f"{path}::{name}" if name else path
    return target in alloc_uses.split()


NO_STD_MACRO_VEC_KEY = "alloc::vec"
NO_STD_TYPE_VEC_KEY = "alloc::vec::Vec"
NO_STD_STRING_KEY = "alloc::string::String"
NO_STD_TOSTRING_KEY = "alloc::string::ToString"

# 判定「一个 .rs 文件是否用到 vec! / Vec / String / format! / to_string()」
USE_VEC_MACRO = re.compile(r"(?<![\w:])vec!\s*\[")
USE_VEC_TYPE = re.compile(r"(?<![\w:])Vec\s*<")
USE_STRING_TYPE = re.compile(r"(?<![\w:])String\b")
USE_STRING_MACRO = re.compile(r"(?<![\w:])format!\s*\(")
# `.to_string()` 方法调用同样依赖 alloc::string::ToString trait
# （usize 等数值类型无固有 to_string；cgb463 实证盲区）
USE_TOSTRING_METHOD = re.compile(r"\.to_string\s*\(")

# 第三方源码树特征：src/ 下出现自带 Cargo.toml 的目录 = 未受内核编译图管辖的
# 外来工程（内核为单一 workspace 成员 varix，任何嵌套 Cargo.toml 都是污染）
NESTED_CARGO_TOML = "Cargo.toml"


def repo_root() -> Path:
    """定位 kernel/ 目录（本脚本位于 kernel/_tools/）。"""
    return Path(__file__).resolve().parent.parent


def iter_rs_files(src: Path):
    """遍历 src 下所有 .rs（排除第三方源码树，见 G4）。"""
    for rs in sorted(src.rglob("*.rs")):
        yield rs


def third_party_roots(src: Path) -> list[Path]:
    """找出 src/ 下自带 Cargo.toml 的外来工程根目录。

    注意：不能只看 src/ 的第一层子目录——外来工程可能嵌在更深处
    （实测 `src/svstar2/zz_src-tauri/` 自带 Cargo.toml + tauri.conf.json，
    是 src/ 下第二层的 Tauri 桌面应用源码树）。故此处整棵 rglob。
    """
    roots: list[Path] = []
    if not src.is_dir():
        return roots
    for nested in sorted(src.rglob(NESTED_CARGO_TOML)):
        parent = nested.parent
        # kernel/ 自身是 workspace 根，其 Cargo.toml 不算外来工程
        if parent == src.parent.parent:
            continue
        # 取最外层命中：若父目录已在 roots 内则跳过（避免嵌套重复计数）
        if not any(parent == r or _is_under(parent, r) for r in roots):
            roots.append(parent)
    return roots


def _is_under(path: Path, ancestor: Path) -> bool:
    try:
        path.relative_to(ancestor)
        return True
    except ValueError:
        return False


def _strip_comments(line: str, in_block: bool) -> str:
    """剥离行内注释与块注释，返回纯代码部分；in_block 为传入的块注释状态。"""
    out = []
    i = 0
    n = len(line)
    while i < n:
        if in_block:
            end = line.find("*/", i)
            if end == -1:
                return "".join(out), True  # 信号：仍在块注释中
            in_block = False
            i = end + 2
            continue
        if line.startswith("//", i):
            break
        if line.startswith("/*", i):
            in_block = True
            i += 2
            continue
        out.append(line[i])
        i += 1
    return "".join(out), in_block


def _strip_comments_and_strings(line: str, in_block: bool):
    """G2 专用：剥离注释**与字符串/char 字面量内容**，返回纯代码骨架。

    为什么 G2 不能只剥注释：判据数据表里大量字符串含 "unwrap("、"panic!()"
    等文本（如 cga147_secfin.rs:316 `("unwrap(", 1),`），PANIC_RE 会在
    字符串字面量里误命中 ⇒ 造出假缺陷。字符串里的 // 与 /* 也必须留在
    字符串态内处理，否则 `"http://x"` 会被误当行注释截断后半行代码。
    """
    out = []
    i = 0
    n = len(line)
    while i < n:
        if in_block:
            end = line.find("*/", i)
            if end == -1:
                return "".join(out), True
            in_block = False
            i = end + 2
            continue
        if line.startswith("//", i):
            break
        if line.startswith("/*", i):
            in_block = True
            i += 2
            continue
        ch = line[i]
        if ch == '"':
            # 双引号字符串：跳到非转义的收口引号，内容不进入输出
            j = i + 1
            while j < n:
                if line[j] == "\\":
                    j += 2
                    continue
                if line[j] == '"':
                    break
                j += 1
            i = min(j + 1, n)
            out.append('""')  # 保留字符串占位，维持 token 边界
            continue
        if ch == "'":
            # char 字面量 'x' / '\n' / '\u{...}' / '\'}' vs 生命周期 'a —— 判别：
            # 生命周期撇号后必跟标识符字符（[A-Za-z0-9_]）；非标识符字符跟随
            # （'}'、'('、' ' 等）必为 char 字面量。
            # 2026-10-23 修复：旧启发式只看「两字符内能否闭合」，'}' 这类
            # 非闭合形态被误判为生命周期 ⇒ 字面量里的花括号泄进深度计算
            # （snap.rs 实测：rfind('}') 一处即令整个 #[cfg(test)] 域提前
            # 退出，9 条测试行假性落进内核桶）。
            nxt = line[i + 1] if i + 1 < n else ""
            if nxt.isalnum() or nxt == "_":
                # 标识符跟随：两字符内能闭合的是单字符 char（'x' / 转义），否则是生命周期
                if i + 2 < n and line[i + 2] == "'":
                    i = i + 3
                    out.append("''")
                    continue
                if line[i + 1] == "\\" and i + 3 < n and line[i + 3] == "'":
                    i = i + 4
                    out.append("''")
                    continue
                out.append(ch)          # 生命周期撇号，仅输出撇号本身
                i += 1
                continue
            # 非标识符跟随：必为 char 字面量，扫描到收口引号（处理转义）
            j = i + 1
            while j < n:
                if line[j] == "\\":
                    j += 2
                    continue
                if line[j] == "'":
                    break
                j += 1
            if j < n:
                out.append("''")
                i = j + 1
                continue
            out.append(ch)
            i += 1
            continue
        out.append(ch)
        i += 1
    return "".join(out), in_block


def strip_test_scopes(lines: list[str]) -> list[bool]:
    """标出每一行是否位于 `#[cfg(test)]` 作用域内。

    原理：lib.rs 是 `#![cfg_attr(not(test), no_std)]` —— 测试构建下 std prelude
    自动生效，`Vec`/`String`/`vec!` 解析**不需要** `use alloc::`；
    只有真内核构建（--features kernel-image）才强制要求显式引入。
    因此三件套检测必须排除 `#[cfg(test)]` 作用域内的用法，否则整册误报
    （实测误报率约 55%：1055 条里绝大多数位于 #[cfg(test)] 函数内）。

    实现：扫描到 `#[cfg(test)]`（或 `#[cfg(all(test, ...))]`）时记录当前
    花括号深度，直到该深度闭合为止，该区间内的行标记为测试作用域。
    """
    in_test: list[bool] = []
    depth = 0
    test_depth: int | None = None
    pending_cfg_test = False
    pending_age = 0          # pending 已等待的行数，防止无花括号时无限等待
    in_block = False

    for raw in lines:
        # 2026-10-23 修复：花括号计量必须走字符串/注释剥离器。
        # 旧实现只 split("//") 剥注释不剥字符串 ⇒ 字符串里的不平衡花括号
        # （format!("...}}") 转义闭括号、"{"、rfind('}') 等）全部泄进深度，
        # 实测 snap.rs:1234 一行 format 串令 #[cfg(test)] 域提前退出，
        # 域内 9 条测试行假性落进内核桶。
        code, in_block = _strip_comments_and_strings(raw, in_block)
        stripped = code.strip()

        # 本行是否携带 #[cfg(test)] / #[cfg(all(test, ...))] 标记
        opens_cfg = bool(
            re.match(r"#\[cfg\(\s*test\s*\)\s*\]", stripped)
            or re.match(r"#\[cfg\(\s*all\(\s*test\b", stripped)
        )
        # 2026-10-23 二补：裸 `#[test]` 函数（不在 cfg(test) mod 内）同样是
        # 宿主侧测试作用域（std 可用），实测 mech_zram.rs:612 裸 #[test]
        # 4 行断言落进内核桶。与 cfg(test) 同机制落地。
        opens_bare_test = bool(re.match(r"#\[test\]\s*$", stripped))

        # 本行所属作用域 = 进入本行之前的状态
        in_test.append(test_depth is not None)

        opens = code.count("{")
        closes = code.count("}")
        depth += opens - closes

        if (opens_cfg or opens_bare_test) and test_depth is None:
            # 防嵌套重落地：已在测试作用域内时不得更新 test_depth，
            # 否则内层 fn 闭合时会把外层 mod tests 一并提前退出。
            pending_cfg_test = True
            pending_age = 0

        # 关键：`#[cfg(test)]` 与 `{` 常分处两行（实测 a11y.rs:927/928），
        # 因此 pending 必须跨行保持，直到遇到开启用的 `{` 才落地；
        # 但若 5 行内没有 `{`，判定为无效标记并放弃（防跑飞）。
        if pending_cfg_test:
            pending_age += 1
            if opens > 0:
                test_depth = depth
                pending_cfg_test = False
                # 2026-10-23 修复：落地行自身也属测试作用域。
                # 实测坑：`#[cfg(test)]` 下一行是 `pub fn xxx(...) {` 单函数形态时，
                # 签名行的 `{` 若计入深度而函数体闭 `}` 被跳过 ⇒ 每处 +1 永久漂移
                # （mech_inflate.rs 实测漂移 +2，导致后续所有 fn 全被吞进前一个
                # 函数作用域，内嵌判据函数识别与 panic 面分桶双双失真）。
                in_test[-1] = True
            elif pending_age > 5 or closes > 0:
                pending_cfg_test = False

        # 退出测试作用域：花括号深度回落
        if test_depth is not None and depth < test_depth:
            test_depth = None

    return in_test


def is_third_party(path: Path, tp_roots: list[Path]) -> bool:
    for tp in tp_roots:
        try:
            path.relative_to(tp)
            return True
        except ValueError:
            continue
    return False


# --------------------------------------------------------------------------
# G1 编译门禁
# --------------------------------------------------------------------------


def gate_cargo_check(root: Path, timeout: int = 5400):
    """G1 编译门禁。

    **必须走项目官方 alias `cargo kcheck`**，即
    `cargo check --target x86_64-unknown-none --features kernel-image`。

    原因（AI-3 实测结论）：宿主侧 `cargo check --workspace --all-targets`
    走的是 `#![cfg_attr(not(test), no_std)]` 的 **std 宿主路径**，它**看不到**
    真内核 target 下的编译错误——实测两套构建结果完全背离：

      宿主构建（all-targets）：0 error / 9200 warnings
      内核构建（kcheck）     ：曾报 317 errors（E0004 等）

    换言之：`cargo test` 全绿 ≠ 内核镜像能编译。若门禁只跑宿主构建，
    等于对 371 万行内核代码的核心交付物（内核 ELF）不做任何校验。
    故本门禁默认以 kcheck 为准；宿主测试构建作为 G1b 单列。
    """
    cmd = ["cargo", "kcheck", "--message-format=short"]
    try:
        proc = subprocess.run(
            cmd, cwd=root, capture_output=True, text=True,
            encoding="utf-8", errors="replace", timeout=timeout,
        )
    except FileNotFoundError:
        return {"gate": "G1", "ok": False, "fatal": "cargo not found in PATH"}
    except subprocess.TimeoutExpired:
        return {"gate": "G1", "ok": False, "fatal": f"cargo kcheck 超时（>{timeout}s）"}

    out = (proc.stdout or "") + (proc.stderr or "")
    errors = [l for l in out.splitlines() if ": error" in l]
    warnings = [l for l in out.splitlines() if ": warning" in l]
    summary = re.findall(r"generated (\d+) warnings?", out)
    total_warn = sum(int(a) for a in summary) if summary else len(warnings)

    # 错误码与涉及文件分布（归因用）
    by_code: dict[str, int] = {}
    by_file: dict[str, int] = {}
    for line in errors:
        m = re.search(r"(E\d{4})", line)
        if m:
            by_code[m.group(1)] = by_code.get(m.group(1), 0) + 1
        f = line.split(":")[0].strip()
        if f:
            by_file[f] = by_file.get(f, 0) + 1

    return {
        "gate": "G1",
        "name": "内核编译门禁 cargo kcheck（--target x86_64-unknown-none --features kernel-image）",
        "cmd": " ".join(cmd),
        "ok": proc.returncode == 0 and not errors,
        "exit_code": proc.returncode,
        "error_count": len(errors),
        "warning_total": total_warn,
        "errors_by_code": dict(sorted(by_code.items(), key=lambda kv: -kv[1])),
        "errors_by_file": dict(
            sorted(by_file.items(), key=lambda kv: -kv[1])[:20]
        ),
        "errors_sample": errors[:20],
        "errors_all": errors,
    }


def gate_host_build(root: Path, timeout: int = 5400):
    """G1b 宿主测试构建门禁（判据侧质量，与内核镜像路径分离）。

    单独成一门的理由：两套构建的错误/警告口径完全不同（宿主侧走 std），
    混在一起会让人误以为「宿主绿 = 内核绿」。
    """
    cmd = [
        "cargo", "check", "--workspace", "--all-targets", "--message-format=short",
    ]
    try:
        proc = subprocess.run(
            cmd, cwd=root, capture_output=True, text=True,
            encoding="utf-8", errors="replace", timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return {"gate": "G1b", "ok": False, "fatal": f"宿主 check 超时（>{timeout}s）"}

    out = (proc.stdout or "") + (proc.stderr or "")
    errors = [l for l in out.splitlines() if ": error" in l]
    summary = re.findall(r"generated (\d+) warnings?", out)
    total_warn = sum(int(a) for a in summary) if summary else 0
    return {
        "gate": "G1b",
        "name": "宿主测试构建 cargo check --workspace --all-targets（判据侧，非内核镜像）",
        "cmd": " ".join(cmd),
        "ok": proc.returncode == 0 and not errors,
        "error_count": len(errors),
        "warning_total": total_warn,
        "errors_sample": errors[:10],
        "errors_all": errors,
    }


# --------------------------------------------------------------------------
# G2 panic 面
# --------------------------------------------------------------------------

# 内嵌判据函数识别：大量判据体不住在 *_checks.rs 里，而是内嵌在实现文件的
# `fn run_xxx_checks()` / `fn checks_xxx(cs: &mut CheckSet)` / 签名含 CheckSet
# 的函数中（实测 vea53_hotplug 的 checks_slots/checks_contract、aurora/window.rs
# 的 run_window_checks 等）。这类函数体内的 unwrap/expect 属判据性质（可议、
# 人工裁决），与实现层 panic 面分开记账。
FN_HEAD_RE = re.compile(r"(?:pub\s+)?(?:unsafe\s+)?(?:const\s+)?fn\s+(\w+)")
EMBED_CHECK_NAME_RE = re.compile(r"^(?:run_\w+_checks|checks?_\w+|\w+_checks)$")
CHECKSIG_RE = re.compile(r"\bCheckSet\b")


def _mark_embedded_check_fns(lines: list[str], in_test: list[bool]) -> list[bool]:
    """返回与 lines 等长的标记数组：该行是否处于内嵌判据函数体内。

    花括号深度追踪（与判据同源的剥注释/字符串管线），函数头允许跨行；
    函数为判据的判据：函数名匹配 EMBED_CHECK_NAME_RE 或头部签名含 CheckSet。
    """
    mark = [False] * len(lines)
    in_block = False
    depth = 0
    cur = None        # [is_check, depth_at_open]
    pending = None    # [is_check, depth_at_fn, header_buf, header_closed]
    for i, line in enumerate(lines):
        if in_test[i]:
            mark[i] = bool(cur and cur[0])
            continue
        code, in_block = _strip_comments_and_strings(line, in_block)
        if cur is not None:
            mark[i] = cur[0]
        if not code.strip():
            continue
        if pending is not None:
            pending[2] += " " + code.strip()
            if "{" in code:
                pending[3] = True
                cur = [pending[0], pending[1]]
                pending = None
        elif cur is None:
            m = FN_HEAD_RE.search(code)
            if m:
                hdr = code[code.index("("):] if "(" in code else code
                is_check = bool(
                    EMBED_CHECK_NAME_RE.match(m.group(1)) or CHECKSIG_RE.search(hdr)
                )
                if "{" in code:
                    cur = [is_check, depth]
                else:
                    pending = [is_check, depth, code, False]
        opens = code.count("{") - code.count("}")
        if opens != 0:
            depth += opens
            while cur is not None and depth <= cur[1]:
                cur = None
    return mark


def gate_panic_surface(src: Path, tp_roots: list[Path]):
    kernel_hits = []   # 实现层：硬门禁
    check_hits = []    # 判据层（含内嵌判据函数）：需人工裁决，登记不阻断
    test_hits = []     # #[cfg(test)] 内：宿主侧有 std，判据性质，与内核镜像无关
    per_file = {}

    for rs in iter_rs_files(src):
        if is_third_party(rs, tp_roots):
            continue
        try:
            text = rs.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        lines = text.splitlines()
        in_test = strip_test_scopes(lines)
        in_check_fn = _mark_embedded_check_fns(lines, in_test)
        is_check = bool(CHECK_FILE_RE.search(rs.as_posix()))
        hits = 0
        allows = 0
        in_block = False
        prev_raw = ""
        for lineno, line in enumerate(lines):
            if in_test[lineno]:
                prev_raw = line
                continue
            code, in_block = _strip_comments_and_strings(line, in_block)
            if not code.strip():
                prev_raw = line
                continue
            if PANIC_RE.search(code):
                # 方法定义侧（fn unwrap( / fn expect( ）不算 panic 面
                if DEF_RE.search(code):
                    prev_raw = line
                    continue
                # G2-ALLOW 豁免：同行或紧邻上一行注释里的显式标记（设计意图型 fail-fast）
                if G2_ALLOW_RE.search(line) or G2_ALLOW_RE.search(prev_raw):
                    allows += 1
                    prev_raw = line
                    continue
                hits += 1
                item = (
                    f"{rs.relative_to(src.parent).as_posix()}:{lineno}: {line.strip()[:110]}"
                )
                if is_check or in_check_fn[lineno]:
                    check_hits.append(item)
                else:
                    kernel_hits.append(item)
                    per_file[rs.relative_to(src.parent).as_posix()] = (
                        per_file.get(rs.relative_to(src.parent).as_posix(), 0) + 1
                    )
            prev_raw = line

    top = sorted(per_file.items(), key=lambda kv: -kv[1])[:15]
    return {
        "gate": "G2",
        "name": "panic 面（铁律 1：内核路径禁 unwrap/expect/panic!/todo!/unimplemented!；G2-ALLOW 标记豁免）",
        "ok": len(kernel_hits) == 0,
        "kernel_path_hits": len(kernel_hits),
        "check_path_hits": len(check_hits),
        "allow_hits": allows,
        "test_scope_hits": len(test_hits),
        "kernel_sample": kernel_hits[:20],
        "check_sample": check_hits[:10],
        "test_sample": test_hits[:5],
        "top_files": top,
    }


# --------------------------------------------------------------------------
# G3 no_std 三件套
# --------------------------------------------------------------------------


def gate_no_std_trio(src: Path, tp_roots: list[Path]):
    missing = []
    checked = 0

    for rs in iter_rs_files(src):
        if is_third_party(rs, tp_roots):
            continue
        try:
            text = rs.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue

        lines = text.splitlines()
        in_test = strip_test_scopes(lines)

        # 只看**内核构建路径**上的用法，并剔除注释。
        # 注释里提到类型名是本项目常态且往往是「零堆纪律」这类**禁令声明**
        # （实测 a11ygate.rs:20 `//! 零堆纪律：无 Vec/String/Box/format!`，
        #  正则跨行命中注释里的 `String` ⇒ 误报成缺项）。
        # 故先逐行剥 `//` 与 `/* */`，再判是否真的用了这些名字。
        stripped_lines = []
        in_block = False
        for ln, is_t in zip(lines, in_test):
            if is_t:
                stripped_lines.append("")
                continue
            code_only, in_block = _strip_comments(ln, in_block)
            stripped_lines.append(code_only)

        kernel_text = "\n".join(stripped_lines)
        if not kernel_text.strip():
            continue

        need_macro = bool(USE_VEC_MACRO.search(kernel_text))
        need_type = bool(USE_VEC_TYPE.search(kernel_text))
        need_string = bool(
            USE_STRING_TYPE.search(kernel_text)
            or USE_STRING_MACRO.search(kernel_text)
            or USE_TOSTRING_METHOD.search(kernel_text)
        )
        if not (need_macro or need_type or need_string):
            continue

        # 若文件整体走 std（罕见但存在，如 extern crate std 的宿主侧代码），跳过
        if re.search(r"^\s*(pub\s+)?use\s+std::", text, re.M):
            continue

        checked += 1
        alloc_set = _collect_alloc_uses(text).split()
        lacks = []
        # vec! 宏与 Vec 类型是两个名字，各自独立判定
        if need_macro and "alloc::vec" not in alloc_set:
            lacks.append("vec!(宏) 缺 `use alloc::vec;`")
        if need_type and "alloc::vec::Vec" not in alloc_set:
            lacks.append("Vec(类型) 缺 `use alloc::vec::Vec;`")
        if need_string and not (
            "alloc::string::String" in alloc_set
            or "alloc::string::ToString" in alloc_set
        ):
            lacks.append("String/format!/to_string() 缺 `use alloc::string::{String, ToString};`")
        if lacks:
            missing.append({
                "file": rs.relative_to(src.parent).as_posix(),
                "lacks": lacks,
            })

    return {
        "gate": "G3",
        "name": "no_std 三件套（铁律 6：宏 vec / 类型 Vec / String 三名分别引入）",
        "ok": len(missing) == 0,
        "files_checked": checked,
        "missing_count": len(missing),
        "note": "已排除 #[cfg(test)] 作用域与 std 引入文件（测试构建下 std prelude 生效，"
                "不构成内核缺项）；真内核构建请以 --features kernel-image 编译为准",
        "missing": missing[:30],
    }


# --------------------------------------------------------------------------
# G4 编译图纯净度
# --------------------------------------------------------------------------


def gate_graph_purity(src: Path, tp_roots: list[Path]):
    detail = []
    for tp in tp_roots:
        n_rs = len(list(tp.rglob("*.rs"))) if tp.is_dir() else 0
        try:
            size_mb = sum(f.stat().st_size for f in tp.rglob("*") if f.is_file()) / 1024 / 1024
        except OSError:
            size_mb = 0.0
        detail.append({
            "path": tp.relative_to(src.parent).as_posix(),
            "rs_files": n_rs,
            "size_mb": round(size_mb, 2),
        })
    return {
        "gate": "G4",
        "name": "编译图纯净度（src/ 下不得混入自带 Cargo.toml 的外来工程）",
        "ok": len(tp_roots) == 0,
        "foreign_projects": len(tp_roots),
        "detail": detail,
    }


# --------------------------------------------------------------------------
# 基线对比
# --------------------------------------------------------------------------


def diff_against_baseline(current: dict, baseline: dict):
    """与历史基线对比，只报增量（用于 CI 增量门禁：不得劣化）。"""
    out = []
    for key, path in (
        ("kernel_path_hits", ("gates", "G2", "kernel_path_hits")),
        ("check_path_hits", ("gates", "G2", "check_path_hits")),
        ("missing_count", ("gates", "G3", "missing_count")),
        ("error_count", ("gates", "G1", "error_count")),
    ):
        node = baseline
        for seg in path:
            node = node.get(seg, {}) if isinstance(node, dict) else {}
        old = node.get(key, 0)
        new = current.get(key, 0)
        if new > old:
            out.append({"metric": key, "baseline": old, "current": new, "delta": new - old})
    return out


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description="Varix 内核质量门禁（AI-3 交付）")
    ap.add_argument("--no-cargo", action="store_true", help="跳过 G1/G1b（只跑静态门禁，秒级）")
    ap.add_argument("--no-host", action="store_true", help="只跑内核 kcheck，跳过宿主构建 G1b")
    ap.add_argument("--json", action="store_true", help="机读 JSON 输出")
    ap.add_argument("--baseline", help="基线 JSON 文件路径，用于增量对比")
    ap.add_argument("--root", help="kernel/ 根目录（默认自动定位）")
    args = ap.parse_args()

    root = Path(args.root).resolve() if args.root else repo_root()
    src = root / "varix" / "src"
    if not src.is_dir():
        print(f"FAIL: 找不到源码目录 {src}", file=sys.stderr)
        return 2

    tp_roots = third_party_roots(src)
    gates: dict[str, dict] = {}

    if not args.no_cargo:
        gates["G1"] = gate_cargo_check(root)
        if not args.no_host:
            gates["G1b"] = gate_host_build(root)
    gates["G2"] = gate_panic_surface(src, tp_roots)
    gates["G3"] = gate_no_std_trio(src, tp_roots)
    gates["G4"] = gate_graph_purity(src, tp_roots)

    result = {
        "tool": "varix_quality_gate",
        "owner": "AI-3 质量门禁官",
        "anchor": "docs/Varix/7AI分工总案.md · AI-3",
        "root": str(root),
        "gates": gates,
    }

    regress = []
    if args.baseline and Path(args.baseline).is_file():
        try:
            base = json.loads(Path(args.baseline).read_text(encoding="utf-8"))
            regress = diff_against_baseline(gates, base)
            result["regressions"] = regress
        except (OSError, json.JSONDecodeError) as exc:
            result["baseline_error"] = str(exc)

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        _render_human(result)

    hard_ok = all(g.get("ok", False) for g in gates.values())
    # 增量劣化视同失败
    return 0 if (hard_ok and not regress) else 1


def _render_human(result: dict) -> None:
    print("=" * 72)
    print("Varix 内核质量门禁 · AI-3 质量门禁官")
    print(f"锚点：{result['anchor']}")
    print("=" * 72)
    for key in ("G1", "G1b", "G2", "G3", "G4"):
        g = result["gates"].get(key)
        if not g:
            continue
        mark = "PASS" if g.get("ok") else "FAIL"
        print(f"\n[{mark}] {key} · {g.get('name','')}")
        if g.get("fatal"):
            print(f"       {g['fatal']}")
            continue
        if key in ("G1", "G1b"):
            print(f"       命令：{g.get('cmd','')}")
            print(f"       error={g.get('error_count')}  warning={g.get('warning_total')}")
            if g.get("errors_by_code"):
                print("       错误码分布：")
                for code, n in list(g["errors_by_code"].items())[:8]:
                    print(f"         {n:5d}  {code}")
            if g.get("errors_by_file"):
                print("       涉及文件 TOP：")
                for f, n in list(g["errors_by_file"].items())[:8]:
                    print(f"         {n:5d}  {f}")
            for e in g.get("errors_sample", [])[:5]:
                print(f"       ! {e}")
        elif key == "G2":
            print(f"       内核路径命中 = {g['kernel_path_hits']}  （硬门禁）")
            print(f"       判据层命中   = {g['check_path_hits']}  （需人工裁决）")
            print(f"       #[cfg(test)] = {g['test_scope_hits']}  （宿主侧 std，豁免）")
            for h in g["kernel_sample"][:10]:
                print(f"       ! {h}")
            if g["top_files"]:
                print("       内核路径命中最多文件：")
                for f, n in g["top_files"][:8]:
                    print(f"         {n:5d}  {f}")
        elif key == "G3":
            print(f"       内核路径需检查文件={g['files_checked']}  缺项={g['missing_count']}")
            print(f"       说明：{g['note']}")
            for m in g["missing"][:10]:
                print(f"       ! {m['file']}: {'; '.join(m['lacks'])}")
        elif key == "G4":
            print(f"       外来工程 = {g['foreign_projects']}")
            for d in g["detail"]:
                print(f"       ! {d['path']}  ({d['rs_files']} rs, {d['size_mb']}MB)")

    print("\n" + "=" * 72)
    hard_ok = all(g.get("ok", False) for g in result["gates"].values())
    if result.get("regressions"):
        print("检测到相对基线的劣化项：")
        for r in result["regressions"]:
            print(f"  ! {r['metric']}: {r['baseline']} → {r['current']} (+{r['delta']})")
    print(f"总判定：{'PASS' if hard_ok and not result.get('regressions') else 'FAIL'}")
    print("=" * 72)


if __name__ == "__main__":
    sys.exit(main())