# AI-K1 检查项对账表 · perfstar 17 域 CheckSet × 主册判据（F041-F057）

> **对账口径**：本文把 `kernel/varix/src/perfstar/` 全部 **541 项 CheckSet 检查项**（宿主同构 f475 入口 tally 口径，2026-09-26 深化批次三后）逐条映射到主册《Varix STAR I start.md》B-3 深化设计报告（G-B-01~G-B-17）的**判据原文 / 设计细节 / 状态与异常 / 数据与存储 / 交互设计**锚点。一处一事实：每项检查都能回答「它在验主册的哪句话」。
> **验证状态**：541/541 全绿（隔离验证舱 `_attic/aik1-f041-f057-deep-verify/`，239 宿主单测同绿）。
> 域表注册：`robust.rs` 274 域函数指针表；批次沿革：v1 判据实装（156 项）→ 深化批次一（+10）→ 深化批次二（+11）→ **深化批次三（+364，见 §后半部新增对账）**。
> **容量纪律**：`CheckSet::MAX_CHECKS = 64`，单域检查项超限会被**静默丢弃**（`add` 满时只记 dropped）。深化批次三按此拆段挂载——十二查登记册（12 项）挂 F041、用户故事场景拆前段 23 项挂 F041 / 后段 13 项挂 F057，故 F041=57、F057=40，**十七域无一触顶、零截断**（实测逐域计数见 §深化批次三）。

| 域 | 判据锚 | CheckSet 项数 | 其中批次三新增 | 全绿 |
| --- | --- | --- | --- | --- |
| F041 frameledger | G-B-01 | 57 | +48（深化件 13 + 十二查 12 + 场景前段 23） | ✅ |
| F042 frameattr | G-B-02 | 32 | +17 | ✅ |
| F043 startprof | G-B-03 | 25 | +15 | ✅ |
| F044 prefetch2 | G-B-04 | 27 | +18 | ✅ |
| F045 pagewater | G-B-05 | 32 | +20 | ✅ |
| F046 wcoalesce | G-B-06 | 27 | +19 | ✅ |
| F047 latbudget | G-B-07 | 23 | +15 | ✅ |
| F048 cpufreq | G-B-08 | 31 | +21 | ✅ |
| F049 idlezero | G-B-09 | 30 | +19 | ✅ |
| F050 intrcoal | G-B-10 | 22 | +14 | ✅ |
| F051 bigpage | G-B-11 | 27 | +17 | ✅ |
| F052 heapfrag | G-B-12 | 37 | +21 | ✅ |
| F053 bootpar | G-B-13 | 27 | +18 | ✅ |
| F054 imgsimd | G-B-14 | 35 | +26 | ✅ |
| F055 glyphcache | G-B-15 | 35 | +23 | ✅ |
| F056 dirtyrect | G-B-16 | 34 | +24 | ✅ |
| F057 iotier | G-B-17 | 40 | +29（深化件 16 + 场景后段 13） | ✅ |
| **合计** | | **541** | **+364** | **✅** |

---

## F041 帧率账本（G-B-01）· 9 项

主册判据：**账本打点自身开销 <0.1% CPU 实测；帧率图与实测录屏逐帧对得上（抽 10 帧核对）。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| cap_64kb | 【数据与存储】逐帧环形缓冲内存 64KB | 环容量 = 65536/10B = 6553 条硬预算 |
| sample10_exact | 验收判据「抽 10 帧核对」 | 10 帧已知跨度逐帧读回精确一致 |
| self_cost_below_01pct | 验收判据 <0.1% CPU | 打点开销模型 permille < 1 |
| over_budget_downsample | 【状态与异常】超预算自动降采样每 2 帧记 1 帧 | 触发 + 比例 = 2 + 标注位 |
| fps_lines | 【交互设计】80fps 实线 12.5ms / 60fps 虚线 16.6ms | 双线常量精确 |
| segment_thresholds | 【设计细节】独立阈值合成 6ms/提交 3ms/输入 1.5ms | 三段独立红线各自触发 |
| break_not_faked | 【状态与异常】崩溃标注断点不伪造连续 | series 断点 = None |
| minute_agg | 【功能定义】24 小时分钟聚合 | 均值/峰值/超线帧数/事件率精确 |
| adaptive_ratio_1000fps | 【数据与存储】60s×1000fps 上限 | ratio = ceil 保 60s 窗口 |

## F042 帧率归因器（G-B-02）· 15 项

主册判据：**构造四类掉帧样本各一，归器全部命中正确主因；误报率 <10%（正常拖动 100 秒样本零误报）。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| hit_input_storm | 【设计细节】输入事件 >200/秒 | 样本一命中输入风暴 |
| hit_big_dirty | 【设计细节】单帧脏区 >屏幕 60% | 样本二命中大脏区 |
| hit_io_block | 【设计细节】帧内 IO 等待 >2ms | 样本三命中 IO 阻塞 |
| hit_sched_preempt | 【设计细节】帧被抢占 >3 次 | 样本四命中调度抢占 |
| zero_fp_normal_drag | 验收判据误报率 <10% | 正常拖动 12000 帧零归因（结构保证） |
| mixed_no_hardcode | 【状态与异常】多因并发如实混合归因 | 无一回线 → Mixed 按占比降序 |
| gate_over_budget | 【设计细节】帧超预算才归因 | 门 = 12.5ms（80fps 线） |
| silent_disable | 【状态与异常】归因器异常静默停用 + 诊断报备 | 停用后 analyze 恒 None + 原因在册 |
| evidence_by_ref | 【数据与存储】证据引用账本条目不复制 | 事件只持 frame_seq 链接 |
| unclassified_honest | 【状态与异常】不硬编主因 | 四类未达阈 → Unclassified 诚实计数 |
| retention_7d | 【数据与存储】保留 7 天 | 按天计数环 7 槽 |
| evidence_bridge_same_source | 【数据与存储】不复制一处一事实（深化一） | from_ledger_frame 与账本帧逐位同源 |
| wake_storm_evidence | G-B-09【状态与异常】唤醒风暴归因报告给 F042（深化一） | 风暴证据入环 + 升序 |
| storm_evidence_survives_disable | 同上 + 静默停用不停观测（深化一） | 停用态照收证据 |
| attr_breakdown | 【交互设计】四类嫌疑占比条形图（深化二） | 贡献数组精确 + 最大余数法占比和 = 1000‰ |

## F043 冷启动画像（G-B-03）· 10 项

主册判据：**五段边界与内核打点一一对应（无重叠无遗漏）；同应用 10 次启动画像方差 <15%（测量自身稳定）。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| five_segments | 【设计细节】五段刻度定义 | 装载/重定位/首帧/可交互/稳定 |
| contiguous_ok | 验收判据五段边界一一对应 | 段间时间戳连续无重叠 |
| gap_rejected | 同上（无遗漏） | 时间倒退/断档拒绝 |
| stable_window_5s | 【设计细节】稳定止于 5 秒观察窗结束 | STABLE_WINDOW_MS = 5000 |
| abort_marked | 【状态与异常】启动中途崩溃标记中止于第 X 段 | record_abort 在档 |
| privacy_gate | 【设计细节】画像收集受隐私总闸（F036 同开关）控制 | 置闸后丢弃 + 计数 |
| median_second_launch | 【数据与存储】星卡取二次启动中位数 | seq≥2 才计入 |
| cv_below_15pct | 验收判据 10 次启动方差 <15% | 变异系数判据线 |
| compare_view | 【交互设计】首次 vs 二次对比视图 | compare_first_second 并排数据 |
| history_20_cap | 【数据与存储】保留最近 20 次启动 | LRU 上限 20 |

## F044 预取指纹 v2（G-B-04）· 9 项

主册判据：**「常用 50 件」二次启动均值 ≤ 首次 50%；指纹命中率 >70% 实测。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| bitmap_order | 【功能定义】页位图 + 访问序 | 位图/顺序区双结构在位 |
| plan_recency_first | 【设计细节】热页先读 | 预读计划按最近访问序 |
| roundtrip | 【数据与存储】cache/prefetch/<hash>.pf 序列化 | serialize→deserialize 精确往返 |
| corrupt_discard | 【状态与异常】指纹损坏弃用走无预取路径 | checksum 不符 → None（正确但慢） |
| version_invalidate | 【状态与异常】版本变化指纹失效重建 | note_version 失效在档 |
| generate_delay_30s | 【设计细节】应用退出后 30 秒后台生成 | GENERATE_DELAY_MS = 30_000 |
| hit_rate_100pct | 验收判据指纹命中率 | 预读命中记账精确 |
| lru_evict | 【数据与存储】上限 8MB/应用 LRU 驱逐 | 超限逐出最旧应用 |
| priority_below_fg | 【设计细节】预读优先级永远低于前台 IO（F057 分级） | PRIORITY_NOTE 语义在案 |

## F045 页缓存水位策略（G-B-05）· 12 项

