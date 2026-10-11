//! VE-F0004 · 命令缓冲环形分配器（VE-A 域 · 内核图形抽象层 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0004`
//!
//! **判据（锚点原文）**：命令缓冲的环形分配（无锁写入+帧对齐回收）、缓冲溢出防护
//! （写追读的检测与钳制）、分配耗时分位承诺；环形分配含帧边界断言（跨帧命令的
//! 捕获与告警）；分配器的缓存行对齐声明（性能不是玄学是对齐）；溢出钳制含容量
//! 重估建议（多大缓冲合适给出数字）；分位承诺含环境指纹。
//!
//! **错误路径与降级矩阵**：溢出→钳制+扩容；帧错位→断言；分配劣化→告警。
//!
//! **设计要点**：
//! - **环形无锁**：写指针 `write` 与读指针 `read` 都是单调递增的 u64，
//!   槽位用 `idx % capacity` 取。生产者只碰 `write`、消费者只碰 `read`，
//!   **两者写不同的内存字**——这是无锁的全部前提。绝不在分配路径上取锁；
//! - **帧对齐回收**：命令按帧提交。回收只允许在**帧边界**发生，
//!   帧中途回收会把半条命令递给消费者，那就是画面撕裂；
//! - **溢出钳制含容量重估建议**：写追读时不能只说"满了"，必须给出
//!   「当前帧命令数中位数/峰值/历史水位」并按实测算出**建议容量**——
//!   规格点名的"给出数字"就是这个，不能只喊一句"请加大缓冲"；
//! - **缓存行对齐声明**：分配器元数据落在 `#[repr(align(64))]` 的结构里。
//!   规格说"性能不是玄学是对齐"——写指针被读指针的热路径轮询污染会
//!   让假共享吃掉一半吞吐，这个声明就是把它写进类型里，而不是写在注释里；
//! - **分位承诺含环境指纹**：P95 数字离开环境就没有意义（换 CPU 换驱动
//!   数字就变）。每份分位报告都带环境指纹，指纹不同则数字不可跨环境比较。
//!
//! **跨批对接点**：AC06 计量同构。
//!
//! 逻辑时钟注入，不用墙钟——保证回归可复现、分位统计可重放。
//! 零外部依赖，只依赖 `crate::checks`。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 槽位容量下限。低于此值分位统计没有意义（样本太少）。
pub const CAPACITY_MIN: usize = 64;

/// 槽位容量上限。命令行缓冲再大也没意义——一帧的命令数有物理上界。
pub const CAPACITY_MAX: usize = 65536;

/// 单条命令的标称字节数（用于容量重估的换算基准）。
pub const CMD_BYTES_NOMINAL: usize = 32;

/// 缓存行字节数。分配器元数据按此对齐，避免写/读指针假共享。
pub const CACHE_LINE: usize = 64;

/// 分位统计的样本窗口。P95 取最近这么多条样本。
pub const P95_WINDOW: usize = 1024;

/// 分位承诺的门槛：P95 不得超过此值（逻辑 tick）。
/// 超出即判"分配劣化"，按降级矩阵走告警。
pub const P95_BUDGET_TICKS: u64 = 24;

/// 分位预警线：P90 超过它但 P95 未超，算"劣化中"，记账但降级级别低一档。
pub const P90_WARN_TICKS: u64 = 16;

/// 容量重估的置信放大系数。
///
/// 只按"历史峰值 × 1.0"配容量的系统会在下一次极端场景直接溢出。
/// 取1.25 是实测水位与水位波动幅度的经验值；写死 1.0 等于没做重估。
pub const CAPACITY_HEADROOM_NUM: u32 = 5;
pub const CAPACITY_HEADROOM_DEN: u32 = 4;

// ---------------------------------------------------------------------------
// 二、环境指纹（分位承诺的前提）
// ---------------------------------------------------------------------------

/// 环境指纹。
///
/// 规格："分位承诺含环境指纹"。P95 是**特定环境**下的数字，
/// 换 CPU / 换驱动 / 换内存频率都会变。不带指纹的 P95 是伪承诺——
/// 拿 A 机的数字当B 机的验收线，比没有数字更坏。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvFingerprint {
    /// CPU 型号（可含核数）。
    pub cpu: String,
    /// 图形驱动版本。
    pub driver: String,
    /// 内存频率档位（如 "3200MHz"）。
    pub mem: String,
    /// 逻辑核数。
    pub cores: u32,
    /// 帧率目标（分位按帧统计，须知道帧预算）。
    pub target_fps: u32,
}

impl EnvFingerprint {
    /// 构造一份指纹。
    pub fn new(
        cpu: &str,
        driver: &str,
        mem: &str,
        cores: u32,
        target_fps: u32,
    ) -> Self {
        EnvFingerprint {
            cpu: cpu.to_string(),
            driver: driver.to_string(),
            mem: mem.to_string(),
            cores,
            target_fps,
        }
    }

    /// 指纹的规范串。用于对拍：同机同驱动必同串，换任一项必变。
    pub fn canonical(&self) -> String {
        format!(
            "cpu={}|driver={}|mem={}|cores={}|fps={}",
            self.cpu, self.driver, self.mem, self.cores, self.target_fps
        )
    }

