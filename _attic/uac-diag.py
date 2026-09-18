#!/usr/bin/env python3
"""UAC 弹窗行为诊断（无提权）：读 UAC 策略注册表。"""
import subprocess

PS = r"""
$k = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
$v = Get-ItemProperty -Path $k
Write-Output ("EnableLUA: " + $v.EnableLUA)
Write-Output ("ConsentPromptBehaviorAdmin: " + $v.ConsentPromptBehaviorAdmin + "  (0=no prompt, 5=default consent)")
Write-Output ("PromptOnSecureDesktop: " + $v.PromptOnSecureDesktop)
Write-Output ("ConsentPromptBehaviorUser: " + $v.ConsentPromptBehaviorUser)
"""

r = subprocess.run(["powershell", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
print(r.stderr.decode("gbk", "replace"))
