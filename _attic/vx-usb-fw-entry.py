# -*- coding: utf-8 -*-
"""两步走（提权）：
1) 只读枚举 U 盘 BCD（Y:\\EFI\\Microsoft\\Boot\\BCD）确认 Windows 加载器在位；
2) 用系统 BCD 给 U 盘 Windows 追加一条固件引导项（/application BOOTMGR，
   device=Y:，path=bootmgfw.efi），appendlast 到 {fwbootmgr} displayorder——
   内置项零改动、顺序保持内置居首。全程日志落盘。"""
import os
import subprocess
import sys
import uuid

USB_BCD = r"Y:\EFI\Microsoft\Boot\BCD"
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-usb-fw-entry.rpt")
lines = []


def run(cmd, desc):
    p = subprocess.run(cmd, capture_output=True)
    out = (p.stdout + p.stderr).decode("gbk", "replace").strip()
    lines.append(f"=== [{desc}] rc={p.returncode}\n{out}")
    return p.returncode, out


def main():
    # 1) U 盘 BCD 只读体检
    run(["bcdedit", "/store", USB_BCD, "/enum"], "usb-bcd-enum")

    # 2) 新固件项
    new_guid = "{" + str(uuid.uuid4()) + "}"
    rc, _ = run(["bcdedit", "/create", new_guid, "/d", "VARIX Windows (USB)",
                 "/application", "BOOTMGR"], "create-entry")
    if rc != 0:
        lines.append("CREATE-FAIL")
        return
    run(["bcdedit", "/set", new_guid, "device", "partition=Y:"], "set-device")
    run(["bcdedit", "/set", new_guid, "path", r"\EFI\Microsoft\Boot\bootmgfw.efi"],
        "set-path")
    run(["bcdedit", "/set", "{fwbootmgr}", "displayorder", new_guid, "/addlast"],
        "append-displayorder")
    rc, out = run(["bcdedit", "/enum", "{fwbootmgr}"], "verify-fwbootmgr")
    lines.append(("USB-FW-ENTRY-DONE" if new_guid.split("{")[1][:8] in out
                  else "VERIFY-FAIL") + f" new={new_guid}")


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
                print(open(LOG, encoding="utf-8").read()[-2500:])
                break
        else:
            print("timeout waiting report")