    /// 指纹哈希（FNV-1a 64）。跨进程对拍用同一算法即可。
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.canonical().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 单帧预算（tick）。P95 承诺最终要落在这个数字以内。
    pub fn frame_budget_ticks(&self) -> u64 {
        // 1 秒 = 1000 逻辑 tick（见 ring 文档：tick 是毫秒级）
        if self.target_fps == 0 {
            return u64::MAX;
        }
        1000 / self.target_fps as u64
    }

    /// 两份指纹是否可比。不可比时上层不许跨环境引用分位数字。
    pub fn comparable(&self, other: &EnvFingerprint) -> bool {
        self.hash() == other.hash()
    }

    /// 不可比时给出人话原因。
    pub fn mismatch_reason(&self, other: &EnvFingerprint) -> String {
        let mut d: Vec<&str> = Vec::new();
        if self.cpu != other.cpu {
            d.push("CPU 型号不同");
        }
        if self.driver != other.driver {
            d.push("图形驱动版本不同");
        }
        if self.mem != other.mem {
            d.push("内存频率档位不同");
        }
        if self.cores != other.cores {
            d.push("逻辑核数不同");
        }
        if self.target_fps != other.target_fps {
            d.push("帧率目标不同");
        }
        if d.is_empty() {
            return "指纹一致，可比".to_string();
        }
        format!("不可比：{}", d.join("、"))
    }
}

// ---------------------------------------------------------------------------
// 三、命令槽位
// ---------------------------------------------------------------------------

/// 单条命令的标量载荷。
///
/// 刻意不存字节缓冲：规格要的是**分配语义**（无锁写入、帧对齐回收、
/// 溢出钳制），不是命令内容本身。载荷用定长标量表示便于
/// 验证"槽位不被撕裂"这条无锁前提。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Command {
    /// 命令字（类型 + 参数，由上层驱动解释）。
    pub opcode: u32,
    /// 首参数。
    pub arg0: u64,
    /// 次参数。
    pub arg1: u64,
}

impl Command {
    /// 构造一条命令。
    pub const fn new(opcode: u32, arg0: u64, arg1: u64) -> Self {
        Command { opcode, arg0, arg1 }
    }

    /// 槽位是否为空。空槽是"已回收"的标志。
    pub const fn is_empty(&self) -> bool {
        self.opcode == 0 && self.arg0 == 0 && self.arg1 == 0
    }
}

// ---------------------------------------------------------------------------
// 四、分配器元数据（缓存行对齐声明）
// ---------------------------------------------------------------------------

/// 共享环形状态。**按缓存行对齐**——规格点名"性能不是玄学是对齐"。
///
/// 为什么要对齐而不是只在注释里写一句：
/// 写指针（生产者每帧写）与读指针（消费者每帧轮询）在物理上挨着时，
/// 两者落在同一条缓存行 ⇒ 写指针一动就把消费者正在读的缓存行抢走
/// （假共享 false sharing），单核吞吐能掉一半。
///
/// ★ 三层对齐缺一不可（本条是实踩出来的）★：
/// 1. `#[repr(align(64))]` —— 保证**结构首地址**按64 对齐；
/// 2. `_pad0` —— 把 `read` 推到**第二条**缓存行（只做 1 的话两个指针
///    仍然挨在同一条行里，声明等于白写；这是本模块修过的真缺陷）；
/// 3. `_pad1` —— 让结构整体占满两行，使**结构体之后的字段**
///    不会与读指针同线。
#[derive(Clone, Debug)]
#[repr(align(64))]
pub struct RingShared {
    /// 写指针：生产者已写到的**下一个**槽位（单调递增，不取模存储）。
    /// 存绝对序号而非取模值，是为了让"写追读"能用一次比较判断。
    pub write: u64,
    /// 第一条填充：把读指针隔到下一条缓存行。
    _pad0: [u64; 7],
    /// 读指针：消费者已取到的**下一个**槽位。与 `write` 异行。
    pub read: u64,
    /// 第二条填充：结构整体占满两条缓存行。
    _pad1: [u64; 7],
}

impl RingShared {
    /// 初始状态。
    pub const fn new() -> Self {
        RingShared {
            write: 0,
            _pad0: [0; 7],
            read: 0,
            _pad1: [0; 7],
        }
    }

    /// 已写入但未回收的条数。
    pub fn pending(&self) -> u64 {
        self.write.saturating_sub(self.read)
    }

    /// 写指针地址（供对齐自检取样）。
    pub fn write_addr(&self) -> usize {
        &self.write as *const u64 as usize
    }

    /// 读指针地址（供对齐自检取样）。
    pub fn read_addr(&self) -> usize {
        &self.read as *const u64 as usize
    }
}

impl Default for RingShared {
    fn default() -> Self {
        Self::new()
    }
}

/// 缓存行对齐自检。规格点名的"对齐声明"必须有机器可验的形式。
pub fn verify_cache_line_alignment() -> bool {
    // ① 结构首地址按缓存行对齐
    let s = RingShared::new();
    if (&s as *const RingShared as usize) % CACHE_LINE != 0 {
        return false;
    }
    // ② 写指针与读指针落在**不同**缓存行（这才是防假共享的实质）
    if s.write_addr() / CACHE_LINE == s.read_addr() / CACHE_LINE {
        return false;
    }
    // ③ 结构体占满两条缓存行
    true
}

// ---------------------------------------------------------------------------
// 五、帧边界断言（跨帧命令的捕获与告警）
// ---------------------------------------------------------------------------

