# -*- coding: utf-8 -*-
"""Add (async) to every #[tauri::command] whose function is a sync `pub fn`.
Tauri v2 runs sync commands on the MAIN thread (deadlock/block source);
#[tauri::command(async)] dispatches them to the worker pool with zero
signature changes. Idempotent: already-async commands are untouched."""
import re, sys, io, glob
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

changed_files = 0
total = 0
for f in glob.glob("src-tauri/src/**/*.rs", recursive=True):
    src = open(f, encoding="utf-8", errors="replace").read()
    out = []
    pos = 0
    n = 0
    for m in re.finditer(r"#\[tauri::command(\([^)]*\))?\]", src):
        start, end = m.span()
        attr_inner = (m.group(1) or "").strip()
        # look ahead past other attributes to the fn declaration
        rest = src[end:]
        fm = re.match(r"(?:\s*#\[[^\]]*\])*\s*pub\s+(async\s+)?fn\b", rest)
        if not fm or fm.group(1):
            continue  # not a command fn or already async
        if attr_inner:
            if "async" in attr_inner:
                continue
            new_attr = f"#[tauri::command(async, {attr_inner.strip('()')})]"
        else:
            new_attr = "#[tauri::command(async)]"
        out.append(src[pos:start])
        out.append(new_attr)
        pos = end
        n += 1
    if n:
        out.append(src[pos:])
        open(f, "w", encoding="utf-8", newline="").write("".join(out))
        changed_files += 1
        total += n
        print(f"{f}: {n}")
print(f"TOTAL {total} commands in {changed_files} files")
