# Varix STAR I · 上游借力件登记册（upstream-registry）

> **制度出处**：MD1 第 6 章补（集成协议）与 18.4（上游关系协议）；承接 MD3 任务 T-302——"所有借力件登记在 `docs/upstream-registry.md`（版本、许可、补丁集、跟进人、节奏）——上游 Registry 是借力战略的仪表盘"。
> **登记日**：2026-09-25（WP-405 收官后补齐，台账"外部清单对账"段指认的唯一硬缺口）。
> **维护纪律**：季度升级窗（每年 1/4/7/10 月）内逐件复核本表；窗口外锁死版本，禁止顺手升级；修改需立案（Fork Proposal）；回馈义务（垫片发现上游缺陷必须提 issue）。

## 一、锁定状态分层（诚实登记）

本册逐件标注三层锁定状态，**不把计划面写成实装面**：
- **L1 已锁定实装**：锁定常量或依赖 pin 在仓库代码/构建面可 grep；
- **L2 锁定登记**：锁定版本号在本册与 MD2 登记在案，crate 接线随包内窗口；
- **L3 计划锚**：集成协议已定（MD1 第 6 章补），版本号待接线窗口开启时锁定——**接线前不锁号，锁号必须进本表**。

## 二、登记册（按 MD1 6.2 总表 + 18.4 补充逐件登记）

| # | 资产 | 角色 | 许可 | 锁定状态 | VARIX 侧落位 | 升级节奏 | 补丁集 | 跟进人 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Wine / Proton | Win32 替身库（exe 兼容） | LGPL 2.1+ | **L1**：`WINE_LOCK = LockEntry { component: "wine", locked_ver: 9 }`（`wineshim.rs` 行 124，reshim_needed 按版本漂移触发四路重校验）+ `WINE_SOURCE_DIFF_LINES = 0`（零侵入是常量不是口号，B-1003） | 支架五模块：winecare（看门狗）/winepfx（前缀模板）/wineshim（四路垫片）/winecase（判例流水线）/wineattr（评级）；本体二进制树随 `/opt/wine/` 实机接线 | 季度窗，跟随上游 stable；补丁数目标 < 20 文件（当前 **0**——垫片全在 VARIX 侧） | 零 | AI01（宿主）/ Variable（实机） |
| 2 | Servo | Web 渲染引擎（桥接级承载） | Apache-2.0 / MIT | **L3**：接线前不锁号；集成协议已定（Rust 依赖树按 crate 分层取用：渲染/JS/网络） | winsurf 提交面（display list → 合成器）+ 输入注入 + 网络走 smoltcp；feature flag 隔离实验能力 | 双周评估 nightly 精选版（MD1 第 6 章补），接线窗口随桥接级应用面 | 目标零（VARIX 侧适配） | AI01 |
| 3 | relibc | POSIX libc（翻译柜台伴生层） | MIT / Apache-2.0 | **L3**：源码级融合（Rust 同门），syscall 抽象点逐个对准 VARIX 语义 | lxgov/lxerrno/lxrun 转译层的用户态伴生面；relibc 自带一致性测试套全量跑绿为验收线 | 接线窗口随 Linuxulator 实机 LTP 面 | 源码级融合 = 后端适配非 fork | AI01 |
| 4 | smoltcp | TCP/IP 栈（内核网络栈） | MIT / Apache-2.0 | **L3**：网络面实机接线窗口（netthr/usbnet/dohsw 宿主模型面已备） | gnet/net 域集成；转译柜台网络调用改名直通的对端 | 季度窗 | 目标零 | AI01 / Variable |
| 5 | rustls | TLS | Apache-2.0 / MIT | **L3**：随 smoltcp 接线窗口 | 网络栈 TLS 组件（转译柜台 17.4 面） | 季度窗 | 目标零 | AI01 |
| 6 | cosmic-text | 文本排版（编辑器/桌面字排） | MIT / Apache-2.0 | **L3**：随字排实装窗口（fontsub 覆盖位图/回退链宿主面已备） | edcore/fontsub 的字排引擎位 | 季度窗 | 目标零 | AI01 |
| 7 | iced / egui（二选一） | GUI 框架基座候选 | MIT / Apache-2.0 | **L3**：候选评估期不锁号（sdkwidgets 控件基类面自持，框架选型随原生应用框架窗口） | vx-SDK 控件层的候选基座 | 季度窗评估 | 目标零 | AI01 |
| 8 | Mesa（iris / llvmpipe） | GPU 驱动（远期）/ 软渲染保底 | MIT | **L3**：R3 攻坚窗口（S406 立项包承接——UHD 630 走 iris 高优，llvmpipe 退役为无 GPU 回退） | r3scan 硬件清单固化 + SubmitBackend trait 双后端接口位（CpuBackend 实际 + GpuBackendStub）；path3/wingl/esoft 提交面 | 季度窗（R3 期起） | 目标零（DRI 管线适配层在 VARIX 侧） | AI01 / Variable |
| 9 | 上游 Linux 发行版仓库 | 软件池（数万包二进制直插） | 各包自带（用户态） | **不适用锁定**：软件池按星图判例逐包评级（starmapdir/starmapgate 门禁） | 直插级应用经 Linuxulator 转译柜台接入；**GPL 内核代码零接触**（转译层为 VARIX 自有实现，WSL1/FreeBSD Linuxulator 同构立场） | 随星图收录节奏 | 不适用 | AI01 |
| 10 | ReactOS | Win32 实现参考 | GPL | **只读参考，零链接**（许可红线：GPL 参考不进 VARIX 代码树） | 设计参考面（Wine 支架与判例设计引用） | 不适用 | 不适用 | AI01 |
| 11 | ext4_rs | ext4 只读实现（y5/ext4_rs，crosvm 同源） | Apache-2.0（crosvm 系） | **L2**：`=1.3.3` 锁定登记（MD2 行 503——crates.io 同名 `ext4` crate 为另一只读项目**非目标**）；crate 实际接入验证随 BlockDevice trait 对接块层窗口 | fswl/fsyncp 存储面的文件系统引擎位 | 季度窗（crosvm 同源跟进） | 目标零 | AI01 |
| 12 | FFmpeg | 音视频解码（软解标定锚） | LGPL 2.1+（**动态链接、独立进程**——B-805 登记锚） | **L2**：独立进程边界 + 动态链接形态登记（viddec.rs）；版本号随分发镜像锁定 | viddec 两核上限吞吐模型（H.264/H.265 实时、AV1 如实不承诺）的消费端 | 季度窗 | 目标零 | AI01 |
| 13 | Limine | 引导器 | BSD-2-Clause | **L1-锚**：引导配置面在库（limine.rs/bootcfg.rs/bootselect.rs，菜单两项 VARIX/Windows）；版本号随发布镜像指纹登记（reprobuild 可复现四件套） | 引导菜单/防自锁闸门/双槽回滚选单的承载者 | 随发布镜像 | 菜单配置模板（零源码补丁） | AI01 / Variable |
| 14 | QEMU + OVMF | 三层测试环境第一层 | GPL 2.0（QEMU）/ BSD（OVMF） | **L1-锚**：测试工具链（35.2 第一层）；qemuenv 四替身注入框架的对练环境；版本随 CI 指纹 | qemuenv.rs（InjectSpec 四替身/PowerDrill 百次断电/VTime/HandoverDrill）+ varix-qemu.iso 产线 | 随 CI 窗口 | 零 | AI01 |

