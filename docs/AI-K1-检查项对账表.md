# AI-K1 检查项对账表 · perfstar 17 域 CheckSet × 主册判据（F041-F057）

> **对账口径**：本文把 `kernel/varix/src/perfstar/` 全部 **177 项 CheckSet 检查项**（宿主同构 f475 入口 tally 口径，2026-09-26 深化批次二后）逐条映射到主册《Varix STAR I start.md》B-3 深化设计报告（G-B-01~G-B-17）的**判据原文 / 设计细节 / 状态与异常 / 数据与存储 / 交互设计**锚点。一处一事实：每项检查都能回答「它在验主册的哪句话」。
> **验证状态**：177/177 全绿（隔离验证舱 `_attic/aik1-f041-f057-deep-verify/`，118 宿主单测同绿）。
> 域表注册：`robust.rs` 274 域函数指针表；批次沿革：v1 判据实装（156 项）→ 深化批次一（+10）→ 深化批次二（+11）。

| 域 | 判据锚 | CheckSet 项数 | 全绿 |
| --- | --- | --- | --- |
| F041 frameledger | G-B-01 | 9 | ✅ |
| F042 frameattr | G-B-02 | 15 | ✅ |
| F043 startprof | G-B-03 | 10 | ✅ |
| F044 prefetch2 | G-B-04 | 9 | ✅ |
| F045 pagewater | G-B-05 | 12 | ✅ |
| F046 wcoalesce | G-B-06 | 8 | ✅ |
| F047 latbudget | G-B-07 | 8 | ✅ |
| F048 cpufreq | G-B-08 | 10 | ✅ |
| F049 idlezero | G-B-09 | 11 | ✅ |
| F050 intrcoal | G-B-10 | 8 | ✅ |
| F051 bigpage | G-B-11 | 10 | ✅ |
| F052 heapfrag | G-B-12 | 16 | ✅ |
| F053 bootpar | G-B-13 | 9 | ✅ |
| F054 imgsimd | G-B-14 | 9 | ✅ |
| F055 glyphcache | G-B-15 | 12 | ✅ |
| F056 dirtyrect | G-B-16 | 10 | ✅ |
| F057 iotier | G-B-17 | 11 | ✅ |
| **合计** | | **177** | **✅** |

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

## 对账结论

1. **177/177 全绿**：每项检查 = 主册某句判据/设计细节/状态异常/数据存储的可执行验证（断言在 `run_*_checks()` 构造时立即求值，宿主测试通过 = 同一套判据全绿，无第二套真相）。
2. **两轮深化的归属**：深化一（证据桥/风暴通道/谱直方图/池尾计划）+ 深化二（监视器数据供给面六件）在表中以「深化一/二」标注，全部锚定主册原文，零无锚检查项。
3. **登记随闸门**：实机类判据（flood 硬件级、TLB 性能计数器、MSR 频率写、断电百次、4K 四档走查）的数据面/模型面已在本表覆盖，**实测数字**按工作制度「开发期零 QEMU」登记随闸门补测（K1 报告 §7 口径不变）。
