# audit_domain_closure.py — 域判据闭环对账（只读，不改代码）
# 
# 用途：核对 robust.rs 的 run_kernel_checkup 函数指针表，是否覆盖了各域
# 内所有 "返回 CheckSet 的 pub run_* 入口"，并检出重复登记 / 孤儿函数。
# 
# 关键教训（写这个脚本的原因）：Windows glob 返回反斜杠路径，若用
# `endswith('/mod')` 过滤会静默失效，把 mod.rs 自身也算成子模块，导致
# 全表误报为"孤儿"。必须先统一分隔符再过滤。

import os
import re
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
VARIX = os.path.join(ROOT, "..", "varix")
ROBUST = os.path.join(VARIX, "src", "robust.rs")

# 表的定位：函数指针表以 `let domains: [fn() -> CheckSet; N] = [` 开头
# 以同缩进的 `];` 结束。
TABLE_RE = re.compile(r"let\s+domains\s*:\s*\[fn\(\)\s*->\s*CheckSet;\s*(\d+)\]\s*=\s*\[")

# 文件读取缓存。resolve_body / resolve_helper 会对同一文件反复读取
#（聚合器 -> helper -> 子聚合器），全内核 2166 个文件下不缓存会退化到
# 几分钟。键为绝对路径。
_CLEAN_CACHE = {}


def clean_of(path):
    """读取并剥注释，带进程内缓存。"""
    if path not in _CLEAN_CACHE:
        _CLEAN_CACHE[path] = strip_comments(open(path, encoding="utf-8").read())
    return _CLEAN_CACHE[path]


def read_table():
    lines = open(ROBUST, encoding="utf-8").read().splitlines()
    start = decl_len = None
    for i, ln in enumerate(lines):
        m = TABLE_RE.search(ln)
        if m:
            start, decl_len = i, int(m.group(1))
            break
    if start is None:
        sys.exit("FATAL: 未在 robust.rs 中找到 domains 函数指针表")
    entries = []
    for i in range(start + 1, len(lines)):
        if re.match(r"^\s*\];", lines[i]):
            end = i
            break
        s = lines[i].strip().rstrip(",")
        if s.startswith("crate::") and "::run_" in s:
            entries.append(s)
    else:
        sys.exit("FATAL: 表未闭合")
    return decl_len, len(entries), entries, start + 1, end + 1


def collect_runs(domain):
    """收集域内所有 `pub fn run_xxx(...) -> CheckSet` 入口。"""
    out = []
    base = os.path.join(VARIX, "src", domain)
    for dirpath, _dirs, files in os.walk(base):
        for fn in sorted(files):
            if not fn.endswith(".rs"):
                continue
            full = os.path.join(dirpath, fn)
            rel = os.path.relpath(full, os.path.join(VARIX, "src"))
            parts = rel.replace("\\", "/").split("/")
            if parts[-1] == "mod.rs":
                continue  # 聚合层，不是子模块判据
            # parts = ['perfstar', 'bigpage.rs'] -> Rust 模块路径 perfstar::bigpage
            # 通用做法：整条路径去掉 .rs 后缀即模块链（兼容嵌套子目录）。
            mod = "::".join(parts[:-1] + [parts[-1][:-3]])
            text = open(full, encoding="utf-8").read()
            # 只认返回 CheckSet 的入口；run_parallel / run_until / run_standard
            # 等辅助驱动器签名不同，不应进表。
            for m in re.finditer(
                r"pub\s+fn\s+(run_[A-Za-z0-9_]+)\s*\(\s*\)\s*->\s*([A-Za-z0-9_:<>]+)",
                text,
            ):
                ret = m.group(2)
                if ret != "CheckSet":
                    continue
                out.append("crate::%s::%s" % (mod, m.group(1)))
    return sorted(out)


