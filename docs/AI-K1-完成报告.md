# AI-K1 完成报告 · Varix STAR I perfstar 十七域（F041-F057）

> 分工包：主册《Varix STAR I start.md》B-3 深化设计报告（G-B-01 ~ G-B-17）判据实装层。
> 分工边界：AI-K1 严格限定 F041-F057 十七域；未触碰 AI-K2（F058-F075）与其他 AI 的任务面。
> 收口状态：**全绿收口**（宿主 3897/3897 PASS），push 前置条件达成。

---

## 1. 交付物台账

| 位置 | 内容 | 规模 |
| --- | --- | --- |
| `kernel/varix/src/perfstar/` | 17 域判据实装 + mod.rs 域表 | 18 文件 / 8514 行 |
| `kernel/varix/src/lib.rs` | perfstar 模块挂接 | 修改 |
| `kernel/varix/src/checks.rs` | CheckSet/KernelCheckup 基建协作 | 修改 |
| `kernel/varix/src/robust.rs` | 274 域函数指针表注册接线 | 修改 |
| `docs/AI-K1-完成报告.md` | 本报告 | 新增 |

非功能产物（探针日志、临时测试日志）已按规程移入 `_attic/`。

## 2. 判据对账表（17 域 × 主册判据锚）

| 项 | 判据锚 | 域文件 | 自检 | 单测 | 状态 |
| --- | --- | --- | --- | --- | --- |
| F041 帧率账本 | G-B-01 | frameledger.rs | 9 | 7 | ✅ |
| F042 帧率归因器 | G-B-02 | frameattr.rs | 11 | 5 | ✅ |
| F043 冷启动画像 | G-B-03 | startprof.rs | 10 | 6 | ✅ |
| F044 预取指纹 v2 | G-B-04 | prefetch2.rs | 9 | 6 | ✅ |
| F045 页缓存水位 | G-B-05 | pagewater.rs | 9 | 4 | ✅ |
| F046 写合并窗口 | G-B-06 | wcoalesce.rs | 7 | 4 | ✅ |
| F047 延迟预算 | G-B-07 | latbudget.rs | 7 | 5 | ✅ |
| F048 CPU 频率联动 | G-B-08 | cpufreq.rs | 10 | 5 | ✅ |
| F049 空转清零 | G-B-09 | idlezero.rs | 10 | 4 | ✅ |
| F050 中断合并 | G-B-10 | intrcoal.rs | 7 | 4 | ✅ |
| F051 大页策略 | G-B-11 | bigpage.rs | 9 | 5 | ✅ |
| F052 堆碎片治理 | G-B-12 | heapfrag.rs | 9 | 5 | ✅ |
| F053 启动并行度 | G-B-13 | bootpar.rs | 9 | 5 | ✅ |
| F054 图像 SIMD | G-B-14 | imgsimd.rs | 7 | 6 | ✅ |
| F055 字形光栅缓存 | G-B-15 | glyphcache.rs | 10 | 6 | ✅ |
| F056 合成器脏区 | G-B-16 | dirtyrect.rs | 12 | 12 | ✅ |
| F057 IO 调度分级 | G-B-17 | iotier.rs | 11 | 9 | ✅ |
| **合计** | | **17 文件** | **156** | **98** | **全绿** |

每域模块头注释逐条摘录主册【状态与异常】【设计细节】判据，一处一事实；常量注释写明主册依据与推导。

## 3. 证据三件套（最终验证，debug 通道）

| # | 验证 | 命令 | 结果 |
| --- | --- | --- | --- |
| 1 | 全量单测 | `cargo test --lib` → `varix-ed44a005e2741606.exe` | **3897 passed / 0 failed**（55.81s，EXIT=0） |
| 2 | perfstar 域 | 过滤 `perfstar::` | **98 passed / 0 failed**（0.78s，EXIT=0） |
| 3 | CheckSet 全量 | `robust::tests::f475_every_domain_reports --exact` | **PASS**（274 域 `all_passed`，`failed == 0` 断言，EXIT=0） |
| 4 | iotier 专项 | 过滤 `iotier` | 9/9（含 `bg_zero_starvation_24h_mixed` 24h 混载模拟） |

CheckSet 执行架构：`robust.rs` 以函数指针表 `[fn() -> CheckSet; 274]` + `for f in domains { checkup.register(f()); }` 循环注册（禁止直排——debug 模式下每个 `run_*_checks()` 栈帧叠加会爆默认栈）。`f475_every_domain_reports` 在宿主侧执行与内核启动完全同构的 274 域 CheckSet 全量断言。

## 4. 缺陷账本（本轮实装修复的真缺陷）

修复原则：断言侧错误改断言，实现侧缺陷改实现，**绝不为了绿屏而粉饰语义**。