主册判据：**断电百次中脏页丢失窗口 ≤ 水位规则承诺值；压测（连续开关大图）无 OOM。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| watermarks | 【功能定义】三档高 3.2GB/中 2.4GB/低 1.6GB | 三档常量精确 |
| tier_switch | 【设计细节】三档切换条件文档化 | >1GB 高档……全表判定 |
| poll_cadence_500ms | 【设计细节】水位判定每 500ms 一次（功耗账） | 节拍内重复 poll 零动作 |
| lru_evict | 【功能定义】到线按 LRU 回收文件页 | 最旧先出 + 字节账同步 |
| dirty_cap_20pct | 【功能定义】脏页占比上限 20% 超限强制冲刷 | 200‰ 上限触发冲刷 |
| shared_ro_excluded | 【设计细节】SHARED 只读卷不计脏页 | 分区标记豁免 |
| oom_warn_edge | 【设计细节】内存耗尽前 200MB 警戒 → 冲刷 + 通知后台释放 | 边沿触发不骚扰 |
| regret_metric | 【状态与异常】回收后悔指标调参依据 | 重读率 permille 精确 |
| anon_not_reclaimed | 【设计细节】LRU 文件页/匿名页分列（匿名页不进回收） | 匿名页不可弃 |
| tri_area_sums_1000 | 【交互设计】三区图应用/缓存/空闲面积图（深化二） | 累计差分 permille 和恒 1000 |
| quota_yield | 【状态与异常】应用配额挤压缓存 → 缓存先让（深化二） | 置旗 → 多让 25% + 计数 |
| quota_yield_one_shot | 同上（让路是单次动作非常态降档） | 二次 poll 不再让 |

## F046 写合并窗口自适应（G-B-06）· 8 项

主册判据：**断电百次按三档各跑一遍全绿；写入合并率重载档 >40% 实测；fsync 延迟 P99 <10ms 不受档位影响。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| three_windows | 【功能定义】空闲 1s/正常 5s/重载 8s 三档 | 窗口常量精确（8s = 断电承诺） |
| tier_thresholds | 【设计细节】<10/s 且 <5% 空闲；>200/s 或 >15% 重载 | 双信号判定 |
| dwell_hysteresis_5s | 【状态与异常】档位抖动 → 迟滞驻留 5s | 双向驻留 + 抑制计数 |
| switch_audited | 【设计细节】档位切换写一行审计 | 审计环行精确（从/到/信号） |
| merge_rate_heavy_over_40 | 验收判据重载档合并率 >40% | ≥400‰ |
| fsync_p99_under_10ms | 验收判据 fsync P99 <10ms +【功能定义】硬承诺重载档立即执行 | 环上 P99 < 10_000μs 且重载档立即执行计数 = 200 |
| power_loss_promise | 【设计细节】窗口上限 8s = 断电丢失窗口承诺 | POWER_LOSS_WINDOW = 重载窗同一数字 |
| write_curve_60s | 【交互诊断】最近 60 秒实际写入量曲线（深化二） | 逐秒桶翻页/累加/滑出 |

## F047 调度器延迟预算（G-B-07）· 8 项

主册判据：**四类 p99 各自达标（2000μs 总预算内）；压力混载下输入类 p99 仍达标。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| budget_table | 【功能定义】输入 500/合成 800/音频 300/普通 400μs | 四类子预算常量 + 总 2000 |
| priority_map | 【设计细节】优先级映射（输入最高带抢占…） | 四类优先级/抢占权精确 |
| four_class_p99_ok | 验收判据四类 p99 各自达标 | 正常负载四类全绿 |
| mixed_load_input_ok | 验收判据压力混载输入类 p99 仍达标 | 混载 10k 样本输入 p99 ≤ 500μs |
| aging_correction | 【状态与异常】优先级饥饿 → 老化自修正 + 记录事件 | 修正事件环（前/后优先级） |
| zero_sample_honest | 【状态与异常】长期零样本观察窗说明 | p99 = None 诚实 |
| ring_10k | 【数据与存储】延迟采样每类环形 10,000 条（~160KB） | 容量常量 + 内存口径 |
| lane_curve_60s | 【交互设计】最近 60 秒各类 p99 泳道曲线 + 超标红标（深化二） | 秒末冻结 p99/断点/超标点可判 |

## F048 CPU 频率联动（G-B-08）· 10 项

主册判据：**交互突发响应（点按到满频）<50ms；续航对比自动策略 vs 固定高频提升 >15%。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| burst_under_50ms | 验收判据突发 <50ms | burst 模型耗时 = 10ms（MSR 写）≤ 50 |
| down_hysteresis_5s | 【设计细节】降档迟滞 5s（防抖） | 迟滞窗判定 |
| type_compute_high | 【功能定义】持续负载 30s 构建型=高频 | cc1/rustc/ld 等签名 → 档 1 |
| type_throughput_low | 【功能定义】下载=低频 | netd/curl 等签名 → 低档 |
| idle_down_5s | 【功能定义】空闲 5s 降最低档 | 空窗 + 迟滞后降档 |
| pss_graceful_mid | 【状态与异常】_PSS 不可读 → 固定中档 + 诊断标注 | graceful 路径 + 决策日志 |
| temp_forces_down | 【状态与异常】温度超限（F197）强制降档优先 | 温度红线压过升档 |
| manual_silent_cap | 【交互设计】静音档封顶低频（手动档=自动策略边界） | clamp_target 唯一封顶判据 |
| fail_then_lock_safe | 【状态与异常】切换失败重试一次后锁定安全档 | 失败注入 → 重试 → 锁定 |
| fail_twice_locks | 同上 | 锁定后任何路径不再切换 |

## F049 空转清零工程（G-B-09）· 11 项

主册判据：**纯桌面静置 60 秒：合成器 CPU <0.5%、内核唤醒次数 <10 次；两数字同录在案。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| idle_three_conditions | 【功能定义】无脏区、无动画、无光标移动 | 三条件与 |
| dirty_blocks_idle | 【设计细节】事件源统一 fence——脏区 | 有脏 → 不空闲 |
| animation_blocks_idle | 【设计细节】VSync 定时器仅在动画注册时 armed | 注册/注销armed 翻转 |
| cursor_window_blocks_idle | 【设计细节】光标闪烁只在相关窗口可见时 armed | 可见性门控 |
| vsync_disarmed_when_no_anim | 同 animation_blocks_idle（反向） | 无动画 VSync 解除 |
| idle_60s_gate_pass | 验收判据 60 秒唤醒 <10 | 阈内过闸 |
| idle_60s_gate_fail | 同上（反向验证） | 超阈不过闸 |
| progress_anim_legit | 【状态与异常】应用挂常驻动画按需唤醒不算违规 | 进度条唤醒有意义 |
| wake_storm_reported | 【状态与异常】唤醒风暴 >60/s 归因报告 F042 | 风暴报告计数 |
| wake_cause_ledger | 【数据与存储】唤醒账目（来源在册） | wake_log 三源记录 |
| wake_by_source | 【数据与存储】唤醒原因分类计数（深化二） | 三源逐类累计 + 总和对账 |

## F050 中断合并（G-B-10）· 8 项

主册判据：**flood 测试（70 级）零丢弃；滚动场景输入类 p99 延迟 ≤8ms。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| flood70_zero_drop | 验收判据 flood-publishes 70 级零丢弃 | dropped 恒 0（正确性根基） |
| position_keep_latest | 【设计细节】鼠标位置类只保最新（可覆盖） | 同键覆盖 + merged 计数 |
| discrete_keep_all | 【设计细节】按键类全保（不可丢） | 离散事件全交付 |
| merge_key_isolation | 【设计细节】同设备同类型才合并（键盘不与滚轮混批） | 合并键隔离 |
| batch_budget_hard | 【设计细节】批处理预算 2000ns 硬不超卖 | 单批 ≤ 预算/单件成本 |
| storm_breaker | 【状态与异常】中断风暴 → 熔断降频 + 诊断告警 | 熔断 + 告警计数 |
| shrink_on_backlog | 【状态与异常】合并延迟 >8ms → 动态收缩窗口 | 积压 → 窗口减半 |
| minute_agg_row | 【数据与存储】合并统计入账本（深化二） | 分钟行 inputs/merged/delivered/max_batch 精确 |

## F051 大页策略（G-B-11）· 10 项

主册判据：**字形图集场景 TLB miss 率实测下降 >50%；大页池启动预留成功率 >95%。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| pool_capacity | 【设计细节】三类区尺寸预算 | 64MB 池 16 槽 / 需求 11 槽 |
| boot_reserve_full | 验收判据预留成功率 >95% | 4K 双缓冲全落大页 |
| slots_used | 【数据与存储】启动预留一次锁定 | 槽位占用精确 |
| regions_marked | 【功能定义】三类静态区 | 内核/图集/帧缓冲全标注 |
| degrade_order_fb_first | 【设计细节】降级顺序帧缓冲先降内核最后 | DEGRADE_ORDER 常量 |
| tlb_reduction_over_50 | 验收判据 TLB miss 下降 >50% | 图集场景降幅 >500‰ |
| tlb_entries_math | 【设计细节】TLB 模型 | 8MB 工作集 2048 项 → 2 项 |
| only_grow_invariant | 【状态与异常】只增不减防碎片 | 无 free API 结构自证 + 不变量 |
| ownership_map | 【数据与存储】槽位归属 | 内核 0/图集 1-2/帧缓冲 3-10 |
| tail_free_slot | G-B-14「大页池尾部」的 F051 查询面（深化一） | 尾槽选取正确 |