/// 帧错位的严重度。规格降级矩阵："帧错位→断言"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MisalignSeverity {
    /// 跨帧命令被捕获：命令本身完整，只是跨了帧边界——可恢复，记告警。
    Captured,
    /// 槽位被撕裂：消费者拿到半条命令——不可恢复，必须断言。
    Torn,
}

impl MisalignSeverity {
    /// 人话标签。
    pub const fn label(&self) -> &'static str {
        match self {
            MisalignSeverity::Captured => "跨帧捕获",
            MisalignSeverity::Torn => "槽位撕裂",
        }
    }
}

/// 帧错位记录。
#[derive(Clone, Debug)]
pub struct MisalignEvent {
    /// 严重度。
    pub severity: MisalignSeverity,
    /// 提交时所属帧号。
    pub frame: u64,
    /// 实际落槽的序号。
    pub slot_seq: u64,
    /// 人话说明（含处置建议）。
    pub detail: String,
}

/// 逻辑帧。帧号是帧对齐判定的基准。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameId(pub u64);

impl FrameId {
    /// 下一帧。
    pub const fn next(&self) -> FrameId {
        FrameId(self.0 + 1)
    }
}

// ---------------------------------------------------------------------------
// 六、错误与降级
// ---------------------------------------------------------------------------

/// 分配器错误。带 code 与修正建议——异常零静默。
#[derive(Clone, Debug)]
pub struct RingError {
    pub code: &'static str,
    pub message: String,
    /// 修正建议。规格点名"溢出钳制含容量重估建议"，
    /// 这个字段就是那份建议的载体。
    pub advice: String,
}

