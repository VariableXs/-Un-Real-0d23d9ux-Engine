# 批次八应用脚本：追加片段 + deep7 程序化接线（嵌套表达式自底向上构造——配平由构造保证）+ 幂等
import io, re

FILES = ["peblend","pebind","wow64","winmgr","gdiface","gdiplus","comdlg","reghive",
         "fsredir","envsess","persrc","mlangres","dragdrop","clipfmt","excface","dblrun"]

for f in FILES:
    p = "kernel/varix/src/compatstar/%s.rs" % f
    s = io.open(p, encoding="utf-8").read()
    frag = io.open("_attic/aic1-b8/b8-%s.rs" % f, encoding="utf-8").read()
    assert frag.startswith("\n// ------"), f
    if "run_%s_deep7_checks" % f in s:
        print("%s: already applied, skip" % f)
        continue
    s = s + frag

    # 程序化重建聚合函数：收集所有 run_X_*_checks 调用名（含新 deep7）
    names = sorted(set(re.findall(r"run_%s_(base|deep\d*)_checks" % f, s)),
                   key=lambda k: (0 if k == "base" else 1 if k == "deep" else 1 + int(k[4:])))
    assert "base" in names and "deep7" in names, (f, names)
    calls = ["run_%s_%s_checks" % (f, k) for k in names]
    expr = calls[-1]
    for c in reversed(calls[:-1]):
        expr = "CheckSet::merge(%s, %s)" % (c, expr)
    canon = "pub fn run_%s_checks() -> CheckSet {\n    %s\n}" % (f, expr)
    m = re.search(r"pub fn run_%s_checks\(\) -> CheckSet \{.*?\n\}" % f, s, re.S)
    assert m, f
    s = s[:m.start()] + canon + s[m.end():]
    io.open(p, "w", encoding="utf-8", newline="").write(s)
    print("%s: appended + rewired (%d segments)" % (f, len(calls)))
print("batch-8 applied to %d files" % len(FILES))