**件数口径**：MD1 6.2 总表 10 件 + 18.4 补 ext4 crate + 工具链侧 Limine/QEMU-OVMF + 解码 FFmpeg = **14 件登记**（用户侧"十三件"口径以本表逐件清单为准对齐）。

## 三、运营侧四条协议（MD1 18.4 照录为本册执行条款）

1. **版本锁定表公开**：本表即公开面，随星图"关于"页展示（18.4 第 1 条——"我们站在谁的哪个版本上"随时可查）。
2. **季度升级窗**：每年 1/4/7/10 月；窗口内完成升级评估、回归测试（reggate 红即阻断）与星卡批量复评；窗口外锁死版本。窗口动作逐件记录进本表"锁定状态"列。
3. **修改需立案**：任何上游改动先立 Fork Proposal 说明为何不能在 VARIX 侧包垫片；批准后修改独立成 patch 集维护，禁止改源码树内副本。当前全部 14 件补丁集状态 = 目标零（Wine 已实证零侵入）。
4. **回馈义务**：垫片发现上游缺陷必提 issue；patch 能上游的上游，减少长期分叉。

## 四、登记册与判据台账的勾稽

| 本表条目 | 判据/CheckSet 勾稽点 |
| --- | --- |
| Wine 行 | B-1001~1007（WP-302，winecare/pfx/shim/case 七判据）；WINE_LOCK 实装见 wineshim.rs CheckSet "版本一致零重校验/漂移四路全重校验" |
| ext4_rs 行 | WP-102 存储面锁定纪律（MD2 行 503 施工要点"白名单与版本锁定同日落地"） |
| FFmpeg 行 | B-805（WP-208 viddec，LGPL 动态链接登记锚） |
| Mesa 行 | B-804（WP-208 r3scan）+ S406 立项包（WP-405 文档第二件） |
| QEMU+OVMF 行 | B-4101~4104（WP-404 qemuenv）；断电百次 DRILL-OK（m1 收账） |
| Limine 行 | B-301~307 引导链（WP-103）+ 防自锁闸门（B-706） |
| 软件池/ReactOS 行 | 转译柜台白名单（B-401~407）+ 许可红线（6.3 不碰 GPL 内核代码） |