## F052 堆碎片治理（G-B-12）· 16 项

主册判据：**7 天烤机碎片率 <15%；六档分配延迟 P99 <1μs；零堆纪律 grep 自证。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| six_classes | 【功能定义】8/16/32/64/128/256B 六档 | 档位常量 + 直通阈值 512B |
| heap_56kb | 【功能定义】内核堆 56KB 分块（实测既有） | 六档区 + 直通区 = 56KB |
| alloc_roundtrip | 【设计细节】位图空闲链 O(1) | 分配/释放 round-trip |
| split_on_exhaustion | 【状态与异常】档耗尽 → 相邻切分（带开销标注） | split_events 计数 |
| fail_path_explicit | 【状态与异常】分配失败路径全测（panic 演练） | 显式 None + 计数 |
| bake_frag_under_15 | 验收判据 7 天烤机碎片率 <15% | 20k 混载模拟 < 150‰ |
| latency_p99_under_1us | 验收判据六档分配延迟 P99 <1μs | 最坏路径访存模型 <1000ns |
| zero_heap_struct | 验收判据零堆纪律 grep 自证 | 结构体字节账精确（含对齐填充） |
| frag_alert_25 | 【状态与异常】碎片率 >25% → 告警 + 归因档位 | 告警 + 归因在册 |
| spec_bucket_edges | 【设计细节】分配谱直方图（数据先行，深化一） | 桶边界与 class_for 逐位同源 |
| spec_seven_day_roll | 【设计细节】先采集 1 周（深化一） | 7 天滚动覆盖 + 累计守恒 |
| spec_attribution_small_storm | 【状态与异常】归因哪类分配模式（深化一） | 微对象风暴谱判定 |
| spec_attribution_passthrough | 同上（深化一） | 直通抖动谱判定 |
| spec_review_insufficient | 【设计细节】数据先行不拍脑袋（深化一） | 样本 <1000 不出建议 |
| spec_review_add_512 | 【设计细节】档位边界按谱定（深化一） | 512 进档建议 |
| frag_curve_7d | 【交互设计】碎片率曲线 7 天（深化二） | 日采样环形滚动 |

## F053 启动并行度（G-B-13）· 9 项

主册判据：**四链并行后内核段总耗时 ≤ 串行版 60%；8 秒线分解每段预算甘特图可见偏差 <10%。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| dependency_matrix | 【设计细节】四链依赖矩阵文档化 | ACPI/PCI/USB/存储依赖声明 |
| parallel_le_60pct | 验收判据并行 ≤ 串行 60% | 并行总时实测比 |
| gantt_12_bars | 【交互设计】甘特图逐链每阶段起止 | 12 条甘特条目 |
| boot_8s_segments | 【功能定义】8 秒线分解（固件 4s/Limine 0.3s/内核 2.1s/动画 1s） | 分段预算常量 |
| anim_at_80pct | 【设计细节】动画启动点提前到内核段 80% | 提前计算 |
| storage_fail_safe_mode | 【状态与异常】存储链失败仍可进安全模式（F193） | 失败链跳过标注 |
| timeouts | 【设计细节】每链超时独立（USB 3s/存储 1s） | 超时常量 |
| segment_deviation | 验收判据实测偏差 <10% | 偏差 permille 计算 |
| lock_wait_attributed | 【状态与异常】并行竞争 → 锁等待归因入时间线 | lock_waits 记账 |

## F054 图像解码 SIMD（G-B-14）· 9 项

主册判据：**4K PNG 解码 ≤150ms、JPEG ≤120ms；SIMD 与标量结果逐像素一致。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| isa_detected | 【交互设计】诊断面板显示指令集档 | SSE4.2/AVX2 运行时探测 |
| simd_bitexact | 验收判据 SIMD 与标量逐位一致 | 四滤波 × 多长度对拍 |
| paeth_reference | 【设计细节】RFC 2083 Paeth 公式 | 参考值精确 |
| corrupt_rejected | 【状态与异常】损坏文件解码错误如实（不产出半图） | 非法滤波号拒绝 |
| fallback_chain | 【状态与异常】AVX2 探测失败无缝回退 SSE4.2 | 回退链同结果 |
| streaming_plan | 【状态与异常】>64MP 流式分块（内存峰值受控） | 分块规划 |
| budget_lines_registered | 验收判据 150ms/120ms 实测线 | 常量登记（随闸门实测） |
| decode_buf_plan | 【设计细节】对齐分配走大页池尾部（深化一） | 尾槽计划 + 4K 回退 |
| decode_buf_pool_tail_live | 同上（跨域对接一处一事实） | 与 F051 池态联动 |

## F055 字形光栅缓存（G-B-15）· 12 项

主册判据：**中文长文档滚动 10 分钟帧耗时方差 <20%；图集命中率 >90%。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| paged_atlas_consts | 【功能定义】图集分页 512×512px | 页常量 + 配额 16MB |
| lookup_hit | 【功能定义】热字形常驻 | 命中路径 |
| frame_protection | 【设计细节】当前帧用到的字形禁止驱逐 | 帧保护集合 |
| hot_set_resident | 【设计细节】预热常用 3500 汉字 + ASCII 常驻 | 热集禁驱逐 |
| grow_on_low_hitrate | 【状态与异常】命中率 <80% → 自动扩一页 | maybe_grow |
| slow_raster_queued | 【状态与异常】光栅 >2ms → 后台线程光栅化 | 慢队列入队 |
| atomic_swap | 【状态与异常】前台旧版占位到位后原子换 | commit_slow 原子换 |
| hitrate_over_90 | 验收判据命中率 >90% | 常用字场景判据 |
| font_version_rebuild | 【设计细节】换字体版本 = 图集全重建显式日志 | 版本变更重建 + 计数 |
| memory_accounting | 【数据与存储】图集配额 16MB（4K 管线） | 页数 × 256KB 口径 |
| occupancy_none_when_empty | 【交互设计】诊断面板显示占用（深化二） | 空图集 None |
| atlas_occupancy_in_range | 同上（深化二） | 实占 permille 在界内 |

## F056 合成器脏区深化（G-B-16）· 10 项

主册判据：**打字场景脏区面积 P95 <5% 屏；光标移动 60fps 恒定且零内容重绘；弹窗动画帧内合成矩形 = 弹窗矩形 + 阴影环带。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| damage_consts | 【数据与存储】每窗口脏矩形 8 个上限 | 窗口/环带/爆炸常量 |
| cursor_zero_content | 【功能定义】光标独立层移动零内容重绘 | 光标 plane 独立记账 |
| typing_p95_below_5pct | 验收判据打字 P95 <5% 屏 | 直方图 P95 ≤ 50‰ |
| popup_plus_shadow_band | 验收判据弹窗矩形 + 阴影环带 | 合成矩形 = 弹窗 + 16px 环带 |
| shadow_prebuilt_once | 【设计细节】阴影 16px 环带预渲染不逐帧重算 | 预渲染一次 |
| overflow_merges_whole | 【数据与存储】8 上限溢出合并整窗 | 包围盒语义 |
| explosion_throttle_30fps | 【状态与异常】脏区爆炸 → 合并限频 30fps | 爆炸限频间隔 |
| layer_intersect_clip | 【状态与异常】层重叠 → 层间脏区求交裁剪 | 窗口外裁剪 |
| interval_merge | 【设计细节】脏区合并（相邻 ≤1px 合并） | 扫描线合并判停 |
| no_fullscreen_path | 【功能定义】全屏重绘从词典删除 | 无整屏路径 |

## F057 IO 调度分级（G-B-17）· 11 项

主册判据：**「下载中打开目录」目录延迟 ≤ 无下载时的 1.2 倍；后台任务零饥饿（24h 混载全部完成）。**

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| tier_consts | 【设计细节】截止期前台 50ms/后台 2s/批量无 | 常量 + 读<64KB 交互倾向 |
| classify_rules | 【设计细节】class 判定按进程标签 + 请求特征 | 四规则判定 + 归因短语 |
| fsync_bypasses_all | 【设计细节】分级不改变 fsync 硬承诺（B-7xx 优先于一切） | fsync 越过一切队列 |
| fg_jumps_full_bg_queue | 【功能定义】前台插队 | 满队列下前台先出 |
| dir_delay_within_120pct | 验收判据目录延迟 ≤ 1.2 倍 | 下载中目录延迟比 |
| fg_edf_order | 【功能定义】调度按截止期+权重混合 | 前台 EDF 序 |
| fg_saturation_pauses_bg | 【状态与异常】前台持续满载 60s → 后台暂停 | 反向保护触发 |
| pause_holds_within_max | 【状态与异常】暂停期内保持（让出窗语义） | 暂停有界 |
| pause_max_resumes_bg | 【状态与异常】防饿死反向保护时长有界（5s） | 到期自动解除 |
| batch_yields_every_5min | 【状态与异常】批量超 30 分钟分段让路每 5min 让 1s | 让路节奏 |
| audit_records_degrade_why | 【数据与存储】分级事件审计（谁被降级/为什么） | 审计环归因短语 |

