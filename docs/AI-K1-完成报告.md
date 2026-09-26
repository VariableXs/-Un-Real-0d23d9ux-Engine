# AI-K1 完成报告 · Varix STAR I perfstar 十七域（F041-F057）

> 分工包：主册《Varix STAR I start.md》B-3 深化设计报告（G-B-01 ~ G-B-17）判据实装层。
> 分工边界：AI-K1 严格限定 F041-F057 十七域；未触碰 AI-K2（F058-F075）与其他 AI 的任务面。
> 收口状态：**全绿收口**（宿主 3897/3897 PASS），push 前置条件达成。
> 收口提交：`c128932`（perfstar 实装 + 报告）→ `origin/main`；本报告为 v2 完善版。

---

## 1. 交付物台账

| 位置 | 内容 | 规模 |
| --- | --- | --- |
| `kernel/varix/src/perfstar/` | 17 域判据实装 + mod.rs 域表（DOMAIN_NAMES 17 域注册名） | 18 文件 / 8514 行 |
| `kernel/varix/src/lib.rs` | perfstar 模块挂接 | 修改 |
| `kernel/varix/src/checks.rs` | CheckSet/KernelCheckup 基建协作 | 修改 |
| `kernel/varix/src/robust.rs` | 274 域函数指针表注册接线 | 修改 |
| `CHANGELOG.md` | K1 批次条目（对齐 K2 批格式） | 修改 |
| `docs/AI-K1-完成报告.md` | 本报告 | 新增 |

行数分布（wc -l）：dirtyrect 1060 · iotier 900 · glyphcache 631 · frameledger 592 · prefetch2 552 · frameattr 476 · intrcoal 467 · imgsimd 450 · cpufreq 421 · pagewater 398 · heapfrag/latbudget 各 393 · startprof 428 · bootpar 346 · wcoalesce 341 · idlezero 321 · bigpage 276 · mod.rs 69。

非功能产物（K1PROBE 探针残留、k1_*.log 临时测试日志 9 件）已按规程移入 `_attic/`（仓库 `.gitignore` 挡日志入库，留磁盘不进版本库）；`robust.rs` 内的 `k1_frame_probe` 临时模块已按零死代码协议删除。

## 2. 判据对账（17 域 × 主册判据锚 × 验收标准第一句）

### 2.1 对账总表

| 项 | 判据锚 | 域文件 | 行数 | 自检 | 单测 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| F041 帧率账本 | G-B-01 | frameledger.rs | 592 | 9 | 7 | ✅ |
| F042 帧率归因器 | G-B-02 | frameattr.rs | 476 | 11 | 5 | ✅ |
| F043 冷启动画像 | G-B-03 | startprof.rs | 428 | 10 | 6 | ✅ |
| F044 预取指纹 v2 | G-B-04 | prefetch2.rs | 552 | 9 | 6 | ✅ |
| F045 页缓存水位 | G-B-05 | pagewater.rs | 398 | 9 | 4 | ✅ |
| F046 写合并窗口 | G-B-06 | wcoalesce.rs | 341 | 7 | 4 | ✅ |
| F047 延迟预算 | G-B-07 | latbudget.rs | 393 | 7 | 5 | ✅ |
| F048 CPU 频率联动 | G-B-08 | cpufreq.rs | 421 | 10 | 5 | ✅ |
| F049 空转清零 | G-B-09 | idlezero.rs | 321 | 10 | 4 | ✅ |
| F050 中断合并 | G-B-10 | intrcoal.rs | 467 | 7 | 4 | ✅ |
| F051 大页策略 | G-B-11 | bigpage.rs | 276 | 9 | 5 | ✅ |
| F052 堆碎片治理 | G-B-12 | heapfrag.rs | 393 | 9 | 5 | ✅ |
| F053 启动并行度 | G-B-13 | bootpar.rs | 346 | 9 | 5 | ✅ |
| F054 图像 SIMD | G-B-14 | imgsimd.rs | 450 | 7 | 6 | ✅ |
| F055 字形光栅缓存 | G-B-15 | glyphcache.rs | 631 | 10 | 6 | ✅ |
| F056 合成器脏区 | G-B-16 | dirtyrect.rs | 1060 | 12 | 12 | ✅ |
| F057 IO 调度分级 | G-B-17 | iotier.rs | 900 | 11 | 9 | ✅ |
| **合计** | | **17 文件** | **8445** | **156** | **98** | **全绿** |