def collect_folded():
    """收集所有"折叠聚合器"内实际被调用的判据符号。

    为什么要单独收：CGPU / H2 / UNX 等域不逐条进domains 表，而是由
    `run_xxx_checks()` 把N 个子集折叠成一条汇总判据。若只拿文件内
    `run_*` 与 domains 表比对，会把这些已被折叠覆盖的入口全误报成
    "孤儿"（第一版脚本就犯了这个错，报了 167 个假孤儿）。

    覆盖来源两类：
      1. robust.rs 里 `checkup.register(crate::X::run_yyy_checks())` 的直挂聚合；
      2. 各聚合器函数体内出现的 `submodule::run_zzz_checks()` 调用。
    """
    folded = set()

    # (1) robust.rs 直挂的聚合器：收集其函数体内所有 ::run_xxx( 调用
    robust = open(ROBUST, encoding="utf-8").read()
    for m in re.finditer(
        r"checkup\.register\(\s*crate::([a-z0-9_]+(?:::[a-z0-9_]+)*)::(run_[a-z0-9_]+)\s*\(\s*\)\s*\)",
        robust,
    ):
        path = "crate::%s::%s" % (m.group(1), m.group(2))
        folded.add(path)
        folded |= resolve_body(path)

    # (2) 各域 mod.rs / 聚合层里的聚合器（即使未直挂也纳入，避免漏判）
    for dirpath, _dirs, files in os.walk(os.path.join(VARIX, "src")):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            full = os.path.join(dirpath, fn)
            rel = os.path.relpath(full, os.path.join(VARIX, "src"))
            seg = rel.replace("\\", "/").split("/")
            mod = "::".join(seg[:-1] + [seg[-1][:-3]])
            # mod.rs 对应 Rust 的父模块本身，不叫 `cgpu::mod`。
            # 不剥掉这层，函数体内的 `sub::run_x()` 会被拼成
            # crate::cgpu::mod::sub::run_x，与实际路径差一段，
            # 导致"已被折叠覆盖"被误判成孤儿。
            if mod.endswith("::mod"):
                mod = mod[: -len("::mod")]
            elif mod == "mod":
                mod = ""
            symbol_prefix = "crate::" + mod if mod else "crate"
            text = clean_of(full)
            for am in re.finditer(r"pub\s+fn\s+(run_[A-Za-z0-9_]+)\s*\(\s*\)\s*->\s*[A-Za-z0-9_:]*CheckSet", text):
                sym = "%s::%s" % (symbol_prefix, am.group(1))
                folded.add(sym)
                folded |= resolve_body(sym)
    return folded


