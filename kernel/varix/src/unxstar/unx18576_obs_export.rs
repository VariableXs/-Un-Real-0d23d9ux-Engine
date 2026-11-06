//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18576 · 版本面观测总出口（统一导出器） · 0x80BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18576-J1 裁决轨迹/画像/性能三族单点导出，消费方
//!       （AI-87/AI-25）签名一致
//! 设计要点：
//!   - 三族记录定容账：裁决轨迹 Trace / 画像 Profile / 性能 Perf，
//!     每族独立序号账，族内序号严格单调递增（乱序拒）；
//!   - 记录校验和 = family + seq + Σpayload（wrapping 和），落账前核验，
//!     篡改必拒（注入核证据基底）；
//!   - 单点导出：唯一出口 export_all() 产出定版 ExportPack（魔数/版本/
//!     三族计数/包校验和），封印前导出必拒、封印后追加必拒；
//!   - 消费方签名一致：AI-87 消费（魔数+版本+轨迹/画像计数），
//!     AI-25 消费（魔数+版本+性能计数+包校验和），两方签名核验均须过
//!     且魔数/版本同源；
//!   - 导出器状态机 Open→Sealed→Exported 单向；
//!   - 注入核四类：族号越界 / 校验和不符 / 序号乱序 / 封印前导出，
//!     逐类 10/10 轮必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80BC0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 本模块结构清单：
//!   数据结构：ObsRecord（族/序/载荷/校验和四元）、ObsExporter（三族账）
//!   状态机  ：ExpPhase 三态单向（Open→Sealed→Exported）
//!   账本    ：family_counts 三族计数账、seq_last 各族末序账
//!   错误路径：ObsCode 闭集 12 码，code() = SEG_BASE + n
//!   注入核  ：inject_rounds（四类坏样本 × 10 轮，逐类全抓）
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80BC0，AI-VX11-1105 批09 G1 独占区）。
pub const SEG_BASE: u32 = 0x80BC0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 每族记录容量（冻结：16 条/族）。
pub const FAMILY_CAP: usize = 16;
/// 记录载荷字数（冻结：8 字）。
pub const PAYLOAD_WORDS: usize = 8;
/// 导出包魔数（写死：段基 | 0xOB5 标识位）。
pub const PACK_MAGIC: u32 = 0x80BC0 | 0xB5000;
/// 导出包版本（冻结：v1）。
pub const PACK_VERSION: u16 = 1;
/// 序号上限（冻结：u16 域，单族 ≤ 65535）。
pub const SEQ_MAX: u32 = 65535;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 三族闭集 ───────────────────────────────────────────────────
/// 族号：Trace=0 / Profile=1 / Perf=2。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsFamily {
    Trace,
    Profile,
    Perf,
}

impl ObsFamily {
    /// 族序号。
    pub fn ord(self) -> usize {
        match self {
            ObsFamily::Trace => 0,
            ObsFamily::Profile => 1,
            ObsFamily::Perf => 2,
        }
    }
    /// 族号 → 族（域外 None）。
    pub fn from_ord(v: u32) -> Option<ObsFamily> {
        match v {
            0 => Some(ObsFamily::Trace),
            1 => Some(ObsFamily::Profile),
            2 => Some(ObsFamily::Perf),
            _ => None,
        }
    }
    /// 全族闭集。
    pub const ALL: [ObsFamily; 3] = [ObsFamily::Trace, ObsFamily::Profile, ObsFamily::Perf];
}

// ── 导出器状态机（单向三态） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpPhase {
    /// 开账：追加记录中。
    Open,
    /// 已封印：只读，待导出。
    Sealed,
    /// 已导出：单点出口已发。
    Exported,
}

// ── 观测记录 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsRecord {
    /// 族号 0..3。
    pub family: u8,
    /// 族内序号（1 起单调）。
    pub seq: u32,
    /// 载荷 8 字。
    pub payload: [u32; PAYLOAD_WORDS],
    /// 校验和 = family + seq + Σpayload（wrapping）。
    pub checksum: u32,
}

