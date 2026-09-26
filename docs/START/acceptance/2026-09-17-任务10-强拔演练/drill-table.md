# 任务10 强拔演练（QEMU 先行等价验证）—— 2026-09-18 01:33

- 三阶段（boot/menu/running）各 ×3 轮；拔盘 = HMP drive_del；
- boot 阶段拔 cdrom0（引导介质本体，等价 U 盘一体拔出）；
- menu/running 拔 nv1（数据盘）；恢复 = 重建同内容盘重新引导至 lifecycle complete；
- 异常词扫描：PANIC/panic/fatal/#DF/triple fault/rebooting。

| 阶段 | 轮 | 动作 | 结果 |
|---|---|---|---|---|
| boot | 1 | drive_del ide2-cd0=OK | 观察 8s：零 panic/零 fatal |
| boot | 1 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| boot | 2 | drive_del ide2-cd0=OK | 观察 8s：零 panic/零 fatal |
| boot | 2 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| boot | 3 | drive_del ide2-cd0=OK | 观察 8s：零 panic/零 fatal |
| boot | 3 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| menu | 1 | drive_del nv1=OK | 观察 8s：零 panic/零 fatal |
| menu | 1 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| menu | 2 | drive_del nv1=OK | 观察 8s：零 panic/零 fatal |
| menu | 2 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| menu | 3 | drive_del nv1=OK | 观察 8s：零 panic/零 fatal |
| menu | 3 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| running | 1 | drive_del nv1=OK | 观察 8s：零 panic/零 fatal |
| running | 1 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| running | 2 | drive_del nv1=OK | 观察 8s：零 panic/零 fatal |
| running | 2 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |
| running | 3 | drive_del nv1=OK | 观察 8s：零 panic/零 fatal |
| running | 3 | 恢复 | 重建盘重新引导：lifecycle complete 达成（可恢复） |

## 结论

- 9 轮拔盘全部执行；恢复失败 0 轮（如实登记，见逐轮表）。
- boot 阶段为引导链边界探测：拔引导介质本体，若 SeaBIOS/Limine 已完成
  全量装载则引导继续；若介质读取仍在窗口内则引导中止——两种行为均为
  引导器如实表现，不粉饰；恢复轮验证重建后引导链完整。
- 实机 U 盘强拔（物理拔出）登记为待用户协作项：QEMU drive_del 等价
  语义为"介质即刻消失"，与物理拔出在 NVMe/USB 控制器错误路径上存在
  差异，实机验证以 U 盘实插实拔为准。