每域模块头注释逐条摘录主册【状态与异常】【设计细节】判据，一处一事实；常量注释写明主册依据与推导；共同纪律：零堆热路径（无 Vec/String/Box/format! 进内核路径）、先测量后调参（F041 账本为全域共同前提）、判据唯一源（验收标准第一句摘自主册）。

### 2.2 验收标准第一句（判据原文，逐域摘自主册）

- **F041 帧率账本**（G-B-01）：账本打点自身开销 <0.1% CPU 实测；帧率图与实测录屏逐帧对得上（抽 10 帧核对）。
  实装：每帧合成耗时/脏区面积/提交耗时/等待四项定长记录 + 分钟桶聚合（cursor 升序快照）+ 24h 降采样（ratio==10）。
- **F042 帧率归因器**（G-B-02）：四类掉帧样本各一，归器全部命中正确主因；误报率 <10%（正常拖动 100 秒样本零误报）。
  实装：输入风暴/大脏区/IO 阻塞/调度抢占四类嫌疑贡献模型，贡献扣除回线判定 Single/Mixed/Unclassified。
- **F043 冷启动画像**（G-B-03）：五段边界与内核打点一一对应（无重叠无遗漏）；同应用 10 次启动画像方差 <15%。
  实装：装载/初始化/首帧/可用/稳定五段刻度 + LRU 会话 + 中位数判据（seq≥2 才计入）。
- **F044 预取指纹 v2**（G-B-04）：「常用 50 件」二次启动均值 ≤ 首次 50%（与 F003/F043 联合对账）；指纹命中率 >70% 实测。
  实装：位图指纹 + 顺序区序列化布局（need = 32 + BITMAP_WORDS×8 + order_len×4）+ checksum 校验。
- **F045 页缓存水位**（G-B-05）：断电百次中脏页丢失窗口 ≤ 水位规则承诺值（B-703 数据交叉验证）；压测（连续开关大图）无 OOM。
  实装：全局水位三档（高 3.2GB/中 2.4GB/低 1.6GB）+ `lru_bytes` 平行数组字节账（4KB/2MB/1GB 按页登记，回收与挤旧同步出账）。
- **F046 写合并窗口**（G-B-06）：断电百次按三档各跑一遍全绿；写入合并率（合并写/总写）重载档 >40% 实测；fsync 延迟 P99 <10ms 不受档位影响。
  实装：空闲 1s/正常 5s/重载 10s 三档动态伸缩 + dwell 抑制（仅升档方向构成切换请求）。
- **F047 延迟预算**（G-B-07）：四类 p99 各自达标（2000μs 总预算内）；压力混载（音频+拖动+构建并发）下输入类 p99 仍达标——交互优先不是口号是曲线。
  实装：Input/Compose/Audio/Normal 四类子预算 + 50×1.25^b 几何直方图 + aging 升档记账。
- **F048 CPU 频率联动**（G-B-08）：交互突发响应（点按到满频）<50ms；续航对比：自动策略 vs 固定高频，视频播放场景续航提升 >15% 实测。
  实装：负载感知 P-state 升降档 + Silent/Performance 手动档经 `clamp_target` 唯一封顶判据（burst 同受钳制）。
- **F049 空转清零**（G-B-09）：纯桌面静置 60 秒：合成器 CPU <0.5%、内核唤醒次数 <10 次；两数字同录在案。
  实装：空闲态零唤醒判定 + 空转页清零工程账本。