---

# 深化批次三新增对账（364 项）

> 深化批次三把主册里「说到了但没落到数据结构/可测接口」的判据逐条补齐，并为十七域补上分工书**通用十二查**登记册与**用户故事场景验收**。新增件：`perfkit.rs`（共用底盘）、`k1walk.rs`（十二查登记册）、`k1scene.rs`（十七场景）、以及十七域各自的 `<domain>_ext.rs`。
> 全部新增项同样遵守「零无锚检查项」：每项都能回答主册的哪一句话。

## F041 · 深化件 `frameledger_ext`（13 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| axis_and_lines | 【交互设计】纵轴 0-25ms + 80/60fps 双线 | 双线千分坐标 500 / 664 |
| severity_red_yellow_green | 【设计细节】目标线与底线视觉分级 | 破底线=红 / 超目标线或段超阈=黄 / 其余绿 |
| frame_detail_four_spans | 【交互设计】悬停弹出该帧四项数据 | 四项 + 严重度 + 三个标志位齐全 |
| minute_row_roundtrip | 【数据与存储】格式对齐 vxbench（F061 直接消费） | 38B 行 encode/decode 往返等值 |
| crc_catches_bitflip | 同上（账本不可静默损坏） | 单位翻转必被 CRC 抓到，不产出半行 |
| bad_magic_reported | 开放格式（F126） | 非本格式直说 BadMagic |
| bad_version_reported | 开放格式版本化 | 未来版本带版本号上报，不静默丢弃 |
| short_buffer_reported | 边界 | 缓冲不足返回需求字节数 |
| file_rotates_at_24h | 【数据与存储】24 小时分钟聚合落账本文件 | 1440 行满即轮转 |
| downsample_recovers_after_budget_ok | 【状态与异常】超预算自动降采样（主册只写降，**回落由本件补齐**） | 开销回落连续达标后恢复全量，worst_tier 留档 |
| raise_is_debounced | 同（防抖） | 单帧抖动不切档 |
| probe_pairing_selfcheck | 【设计细节】三处测量点分段计时 | 漏起表/重入/时钟倒挂三类故障显式，悬空可查 |
| window60_timestamp_cut | 【数据与存储】滚动窗口保留 60 秒逐帧 | 按**时间戳**裁剪（非按条数），边界含 60s |

## F042 · 深化件 `frameattr_ext`（17 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| suspects_named_and_thresholded | 【设计细节】四类判定阈值 | 名称与阈值文案同源枚举 |
| evidence_is_reference / evidence_below_threshold_not_counted / evidence_none_is_honest | 【交互设计】每类的原始证据（哪次 IO/哪个窗口多大脏区） | 引用不复制；未达阈不算；无证据就是 None |
| top_is_max_share | 【交互设计】头号归因 | 占比最大者，并列取编号小者 |
| mixed_when_no_majority / single_cause_not_mixed / unclassified_is_none | 【状态与异常】多因并发如实混合归因，不硬编主因 | ≤500‰ 即混合；全零 = 未归类 |
| casebook_covers_oldest | 【数据与存储】保留 7 天 | 案例簿环满覆盖最旧并计数 |
| history_row_complete | 【交互设计】列表（时间/掉帧数/头号归因/证据链接） | 四列齐全且链接可回查 |
| health_disables_and_reports / disabled_skips_counted / health_restores_after_streak | 【状态与异常】自身异常 → 静默停用 + 诊断报备（不拖累合成器） | 连续 3 次异常停用并报备；停用只计数不执行；连续 30 次健康才恢复 |
| fp_zero_on_clean_samples / fp_counts_only_wrong_conclusions / fp_true_drop_attributed_is_correct / fp_window_100s | 【验收判据】误报率 <10%（正常拖动 100 秒零误报） | 只有「没掉帧却给了结论」算误报；窗长达标可判 |

## F043 · 深化件 `startprof_ext`（15 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| seg_boundaries_are_constants | 【设计细节】段边界写死在打点常量 | 五段边界原文常量化 |
| bar_widths_sum_1000 / bar_width_is_proportional / zero_profile_not_faked | 【交互设计】五段横向条形图，段宽按耗时比例 | 最大余数法配平和恒 1000；零耗时不伪造比例 |
| compare_speedup / compare_partial / compare_no_baseline | 【交互设计】「首次 vs 二次」并排 | 提速千分 + 达标判定 + 缺基线的诚实文案 |
| launch_seq_cold_then_warm | 【设计细节】序号 1=冷、2 起算热 | 版本重置后回到冷 |
| profilebook_version_isolated / profilebook_keeps_20 | 【数据与存储】按 (应用版本, 启动序号) 记录；保留最近 20 次 | 版本隔离不混账；超 20 覆盖最旧 |
| abort_label_is_explicit | 【状态与异常】中止于第 X 段 | 「中止于首帧段」标注完整 |
| privacy_gate_skips_collection_only | 【设计细节】隐私总闸（F036 同开关） | 关闸只停采集不停启动，跳过数可查 |
| variance_cv_below_15pct / variance_cv_detects_spread / isqrt_correct | 【验收判据】10 次启动方差 <15% | 变异系数千分账 + 整数平方根自证 |

## F044 · 深化件 `prefetch2_ext`（18 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| pf_header_roundtrip / pf_bad_crc_is_corrupt / pf_bad_version_reported | 【数据与存储】`cache/prefetch/<hash>.pf`，格式自定开放（F126） | 40B 头魔数/版本/CRC round-trip 与三类错误显式 |
| pf_budget_8mb_per_app | 【数据与存储】上限 8MB/应用 | 超限即拒，不写坏文件 |
| pf_validity_three_states | 【状态与异常】版本变化失效重建 / 损坏弃用不报错 | Valid / VersionStale / Corrupt 三态 |
| pf_store_lru_evicts / pf_store_rejects_over_budget | 【数据与存储】LRU 驱逐 | 最久未用先走；超配额拒绝计数 |
| pf_batch_eight_and_tail | 【设计细节】批量 8 页一组 | 尾批不补齐，不伪造页 |
| pf_remaining_batches / pf_remaining_decreases | 同（进度面） | 剩余批数预估（尾批计入） |
| pf_priority_below_fg | 【设计细节】预读优先级永远低于前台 IO（F057） | 预读恒落后台级 |
| gen_not_due_before_30s / gen_postponed_when_busy / gen_runs_when_idle / gen_cancelled_on_relaunch | 【设计细节】应用退出后 30 秒后台完成（不抢退出体验） | 到期/前台忙推迟/空闲执行/重启取消 |
| hit_rate_passes / hit_rate_below_redline_fails / hit_rate_zero_samples_not_pass | 【验收判据】命中率 >70% | 千分账；零预读不算达标 |

## F045 · 深化件 `pagewater_ext`（20 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| tiers_are_32_24_16_gb | 【功能定义】高 3.2GB/中 2.4GB/低 1.6GB | 三档数值精确 |
| lru_file_before_anon / pages_needed_rounds_up | 【设计细节】LRU 按文件页/匿名页分列（文件页优先弃） | 先文件后匿名；空列不越界 |
| readonly_volume_excluded / global_dirty_excludes_readonly | 【设计细节】脏页 20% 上限按分区分别计（SHARED 只读卷不计） | 只读卷脏页千分恒 0 且不进分母 |
| flush_ignores_readonly / flush_clamps_to_dirty | 同（冲刷面） | 只读卷不冲；冲刷量钳到脏页数 |
| tick_gate_500ms | 【设计细节】水位判定每 500ms 一次（功耗账） | 节流命中率可量化 |
| tier_rule_documentized / knobs_registered / knob_clamped | 【设计细节】三档切换条件文档化（全表在旋钮清单） | 条件表可查；7 个旋钮登记且可钳制 |
| regret_in_window / no_regret_outside_window / regret_counted_once | 【状态与异常】记录「回收后悔」指标 | 窗内读回计后悔；同页不重复计数 |
| emergency_enters_flushes_notifies / emergency_hysteresis | 【状态与异常】耗尽前 200MB 警戒 → 全局冲刷 + 通知 F195 | 进入/冲刷/通知/迟滞解除 |
| cede_quarter_once | 【状态与异常】配额挤压 → 缓存先让 | 一次性让 25%，次数暴涨即调参信号 |
| readonly_view_no_setter | 【交互设计】档位只读展示（防乱调） | 类型层面无 setter |
| decision_log_only_on_change / reclaim_curve_accumulates | 【数据与存储】策略决策日志入诊断快照（F174） | 只在真切档时记；回收曲线入 60 秒窗 |

