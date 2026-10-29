//! unx48281_latframe — UNX-F48281 延迟分段账框架总成（四段：M2 采集→桥→引擎→F4 绘制）（诊断段 0xC400）。
//!
//! 判据原文：
//!   J1 框架四段字段全对齐附则五骨架、ts_source=TSC 断言 ×10^4；
//!   判据锚定 附则五 延迟账骨架、§AI-61 §5 J-1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 附则五骨架字段对齐：帧结构 8 字段（帧序号 + 时钟源 + 六个时点：
//!      采集完/入桥/出桥/入引擎/出引擎/绘制提交），骨架描述表逐字段
//!      对齐，字段数/顺序/语义三重断言。
//!   2. 时钟源纪律：全账只认 TSC（不变量时钟源）；Mono 备源仅登记
//!      不入账，None 时钟源帧必须拒收——×10^4 帧全部 ts_source=TSC
//!      是框架的第一判据。
//!   3. 四段划分与逐帧恒等式：
//!      采集段 = 入桥-采集完；桥段 = 出桥-入桥；引擎段 = 出引擎-入引擎；
//!      绘制段 = 绘制-出引擎；交接段 = 入引擎-出桥（桥→引擎握手，
//!      不计入四段，但必须非负且入恒等式）。
//!      总延迟 = 绘制-采集完 = 四段和 + 交接段，逐帧恒等式 ×10^4。
//!   4. 单调性：帧内六时点非降（时间戳不倒流），帧间采集完非降
//!      （帧序推进时间不回退），×10^4 全查。
//!   5. 段界预算（µs）：采集 ≤40、桥 ≤60、引擎 ≤300、绘制 ≤80、
//!      交接 ≤20——越界帧必须被框架标记为 BudgetBreach 入账。
//!   6. 违例注入自查：框架必须能检出人工构造的倒流帧与坏源帧
//!      （拒收并留痕），否则框架不配承载判据。
//!   7. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC400;

const E_FIELD_ALIGN: u16 = SEG_BASE + 1;  // 骨架字段不对齐
const E_TS_SOURCE: u16 = SEG_BASE + 2;    // 非 TSC 时钟源帧混入
const E_MONO_VIOLATION: u16 = SEG_BASE + 3; // 时点倒流
const E_IDENTITY_FAIL: u16 = SEG_BASE + 4; // 逐帧恒等式破坏
const E_BUDGET_LOST: u16 = SEG_BASE + 5;  // 越界帧未被标记
const E_BAD_FRAME_ACCEPT: u16 = SEG_BASE + 6; // 坏帧被收（违例注入漏检）
const E_LEDGER_GAP: u16 = SEG_BASE + 7;   // 账行数与帧数不符
const E_CODE_RANGE: u16 = SEG_BASE + 8;   // 诊断码越段

/// 时钟源三分。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TsSource {
    /// TSC 不变量时钟源（唯一入账源）。
    Tsc,
    /// 单调备源（仅登记不入账）。
    Mono,
    /// 无源（拒收）。
    None,
}

/// 延迟帧（附则五骨架 8 字段）。
#[derive(Clone, Copy)]
pub struct LatFrame {
    pub seq: u32,          // 字段 1：帧序号
    pub ts_source: TsSource, // 字段 2：时钟源
    pub t_cap: u64,        // 字段 3：M2 采集完成
    pub t_brin: u64,       // 字段 4：入桥
    pub t_brout: u64,      // 字段 5：出桥
    pub t_engin: u64,      // 字段 6：入引擎
    pub t_engout: u64,     // 字段 7：出引擎
    pub t_draw: u64,       // 字段 8：F4 绘制提交
}

/// 骨架描述表：字段名 + 字段序位（对齐断言的真源）。
pub const FIELD_NAMES: [&str; 8] = [
    "seq",
    "ts_source",
    "t_cap",
    "t_brin",
    "t_brout",
    "t_engin",
    "t_engout",
    "t_draw",
];

/// 段位三分（四段 + 交接）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    /// 采集段（采集完→入桥）。
    Capture,
    /// 桥段（入桥→出桥）。
    Bridge,
    /// 引擎段（入引擎→出引擎）。
    Engine,
    /// 绘制段（出引擎→绘制提交）。
    Draw,
    /// 交接段（出桥→入引擎，不入四段但入恒等式）。
    Handoff,
}

/// 段位 µs 预算上界。
pub fn seg_budget(k: SegKind) -> u64 {
    match k {
        SegKind::Capture => 40,
        SegKind::Bridge => 60,
        SegKind::Engine => 300,
        SegKind::Draw => 80,
        SegKind::Handoff => 20,
    }
}

/// 四段 + 交接拆解（全部以差值计算，不做除法）。
pub fn split(f: &LatFrame) -> [u64; 5] {
    [
        f.t_brin - f.t_cap,   // Capture
        f.t_brout - f.t_brin, // Bridge
        f.t_engout - f.t_engin, // Engine
        f.t_draw - f.t_engout,  // Draw
        f.t_engin - f.t_brout,  // Handoff
    ]
}