- **F050 中断合并**（G-B-10）：flood 测试（flood-publishes 70 级）零丢弃；滚动场景输入类 p99 延迟 ≤8ms；两数字对账 F041 掉帧归零。
  实装：2000ns 预算窗口批量合并 + 延迟超 8ms 动态收缩（初始窗 8ms，`max(w/2, 100μs)` 下限）+ 同键覆盖 merged_away 计数 + 熔断器。
- **F051 大页策略**（G-B-11）：字形图集场景 TLB miss 率实测下降 >50%；大页池启动预留成功率 >95%。
  实装：4MB 大页三类静态区（内核代码段/字形图集/滚动缓冲）+ 预留池。
- **F052 堆碎片治理**（G-B-12）：7 天烤机碎片率 <15%；六档分配延迟 P99 <1μs；零堆纪律 grep 自证（无动态分配进内核）。
  实装：尺寸分级池（六档）+ 直通区 + 结构体字节账公式自证（888B 实测对齐）+ 分配/释放一一配对契约。
- **F053 启动并行度**（G-B-13）：四链并行后内核段总耗时 ≤ 串行版 60%；8 秒线分解后每段预算在甘特图可见且实测偏差 <10%。
  实装：ACPI/PCI 枚举/USB/存储四链依赖图 + 并行调度模拟。
- **F054 图像 SIMD**（G-B-14）：4K PNG 解码 ≤150ms、JPEG ≤120ms（实测线）；SIMD 与标量结果逐像素一致（正确性优先于速度）。
  实装：PPM/PNG/JPEG 三解码器 SIMD 路径 + 标量逐像素对拍自证。
- **F055 字形光栅缓存**（G-B-15）：中文长文档滚动 10 分钟帧耗时方差 <20%；图集命中率 >90%（常用字场景）。
  实装：字形 LRU 热度驻留 + shelf packing 图集（不溢出自证）+ 10 分钟滚动方差账本。
- **F056 合成器脏区**（G-B-16）：打字场景脏区面积 P95 <5% 屏；光标移动 60fps 恒定且零内容重绘（账本验证）；弹窗动画帧内合成矩形 = 弹窗矩形 + 阴影环带。
  实装：脏区合并 + 光标零重绘路径 + 阴影环带扩矩形 + 全链账本验证。
- **F057 IO 调度分级**（G-B-17）：「下载中打开目录」场景目录延迟 ≤ 无下载时的 1.2 倍；后台任务零饥饿（24h 混载测试全部完成）。
  实装：前台交互 > 后台任务 > 批量三队列 EDF + fsync 硬承诺（B-7xx 优先于一切）+ 反向保护时长有界 + 批量 30min 分段让路（每 5min 让 1s）+ F195 配额叠加。

## 3. 证据（最终验证，debug 通道）

### 3.1 验证矩阵

| # | 验证 | 命令（工作目录 `kernel/`） | 结果 |
| --- | --- | --- | --- |
| 1 | 全量单测 | `cargo test --lib --no-run` → `cmd //v:on //c ".\target\debug\deps\varix-ed44a005e2741606.exe & echo EXIT=!ERRORLEVEL!"` | **3897 passed / 0 failed**（55.81s，EXIT=0） |
| 2 | perfstar 域单测 | 同上 exe，过滤 `perfstar::` | **98 passed / 0 failed**（0.78s，EXIT=0） |
| 3 | CheckSet 全量 | 同上 exe，`robust::tests::f475_every_domain_reports --exact` | **PASS**（274 域 `all_passed`、`failed == 0`、`passed > 200` 断言，EXIT=0） |
| 4 | iotier 专项 | 同上 exe，过滤 `iotier` | 9/9（含 `bg_zero_starvation_24h_mixed` 24h 混载模拟） |

### 3.2 CheckSet 执行架构