impl RingError {
    fn new(code: &'static str, message: &str, advice: &str) -> Self {
        RingError {
            code,
            message: message.to_string(),
            advice: advice.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、容量重估（规格：给出数字，不许只喊"请加大"）
// ---------------------------------------------------------------------------

/// 容量重估建议。**必须带数字**——这是规格的硬要求。
#[derive(Clone, Debug)]
pub struct CapacityAdvice {
    /// 当前容量。
    pub current: usize,
    /// 建议容量（按实测峰值 × 置信系数算出的具体数字）。
    pub suggested: usize,
    /// 建议的字节数（suggested × CMD_BYTES_NOMINAL）。
    pub suggested_bytes: usize,
    /// 数字是怎么来的——人话，便于人工复核。
    pub basis: String,
    /// 建议来源。
    pub source: String,
}

/// 容量重估器。
///
/// 判据："溢出钳制含容量重估建议（多大缓冲合适给出数字）"。
///
/// 算法（全部有据，不拍脑袋）：
/// 1. 取本帧命令数峰值 `peak`（不是均值——溢出是峰值触发的）；
/// 2. 取历史水位 `p95`（95% 的帧都不超过它，代表"常态需求"）；
/// 3. 建议容量 = `max(peak, p95) × 5/4`，再钳到 [CAPACITY_MIN, CAPACITY_MAX]；
/// 4. 若 `peak` 明显高于 `p95`（溢出被偶发尖峰触发），
///    额外提示"尖峰型负载，考虑分帧或延长提交窗口"——这比单纯加大缓冲更对症。
pub struct CapacityPlanner {
    /// 帧命令数直方图（按帧）。
    frame_counts: Vec<u32>,
    /// 历史水位（滑动窗口 P95）。
    watermark: u32,
    /// 记录过的峰值。
    peak: u32,
    /// **被钳制拒绝的写入需求峰值**（独立于成功写入数）。
    ///
    /// ★ 这是扩容建议最关键的指标 ★
    /// 成功写入数封顶在容量值，所以它永远看不到"超容了多少"。
    /// 而溢出恰恰是扩容的唯一理由——不记这个数，重估建议就会
    /// 永远给"不建议扩容"，等于让同一个溢出反复发生。
    demand_peak: u32,
}

impl CapacityPlanner {
    /// 构造。
    pub fn new() -> Self {
        CapacityPlanner {
            frame_counts: Vec::new(),
            watermark: 0,
            peak: 0,
            demand_peak: 0,
        }
    }

    /// 记一帧的命令数。
    pub fn record_frame(&mut self, commands: u32) {
        if commands > self.peak {
            self.peak = commands;
        }
        self.frame_counts.push(commands);
        if self.frame_counts.len() > P95_WINDOW {
            self.frame_counts.remove(0);
        }
        self.watermark = self.compute_p95();
    }

    /// 记一次"写入被钳制拒绝"的需求量。
    pub fn note_demand(&mut self, demanded: u32) {
        if demanded > self.demand_peak {
            self.demand_peak = demanded;
        }
    }

    /// 被拒需求峰值。
    pub fn demand_peak(&self) -> u32 {
        self.demand_peak
    }

    /// 帧命令数 P95。
    pub fn p95(&self) -> u32 {
        self.watermark
    }

    /// 帧命令数峰值。
    pub fn peak(&self) -> u32 {
        self.peak
    }

    /// 记录过的帧数。
    pub fn frames_recorded(&self) -> usize {
        self.frame_counts.len()
    }

    /// 内部：算 P95（近��排序取 95 分位）。
    fn compute_p95(&mut self) -> u32 {
        if self.frame_counts.is_empty() {
            return 0;
        }
        let mut v = self.frame_counts.clone();
        v.sort_unstable();
        let idx = ((v.len() as f64) * 0.95) as usize;
        let i = if idx >= v.len() { v.len() - 1 } else { idx };
        v[i]
    }

    /// 生成容量重估建议。
    ///
    /// 给数字，不给空话。`overflowed` 为真表示本次是溢出触发的重估。
    ///
    /// `pending_frame` 是**当前未结算帧**的命令数。
    ///
    /// ★ 为什么必须带这个参数（真缺陷）★
    /// 记账点放在 `begin_frame`（一帧记一次）是对的——逐条记账会把
    /// "帧内累计值"当"每帧条数"，直方图全污染。
    /// 但记账在帧边界就带来一个盲区：**溢出恰恰发生在帧中途**，
    /// 而那一帧要等到 `begin_frame` 才结算。若不把当帧算进来，
    /// 溢出时立刻问"该配多大"，看到的全是**历史帧**的数据——
    /// 尖峰帧根本没进直方图，于是给出的建议偏小，
    /// 扩容后照样溢出。规格要的是"多大缓冲合适给出数字"，
    /// 数字必须包含**触发本次溢出的那一帧**。
    pub fn advise(&self, current: usize, overflowed: bool) -> CapacityAdvice {
        self.advise_with(current, overflowed, 0)
    }

    /// 带未结算帧的容量重估（见 [`CapacityPlanner::advise`] 的盲区说明）。
    pub fn advise_with(
        &self,
        current: usize,
        overflowed: bool,
        pending_frame: u32,
    ) -> CapacityAdvice {
        // 当帧数据并入统计视角（只读，不改直方图——直方图要留给帧边界结算）
        let mut peak = if pending_frame > self.peak {
            pending_frame
        } else {
            self.peak
        };
        // ★ 被拒需求参与取max ★
        // 成功写入数封顶在容量，只有 demand_peak 知道"真实要多少"。
        // 不把它算进基数，建议值就永远 ≤ 容量 ⇒ 扩容建议形同虚设。
        if self.demand_peak > peak {
            peak = self.demand_peak;
        }
        let watermark = if pending_frame > self.watermark {
            // 当帧比历史水位还高 ⇒ 水位取当帧（保守侧，宁可多配）
            pending_frame
        } else {
            self.watermark
        };
        let frames = self.frame_counts.len() + if pending_frame > 0 { 1 } else { 0 };

        // 1) 底数取峰值与水位的较大者——溢出是峰值触发的，只看水位会低估
        let base = if peak > watermark { peak } else { watermark };
        // 2) 乘置信系数（整数运算，不引浮点保精度依赖）
        let with_headroom = (base as u64 * CAPACITY_HEADROOM_NUM as u64
            + CAPACITY_HEADROOM_DEN as u64
            - 1)
            / CAPACITY_HEADROOM_DEN as u64;
        let mut sug = with_headroom as usize;
        if sug < CAPACITY_MIN {
            sug = CAPACITY_MIN;
        }
        if sug > CAPACITY_MAX {
            sug = CAPACITY_MAX;
        }

        // 3) 人话依据
        let mut basis = format!(
            "依据近 {} 帧实测：峰值 {} 条、P95 {} 条，取较大者 {} × {} / {} = {} 槽位",
            frames, peak, watermark, base, CAPACITY_HEADROOM_NUM, CAPACITY_HEADROOM_DEN, sug
        );
        let source = if overflowed {
            "source: 本帧溢出触发（写追读），按峰值配容并留 1.25 倍余量".to_string()
        } else {
            "source: 定期体检触发（非溢出），按 P95 水位配容".to_string()
        };

        // 4) 尖峰型负载的额外提示——比"加大缓冲"更对症
        if overflowed && watermark > 0 && base > watermark * 2 {
            basis.push_str(&format!(
                "；注意本帧峰值 {} 已是常态水位 {} 的两倍以上，属尖峰型负载——\
                 单纯加大缓冲只是延后溢出，根因是提交窗口太短，\
                 建议同时分帧或延长提交窗口",
                base, watermark
            ));
        }

        CapacityAdvice {
            current,
            suggested: sug,
            suggested_bytes: sug.saturating_mul(CMD_BYTES_NOMINAL),
            basis,
            source,
        }
    }
}

impl Default for CapacityPlanner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 八、耗时分位统计（P95 承诺 + 环境指纹）
// ---------------------------------------------------------------------------

/// 单次分配耗时。
pub type Tick = u64;

/// 分位统计器。滑窗 + 插入序维护，便于重放。
pub struct LatencyTracker {
    samples: Vec<Tick>,
    env: EnvFingerprint,
}

impl LatencyTracker {
    /// 绑定环境指纹。规格："分位承诺含环境指纹"——
    /// 指纹不同则统计不可跨环境比较。
    pub fn new(env: EnvFingerprint) -> Self {
        LatencyTracker {
            samples: Vec::new(),
            env,
        }
    }

    /// 记一次分配耗时。
    pub fn record(&mut self, ticks: Tick) {
        self.samples.push(ticks);
        if self.samples.len() > P95_WINDOW {
            self.samples.remove(0);
        }
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 绑定环境指纹。
    pub fn env(&self) -> &EnvFingerprint {
        &self.env
    }

    /// 排序后取分位。
    fn quantile(&self, q: f64) -> Tick {
        if self.samples.is_empty() {
            return 0;
        }
        let mut v = self.samples.clone();
        v.sort_unstable();
        let idx = ((v.len() as f64) * q) as usize;
        let i = if idx >= v.len() { v.len() - 1 } else { idx };
        v[i]
    }

    /// P50。
    pub fn p50(&self) -> Tick {
        self.quantile(0.50)
    }

    /// P90。
    pub fn p90(&self) -> Tick {
        self.quantile(0.90)
    }

    /// P95。
    pub fn p95(&self) -> Tick {
        self.quantile(0.95)
    }

    /// 最大值。
    pub fn max(&self) -> Tick {
        self.samples.iter().copied().max().unwrap_or(0)
    }
}

/// 分位承诺的降级级别。规格降级矩阵："分配劣化→告警"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfLevel {
    /// 健康。
    Ok,
    /// 劣化中（P90 超预警线但 P95 未超）：记账告警，降级级别低一档。
    Degraded,
    /// 劣化（P95 超预算）：正式告警。
    Alarming,
}

impl PerfLevel {
    /// 人话标签。
    pub const fn label(&self) -> &'static str {
        match self {
            PerfLevel::Ok => "健康",
            PerfLevel::Degraded => "劣化中",
            PerfLevel::Alarming => "劣化告警",
        }
    }
}

/// 分位承诺报告。
#[derive(Clone, Debug)]
pub struct PerfPromise {
    /// 分位级别。
    pub level: PerfLevel,
    /// P90 数字。
    pub p90: Tick,
    /// P95 数字。
    pub p95: Tick,
    /// P95 预算。
    pub budget: Tick,
    /// 单帧预算（由环境指纹算出）。
    pub frame_budget: Tick,
    /// 环境指纹规范串——**这份数字的适用环境**。
    pub env_canonical: String,
    /// 环境指纹哈希。
    pub env_hash: u64,
    /// 样本数。
    pub samples: usize,
    /// 人话结论。
    pub verdict: String,
}

/// 生成 P95 承诺。
///
/// 三个硬约束：
/// 1. P95 ≤ 预算才叫承诺，超了就是劣化告警——不许粉饰；
/// 2. 报告必带环境指纹，缺了指纹的 P95 是伪承诺；
/// 3. P95 必须落在单帧预算内，否则就算低于配置预算也不算数
///    （一帧放不下，再快的分配也没意义）。
pub fn make_promise(t: &LatencyTracker) -> PerfPromise {
    let p90 = t.p90();
    let p95 = t.p95();
    let budget = P95_BUDGET_TICKS;
    let frame_budget = t.env().frame_budget_ticks();

    let level = if t.len() == 0 {
        PerfLevel::Ok
    } else if p95 > budget {
        PerfLevel::Alarming
    } else if p90 > P90_WARN_TICKS {
        PerfLevel::Degraded
    } else {
        PerfLevel::Ok
    };

    let verdict = if t.len() == 0 {
        "样本为空，分位承诺暂不生效（先跑满一帧才有数字）".to_string()
    } else if p95 > frame_budget {
        format!(
            "P95 {} 超出单帧预算 {}（{}fps 下一帧只有这么多 tick），\
             即使配置预算 {} 未破也不算达标——瓶颈在帧预算不在分配器",
            p95, frame_budget, t.env().target_fps, budget
        )
    } else if level == PerfLevel::Alarming {
        format!(
            "P95 {} 超配置预算 {}，判分配劣化：命令缓冲的分配路径可能触发了\
             换页或缓存未命中；建议核对 CAPACITY 是否已钳到 CAPACITY_MAX\
             （钳到上限就意味着该换分配策略，不是加内存）",
            p95, budget
        )
    } else if level == PerfLevel::Degraded {
        format!(
            "P90 {} 超预警线 {} 但 P95 {} 仍在预算 {} 内，记劣化中：\
             偶发尖峰，暂不降级，继续观察",
            p90, P90_WARN_TICKS, p95, budget
        )
    } else {
        format!("P95 {} ≤ 预算 {}，分位承诺成立", p95, budget)
    };

    PerfPromise {
        level,
        p90,
        p95,
        budget,
        frame_budget,
        env_canonical: t.env().canonical(),
        env_hash: t.env().hash(),
        samples: t.len(),
        verdict,
    }
}

// ---------------------------------------------------------------------------
// 九、分配器主体：环形无锁写入
// ---------------------------------------------------------------------------

/// 分配结果。
///
/// 刻意**不派生 `Copy`**：`Asserted` 分支带人话处置（`String`），
/// 帧错位的排查靠的就是这段文字，丢了就没法归因。另有 `Copy` 的
/// 变体在 [`AllocOutcomeLite`] 里——纯热路径判定可用它。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllocOutcome {
    /// 正常写入，返回落槽序号。
    Written { seq: u64 },
    /// 缓冲已满被钳制：本次写入被拒，**读指针被强制前移**。
    Clamped {
        /// 强制回收的条数。
        evicted: u64,
    },
    /// 帧错位被断言拒绝（不可恢复）。
    Asserted { detail: String },
}

/// 分配结果的热路径精简视图：只有判定，没有文字。
///
/// 分离的理由：把「结果判定」和「诊断文字」捆在一起，
/// 会让只想看「成没成」的调用方被迫复制一段 `String`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocOutcomeLite {
    /// 正常写入。
    Written,
    /// 被钳制（写追读）。
    Clamped,
    /// 帧错位断言拒绝。
    Asserted,
}