impl ObsRecord {
    pub const fn empty() -> ObsRecord {
        ObsRecord { family: 0, seq: 0, payload: [0u32; PAYLOAD_WORDS], checksum: 0 }
    }
    /// 校验和推导（写死公式，落账前重算比对）。
    pub fn derive_checksum(family: u32, seq: u32, payload: &[u32; PAYLOAD_WORDS]) -> u32 {
        let mut acc = family.wrapping_add(seq);
        let mut i = 0;
        while i < PAYLOAD_WORDS {
            acc = acc.wrapping_add(payload[i]);
            i += 1;
        }
        acc
    }
    /// 本记录校验和自洽。
    pub fn self_consistent(&self) -> bool {
        Self::derive_checksum(self.family as u32, self.seq, &self.payload) == self.checksum
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 族号越界
    FamilyRange,
    /// 校验和不符（篡改）
    ChecksumMismatch,
    /// 序号乱序（族内回退/重复）
    SeqDisorder,
    /// 封印前导出
    ExportBeforeSeal,
    /// 封印后追加
    AppendAfterSeal,
    /// 族账满
    FamilyFull,
    /// 二次导出
    AlreadyExported,
    /// 消费方签名不一致（AI-87）
    SigAi87Broken,
    /// 消费方签名不一致（AI-25）
    SigAi25Broken,
    /// 包校验和不符
    PackChecksumBroken,
    /// 导出前未封印全族（有空族）
    EmptyFamily,
    /// 双跑不一致
    NotReproducible,
}

impl ObsCode {
    /// 诊断码 = SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::FamilyRange => "族号越界：0..3 闭集外",
            ObsCode::ChecksumMismatch => "校验和不符：篡改必拒",
            ObsCode::SeqDisorder => "序号乱序：族内须单调",
            ObsCode::ExportBeforeSeal => "封印前导出",
            ObsCode::AppendAfterSeal => "封印后追加：只读红线",
            ObsCode::FamilyFull => "族账满：16 条上限",
            ObsCode::AlreadyExported => "二次导出：单点出口幂等",
            ObsCode::SigAi87Broken => "AI-87 签名不一致",
            ObsCode::SigAi25Broken => "AI-25 签名不一致",
            ObsCode::PackChecksumBroken => "包校验和不符",
            ObsCode::EmptyFamily => "有空族：三族须齐备",
            ObsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（12 码）。
    pub const ALL: [ObsCode; 12] = [
        ObsCode::FamilyRange,
        ObsCode::ChecksumMismatch,
        ObsCode::SeqDisorder,
        ObsCode::ExportBeforeSeal,
        ObsCode::AppendAfterSeal,
        ObsCode::FamilyFull,
        ObsCode::AlreadyExported,
        ObsCode::SigAi87Broken,
        ObsCode::SigAi25Broken,
        ObsCode::PackChecksumBroken,
        ObsCode::EmptyFamily,
        ObsCode::NotReproducible,
    ];
}

/// 码 → 明细反查（闭集外返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ObsCode::ALL.len() {
        if ObsCode::ALL[i].code() == code {
            return ObsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80BC0 段闭集内"
}

// ── 导出包 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportPack {
    /// 魔数。
    pub magic: u32,
    /// 版本。
    pub version: u16,
    /// 三族计数。
    pub counts: [u32; 3],
    /// 包校验和 = magic + version + Σcounts（wrapping）。
    pub checksum: u32,
}

impl ExportPack {
    /// 包校验和推导（写死公式）。
    pub fn derive_checksum(magic: u32, version: u16, counts: &[u32; 3]) -> u32 {
        magic
            .wrapping_add(version as u32)
            .wrapping_add(counts[0])
            .wrapping_add(counts[1])
            .wrapping_add(counts[2])
    }
    /// 包自洽。
    pub fn self_consistent(&self) -> bool {
        Self::derive_checksum(self.magic, self.version, &self.counts) == self.checksum
    }
}

// ── 统一导出器 ─────────────────────────────────────────────────
pub struct ObsExporter {
    /// 三族记录账（定容）。
    pub records: [Vec<ObsRecord>; 3],
    /// 三族计数。
    pub family_counts: [u32; 3],
    /// 各族末序（0 = 空族）。
    pub seq_last: [u32; 3],
    /// 状态机。
    pub phase: ExpPhase,
    /// 导出包（导出后落账）。
    pub pack: Option<ExportPack>,
    /// 尝试计数。
    pub attempts: u32,
    /// 拒收计数。
    pub rejected: u32,
}