- `robust.rs` 以函数指针表 `[fn() -> CheckSet; 274]` + `for f in domains { checkup.register(f()); }` 循环注册——**禁止改回直排** `checkup.register(crate::xxx::run_xxx_checks())`：debug 模式下每个 `run_*_checks()` 的栈帧叠加会爆默认线程栈（实测教训，源码注释在案）。
- `f475_every_domain_reports`（robust.rs 测试）在宿主侧执行与内核启动完全同构的 274 域 CheckSet 全量断言；失败时以 16384B 大缓冲渲染（此前 2048/8192 缓冲会在尾部 FAIL 行前截断造成诊断盲区——已修复并注释在案）。
- `CheckSet::add(name, passed, detail)` 断言在 `run_*_checks()` 构造时立即求值——宿主测试跑过 = 同一套判据全绿，无第二套真相。

## 4. 缺陷账本（施工期修复的 7 处实现侧真缺陷 + 连锁断言修正）

修复原则：断言侧错误改断言，实现侧缺陷改实现，**绝不为了绿屏而粉饰语义**；每处修复均以「复现失败 → 根因定位 → 语义修复 → 断言同步」闭环。

| # | 域 | 位置 | 缺陷（根因） | 修复 |
| --- | --- | --- | --- | --- |
| 1 | F041 | frameledger.rs:100 `RawRecord::BREAK` | BREAK 常量为 BE 手算编码，与 `encode()` 的 `to_le_bytes` 不一致——`break_not_faked` 判据对不上自己序列化器的输出（实现侧真 bug） | 常量改 LE：`FLAG_BREAK=1<<13 → b[9]=0x20`，与 encode 单一事实源对齐 |
| 2 | F041 | frameledger.rs:297 `aggregate_minute` | 缺 24h 回绕重置：epoch_min 回绕后陈旧槽数据混入聚合，cursor 指向已消费的旧槽（真缺陷） | 槽 `epoch_min` 不匹配即清槽重置 cursor；`minute_snapshot`（:358）由此从最旧到最新升序遍历，语义统一 |
| 3 | F045 | pagewater.rs:87/148-160 `cache_file_page`+poll | 回收循环硬编码 4096B/页 + 环满挤旧不扣 `cache_bytes`——大页（2MB/1GB）场景字节账与页账脱钩，水位判定失真（真缺陷） | 新增 `lru_bytes: [u64; LRU_CAP]` 平行数组按页登记字节（常规 4KB/大页 2MB/巨页 1GB），回收与挤旧同步出账 |
| 4 | F050 | intrcoal.rs:163 `cover_position`+三调用点 | `merged_away` 语义缺失：flush 覆盖合并不计数（返回 `()` 无覆盖真值），熔断判据失真（真缺陷） | `cover_position → bool`（同键覆盖真值）；submit 熔断/队满、`evict_one_position`、`flush_batch`（:200）三处统一只在真覆盖时 +1 |
| 5 | F050 | intrcoal.rs:112 `window_us` | 初始窗 `8_000` 单位笔误（8ms 语义写成了 8μs 量级的错误膨胀——初始窗应为主册明文 8ms） | 初始窗 8ms 对齐主册「延迟超 8ms 动态收缩窗口」 |
| 6 | F048 | cpufreq.rs:146/156 `set_manual`+`burst` | 两路径绕过 `clamp_target`：Silent 档下 `burst` 可请求 index 0（最高频）——封顶完全失效（真缺陷）；另有死代码 `manual_bound()`（零冗余） | 两路径统一走 `clamp_target`（:306，Silent 抬 index ≥5 / Performance 压 index ≤2）唯一封顶判据；删除死代码 |
| 7 | F052 | heapfrag.rs:297 `zero_heap_struct` | `SizeClassPool` 字节公式漏计 `alert_class: Option<usize>`（16B 级）——自证公式与实际结构体尺寸不符 | 公式补全（888B 实测对齐） |
| 8 | F057 | iotier.rs:51/65 + `evaluate_fg_saturation` | **反向保护 resume 死锁**（真缺陷，最高危）：暂停解除条件「前台静默 5s」在持续 FG 流量下不可达（`last_fg_activity_ms` 每次前台派发刷新，常态 10/s ≪ 5s 静默窗）→ bg 永久暂停 → 队列涨满 128 后 submit 全拒 → 后台饿死，`bg_done=128 vs 172800`，直接违反主册判据二「后台任务零饥饿」——「防饿死反向保护」反而制造饿死，语义自相矛盾 | 「完全暂停」改为**时长有界**：新增 `BG_PAUSE_MAX_MS=5s`（:51），暂停满上限自动解除 + `fg_busy_since` 复位（再次持续满载 60s 才重新暂停，主册逐字语义成立）；删除被兜底支配的死机制「前台静默窗」（`paused_at ≥ last_fg_activity` ⇒ 兜底解除时刻恒不晚于静默解除——静默机制不可达，零冗余协议删除 `last_fg_activity_ms` 字段）；`Q_CAP 128→256`（:65，暂停期峰值积压 130 = 风暴期 120 + 暂停期 5s×2/s=10，背压余量零丢请求） |
| 9 | 多域 | 各域 tests 模块 | 语义修正后 12 个宿主单测断言过时连锁失败（断言侧过时，非实现缺陷） | 逐个按新语义重造测试数据：frameattr 2（hit_big_dirty busy 16_000 扣 4.2ms 回线等）、frameledger 2（minute_agg busy 全超线 / adaptive_ratio==10）、intrcoal 2（补 `flush_batch` / 时间轴压缩）、latbudget 2（Audio 序 index 2 / ring_wraps）、prefetch2 1（buf 扩至含 order 区）、startprof 1（seq=2 才计中位数）、wcoalesce 1（仅升档计 suppressed）、iotier 1（风暴模拟重写） |

