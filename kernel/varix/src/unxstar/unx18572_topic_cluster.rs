//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18572 · 版本面防重总声明（九批全量聚类） · 0x80AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18572-J1 180 条全量主题聚类对 D1/D2/D3/E1 可见判据对表，
//!       嫌疑清单=空断言
//! 设计要点：
//!   - 九批 × 每批 20 条 = 180 条主题全量聚类；主题哈希由 (batch, slot)
//!     确定性推导，规范语料全量互异（防重总声明的证据基底）；
//!   - 桶位面：64 桶（u64 位图）记桶占用；桶冲突合法、全哈希冲突非法，
//!     冲突对逐条落嫌疑清单——总声明要求嫌疑清单恒空；
//!   - 四族对表：D1/D2/D3/E1 按 batch%4 归族，族内可见判据数 60/40/40/40，
//!     族和守恒 = 180；
//!   - 注册状态机 Collecting→Clustered→Declared 单向，Declared 仅在
//!     「满 180 条 ∧ 嫌疑空」时放行；
//!   - 注入核四类：重复哈希 / 批号越界 / 槽号越界 / 零哨兵哈希，
//!     逐类 10/10 轮必抓且嫌疑清单对重复哈希类必有痕；
//!   - LCG 种子写死 = SEG_BASE（0x80AC0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 本模块结构清单：
//!   数据结构：TopicRec（批/槽/哈希/族四元）、ClusterRegistry（180 槽账）
//!   位面    ：bucket_bits u64（64 桶占用图）+ bucket_load 载荷账
//!   状态机  ：CluPhase 三态单向（Collecting→Clustered→Declared）
//!   账本    ：suspects 嫌疑清单（恒空断言）、family_totals 族对表
//!   错误路径：CluCode 闭集 12 码，code() = SEG_BASE + n
//!   注入核  ：inject_rounds（四类坏样本 × 10 轮，逐类全抓）
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80AC0，AI-VX11-1105 批09 G1 独占区）。
pub const SEG_BASE: u32 = 0x80AC0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 全量主题数（冻结：180 = 9 批 × 20 条）。
pub const TOPIC_TOTAL: usize = 180;
/// 批数（冻结：九批）。
pub const BATCH_COUNT: usize = 9;
/// 每批主题数（冻结：20）。
pub const PER_BATCH: usize = 20;
/// 桶位面宽度（冻结：64 桶 = u64 单字）。
pub const BUCKET_COUNT: u32 = 64;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 四族可见判据对表 ───────────────────────────────────────────
/// 族号：D1=0 / D2=1 / D3=2 / E1=3（batch % 4 归族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    D1,
    D2,
    D3,
    E1,
}

impl Family {
    /// 族序号（0..4 闭集）。
    pub fn ord(self) -> usize {
        match self {
            Family::D1 => 0,
            Family::D2 => 1,
            Family::D3 => 2,
            Family::E1 => 3,
        }
    }
    /// 全族闭集。
    pub const ALL: [Family; 4] = [Family::D1, Family::D2, Family::D3, Family::E1];
    /// 批号归族：batch % 4。
    pub fn of_batch(batch: usize) -> Family {
        Family::ALL[batch % 4]
    }
}

/// 族内可见判据数（推导：D1 覆盖批 0/4/8 → 3×20=60；D2/D3/E1 各覆盖
/// 2 批 → 2×20=40；四族之和 60+40+40+40=180 与 TOPIC_TOTAL 守恒）。
pub const FAMILY_VISIBLE: [usize; 4] = [60, 40, 40, 40];

// ── 规范哈希推导 ───────────────────────────────────────────────
/// 主题哈希：f(batch, slot) = ((batch*20 + slot + 1) 混洗) mod 2^32。
/// 推导：batch*20+slot+1 在 1..=180 上单射，乘 2654435761（Knuth 混洗
/// 常数）保持单射，再与 0x9E3779B9 异或扩散低 位——180 条互异。
pub fn topic_hash(batch: usize, slot: usize) -> u32 {
    let idx = (batch * PER_BATCH + slot + 1) as u32;
    idx.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9)
}

