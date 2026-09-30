# Portable 启动套件

无U盘也能开干，`D:\Variable-USB` 就是你的未来U盘。

## 开始 3步

1. **造盘** (管理员PowerShell)
   ```powershell
   .\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB -SizeGB 150
   # 机械盘加 -Fixed，固态默认动态
   ```

2. **隔离测试**
   ```powershell
   .\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx
   ```
   弹出的窗口就是你的便携系统，删C盘/装Blender/崩溃都只在子盘，关机丢弃即还原。

3. **有盘后部署**
   ```powershell
   .\Deploy-To-USB.ps1 -Src D:\Variable-USB -Dst E:\
   # E: 是你的2TB双接口盘
   ```

详见 `../docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 12章。