impl ObsExporter {
    pub fn new() -> ObsExporter {
        ObsExporter {
            records: [Vec::new(), Vec::new(), Vec::new()],
            family_counts: [0u32; 3],
            seq_last: [0u32; 3],
            phase: ExpPhase::Open,
            pack: None,
            attempts: 0,
            rejected: 0,
        }
    }

    /// 追加一条观测记录：封印→族号→校验和→序号→容量五闸依次过闸。
    pub fn add_record(&mut self, rec: ObsRecord) -> Result<usize, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸一：封印后追加必拒。
        if self.phase != ExpPhase::Open {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ObsCode::AppendAfterSeal.code());
        }
        // 闸二：族号闭集。
        let fam = match ObsFamily::from_ord(rec.family as u32) {
            Some(f) => f,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(ObsCode::FamilyRange.code());
            }
        };
        // 闸三：校验和重算比对。
        if !rec.self_consistent() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ObsCode::ChecksumMismatch.code());
        }
        // 闸四：族内序号严格单调（> 末序）。
        let f = fam.ord();
        if rec.seq == 0 || rec.seq > SEQ_MAX || rec.seq <= self.seq_last[f] {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ObsCode::SeqDisorder.code());
        }
        // 闸五：族账容量。
        if self.records[f].len() >= FAMILY_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ObsCode::FamilyFull.code());
        }
        self.records[f].push(rec);
        self.family_counts[f] = self.family_counts[f].saturating_add(1);
        self.seq_last[f] = rec.seq;
        Ok(self.records[f].len() - 1)
    }

    /// 封印：三族齐备（每族 ≥1 条）才允许，且仅一次。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != ExpPhase::Open {
            return Err(ObsCode::AppendAfterSeal.code());
        }
        let mut f = 0;
        while f < 3 {
            if self.records[f].is_empty() {
                return Err(ObsCode::EmptyFamily.code());
            }
            f += 1;
        }
        self.phase = ExpPhase::Sealed;
        Ok(())
    }

    /// 单点导出：仅封印态允许，产出定版 ExportPack 并置 Exported。
    pub fn export_all(&mut self) -> Result<ExportPack, u32> {
        if self.phase == ExpPhase::Open {
            return Err(ObsCode::ExportBeforeSeal.code());
        }
        if self.phase == ExpPhase::Exported {
            return Err(ObsCode::AlreadyExported.code());
        }
        let counts = [
            self.family_counts[0],
            self.family_counts[1],
            self.family_counts[2],
        ];
        let pack = ExportPack {
            magic: PACK_MAGIC,
            version: PACK_VERSION,
            counts,
            checksum: ExportPack::derive_checksum(PACK_MAGIC, PACK_VERSION, &counts),
        };
        if !pack.self_consistent() {
            return Err(ObsCode::PackChecksumBroken.code());
        }
        self.pack = Some(pack);
        self.phase = ExpPhase::Exported;
        Ok(pack)
    }

    /// AI-87 消费方签名核验：签名 = (魔数, 版本, 轨迹+画像计数和)。
    pub fn sig_ai87(&self) -> Result<(u32, u16, u32), u32> {
        match self.pack {
            Some(p) => {
                if p.magic != PACK_MAGIC || p.version != PACK_VERSION {
                    return Err(ObsCode::SigAi87Broken.code());
                }
                let n = p.counts[0].saturating_add(p.counts[1]);
                if n != self.family_counts[0].saturating_add(self.family_counts[1]) {
                    return Err(ObsCode::SigAi87Broken.code());
                }
                Ok((p.magic, p.version, n))
            }
            None => Err(ObsCode::SigAi87Broken.code()),
        }
    }

    /// AI-25 消费方签名核验：签名 = (魔数, 版本, 性能计数, 包校验和)。
    pub fn sig_ai25(&self) -> Result<(u32, u16, u32, u32), u32> {
        match self.pack {
            Some(p) => {
                if p.magic != PACK_MAGIC
                    || p.version != PACK_VERSION
                    || p.counts[2] != self.family_counts[2]
                    || !p.self_consistent()
                {
                    return Err(ObsCode::SigAi25Broken.code());
                }
                Ok((p.magic, p.version, p.counts[2], p.checksum))
            }
            None => Err(ObsCode::SigAi25Broken.code()),
        }
    }

    /// 全账自洽：逐条记录校验和 + 族账计数守恒。
    pub fn ledger_conserve(&self) -> Result<(), u32> {
        let mut f = 0;
        while f < 3 {
            let mut sum = 0u32;
            let mut i = 0;
            let n = self.records[f].len();
            while i < n {
                let r = match self.records[f].get(i) {
                    Some(r) => *r,
                    None => return Err(ObsCode::FamilyRange.code()),
                };
                if !r.self_consistent() {
                    return Err(ObsCode::ChecksumMismatch.code());
                }
                sum = sum.saturating_add(1);
                i += 1;
            }
            if sum != self.family_counts[f] {
                return Err(ObsCode::PackChecksumBroken.code());
            }
            f += 1;
        }
        Ok(())
    }

    /// 摘要（双跑对拍用）。
    pub fn summary(&self) -> (u8, u32, u32, u32) {
        let ph = match self.phase {
            ExpPhase::Open => 0u8,
            ExpPhase::Sealed => 1u8,
            ExpPhase::Exported => 2u8,
        };
        (ph, self.family_counts[0], self.family_counts[1], self.family_counts[2])
    }
}

