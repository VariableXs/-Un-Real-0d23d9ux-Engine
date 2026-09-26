# -*- coding: utf-8 -*-
"""追加 2026-09-22 交接目标修正笔记到工作日志。"""
note = (
    "\n## 23:48-00:05 交接目标修正（用户实测仍进内置 Windows）\n"
    "- 根因：bootopt DEFAULT_HANDOFF_TARGET=Internal，boot-select.json 未布 handoff_target/"
    "usb_windows_esp_guid；handoff.rs 对 Internal 目标 guid_ref=None → "
    "resolve_windows_entry_for 任意匹配 BootOrder 里的 Windows 项 = 内置盘 → BootNext 写到"
    "内置 Windows。S1.3 的 usb GUID 匹配机制在但从未被配置激活。\n"
    "- 修复：boot-select.json 增 handoff_target=usb + usb_windows_esp_guid="
    "{636786cb-e967-49f6-b0df-7608909d1f11}（U 盘 ESP disk1part1）。坑：SHARED 卷旧配置被"
    "部署脚本视为真相源会盖掉 repo 种子——先提权 diskpart 挂 T: 用 shutil 直写 SHARED 根"
    "（cmd /c copy 引号转发损坏，改 Python 原生复制），回读哈希 3485967A 一致后再跑部署，"
    "ESP 副本同步同哈希。\n"
    "- 卷标实况：S=VARIX_SYS(63G)、X=WIN_ENGINE(285G USB Win11 系统卷)、V=SNAPSHOT、"
    "SHARED=exFAT 554G 无盘符。提交 3d84b61 已推。\n"
)
with open(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\.workbuddy\memory\2026-09-22.md",
          "a", encoding="utf-8") as f:
    f.write(note)
print("appended")