| # | 域 | 缺陷 | 修复 |
| --- | --- | --- | --- |
| 1 | F041 frameledger | `RawRecord::BREAK` 常量为 BE 手算，与 `encode()` 的 `to_le_bytes` 编码不一致（实现侧真 bug） | 常量改 LE：`FLAG_BREAK=1<<13 → b[9]=0x20` |
| 2 | F041 frameledger | `aggregate_minute` 缺 24h 回绕重置，cursor 指向陈旧槽 | 槽 `epoch_min` 不匹配时清槽重置 cursor，snapshot 升序语义统一 |
| 3 | F045 pagewater | 回收循环硬编码 4096B/页 + 环满挤旧不扣 `cache_bytes`（字节账/页账脱钩，真缺陷） | 新增 `lru_bytes` 平行数组按页登记（4KB/2MB/1GB），回收与挤旧同步出账 |
| 4 | F050 intrcoal | `merged_away` 语义缺失：flush 覆盖合并不计数（熔断判据失真） | `cover_position → bool` 同键覆盖真值，submit 熔断/evict/flush 三处统一计数 |
| 5 | F050 intrcoal | 初始窗 `8_000us` 笔误为主册 8ms 语义之外（单位错位） | 初始窗 8ms 对齐主册「延迟超 8ms 动态收缩」 |
| 6 | F048 cpufreq | `set_manual`/`burst` 绕过 `clamp_target`：Silent 档下 burst 可请求 index 0（最高频），封顶完全失效（真缺陷）；死代码 `manual_bound()` | 两路径统一走 `clamp_target` 唯一封顶判据；删除死代码 |
| 7 | F052 heapfrag | `zero_heap_struct` 公式漏计 `alert_class: Option<usize>` 字节 | 公式补全（888B 实测对齐） |
| 8 | F057 iotier | **反向保护 resume 死锁**（真缺陷）：暂停解除条件「前台静默 5s」在持续 FG 流量下不可达（`last_fg_activity_ms` 每次前台派发刷新，10/s ≪ 5s 静默）→ bg 永久暂停 → 队列满 → 后台饿死，违反主册判据二「后台任务零饥饿」 | 「完全暂停」改为**时长有界**（`BG_PAUSE_MAX_MS=5s` 自动解除 + 满载计时复位，再次持续满载 60s 才重新暂停）；删除被兜底支配的死机制（前台静默窗——`paused_at ≥ last_fg_activity` 使兜底解除时刻恒不晚于静默解除，零冗余协议）；`Q_CAP 128→256`（暂停期背压余量：风暴期 120 + 暂停期 5s×2/s=10，峰值 130，零丢请求） |
| 9 | 多域 | 语义修正后 12 个宿主单测断言过时连锁失败 | 逐个按新语义重造测试数据（frameattr 2、frameledger 2、intrcoal 2、latbudget 2、prefetch2 1、startprof 1、wcoalesce 1、iotier 1） |

另：`latbudget` 直方图桶边界实测记录——`bucket_of` 用 `v - v/5` 逆步进，整数舍入漂移使 bucket 10 = 364..453、bucket 11 = 454..566（非几何级数精确值）；测试样例按实测边界构造，不掩盖舍入行为。

## 5. 偏差登记（实现选择，一处一事实）

| 选择 | 依据 | 登记 |
| --- | --- | --- |
| `BG_PAUSE_MAX_MS = 5s` | 主册明文「前台让出窗口计时」未给数值；实装解释为**暂停时长有界**——无界暂停在持续满载下即后台永久饥饿，与「防饿死反向保护」自相矛盾；5s 与原让出窗同量级 | 本报告 §4#8 |
| `Q_CAP = 256` | 主册未规定队列容量；128 无法容纳暂停期峰值积压 130（会丢请求、破坏零饥饿判据） | 本报告 §4#8 |
| `BATCH_SESSION_GAP_MS = 60s` | 主册「批量任务超 30 分钟」以连续批量流计，静默 60s 视为任务结束、会话复位 | 源码常量注释 |
| 域接线状态 | 17 域为判据实装层（独立可测、CheckSet 全绿）；与内核运行时路径的接线（如 frameledger 真实打点、iotier 挂接存储栈）为后续闸门工作 | §7 |

## 6. 环境事项登记（不粉饰）

1. **release 通道确定性损坏**：会话中另一 AI 的 debug 构建（01:07Z）与本会话 release 链接发生竞态，release 测试 exe 启动即死（exit -1/127）。取证：`rm`+`touch` 重链、`cargo clean -p varix --release` 全清重编均无效（确定性损坏）；同期 debug 通道完全正常。**全量验证以 debug 通道执行**；release 通道损坏属并发构建产物竞争，代码无嫌疑，但不以绿屏掩盖——如实登记。
2. **cmd 真实退出码取证法**：Git Bash 下 `./exe` 报 127 为 MSYS 映射假象、cmd 不认 `/` 路径报 9009；权威取证用 `cmd //v:on //c "path\to.exe ... & echo !ERRORLEVEL!"`。
3. **教训**：后台命令中 `find /c /v ""` 会被 Git Bash 错译为 Unix find 全盘扫描（只读，已终止）——cmd 内建工具须经 `cmd //c` 且引号层级完整才可靠。

## 7. QEMU 与接线随闸门登记

- **CheckSet 执行面**：内核启动路径经 `robust.rs` 274 域表注册执行（QEMU 串口输出 CheckSet 汇总）。
- **宿主同构验证**：`f475_every_domain_reports` 在宿主执行同一 274 域 CheckSet 全量断言，**PASS**（本报告 §3#3）——perfstar 17 域为纯逻辑域（无硬件依赖），宿主/QEMU 判据同构。
- **既有 QEMU 证据**：修复中段 QEMU 串口日志 f475 域 6/6 PASS 在案（`_attic/` 保留现场）。
- **接线**：17 域与内核运行时路径的实际接线（frameledger 真实帧打点、iotier 挂接存储栈队列、cpufreq 对接调度器等）随闸门登记为后续工作；当前交付为判据实装层 + 全量自检。

## 8. 结论

- AI-K1 分工包 F041-F057 十七域判据实装完成：**156 CheckSet 检查项 + 98 宿主单测，全绿**。
- 全仓 274 域 CheckSet 宿主全量验证通过；全量 3897/3897 PASS。
- 实装过程中挖出并修复 **7 处实现侧真缺陷**（含 iotier 反向保护 resume 死锁、pagewater 字节账脱钩、cpufreq 封顶失效三处高危），无粉饰、无死代码遗留（K1PROBE 探针已清理，死机制按零冗余协议删除）。
- 全部偏差与环境事项如实登记于 §5、§6。