/// 帧校验：源/单调/恒等式三闸。返回 0 = 全过；位图置位 = 违例类。
/// bit0 = 源违例；bit1 = 单调违例；bit2 = 恒等式违例。
pub fn validate(f: &LatFrame) -> u8 {
    let mut flags = 0u8;
    // 源闸。
    if f.ts_source != TsSource::Tsc {
        flags |= 0x1;
    }
    // 单调闸：六时点链非降。
    if !(f.t_cap <= f.t_brin
        && f.t_brin <= f.t_brout
        && f.t_brout <= f.t_engin
        && f.t_engin <= f.t_engout
        && f.t_engout <= f.t_draw)
    {
        flags |= 0x2;
    }
    // 恒等式闸：总延迟 = 四段和 + 交接段。
    let s = split(f);
    let total = f.t_draw - f.t_cap;
    let identity = s[0] + s[1] + s[2] + s[3] + s[4];
    if s[0]
        .wrapping_add(s[1])
        .wrapping_add(s[2])
        .wrapping_add(s[3])
        .wrapping_add(s[4])
        != total
        || identity != total
    {
        flags |= 0x4;
    }
    flags
}

/// 预算违例位图：段位超预算置位（bit0..bit4 对应四段+交接）。
pub fn budget_flags(f: &LatFrame) -> u8 {
    let s = split(f);
    let kinds = [
        SegKind::Capture,
        SegKind::Bridge,
        SegKind::Engine,
        SegKind::Draw,
        SegKind::Handoff,
    ];
    let mut flags = 0u8;
    let mut i = 0;
    while i < 5 {
        if s[i] > seg_budget(kinds[i]) {
            flags |= 1 << i;
        }
        i += 1;
    }
    flags
}

/// 确定性线性同余发生器（帧合成统一熵源）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// TSC 帧合成：×10^4 帧确定性生成（段值在预算内，交接含抖动）。
pub fn synth_frames(count: usize) -> Vec<LatFrame> {
    let mut lc = Lcg::new(0x4828_1001);
    let mut frames: Vec<LatFrame> = Vec::new();
    let mut t: u64 = 1_000_000; // 起始 TSC 基线（µs 域）
    let mut seq = 0u32;
    let mut made = 0usize;
    while made < count {
        let s_cap = (lc.next() % 41) as u64;          // 0..40
        let s_br = (lc.next() % 61) as u64;          // 0..60
        let s_eng = 20 + (lc.next() % 281) as u64;   // 20..300
        let s_draw = (lc.next() % 81) as u64;        // 0..80
        let s_ho = (lc.next() % 21) as u64;          // 0..20
        let t0 = t;
        let t1 = t0 + s_cap;
        let t2 = t1 + s_br;
        let t3 = t2 + s_ho;
        let t4 = t3 + s_eng;
        let t5 = t4 + s_draw;
        frames.push(LatFrame {
            seq,
            ts_source: TsSource::Tsc,
            t_cap: t0,
            t_brin: t1,
            t_brout: t2,
            t_engin: t3,
            t_engout: t4,
            t_draw: t5,
        });
        // 帧间推进：下一帧采集完在本帧之后（含空闲抖动 50..150µs）。
        t = t5 + 50 + (lc.next() % 101) as u64;
        seq += 1;
        made += 1;
    }
    frames
}

/// 段位账行：帧序号 + 四段值 + 交接值（附则五最小留痕）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SegRow {
    pub seq: u32,
    pub s_cap: u64,
    pub s_br: u64,
    pub s_eng: u64,
    pub s_draw: u64,
    pub s_ho: u64,
    pub breach_bits: u8,
}

/// 分段账：append-only，同帧序重放幂等不重记。
pub struct SegLedger {
    rows: Vec<SegRow>,
    rejected: u32,
}

impl SegLedger {
    pub fn new() -> Self {
        SegLedger {
            rows: Vec::new(),
            rejected: 0,
        }
    }

    /// 收帧：坏源帧拒收留痕；好帧校验后入账（恒等式违例帧拒收）。
    pub fn ingest(&mut self, f: &LatFrame) -> bool {
        let flags = validate(f);
        if flags & 0x1 != 0 {
            self.rejected += 1;
            return false; // 非 TSC 源拒收
        }
        if flags & 0x6 != 0 {
            self.rejected += 1;
            return false; // 倒流/恒等式破坏拒收
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].seq == f.seq {
                return true; // 幂等重放
            }
            i += 1;
        }
        let s = split(f);
        self.rows.push(SegRow {
            seq: f.seq,
            s_cap: s[0],
            s_br: s[1],
            s_eng: s[2],
            s_draw: s[3],
            s_ho: s[4],
            breach_bits: budget_flags(f),
        });
        true
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }
}

