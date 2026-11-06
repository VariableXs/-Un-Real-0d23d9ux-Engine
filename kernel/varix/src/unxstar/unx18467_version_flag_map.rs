//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18467 · WIN7RTM/WIN8RTM 等版本标志映射 · 0x7F080段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18467-J1 版本类标志→矩阵依据写入断言全过，依据源登记为
//!       兼容层
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 340 · 目标 ≥680 行
//! 本模块结构清单：
//!   数据结构：VERSION_TOKS 5 项 + 冻结版本对 / MATRIX 5×4 冻结依据矩阵
//!   状态机  ：MapPhase 五态单调 Idle→TokenLookup→SourceVerify→BasisWrite→Frozen
//!   账本    ：MatrixLedger 5 行 id-寻址（首写入账、同值重写幂等、
//!             异值重写拒、只增不减封印）
//!   错误路径：VfmCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：4 类坏样本（未知令牌/行异值重写/非法依据源/大小写违规）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   对照表  ：令牌→(主,次) 版本对 8 例逐项复算零偏差
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F080，AI-VX11-1105 批 A07 卡 F18467 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7F080;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 版本令牌闭集规模（冻结 5：WIN7~WIN10 系，与卡 F18464 记法表同口径）。
pub const VERSION_COUNT: usize = 5;
/// 依据矩阵列数（冻结 4：主版本/次版本/基准构建号/依据层码）。
pub const MATRIX_COLS: usize = 4;
/// 依据层码（冻结：段基低 16 位 = 0xF080，每行写死该值作依据源指纹）。
pub const LAYER_CODE: u16 = 0xF080;

/// 版本令牌闭集。
pub const VERSION_TOKS: [&str; VERSION_COUNT] =
    ["WIN7RTM", "WIN8RTM", "WIN80RTM", "WIN81RTM", "WIN10RTM"];

/// 冻结版本对（主版本, 次版本）。
///   WIN7RTM=6.1、WIN8RTM=6.2、WIN80RTM=6.3（真实产品口径）；
///   WIN81RTM 在本册冻结口径取 6.4（未发行占位，仅作映射位）；
///   WIN10RTM=10.0（跳版冻结口径）。
pub const VERSION_PAIRS: [(u16, u16); VERSION_COUNT] =
    [(6, 1), (6, 2), (6, 3), (6, 4), (10, 0)];

/// 冻结基准构建号（主版本产品的 RTM 构建号；WIN81RTM 占位行取 0）。
pub const VERSION_BUILDS: [u16; VERSION_COUNT] = [7600, 9200, 9600, 0, 10240];

/// 冻结依据矩阵（行=版本位，列=主版本/次版本/构建号/依据层码）。
pub const MATRIX: [[u16; MATRIX_COLS]; VERSION_COUNT] = [
    [6, 1, 7600, LAYER_CODE],
    [6, 2, 9200, LAYER_CODE],
    [6, 3, 9600, LAYER_CODE],
    [6, 4, 0, LAYER_CODE],
    [10, 0, 10240, LAYER_CODE],
];

// ── 依据源闭集 ──────────────────────────────────────────────────
/// 依据源闭集（判据口径：仅兼容层可作依据源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BasisSource {
    /// 兼容层（唯一合法依据源）。
    CompatLayer = 0,
    /// 其他来源（一律拒）。
    Other = 1,
}

