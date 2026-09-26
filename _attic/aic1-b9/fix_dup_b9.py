# 批次九修复：lnkfile/persrc 的重复段替换为锚既有面版本
import io

def replace_block(path, fxx_marker, frag_path):
    s = io.open(path, encoding="utf-8").read()
    i = s.find(fxx_marker)
    assert i > 0, path
    # 回退到本块起始的虚线行
    j = s.rfind("// ---------------------------------------------------------------------------", 0, i)
    assert j > 0, path
    frag = io.open(frag_path, encoding="utf-8").read()
    assert frag.startswith("\n// ------"), frag_path
    s = s[:j] + frag[1:]  # frag 自带前导换行
    io.open(path, "w", encoding="utf-8", newline="").write(s)
    print(path, "block replaced")

replace_block("kernel/varix/src/compatstar/lnkfile.rs",
              "// F013 · 深化批次九", "_attic/aic1-b9/b9-lnkfile.rs")
replace_block("kernel/varix/src/compatstar/persrc.rs",
              "// F014 · 深化批次九", "_attic/aic1-b9/b9-persrc.rs")
