//! UNX-F0454 坏表注入演练（校验破坏检出） · 0x75340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   三类坏表注入检出 100%，好表误伤 0，演练账只追加不抹账。
//! 设计要点：
//!   1. 表模型走字节像路线（与 F0453 的字段四元组路线互为印证）：
//!      表 = 字节序列，头布局 签名@0..4 / 长度@4..8(LE) / 校验和@9；
//!   2. 坏表三路注入闭集：校验和字节 +1 / 签名 4 字节改写 FAKE /
//!      长度字段虚高 0x10000，三路对 12 表语料打满 36 格矩阵全抓；
//!   3. 检出判据从严：不仅要求拒收，还要求拒收码与注入类型一一对应
//!      （错码不算抓），好表干净路误伤必须为 0；
//!   4. 演练账只追加不抹账：无删除通道，抹账请求显性拒并计数，
//!      批次号单调递增，倒退显性拒；
//!   5. 漏检注入必显：账内混入一笔 caught=false，汇总判定立即破绿；
//!   6. 双跑一致红线：同语料两轮演练账逐笔相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75340，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x75340;

/// ACPI 表头最小长度（36 字节，ACPI 6.5 §5.2.6）。
pub const MIN_HDR: u32 = 36;
/// 单表长度帽（与 F0453 同值冻结，防长度烂值）。
pub const LEN_CAP: u32 = 0x40000;
/// 语料表数（12 表）。
pub const CORPUS_TABLES: usize = 12;
/// 演练矩阵格数（3 类 × 12 表）。
pub const MATRIX_CELLS: usize = 36;
/// 校验和字节在头内的偏移（ACPI 头第 10 字节，下标 9）。
pub const CSUM_OFF: usize = 9;
/// 长度字段在头内的偏移（下标 4，4 字节 LE）。
pub const LEN_OFF: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillCode {
    /// 校验和破坏（全图像字节和模 256 != 0）
    CsumBreak,
    /// 签名伪造（不在签名闭集）
    SigForge,
    /// 长度烂（长度字段与字节像实际长不符 / 越帽）
    LenBad,
    /// 批次倒退（演练批次号回退）
    BatchRegress,
    /// 抹账请求（只追加红线）
    NoErase,
    /// 空账判绿（没打满矩阵就报全抓）
    EmptyGreen,
    /// 双跑不一致
    NotReproducible,
}

impl DrillCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DrillCode::CsumBreak => "校验和破坏：全像字节和模 256 应为 0；建议重算下标 9 校验字节",
            DrillCode::SigForge => "签名伪造：签名不在闭集；建议比对 ACPI 签名表逐字节核对",
            DrillCode::LenBad => "长度烂：头内长度与像长不符或越 256KiB 帽；建议重读 Length 字段",
            DrillCode::BatchRegress => "批次倒退：演练批次只增不减；建议新批次取 last+1",
            DrillCode::NoErase => "抹账被拒：演练账只追加；建议以更正批追加冲正而非删除",
            DrillCode::EmptyGreen => "空账判绿：矩阵未打满不得报全抓；建议先补齐 36 格再汇总",
            DrillCode::NotReproducible => "双跑不一致：同语料两轮账不等；建议冻结语料与注入序",
        }
    }
    pub const ALL: [DrillCode; 7] = [
        DrillCode::CsumBreak,
        DrillCode::SigForge,
        DrillCode::LenBad,
        DrillCode::BatchRegress,
        DrillCode::NoErase,
        DrillCode::EmptyGreen,
        DrillCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段外码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DrillCode::ALL.len() {
        if let Some(c) = DrillCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75340 段闭集内"
}

// ── 表字节像（真实 ACPI 头布局建模） ────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableImg {
    pub bytes: Vec<u8>,
}

/// 已知签名闭集（12 项）。
pub const KNOWN_SIGS: [[u8; 4]; 12] = [
    *b"XSDT", *b"RSDT", *b"FADT", *b"APIC", *b"HPET", *b"MCFG",
    *b"SRAT", *b"SLIT", *b"BGRT", *b"DSDT", *b"FACS", *b"SSDT",
];

/// 全像字节和（u32 累加后取模，避免中间截断误差）。
fn img_sum(bytes: &[u8]) -> u32 {
    let mut s: u32 = 0;
    for b in bytes.iter() {
        s += *b as u32;
    }
    s
}