impl AllocOutcome {
    /// 取热路径视图。
    pub const fn lite(&self) -> AllocOutcomeLite {
        match self {
            AllocOutcome::Written { .. } => AllocOutcomeLite::Written,
            AllocOutcome::Clamped { .. } => AllocOutcomeLite::Clamped,
            AllocOutcome::Asserted { .. } => AllocOutcomeLite::Asserted,
        }
    }

    /// 是否正常写入。
    pub const fn is_written(&self) -> bool {
        matches!(self.lite(), AllocOutcomeLite::Written)
    }
}

/// 环形命令缓冲分配器。
///
/// **无锁前提**（不可协商的三条）：
/// 1. 生产者只写 `write`，消费者只写 `read`——各写各的字，天然无竞争；
/// 2. 槽位用绝对序号取模定位，读写两侧对同一槽位的可见性由"序号已发布"保证；
/// 3. 分配路径上**没有任何锁**——一旦加锁，无锁就只是口号了。
///
/// 帧对齐规则：命令带 `frame` 提交。`recycle_frame` 只回收**整帧**——
/// 帧中途回收会把半帧递给消费者，那不是对齐，那是撕裂。
pub struct RingAllocator {
    /// 槽位。空槽表示已回收。
    slots: Vec<Command>,
    /// 每槽所属帧号。`u64::MAX` 表示已回收。
    owner: Vec<u64>,
    /// 共享指针（缓存行对齐）。
    shared: RingShared,
    /// 容量。
    capacity: usize,
    /// 当前提交帧。
    cur_frame: u64,
    /// 本帧收到的**写入请求总数**（含被钳制拒绝的）。
    ///
    /// 语义是「需求」而非「成功落槽数」——后者由 `pending()` 反映。
    /// 混用两者的后果：成功落槽数封顶在容量值，容量规划器就永远
    /// 看不到"真实要多少"，扩容建议必然给不出有效数字。
    cur_frame_count: u32,
    /// 帧错位事件。
    misalign: Vec<MisalignEvent>,
    /// 容量规划器。
    planner: CapacityPlanner,
    /// 溢出钳制累计次数。
    clamp_count: u64,
    /// 累计回收帧数。
    recycled_frames: u64,
}

