# AI-S1 行数与检查项对账 · 深化批次（F171-F185）

> 口径：纯功能代码（不含测试/注释/空行——wc -l 全文件口径，与 S2/K1/K2 报告同源）。
> 判据唯一源：《Varix STAR I start.md》G-1 深化设计报告（G-G-01~G-G-15）。
> 检查项对账：每模块双层自检——主判据层（run_*_checks，对账主册判据第一句）+ 深化层（run_*_deep_checks，对账主册【设计细节】子句）。双层全部实算、零声明、零占位。

## 1. 行数对账（逐模块 · 诚实口径）

| 项 | 模块 | 首批行数 | 深化后行数 | 主册上限 | 达成率（深化后） |
| --- | --- | --- | --- | --- | --- |
| F171 | bootmenu.rs | 598 | 883 | 1,690 | 52.2% |
| F172 | selftestviz.rs | 542 | 790 | 3,250 | 24.3% |
| F173 | panicscreen.rs | 1,058 | 1,279 | 2,795 | 45.8% |
| F174 | diagsnap.rs | 633 | 792 | 1,625 | 48.7% |
| F175 | crashiso.rs | 557 | 721 | 2,080 | 34.7% |
| F176 | memguard.rs | 612 | 754 | 3,380 | 22.3% |
| F177 | capenforce.rs | 458 | 617 | 1,690 | 36.5% |
| F178 | signbadge.rs | 355 | 513 | 3,055 | 16.8% |
| F179 | permaudit.rs | 513 | 670 | 1,690 | 39.6% |
| F180 | pwrdrill.rs | 528 | 696 | 2,925 | 23.8% |
| F181 | handoffchk.rs | 469 | 671 | 3,120 | 21.5% |
| F182 | duoclock.rs | 433 | 594 | 2,145 | 27.7% |
| F183 | diskhealth.rs | 522 | 669 | 3,120 | 21.4% |
| F184 | hotplug.rs | 534 | 716 | 2,210 | 32.4% |
| F185 | romount.rs | 369 | 556 | 1,950 | 28.5% |
| — | mod.rs（域聚合） | 105 | 145 | — | — |
| **合计** | **16 文件** | **8,286** | **11,066** | **36,725** | **30.1%** |

深化增量 +2,780 行（+33.6%）；对齐团队深化先例（AI-V1 深化批 41.2%、AI-H2 批 +3,909 行）。

## 2. 检查项对账（双层 344 条 · 逐模块）

