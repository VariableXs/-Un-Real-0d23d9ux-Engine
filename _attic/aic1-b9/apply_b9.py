# 批次九应用：追加片段 + deep8 程序化接线（调用带括号！生成器修正版）
import io, re

FILES = ["condrv", "lnkfile", "fontchain", "comloc", "clipfmt", "gdiface",
         "reghive", "gdiplus", "peblend", "wow64", "winmgr", "comdlg",
         "fsredir", "persrc", "dragdrop", "excface"]

for f in FILES:
    p = "kernel/varix/src/compatstar/%s.rs" % f
    s = io.open(p, encoding="utf-8").read()
    frag = io.open("_attic/aic1-b9/b9-%s.rs" % f, encoding="utf-8").read()
    assert frag.startswith("\n// ------"), f
    if "run_%s_deep8_checks" % f in s:
        print("%s: already applied, skip" % f)
        continue
    s = s + frag

    names = sorted(set(re.findall(r"run_%s_(base|deep\d*)_checks" % f, s)),
                   key=lambda k: (0 if k == "base" else 1 if k == "deep" else 1 + int(k[4:])))
    assert "base" in names and "deep8" in names, (f, names)
    calls = ["run_%s_%s_checks()" % (f, k) for k in names]  # 带括号！
    expr = calls[-1]
    for c in reversed(calls[:-1]):
        expr = "CheckSet::merge(%s, %s)" % (c, expr)
    canon = "pub fn run_%s_checks() -> CheckSet {\n    %s\n}" % (f, expr)
    m = re.search(r"pub fn run_%s_checks\(\) -> CheckSet \{.*?\n\}" % f, s, re.S)
    assert m, f
    s = s[:m.start()] + canon + s[m.end():]
    io.open(p, "w", encoding="utf-8", newline="").write(s)
    print("%s: appended + rewired (%d segments)" % (f, len(calls)))
print("batch-9 applied to %d files" % len(FILES))
