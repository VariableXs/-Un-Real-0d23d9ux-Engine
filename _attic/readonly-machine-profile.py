#!/usr/bin/env python3
"""只读抓取真机配置(CPU/内存/显示/固件)——用于 QEMU 贴实机模拟参数。"""
import subprocess


def ps(cmd, timeout=90):
    r = subprocess.run(
        ["powershell", "-NoProfile", "-Command", cmd],
        capture_output=True, timeout=timeout,
    )
    return r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")


print("=== CPU ===")
print(ps(
    "Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,"
    "NumberOfLogicalProcessors,MaxClockSpeed | Format-List | Out-String"
))
print("=== 内存 ===")
print(ps(
    "Get-CimInstance Win32_PhysicalMemory | Select-Object Manufacturer,Capacity,Speed | Format-Table -AutoSize | Out-String; "
    "[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)"
))
print("=== 显示适配器 ===")
print(ps(
    "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | Format-List | Out-String"
))
print("=== 机型/固件 ===")
print(ps(
    "Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,SystemFamily | Format-List | Out-String; "
    "Get-CimInstance Win32_BIOS | Select-Object SMBIOSBIOSVersion | Format-List | Out-String"
))