| 项 | 模块 | 主判据层 | 深化层 | 合计 | 深化层对账锚（主册【设计细节】子句 → 检查项） |
| --- | --- | --- | --- | --- | --- |
| F171 | bootmenu | 12 | 13 | 25 | limine.conf 解析（timeout/default/条目名/注释跳过/CR 兼容）· 条目超容截断 · 卡内图标格 48px+文字起 80px · 环帧角度 12k° 映射 · 资产分项账与总账同式 · 圆角软点阵 8 格 · 超时 0=无限等待 · default 越界钳回 · config→选择全链贯通 · 环帧 F173 复用锚 |
| F172 | selftestviz | 13 | 12 | 25 | kinfo 名册 9/11/10 项身份表（phys-map…zombie-ok…ext4-mount 逐名）· 名册-预期计数对拍 · F053 时间线节点导出（点亮+150ms 闭合/未亮零节点）· 环几何 64px/星徽 96px · 环帧绑定真实进度 ‰ · 开发态文字全输出（逐项 PASS 行/缓冲封口/未执行不出行）· 清屏协议联动 |
| F173 | panicscreen | 13 | 10 | 23 | 帮助篇映射表（注册/查询/未登记兜底 0/非法码兜底/16 上限）· dump 包头格式（VXDP 魔数/版本/帧号/字节数/地址/错误码+校验和，撕裂必拒）· 持久帧 round-trip（VXPR/累计次数/连续次数——≥2 安全模式询问数据面贯通）· 码→帮助链贯通 |
| F174 | diagsnap | 14 | 12 | 26 | 配置指纹（确定性/内容敏感）· F127 子集打包帧（四节区长度表/版本拒收）· 快照编号持久帧（round-trip/撕裂拒收/restore 链贯通）· 节流 10s 常量 · 磁盘紧张 20→5 档 · 栈 16 帧 · 热键常量 |
| F175 | crashiso | 12 | 10 | 22 | 遮罩中央卡三段齐（主册逐字文案/双钮位）· 视觉规格常量（20% 黑幕/卡 480×200/动画 200ms）· 焦点移交动画 150ms 时序 · 诊断导出行（人话四路径映射/事件序×终态复核）· 批量告警/参数容量常量 |
| F176 | memguard | 14 | 10 | 24 | 页号映射三态 · 护栏页号推算（块尾页内/页界）· 触碰判定与 access 对齐 · 毒值 0xDD 指纹 · 毒页填充模型 · F132 差异表豁免导出（留名保真/公开面=登记面）· 页宽 4KB · 六类故障身份 · 4096 上限 |
| F177 | capenforce | 10 | 12 | 22 | 规则注册/解析（通知「{规则名}」真话源）· 未注册降级 · 规则-执法点绑定 · 32 上限 · F194 序号链级联哈希（改/删/插断链）· 事件点敏感 · genesis 对齐 · 升档载荷（只升不降+依据数字/钳位）· 风暴常量 · 四执法点固定 · 执法播报解耦 |
| F178 | signbadge | 11 | 11 | 22 | 证书链验证模型（锚内根→链验/根不在→自签/断签→未签/零链超深→未签/服务坏→未签——fail-closed 链面）· 信任列表持久帧（round-trip/超容拒/接 resolver 贯通）· 任务栏同态（两处不分叉）· 角标位 -4px · 链深 8 上限 |
| F179 | permaudit | 11 | 9 | 20 | 能力枚举六值固定 · 能力人话名 · 授予事件语义化构造 · 导出行人话三源 · 日序保持 · 收回时间序（收回日后的授予必拒）· 收回执法贯通六能力全拒 · 授予史可回放 · 统计卡零值口径 |
| F180 | pwrdrill | 11 | 10 | 21 | 开放 JSON 归档（九字段键序稳定/合法 UTF-8/缓冲封口）· 8 周分布数据面（周序三值）· F061 错峰检查器（交叠如实报告+参数化可行性）· 归档路径锚 · F053 均时口径 · 双盲 ‰ 出口 · 三查枚举面 |
| F181 | handoffchk | 12 | 12 | 24 | 三查探测引擎（存在/哈希逐字节/回路）· RealGateProbe 注入适配（面板状态机吃实型引擎，绿通/红停贯通）· 面板渲染行三态图标（绿勾/红叉/灰问号）· 渲染全绿与状态机同源 · 面板几何 400×240 · F039 共用入口（同引擎同结果/红态透传）· 行序对齐闸门条件序 · 探测耗时进预算 |
| F182 | duoclock | 12 | 10 | 22 | F187 状态行三态文案（主册句式逐字）· HH:MM 格式化（14:32 样例/日界两端）· 带校验和帧（v1.1 追加扩展/round-trip/撕裂拒收/20B 帧长）· 置信度信任分级（NTP>RTC>推断）· 零漂移数字-结论一致 · 黄标透传 UI 面 |
| F183 | diskhealth | 13 | 10 | 23 | SMART 读取面（有主控/无主控 triage→健康页灰行贯通）· TBW 模型来源标注（P/E cycle）· 三层口径折叠说明（四行/逐行自洽/三态全覆盖）· 月键构造（跨年不碰撞）· 黄段提醒裁决（绿段零打扰/同月不重提/红段同规）· 错误行实测口径 · 块层提交语义声明 |
| F184 | hotplug | 12 | 10 | 22 | 插拔审计环（全链 1:1/时序回放/环满回卷）· toast 生命周期（5s 自动收+手动关——浮层完整出路）· 卷元数据三件套（盘符/卷标/容量）· GB 大数换算 · 卷标 16 封顶 · 事件语义联动（EjectedSafe/EjectedDirty）· SHARED 盘符审计 |
| F185 | romount | 13 | 10 | 23 | 挂载源 trait（B-705 注入口实型：策略型/可写快捷判定/未知卷诚实）· 「拷到 VARIX 区」动作条目（源路径+目标锚）· 动作队列 FIFO（进出序/8 上限满拒计数/空队 None）· 源路径 64 封顶 · 提示条替换不堆叠 · 帮助链+台阶钮双出路 |
| **合计** | **15+聚合** | **183** | **161** | **344** | 聚合器另按 15+15=30 行登记进两个域聚合（主/深），经 robust.rs 体检表单聚合行注册 |