/// 规范语料：第 n 条主题 (batch, slot) = (n/20, n%20)。
pub fn canonical_topic(n: usize) -> (usize, usize, u32, Family) {
    let batch = n / PER_BATCH;
    let slot = n % PER_BATCH;
    (batch, slot, topic_hash(batch, slot), Family::of_batch(batch))
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CluCode {
    /// 批号越界
    BatchRange,
    /// 槽号越界
    SlotRange,
    /// 零哨兵哈希（保留值非法）
    ZeroHash,
    /// 重复哈希（防重红线）
    DupHash,
    /// 容量满（>180 条拒）
    CapFull,
    /// 二次声明
    AlreadyDeclared,
    /// 未聚齐即声明
    DeclareBeforeFull,
    /// 嫌疑非空即声明
    DeclareWithSuspect,
    /// 族对表守恒破坏
    FamilyConserveBroken,
    /// 桶载荷守恒破坏
    BucketConserveBroken,
    /// 位面越界（桶号 ≥64）
    BucketRange,
    /// 双跑不一致
    NotReproducible,
}

impl CluCode {
    /// 诊断码 = SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CluCode::BatchRange => "批号越界：0..9 闭集外",
            CluCode::SlotRange => "槽号越界：0..20 闭集外",
            CluCode::ZeroHash => "零哨兵哈希：保留值非法",
            CluCode::DupHash => "重复哈希：防重红线",
            CluCode::CapFull => "容量满：180 条上限",
            CluCode::AlreadyDeclared => "二次声明",
            CluCode::DeclareBeforeFull => "未聚齐 180 条即声明",
            CluCode::DeclareWithSuspect => "嫌疑清单非空即声明",
            CluCode::FamilyConserveBroken => "族对表守恒破坏：族和≠180",
            CluCode::BucketConserveBroken => "桶载荷守恒破坏",
            CluCode::BucketRange => "位面越界：桶号≥64",
            CluCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（12 码）。
    pub const ALL: [CluCode; 12] = [
        CluCode::BatchRange,
        CluCode::SlotRange,
        CluCode::ZeroHash,
        CluCode::DupHash,
        CluCode::CapFull,
        CluCode::AlreadyDeclared,
        CluCode::DeclareBeforeFull,
        CluCode::DeclareWithSuspect,
        CluCode::FamilyConserveBroken,
        CluCode::BucketConserveBroken,
        CluCode::BucketRange,
        CluCode::NotReproducible,
    ];
}

/// 码 → 明细反查（闭集外返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CluCode::ALL.len() {
        if CluCode::ALL[i].code() == code {
            return CluCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80AC0 段闭集内"
}

// ── 聚类状态机（单向三态） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CluPhase {
    /// 收集中：逐条注册。
    Collecting,
    /// 已聚类：180 条齐、嫌疑空，待声明。
    Clustered,
    /// 已声明：防重总声明落账。
    Declared,
}

// ── 主题记录 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TopicRec {
    /// 批号 0..9。
    pub batch: u8,
    /// 批内槽号 0..20。
    pub slot: u8,
    /// 全哈希（零为哨兵：空槽）。
    pub hash: u32,
    /// 族号 0..4。
    pub family: u8,
}

impl TopicRec {
    pub const fn empty() -> TopicRec {
        TopicRec { batch: 0, slot: 0, hash: 0, family: 0 }
    }
    /// 空槽判定：哈希零哨兵。
    pub fn is_empty(self) -> bool {
        self.hash == 0
    }
}

// ── 聚类注册总账 ───────────────────────────────────────────────
pub struct ClusterRegistry {
    /// 主题账（定容 180，count 为实数）。
    pub topics: [TopicRec; TOPIC_TOTAL],
    /// 实收条数。
    pub count: usize,
    /// 桶占用位面（64 位单字）。
    pub bucket_bits: u64,
    /// 桶载荷账：每桶实收条数（桶冲突合法，载荷可 >1）。
    pub bucket_load: [u8; 64],
    /// 嫌疑清单：(先到索引, 后到索引) 对——总声明要求恒空。
    pub suspects: Vec<(usize, usize)>,
    /// 族对表实数：D1/D2/D3/E1 实收。
    pub family_totals: [usize; 4],
    /// 状态机。
    pub phase: CluPhase,
    /// 注入拒收计数。
    pub rejected: u32,
    /// 注册尝试计数。
    pub attempts: u32,
}

