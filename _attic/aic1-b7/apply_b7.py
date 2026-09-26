# -*- coding: utf-8 -*-
# AI-C1 批次七：片段追加 + deep6 接线（17 域——F003/F005/F015 轮休）
import io

files = ["peblend","wow64","gdiface","gdiplus","comdlg","reghive","fsredir","condrv","lnkfile","fontchain","clipfmt","comloc","excface","dblrun"]

for f in files:
    p = "kernel/varix/src/compatstar/%s.rs" % f
    s = io.open(p, encoding='utf-8').read()
    if ("run_%s_deep6_checks" % f) in s:
        print("skip (already applied):", f)
        continue
    frag = io.open("_attic/aic1-b7/b7-%s.rs" % f, encoding='utf-8').read()
    assert frag.startswith('\n// ------'), f
    assert ("run_%s_deep6_checks" % f) not in s, f
    s = s + frag
    old = "run_%s_deep5_checks())))))" % f
    new = "run_%s_deep5_checks(), CheckSet::merge(run_%s_deep6_checks()))))))" % (f, f)
    assert s.count(old) == 1, (f, s.count(old))
    s = s.replace(old, new)
    io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("b7 appended+rewired", len(files))