// ── 规范语料：各族 4 条确定性记录 ──────────────────────────────
/// 规范记录推导：payload[k] = (family*16 + k) * 11 + seq（写死公式）。
pub fn make_record(family: usize, seq: u32) -> ObsRecord {
    let mut payload = [0u32; PAYLOAD_WORDS];
    let mut k = 0;
    while k < PAYLOAD_WORDS {
        payload[k] = ((family as u32) * 16 + k as u32) * 11 + seq;
        k += 1;
    }
    ObsRecord {
        family: family as u8,
        seq,
        payload,
        checksum: ObsRecord::derive_checksum(family as u32, seq, &payload),
    }
}

/// 规范语料每族条数（推导：黄金链 3 族 × 4 条 = 12 条）。
pub const CANON_PER_FAMILY: usize = 4;

// ── 黄金全链：三族各 4 条→封印→导出→双签名核验→全账自洽 ────────
pub fn golden_run() -> Result<(u8, u32, u32, u32), u32> {
    let mut ex = ObsExporter::new();
    let mut f = 0;
    while f < 3 {
        let mut k = 1;
        while k <= CANON_PER_FAMILY as u32 {
            ex.add_record(make_record(f, k))?;
            k += 1;
        }
        f += 1;
    }
    ex.seal()?;
    let pack = ex.export_all()?;
    if !pack.self_consistent() {
        return Err(ObsCode::PackChecksumBroken.code());
    }
    // 双消费方签名一致。
    let s87 = ex.sig_ai87()?;
    let s25 = ex.sig_ai25()?;
    if s87.0 != s25.0 || s87.1 != s25.1 {
        return Err(ObsCode::SigAi87Broken.code());
    }
    ex.ledger_conserve()?;
    let s = ex.summary();
    Ok(s)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ObsCode::NotReproducible.code());
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

/// 注入 10/10：类0 族号越界 / 类1 校验和篡改 / 类2 序号乱序 / 类3 封印前导出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类0：族号越界（3）。
        let mut e0 = ObsExporter::new();
        let mut bad0 = make_record(0, 1);
        bad0.family = 3;
        led.record(0, e0.add_record(bad0).err() == Some(ObsCode::FamilyRange.code()));
        // 类1：校验和篡改（载荷改一字不重算）。
        let mut e1 = ObsExporter::new();
        let mut bad1 = make_record(0, 1);
        bad1.payload[0] = bad1.payload[0].wrapping_add(1);
        led.record(1, e1.add_record(bad1).err() == Some(ObsCode::ChecksumMismatch.code()));
        // 类2：序号乱序（先 2 后 1）。
        let mut e2 = ObsExporter::new();
        let _ = e2.add_record(make_record(0, 2))?;
        led.record(2, e2.add_record(make_record(0, 1)).err() == Some(ObsCode::SeqDisorder.code()));
        // 类3：封印前导出。
        let mut e3 = ObsExporter::new();
        let _ = e3.add_record(make_record(0, 1))?;
        led.record(3, e3.export_all().err() == Some(ObsCode::ExportBeforeSeal.code()));
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────
/// 探针 A：黄金链全通。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：封印后追加必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut ex = ObsExporter::new();
    let mut f = 0;
    while f < 3 {
        let _ = ex.add_record(make_record(f, 1))?;
        f += 1;
    }
    ex.seal()?;
    match ex.add_record(make_record(0, 9)) {
        Err(c) if c == ObsCode::AppendAfterSeal.code() => Ok(()),
        _ => Err(ObsCode::AppendAfterSeal.code()),
    }
}