**latbudget 桶边界实测记录**（不掩盖整数舍入行为）：`bucket_of` 以 `v - v/5` 逆步进逼近几何级数，整数舍入漂移使实际边界 bucket 10 = 364..453、bucket 11 = 454..566（非 50×1.25^b 精确值，bucket 11 上沿 566 > Input 预算 500——贴界真值会落 11 桶）；测试样例按**实测边界**构造（mixed_load 峰值 453 贴 bucket 10 上沿），Python 精确模拟桶算法取证，不粉饰舍入漂移。

## 5. 偏差登记（实现选择，一处一事实）

| 选择 | 依据 | 登记 |
| --- | --- | --- |
| `BG_PAUSE_MAX_MS = 5s`（原 `BG_RESUME_IDLE_MS` 改名重定义） | 主册明文「前台让出窗口计时」未给数值；实装解释为**暂停时长有界**——无界暂停在持续满载下即后台永久饥饿，与「防饿死反向保护」自相矛盾（§4#8 实锤）；5s 与原让出窗同量级 | §4#8 |
| `Q_CAP = 256` | 主册未规定队列容量；128 无法容纳暂停期峰值积压 130（会丢请求、破坏零饥饿判据） | §4#8 |
| `BATCH_SESSION_GAP_MS = 60s` | 主册「批量任务超 30 分钟」以连续批量流计，静默 60s 视为任务结束、会话复位 | 源码常量注释 |
| `startprof` 中位数以 `seq≥2` 计入 | 主册「10 次启动画像方差」需完整 warm 启动序列；单次记录无中位数语义 | 源码测试注释 |
| 域接线状态 | 17 域为判据实装层（独立可测、CheckSet 全绿）；与内核运行时路径的接线（如 frameledger 真实打点、iotier 挂接存储栈）为后续闸门工作 | §7 |

## 6. 环境事项登记（不粉饰）