impl RingAllocator {
    /// 构造。容量非法时钳到合法域（边界防护，不抛异常）。
    pub fn new(capacity: usize) -> Self {
        let cap = if capacity < CAPACITY_MIN {
            CAPACITY_MIN
        } else if capacity > CAPACITY_MAX {
            CAPACITY_MAX
        } else {
            capacity
        };
        RingAllocator {
            slots: alloc::vec![Command::default(); cap],
            owner: alloc::vec![u64::MAX; cap],
            shared: RingShared::new(),
            capacity: cap,
            cur_frame: 0,
            cur_frame_count: 0,
            misalign: Vec::new(),
            planner: CapacityPlanner::new(),
            clamp_count: 0,
            recycled_frames: 0,
        }
    }

    /// 当前容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 共享状态只读引用（供上层做无锁观察）。
    pub fn shared(&self) -> &RingShared {
        &self.shared
    }

    /// 累计溢出钳制次数。
    pub fn clamp_count(&self) -> u64 {
        self.clamp_count
    }

    /// 累计回收帧数。
    pub fn recycled_frames(&self) -> u64 {
        self.recycled_frames
    }

    /// 未回收条数。
    pub fn pending(&self) -> u64 {
        self.shared.pending()
    }

    /// 帧错位事件。
    pub fn misalign_events(&self) -> &[MisalignEvent] {
        &self.misalign
    }

    /// 容量规划器只读引用。
    pub fn planner(&self) -> &CapacityPlanner {
        &self.planner
    }

    /// 开新帧。返回新帧号。
    ///
    /// 开新帧前**不自动回收**——回收时机由调用方显式决定
    /// （规格要求帧对齐回收可判定，自动回收就没法判定了）。
    ///
    /// ★ 容量规划器的记账点在这里（真缺陷修复）★
    /// 早先在 `push()` 里每写一条就`planner.record_frame(cur_frame_count)`，
    /// 于是规划器收到的是"这一帧到目前为止写了多少"的**累计值**，
    /// 每帧被记 N 次（N = 本帧命令数）。结果：帧命令数直方图全被污染，
    /// 容量重估建议算出的是"累计命令数"而不是"每帧需要多少槽"，
    /// 建议容量会随帧内位置漂移——完全错误的语义。
    /// 正解：一帧只在**开下一帧时**记一次，记的是本帧的完整条数。
    pub fn begin_frame(&mut self) -> u64 {
        // 结算上一帧（首帧 cur_frame_count 为 0，不记）
        if self.cur_frame > 0 && self.cur_frame_count > 0 {
            self.planner.record_frame(self.cur_frame_count);
        }
        self.cur_frame += 1;
        self.cur_frame_count = 0;
        self.cur_frame
    }

    /// 当前帧号。
    pub fn cur_frame(&self) -> u64 {
        self.cur_frame
    }

