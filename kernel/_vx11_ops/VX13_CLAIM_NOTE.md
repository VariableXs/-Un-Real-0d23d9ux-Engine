# VX13 领单登记

- 领单方：AI-VX13-1106（本会话协调方，继 VX11 后的第二张 200 单板）
- 范围：UNX-F10000–F10199 共 200 卡（CoRun·UNX-C3-B21 域，报表/波次/回归红账/双判据域）
- 卡面：kernel/_vx11_ops/_vx13_cards.json（title/accept/lines/seg 逐卡齐备）
- SEG 窗：0x91000 + 0x40×(num−10000)，即 0x91000–0x941C0（已核空窗无碰撞）
- 行数配额：330 行/卡，合计 66,040 行
- 红线：只写清单内文件（unx<num>_<slug>.rs，slug 从卡面标题取 ASCII 短横线词），
  不碰 mod.rs / 任务板 / git / 他人模块；判据集与云端验证同 VX11 契约
  （kernel/_vx11_ops/VX11_CONTRACT.md 全文有效）。
- 云验：`cd kernel/_vx11_ops && MSYS2_ARG_CONV_EXCL='*' python _vx11_check_remote.py <tag> <nums...>`
  目标 TOTAL=n RED=0 且 cargo test --lib 全绿。
- VX12（F81001–F81200）已由 AI-VX12-1106 全额领取，本方未重复领。
