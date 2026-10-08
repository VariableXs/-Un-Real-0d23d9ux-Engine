//! perfstar — Varix STAR I start · B 性能域深化（F041~F057 · AI-K1 分工包）。
//!
//! 本目录是《Varix STAR I start.md》主册 B-3 深化设计报告（G-B-01 ~ G-B-17）
//! 的判据实装层。十七项各占一个子模块，一项一事实：
//!
//! | 项 | 判据锚 | 子模块 |
//! | --- | --- | --- |
//! | F041 帧率账本 | G-B-01 | [`frameledger`] |
//! | F042 帧率归因器 | G-B-02 | [`frameattr`] |
//! | F043 冷启动画像 | G-B-03 | [`startprof`] |
//! | F044 预取指纹 v2 | G-B-04 | [`prefetch2`] |
//! | F045 页缓存水位策略 | G-B-05 | [`pagewater`] |
//! | F046 写合并窗口自适应 | G-B-06 | [`wcoalesce`] |
//! | F047 调度器延迟预算深化 | G-B-07 | [`latbudget`] |
//! | F048 CPU 频率联动 | G-B-08 | [`cpufreq`] |
//! | F049 空转清零工程 | G-B-09 | [`idlezero`] |
//! | F050 中断合并 | G-B-10 | [`intrcoal`] |
//! | F051 大页策略 | G-B-11 | [`bigpage`] |
//! | F052 堆碎片治理 | G-B-12 | [`heapfrag`] |
//! | F053 启动并行度 | G-B-13 | [`bootpar`] |
//! | F054 图像解码 SIMD | G-B-14 | [`imgsimd`] |
//! | F055 字形光栅缓存 | G-B-15 | [`glyphcache`] |
//! | F056 合成器脏区深化 | G-B-16 | [`dirtyrect`] |
//! | F057 IO 调度分级 | G-B-17 | [`iotier`] |
//!
//! 共同纪律（与主册铁律对齐）：
//! - **零堆热路径**：所有内核路径定长结构，无 Vec/String/Box/format!。
//! - **一处一事实**：每条常量在注释里写明主册依据与推导。
//! - **先测量后调参**：账本（F041）是全域共同前提，一切数字与监视器同源。
//! - **判据唯一源**：验收标准第一句摘自主册判据，十二查叠加执行。

/// 十七域共用底盘（深化批次三）：诊断报备环 / 保留期轮转 / 旋钮清单 /
/// 60 秒逐秒环——主册反复点名的四条共同纪律，一处实装十七域共用，
/// 不为每个域复制一份（零冗余）。
pub mod perfkit;
/// 十七域 · 通用十二查登记册（深化批次三）：三落位 + 理由 / ≤4 段导航路径链 /
/// 说明句三件套 / 可调参数清单 / 4K 四档走查 / 性能线数值——对账单一源。
pub mod k1walk;
/// 十七域 · 用户故事场景验收（深化批次三）：把主册【用户故事】里的具体场景与
/// 数字做成可执行验收（拖动顿挫 / 复制卡顿 / VS Code 画像 / 40MB→9MB / 4K 图
/// 洪峰 / 拔 U 盘 / 混载 / 搜索突发 / 挂机下载 / 滚轮洪峰 / 滚动一小时 /
/// 烤机一周 / 7.4 秒开机 / 壁纸即换即见 / 500 页滚动 / 打字三件事 / 下载开目录）。
pub mod k1scene;

// ---------------------------------------------------------------------------
// 深化批次四「机制总成」（mech_*）：批次一~三落的是判据账本/判定/曲线，
// 本批补齐其下方的**算法本体**——调度核心、分配器、编解码器、缓存驱逐、
// 预算闸门。共用件（mech_sim/mech_stats）供十七域；域专属件按消费域挂接。
// 各件判据锚点见 docs/AI-K1-检查项对账表.md 批次四段。
// ---------------------------------------------------------------------------

/// 共用：确定性离散事件仿真底盘（时钟/事件堆/xorshift/回放摘要）。
pub mod mech_sim;
/// 共用：流式统计算术（Welford 方差 / 单调滑窗峰值 / EWMA 迟滞 / isqrt）。
pub mod mech_stats;
/// F054+F041：DEFLATE 编码器 + LZ77 哈希链（账本落盘压缩端）。
pub mod mech_deflate;
/// F054+F041：DEFLATE 解码器 + zlib 封装 + PNG 容器装配（全三型块 + Adam7）。
pub mod mech_inflate;
/// F057：EDF 截止期堆 + WFQ 虚拟时间公平 + 批量老化提升（调度本体）。
pub mod mech_edfq;
/// F051+F052：伙伴分配器（split/merge + 双重释放检出 + 碎片谱压力）。
pub mod mech_buddy;
/// F044+F045：CLOCK 第二次机会缓存 + LRU-K 驱逐（顺序洪峰不污染热点）。
pub mod mech_clocklru;
/// F050：分层时间轮（4 级 × 64 槽，同键覆盖 merged_away 容器级语义）。
pub mod mech_wheel;
/// F047+F048：HTB 分层令牌桶（类保证+父借用）+ PID 调频控制环（漏积分）。
pub mod mech_tokens;
/// F053：DAG 拓扑排序 + 环检测 + 关键路径 + m-worker 并行 makespan。
pub mod mech_dag;
/// F046：三级基数树脏页追踪（定长节点池 + 连续段弹出 = 合并写下盘接口）。
pub mod mech_radix;
/// F045+F049：水位均衡回收循环（kswapd 同族）+ 空闲清零细流节流。
pub mod mech_scan;

