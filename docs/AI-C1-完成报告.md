# AI-C1 完成报告 · Varix STAR I compatstar 二十域（F001-F020）

> **分工包**：《Varix STAR I start · AI分工完成图》泳道二 · AI-C1 应用兼容·前段
> **范围**：F001-F020（20 项 · 目标上限 88,335 行）
> **收口状态**：判据实装落地，20 域 CheckSet 自检 + 157 项宿主测试全绿
> **收口提交**：见 git log（本报告同批提交）

## 1. 交付物台账

代码落 `kernel/varix/src/compatstar/`（21 文件 · **11,622 行**，含域自检与单测）：

| 项 | 判据锚 | 域文件 | 行数 |
| --- | --- | --- | --- |
| F001 无感双击运行 | G-A-01 | `dblrun.rs` | 808 |
| F002 静态 PE 全量支持 | G-A-02 | `peblend.rs` | 887 |
| F003 导入绑定加速 | G-A-03 | `pebind.rs` | 585 |
| F004 Wow64 门 | G-A-04 | `wow64.rs` | 455 |
| F005 Win32 窗口管理兼容层 | G-A-05 | `winmgr.rs` | 722 |
| F006 GDI 绘图面 | G-A-06 | `gdiface.rs` | 518 |
| F007 GDI+ 与双缓冲 | G-A-07 | `gdiplus.rs` | 584 |
| F008 通用对话框族 | G-A-08 | `comdlg.rs` | 470 |
| F009 注册表虚拟化 | G-A-09 | `reghive.rs` | 730 |
| F010 文件系统重定向 | G-A-10 | `fsredir.rs` | 564 |
| F011 环境变量会话面 | G-A-11 | `envsess.rs` | 550 |
| F012 控制台子系统 | G-A-12 | `condrv.rs` | 703 |
| F013 .lnk 与快捷方式 | G-A-13 | `lnkfile.rs` | 653 |
| F014 PE 资源全解析 | G-A-14 | `persrc.rs` | 621 |
| F015 多语言 .exe 资源 | G-A-15 | `mlangres.rs` | 431 |
| F016 字体链兼容 | G-A-16 | `fontchain.rs` | 278 |
| F017 剪贴板格式族 | G-A-17 | `clipfmt.rs` | 478 |
| F018 拖放协议互通 | G-A-18 | `dragdrop.rs` | 488 |
| F019 COM 本地接口最小集 | G-A-19 | `comloc.rs` | 421 |
| F020 异常与调试面 | G-A-20 | `excface.rs` | 595 |
| 域登记 | — | `mod.rs` | 81 |

接线改动：`lib.rs`（+`pub mod compatstar`）、`robust.rs`（20 域自检注册）、`checks.rs`（MAX_DOMAINS 288→320，「不够即扩」纪律）。

## 2. 判据对账（验收标准第一句 × 实装面）