## 3. 深化层新增功能面清单（按模块——全部真实逻辑，零包装层）

- **F171**：`parse_limine_conf`（limine.conf 唯一源解析器：三类行+注释+CR+超容截断）、`entries_from_config`（config→条目卡）、`ring_frame_angle_deg`（30 帧角度图集）、`AssetItemization`（资产分项账）、`corner_pixel_outside/corner_cut_pixels`（8px 圆角软点阵模板）。
- **F172**：`KinfoSuite` 名册（9/11/10 项身份表）、`item_done/item_pass` 项级位图（feed 置位）、`export_timeline_nodes`（F053 节点导出）、`build_dev_text`（开发态文字全输出——逐项 PASS/FAIL 行）、`ring_frame_from_progress`（进度环真实绑定）。
- **F173**：`HelpMap`（帮助篇映射表）、`build/verify_dump_header`（F020 dump 包头+校验和）、`encode/decode_persist`（跨重启持久帧——连续计数=防循环数据面）。
- **F174**：`config_fingerprint`（配置指纹）、`encode/decode_package`（F127 子集四节区打包帧）、`encode/decode_counter`（快照编号持久帧）+ `SnapshotStore::counter()` 对账口。
- **F175**：`MaskCard`（遮罩中央卡模型）、`focus_anim_done`（移交动画时序）、`DiagRow/export_diag_rows`（F120 诊断列表导出——人话路径映射）。
- **F176**：`page_of/guard_page_of/touches_guard_page`（页粒度护栏映射——PROT_NONE 语义面）、`poison_fill/is_poison_value`（0xDD 毒页模型）、`ExemptExportRow/export_exemptions`（F132 差异表公开面）。
- **F177**：`RuleRegistry`（规则名解析链）、`audit_chain_hash`（F194 级联链哈希——改/删/插断链）、`EscalateHint`（升档引导载荷——只升不降）。
- **F178**：`ChainVerifier`（证书链三态裁决——fail-closed）、`encode/decode_trust_list`（F037 信任持久帧）、`taskbar_hint_badge`（两处同态）、`BADGE_X_FROM_SYSBUTTONS`（乙-1 表位合成）。
- **F179**：`CapKind`（六能力枚举+人话名）、`AuditExportRow/export_rows`（审计段导出行）、`grant_after_revocation_denied`（收回时间序语义位）。
- **F180**：`week_row_to_json`（开放 JSON 归档——F128 键序稳定）、`WeeklyDist/weekly_distribution`（8 周分布面）、`night_window_conflicts_with_f061`（错峰检查器）。
- **F181**：`GateTarget`（三查探测引擎）、`RealGateProbe`（实型注入口适配）、`PanelRender/PanelRow`（面板渲染模型）、`game_shunt_precheck`（F039 共用实型入口）。
- **F182**：`SyncStatus`（F187 页状态行+HH:MM 格式化）、`encode/decode_snapshot_checksummed`（v1.1 校验和扩展帧）、`confidence_trust_rank`（信任分级参考面）。
- **F183**：`SmartReader` trait（主控指标注入面+NoSmart 灰行引擎）、`TBW_MODEL_CITATION`（来源标注）、`explainer_lines/explainer_tiers_consistent`（三层口径折叠说明+不许混装机器面）、`backup_toast_month_key/backup_reminder_due`（月频调度裁决）。
- **F184**：`PlugAudit`（插拔审计环——时序回放/环满回卷）、`ToastLifecycle`（浮层完整出路）、`VolumeMeta`（卷元数据+GB 换算）。
- **F185**：`MountSource` trait + `FakeMounts`（B-705 注入口实型）、`CopyFallbackJob/CopyQueue`（台阶动作队列——FIFO/满拒/空队）、提示条替换语义。