// ---------------------------------------------------------------------------
// 深化批次五「机制总成·续」（mech_* 九件）：批次四补了账本下方的算法本体，
// 本批补主册【设计细节】点名但批次四未覆盖的九个机制面——成员判定、调度
// 公平、合批、调频、OOM、预读、slab、温控、内存压缩。全部零堆、零浮点、
// 自带 run_checks（CheckSet 判例入对账）。
// ---------------------------------------------------------------------------

/// F046：布隆过滤器 + 计数布隆（合并窗成员判定，零假阴性硬面）。
pub mod mech_bloom;
/// F047：CFS 公平调度器（nice 权重表 + vruntime 红黑语义 + 最小 vruntime 逃逸）。
pub mod mech_cfs;
/// F050+F057：事件合批器（同键覆盖 + 窗口到期成批 + 预算内批量语义）。
pub mod mech_coalesce;
/// F048：ondemand 与 schedutil 双调频策略器（迟滞升档 + util 直映 + 定点钳制）。
pub mod mech_gov;
/// F045+F052：OOM 选择器（badness 记分 + 保护豁免 + 逐出顺序与回收闭环）。
pub mod mech_oom;
/// F044：顺序预读窗口状态机（同步缺页起步 ×2 翻倍 + 随机访问降窗关闭）。
pub mod mech_readahead;
/// F052：SLAB 分配器（六档尺寸类 + 着色偏移 + 双重释放检出 + in-use 账）。
pub mod mech_slab;
/// F048+F197：热节流状态机（四档频率封顶 + 迟滞回降 + 临界段直停）。
pub mod mech_thermal;
/// F045+F058：zram 内存压缩池（LZ4 块编解码 + 三档尺寸类存储 + 直存回退）。
pub mod mech_zram;