## F046 · 深化件 `wcoalesce_ext`（19 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| windows_1_5_8 | 【功能定义】空闲 1s/正常 5s/重载 8s | 三档窗口精确 |
| classify_idle_needs_both / classify_heavy_needs_either | 【设计细节】请求 <10/s 且脏页 <5% = 空闲；>200/s 或脏页 >15% = 重载 | 「且」/「或」语义逐条 |
| knobs_registered | 【设计细节】存储栈旋钮（MD2 附录 K） | 8 个旋钮登记 |
| feed_unfed_is_normal / feed_shared_from_f045 | 【数据与存储】档位依据 = 每秒写请求数 + 脏页占比（**F045 数据源共享**） | 未喂数据走保守中间态；本域不自算脏页 |
| dwell_suppresses_switch | 【状态与异常】档位抖动迟滞（切档后至少驻留 5s） | 驻留未满的切档被抑制并计数 |
| tier_switch_into_diag_snapshot | 【交互诊断】档位切换事件入诊断快照 | 切档行投递可查 |
| loss_window_computable / loss_window_by_tier_ordered / loss_within_promise | 【设计细节】窗口上限 8s 的依据 = 断电丢失窗口承诺 | 最坏丢失字节可算且随档递增 |
| drill_all_tiers_green / drill_gap_not_masked / drill_fsck_fails | 【验收判据】断电百次按三档各跑一遍全绿 | 缺一档/有 fsck 均判不达标（不粉饰） |
| fsync_p99_all_tiers_pass / fsync_p99_heavy_slow_fails / fsync_p99_no_sample_not_pass | 【验收判据】fsync P99 <10ms 不受档位影响 | 三档各自独立；零样本不成立 |
| rerun_ledger | 【状态与异常】B-702/B-703 自适应模式重测重录 | 重测批次与结果留档 |
| diag_text_per_tier | 【交互诊断】当前档位与写入曲线 | 三档人话说明 |

## F047 · 深化件 `latbudget_ext`（15 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| budget_sum_is_total / budget_adr_note_present | 【设计细节】预算表编译期常量（变更走 ADR） | 四类和 = 2000μs 自洽；ADR 说明在码 |
| priority_map_matches_book / only_input_is_preemptive | 【设计细节】输入=最高带抢占、音频=高带 deadline、合成=高、普通=rr 老化 | 映射表逐条；仅输入带抢占 |
| effective_priority_boosted | 同（老化提升） | 提升后优先级变高且不越过 0 |
| snapshot_only_over_budget / over_permille_zero_when_in_budget | 【交互设计】点击超标点看线程与优先级快照 | 只记超标点；未超标幅度为 0 |
| aging_requires_self_cause_and_cooldown / aging_capped_then_giveup / aging_decays_back | 【状态与异常】归因到自身策略 → 自修正（优先级老化）并记录修正事件 | 非自身原因不动；冷却；达上限放弃并报备；会回落 |
| observation_note_three_states / zero_long_text_is_honest | 【状态与异常】某类长期零样本 → 观察窗说明（不代表无风险） | 三态判定 + 诚实文案 |
| energy_downgrade_reported / energy_restores_balanced | 【设计细节】energy=balanced 在大核可用时保持 | 大核不可用降级并报备 |
| ring_budget_160kb | 【数据与存储】每类环形 10,000 条（内存 ~160KB） | 4×10000×4B = 160,000B 精确自证 |

## F048 · 深化件 `cpufreq_ext`（21 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| pss_table_descending / pss_nondescending_detected | 【功能定义】Y7000 实机频率表（ACPI _PSS） | 表必须降序（index 0 = 最高频）；非降序被检出 |
| pss_fallback_mid_and_reported / pss_fallback_text_explainable | 【状态与异常】_PSS 不可读 → 固定中档 + 诊断标注 | 降级 + 报备 + 用户可读文案 |
| switch_retry_once_then_lock / switch_ok_clears_retry / switch_reset_unlocks | 【状态与异常】切换失败 → 重试一次后锁定安全档 | 一次重试；再失败锁定并报备；解锁需显式复位 |
| thermal_preempts_policy / thermal_release | 【状态与异常】温度超限（F197）强制降档优先于本策略 | 越权只许更保守；恒优先 |
| builder_list_prefix_match | 【设计细节】30s 类型判定按线程名签名（构建工具名单） | 前缀匹配（带版本号后缀也命中） |
| class_not_before_window / class_verdict_after_window / class_interactive_wins_tie | 【设计细节】持续负载 30s 后按类型选档 | 窗满才下结论；平票交互优先 |
| switch_timer_passes / switch_timer_detects_over | 【设计细节】频率切换自身耗时 <10ms（MSR 写） | 耗时账可判超线 |
| burst_meter_passes / burst_meter_detects_over | 【验收判据】交互突发响应 <50ms | 突发账 |
| battery_gain_passes / battery_gain_below_line / battery_no_baseline | 【验收判据】续航提升 >15% | 双策略对照；缺对照不判达标 |
| knobs_registered | 【设计细节】全策略参数进旋钮清单（无隐藏魔法数） | 7 个旋钮登记 |

## F049 · 深化件 `idlezero_ext`（19 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| always_vs_optional_sources / fence_refuses_disarm_always | 【设计细节】事件源统一 fence（输入/VSync/IPC） | 常 armed 源不可关（关了丢事件） |
| fence_detects_stray_wake / fence_counts_armed_wake / optional_sources_all_off | 同（栅栏漏检） | 未 armed 却来唤醒 = 漏检；可关源全关才可能零唤醒 |
| caret_arming_three_conditions / caret_reason_specific | 【设计细节】光标闪烁只在相关窗口可见时 armed | 三条件全真才 armed；未 armed 给具体原因 |
| deep_sleep_y7000_recorded / deep_sleep_fallback_labeled | 【设计细节】深睡用 MONITOR/MWAIT（实测 Y7000 支持性记档） | 支持性记档；不支持走软路径并标注 |
| anim_register_exempts / anim_reregister_updates / anim_unregister / anim_full_rejected | 【状态与异常】常驻动画（进度条）按需唤醒（不算空转违规） | 登记者唤醒豁免；重复登记更新；容量满被拒计数 |
| idle_60s_pass_both_numbers_on_record | 【验收判据】静置 60 秒：CPU <0.5% 且唤醒 <10 次，**两数字同录在案** | 双指标同窗同判，结论带证据留档 |
| idle_incomplete_window / idle_three_failure_modes / idle_exempted_not_counted / idle_no_cpu_sample_fails | 同（判据可辨） | 窗未满不结论；三种失败态可辨；豁免不计；零样本记未知不冒充 0% |
| wake_storm_report_channel | 【状态与异常】唤醒风暴 → 归因报告给 F042 | 报备通道可用 |

## F050 · 深化件 `intrcoal_ext`（14 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| merge_key_device_and_kind | 【设计细节】同设备同类型才合并（键盘不与滚轮混批） | 键 = 设备 + 类型 |
| coverage_by_kind | 【设计细节】可覆盖与不可丢的二分是正确性根基 | 离散事件永不覆盖 |
| budget_hard_no_oversell / budget_resets_per_batch | 【设计细节】批处理预算 2000ns 超则留队（预算硬，不超卖） | 超预算留队计数；每轮重置 |
| net_coalesce_by_flow / net_flush_delivers_pending / net_full_passthrough_not_drop | 【功能定义】输入**与网络**中断按突发窗口合并 | 按 flow 合并（键不同于输入）；槽满直通不丢包 |
| flood_70_zero_drop / flood_gap_not_masked | 【验收判据】flood-publishes 70 级零丢弃 | 跑满 70 级且 dropped 恒 0；差一级不达标 |
| storm_breaker_trip_and_recover / breaker_reduces_sampling | 【状态与异常】中断风暴 → 熔断降频 + 诊断告警 | 触发/降频（跳闸当秒不丢批）/连续平静 5 秒恢复 |
| coalesce_stats_for_ledger / coalesce_curve_per_sec | 【数据与存储】合并统计（每秒合并率/最大批）入账本 | 合并率与最大批；60 秒逐秒曲线 |
| latency_redline_8ms | 【状态与异常】合并延迟 >8ms 动态收缩 | 常量在位 |

## F051 · 深化件 `bigpage_ext`（17 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| region_budgets / framebuffer_4k_two_calibers | 【设计细节】内核 4MB/图集 8MB/帧缓冲按分辨率（4K 双缓冲 32MB） | 主册「32MB」对应单缓冲 32 位口径，双缓冲精确值如实双登记 |
| pages_needed_rounds_up | 同（页数换算） | 4MB 页向上取整 |
| degrade_order_frozen | 【设计细节】预留失败降级顺序：帧缓冲先降、内核最后 | 顺序冻结 |
| reserve_kernel_last_to_degrade / partial_degrade_labeled | 【状态与异常】物理连续内存不足 → 按区独立降级并标注 | 池不足时内核保住、帧缓冲先砍；部分降级可查 |
| reserve_success_rate / reserve_zero_pool_fails | 【验收判据】预留成功率 >95% | 千分账；零池不达标 |
| shortage_reported | 同（零静默） | 缺页进诊断报备 |
| fragment_guard_never_releases / fragment_guard_clean_is_intact | 【状态与异常】只增不减策略防碎片 | 释放请求被拒并计数；纪律可验证 |
| tlb_drop_passes / tlb_one_sided_not_pass / tlb_weak_drop_fails | 【验收判据】TLB miss 率下降 >50% | 双路径对拍；单路径数据不可比，不冒充达标 |
| gpu_assessment_honest | 【功能定义】R3 GPU 摸底评估核显侧大页可行性 | 未评估就是未评估，不默认可行 |
| usage_view_readonly / usage_zero_access | 【交互设计】显示大页占用与命中率 | 只读投影；零访问不冒充结论 |