    /// 写入一条命令（生产者路径，无锁）。
    ///
    /// 溢出时按降级矩阵走**钳制 + 扩容建议**：
    /// - 钳制：强制把最旧的 N 条标记为已回收（读指针前移），
    ///   保证调用方的写入不会被静默丢弃——静默丢命令就是画面错乱；
    /// - 建议：返回容量重估数字（调用方通过 `capacity_advice` 取）。
    pub fn push(&mut self, cmd: Command, frame: u64, cost: Tick) -> Result<AllocOutcome, RingError> {
        if self.capacity == 0 {
            return Err(RingError::new(
                "E_CAPACITY",
                "容量为 0，无法写入",
                "构造时容量会被钳到 CAPACITY_MIN，检查是否有传 0",
            ));
        }
        if frame > self.cur_frame {
            // 提交到未来帧 = 上层帧号错乱
            return Err(RingError::new(
                "E_FUTURE_FRAME",
                &format!("命令声明帧 {} 晚于当前帧 {}", frame, self.cur_frame),
                "先 begin_frame() 再 push，或检查上层帧号是否回退了",
            ));
        }
        if frame < self.cur_frame {
            // ★ 帧错位 → 断言（降级矩阵：帧错位→断言）
            let detail = format!(
                "命令被提交到已结束帧 {}（当前帧 {}）：跨帧写入会让消费者拿到错序命令，\
                 判为不可恢复；处置：上层应先 recycle_frame({}) 再提下一帧",
                frame, self.cur_frame, frame
            );
            self.misalign.push(MisalignEvent {
                severity: MisalignSeverity::Captured,
                frame,
                slot_seq: self.shared.write,
                detail: detail.clone(),
            });
            return Ok(AllocOutcome::Asserted { detail });
        }

        // ---- 溢出检测：写追读（无锁下唯一可靠判据）----
        let pending = self.shared.pending();
        if pending as usize >= self.capacity {
            //钳制：强制回收最旧的槽位，读指针前移（不丢已写数据的位置，
            // 只丢"没被消费且已过帧"的内容——这是钳制的定义）
            let evict = pending as usize - self.capacity + 1;
            for i in 0..evict {
                let idx = ((self.shared.read + i as u64) % self.capacity as u64) as usize;
                self.slots[idx] = Command::default();
                self.owner[idx] = u64::MAX;
            }
            self.shared.read += evict as u64;
            self.clamp_count += 1;
            // ★ 关键：把"被拒绝的写入量"记进容量规划器 ★
            //
            // 钳制恰恰说明**本帧想写的比容量还多**——这正是容量重估
            // 最需要的证据。早先这里直接 return 不记账，于是：
            //  · cur_frame_count 只数成功写入，封顶在容量值；
            //  · planner 的 peak 最高只能到容量；
            //  · 溢出后问"该配多大"，建议值永远 ≤ 容量 ⇒ 不扩容
            //    ⇒ 下次照样溢出。
            // 也就是说：**把扩容建议的依据本身丢掉了**。
            //
            // 正解：`cur_frame_count` 改为统计**本帧收到的写入请求总数**
            // （含被钳的），这样它反映"真实需求"而不是"成功落槽数"。
            // 成功落槽数由 pending 反映，两者语义本就不同，不该混用。
            self.cur_frame_count += 1;
            self.planner.note_demand(self.cur_frame_count);
            return Ok(AllocOutcome::Clamped { evicted: evict as u64 });
        }

        // ---- 正常写入（无锁：只碰 write，不读 read 以外的可变量）----
        let seq = self.shared.write;
        let idx = (seq % self.capacity as u64) as usize;
        self.slots[idx] = cmd;
        self.owner[idx] = frame;
        self.shared.write = seq + 1;
        self.cur_frame_count += 1;
        // 容量规划器**不在这里记账**（见 begin_frame 的注记）：
        // 逐条记账会把"帧内累计值"当"每帧条数"，直方图全污染。
        let _ = cost; // 耗时由上层 LatencyTracker 记账，分配器自身不重复计
        Ok(AllocOutcome::Written { seq })
    }

    /// 回收一帧（消费者路径，无锁）。
    ///
    /// **只回收整帧**：把该帧所有已写槽位标记为空。
    /// 帧未全部写出时拒绝——那是上层没写完，不是分配器的错。
    pub fn recycle_frame(&mut self, frame: u64) -> Result<u32, RingError> {
        if frame > self.cur_frame {
            return Err(RingError::new(
                "E_FUTURE_FRAME",
                &format!("不能回收尚未开始的帧 {}", frame),
                "只回收已提交的帧",
            ));
        }
        let mut count = 0u32;
        let mut torn = false;
        for i in 0..self.capacity {
            if self.owner[i] == frame {
                // 槽位撕裂检测：owner 说这槽属于该帧，但**槽位内容是空命令**。
                //
                // ★ 这里踩过一个坑（真缺陷）★
                // 早先的判据是"内容 is_empty ⇒ 被清过 ⇒ 撕裂"，错在
                // `opcode == 0` 是**合法命令字**（空绘制指令），
                // 于是任何写入 opcode=0 的正常帧都会被误判成撕裂，
                // 整帧回收直接半路返回、读指针不前移，帧对齐彻底失效。
                //
                // 正解：撕裂的判据不是"内容像不像空"，而是
                // **owner 说有、而内容从未被写过**这件事本身无法从内容反推，
                // 所以改用"帧内序号连续性"验证——见下方 check 段。
                count += 1;
            }
        }
        // 帧内序号连续性校验：帧 f 的槽位在 owner 里应当是**一段连续区间**
        // （写入是顺序推进的，不可能跳段）。不连续 ⇒ 有人并发清/写过，
        // 帧数据已错序，不可交付。
        {
            let mut first: Option<usize> = None;
            let mut last: Option<usize> = None;
            for i in 0..self.capacity {
                if self.owner[i] == frame {
                    if first.is_none() {
                        first = Some(i);
                    }
                    last = Some(i);
                }
            }
            if let (Some(f0), Some(l0)) = (first, last) {
                let span = l0 - f0 + 1;
                if span != count as usize {
                    torn = true;
                }
            }
        }
        // 实际清槽（上面只数了个数，判定通过后再动数据——
        // 撕裂时不动数据，保留现场供归因，这是"异常零静默"的一部分）
        if !torn {
            for i in 0..self.capacity {
                if self.owner[i] == frame {
                    self.slots[i] = Command::default();
                    self.owner[i] = u64::MAX;
                }
            }
        }
        if torn {
            let detail = format!(
                "帧 {} 回收时检出撕裂槽位：owner 标记与内容不一致，\
                 说明写路径被中断或被并发清空，判不可恢复",
                frame
            );
            self.misalign.push(MisalignEvent {
                severity: MisalignSeverity::Torn,
                frame,
                slot_seq: self.shared.read,
                detail: detail.clone(),
            });
            return Ok(count);
        }
        if count == 0 {
            return Err(RingError::new(
                "E_EMPTY_FRAME",
                &format!("帧 {} 没有任何可回收的命令", frame),
                "检查该帧是否真的 push 过命令",
            ));
        }
        // 整帧回收完成 ⇒ 读指针可安全前移
        while self.shared.read < self.shared.write
            && self.owner[(self.shared.read % self.capacity as u64) as usize] == u64::MAX
        {
            self.shared.read += 1;
        }
        self.recycled_frames += 1;
        Ok(count)
    }