/// 由签名/表体大小造好表字节像：体内容按位置斜坡填，校验字节封平和。
pub fn build_good_image(sig: &[u8; 4], extra: u32) -> TableImg {
    let total = (MIN_HDR + extra) as usize;
    let mut bytes: Vec<u8> = Vec::new();
    let mut i: usize = 0;
    while i < total {
        // 斜坡体：0x41 起循环步进，保证确定性（无随机）。
        bytes.push((0x41 + (i as u32 % 26)) as u8);
        i += 1;
    }
    // 写签名。
    let mut k = 0;
    while k < 4 {
        if let Some(slot) = bytes.get_mut(k) {
            if let Some(sb) = sig.get(k) {
                *slot = *sb;
            }
        }
        k += 1;
    }
    // 写长度（LE 4 字节）。
    let lb = (total as u32).to_le_bytes();
    let mut k = 0;
    while k < 4 {
        if let Some(slot) = bytes.get_mut(LEN_OFF + k) {
            if let Some(sb) = lb.get(k) {
                *slot = *sb;
            }
        }
        k += 1;
    }
    // 固定修订号 2 与 OEM 保留位；校验字节先清零再封平和（ACPI 语义）。
    if let Some(slot) = bytes.get_mut(8) {
        *slot = 2;
    }
    if let Some(slot) = bytes.get_mut(CSUM_OFF) {
        *slot = 0;
    }
    let sum_wo = img_sum(&bytes) & 0xFF;
    if let Some(slot) = bytes.get_mut(CSUM_OFF) {
        *slot = (0x100u32 - sum_wo) as u8;
    }
    TableImg { bytes }
}

/// 读头内长度字段（越界返回 None）。
pub fn read_len_field(img: &TableImg) -> Option<u32> {
    let mut v: u32 = 0;
    let mut k = 0;
    while k < 4 {
        let b = match img.bytes.get(LEN_OFF + k) {
            Some(x) => *x,
            None => return None,
        };
        v |= (b as u32) << (8 * k);
        k += 1;
    }
    Some(v)
}

/// 读签名（不足 4 字节返回 None）。
pub fn read_sig(img: &TableImg) -> Option<[u8; 4]> {
    let mut sig: [u8; 4] = [0; 4];
    let mut k = 0;
    while k < 4 {
        let b = match img.bytes.get(k) {
            Some(x) => *x,
            None => return None,
        };
        match sig.get_mut(k) {
            Some(slot) => *slot = b,
            None => return None,
        }
        k += 1;
    }
    Some(sig)
}

fn sig_known(sig: &[u8; 4]) -> bool {
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

/// 演练验表（从严三闸）：签名闭集 → 长度自洽 → 全像平和。
pub fn drill_verify(img: &TableImg) -> Result<(), u32> {
    let sig = match read_sig(img) {
        Some(s) => s,
        None => return Err(DrillCode::SigForge.val()),
    };
    if !sig_known(&sig) {
        return Err(DrillCode::SigForge.val());
    }
    let actual = img.bytes.len() as u32;
    let claimed = match read_len_field(img) {
        Some(v) => v,
        None => return Err(DrillCode::LenBad.val()),
    };
    if claimed != actual || claimed < MIN_HDR || claimed > LEN_CAP || claimed % 4 != 0 {
        return Err(DrillCode::LenBad.val());
    }
    if img_sum(&img.bytes) & 0xFF != 0 {
        return Err(DrillCode::CsumBreak.val());
    }
    Ok(())
}

// ── 注入类型闭集与坏表构造器 ────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillKind {
    /// 校验和字节 +1
    Csum,
    /// 签名 4 字节改 FAKE
    Sig,
    /// 长度字段虚高 0x10000
    Len,
}

impl DrillKind {
    pub fn slot(self) -> usize {
        match self {
            DrillKind::Csum => 0,
            DrillKind::Sig => 1,
            DrillKind::Len => 2,
        }
    }
    pub fn expect_code(self) -> u32 {
        match self {
            DrillKind::Csum => DrillCode::CsumBreak.val(),
            DrillKind::Sig => DrillCode::SigForge.val(),
            DrillKind::Len => DrillCode::LenBad.val(),
        }
    }
    pub const ALL: [DrillKind; 3] = [DrillKind::Csum, DrillKind::Sig, DrillKind::Len];
}