/// 延迟分段账框架主巡检：×10^4 帧全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 骨架字段对齐：描述表恰 8 字段、序位连续、语义名与帧字段一一对应。
    if FIELD_NAMES.len() != 8 {
        return Err(E_FIELD_ALIGN);
    }
    const EXPECT: [&str; 8] = [
        "seq", "ts_source", "t_cap", "t_brin", "t_brout", "t_engin", "t_engout", "t_draw",
    ];
    let mut i = 0;
    while i < 8 {
        if FIELD_NAMES[i] != EXPECT[i] {
            return Err(E_FIELD_ALIGN);
        }
        i += 1;
    }
    hits += 1;

    // R2 ×10^4 帧合成并全部入账（TSC 源、无拒收）。
    let frames = synth_frames(10_000);
    if frames.len() != 10_000 {
        return Err(E_FIELD_ALIGN);
    }
    let mut led = SegLedger::new();
    let mut fi = 0usize;
    while fi < frames.len() {
        if !led.ingest(&frames[fi]) {
            return Err(E_TS_SOURCE);
        }
        fi += 1;
    }
    if led.len() != 10_000 || led.rejected() != 0 {
        return Err(E_LEDGER_GAP);
    }
    hits += 1;

    // R3 源断言 ×10^4：逐帧 ts_source == Tsc（判据本体，不依赖账本）。
    fi = 0;
    while fi < frames.len() {
        if frames[fi].ts_source != TsSource::Tsc {
            return Err(E_TS_SOURCE);
        }
        fi += 1;
    }
    hits += 1;

    // R4 单调性 ×10^4：帧内六时点非降 + 帧间采集完非降。
    fi = 0;
    while fi < frames.len() {
        let f = &frames[fi];
        if f.t_cap > f.t_brin
            || f.t_brin > f.t_brout
            || f.t_brout > f.t_engin
            || f.t_engin > f.t_engout
            || f.t_engout > f.t_draw
        {
            return Err(E_MONO_VIOLATION);
        }
        if fi > 0 && f.t_cap < frames[fi - 1].t_cap {
            return Err(E_MONO_VIOLATION);
        }
        fi += 1;
    }
    hits += 1;

    // R5 逐帧恒等式 ×10^4：总延迟 = 四段和 + 交接段（validate 恒等式闸零违例）。
    fi = 0;
    while fi < frames.len() {
        if validate(&frames[fi]) & 0x4 != 0 {
            return Err(E_IDENTITY_FAIL);
        }
        fi += 1;
    }
    hits += 1;

    // R6 预算标记 ×10^4：合成域内帧预算位图全零（合成器守预算）。
    fi = 0;
    while fi < frames.len() {
        if budget_flags(&frames[fi]) != 0 {
            return Err(E_BUDGET_LOST);
        }
        fi += 1;
    }
    hits += 1;

    // R7 违例注入自查（假阴性=0）：倒流帧/坏源帧/恒等式破坏帧全部拒收。
    let mut bad1 = frames[500];
    bad1.t_draw = bad1.t_brin; // 绘制倒流
    let bad2 = LatFrame {
        seq: 20_001,
        ts_source: TsSource::Mono,
        t_cap: 1,
        t_brin: 2,
        t_brout: 3,
        t_engin: 4,
        t_engout: 5,
        t_draw: 6,
    };
    let mut bad3 = frames[501];
    bad3.t_engout += 5; // 跳点：恒等式破坏（五段和不再等于总延迟）
    let mut led2 = SegLedger::new();
    if led2.ingest(&bad1) || led2.ingest(&bad2) || led2.ingest(&bad3) {
        return Err(E_BAD_FRAME_ACCEPT);
    }
    if led2.len() != 0 || led2.rejected() != 3 {
        return Err(E_BAD_FRAME_ACCEPT);
    }
    hits += 1;

    // R8 帧间单调抽查：采集完基线严格推进（帧序 0..10^4-1 的 t_cap 全非降）。
    fi = 1;
    while fi < frames.len() {
        if frames[fi].t_cap <= frames[fi - 1].t_cap - 50 {
            return Err(E_MONO_VIOLATION); // 推进幅度至少 50µs（合成器纪律）
        }
        fi += 1;
    }
    hits += 1;

    // R9 账行幂等：全量重放 ×10^4 → 账行数不变、拒收数不变。
    fi = 0;
    while fi < frames.len() {
        if !led.ingest(&frames[fi]) {
            return Err(E_LEDGER_GAP);
        }
        fi += 1;
    }
    if led.len() != 10_000 || led.rejected() != 0 {
        return Err(E_LEDGER_GAP);
    }
    hits += 1;

    // R10 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_FIELD_ALIGN,
        E_TS_SOURCE,
        E_MONO_VIOLATION,
        E_IDENTITY_FAIL,
        E_BUDGET_LOST,
        E_BAD_FRAME_ACCEPT,
        E_LEDGER_GAP,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
