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

3. **有盘后部署**（推荐用 AI-5 的四阶段编排，带目标盘安全闸）
   ```powershell
   .\AI5\Deploy-To-USB.ps1 -Action Preflight -Src D:\Variable-USB -Dst E:\
   .\AI5\Deploy-To-USB.ps1 -Action Stage4  -Src D:\Variable-USB -Dst E:\
   .\AI5\Deploy-To-USB.ps1 -Action Verify  -Dst E:\
   # E: 是你的1TB双接口盘；脚本会拒绝把宿主系统盘当目标，且不用 robocopy /MIR
   ```
   > 旧的一行版 `.\Deploy-To-USB.ps1 -Src ... -Dst ...` 仍可用，但无目标盘校验。

4. **测试与验收**（AI-5，第11+12章）
   ```powershell
   .\AI5\Compat-Matrix.ps1 -Action List      # Top200 兼容矩阵
   .\AI5\Chaos-Inject.ps1  -Action List      # 12 个混沌场景
   .\AI5\Bench-Perf.ps1    -Action Run -TestDrive D:\
   .\AI5\Accept-Gate.ps1   -Action Report    # 14 项验收汇总
   .\AI5\Maintenance.ps1   -Action Status    # 运维现状
   ```

5. **自检**（必须在 Windows 上跑）
   ```powershell
   pwsh -NoProfile -File tests\Run-PortableTests.ps1
   ```

详见 `../docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 11/12 章、`../docs/AI5-测试交付.md` 与 `AI5/README.md`。
