# -*- coding: utf-8 -*-
"""排查 full-audit 卡点（只读）：ISO 挂载状态、USB 盘分区访问路径现状。"""
import os
import subprocess

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "vx-audit-probe.rpt")
out = []


def run_ps(cmd, timeout=90):
    r = subprocess.run(["powershell", "-NoProfile", "-Command", cmd],
                       capture_output=True, timeout=timeout)
    return r.stdout.decode("gbk", "replace").strip(), r.stderr.decode("gbk", "replace").strip()


def main():
    o, e = run_ps(
        "try { $di = Get-DiskImage -ImagePath "
        "'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main\\varix-qemu.iso' -ErrorAction Stop; "
        "'ISO-ATTACHED=' + $di.Attached } catch { 'ISO-QUERY-ERR: ' + $_.Exception.Message }")
    out.append(o or ("ERR: " + e))
    o, e = run_ps(
        "$v = Get-Volume | Where-Object DriveLetter | ForEach-Object { "
        "$_.DriveLetter + ':' + $_.FileSystemLabel }; $v -join ' | '")
    out.append("VOLUMES: " + o)
    o, e = run_ps(
        "$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
        "if ($d) { 'USB DISK ' + $d.Number; Get-Partition -DiskNumber $d.Number | "
        "ForEach-Object { 'part#' + $_.PartitionNumber + ' paths=[' + "
        "($_.AccessPaths -join ',') + ']' } } else { 'NO-USB' }")
    out.append(o or ("ERR: " + e))
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")


if __name__ == "__main__":
    try:
        main()
    except Exception as ex:
        out.append("EXC: " + repr(ex))
        with open(OUT, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    print(open(OUT, encoding="utf-8").read())