## F052 · 深化件 `heapfrag_ext`（21 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| bitmap_alloc_first_free / bitmap_free_and_double_free_detected / bitmap_free_out_of_range | 【设计细节】每档空闲链用位图（O(1) 分配） | 首个零位分配；重复释放/越界释放被记不 panic |
| bitmap_pairing_consistent / bitmap_full_fails_cleanly | 同（契约） | 分配/释放一一配对；填满后干净失败 |
| class_table_fit / class_table_bytes_safe | 【设计细节】大块直通阈值 512B | 六档 + 直通分界；越界取 0 |
| internal_fragment_computed | 同（内部碎片） | 档尺寸 − 请求尺寸；直通为 0 |
| split_from_nearest_bigger / split_exhausted / split_direct / split_cost_recorded | 【状态与异常】某档耗尽 → 相邻档切分（**带切分开销标注**） | 只取最近更大档；无档可切；开销入账 |
| fail_path_explained / fail_path_mid_rescue | 【状态与异常】分配失败路径全测（panic 演练 B-2903） | 四阶段各自可解释，终态仍不崩溃 |
| frag_alarm_with_hysteresis / frag_alarm_no_flap / frag_insufficient_data | 【状态与异常】碎片率 >25% → 告警 + 归因 | 迟滞解除；边界抖动不刷屏；样本不足不出归因 |
| frag_target_15pct | 【功能定义】长期运行碎片率 <15% | 达标线判定 |
| alloc_latency_all_classes_pass / alloc_latency_gap_exposed / alloc_latency_slow_class_fails | 【验收判据】六档分配延迟 P99 <1μs | 分档桶账；缺档样本暴露；慢档不达标 |

## F053 · 深化件 `bootpar_ext`（18 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| per_chain_timeout / timeout_verdict_fatal_only_for_strict / timeout_table_complete | 【设计细节】每链超时阈值独立（USB 3s 宽限/存储 1s 严格） | 逐链阈值；仅严格链判失败 |
| dep_matrix_real_vs_inertial / inertial_not_blocking | 【设计细节】四链依赖矩阵文档化（多数「依赖」是历史串行惯性） | 真依赖阻挡并行，惯性依赖不阻挡且可计数 |
| failure_propagates_with_label / inertial_dep_not_skipped / storage_failure_still_safe_mode | 【状态与异常】某链失败 → 依赖该链的阶段跳过并标注（存储链失败仍可进 F193） | 跳过带原因；惯性依赖不跳过；安全模式结论 |
| lock_wait_attributed / lock_wait_snapshot | 【状态与异常】并行竞争 → 锁等待归因入时间线 | 谁等谁、等多久、按链累计 |
| anim_kickoff_at_80pct / anim_perceived_gain / anim_gain_zero_before_fire | 【设计细节】动画启动点提前到内核段 80% 处 | 80% 起播；感知收益可算；未起播不冒充收益 |
| gantt_parallel_vs_serial / parallel_ratio_meets_60pct | 【验收判据】四链并行后内核段 ≤ 串行版 60% | 并行段取「最晚结束 − 最早开始」时长，不拿绝对时刻比 |
| budget8s_segments / budget8s_deviation / budget8s_per_segment_not_total | 【验收判据】8 秒线分解后每段预算可见且偏差 <10% | 逐段判定（总量达标但单段超标不算） |

## F054 · 深化件 `imgsimd_ext`（26 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| isa_probe_and_fallback | 【功能定义】SSE4.2 基线全机可用，AVX2 运行时探测启用 | 探测与无缝回退；通道宽度随档 |
| paeth_predictor / filter_sub / filter_up / filter_none / filter_average / filter_paeth | 【设计细节】**行过滤**热点（占解码 80% 耗时之一） | PNG 五型重建逐型正确（含 Paeth 三距离） |
| lanes_equal_scalar_sub / lanes_equal_scalar_up_tail / lanes_falls_back_for_dependent_filters | 【验收判据】**SIMD 与标量逐像素一致（正确性优先于速度）** | 通道路径与标量逐位对拍；有跨字节依赖的过滤回落标量 |
| row_filter_length_mismatch_errors | 同（边界） | 长度不符错误如实 |
| decode_error_three_parts / decode_error_truncated | 【状态与异常】损坏文件 → 解码错误如实 | 三要素齐全（发生了什么/为什么/下一步） |
| half_frame_blocked_while_incomplete / half_frame_deliverable_when_complete / half_frame_blocked_on_failure | 【状态与异常】**不产出半图** | 未完成/失败均拦截 |
| idct_matches_reference / idct_dc_only_is_flat | 【设计细节】**IDCT** 热点 | 分离式与朴素实现落在定点容差内；纯 DC 块平坦 |
| stream_huge_detected / stream_plan_chunked / stream_shrink_to_fit | 【状态与异常】超大图（>64MP）→ 流式分块（内存峰值受控） | 严格大于边界；按行切不撕裂；超上限自动缩块 |
| bench_samples_three_4k | 【设计细节】基准样本固定三张（4K 照片/截图/插画） | 三样本且均 4K |
| bench_not_measured_not_pass / bench_jpeg_over_line / bench_both_pass | 【验收判据】4K PNG ≤150ms、JPEG ≤120ms | 双线各自判定；未测全不达标 |
| bitwise_failure_blocks_delivery | 【验收判据】正确性优先于速度 | 对拍不过，速度达标也不交付 |

## F055 · 深化件 `glyphcache_ext`（23 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| warmup_set_size_and_coverage / warmup_progress / warmup_complete / warmup_disabled_honest | 【设计细节】常用字集预热：3500 汉字 + ASCII | 3628 字；区段覆盖判定；进度可查；关掉就实说 |
| pin_blocks_eviction / pin_clears_at_frame_end / pin_capacity_respected | 【设计细节】当前帧用到的字形禁止驱逐（双缓冲页交换） | 钉住阻止驱逐并计数；帧末解钉；满槽如实返回 |
| page_exchange_is_atomic / page_exchange_cycles | 同（原子换页） | 前台忙则推迟；类型层面排除「换到一半」 |
| asset_version_rebuild_reasons / asset_version_log_entry | 【设计细节】灰度 AA 与 hinting 参数冻结进资产版本（换字体版本 = 全重建，**显式日志**） | 三参数各自原因；日志条目 `aa=1 hint=1 font=0000abcd` |
| autogrow_hysteresis / autogrow_resets_on_recovery / autogrow_at_cap_honest | 【状态与异常】命中率 <80% → 自动扩一页（上限 32MB） | 连续 3 次才扩；回升清零；到顶如实不扩 |
| hit_target_90pct | 【验收判据】命中率 >90% | 达标线判定 |
| raster_over_2ms_goes_background / raster_waits_for_front / raster_swaps_when_idle | 【状态与异常】光栅耗时 >2ms → 后台光栅化（前台用旧版占位，到位原子换） | 转后台；占位代价可查；前台在用不许换 |
| quota_16mb_base / quota_32mb_cap / quota_no_persistence | 【数据与存储】配额 16MB；持久化不做 | 页粒度账；32MB 硬顶；策略声明 |
| page_geometry | 【功能定义】图集分页每页 512×512 | 512² 灰度 = 256KB/页 |
| atlas_cap_reported | 同（可感知事件） | 到顶进诊断报备 |

## F056 · 深化件 `dirtyrect_ext`（24 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| rect_ops / rect_empty | 脏矩形基础 | 相交/交集/包围盒/空矩形 |
| interval_merge / interval_covered_len / interval_rejects_bad_input / interval_capacity_respected | 【设计细节】**脏区合并 = 区间树相交合并（O(n log n)）** | 重叠与相接都合并；覆盖长度可量化；非法区间与容量满被拒 |
| shadow_band_16px / shadow_worth_prerender | 【设计细节】阴影按 16px 环带预渲染（不逐帧重算） | 外扩矩形 + 环带面积；**没有**逐帧重算入口 |
| anim_rect_registry / anim_rect_capacity | 【设计细节】动画矩形集在动画注册时声明 | 注册即知；并集可算；结束注销；容量满被拒 |
| cursor_hardware_path / cursor_overlay_path_detects_repaint | 【设计细节】光标层 = 硬件 cursor plane 或独立覆盖面（软路径兜底） | 双路径均保证零内容重绘；软路径若连带重合成即判不达标 |
| throttle_trips_and_notifies / throttle_defers_excess / throttle_recovers | 【状态与异常】脏区爆炸 → 合并限频 30fps + F042 归因 | 触发/推迟代价/连续正常 3 秒恢复；每轮只通知一次 |
| layer_clip_occluded / layer_clip_multi | 【状态与异常】层重叠动画 → 层间脏区求交裁剪 | 完全被遮的脏区可丢弃 |
| window_dirty_at_cap / window_dirty_overflow_collapses / window_dirty_ignores_empty | 【数据与存储】每窗口脏矩形 8 个上限，溢出合并为整窗 | 到 8 才溢出；退化单向（帧末才复位）；空矩形不占名额 |
| typing_dirty_within_5pct / typing_dirty_over_budget_detected | 【验收判据】打字场景脏区面积 P95 <5% 屏 | 千分账与超标检出 |
| popup_compose_equals_rect_plus_band / popup_compose_overshoot_detected | 【验收判据】弹窗动画合成矩形 = 弹窗矩形 + 阴影环带 | 不多不少；超框量可检 |