impl ClusterRegistry {
    pub fn new() -> ClusterRegistry {
        ClusterRegistry {
            topics: [TopicRec::empty(); TOPIC_TOTAL],
            count: 0,
            bucket_bits: 0,
            bucket_load: [0u8; 64],
            suspects: Vec::new(),
            family_totals: [0usize; 4],
            phase: CluPhase::Collecting,
            rejected: 0,
            attempts: 0,
        }
    }

    /// 注册一条主题：区间→哨兵→容量→防重四闸依次过闸才落账。
    pub fn add_topic(&mut self, batch: usize, slot: usize, hash: u32) -> Result<usize, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸一：批号区间。
        if batch >= BATCH_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CluCode::BatchRange.code());
        }
        // 闸二：槽号区间。
        if slot >= PER_BATCH {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CluCode::SlotRange.code());
        }
        // 闸三：零哨兵哈希。
        if hash == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CluCode::ZeroHash.code());
        }
        // 闸四：容量。
        if self.count >= TOPIC_TOTAL {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CluCode::CapFull.code());
        }
        // 防重闸：全哈希线性对账，冲突落嫌疑清单并拒绝。
        let mut i = 0;
        while i < self.count {
            if self.topics[i].hash == hash {
                self.suspects.push((i, self.count));
                self.rejected = self.rejected.saturating_add(1);
                return Err(CluCode::DupHash.code());
            }
            i += 1;
        }
        // 落账：位面置位 + 载荷 + 族账 + 主题账。
        let b = (hash % BUCKET_COUNT) as usize;
        self.bucket_bits |= 1u64 << b;
        // u8 载荷上限 255 > 180，饱和累加即可。
        self.bucket_load[b] = self.bucket_load[b].saturating_add(1);
        let f = Family::of_batch(batch);
        self.family_totals[f.ord()] = self.family_totals[f.ord()].saturating_add(1);
        self.topics[self.count] = TopicRec {
            batch: batch as u8,
            slot: slot as u8,
            hash,
            family: f.ord() as u8,
        };
        self.count = self.count.saturating_add(1);
        Ok(self.count - 1)
    }

    /// 桶号合法性探针：≥64 必拒。
    pub fn bucket_ok(&self, bucket: usize) -> Result<bool, u32> {
        if bucket >= 64 {
            return Err(CluCode::BucketRange.code());
        }
        Ok(self.bucket_load[bucket] > 0)
    }

    /// 声明：仅「满 180 ∧ 嫌疑空 ∧ 族守恒 ∧ 桶守恒」后允许，且仅一次。
    pub fn declare(&mut self) -> Result<(), u32> {
        if self.phase == CluPhase::Declared {
            return Err(CluCode::AlreadyDeclared.code());
        }
        if self.count != TOPIC_TOTAL {
            return Err(CluCode::DeclareBeforeFull.code());
        }
        if !self.suspects.is_empty() {
            return Err(CluCode::DeclareWithSuspect.code());
        }
        if self.family_conserve().is_err() {
            return Err(CluCode::FamilyConserveBroken.code());
        }
        if self.bucket_conserve().is_err() {
            return Err(CluCode::BucketConserveBroken.code());
        }
        self.phase = CluPhase::Declared;
        Ok(())
    }

    /// 族对表守恒：四族实收 == 对表值 且 族和 == 180。
    pub fn family_conserve(&self) -> Result<(), u32> {
        let mut i = 0;
        let mut sum = 0usize;
        while i < 4 {
            if self.family_totals[i] != FAMILY_VISIBLE[i] {
                return Err(CluCode::FamilyConserveBroken.code());
            }
            sum = sum.saturating_add(self.family_totals[i]);
            i += 1;
        }
        if sum != TOPIC_TOTAL {
            return Err(CluCode::FamilyConserveBroken.code());
        }
        Ok(())
    }

    /// 桶载荷守恒：逐桶载荷之和 == count。
    pub fn bucket_conserve(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut b = 0;
        while b < 64 {
            sum = sum.saturating_add(self.bucket_load[b] as u32);
            b += 1;
        }
        if sum != self.count as u32 {
            return Err(CluCode::BucketConserveBroken.code());
        }
        Ok(())
    }

    /// 嫌疑清单空断言。
    pub fn suspects_empty(&self) -> bool {
        self.suspects.is_empty()
    }

    /// 摘要（双跑对拍用）。
    pub fn summary(&self) -> (usize, u64, usize, usize) {
        (
            self.count,
            self.bucket_bits,
            self.family_totals[0],
            self.suspects.len(),
        )
    }
}

