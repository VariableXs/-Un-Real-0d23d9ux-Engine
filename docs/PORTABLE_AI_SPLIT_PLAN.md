# Variable OS - 多AI并行独立完成计划

> 基于 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 30486字，拆为5个独立AI并行，0依赖冲突，全部在 `portable/` 和 `docs/` 独立目录交付。

> [!TIP]
> **总完成度：5个AI各管2-3章，全部 ✅ 即全系统完成，每AI右侧框打勾**

## 分工总览

| AI | 代号 | 负责章节 | 交付物 | 独立目录 | 依赖 | 状态 |
|---|---|---|---|---|---|---|
| AI-1 | 存储核 | 3.存储架构 + 9.性能寿命 | `portable/Create-VHDX.ps1` + `docs/AI1-存储.md` | `portable/AI1/` | 无 | ⬜ 待实施 |
| AI-2 | 隔离核 | 4.极致隔离7层 + 5.永不卡死6件套 | `portable/Test-VM.ps1` + `src-tauri/src/shell/isolation.rs` + `docs/AI2-隔离防崩.md` | `portable/AI2/` | 无 | ⬜ 待实施 |
| AI-3 | 兼容核 | 6.完全兼容5原则 + 7.真Windows三还原 | `src/system/compat/` + `docs/AI3-兼容体验.md` | `src/system/compat/` | 无 | ✅ 已完成(去包裹) |
| AI-4 | 拓展核 | 8.无限拓展 + 10.安全合规 | `Data/Apps` 符号链接 + MSIX + `docs/AI4-拓展安全.md` | `portable/AI4/` | 无 | ⬜ 待实施 |
| AI-5 | 交付核 | 11.测试验收 + 12.交付运维 + 脚本 | `portable/Deploy-To-USB.ps1` + `bench/` + `docs/AI5-测试交付.md` | `portable/AI5/` | 1-4 | ⬜ 待实施 |

> 规则：每AI只改自己目录，禁止改他人目录，通过 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 总表打勾同步进度。

---

## AI-1 存储核 独立计划

**目标：** 1TB 1000MB/s 双接口盘上实现 `150GB固定VHDX + 1700GB Data` 读写分离，寿命80年。

**输入：** `Win11_22H2.iso`
**输出：** 
- `portable/AI1/Create-VHDX.ps1` (已提供，需按1TB优化：SizeGB=150, AllocationUnitSize=64KB, CompactOS)
- `docs/AI1-存储.md` 2000字：VHDX动态/固定选型、差分链、Data符号链接清单

**验收：**
- [ ] `CrystalDiskMark SEQ≥900 4K≥20`
- [ ] `D:\Variable-USB\Variable-OS.vhdx` 150GB固定，`Optimize-VHD` 回收后 <20GB
- [ ] `mklink /D C:\Program Files\Blender D:\Data\Apps\Blender` 成功，VHDX不膨胀

**提示词：**
```
你是AI-1存储核，只做存储。在 portable/AI1/ 下完成 Create-VHDX.ps1，按 docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md 第3章实现，勿改其他AI目录。
```

---

## AI-2 隔离核 独立计划

**目标：** 7层隔离 + 大软件6件套，任意崩溃不卡死，10GB软件6秒可用。

**输出：**
- `portable/AI2/Test-VM.ps1` (Hyper-V 4GB/4核/限额/NAT/三桥全关)
- `src-tauri/src/shell/isolation.rs` (JobObject限额 + 超时熔断800ms + 看门狗3s)
- `docs/AI2-隔离防崩.md` 3000字：7层实现 + 假启动壳 + 按需分页

**验收：**
- [ ] 虚拟机内 `del C:\Windows` 宿主无影响
- [ ] `libcef 0x80000003` 崩溃仅弹Banner，主桌面60fps
- [ ] Blender冷18秒热5秒，可取消

**提示词：**
```
你是AI-2隔离核，只做隔离防崩。在 portable/AI2/ 和 src-tauri/src/shell/isolation.rs 实现第4-5章，勿碰存储/兼容目录。
```

---

## AI-3 兼容核 独立计划

**目标：** 任何软件都能打开，真Windows 1:1。

**输出：**
- `src/system/compat/` 已完成透明tile，后续补 `ShellExecuteExW` 代理 + `IContextMenu` 右键
- `docs/AI3-兼容体验.md` 2500字：5原则 + 像素/行为/系统三还原

**验收：**
- [x] 透明tile已落地 (✅)
- [ ] `ShellExecuteExW` 双击微信/Blender 100%成功
- [ ] 右键出现 7zip/Git 原生菜单

**提示词：**
```
你是AI-3兼容核，负责兼容体验。在 src/system/compat/ 实现第6-7章，已完成去包裹，继续补Shell代理。
```

---

## AI-4 拓展核 独立计划

**目标：** 无限拓展，50GB新软件拷进Data即用。

**输出：**
- `portable/AI4/MSIX-Attach.ps1` + `Data/Apps` 结构
- `docs/AI4-拓展安全.md` 2500字：层式VHDX + MSIX + 插件 + BitLocker

**验收：**
- [ ] `Copy-Item NewApp D:\Data\Apps\ + mklink` 即用，无需重做VHDX
- [ ] `MSIX` 挂载后开始菜单出现
- [ ] BitLocker To Go 拔盘自动锁

---

## AI-5 交付核 独立计划

**目标：** 一键交付，5台机任意电脑可用。

**输出：**
- `portable/AI5/Deploy-To-USB.ps1` + `bench/` 压测 + `docs/AI5-测试交付.md`
- 兼容矩阵 Top200 + 混沌注入

**验收：**
- [ ] 5台机A/B双模式各启动一次
- [ ] `bench/2026-09-07.md` 记录冷/热时间
- [ ] `Deploy-To-USB.ps1` 10分钟部署2TB盘

**依赖：** 等AI1-4的VHDX/隔离/兼容完成后再联调。

---

## 并行协作 Git 规范

```bash
# 每AI独立分支
git checkout -b portable/ai1-storage
# 只提交自己目录
git add portable/AI1/ docs/AI1-*.md
git commit -m "feat(ai1): 存储核 3+9章"
git push origin portable/ai1-storage
# 完成后合到 arena 分支，由交付核统一打勾
```

**进度打勾：** 每AI完成后，在 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 顶部总表对应行将 ⬜ 改 ✅，并在 `docs/PORTABLE_AI_SPLIT_PLAN.md` 本表打勾。

**今日开始：** AI1-4并行，AI5最后联调，1TB 1000MB/s双接口盘到后10分钟部署。

