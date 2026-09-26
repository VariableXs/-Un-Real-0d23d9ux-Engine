# -*- coding: utf-8 -*-
"""部署三证验证（全只读）：U 盘 ESP 上的 kernel 与本地 ISO 内 kernel 的 sha256 一致性。
挂 ESP(Y:) → 提取 /kernel/varix 哈希 → 对比本地 varix-qemu.iso 内同名文件哈希 → 摘盘符。"""
import hashlib
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ISO = os.path.join(os.path.dirname(HERE), "varix-qemu.iso")
LOG = os.path.join(HERE, "vx-verify-deploy.rpt")
out = []


def sha256_file(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest().upper()


def extract_iso_kernel(iso, dest):
    """从 ISO9660 提取 /kernel/varix（简单遍历 ISO 找文件记录太重——用 Windows 自带挂载）。"""
    r = subprocess_run(["powershell", "-NoProfile", "-Command",
                        f"Mount-DiskImage -ImagePath '{iso}' -PassThru | "
                        "Get-Volume | ForEach-Object { $_.DriveLetter }"])
    letter = r.strip().splitlines()[-1].strip() if r.strip() else ""
    return letter


def subprocess_run(cmd):
    import subprocess
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    return r.stdout


def main():
    out.append(f"ISO exists={os.path.exists(ISO)} size={os.path.exists(ISO) and os.path.getsize(ISO)}")
    # 1) 挂 ISO
    letter = extract_iso_kernel(ISO, None)
    out.append(f"ISO mounted at {letter}:")
    iso_kernel = f"{letter}:\\kernel\\varix"
    iso_hash = sha256_file(iso_kernel) if os.path.exists(iso_kernel) else None
    out.append(f"ISO kernel sha256={iso_hash}")
    # 2) 挂 ESP（Y:）读 kernel
    ps = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
          "if (-not $d) { 'NO-DISK' } else {"
          "$p = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq "
          "'{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1;"
          "if ($p.AccessPaths -notcontains 'Y:\\') { Add-PartitionAccessPath "
          "-DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'Y:\\' | Out-Null; 'ESP-MOUNTED' }"
          "else { 'ESP-ALREADY' };"
          "$k = Get-Item 'Y:\\kernel\\varix' -ErrorAction SilentlyContinue;"
          "if ($k) { 'ESP kernel mtime=' + $k.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') + ' size=' + $k.Length }"
          "else { 'ESP kernel missing' } }")
    r2 = subprocess_run(["powershell", "-NoProfile", "-Command", ps])
    out.append(r2.strip())
    if os.path.exists("Y:\\kernel\\varix"):
        esp_hash = sha256_file("Y:\\kernel\\varix")
        out.append(f"ESP kernel sha256={esp_hash}")
        out.append("DEPLOY-VERIFY: " + ("MATCH" if esp_hash == iso_hash else "MISMATCH"))
    # 3) 摘载
    subprocess_run(["powershell", "-NoProfile", "-Command",
                    "$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
                    "if ($d) { $p = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq "
                    "'{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1;"
                    "if ($p -and ($p.AccessPaths -contains 'Y:\\')) { Remove-PartitionAccessPath "
                    "-DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'Y:\\' | Out-Null } };"
                    f"Dismount-DiskImage -ImagePath '{iso}' | Out-Null; 'CLEANED'"])
    out.append("VX-VERIFY-DEPLOY-DONE")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        try:
            main()
        except Exception:
            import traceback
            out.append("EXCEPTION:\n" + traceback.format_exc())
        with open(LOG, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    else:
        import ctypes
        import time
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(120):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read()[-2000:])
                break
        else:
            print("timeout")