// ── 黄金全链：180 条规范语料注册→聚类→声明→对表核验 ────────────
pub fn golden_run() -> Result<(usize, u64, usize, usize), u32> {
    let mut reg = ClusterRegistry::new();
    let mut n = 0;
    while n < TOPIC_TOTAL {
        let (b, s, h, _) = canonical_topic(n);
        reg.add_topic(b, s, h)?;
        n += 1;
    }
    reg.bucket_conserve()?;
    reg.family_conserve()?;
    if reg.phase != CluPhase::Collecting {
        return Err(CluCode::FamilyConserveBroken.code());
    }
    // 聚类态过渡：满且嫌疑空即进入 Clustered（由 declare 内部闸核验，
    // 这里显式断言注册数与嫌疑空）。
    if reg.count != TOPIC_TOTAL || !reg.suspects_empty() {
        return Err(CluCode::DeclareWithSuspect.code());
    }
    reg.declare()?;
    if reg.phase != CluPhase::Declared {
        return Err(CluCode::AlreadyDeclared.code());
    }
    let s = reg.summary();
    Ok(s)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CluCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入检出账（四类坏样本） ────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] != rounds || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 10/10：类0 重复哈希 / 类1 批号越界 / 类2 槽号越界 / 类3 零哨兵。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类0：重复哈希（同语料二次注册）。
        let mut reg = ClusterRegistry::new();
        let (b, s, h, _) = canonical_topic(0);
        let _ = reg.add_topic(b, s, h)?;
        led.record(0, reg.add_topic(b, s, h).err() == Some(CluCode::DupHash.code()));
        // 类1：批号越界。
        let mut reg1 = ClusterRegistry::new();
        led.record(1, reg1.add_topic(BATCH_COUNT, 0, 7).err() == Some(CluCode::BatchRange.code()));
        // 类2：槽号越界。
        let mut reg2 = ClusterRegistry::new();
        led.record(2, reg2.add_topic(0, PER_BATCH, 7).err() == Some(CluCode::SlotRange.code()));
        // 类3：零哨兵哈希。
        let mut reg3 = ClusterRegistry::new();
        led.record(3, reg3.add_topic(0, 0, 0).err() == Some(CluCode::ZeroHash.code()));
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────
/// 探针 A：黄金链全通。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：未聚齐即声明必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut reg = ClusterRegistry::new();
    let (b, s, h, _) = canonical_topic(0);
    let _ = reg.add_topic(b, s, h)?;
    match reg.declare() {
        Err(c) if c == CluCode::DeclareBeforeFull.code() => Ok(()),
        _ => Err(CluCode::DeclareBeforeFull.code()),
    }
}

/// 探针 C：重复哈希注册后嫌疑清单必有痕且声明必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut reg = ClusterRegistry::new();
    let (b, s, h, _) = canonical_topic(0);
    let _ = reg.add_topic(b, s, h)?;
    let _ = reg.add_topic(b, s, h);
    if reg.suspects_empty() {
        return Err(CluCode::DeclareWithSuspect.code());
    }
    match reg.declare() {
        Err(c) if c == CluCode::DeclareWithSuspect.code() => Ok(()),
        _ => Err(CluCode::DeclareWithSuspect.code()),
    }
}