/// 注入一：校验和字节 +1（wrapping，保证恒变）。
pub fn corrupt_csum(img: &TableImg) -> TableImg {
    let mut c = img.clone();
    if let Some(slot) = c.bytes.get_mut(CSUM_OFF) {
        *slot = slot.wrapping_add(1);
    }
    c
}

/// 注入二：签名 4 字节改写 FAKE。
pub fn forge_sig(img: &TableImg) -> TableImg {
    let mut c = img.clone();
    let fake = *b"FAKE";
    let mut k = 0;
    while k < 4 {
        if let Some(slot) = c.bytes.get_mut(k) {
            if let Some(fb) = fake.get(k) {
                *slot = *fb;
            }
        }
        k += 1;
    }
    c
}

/// 注入三：长度字段虚高 0x10000（与像长不符，撞长度闸）。
pub fn bloat_len(img: &TableImg) -> TableImg {
    let mut c = img.clone();
    let claimed = read_len_field(&c).unwrap_or(0);
    let nb = claimed.wrapping_add(0x10000).to_le_bytes();
    let mut k = 0;
    while k < 4 {
        if let Some(slot) = c.bytes.get_mut(LEN_OFF + k) {
            if let Some(sb) = nb.get(k) {
                *slot = *sb;
            }
        }
        k += 1;
    }
    c
}

/// 按类型注入（分派器）。
pub fn inject(kind: DrillKind, img: &TableImg) -> TableImg {
    match kind {
        DrillKind::Csum => corrupt_csum(img),
        DrillKind::Sig => forge_sig(img),
        DrillKind::Len => bloat_len(img),
    }
}

// ── 演练账（只追加不抹账，批次单调） ────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillRec {
    /// 批次号（全局单调递增，从 1 起）。
    pub batch: u32,
    /// 语料表号（0..12）。
    pub tbl: usize,
    /// 注入类型。
    pub kind: DrillKind,
    /// 是否从严命中（错误码与类型一一对应才算）。
    pub caught: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillLedger {
    pub recs: Vec<DrillRec>,
    /// 已用最大批次号（单调水位）。
    pub last_batch: u32,
    /// 抹账请求次数（显性记账，全部拒绝）。
    pub erase_attempts: u32,
}

impl DrillLedger {
    pub fn new() -> DrillLedger {
        DrillLedger { recs: Vec::new(), last_batch: 0, erase_attempts: 0 }
    }

    /// 追加演练笔：批次必须严格大于水位（只进不退）。
    pub fn append(&mut self, batch: u32, tbl: usize, kind: DrillKind, caught: bool) -> Result<(), u32> {
        if batch == 0 || batch <= self.last_batch {
            return Err(DrillCode::BatchRegress.val());
        }
        if tbl >= CORPUS_TABLES {
            return Err(DrillCode::LenBad.val());
        }
        self.recs.push(DrillRec { batch, tbl, kind, caught });
        self.last_batch = batch;
        Ok(())
    }

    /// 抹账请求：一律拒绝并计数（只追加红线，显性不静默）。
    pub fn try_erase(&mut self, _batch: u32) -> Result<(), u32> {
        self.erase_attempts += 1;
        Err(DrillCode::NoErase.val())
    }

    /// 命中格数。
    pub fn caught_cells(&self) -> usize {
        let mut n = 0;
        for r in self.recs.iter() {
            if r.caught {
                n += 1;
            }
        }
        n
    }

    /// 全抓判定：账满 36 格且笔笔命中（空账不认绿）。
    pub fn all_caught(&self) -> bool {
        if self.recs.len() != MATRIX_CELLS {
            return false;
        }
        for r in self.recs.iter() {
            if !r.caught {
                return false;
            }
        }
        true
    }
}

// ── 矩阵演练：3 类注入 × 12 表语料 = 36 格 ──────────────────────
pub fn run_matrix() -> DrillLedger {
    let mut led = DrillLedger::new();
    let mut batch: u32 = 0;
    for kind in DrillKind::ALL {
        let mut t = 0;
        while t < CORPUS_TABLES {
            // 语料长度：36 起按表号斜坡（全 4 对齐），保证差异覆盖。
            let extra = (t as u32) * 8;
            let good = build_good_image(match KNOWN_SIGS.get(t) {
                Some(s) => s,
                None => break,
            }, extra);
            let bad = inject(kind, &good);
            let hit = match drill_verify(&bad) {
                Err(e) => e == kind.expect_code(),
                Ok(()) => false,
            };
            batch += 1;
            let _ = led.append(batch, t, kind, hit);
            t += 1;
        }
    }
    led
}