/// 探针 C：有空族封印必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut ex = ObsExporter::new();
    let _ = ex.add_record(make_record(0, 1))?;
    match ex.seal() {
        Err(c) if c == ObsCode::EmptyFamily.code() => Ok(()),
        _ => Err(ObsCode::EmptyFamily.code()),
    }
}

// ── 段位守卫与文案闭集 ─────────────────────────────────────────
/// 段位守卫：诊断码全落 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < ObsCode::ALL.len() {
        let v = ObsCode::ALL[i].code();
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
    while i < ObsCode::ALL.len() {
        if ObsCode::ALL[i].detail().is_empty() {
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
    v.push(("J2-段位守卫", segment_guard(), "0x80BC0 窗口 0x40 对齐"));
    v.push(("J2-文案闭集", detail_closure(), "12 码明细全非空"));
    // 冻结值：16 条/族、8 字载荷、魔数/版本写死。
    v.push((
        "J2-冻结值一致",
        FAMILY_CAP == 16 && PAYLOAD_WORDS == 8 && PACK_VERSION == 1
            && PACK_MAGIC == 0x80BC0 | 0xB5000,
        "16条/族 8字载荷 v1 魔数写死",
    ));
    // 族闭集往返。
    let mut rt = true;
    let mut f = 0;
    while f < 3 {
        match ObsFamily::from_ord(ObsFamily::ALL[f].ord() as u32) {
            Some(x) if x == ObsFamily::ALL[f] => {}
            _ => rt = false,
        }
        f += 1;
    }
    v.push((
        "J1-族闭集往返",
        rt && ObsFamily::from_ord(3).is_none() && ObsFamily::from_ord(255).is_none(),
        "0/1/2 往返恒等, 3/255 必 None",
    ));
    // 校验和推导自洽：规范记录全部自洽。
    let mut self_ok = true;
    let mut fam = 0;
    while fam < 3 {
        let mut seq = 1;
        while seq <= CANON_PER_FAMILY as u32 {
            if !make_record(fam, seq).self_consistent() {
                self_ok = false;
            }
            seq += 1;
        }
        fam += 1;
    }
    v.push((
        "J1-校验和推导自洽",
        self_ok,
        "3族×4条规范记录全自洽",
    ));
    v
}

/// 动态判据（≤8 条 add_dyn）。
pub fn dyn_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 三族单点导出：导出包三族计数全等且包自洽。
    let mut ex = ObsExporter::new();
    let mut f = 0;
    while f < 3 {
        let mut k = 1;
        while k <= CANON_PER_FAMILY as u32 {
            let _ = ex.add_record(make_record(f, k));
            k += 1;
        }
        f += 1;
    }
    let _ = ex.seal();
    let pack = ex.export_all();
    let pack_ok = match &pack {
        Ok(p) => {
            p.self_consistent()
                && p.counts[0] == CANON_PER_FAMILY as u32
                && p.counts[1] == CANON_PER_FAMILY as u32
                && p.counts[2] == CANON_PER_FAMILY as u32
        }
        Err(_) => false,
    };
    v.push((
        "J1-三族单点导出",
        pack_ok && ex.phase == ExpPhase::Exported,
        format!("counts={:?} phase={:?}", ex.family_counts, ex.phase),
    ));
    // AI-87 签名一致。
    let s87 = ex.sig_ai87();
    let s87_ok = match &s87 {
        Ok((m, ver, n)) => *m == PACK_MAGIC && *ver == PACK_VERSION && *n == 8,
        Err(_) => false,
    };
    v.push((
        "J1-AI-87签名一致",
        s87_ok,
        format!("sig87={:?}", s87),
    ));
    // AI-25 签名一致。
    let s25 = ex.sig_ai25();
    let s25_ok = match &s25 {
        Ok((m, ver, perf, ck)) => {
            *m == PACK_MAGIC && *ver == PACK_VERSION && *perf == CANON_PER_FAMILY as u32 && *ck > 0
        }
        Err(_) => false,
    };
    v.push((
        "J1-AI-25签名一致",
        s25_ok,
        format!("sig25={:?}", s25),
    ));
    // 注入 10/10 四类。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(JUDGE_ROUNDS),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10四类",
        inj_ok,
        format!(
            "t={:?} c={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; 4]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; 4])
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
    // 二次导出幂等拒。
    let second = ex.export_all();
    v.push((
        "J2-二次导出幂等拒",
        second.err() == Some(ObsCode::AlreadyExported.code()),
        format!("导出后 phase={:?}", ex.phase),
    ));
    v
}

/// 探针入口：B09 批统一自检（15 条判据）。
pub fn run_unx18576_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18576 观测统一导出器");
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
        let set = run_unx18576_checks();
        assert!(set.all_passed(), "F18576 判据集存在红项：{}", set.red_items_text());
    }

    #[test]
    fn test_boundary_zero_and_full() {
        // 零边界：序号 0 拒（1 起单调），序号 1 合法。
        let mut ex = ObsExporter::new();
        assert!(ex.add_record(make_record(0, 0)).is_err());
        assert!(ex.add_record(make_record(0, 1)).is_ok());
        // 容量上边界：填满一族后第 17 条拒且账不丢。
        let mut full = ObsExporter::new();
        let mut k = 1;
        while k <= FAMILY_CAP as u32 {
            let _ = full.add_record(make_record(2, k));
            k += 1;
        }
        assert_eq!(full.records[2].len(), FAMILY_CAP);
        let _ = full.add_record(make_record(2, 99));
        assert_eq!(full.records[2].len(), FAMILY_CAP);
    }

    #[test]
    fn test_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught(JUDGE_ROUNDS), "注入存在漏检：{:?}", inj.caught);
        let mut i = 0;
        while i < 4 {
            assert_eq!(inj.tries[i] - inj.caught[i], 0);
            i += 1;
        }
    }

    #[test]
    fn test_order_family_before_checksum() {
        // 判定次序：族号闸先于校验和闸（越界族 + 坏校验和报族越界）。
        let mut ex = ObsExporter::new();
        let mut bad = make_record(0, 1);
        bad.family = 9;
        bad.checksum = 0;
        match ex.add_record(bad) {
            Err(c) if c == ObsCode::FamilyRange.code() => {}
            other => panic!("次序错误：{:?}", other.err()),
        }
    }

    #[test]
    fn test_state_machine_illegal() {
        // 未封印导出拒 → 封印 → 追加拒 → 导出 → 二次导出拒。
        let mut ex = ObsExporter::new();
        let mut f = 0;
        while f < 3 {
            let _ = ex.add_record(make_record(f, 1));
            f += 1;
        }
        assert!(ex.export_all().is_err());
        assert!(ex.seal().is_ok());
        assert!(ex.add_record(make_record(0, 5)).is_err());
        assert!(ex.export_all().is_ok());
        assert!(ex.export_all().is_err());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_seq_monotonic_per_family() {
        // 各族序号独立单调：族 0 到 3、族 1 到 5、族 2 到 7 互不干扰。
        let mut ex = ObsExporter::new();
        assert!(ex.add_record(make_record(0, 3)).is_ok());
        assert!(ex.add_record(make_record(1, 5)).is_ok());
        assert!(ex.add_record(make_record(2, 7)).is_ok());
        // 族 0 再进 2 合法（> 3），再进 1 拒。
        assert!(ex.add_record(make_record(0, 2)).is_err());
        assert!(ex.add_record(make_record(0, 4)).is_ok());
        assert_eq!(ex.family_counts[0], 2);
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert_eq!(ObsCode::ALL.len(), 12);
        let mut i = 0;
        while i < ObsCode::ALL.len() {
            let mut j = i + 1;
            while j < ObsCode::ALL.len() {
                assert_ne!(ObsCode::ALL[i].code(), ObsCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }
}
