#!/usr/bin/env python3
"""部署前只读快检：确认 Disk 1 仍是目标 U 盘 + 盘符占用 + 旧部署状态清理检查。"""
import os
import subprocess

PS = r"""
Get-Disk | Select-Object Number,FriendlyName,@{n='GB';e={[math]::Round($_.Size/1GB,1)}},IsBoot,IsSystem,PartitionStyle | Format-Table -AutoSize | Out-String -Width 200
'--- Volumes ---'
Get-Volume | Where-Object DriveLetter | Select-Object DriveLetter,FileSystemLabel,@{n='GB';e={[math]::Round($_.Size/1GB,1)}} | Sort-Object DriveLetter | Format-Table -AutoSize | Out-String -Width 200
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)

st = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\deploy-state\deploy-state.json"
print("deploy-state.json:", "EXISTS（若 disk=disk1 会误跳过阶段，需删）" if os.path.exists(st) else "不存在（干净起步）")
sys_src = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\syssource"
for name in ("variable.exe",):
    p = os.path.join(sys_src, name)
    print(f"SysSource/{name}:", os.path.getsize(p) if os.path.exists(p) else "MISSING")