/// 干净路：12 好图逐条过闸，误伤计数（必须为 0）。
pub fn clean_pass_false_positives() -> u32 {
    let mut n = 0;
    let mut t = 0;
    while t < CORPUS_TABLES {
        let sig = match KNOWN_SIGS.get(t) {
            Some(s) => *s,
            None => break,
        };
        let good = build_good_image(&sig, (t as u32) * 8);
        if drill_verify(&good).is_err() {
            n += 1;
        }
        t += 1;
    }
    n
}

/// 分路命中数（slot 0/1/2 对应 Csum/Sig/Len）。
pub fn kind_caught(led: &DrillLedger, slot: usize) -> u32 {
    let mut n = 0;
    for r in led.recs.iter() {
        if r.kind.slot() == slot && r.caught {
            n += 1;
        }
    }
    n
}

/// 漏检注入：把账内首笔翻成未命中，全抓判定必须破绿。
pub fn inject_miss(led: &DrillLedger) -> DrillLedger {
    let mut c = led.clone();
    let mut i = 0;
    while i < c.recs.len() {
        if let Some(r) = c.recs.get_mut(i) {
            if r.caught {
                r.caught = false;
                break;
            }
        }
        i += 1;
    }
    c
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_ok() -> bool {
    run_matrix() == run_matrix()
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in DrillCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn matrix_ok() -> bool {
    let led = run_matrix();
    led.all_caught() && led.caught_cells() == MATRIX_CELLS
}
fn per_kind_ok() -> bool {
    let led = run_matrix();
    let mut i = 0;
    while i < 3 {
        if kind_caught(&led, i) != CORPUS_TABLES as u32 {
            return false;
        }
        i += 1;
    }
    true
}
fn clean_ok() -> bool {
    clean_pass_false_positives() == 0
}
fn append_only_ok() -> bool {
    let mut led = run_matrix();
    let before = led.recs.len();
    let e1 = led.try_erase(1);
    let e2 = led.try_erase(2);
    e1 == Err(DrillCode::NoErase.val()) && e2 == Err(DrillCode::NoErase.val())
        && led.erase_attempts == 2 && led.recs.len() == before
}
fn batch_monotonic_ok() -> bool {
    let mut led = DrillLedger::new();
    let g = build_good_image(b"SSDT", 0);
    let _ = led.append(1, 0, DrillKind::Csum, true);
    // 同批重放 → 拒。
    let same = led.append(1, 1, DrillKind::Sig, true);
    // 倒退 → 拒。
    let back = led.append(0, 1, DrillKind::Len, true);
    // 水位不因拒笔后退。
    same == Err(DrillCode::BatchRegress.val())
        && back == Err(DrillCode::BatchRegress.val())
        && led.last_batch == 1
        && drill_verify(&g).is_ok()
}
fn miss_injection_ok() -> bool {
    let led = run_matrix();
    if !led.all_caught() {
        return false;
    }
    !inject_miss(&led).all_caught()
}
fn boundary_ok() -> bool {
    // 最小头 36 过验；虚报长度 32（< 36）拒。
    let good = build_good_image(b"HPET", 0);
    let mut under = good.clone();
    let nb = 32u32.to_le_bytes();
    let mut k = 0;
    while k < 4 {
        if let Some(slot) = under.bytes.get_mut(LEN_OFF + k) {
            if let Some(b) = nb.get(k) {
                *slot = *b;
            }
        }
        k += 1;
    }
    drill_verify(&good).is_ok() && drill_verify(&under) == Err(DrillCode::LenBad.val())
}
fn codeset_ok() -> bool {
    for c in DrillCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let led = run_matrix();
    v.push((
        "J1-矩阵36格全抓",
        matrix_ok(),
        format!("格={}/{} 抓={}", led.recs.len(), MATRIX_CELLS, led.caught_cells()),
    ));
    v.push((
        "J1-校验路12/12",
        kind_caught(&led, DrillKind::Csum.slot()) == CORPUS_TABLES as u32,
        format!("抓={}", kind_caught(&led, DrillKind::Csum.slot())),
    ));
    v.push((
        "J1-签名路12/12",
        kind_caught(&led, DrillKind::Sig.slot()) == CORPUS_TABLES as u32,
        format!("抓={}", kind_caught(&led, DrillKind::Sig.slot())),
    ));
    v.push((
        "J1-长度路12/12",
        kind_caught(&led, DrillKind::Len.slot()) == CORPUS_TABLES as u32,
        format!("抓={}", kind_caught(&led, DrillKind::Len.slot())),
    ));
    v.push((
        "J1-分路从严命中汇总",
        per_kind_ok(),
        "错码不算抓，三路各 12".to_string(),
    ));
    v.push((
        "J1-好表误伤0",
        clean_ok(),
        format!("误伤={}", clean_pass_false_positives()),
    ));
    v.push((
        "J1-演练账只追加不抹账",
        append_only_ok(),
        format!("抹账请求={}", led.erase_attempts + 2),
    ));
    v.push((
        "J1-批次单调倒退拒",
        batch_monotonic_ok(),
        "同批重放/倒批双拒".to_string(),
    ));
    v.push((
        "J1-漏检注入必显",
        miss_injection_ok(),
        "翻一笔未命中即破绿".to_string(),
    ));
    v.push((
        "J2-边界-最小头与虚报短长",
        boundary_ok(),
        format!("36 过 / 32 拒 帽={:#x}", LEN_CAP),
    ));
    v.push((
        "J2-双跑一致",
        repro_ok(),
        "run_matrix 两跑账逐笔相等".to_string(),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:x} 窗口", SEG_BASE),
    ));
    v.push((
        "J3-诊断码闭集",
        codeset_ok(),
        format!("{} 码全查表命中", DrillCode::ALL.len()),
    ));
    v
}

pub fn run_unx0454_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0454 坏表注入演练（校验破坏检出）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_good_image_verifies() {
        let g = build_good_image(b"APIC", 24);
        assert!(drill_verify(&g).is_ok());
        assert_eq!(g.bytes.len(), (MIN_HDR + 24) as usize);
    }

    #[test]
    fn test_matrix_36_all_caught() {
        let led = run_matrix();
        assert!(led.all_caught());
        assert_eq!(led.recs.len(), MATRIX_CELLS);
        assert_eq!(led.caught_cells(), MATRIX_CELLS);
    }

    #[test]
    fn test_each_kind_hits_own_code() {
        let g = build_good_image(b"XSDT", 8);
        assert_eq!(drill_verify(&corrupt_csum(&g)), Err(DrillCode::CsumBreak.val()));
        assert_eq!(drill_verify(&forge_sig(&g)), Err(DrillCode::SigForge.val()));
        assert_eq!(drill_verify(&bloat_len(&g)), Err(DrillCode::LenBad.val()));
    }

    #[test]
    fn test_clean_pass_zero_false_positive() {
        assert_eq!(clean_pass_false_positives(), 0);
    }

    #[test]
    fn test_no_erase_channel() {
        let mut led = run_matrix();
        let n = led.recs.len();
        assert_eq!(led.try_erase(3), Err(DrillCode::NoErase.val()));
        assert_eq!(led.erase_attempts, 1);
        assert_eq!(led.recs.len(), n);
    }

    #[test]
    fn test_batch_monotonic() {
        let mut led = DrillLedger::new();
        assert!(led.append(1, 0, DrillKind::Csum, true).is_ok());
        assert_eq!(led.append(1, 1, DrillKind::Sig, true), Err(DrillCode::BatchRegress.val()));
        assert_eq!(led.append(0, 1, DrillKind::Len, true), Err(DrillCode::BatchRegress.val()));
        assert!(led.append(2, 1, DrillKind::Sig, true).is_ok());
        assert_eq!(led.last_batch, 2);
    }

    #[test]
    fn test_miss_injection_breaks_green() {
        let led = run_matrix();
        assert!(led.all_caught());
        let tampered = inject_miss(&led);
        assert!(!tampered.all_caught());
        assert_eq!(tampered.caught_cells(), MATRIX_CELLS - 1);
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        assert_eq!(run_matrix(), run_matrix());
    }

    #[test]
    fn test_boundary_under_len() {
        assert!(boundary_ok());
    }

    #[test]
    fn test_segment_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x753FF), "未知诊断码：不在 0x75340 段闭集内");
    }
}
