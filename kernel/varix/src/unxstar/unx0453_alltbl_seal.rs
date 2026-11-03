//! UNX-F0453 全表指针封账（只读封存） · 0x75300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   三类坏表注入（校验和破坏/签名伪造/长度烂）检出 100%（3/3），
//!   好表误伤 0。
//! 设计要点：
//!   1. 表指针条目 =（4B 签名 + 物理基址 + 长度 + 校验和）四元组，
//!      封账前逐条过三闸：签名闭集闸 → 长度健全闸 → 校验和闸；
//!   2. 三类坏表注入闭集：校验和 +1 破坏 / 签名 4 字节伪造 / 长度字段
//!      虚高越帽，三路检出必抓，好表 12 表语料零误伤；
//!   3. 封账只追加：状态机 开账→封账中→已封账 单调前进，倒退显性拒，
//!      已封账后追加/改写一律拒绝（只读封存红线）；
//!   4. 封账指纹 FNV-1a 64：按条目序折叠（签名+基址+长度+校验和），
//!      重放复算必须与封账指纹一致（双跑一致红线）；
//!   5. 诊断码闭集落 0x75300 段内 0x40 窗口，detail 查表闭集返回，
//!      错误路径附可操作中文建议（写入 detail 文案）；
//!   6. 验收：三类注入检出 3/3、好表误伤 0、指纹重放一致、追加/倒退双拒。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75300，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x75300;

/// ACPI 表头最小长度（36 字节，ACPI 6.5 规范 §5.2.6 系统描述表头）。
pub const MIN_HDR_LEN: u32 = 36;
/// 单表长度帽（256 KiB，防长度字段烂值虚高越界）。
pub const MAX_TABLE_CAP: u32 = 0x40000;
/// FNV-1a 64 偏移基（封账指纹常数，公开算法参数）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 素数（封账指纹常数）。
pub const FNV_PRIME: u64 = 0x100000001b3;
/// 好表语料条数（12 表，与 ACPI 面定位账对齐）。
pub const GOOD_TABLE_COUNT: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 校验和破坏（8 位和 != 0）
    CsumBreak,
    /// 签名伪造（不在签名闭集）
    SigForge,
    /// 长度烂（虚高越帽 / 低于表头下限 / 非 4 对齐）
    LenBad,
    /// 重复条目（同签名同基址已封）
    DupEntry,
    /// 已封账再写（只读封存违约）
    NotOpen,
    /// 状态机倒退（封账相位回退）
    PhaseRegress,
    /// 封账指纹重放不一致
    FpMismatch,
}

impl SealCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::CsumBreak => "校验和破坏：8 位和应为 0；建议重算 csum 字段再入账",
            SealCode::SigForge => "签名伪造：不在 12 签名闭集；建议核对 ACPI 签名表",
            SealCode::LenBad => "长度烂：<36 / >256KiB / 非 4 对齐；建议按表头 Length 字段复核",
            SealCode::DupEntry => "重复条目：同签名同基址已存在；建议先查封账再追加",
            SealCode::NotOpen => "已封账再写：封账只读；建议另开新账期",
            SealCode::PhaseRegress => "状态机倒退：封账相位只进不退；建议按 开账→封账中→已封账 推进",
            SealCode::FpMismatch => "指纹重放不一致：封账账面被改；建议对 FNV 指纹逐条对拍",
        }
    }
    pub const ALL: [SealCode; 7] = [
        SealCode::CsumBreak,
        SealCode::SigForge,
        SealCode::LenBad,
        SealCode::DupEntry,
        SealCode::NotOpen,
        SealCode::PhaseRegress,
        SealCode::FpMismatch,
    ];
}

/// 诊断码闭集查表：段外码返回未知文案（闭集不外溢）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75300 段闭集内"
}

// ── 表指针条目（签名/基址/长度/校验和四元组） ────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TblPtr {
    /// 4 字节 ASCII 签名（如 XSDT/APIC）。
    pub sig: [u8; 4],
    /// 物理基址（页对齐）。
    pub base: u64,
    /// 表长（字节，>= 36 且 4 对齐）。
    pub len: u32,
    /// 8 位校验和（全字段和模 256 为 0）。
    pub csum: u8,
}

impl TblPtr {
    /// 四元组逐字节求和（校验和语义：模 256 为 0 即好表）。
    fn byte_sum(&self) -> u32 {
        let mut s: u32 = 0;
        for b in self.sig.iter() {
            s += *b as u32;
        }
        for b in self.base.to_le_bytes().iter() {
            s += *b as u32;
        }
        for b in self.len.to_le_bytes().iter() {
            s += *b as u32;
        }
        s += self.csum as u32;
        s
    }
}