## F057 · 深化件 `iotier_ext`（16 项）

| 检查项 | 主册锚点 | 验证内容 |
| --- | --- | --- |
| tier_deadlines | 【设计细节】截止期：前台 50ms/后台 2s/**批量无截止期** | 无截止期 ≠ 0 |
| class_policy_two_factors / class_policy_why_text | 【设计细节】class 判定 = 进程标签 + 请求特征（读 <64KB = 交互倾向） | 批进程小读也不插队；写不享交互倾向；理由可查 |
| deadline_remaining_and_overdue / deadline_precedence | 【设计细节】截止期 + 权重混合 | 剩余预算；无截止期不谈逾期；同级逾期先、等待久者先 |
| queue_depth_view / queue_depth_zero | 【交互设计】三队列实时深度条 | 千分长度 + 背压提示 |
| throughput_per_tier / throughput_series / bg_starvation_detected | 【交互设计】各队列吞吐曲线 | 三队列独立不串账；后台有积压却零吞吐 = 饿死信号 |
| notice_throttled | 【交互设计】下载类任务自动降级的提示出现在通知中心（透明可查） | 同任务 60 秒节流 + 人话文案 |
| degrade_audit_queryable | 【状态与异常】分级事件审计（谁被降级/为什么）入诊断快照 | 审计环可按时序导出、按任务计数、同步报备 |
| scene_no_baseline / scene_within_1_2x / scene_over_line_detected / scene_zero_baseline_not_comparable | 【验收判据】「下载中打开目录」≤ 无下载时的 1.2 倍 | 均值后再比；缺对照/零基线不判达标 |

## K1 共通 · 十二查登记册 `k1walk`（12 项，挂 F041）

| 检查项 | 分工书锚点 | 验证内容 |
| --- | --- | --- |
| walk_17_domains_unique | 总表 17 项 | 十七域登记齐全且标签唯一 |
| walk_all_complete | 十二查 6/8 | 名称 + 说明 + 理由 + 路径链 + 双入口 + 落位控件自洽 |
| walk_paths_within_4 | 十二查 7「≤4 段路径链」 | 1..=4 段且无空段/无残值 |
| walk_blurb_rendered | 十二查 8「说明句三件套」 | 渲染出「名称：说明（控件）」 |
| walk_path_rendered | 十二查 7「逐步走达」 | 渲染 `A > B > C` |
| walk_locus_distribution | 十二查 6 三落位 | A/B/C 均有分布（全 C = 用户永远没入口） |
| walk_locus_control_coherent | 十二查 6/8 | C 类不得配可持久控件（ReadOnly 除外） |
| walk_dpi_four_steps | 十二查 4「四档 DPI」 | 100/125/150/200 齐 |
| walk_dpi_checked_all | 十二查 4 | 十七域均登记已走查（未走查不冒充） |
| walk_perf_lines_registered | 十二查 3「帧率/延迟/内存数值」 | 八条硬线域均给出数值 |
| walk_control_persistable | 十二查 8 | 按钮无持久值，开关/只读有 |
| walk_lookup_by_domain | 对账单一源 | 按域可查，越界返回 None |

## K1 共通 · 用户故事场景 `k1scene`（36 项，前段 23 挂 F041 / 后段 13 挂 F057）

| 检查项 | 主册【用户故事】 | 验证内容 |
| --- | --- | --- |
| scene_f041_drag_stutter / scene_f041_axis | F041「300ms 里有五帧超了 12.5ms 红线」 | 18 帧中 5 帧红标；多一帧即场景对不上 |
| scene_f042_copy_stall / scene_f042_ticket_line | F042「卡顿时刻 IO 阻塞类占比 80%」 | 800‰ + 头号 = io-block + 证据达阈 + 工单行可产出 |
| scene_f043_vscode_profile / scene_f043_second_launch | F043「装载 1.2s/重定位 0.4s/首帧 6.1s/可交互 8.3s」 | 大头在首帧；二次提速 535‰ |
| scene_f044_preheat | F044「首次 40MB、二次只预读 9MB」 | 省 775‰；页数 10240 → 2304 |
| scene_f045_image_flood / scene_f045_tier_drift | F045「连续打开几十张 4K 图后依然流畅」 | 40 张洪峰不 OOM 且回收在干活；档位下沉 |
| scene_f046_unplug / scene_f046_worst_loss / scene_f046_unplug_too_early | F046「写完文档立刻拔 U 盘」 | 等满 1s 零丢失；差 1ms 如实判失败；最坏丢失可算 |
| scene_f047_mixed_load / scene_f047_interaction_first / scene_f047_input_over_fails | F047「音频+拖动+构建并发下输入类 p99 仍达标」 | 四类各自达标且总和 ≤2000μs；输入超线即失败 |
| scene_f048_search_burst / scene_f048_burst_too_slow_fails | F048「按一下搜索框——0.5 秒满频又降回」 | 42ms 满频 + 保持 500ms；80ms 即失败 |
| scene_f049_idle_download / scene_f049_wakes_explained / scene_f049_unexplained_fails | F049「挂机下载时合成器一栏几乎是零」 | CPU 3‰ + 唤醒 6 次；不可解释唤醒必须为 0 |
| scene_f050_wheel_flood / scene_f050_merge_rate / scene_f050_drop_fails | F050「每秒几百个滚轮事件——跟手不跳」 | 420/s 零丢弃、延迟 6ms、合并率 857‰ |
| scene_f051_scroll_hour | F051「长文档连续滚动一小时」 | TLB miss 降 700‰；216,000 帧 |
| scene_f052_week_burn / scene_f052_frag_over_fails | F052「连续运行一周后依然秒级响应」 | 碎片 120‰ + P99 700ns；160‰ 即失败 |
| scene_f053_boot_74 | F053「按电源到看到桌面 7.4 秒」 | 总量 7400ms 在 8s 线内；感知 6980ms 更短 |
| scene_f054_wallpaper_swap / scene_f054_bitwise_mismatch_fails | F054「4K 壁纸换上的瞬间已渲染完成」 | 120/95ms 双线达标；对拍不一致即不交付 |
| scene_f055_long_doc / scene_f055_hit_low_fails | F055「500 页文档滚动 10 分钟」 | 方差 120‰ + 命中 940‰；命中 850‰ 即失败 |
| scene_f056_typing_triple / scene_f056_cursor_repaint_fails | F056「打字 + 光标闪烁 + 时钟走秒三件事同时」 | 三小块合计 <5% 屏；光标零重绘 |
| scene_f057_download_open_dir / scene_f057_bg_starved_fails / scene_f057_eta | F057「后台下载 2GB 同时打开文件管理器」 | 1150‰ ≤ 1200‰；后台未完成即失败；ETA 256s |

---

## 对账结论

1. **541/541 全绿**：每项检查 = 主册某句判据/设计细节/状态异常/数据存储的可执行验证（断言在 `run_*_checks()` 构造时立即求值，宿主测试通过 = 同一套判据全绿，无第二套真相）。
2. **三轮深化的归属**：深化一（证据桥/风暴通道/谱直方图/池尾计划）+ 深化二（监视器数据供给面六件）+ **深化批次三（十七域深化件 + 十二查登记册 + 十七场景，+364 项）**，全部锚定主册原文或分工书通用十二查条款，零无锚检查项。
3. **容量纪律已验证**：`CheckSet::MAX_CHECKS = 64`，本次实测逐域计数 57/32/25/27/32/27/23/31/30/22/27/37/27/35/35/34/40，**无一触顶、零截断**（此前并 36 项场景进 F041 曾静默丢 6 项，已按零静默纪律拆段挂载修正）。
4. **登记随闸门**：实机类判据（flood 硬件级、TLB 性能计数器、MSR 频率写、断电百次、4K 四档走查、真机录屏）的**数据面与模型面**已在本表覆盖，**实测数字**按工作制度「开发期零 QEMU」登记随闸门补测（K1 报告 §7 口径不变）。

---

## 深化批次四对账（mech_* 十二件 · +46 项 · 587/587 全绿）

批次四把批次一~三判据账本下方的**算法本体**补齐为十二个共用/域专属件（mech_*），逐件挂接进消费域的 `run_*_checks()` 检查链。每件判例 = 主册判据的算法级验证（输入构造 → 算法执行 → 与主册口径对拍）。

| 件 | 挂接域（+项数） | 新增检查项 → 主册锚点 |
| --- | --- | --- |
| mech_sim | F041（+6） | eq_stable_order=同刻事件按插入序展开（确定性回放前提）· digest_reproducible=同种子摘要逐位相等/异种子必异 · clock_no_time_travel=时钟拒绝回拨（F182 同语义）· rfc1951_tables_exact/bitwriter_dual_order/code_symbol_mapping=编码侧表与位序锚点（RFC1951 §3.2.5/§3.1.1） |
| mech_stats | F042（+5） | welford_offline_match=单遍方差与离线两遍法一致（整数截断 ±5% 容差登记）· cv_undefined_when_empty=零样本不冒充稳定 · isqrt_exact=u64 全域精确 · slide_and_ewma=滑窗峰值 + EWMA 首样本锚定 · hysteresis_zones=迟滞三分区（Above/Band/Below，回线不抖动底盘语义） |
| mech_clocklru | F044（+3） | clock_second_chance=置位页第二次机会/未置位页先逐 · full_pin_honest=全 pin 不逐不假成功 · lruk_flood_survival=顺序洪峰不逐 K=2 热点（O'Neil 1993 语义） |
| mech_scan | F045（+3） | 水位三档驱动 balance_zone 批量回收、congestion_waits/gave_up 归因计数、TrickleZero 预算钳制与 armed 立即停——F045「连续打开几十张 4K 图后依然流畅」的回收循环本体 |
| mech_radix | F046（+3） | dirty_set_clear_roundtrip=跨位图字边界标脏/清脏/二次清脏报 false · pop_range_longest_run=连续段弹出（合并写下盘接口）· pool_recycle_and_exhaustion=节点回收不随历史增长 + 池满 exhausted 诚实计数 |
| mech_tokens | F047（+3） | TokenBucket restore 事务性回滚 · Htb 叶不足→父借用→失败回滚叶预扣（预算闸门不凭空放行）· PidGov 漏积分 anti-windup + shift clamp（F047 输入类 p99 预算的执行机构） |
| mech_wheel | F050（+3） | insert_drain_order=时刻序到期抽干 · same_key_coalesces=同键同刻覆盖 covered_away 计数（覆盖只发生在槽内）· drain_output_cap=出参容量钳制 + fired 含未交付 + 到期项一律消费防重复触发 |
| mech_buddy | F051（+4） | 伙伴异或恒等式 + 物理摘链合并（state 预判缺陷修复后回归）· free_with_live_buddy 不上卷（碎片率下界成立）· exhaustion_is_honest · 七天分配谱压力模拟（F052 碎片判据的分配语义本体，挂 F051 域容量余量内） |
| mech_dag | F053（+3） | topo_and_cycle_detect=Kahn + SelfDep/Cycle 检出 · critical_path_exact=关键链手算对拍（ACPI→PCI→USB→挂载 4600μs，掩码 0b010111）· parallel_beats_serial_conserves=LPT 并行严格优于串行 + 工作守恒（dispatched 标记修复后回归） |
| mech_inflate | F054（+8） | zlib_stored_roundtrip=手造 zlib 流头/尾校验全链 · dynamic_huffman_handmade=动态表手造位流（HCLEN/CLC_ORDER/重复码全路径）· png_container_roundtrip=RGB 两行两种滤波容器全链（含 Up 滤波 prev 行 off-by-one 缺陷回归）· crc_adler_enforced=CRC 与 Adler 篡改必拒（零静默）· truncated_and_sig_rejected · adam7_plan_totals=七遍像素总和恒等式（多组尺寸）· Adam7 真解码逐像素一致 · 编码器↔解码器 round-trip |
| mech_edfq | F057（+5） | edf_orders_by_deadline_then_fifo=硬截止期主序 · batch_soft_deadline=批量软截止期非零 · aging_pulls_batch_forward=老化提升反向保护 · wfq_weight_order_no_starve=带内 min-vtime 份额趋向 8:2:1 且批量不归零 · heap_full_honest=满堆拒绝计数 |

### 批次四对账结论

1. **587/587 全绿**：批次四 +46 项全部锚定主册判据的算法本体（RFC1950/1951、O'Neil LRU-K、CLOCK 第二次机会、HTB+PID、Kahn/LPT、伙伴系统异或恒等式、Welford、Adam7 平面恒等式），零无锚检查项。
2. **验证抓出真实现缺陷 6 处**（buddy merge 预判、clocklru 插入置位、edfq WFQ 退化为 FIFO、inflate zlib 头未剥离、inflate prev 行 off-by-one、dag 同任务多派），全部修复并以回归判例固化——判例账 = 缺陷账，零静默。
3. **逐域计数** 63/37/25/30/35/30/26/31/30/25/31/37/30/43/35/34/45，无一触 MAX_CHECKS=64、零截断。
4. **舱内外一致**：12 件与挂接域文件双侧逐字节 diff 一致；全量重门禁随闸门（§12.3/§13.3 口径）。

## 批次五对账（机制总成·续九件 · 638/638 全绿收口）

### 批次五新增检查项（+51 · 逐件登记）

| 件 | 挂接域（+项数） | CheckSet 判例登记（名 = 语义） |
| --- | --- | --- |
| mech_bloom | F046（+6） | bloom_no_false_negative=零假阴性硬面 · bloom_fpr_near_theory=FPR 实测 vs (1-e^{-kn/m})^k 对拍 · cb_delete_removes=计数布隆删除生效 · cb_saturate_honest=计数器饱和 15 钳制不翻转 · cb_no_false_negative=计数布隆零假阴性 · bloom_density_bounded=位密度有界 |
| mech_cfs | F047（+6） | cfs_fair_share=8:2:1 权重份额趋向 · cfs_nice_order=nice 低者先行 · cfs_starvation_free=新实体 vruntime 锚定不饿死 · cfs_min_vruntime_monotonic=基准单调不回退 · cfs_escape_guard=睡醒实体逃逸防护 · cfs_pick_deterministic=同 vruntime 确定序 |
| mech_coalesce | F050（+5） | co_high_rate_saves=高速率 10:1 合并 · co_latency_cap=最老事件 ≤ MAX_LATENCY · co_window_grows=密集流窗口上探 · co_window_shrinks=稀疏流窗口回落 · co_full_batch_flush=批满即报不丢 |
| mech_gov | F048（+6） | od_burst_to_max=80% 阈值一步满频 · od_down_slow=降频逐级节流 · od_mid_map=中载映射落表 · su_proportional=util 比例直映 · su_rate_limit=限频窗内 pending 压制 · su_cap_clamped=util 饱和钳制 |
| mech_oom | F045（+6） | oom_biggest_hog=最大占用者优先 · oom_protected_skipped=-1000 豁免跳过 · oom_adj_bias=adj 偏置传导评分 · oom_pgtable_counted=页表页计账 · oom_cascade=连环处决至水位停 · oom_no_victim_honest=全员豁免诚实报错 |
| mech_readahead | F044（+5） | ra_sequential_grows=顺序命中窗口翻倍 · ra_random_shrinks=随机访问降窗 · ra_window_capped=窗口 32 页封顶 · ra_hit_accounted=命中账与行为一致 · ra_async_overlap=预读与缺页解耦 |
| mech_slab | F052（+7） | slab_roundtrip=申请释放指纹回环 · slab_inbounds_ok=恰好写满合法 · slab_overflow_caught=越界 1 字节金丝雀必检 · slab_double_free=双重释放检出 · slab_class_map=200→256 类正确 · slab_churn_stable=300 轮峰值零增长 · slab_exhaust_honest=耗尽诚实计数 |
| mech_thermal | F048（+4） | th_debounce_enter=进入双样本确认 · th_debounce_exit=退出双样本+迟滞 · th_sticky_critical=Critical 粘滞仅复位可离 · th_cap_mapping=档位→频率封顶映射 |
| mech_zram | F045（+6） | zr_roundtrip_text=文本页编解码回环 · zr_roundtrip_binary=二进制图样回环 · zr_zero_page=全零页高压缩 · zr_overlap_run=重叠匹配（off=1）语义 · zr_incompressible_raw=不可压直存诚实标志 · zr_saved_accounting=节省记账与 2:1 门槛 |

### 批次五对账结论

1. **638/638 全绿**：批次五 +51 项全部锚定主册判据的机制本体（LZ4 块格式、CFS vruntime、ondemand/schedutil、oom_badness、SLAB 金丝雀、热状态机迟滞），零无锚检查项；逐件计数 6/6/5/6/6/5/7/4/6，无一触 MAX_CHECKS=64、零截断。
2. **验证抓出真实现缺陷 5 处**（zram 流尾匹配被拒、slab 顶层吞 Corrupt、gov 限频窗永不开、coalesce EWMA 定点单位错、gov 降频判例 off-by-one）+ 判例修正 4 处，全部修复并以回归判例固化——判例账 = 缺陷账，零静默（明细见完成报告 §14.2）。
3. **舱内外一致**：59 文件双侧逐字节 cmp 一致；主 crate 全量门禁随闸门（§12.3/§13.3/§14.3 口径）。
4. **累计口径**：纯功能 19,114 / 87,425 = 21.9%；检查项 638；单测 381。批次六（集成对接件）已登记于完成报告 §14.4 续作清单。
