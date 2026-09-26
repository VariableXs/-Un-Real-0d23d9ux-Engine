# -*- coding: utf-8 -*-
"""追加 2026-09-23 凌晨笔记：真机第三层卡死 + 交接链打通。"""
note = (
    "\n## 00:05-00:30 真机第三层收口（ring3 演示带 #PF + 交接链打通）\n"
    "- 实机实证：真机走完 ring3 受监护演示带（64 进程压力填表→回收）后、拉起 ushell 前 #PF，"
    "异常帧 RIP/CR3 呈 0x77 毒填充腐坏（QEMU 从不复现）。处置：main.rs 快进模式再下一城——"
    "quick（真机）跳过整个演示带，install() 后直接 spawn_ushell_now()（job_flags_reset + "
    "spawn_shell）；full（QEMU/probes_full）保持 run_demo() 全量（门禁语义不变，冒烟 PASS）。\n"
    "- 交接链打通：handoff_target=usb 后内核曾因 BootOrder 无 U 盘 Windows 项而拒绝（如实降级）。"
    "U 盘 ESP（Y:）bootmgfw.efi+BCD 在位；bcdedit /create /application BOOTMGR 被固件拒"
    "（类型参数无效）→ 改用复制既有固件项：{fwbootmgr} displayorder 逐项 /v 查详情，认出 "
    "{2d55a06e}=EFI USB Device（desc_hit），/copy → device partition=Y: + path bootmgfw.efi → "
    "/addlast 追加末尾。新项 {1300a859-b6a1-11f1-ab93-c4c6e62c8a4c}=VARIX Windows (USB)，"
    "内置 {bootmgr} 仍居首零改动。坑：/enum {fwbootmgr} 只出 manager 头不列子项详情，须逐项 "
    "/enum {guid} /v；bcdedit /create fwbootmgr 类型项不可行，/copy 同类型项可行。\n"
    "- 内置 Windows 自启动键已按用户要求删除（reg delete 验证不存在）。U 盘 Win11 Default "
    "模板 Run 键保留（交接落点）。\n"
    "- 内核新哈希 CBD113F8 已上盘（ktest 3147 全绿，spin_lock 单测为并行 flaky 单跑绿）。"
    "提交 292189e 已推。\n"
)
with open(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\.workbuddy\memory\2026-09-22.md",
          "a", encoding="utf-8") as f:
    f.write(note)
print("appended")
