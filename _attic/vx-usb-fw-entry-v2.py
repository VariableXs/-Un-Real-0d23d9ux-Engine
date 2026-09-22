# -*- coding: utf-8 -*-
"""v2：给 U 盘 Windows 追加固件引导项。
先 bcdedit /enum {fwbootmgr} /v 全量枚举，按描述/设备路径识别 U 盘上的
固件项（UEFI/Limine/thinkplus/varix 或含 U 盘 ESP GUID 两种端序形态），
复制它（保留同类型）→ 改 device=Y: + path=bootmgfw.efi → displayorder
/addlast。识别失败则原样退出（绝不蒙）。内置项零改动。"""
import os
import re
import subprocess
import sys

USB_ESP_GUID = "636786cb-e967-49f6-b0df-7608909d1f11"
# 混合端序形态（固件设备路径 HD 节点常用）
MIXED = "cb867863-67e9-f649-b0df-7608909d1f11"
DESC_PAT = re.compile(r"(uefi|limine|varix|thinkplus|usb)", re.I)
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-usb-fw-entry-v2.rpt")
lines = []


def run(cmd, desc):
    p = subprocess.run(cmd, capture_output=True)
    out = (p.stdout + p.stderr).decode("gbk", "replace").strip()
    lines.append(f"=== [{desc}] rc={p.returncode}\n{out}")
    return p.returncode, out


def main():
    rc, out = run(["bcdedit", "/enum", "{fwbootmgr}", "/v"], "enum-v")
    if rc != 0:
        lines.append("ENUM-FAIL")
        return
    # 解析 displayorder 列表 → 逐项 /v 查详情
    ids = re.findall(r"\{[0-9a-fA-F-]{36}\}", out)
    ids = [i for i in dict.fromkeys(ids)
           if i.lower() != "{9dea862c-5cdd-4e70-acc1-f32b344d4795}"]  # {bootmgr}别名跳过
    cand = None
    for ident in ids:
        rc2, det = run(["bcdedit", "/enum", ident, "/v"], f"detail {ident[:8]}")
        if rc2 != 0:
            continue
        desc_hit = DESC_PAT.search(det)
        guid_hit = (USB_ESP_GUID in det.lower()) or (MIXED in det.lower())
        lines.append(f"candidate {ident}: desc_hit={bool(desc_hit)} guid_hit={guid_hit}")
        if (desc_hit or guid_hit) and cand is None:
            cand = ident
    if cand is None:
        lines.append("NO-CANDIDATE-ABORT")
        return
    lines.append(f"chosen source={cand}")
    rc, out2 = run(["bcdedit", "/copy", cand, "/d", "VARIX Windows (USB)"],
                   "copy-entry")
    m = re.search(r"\{[0-9a-fA-F-]{36}\}", out2)
    if rc != 0 or not m:
        lines.append("COPY-FAIL")
        return
    new = m.group(0)
    run(["bcdedit", "/set", new, "device", "partition=Y:"], "set-device")
    run(["bcdedit", "/set", new, "path", r"\EFI\Microsoft\Boot\bootmgfw.efi"],
        "set-path")
    run(["bcdedit", "/set", "{fwbootmgr}", "displayorder", new, "/addlast"],
        "append-order")
    rc, out3 = run(["bcdedit", "/enum", "{fwbootmgr}"], "verify")
    ok = new.split("{")[1][:8] in out3
    lines.append(("USB-FW-ENTRY-DONE" if ok else "VERIFY-FAIL") + f" new={new}")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        try:
            main()
        except Exception:
            import traceback
            lines.append("EXCEPTION:\n" + traceback.format_exc())
        with open(LOG, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    else:
        import ctypes
        import time
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(90):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read()[-3000:])
                break
        else:
            print("timeout waiting report")
