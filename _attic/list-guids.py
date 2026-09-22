# 列出磁盘 1 各分区的 GPT GUID（只读）
import subprocess

r = subprocess.run(
    ["powershell", "-NoProfile", "-c",
     "Get-Partition -DiskNumber 1 | Select PartitionNumber,DriveLetter,GptType,Guid | "
     "Format-List | Out-String -Width 200"],
    capture_output=True,
)
print((r.stdout + r.stderr).decode("gbk", "replace"))