def strip_comments(text):
    """去掉 Rust 行注释与块注释（保留字符串字面量），保留字符数不变。

    保留长度是为了让 clean 与原文的下标一一对应——本脚本后续按偏移切片
    取函数体，替换成短字符串会让所有下标失效。实现上把注释内容替换为空格。
    """
    out = []
    i, n = 0, len(text)
    while i < n:
        two = text[i : i + 2]
        if two == "//":
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth:
                if text[j : j + 2] == "/*":
                    depth += 1
                    j += 2
                elif text[j : j + 2] == "*/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            out.append(" " * (j - i))
            out.append("\n" * text.count("\n", i, j))
            i = j
        elif text[i] == '"':
            # 字符串字面量整体跳过（转义引号要处理）
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(text[i:j])
            i = j
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def resolve_body(symbol):
    """给定 `crate::a::b::run_x`，返回其函数体内调用的所有 ::run_y( 符号。"""
    # 先剥掉末段的函数名，得到"模块路径"；函数体可能写在 <mod>.rs 或
    # <mod>/mod.rs。直接拿完整 symbol 拼路径必然FileNotFound。
    parts = symbol[len("crate::") :].split("::")
    if parts[-1].startswith("run_"):
        parts = parts[:-1]
    rel = os.path.join(*parts) if parts else ""
    candidates = [
        os.path.join(VARIX, "src", rel + ".rs"),
        os.path.join(VARIX, "src", rel, "mod.rs"),
    ]
    full = next((p for p in candidates if os.path.isfile(p)), None)
    if full is None:
        return set()
    text = open(full, encoding="utf-8").read()
    # 先剥注释再定位函数体：`[^{]*` 会被 Rustdoc 示例里的 '{' 提前截断，
    # 第二版脚本就栽在这，导致 cgpu 的 29 条折叠全被判成孤儿。
    clean = clean_of(full)
    fname = symbol.rsplit("::", 1)[-1]
    m = re.search(r"pub\s+fn\s+%s\s*\(\s*\)" % re.escape(fname), clean)
    if not m:
        return set()
    b = clean.find("{", m.end())
    if b < 0:
        return set()
    depth, i = 0, b
    while i < len(clean):
        if clean[i] == "{":
            depth += 1
        elif clean[i] == "}":
            depth -= 1
            if depth == 0:
                break
        i += 1
    body = clean[b:i]
    # prefix 已含 "crate::" 前缀，不再重复拼——否则会得到 crate::crate::cgpu::…
    # 导致所有折叠覆盖判定失效（第四版脚本的 bug）。
    prefix = symbol.rsplit("::", 1)[0]
    out = set()
    for c in re.finditer(r"(?:\b([a-z_][a-z0-9_]*)::)?(run_[a-z0-9_]+)\s*\(\s*\)", body):
        local, callee = c.group(1), c.group(2)
        if local:
            out.add("%s::%s::%s" % (prefix, local, callee))
        else:
            out.add("%s::%s" % (prefix, callee))
    # 模式 B：聚合器不直接列子集，而是遍历一个返回 (标签, 子集) 列表的
    # 辅助函数（h3star_blocks / secstar2_blocks / perfstar2_blocks /
    # compatstar2_blocks 都这么写，且内部还嵌merge_sets(...)）。
    # 不追进辅助函数会把这些域的全部子判据误报成孤儿——第五版脚本的盲区。
    # 判据：本地无参调用、名字不是 run_*（run_* 已在上面按符号解析）、
    # 且源文件里确实存在该函数定义。
    for helper in set(re.findall(r"\b([a-z_][a-z0-9_]*)\s*\(", body)):
        if helper.startswith("run_"):
            continue
        if not re.search(r"fn\s+%s\s*\([^)]*\)" % re.escape(helper), clean):
            continue
        out |= resolve_helper("%s::%s" % (prefix, helper))
    # 模式 C：函数指针数组 `[("F058", mod::run_a, mod::run_b, ...); N]`
    # （perfstar2 五层行级 AND、compatstar2 主检数组都这么写）。
    # 上面那条正则要求函数名后紧跟 "()"，函数指针没有括号，故单列一条：
    # 抓 `mod::run_xxx` 形式的裸符号引用。
    for c in re.finditer(r"(?:\b([a-z_][a-z0-9_]*)::)(run_[a-z0-9_]+)\b(?!\s*\()", body):
        out.add("%s::%s::%s" % (prefix, c.group(1), c.group(2)))
    return out