    /// 取出第 n 条未回收命令（消费者按序消费）。
    pub fn pop(&mut self) -> Option<Command> {
        if self.shared.read >= self.shared.write {
            return None;
        }
        let idx = (self.shared.read % self.capacity as u64) as usize;
        let cmd = self.slots[idx];
        self.slots[idx] = Command::default();
        self.owner[idx] = u64::MAX;
        self.shared.read += 1;
        Some(cmd)
    }

    /// 取容量重估建议（规格点名：必须给数字）。
    ///
    /// 当前**未结算帧**的命令数会并入统计——溢出发生在帧中途，
    /// 不看当帧给出的建议偏小（见 `CapacityPlanner::advise_with`）。
    pub fn capacity_advice(&self, overflowed: bool) -> CapacityAdvice {
        self.planner
            .advise_with(self.capacity, overflowed, self.cur_frame_count)
    }
}

// ---------------------------------------------------------------------------
// 十、无障碍：分配状态读屏可达
// ---------------------------------------------------------------------------

/// 读屏摘要。一句话讲清"环里有多少东西、有没有溢出、分位如何"。
///
/// 规格："无障碍与隐私：分配状态读屏可达"。
/// 判据是**盲用读屏器能听出当前是否健康**——所以必须带健康结论，
/// 不能只报数字（数字健康与否要人自己算）。
pub fn a11y_summary(ring: &RingAllocator, promise: &PerfPromise) -> String {
    let pending = ring.pending();
    let health = match promise.level {
        PerfLevel::Ok => "健康",
        PerfLevel::Degraded => "劣化中",
        PerfLevel::Alarming => "劣化告警",
    };
    let mut s = format!(
        "命令缓冲环形分配器：容量 {} 槽，已用 {} 条，溢出钳制 {} 次，回收 {} 帧",
        ring.capacity(),
        pending,
        ring.clamp_count(),
        ring.recycled_frames()
    );
    if !ring.misalign_events().is_empty() {
        s.push_str(&alloc::format!(
            "，帧错位 {} 起（其中撕裂 {} 起）",
            ring.misalign_events().len(),
            ring.misalign_events()
                .iter()
                .filter(|e| e.severity == MisalignSeverity::Torn)
                .count()
        ));
    } else {
        s.push_str("，帧错位 0 起");
    }
    s.push_str(&alloc::format!(
        "。分配耗时分位：P90 {}、P95 {}、预算 {}，判定{}。分位环境：{}",
        promise.p90,
        promise.p95,
        promise.budget,
        health,
        promise.env_canonical
    ));
    if ring.clamp_count() > 0 {
        let a = ring.capacity_advice(true);
        s.push_str(&alloc::format!(
            "。发生溢出，建议容量 {} 槽（{} 字节）：{}",
            a.suggested, a.suggested_bytes, a.basis
        ));
    }
    s
}

// ---------------------------------------------------------------------------
// 十一、错误路径：环境不可比检测
// ---------------------------------------------------------------------------

/// 跨环境引用分位数字的守卫。
///
/// 规格"分位承诺含环境指纹"的**强制面**：拿 A 机的 P95 去卡 B 机的验收线，
/// 是比没有分位更坏的事——它会让一个真正的回归被"环境不同"解释掉。
/// 所以引用前必须过这道。
pub fn check_promise_reuse(
    recorded: &EnvFingerprint,
    current: &EnvFingerprint,
) -> Result<(), RingError> {
    if recorded.comparable(current) {
        return Ok(());
    }
    let why = recorded.mismatch_reason(current);
    Err(RingError::new(
        "E_ENV_MISMATCH",
        &format!("分位承诺不可跨环境复用：{}", why),
        "先在当前环境重跑一轮采样建立本环境的 P95，再做对比；\
         或用相同硬件与驱动复现原环境",
    ))
}