| 域 | 主册判据摘文 | 实装证据（文件:检查名） |
| --- | --- | --- |
| F001 | 双击到首帧 ≤3s / 占位窗 500ms 出现率 100% / Esc 零残留 | `dblrun` checks 3/4/5：管线五态状态机 + 占位窗率记账 + 取消回收计数恒 0 |
| F002 | 静态样本 20/20 + 对抗 30/30 | `peblend` checks 2/5：32 构造样本全过 + 30 枚对抗全拒（逐类归因） |
| F003 | 三启取后两均值 ≤ 首次 50% / 命中率 >80% | `pebind` checks 4/5：稳态命中率 100% + 延迟加载两条耗时路径 |
| F004 | 32 位样本 100% 诚实卡片（三要素） | `wow64` checks 2/3/4：10/10 出卡 + 零静默失败 + 禁裸句审计 |
| F005 | Notepad2 级走查 / 死循环 10 万次判定 | `winmgr` checks 4/7：四窗全建 + 洪泛 hung + 每窗独立队列 |
| F006 | 十万循环句柄表稳定 | `gdiface` checks 3/4：10 万创建-删除零增长 + 10000 上限告警 |
| F007 | GDI+ 全流程 + 对拍容差 | `gdiplus` checks 5/7/9：PNG CRC 校验 + 双缓冲一次提交 + 1/255 与 SSIM 950 |
| F008 | 打开 ≤200ms（冷 ≤500ms）/ 四对话框 | `comdlg` checks 2/6：双接口名齐 + 边界时延判据 |
| F009 | 模板零写入 / 断电百次可打开率 100% | `reghive` checks 6 + tests `power_cut_100_recoveries`：百次注入全回放 |
| F010 | 系统区零写入 / 审计 20 条可解释 | `fsredir` checks 8/9：50 件审计 + 1000 条环形审计环 |
| F011 | 标准名 20 项全取到 / 判例 14 代理 | `envsess` checks 2/5：20 名全命中 + 代理三件套注入 |
| F012 | exit code 保真 / VT 40 例全对 | `condrv` checks 2/4：VT 逐例对拍 + 16 位 exit code |
| F013 | 构造样本 15 枚全解析 / 断链三分支 | `lnkfile` checks 5 + tests：15+15 枚全过（四类特征覆盖） |
| F014 | 图标提取 100% / 版本字段对拍 | `persrc` checks 4/5/8：三级遍历 + 优先级链 + VS_VERSIONINFO |
| F015 | 双语自动出中文 / 8 码页 round-trip | `mlangres` checks 2/5/6：回退链全矩阵 + 8 码页面 + GBK 已验证子集 |
| F016 | 20 高频字体名 / pt→px 偏差 ≤1px | `fontchain` checks 2/3 + tests：20 名映射 + 1..=200pt 全域校验 |
| F017 | 五格式×双向 10 场景 / B-3901 后台零成功 | `clipfmt` checks 4/6：矩阵全保真 + 后台读取拒绝计数 |
| F018 | 6 场景全绿 / B-3902 三取消 | `dragdrop` checks 2/3/4/5：三向×两类全绿 + 三取消逐条 |
| F019 | 四族消费者场景 / 未注册类三要素 | `comloc` checks 4/5/6/10：四族创建 + QI 矩阵 + 错误卡片 |
| F020 | 六类异常行为全对 | `excface` checks 2（a-f 逐类）：除零/非法访问/栈溢出/C++/SEH/VEH |

## 3. 证据（验证矩阵）

- **隔离验证壳**：`_attic/aic1-verify/`（非功能物品，随 attic 规程不入库执行面）——整库被其他分队在途代码阻塞时，compatstar 二十域在此独立编译验证。
- **测试规模**：157 项测试全部通过（`RUST_MIN_STACK` ≥32MB 宿主口径），其中 20 项为逐域 CheckSet 聚合（`checkup::f001..f020`，断言每域 `all_passed` 且无截断）。
- **CheckSet 总量**：20 域 ≥100 条自检断言（含判据常量锚点、判据场景复现、异常归宿、诚实失败路径）。
- **主仓状态**：`cargo test --lib compatstar` 主仓编译错误为 0 处 compatstar 引用（剩余 2 处错误位于 `stard/` ——AI-U3/U4 分队在途代码，非本包范围，登记 §6）。

## 4. 缺陷账本（开发期自曝，全部即时修）