pub mod bigpage;
/// F051 深化件（深化批次三）：三区预算表与 4K 双口径登记 / 冻结降级顺序的预留
/// 计划（帧缓冲先降·内核最后）/ 分区降级标注 / 只增不减防碎片守卫 /
/// TLB miss 对拍账 / 核显侧可行性评估记录 / 占用与命中率只读投影。
pub mod bigpage_ext;
pub mod bootpar;
/// F053 深化件（深化批次三）：依赖矩阵（真依赖 vs 惯性串行）/ 逐链超时策略
/// （USB 宽限·存储严格）/ 失败传播与跳过标注 / 锁等待归因账 / 动画 80% 起播点
/// / 甘特图并行段时长与串行基准 / 8 秒预算逐段偏差判定。
pub mod bootpar_ext;
pub mod cpufreq;
/// F048 深化件（深化批次三）：ACPI _PSS 表装载与降序校验 / 不可读固定中档并
/// 诊断标注 / 切换守卫（重试一次后锁安全档）/ 温度越权（F197 优先于本策略）/
/// 30s 负载类型判定与构建工具名单 / 切换耗时·突发响应·续航对照三账 / 旋钮清单。
pub mod cpufreq_ext;
pub mod dirtyrect;
/// F056 深化件（深化批次三）：区间树相交合并（定长零堆）/ 16px 阴影环带预
/// 渲染面 / 动画矩形声明登记 / 光标层硬件·软双路径与零内容重绘判据 / 脏区
/// 爆炸限频 30fps 并通知 F042 / 层间脏区求交裁剪 / 每窗口脏矩形 8 上限与溢出
/// 合并整窗 / 弹窗合成矩形 = 矩形 + 阴影环带判据。
pub mod dirtyrect_ext;
pub mod frameattr;
pub mod frameledger;
/// F041 深化件（深化批次三）：帧明细查询面 / 账本落盘格式 / 降采样治理器 /
/// 分段打点成对自检 / 60 秒窗时间戳裁剪。
pub mod frameledger_ext;
/// F042 深化件（深化批次三）：证据引用链 / 掉帧历史列表行 / 7 天案例簿 /
/// 归因器健康状态机（静默停用+诊断报备）/ 误报率账。
pub mod frameattr_ext;
pub mod glyphcache;
/// F055 深化件（深化批次三）：常用字集预热（3500 汉字 + ASCII）/ 当前帧钉住与
/// 双缓冲原子换页 / 资产版本（AA·hinting·字体哈希）全重建决策与显式原因 /
/// 命中率扩页状态机（迟滞 + 32MB 硬顶）/ 光栅超 2ms 转后台与占位代价账 /
/// 16MB 配额账与不做持久化登记。
pub mod glyphcache_ext;
pub mod heapfrag;
/// F052 深化件（深化批次三）：位图分配器（O(1) 分配释放 + 重复释放检出）/
/// 六档与 512B 直通分界表 + 内部碎片 / 相邻档切分（带开销标注）/ 分配失败
/// 四阶段路径演练（B-2903）/ 碎片率告警迟滞状态机 / 六档分配延迟 P99 账。
pub mod heapfrag_ext;
pub mod idlezero;
/// F049 深化件（深化批次三）：事件源统一 fence（常 armed 源不可关）/ 光标
/// 闪烁 armed 三条件与原因文案 / 深睡 MONITOR-MWAIT 支持性记档与软路径标注 /
/// 常驻动画登记（唤醒豁免）/ 60 秒静置双指标验收窗（两数字同录在案）。
pub mod idlezero_ext;
pub mod imgsimd;
/// F054 深化件（深化批次三）：指令集档探测与回退 / PNG 行过滤五型（含 Paeth）
/// 标量与通道化双路径逐位对拍 / JPEG 8×8 IDCT 分离式与朴素实现对拍 /
/// 解码错误三要素与半图拦截 / 超大图流式分块计划 / vxbench 三样本与双线耗时账。
pub mod imgsimd_ext;
pub mod intrcoal;
/// F050 深化件（深化批次三）：合并键（设备+类型）与可覆盖性二分 / 2000ns 硬
/// 预算闸门 / 网络侧按 flow 合并 / flood 70 级零丢弃账 / 中断风暴熔断状态机 /
/// 合并统计（每秒合并率·最大批）入账本。
pub mod intrcoal_ext;
pub mod iotier;
/// F057 深化件（深化批次三）：分级双因子判定（进程标签 + 请求特征）/ 截止期
/// 计算（批量无截止期 ≠ 0）/ 三队列深度投影与吞吐曲线 / 降级通知节流 /
/// 降级审计环（谁被降级·为什么·入诊断快照）/「下载中打开目录」场景对拍账。
pub mod iotier_ext;
pub mod latbudget;
/// F047 深化件（深化批次三）：预算表自洽与 ADR 登记 / 优先级映射表 / 老化
/// 自修正状态机（冷却·上限·回落）/ 零样本观察窗说明 / energy 大核联动 /
/// 采样环 160KB 预算自证 / 超标点线程快照下钻面。
pub mod latbudget_ext;
pub mod pagewater;
/// F045 深化件（深化批次三）：文件页/匿名页分列 LRU / 分区脏页账（只读卷
/// 不计）/ 500ms 判定节流门 / 三档条件表与旋钮清单 / 回收后悔账 / 200MB
/// 警戒状态机 / 配额让路 / 只读展示面 / 决策日志入诊断快照。
pub mod pagewater_ext;
pub mod prefetch2;
/// F044 深化件（深化批次三）：开放格式指纹头（魔数/版本/CRC）/ 有效性三态 /
/// 8MB 每应用配额 + LRU 指纹库 / 八页成批预读 / 退出后 30 秒生成调度 /
/// 命中率账。
pub mod prefetch2_ext;
pub mod startprof;
/// F043 深化件（深化批次三）：五段条形图段宽配平 / 首次 vs 二次对比 /
/// (版本,序号) 复合键 + 20 次保留 / 中止画像标注 / 隐私总闸 / 方差账。
pub mod startprof_ext;
pub mod wcoalesce;
/// F046 深化件（深化批次三）：三档条件表与旋钮清单 / F045 共享数据源接收面 /
/// 切档审计（驻留迟滞 + 入诊断快照）/ 断电丢失窗口估算 / 三档断电百次演练账 /
/// 分档 fsync P99 账 / 交互诊断文案面。
pub mod wcoalesce_ext;

/// 全域自检登记名（robust.rs KernelCheckup 用，每项一个独立域集）。
pub const DOMAIN_NAMES: [&str; 17] = [
    "F041-frameledger",
    "F042-frameattr",
    "F043-startprof",
    "F044-prefetch2",
    "F045-pagewater",
    "F046-wcoalesce",
    "F047-latbudget",
    "F048-cpufreq",
    "F049-idlezero",
    "F050-intrcoal",
    "F051-bigpage",
    "F052-heapfrag",
    "F053-bootpar",
    "F054-imgsimd",
    "F055-glyphcache",
    "F056-dirtyrect",
    "F057-iotier",
];