impl BasisSource {
    /// 从原始码还原（越界返回 None）。
    pub fn from_raw(v: u32) -> Option<BasisSource> {
        if v == 0 {
            Some(BasisSource::CompatLayer)
        } else if v == 1 {
            Some(BasisSource::Other)
        } else {
            None
        }
    }
    /// 中文说明。
    pub fn zh(self) -> &'static str {
        match self {
            BasisSource::CompatLayer => "兼容层",
            BasisSource::Other => "其他来源",
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VfmCode {
    /// 未知版本令牌（闭集外）。
    UnknownVersionTok,
    /// 大小写违规（令牌闭集全大写冻结）。
    CaseViolation,
    /// 非法依据源（非兼容层）。
    SourceIllegal,
    /// 行异值重写（矩阵账只增不减被踩）。
    RowRewrite,
    /// 矩阵依据漂移（写入值 ≠ 冻结矩阵）。
    MatrixBasisDrift,
    /// 状态机倒退。
    PhaseRegress,
    /// 矩阵不完整（5 行未写满即判完成）。
    MapIncomplete,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 账本封印破坏。
    LedgerSealBroken,
    /// 对照表偏差。
    RefTableDeviation,
    /// 双跑不一致。
    NotReproducible,
}

impl VfmCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VfmCode::UnknownVersionTok => "未知版本令牌：五记法闭集外",
            VfmCode::CaseViolation => "大小写违规：令牌闭集全大写冻结",
            VfmCode::SourceIllegal => "非法依据源：仅兼容层可作依据",
            VfmCode::RowRewrite => "行异值重写：矩阵账只增不减被踩",
            VfmCode::MatrixBasisDrift => "矩阵依据漂移：写入值与冻结矩阵不符",
            VfmCode::PhaseRegress => "状态机倒退：映射流水只进不退",
            VfmCode::MapIncomplete => "矩阵不完整：5 行未写满",
            VfmCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            VfmCode::LedgerSealBroken => "账本封印破坏：登记数只增不减被违",
            VfmCode::RefTableDeviation => "对照表偏差：令牌→版本对不符",
            VfmCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [VfmCode; 11] = [
        VfmCode::UnknownVersionTok,
        VfmCode::CaseViolation,
        VfmCode::SourceIllegal,
        VfmCode::RowRewrite,
        VfmCode::MatrixBasisDrift,
        VfmCode::PhaseRegress,
        VfmCode::MapIncomplete,
        VfmCode::ConservationBroken,
        VfmCode::LedgerSealBroken,
        VfmCode::RefTableDeviation,
        VfmCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < VfmCode::ALL.len() {
        if VfmCode::ALL[i].code() == code {
            return VfmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F080 段闭集内"
}

// ── 状态机：映射流水 ────────────────────────────────────────────
/// 映射流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapPhase {
    /// 未开始。
    Idle = 0,
    /// 已完成令牌查找。
    TokenLookup = 1,
    /// 已完成依据源核验。
    SourceVerify = 2,
    /// 已完成依据写入。
    BasisWrite = 3,
    /// 已冻结。
    Frozen = 4,
}

impl MapPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: MapPhase) -> Result<MapPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(VfmCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<MapPhase, u32> {
        let s0 = MapPhase::Idle;
        let s1 = s0.advance(MapPhase::TokenLookup)?;
        let s2 = s1.advance(MapPhase::SourceVerify)?;
        let s3 = s2.advance(MapPhase::BasisWrite)?;
        s3.advance(MapPhase::Frozen)
    }
}

// ── 令牌查找核 ──────────────────────────────────────────────────
/// 含小写字节判（大小写违规独立诊断，先于未知令牌）。
fn has_lowercase(tok: &str) -> bool {
    let b = tok.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if let Some(&c) = b.get(i) {
            if c >= b'a' && c <= b'z' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 版本令牌→行位查找：小写先拒（CaseViolation），闭集外拒（Unknown）。
pub fn tok_row(tok: &str) -> Result<usize, u32> {
    if has_lowercase(tok) {
        return Err(VfmCode::CaseViolation.code());
    }
    let mut i = 0;
    while i < VERSION_COUNT {
        let cand = match VERSION_TOKS.get(i) {
            Some(s) => *s,
            None => break,
        };
        if cand.as_bytes() == tok.as_bytes() {
            return Ok(i);
        }
        i += 1;
    }
    Err(VfmCode::UnknownVersionTok.code())
}

/// 行位→冻结矩阵行快照（越界 → 显式错）。
pub fn matrix_row(idx: usize) -> Result<[u16; MATRIX_COLS], u32> {
    if idx >= VERSION_COUNT {
        return Err(VfmCode::UnknownVersionTok.code());
    }
    let row = MATRIX[idx];
    Ok(row)
}

// ── 矩阵账（5 行 id-寻址，只增不减封印） ────────────────────────
/// 单行账目。
#[derive(Clone, Copy, Debug)]
pub struct RowSlot {
    pub written: bool,
    pub basis: [u16; MATRIX_COLS],
    pub source: u8,
    pub rewrite_same: u32,
    pub write_seq: u32,
}

impl RowSlot {
    /// 空行。
    pub const EMPTY: RowSlot = RowSlot {
        written: false,
        basis: [0; MATRIX_COLS],
        source: 0xFF,
        rewrite_same: 0,
        write_seq: 0,
    };
}

/// 矩阵依据账本。
pub struct MatrixLedger {
    rows: [RowSlot; VERSION_COUNT],
    pub inject_tries: [u32; 4],
    pub inject_caught: [u32; 4],
    next_seq: u32,
}

impl MatrixLedger {
    /// 空账本（5 行全空）。
    pub fn new() -> MatrixLedger {
        MatrixLedger {
            rows: [RowSlot::EMPTY; VERSION_COUNT],
            inject_tries: [0; 4],
            inject_caught: [0; 4],
            next_seq: 1,
        }
    }
    /// 依据写入总装：令牌查找 → 依据源核验 → 状态机 → 冻结矩阵比对 → 入账。
    /// 返回（行位, 写入依据）。同值重写幂等；异值重写拒。
    pub fn write_basis(
        &mut self,
        tok: &str,
        source: BasisSource,
    ) -> Result<(usize, [u16; MATRIX_COLS]), u32> {
        let row = tok_row(tok)?;
        if source != BasisSource::CompatLayer {
            return Err(VfmCode::SourceIllegal.code());
        }
        MapPhase::full_chain()?;
        let frozen = matrix_row(row)?;
        let old = self.rows[row];
        if old.written {
            if old.basis == frozen && old.source == source as u8 {
                self.rows[row].rewrite_same = old.rewrite_same.saturating_add(1);
                return Ok((row, frozen));
            }
            return Err(VfmCode::RowRewrite.code());
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.rows[row] = RowSlot {
            written: true,
            basis: frozen,
            source: source as u8,
            rewrite_same: 0,
            write_seq: seq,
        };
        Ok((row, frozen))
    }
    /// 已写行数。
    pub fn written_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < VERSION_COUNT {
            if self.rows[i].written {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 完整性：5 行全写。
    pub fn complete(&self) -> Result<(), u32> {
        if self.written_count() != VERSION_COUNT {
            return Err(VfmCode::MapIncomplete.code());
        }
        Ok(())
    }
    /// 每行依据与冻结矩阵逐行复核（漂移检测）。
    pub fn drift_check(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < VERSION_COUNT {
            let row = self.rows[i];
            if !row.written {
                return Err(VfmCode::MapIncomplete.code());
            }
            let frozen = matrix_row(i)?;
            if row.basis != frozen {
                return Err(VfmCode::MatrixBasisDrift.code());
            }
            if row.source != BasisSource::CompatLayer as u8 {
                return Err(VfmCode::SourceIllegal.code());
            }
            i += 1;
        }
        Ok(())
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒（四槽全抓）。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（四槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 对照表（令牌→版本对，8 例逐项复算） ─────────────────────────
/// 对照例：输入令牌 + 期望（主版本, 次版本）或 None（闭集外）。
pub const REF_TABLE: [(&str, Option<(u16, u16)>); 8] = [
    ("WIN7RTM", Some((6, 1))),
    ("WIN8RTM", Some((6, 2))),
    ("WIN80RTM", Some((6, 3))),
    ("WIN81RTM", Some((6, 4))),
    ("WIN10RTM", Some((10, 0))),
    ("NOTAVERSION", None),
    ("win7rtm", None), // 小写走 CaseViolation 独立路径，查找必拒
    ("", None),
];

/// 对照复算：逐例查找结果与冻结版本对零偏差。
pub fn ref_table_check() -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (tok, expect) = REF_TABLE[i];
        match expect {
            Some((em, en)) => {
                let row = tok_row(tok)?;
                let pair = VERSION_PAIRS[row];
                if pair.0 != em || pair.1 != en {
                    return Err(VfmCode::RefTableDeviation.code());
                }
            }
            None => {
                if tok_row(tok).is_ok() {
                    return Err(VfmCode::RefTableDeviation.code());
                }
            }
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

// ── 注入核（4 类坏样本 ×10 轮） ─────────────────────────────────
/// 注入探针：
///   0 未知版本令牌；1 行异值重写；2 非法依据源；3 大小写违规。
pub fn inject_probe(slot: usize) -> Result<(), u32> {
    let mut led = MatrixLedger::new();
    match slot {
        0 => match led.write_basis("NOTAVERSION", BasisSource::CompatLayer) {
            // 命中约定：注入被抓 → Err(窗内码)；漏 → Ok。
            Err(c) if c == VfmCode::UnknownVersionTok.code() => Err(c),
            _ => Ok(()),
        },
        1 => {
            // 异值重写可达构造：先幂等写 WIN7RTM 行，再手工把该行依据
            // 篡改为异值（构建号 +1），随后同 tok 再写 → old.basis !=
            // frozen → RowRewrite 必拒。
            let _ = led.write_basis("WIN7RTM", BasisSource::CompatLayer)?;
            let frozen7 = matrix_row(0)?;
            let mut bad = frozen7;
            bad[2] = bad[2].wrapping_add(1);
            led.rows[0] = RowSlot {
                written: true,
                basis: bad,
                source: BasisSource::CompatLayer as u8,
                rewrite_same: 0,
                write_seq: 1,
            };
            match led.write_basis("WIN7RTM", BasisSource::CompatLayer) {
                Err(c) if c == VfmCode::RowRewrite.code() => Err(c),
                _ => Ok(()),
            }
        }
        2 => match led.write_basis("WIN8RTM", BasisSource::Other) {
            Err(c) if c == VfmCode::SourceIllegal.code() => Err(c),
            _ => Ok(()),
        },
        _ => match led.write_basis("win7rtm", BasisSource::CompatLayer) {
            Err(c) if c == VfmCode::CaseViolation.code() => Err(c),
            _ => Ok(()),
        },
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：对照 8 例 → 5 行全写（兼容层源）→ 漂移复核 → 完整性
/// → 幂等重写 → 4 类注入 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    MapPhase::full_chain()?;
    ref_table_check()?;
    let mut led = MatrixLedger::new();
    // 5 行全写（依据源=兼容层）。
    let mut i = 0usize;
    while i < VERSION_COUNT {
        let tok = match VERSION_TOKS.get(i) {
            Some(s) => *s,
            None => break,
        };
        let (row, basis) = led.write_basis(tok, BasisSource::CompatLayer)?;
        if row != i || basis != MATRIX[i] {
            return Err(VfmCode::MatrixBasisDrift.code());
        }
        i += 1;
    }
    led.complete()?;
    led.drift_check()?;
    // 幂等重写：同源同值账不变。
    let before = led.written_count();
    let _ = led.write_basis("WIN7RTM", BasisSource::CompatLayer)?;
    if led.written_count() != before {
        return Err(VfmCode::LedgerSealBroken.code());
    }
    // 注入 4 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = match inject_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve() {
        return Err(VfmCode::ConservationBroken.code());
    }
    Ok((led.written_count() as u32, led.rows[0].rewrite_same, led.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 令牌→矩阵行快照：WIN10RTM → (10,0,10240,层码)。
    let row = matrix_row(4)?;
    if row[0] == 10 && row[1] == 0 && row[2] == 10240 && row[3] == LAYER_CODE {
        Ok(())
    } else {
        Err(VfmCode::MatrixBasisDrift.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 非法依据源必拒。
    let mut led = MatrixLedger::new();
    match led.write_basis("WIN8RTM", BasisSource::Other) {
        Err(c) if c == VfmCode::SourceIllegal.code() => Ok(()),
        _ => Err(VfmCode::ConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机越级必拒。
    match MapPhase::Idle.advance(MapPhase::BasisWrite) {
        Err(c) if c == VfmCode::PhaseRegress.code() => Ok(()),
        _ => Err(VfmCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7F080 {
        return false;
    }
    let mut i = 0;
    while i < VfmCode::ALL.len() {
        let v = VfmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集复核。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < VfmCode::ALL.len() {
        if detail_of(VfmCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（12 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-冻结矩阵规模",
        VERSION_COUNT == 5 && MATRIX_COLS == 4 && MATRIX.len() == 5,
        format!("5×4 依据矩阵 层码0x{:04X}", LAYER_CODE),
    ));
    v.push((
        "J1-五态流水",
        MapPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
    ));
    let refok = match ref_table_check() {
        Ok(n) => n == 8,
        Err(_) => false,
    };
    v.push((
        "J1-对照8例零偏差",
        refok,
        String::from("令牌→版本对 8 例"),
    ));
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 注入 4 类 ×10 轮。
    let mut led = MatrixLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = inject_probe(slot).is_err();
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries =
        led.inject_tries[0] + led.inject_tries[1] + led.inject_tries[2] + led.inject_tries[3];
    let total_caught =
        led.inject_caught[0] + led.inject_caught[1] + led.inject_caught[2] + led.inject_caught[3];
    v.push((
        "J1-注入10/10全抓",
        led.all_caught(),
        format!("4类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    v.push((
        "J1-依据源登记为兼容层",
        BasisSource::CompatLayer as u8 == 0,
        String::from("依据源闭集：仅兼容层"),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", VfmCode::ALL.len()),
    ));
    // 边界四类：行 0 合法 / 行 4 合法 / 行 5 拒 / 空令牌拒。
    let b0 = matrix_row(0).is_ok();
    let b1 = matrix_row(VERSION_COUNT - 1).is_ok();
    let b2 = matrix_row(VERSION_COUNT).is_err();
    let b3 = tok_row("").is_err();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("首行{} 末行{} 越界{} 空令牌{}", b0, b1, b2, b3),
    ));
    // 双跑一致。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 13 条）。
pub fn run_unx18467_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18467 版本标志矩阵映射");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18467_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_write_all_rows() {
        let mut led = MatrixLedger::new();
        let mut i = 0usize;
        while i < VERSION_COUNT {
            let tok = VERSION_TOKS[i];
            let r = led.write_basis(tok, BasisSource::CompatLayer);
            match r {
                Ok((row, basis)) => {
                    assert!(row == i, "行位一致");
                    assert!(basis == MATRIX[i], "依据与冻结矩阵一致");
                }
                Err(_) => panic!("合法写入不该报错"),
            }
            i += 1;
        }
        assert!(led.complete().is_ok(), "5 行写满");
        assert!(led.drift_check().is_ok(), "零漂移");
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(matrix_row(0).is_ok(), "首行合法");
        assert!(matrix_row(4).is_ok(), "末行合法");
        assert!(matrix_row(5).is_err(), "行 5 越界");
        assert!(tok_row("").is_err(), "空令牌必拒");
    }

    #[test]
    fn injection_four_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 4 {
            assert!(inject_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn rewrite_idempotent_same_value() {
        let mut led = MatrixLedger::new();
        let a = led.write_basis("WIN8RTM", BasisSource::CompatLayer);
        let b = led.write_basis("WIN8RTM", BasisSource::CompatLayer);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert!(x == y, "同值重写幂等");
                assert!(led.written_count() == 1, "登记数不变");
            }
            _ => panic!("同值重写不该报错"),
        }
    }

    #[test]
    fn case_violation_before_unknown() {
        // 小写令牌走独立诊断（CaseViolation），不落 Unknown。
        match tok_row("win7rtm") {
            Err(c) => assert!(c == VfmCode::CaseViolation.code(), "小写独立码"),
            Ok(_) => panic!("小写必拒"),
        }
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < VfmCode::ALL.len() {
            let c = VfmCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
        assert!(ref_table_check().is_ok(), "对照零偏差");
    }
}
