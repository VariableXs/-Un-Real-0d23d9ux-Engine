# 列出磁盘 0/1 分区布局（只读）
import subprocess

r = subprocess.run(
    ["powershell", "-NoProfile", "-c",
     "Get-Partition | Select DiskNumber,PartitionNumber,DriveLetter,GptType,Size | "
     "Sort DiskNumber,PartitionNumber | Format-Table -AutoSize | Out-String -Width 200"],
    capture_output=True,
)
print((r.stdout + r.stderr).decode("gbk", "replace"))