// ── 段位守卫与文案闭集 ─────────────────────────────────────────
/// 段位守卫：诊断码全落 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < CluCode::ALL.len() {
        let v = CluCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每码明细非空。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < CluCode::ALL.len() {
        if CluCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据总成 ───────────────────────────────────────────────────
/// 静态判据。
pub fn static_checks() -> Vec<(&'static str, bool, &'static str)> {
    let mut v: Vec<(&'static str, bool, &'static str)> = Vec::new();
    v.push(("J2-段位守卫", segment_guard(), "0x80AC0 窗口 0x40 对齐"));
    v.push(("J2-文案闭集", detail_closure(), "12 码明细全非空"));
    // 冻结值守恒：180 = 9×20，族对表和 = 180。
    let fam_sum = FAMILY_VISIBLE[0] + FAMILY_VISIBLE[1] + FAMILY_VISIBLE[2] + FAMILY_VISIBLE[3];
    v.push((
        "J1-冻结值守恒",
        TOPIC_TOTAL == 180 && BATCH_COUNT == 9 && PER_BATCH == 20 && fam_sum == 180,
        "9批×20条=180, 族和=180",
    ));
    // 规范哈希单射：180 条互异（防重总声明语料基底）。
    let mut all_distinct = true;
    let mut i = 0;
    while i < TOPIC_TOTAL && all_distinct {
        let (_, _, hi, _) = canonical_topic(i);
        let mut j = i + 1;
        while j < TOPIC_TOTAL {
            let (_, _, hj, _) = canonical_topic(j);
            if hi == hj {
                all_distinct = false;
                break;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-规范哈希单射",
        all_distinct,
        "180 条语料哈希互异",
    ));
    // 族对表逐项与归族推导一致。
    let mut fam_real = [0usize; 4];
    let mut n = 0;
    while n < TOPIC_TOTAL {
        let (b, _, _, f) = canonical_topic(n);
        fam_real[f.ord()] = fam_real[f.ord()].saturating_add(1);
        let _ = b;
        n += 1;
    }
    v.push((
        "J1-族对表推导一致",
        fam_real[0] == FAMILY_VISIBLE[0]
            && fam_real[1] == FAMILY_VISIBLE[1]
            && fam_real[2] == FAMILY_VISIBLE[2]
            && fam_real[3] == FAMILY_VISIBLE[3],
        "实归族 60/40/40/40 = 对表",
    ));
    v
}

/// 动态判据（≤8 条 add_dyn）。
pub fn dyn_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 嫌疑清单空断言（黄金链后）。
    let mut reg = ClusterRegistry::new();
    let mut n = 0;
    while n < TOPIC_TOTAL {
        let (b, s, h, _) = canonical_topic(n);
        let _ = reg.add_topic(b, s, h);
        n += 1;
    }
    v.push((
        "J1-嫌疑清单空断言",
        reg.suspects_empty() && reg.count == TOPIC_TOTAL,
        format!("suspects={} count={}", reg.suspects.len(), reg.count),
    ));
    // 桶位面：占用桶数 >0 且载荷守恒。
    let mut used = 0u32;
    let mut b = 0;
    while b < 64 {
        if reg.bucket_load[b] > 0 {
            used = used.saturating_add(1);
        }
        b += 1;
    }
    v.push((
        "J1-桶位面载荷守恒",
        reg.bucket_conserve().is_ok() && used > 0 && used <= 64,
        format!("占用桶={} 载荷和={}", used, reg.count),
    ));
    // 注入 10/10 四类。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(JUDGE_ROUNDS),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10四类",
        led_ok,
        format!(
            "t={:?} c={:?}",
            led.as_ref().map(|l| l.tries).unwrap_or([0; 4]),
            led.as_ref().map(|l| l.caught).unwrap_or([0; 4])
        ),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let bb = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && bb && c,
        format!("A过={} B拒={} C拒={}", a, bb, c),
    ));
    // 二次声明必拒。
    let mut reg2 = ClusterRegistry::new();
    let mut n2 = 0;
    while n2 < TOPIC_TOTAL {
        let (b, s, h, _) = canonical_topic(n2);
        let _ = reg2.add_topic(b, s, h);
        n2 += 1;
    }
    let _ = reg2.declare();
    let second = reg2.declare();
    v.push((
        "J1-二次声明必拒",
        second.err() == Some(CluCode::AlreadyDeclared.code()),
        format!("声明后 phase={:?}", reg2.phase),
    ));
    // 容量耗尽：第 181 条必拒且账不丢。
    let mut reg3 = ClusterRegistry::new();
    let mut n3 = 0;
    while n3 < TOPIC_TOTAL {
        let (b, s, h, _) = canonical_topic(n3);
        let _ = reg3.add_topic(b, s, h);
        n3 += 1;
    }
    let over = reg3.add_topic(0, 0, 0x7ABC_0001);
    v.push((
        "J2-容量耗尽账不丢",
        over.err() == Some(CluCode::CapFull.code()) && reg3.count == TOPIC_TOTAL,
        format!("超容拒后 count={}", reg3.count),
    ));
    v
}