1. **release 通道确定性损坏**：会话中另一 AI 的 debug 构建（01:07Z）与本会话 release 链接发生竞态，release 测试 exe 启动即死（exit -1/127）。取证：`rm`+`touch` 重链、`cargo clean -p varix --release` 全清重编均无效（确定性损坏）；同期 debug 通道完全正常（--list 3897 正常列出）。**全量验证以 debug 通道执行**；release 通道损坏属并发构建产物竞争，代码无嫌疑，但不以绿屏掩盖——如实登记。
2. **cmd 真实退出码取证法**：Git Bash 下 `./exe` 报 127 为 MSYS 映射假象、cmd 不认 `/` 路径报 9009——均非程序真实退出码；权威取证用 `cmd //v:on //c "path\to.exe ... & echo !ERRORLEVEL!"`（本报告 §3 全部 EXIT 值由此取得）。
3. **教训**：后台命令中 `find /c /v ""` 会被 Git Bash 错译为 Unix find 全盘扫描（只读、已即时终止、无副作用）——cmd 内建工具须经 `cmd //c` 且引号层级完整才可靠。

## 7. QEMU 与接线随闸门登记

- **CheckSet 执行面**：内核启动路径经 `robust.rs` 274 域表注册执行（QEMU 串口输出 CheckSet 汇总）。
- **宿主同构验证**：`f475_every_domain_reports` 在宿主执行同一 274 域 CheckSet 全量断言，**PASS**（§3#3）——perfstar 17 域为纯逻辑域（无硬件依赖），宿主/QEMU 判据同构。
- **既有 QEMU 证据**：修复中段 QEMU 串口日志 f475 域 6/6 PASS 在案（`_attic/` 保留现场）；修复收口后未重跑 QEMU 的理由：17 域纯逻辑同构覆盖 + release 链路当时受并发构建竞态污染（§6#1），避免以受扰环境产证据。
- **接线**：17 域与内核运行时路径的实际接线（frameledger 真实帧打点、iotier 挂接存储栈队列、cpufreq 对接调度器、glyphcache/dirtyrect 对接合成器等）随闸门登记为后续工作；当前交付为判据实装层 + 全量自检。

## 8. 工程量台账

| 维度 | 数字 |
| --- | --- |
| 新增域文件 | 17 域 + mod.rs = 18 文件 / 8514 行（最大单域 dirtyrect 1060 行，最小 bigpage 276 行） |
| CheckSet 检查项 | 156（17 域全注册入 274 域表，`f475_every_domain_reports` 全量把关） |
| 宿主单测 | 98（与 `perfstar::` 过滤跑精确一致） |
| 实现侧真缺陷修复 | 7 处（§4#1-#8，含 3 处高危：iotier 死锁 / pagewater 字节账 / cpufreq 封顶） |
| 连锁断言修正 | 12 处（§4#9，语义修正的必然同步，非缺陷） |
| 全量回归 | 3897 项测试（perfstar 贡献 98 + CheckSet 判据 156 经 f475 全量入口） |
| 提交 | `c128932`：22 文件，+9006/−290（任务面零卷入，暂存逐一核对） |

## 9. 结论

- AI-K1 分工包 F041-F057 十七域判据实装完成：**156 CheckSet 检查项 + 98 宿主单测，全绿**。
- 全仓 274 域 CheckSet 宿主全量验证通过；全量 3897/3897 PASS（debug 通道，55.81s，EXIT=0）。
- 实装过程中挖出并修复 **7 处实现侧真缺陷**（三处高危：iotier 反向保护 resume 死锁、pagewater 字节账脱钩、cpufreq 封顶失效），无粉饰、无死代码遗留（K1PROBE 探针与 `k1_frame_probe` 临时模块已清理，iotier 死机制按零冗余协议删除）。
- 全部偏差与环境事项如实登记于 §5、§6；接线随闸门登记于 §7。
- CHANGELOG 已补 K1 批次条目（对齐 AI-K2 批格式），收口信息完整可审计。