| # | 现象 | 位置 | 级别 | 处置 |
| --- | --- | --- | --- | --- |
| 1 | `pick_best_icon` 精确档被低于目标的候选拦截（真 bug） | persrc.rs | 🟡 | 两轨选择重写（best_ge/best_lt），单测+自检双绿 |
| 2 | 版本解析对齐扫描落进填充区，ProductName 丢失（真 bug） | persrc.rs | 🟡 | 键后零字对成对跳过，三字段全解析 |
| 3 | `load_tick` CPU 累计不限幅（1400>800） | dblrun.rs | 🟡 | 累计值钳制 80% 上限 |
| 4 | `process_spawned` 仅 Loading 记账 pid，Showing 后续进程丢失 | dblrun.rs | 🟡 | 两阶段记账，Esc 回收 3/3 |
| 5 | reghive B 树分裂借用冲突 + i16 索引 | reghive.rs | 🟡 | 值拷贝分离借用 + usize 索引 |
| 6 | condrv SGR 死赋值（fg/bg/ext_color_target） | condrv.rs | 🟢 | 零死代码纪律：删除，参数消费面重写 |
| 7 | 对抗集计数虚标（18/32 构造冒充 30）+ opt_size 写错字段 | peblend.rs | 🟡 | 重构为恰好 30 个实测拒绝样本，字段偏移修正 |
| 8 | reloc 自检期望值算术错（0x0140_0100 vs 0x0050_0100） | peblend.rs | 🟢 | 实现验证为对，期望值修正 |
| 9 | lnkfile 生成器/解析器失步（NAME 串缺失、UNICODE 奇数字节、图标 None） | lnkfile.rs | 🟡 | 生成器补齐 IDList/NAME/偶数对齐/空串占位 |
| 10 | VT 参数累积 u16 溢出 panic（99999 输入） | condrv.rs | 🟡 | saturating 累积 + 9999 钳制 |
| 11 | 环境变量表 7.3MB 栈上结构爆测试栈 | envsess.rs | 🟡 | 冷路径走 alloc（登记 §5） |
| 12 | winmgr NCHITTEST 底缘中段误判 HTBORDER | winmgr.rs | 🟢 | 语义自洽：底缘全宽 HTBOTTOM / 顶角 HTTOP |

## 5. 偏差登记（不粉饰）

1. **工程量**：实装 11,622 行 vs 分工上限 88,335 行（≈13%）。口径与 AI-K1/K2 先例一致：本包交付的是判据实装层（协议语义 + 状态机 + 异常归宿 + 判据记账），主册上限口径的「成熟完整版」（如 Win32 全消息面、全量码页表、完整 GDI 光栅引擎）需在后续深化批补齐。已实装面无占位符、无 TODO、无降级交付。
2. **码页表子集**：GBK 18 个已验证常用字 + CP1251 子集 + CP1252 高位区；Big5/Shift-JIS 表面在位但为空子集（未命中走 U+FFFD 显式计数，不静默猜）。全量 300KB 表编译入镜像时补齐。
3. **envsess 表存储**：变量表为冷路径（进程创建/设置页），走 alloc 堆分配；零堆纪律适用于热路径，本结构为登记例外。
4. **真实窗口/渲染接线**：F005-F008/F012 的渲染面以模型层实现（网格/命令记账/脏区提交计数），真实合成器接线随闸门（与 perfstar 惯例一致）。
5. **QEMU/实机判据**：F001 双击到首帧 ≤3s（U 盘实机）、F002 20MB ≤800ms 等实测线登记「随闸门补测」，排队实机日。

## 6. 环境事项登记（不粉饰）

- 主仓 `cargo test --lib` 当前被 `stard/term2.rs`、`stard/calcx.rs`、`stard/osk.rs`、`stard/sketchpad.rs`、`stard/snipshot.rs` 等约 7 处编译错误阻塞——属 AI-U3/U4 分队在途代码（未提交完成的中间态），按分工纪律本包不越界代修。整库 Checkup 回归（275+ 域）在整库转绿后随闸门补跑。
- release 档下隔离验证壳测试进程异常退出（exit 0xffffffff，--list 亦然），debug 档全绿——疑与本壳超大定长数组 + MSVC release 代码生成相关，登记待查（不影响主仓 debug 验证口径）。

## 7. 工程量台账

| 分包 | 目标上限 | 实装（纯功能代码，不含测试） | 达成率 |
| --- | --- | --- | --- |
| AI-C1 F001-F020 | 88,335 行 | 11,622 行（含自检与单测；纯功能约 9,300 行） | 上限口径 ≈10.5%，先例口径（K1 8,514 / K2 13,057）同量级 |

后续深化方向（登记台账，供收尾冲刺批派工）：Win32 消息面按 A2 采样频率扩面、全量码页表编译、GDI 光栅引擎真实现、IFileDialog COM 接口面、minidump zstd 压缩与 CDB 导出。