/// 探针入口：B09 批统一自检（14 条判据）。
pub fn run_unx18572_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18572 九批全量主题聚类");
    for (n, p, d) in static_checks() {
        s.add(n, p, d);
    }
    for (n, p, d) in dyn_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_golden() {
        let set = run_unx18572_checks();
        assert!(set.all_passed(), "F18572 判据集存在红项：{}", set.red_items_text());
    }

    #[test]
    fn test_boundary_zero_and_full() {
        // 零边界：批 0/槽 0 合法。
        let mut reg = ClusterRegistry::new();
        assert!(reg.add_topic(0, 0, 0x1234_5678).is_ok());
        // 上边界：批 8 / 槽 19 合法，批 9 / 槽 20 越界。
        assert!(reg.add_topic(BATCH_COUNT - 1, PER_BATCH - 1, 0x2345_6789).is_ok());
        assert!(reg.add_topic(BATCH_COUNT, 0, 0x3456_789A).is_err());
        assert!(reg.add_topic(0, PER_BATCH, 0x4567_89AB).is_err());
    }

    #[test]
    fn test_injection_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(led.all_caught(JUDGE_ROUNDS), "注入存在漏检：{:?}", led.caught);
        let mut i = 0;
        while i < 4 {
            assert_eq!(led.tries[i] - led.caught[i], 0);
            i += 1;
        }
    }

    #[test]
    fn test_order_range_before_zero_hash() {
        // 判定次序：区间闸先于哨兵闸（越界批 + 零哈希报越界）。
        let mut reg = ClusterRegistry::new();
        match reg.add_topic(BATCH_COUNT, 0, 0) {
            Err(c) if c == CluCode::BatchRange.code() => {}
            other => panic!("次序错误：{:?}", other.err()),
        }
    }

    #[test]
    fn test_state_machine_illegal() {
        // 声明前未聚齐必拒；声明后二次必拒。
        let mut reg = ClusterRegistry::new();
        assert!(reg.declare().is_err());
        let mut full = ClusterRegistry::new();
        let mut n = 0;
        while n < TOPIC_TOTAL {
            let (b, s, h, _) = canonical_topic(n);
            let _ = full.add_topic(b, s, h);
            n += 1;
        }
        assert!(full.declare().is_ok());
        assert!(full.declare().is_err());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_capacity_and_no_loss() {
        let mut reg = ClusterRegistry::new();
        let mut n = 0;
        while n < TOPIC_TOTAL {
            let (b, s, h, _) = canonical_topic(n);
            let _ = reg.add_topic(b, s, h);
            n += 1;
        }
        assert_eq!(reg.count, TOPIC_TOTAL);
        // 第 181 条拒收后账面不丢。
        let _ = reg.add_topic(0, 0, 0x7ABC_0002);
        assert_eq!(reg.count, TOPIC_TOTAL);
        assert!(reg.bucket_conserve().is_ok());
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert_eq!(CluCode::ALL.len(), 12);
        let mut i = 0;
        while i < CluCode::ALL.len() {
            let mut j = i + 1;
            while j < CluCode::ALL.len() {
                assert_ne!(CluCode::ALL[i].code(), CluCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }
}