## 4. 深化批验证证据

| 口径 | 结果 |
| --- | --- |
| 隔离 crate 全量测试 | **84/84 全绿**（72 单测+2 域聚合+3 对账 dump+7 深化批新增用例——含 2 个新聚合全绿断言） |
| 主判据聚合 `run_secstar_checks` | 15/15 模块行全绿 |
| 深化聚合 `run_secstar_deep_checks` | 15/15 模块行全绿（161 条深化检查项全实算） |
| no_std `--features kernel-image` | 零错误 **零警告** |
| 主仓 `cargo check --lib` | secstar 零错误（并行分队在途错误不属本域） |
| 零堆纪律 | 16 文件零 alloc（深化层同样零堆——全部定长缓冲） |
| 死代码 | CONF_LINE_CAP 声明未用即删（不遮丑）——grep 自证零 TODO/零占位 |

## 5. 深化批施工伤账本（自抓自修）

| # | 现象 | 严重度 | 处置 |
| --- | --- | --- | --- |
| 1 | `entries_from_config` 数组 repeat 表达式要求 Copy（MenuEntry 32B 内联无 Copy） | 🟢 | `core::array::from_fn` 逐槽构建 |
| 2 | `MenuEntry::new` 的 `&'static str` 约束挡住 config 解析的动态 &str | 🟡 | 放宽为 `&str`（内部拷贝进定长缓冲——无生命周期外泄） |
| 3 | `KinfoSuite::new` const fn 内 IndexMut/min 不可 const 调用（静态名册编译不过） | 🟡 | 名册改 `&'static [&'static str]`（const 友好零拷贝） |
| 4 | dev_text 全输出断言硬编码行长 21（实际随项名变化） | 🟡 | 改逐行求和口径（6+名长+5+1） |
| 5 | dump 包头对拍切片 [16..29] 少一字节（码 14B 落 [16..30]） | 🟡 | 修正切片界 |
| 6 | `help_map_cap`/`rule_cap` 循环覆盖式断言（后续失败覆盖先前 true——逻辑假绿） | 🟡 | 改 `first_fail == Some(CAP)` 语义 |
| 7 | `cap_kind_six` 枚举序号断言写错值（Network=1 非 5） | 🟡 | 对齐枚举定义 |
| 8 | `f061_stagger_feasible` 跨日加法未取模（23:00+180min=1560>120 误判） | 🟡 | 模 1440 归一日界 |
| 9 | `copy_job_fields` 路径长度数错（"/doc/report.docx"=16 非 15） | 🟢 | 核对修正 |
| 10 | `diag_row_terminal_states` 事件序×终态映射错位（rows[1] 是 app11 强杀事件非 app12） | 🟡 | 事件序注释+断言重排 |
| 11 | `counter_restore_chain` 用 `last_id()`（ring 读取，空环 None）断言计数器值 | 🟡 | 新增 `counter()` 对账口（真实功能面） |
| 12 | `crashiso` 深化检查恒真式两处（`a!=b || (a&&b)` 型假断言） | 🟡 | 重写为真断言（双钮齐备/重启线完整） |
| 13 | `CONF_LINE_CAP` 声明未用 | 🟢 | 删除（allow(dead_code) 是遮丑不是清账） |
| 14 | 3 处 mut/未用变量告警 | 🟢 | 清零 |