def resolve_helper(symbol, _depth=0):
    """解析返回 (标签, 子集) 列表的辅助函数，如 `h3star_blocks()`。

    这类函数把 (标签, 子集) 以 `[("tag", mod::run_x_checks()), ...]` 形式
    返回，内部还常嵌 `merge_sets(...)`；解析时提取全部 `mod::run_x()`。
    `_depth` 防止 helper 互相调用时无限递归。
    """
    if _depth > 4:
        return set()
    parts = symbol.split("::")
    fname = parts[-1]
    modparts = parts[:-1]
    # 剥掉开头的 `crate`——它不是目录层级，留着会让路径永不命中。
    # 与 resolve_body 里的同名处理保持一致。
    if modparts and modparts[0] == "crate":
        modparts = modparts[1:]
    rel = os.path.join(*modparts) if modparts else ""
    candidates = [
        os.path.join(VARIX, "src", rel + ".rs"),
        os.path.join(VARIX, "src", rel, "mod.rs"),
    ]
    full = next((p for p in candidates if os.path.isfile(p)), None)
    if full is None:
        return set()
    clean = clean_of(full)
    m = re.search(r"fn\s+%s\s*\([^)]*\)" % re.escape(fname), clean)
    if not m:
        return set()
    b = clean.find("{", m.end())
    if b < 0:
        return set()
    depth, i = 0, b
    while i < len(clean):
        if clean[i] == "{":
            depth += 1
        elif clean[i] == "}":
            depth -= 1
            if depth == 0:
                break
        i += 1
    body = clean[b:i]
    # modparts 已剥掉 `crate`，这里补回前缀，保证与表内符号同形。
    prefix = "crate::" + "::".join(modparts) if modparts else "crate"
    out = set()
    for c in re.finditer(r"(?:\b([a-z_][a-z0-9_]*)::)?(run_[a-z0-9_]+)\s*\(\s*\)", body):
        local, callee = c.group(1), c.group(2)
        if local:
            out.add("%s::%s::%s" % (prefix, local, callee))
        else:
            out.add("%s::%s" % (prefix, callee))
    # 模式 C 同样适用于 helper：函数指针数组元素没有括号。
    for c in re.finditer(r"(?:\b([a-z_][a-z0-9_]*)::)(run_[a-z0-9_]+)\b(?!\s*\()", body):
        out.add("%s::%s::%s" % (prefix, c.group(1), c.group(2)))
    # 模式 D：字符串模块名常量表 + match 分派函数（secstar2 的
    # `MODULES: [(&str,&str)]` + `tables_for()` 按字符串 match 返回
    # fn() -> CheckSet 数组）。分派体是 `"syspart" => chain([...])`
    # 或 `"x" => vec![...]` 两种形态，容器名不固定，故匹配
    # `"模块名" => ...(...)` 后取该层括号内的全部内容。
    for c in re.finditer(r'"[a-z0-9_]+"\s*=>\s*(?:chain|vec!)\s*\((.*?)\)\s*,?\s*(?:"|_|\})', body, re.S):
        for cc in re.finditer(r"(?:\b([a-z_][a-z0-9_]*)::)?(run_[a-z0-9_]+)\b", c.group(1)):
            local, callee = cc.group(1), cc.group(2)
            if local:
                out.add("%s::%s::%s" % (prefix, local, callee))
            else:
                out.add("%s::%s" % (prefix, callee))
    # 兜底：同一文件里任何 `mod::run_xxx` 裸引用都算被本helper 触达。
    # 上面的精确匹配对多层嵌套 (chain([...])) 容易漏，这一条保证不漏判
    # ——宁可多算覆盖（漏报），不可虚报孤儿。
    for c in re.finditer(r"\b([a-z_][a-z0-9_]*)::(run_[a-z0-9_]+)\b", body):
        out.add("%s::%s::%s" % (prefix, c.group(1), c.group(2)))
    # helper 内再调 helper（secstar2的 tables_for 等）
    if _depth < 4:
        for helper in set(re.findall(r"\b([a-z_][a-z0-9_]*)\s*\(", body)):
            if helper.startswith("run_"):
                continue
            if not re.search(r"fn\s+%s\s*\([^)]*\)" % re.escape(helper), clean):
                continue
            out |= resolve_helper("%s::%s" % (prefix, helper), _depth + 1)
    return out


def main():
    decl_len, n_entries, entries, tstart, tend = read_table()
    print("=== robust::run_kernel_checkup 表 ===")
    print("  声明长度 : %d" % decl_len)
    print("  实际条目 : %d" % n_entries)
    print("  行范围   : %d-%d" % (tstart, tend))
    print("  长度一致 : %s" % ("是" if decl_len == n_entries else "否 <-- 必须修"))
    print()

    seen = set()
    dups = []
    for e in entries:
        if e in seen:
            dups.append(e)
        seen.add(e)
    print("  重复登记 : %s" % (", ".join(dups) if dups else "无"))
    print()

    folded = collect_folded()
    covered = seen | folded
    print("  折叠聚合覆盖符号: %d" % len(folded))
    print()

    print("=== 各域闭环对账（表内直排 + 折叠覆盖）===")
    domains = sys.argv[1:] or ["perfstar", "compatstar", "ustar3", "cgpu", "unxstar"]
    total_orphan = 0
    for d in domains:
        runs = collect_runs(d)
        if not runs:
            print("  %-12s 无 CheckSet 入口" % d)
            continue
        orphan = [r for r in runs if r not in covered]
        total_orphan += len(orphan)
        inreg = [e for e in entries if e.startswith("crate::%s::" % d)]
        infold = [r for r in runs if r in folded and r not in seen]
        print(
            "  %-12s 文件内 %3d / 表内 %3d / 折叠内 %3d / 真孤儿 %d"
            % (d, len(runs), len(inreg), len(infold), len(orphan))
        )
        for o in orphan:
            print("      ORPHAN %s" % o)
    print()
    print("真孤儿合计: %d" % total_orphan)


if __name__ == "__main__":
    main()