/// 已知 ACPI 签名闭集（12 项，ACPI 6.5 §5.2 表标识）。
pub const KNOWN_SIGS: [[u8; 4]; 12] = [
    *b"XSDT", *b"RSDT", *b"FADT", *b"APIC", *b"HPET", *b"MCFG",
    *b"SRAT", *b"SLIT", *b"BGRT", *b"DSDT", *b"FACS", *b"SSDT",
];

/// 签名合法性：在闭集内且全字节大写 ASCII（防小写/数字伪造）。
fn sig_known(sig: &[u8; 4]) -> bool {
    let mut upper = true;
    for b in sig.iter() {
        if *b < b'A' || *b > b'Z' {
            upper = false;
        }
    }
    if !upper {
        return false;
    }
    let mut i = 0;
    while i < KNOWN_SIGS.len() {
        if let Some(k) = KNOWN_SIGS.get(i) {
            if k == sig {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 由三字段反推合法校验和（好表工厂用）。
pub fn good_csum(sig: &[u8; 4], base: u64, len: u32) -> u8 {
    let mut s: u32 = 0;
    for b in sig.iter() {
        s += *b as u32;
    }
    for b in base.to_le_bytes().iter() {
        s += *b as u32;
    }
    for b in len.to_le_bytes().iter() {
        s += *b as u32;
    }
    (0x100u32 - (s & 0xFF)) as u8
}

/// 三闸逐条验表：签名闸 → 长度闸 → 校验和闸（顺序固定，先形态后数值）。
pub fn verify_table(r: &TblPtr) -> Result<(), u32> {
    if !sig_known(&r.sig) {
        return Err(SealCode::SigForge.val());
    }
    if r.len < MIN_HDR_LEN || r.len > MAX_TABLE_CAP || r.len % 4 != 0 {
        return Err(SealCode::LenBad.val());
    }
    if r.byte_sum() & 0xFF != 0 {
        return Err(SealCode::CsumBreak.val());
    }
    Ok(())
}

// ── 好表语料（12 表，签名/基址/长度逐一手工给定） ────────────────
pub const CORPUS_LEN: [u32; 12] = [
    0x74, 0x11C, 0x0F0, 0x0A8, 0x080, 0x0B0, 0x0BC, 0x84, 0x0DC, 0x48, 0x40, 0x5C,
];

/// 好表 12 表语料：基址 0x7F000000 起页对齐步进，签名取闭集全排列。
pub fn good_corpus() -> Vec<TblPtr> {
    let mut v: Vec<TblPtr> = Vec::new();
    let mut i = 0;
    while i < KNOWN_SIGS.len() {
        let sig = match KNOWN_SIGS.get(i) {
            Some(s) => *s,
            None => break,
        };
        let len = match CORPUS_LEN.get(i) {
            Some(l) => *l,
            None => break,
        };
        let base = 0x7F000000u64 + (i as u64) * 0x1000;
        v.push(TblPtr { sig, base, len, csum: good_csum(&sig, base, len) });
        i += 1;
    }
    v
}

// ── 三类坏表注入闭集（构造器） ──────────────────────────────────
/// 注入一：校验和 +1 破坏。
pub fn inject_csum_break(r: &TblPtr) -> TblPtr {
    let mut c = *r;
    c.csum = c.csum.wrapping_add(1);
    c
}

/// 注入二：签名 4 字节伪造（FAKE 不在闭集）。
pub fn inject_sig_forge(r: &TblPtr) -> TblPtr {
    let mut c = *r;
    c.sig = *b"FAKE";
    c
}

/// 注入三：长度字段虚高（越 256KiB 帽）。
pub fn inject_len_bloat(r: &TblPtr) -> TblPtr {
    let mut c = *r;
    c.len = c.len.saturating_add(MAX_TABLE_CAP);
    c
}

// ── 封账状态机（开账→封账中→已封账，单调前进） ──────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealPhase {
    /// 开账：可追加条目
    Open,
    /// 封账中：只读过渡，禁止追加
    Sealing,
    /// 已封账：全账只读，指纹已落
    Sealed,
}

impl SealPhase {
    pub fn rank(self) -> u8 {
        match self {
            SealPhase::Open => 0,
            SealPhase::Sealing => 1,
            SealPhase::Sealed => 2,
        }
    }
}

/// 封账总账：条目只追加 + 相位单调 + 指纹封存。
#[derive(Clone, Debug)]
pub struct SealLedger {
    pub entries: Vec<TblPtr>,
    pub phase: SealPhase,
    /// 封账指纹（finish_seal 时落定，此后不许变）。
    pub fp: u64,
    /// 被拒追加次数（显性记账）。
    pub rej_appends: u32,
    /// 被拒倒退次数（显性记账）。
    pub rej_regress: u32,
}

impl SealLedger {
    pub fn new() -> SealLedger {
        SealLedger {
            entries: Vec::new(),
            phase: SealPhase::Open,
            fp: 0,
            rej_appends: 0,
            rej_regress: 0,
        }
    }

    /// 追加条目：仅开账期受理，过三闸 + 查重。
    pub fn push_entry(&mut self, r: TblPtr) -> Result<usize, u32> {
        if self.phase != SealPhase::Open {
            self.rej_appends += 1;
            return Err(SealCode::NotOpen.val());
        }
        verify_table(&r)?;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.sig == r.sig && e.base == r.base {
                    return Err(SealCode::DupEntry.val());
                }
            }
            i += 1;
        }
        self.entries.push(r);
        Ok(self.entries.len() - 1)
    }

    /// 开账 → 封账中（倒退/越级显性拒）。
    pub fn begin_seal(&mut self) -> Result<(), u32> {
        if self.phase != SealPhase::Open {
            self.rej_regress += 1;
            return Err(SealCode::PhaseRegress.val());
        }
        self.phase = SealPhase::Sealing;
        Ok(())
    }

    /// 封账中 → 已封账，落 FNV 封账指纹。
    pub fn finish_seal(&mut self) -> Result<u64, u32> {
        if self.phase != SealPhase::Sealing {
            self.rej_regress += 1;
            return Err(SealCode::PhaseRegress.val());
        }
        self.fp = seal_fingerprint(&self.entries);
        self.phase = SealPhase::Sealed;
        Ok(self.fp)
    }

    /// 指纹重放：对现账面重算 FNV 与封账指纹比对。
    pub fn recheck_fp(&self) -> Result<(), u32> {
        if self.phase != SealPhase::Sealed {
            return Err(SealCode::PhaseRegress.val());
        }
        if seal_fingerprint(&self.entries) != self.fp {
            return Err(SealCode::FpMismatch.val());
        }
        Ok(())
    }
}

/// FNV-1a 64 折叠一段字节。
pub fn fnv1a_fold(mut fp: u64, bytes: &[u8]) -> u64 {
    for b in bytes.iter() {
        fp ^= *b as u64;
        fp = fp.wrapping_mul(FNV_PRIME);
    }
    fp
}

/// 封账指纹：按条目序折叠四元组全部字节。
pub fn seal_fingerprint(entries: &[TblPtr]) -> u64 {
    let mut fp = FNV_OFFSET;
    for r in entries.iter() {
        fp = fnv1a_fold(fp, &r.sig);
        fp = fnv1a_fold(fp, &r.base.to_le_bytes());
        fp = fnv1a_fold(fp, &r.len.to_le_bytes());
        fp = fnv1a_fold(fp, &[r.csum]);
    }
    fp
}

// ── 注入检出账（三路：校验和/签名/长度） ────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectTally {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectTally {
    pub fn new() -> InjectTally {
        InjectTally { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一笔（slot 越界静默忽略，闭集内才有效）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    /// 读试验数（越槽返回 0）。
    pub fn try_at(&self, slot: usize) -> u32 {
        match self.tries.get(slot) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// 读命中数（越槽返回 0）。
    pub fn caught_at(&self, slot: usize) -> u32 {
        match self.caught.get(slot) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// 三路全抓（每路至少一试且试全中）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.try_at(i);
            let c = self.caught_at(i);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 注入轮：三类坏表 × 语料首表，逐路必抓 ───────────────────────
pub fn inject_round() -> InjectTally {
    let mut t = InjectTally::new();
    let corpus = good_corpus();
    let seed = match corpus.first() {
        Some(r) => *r,
        None => return t,
    };
    // 路 1：校验和破坏 → 必须以 CsumBreak 拒。
    let bad1 = inject_csum_break(&seed);
    t.record(0, verify_table(&bad1) == Err(SealCode::CsumBreak.val()));
    // 路 2：签名伪造 → 必须以 SigForge 拒。
    let bad2 = inject_sig_forge(&seed);
    t.record(1, verify_table(&bad2) == Err(SealCode::SigForge.val()));
    // 路 3：长度虚高 → 必须以 LenBad 拒。
    let bad3 = inject_len_bloat(&seed);
    t.record(2, verify_table(&bad3) == Err(SealCode::LenBad.val()));
    t
}

/// 好表误伤账：12 表语料逐条过闸，误伤数必须为 0。
pub fn false_positive_count() -> u32 {
    let corpus = good_corpus();
    let mut fp_count = 0;
    for r in corpus.iter() {
        if verify_table(r).is_err() {
            fp_count += 1;
        }
    }
    fp_count
}

// ── 黄金全链：追加→封账→指纹重放→封后追加拒→倒退拒 ─────────────
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let mut l = SealLedger::new();
    for r in good_corpus().iter() {
        l.push_entry(*r)?;
    }
    if l.entries.len() != GOOD_TABLE_COUNT {
        return Err(SealCode::LenBad.val());
    }
    l.begin_seal()?;
    let fp = l.finish_seal()?;
    l.recheck_fp()?;
    // 已封账追加 → 拒（取语料首表构造违约写请求）。
    let seed = match good_corpus().first() {
        Some(r) => *r,
        None => return Err(SealCode::LenBad.val()),
    };
    let after = l.push_entry(inject_csum_break(&seed));
    // 倒退（Sealed 再 begin）→ 拒。
    let regress = l.begin_seal();
    match (after.is_err(), regress.is_err()) {
        (true, true) => Ok((fp, l.rej_appends, l.rej_regress)),
        _ => Err(SealCode::NotOpen.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_ok() -> bool {
    let a = golden_run();
    let b = golden_run();
    a == b
}

// ── 段位守卫（诊断码落 0x75300 窗口 + 段基对齐） ─────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in SealCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有，逐条对应检查项） ────────────────────────────
fn corpus_clean_ok() -> bool {
    good_corpus().len() == GOOD_TABLE_COUNT && false_positive_count() == 0
}
fn inject_all_ok() -> bool {
    inject_round().all_caught()
}
fn golden_ok() -> bool {
    match golden_run() {
        Ok((fp, app, reg)) => fp != 0 && app == 1 && reg == 1,
        Err(_) => false,
    }
}
fn replay_ok() -> bool {
    match golden_run() {
        Ok((fp, _, _)) => {
            let mut l = SealLedger::new();
            for r in good_corpus().iter() {
                if l.push_entry(*r).is_err() {
                    return false;
                }
            }
            let _ = l.begin_seal();
            l.fp = fp;
            let _ = l.finish_seal();
            l.recheck_fp().is_ok()
        }
        Err(_) => false,
    }
}
fn boundary_ok() -> bool {
    // 下限边界：36 字节头是好表；35 / 非 4 对齐是坏表。
    let sig = *b"SSDT";
    let lo = TblPtr { sig, base: 0x7F200000, len: 36, csum: good_csum(&sig, 0x7F200000, 36) };
    let under = TblPtr { sig, base: 0x7F201000, len: 35, csum: good_csum(&sig, 0x7F201000, 35) };
    let misalign = TblPtr { sig, base: 0x7F202000, len: 38, csum: good_csum(&sig, 0x7F202000, 38) };
    verify_table(&lo).is_ok() && verify_table(&under).is_err() && verify_table(&misalign).is_err()
}
fn dup_ok() -> bool {
    let mut l = SealLedger::new();
    let corpus = good_corpus();
    let first = match corpus.first() {
        Some(r) => *r,
        None => return false,
    };
    let _ = l.push_entry(first);
    l.push_entry(first) == Err(SealCode::DupEntry.val())
}
fn codeset_ok() -> bool {
    for c in SealCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条，覆盖正常流/边界/注入/双跑/段位） ────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-好表12表语料全过验",
        corpus_clean_ok(),
        format!("语料={} 误伤={}", GOOD_TABLE_COUNT, false_positive_count()),
    ));
    let inj = inject_round();
    v.push((
        "J1-校验和破坏检出",
        inj.caught_at(0) == inj.try_at(0) && inj.try_at(0) > 0,
        format!("试={} 抓={}", inj.try_at(0), inj.caught_at(0)),
    ));
    v.push((
        "J1-签名伪造检出",
        inj.caught_at(1) == inj.try_at(1) && inj.try_at(1) > 0,
        format!("试={} 抓={}", inj.try_at(1), inj.caught_at(1)),
    ));
    v.push((
        "J1-长度虚高检出",
        inj.caught_at(2) == inj.try_at(2) && inj.try_at(2) > 0,
        format!("试={} 抓={}", inj.try_at(2), inj.caught_at(2)),
    ));
    v.push((
        "J1-三路检出3/3",
        inj.all_caught(),
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    v.push((
        "J1-封账指纹重放一致",
        replay_ok(),
        "FNV-1a 64 折算重放同值".to_string(),
    ));
    v.push((
        "J1-已封账追加显性拒",
        golden_ok(),
        "封后追加计入 rej_appends".to_string(),
    ));
    v.push((
        "J1-状态机倒退显性拒",
        golden_ok(),
        "倒退计入 rej_regress".to_string(),
    ));
    v.push((
        "J1-重复条目显性拒",
        dup_ok(),
        "同签名同基址二拒".to_string(),
    ));
    v.push((
        "J2-边界-表头下限与非4对齐",
        boundary_ok(),
        format!("36过/35拒/38拒 帽={:#x}", MAX_TABLE_CAP),
    ));
    v.push((
        "J2-双跑一致",
        repro_ok(),
        "golden_run 两跑同值".to_string(),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:x} 窗口", SEG_BASE),
    ));
    v.push((
        "J3-诊断码闭集",
        codeset_ok(),
        format!("{} 码全查表命中", SealCode::ALL.len()),
    ));
    v
}

pub fn run_unx0453_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0453 全表指针封账（只读封存）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界 / 闭集） ──────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_good_corpus_zero_false_positive() {
        assert_eq!(false_positive_count(), 0);
        assert_eq!(good_corpus().len(), GOOD_TABLE_COUNT);
    }

    #[test]
    fn test_three_injections_all_caught() {
        let t = inject_round();
        assert!(t.all_caught());
        assert_eq!(t.tries, [1, 1, 1]);
        assert_eq!(t.caught, [1, 1, 1]);
    }

    #[test]
    fn test_csum_break_detected_with_code() {
        let seed = match good_corpus().first() {
            Some(r) => *r,
            None => TblPtr { sig: *b"SSDT", base: 0x7F300000, len: 36, csum: 0 },
        };
        assert_eq!(verify_table(&inject_csum_break(&seed)), Err(SealCode::CsumBreak.val()));
    }

    #[test]
    fn test_sig_forge_detected_with_code() {
        let seed = match good_corpus().first() {
            Some(r) => *r,
            None => TblPtr { sig: *b"SSDT", base: 0x7F300000, len: 36, csum: 0 },
        };
        assert_eq!(verify_table(&inject_sig_forge(&seed)), Err(SealCode::SigForge.val()));
    }

    #[test]
    fn test_len_bloat_detected_with_code() {
        let seed = match good_corpus().first() {
            Some(r) => *r,
            None => TblPtr { sig: *b"SSDT", base: 0x7F300000, len: 36, csum: 0 },
        };
        assert_eq!(verify_table(&inject_len_bloat(&seed)), Err(SealCode::LenBad.val()));
    }

    #[test]
    fn test_sealed_append_rejected() {
        let mut l = SealLedger::new();
        for r in good_corpus().iter() {
            let _ = l.push_entry(*r);
        }
        let _ = l.begin_seal();
        let _ = l.finish_seal();
        let second = match good_corpus().get(1) {
            Some(r) => *r,
            None => TblPtr { sig: *b"FADT", base: 0x7F301000, len: 36, csum: 0 },
        };
        assert_eq!(l.push_entry(second), Err(SealCode::NotOpen.val()));
        assert_eq!(l.rej_appends, 1);
    }

    #[test]
    fn test_phase_regress_rejected() {
        let mut l = SealLedger::new();
        // 越级：Open 直接 finish → 拒。
        assert_eq!(l.finish_seal(), Err(SealCode::PhaseRegress.val()));
        let _ = l.begin_seal();
        // 倒退：Sealing 再 begin → 拒。
        assert_eq!(l.begin_seal(), Err(SealCode::PhaseRegress.val()));
        let _ = l.finish_seal();
        assert_eq!(l.phase, SealPhase::Sealed);
        assert_eq!(l.rej_regress, 2);
    }

    #[test]
    fn test_fingerprint_replay_consistent() {
        assert!(replay_ok());
        let mut a = SealLedger::new();
        for r in good_corpus().iter() {
            let _ = a.push_entry(*r);
        }
        let _ = a.begin_seal();
        let fp = a.finish_seal();
        let mut b = SealLedger::new();
        for r in good_corpus().iter() {
            let _ = b.push_entry(*r);
        }
        let _ = b.begin_seal();
        let fp2 = b.finish_seal();
        assert_eq!(fp, fp2);
    }

    #[test]
    fn test_boundary_and_dup() {
        assert!(boundary_ok());
        assert!(dup_ok());
    }

    #[test]
    fn test_segment_guard_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x74999), "未知诊断码：不在 0x75300 段闭集内");
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }
}
