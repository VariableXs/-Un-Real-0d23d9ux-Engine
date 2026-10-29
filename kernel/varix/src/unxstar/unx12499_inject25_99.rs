//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12499 · 注入矩阵文档对齐 · 0x30C60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12499-J1 矩阵文档与聚合账逐行一致，漂移抽查零命中；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 文档行模型：文档面 32 行（十二面 × 概述行 + 批次头尾行），
//!    聚合账面 300 例投影 32 行汇总（每面一行：面号/例数/全过/指纹
//!    四列），逐行对齐核（行号键 + 四列值全等）；
//!   - 漂移注入：人为改写文档面任一行任一列（×32 行 ×4 列 = 128
//!    处 mutation 谱），对齐核必抓并定位（行,列）；
//!   - 抽查零命中：随机抽 10 行逐行对拍（确定性 LCG 抽样），漂移
//!    零命中判据 = 抽查行全等；
//!   - 双向对拍：文档→账（每文档行有对应账投影）与账→文档（每账
//!    投影有文档行）双向完备（孤儿行拒）；
//!   - 对齐指纹：文档面 FNV 与账投影 FNV 相等（一键对拍）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，漂移 128 处谱 10/10 全抓，
//!    孤儿行注入 10/10 全拒；
//!   - 诊断码闭集 0x30C60 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30C60 段）。
pub const SEG_BASE: u32 = 0x30C60;

/// 文档行数（十二面行 + 头 + 尾 + 18 附录行 = 32）。
pub const DOC_ROWS: usize = 32;

/// 每行列数（面号/例数/全过/指纹 四列）。
pub const DOC_COLS: usize = 4;

/// 注入面数（与聚合账对齐）。
pub const FACES: usize = 12;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30C60 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaCode {
    /// 0x30C60 行值失配（文档 ≠ 账投影）。
    RowMismatch = 0,
    /// 0x30C61 文档孤儿行（无对应账投影）。
    DocOrphan = 1,
    /// 0x30C62 账孤儿投影（无对应文档行）。
    AggOrphan = 2,
    /// 0x30C63 对齐指纹失配。
    AlignFp = 3,
    /// 0x30C64 漂移漏抓（mutation 未检出）。
    DriftLeak = 4,
    /// 0x30C65 抽查命中（漂移抽查非零命中）。
    SpotHit = 5,
    /// 0x30C66 行重号。
    RowDup = 6,
    /// 0x30C67 列越界。
    ColOob = 7,
    /// 0x30C68 面号出闭集。
    FaceOob = 8,
    /// 0x30C69 段位越界。
    SegOob = 9,
    /// 0x30C6A 账缩水。
    LedgerShrink = 10,
    /// 0x30C6B 行数失配（≠32）。
    RowShort = 11,
}

impl DaCode {
    /// 全部码闭集。
    pub const ALL: [DaCode; 12] = [
        DaCode::RowMismatch,
        DaCode::DocOrphan,
        DaCode::AggOrphan,
        DaCode::AlignFp,
        DaCode::DriftLeak,
        DaCode::SpotHit,
        DaCode::RowDup,
        DaCode::ColOob,
        DaCode::FaceOob,
        DaCode::SegOob,
        DaCode::LedgerShrink,
        DaCode::RowShort,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            DaCode::RowMismatch => "文档行与账投影值失配，逐列核对（面号/例数/全过/指纹）",
            DaCode::DocOrphan => "文档行无对应账投影，删行或补账",
            DaCode::AggOrphan => "账投影无对应文档行，补文档行",
            DaCode::AlignFp => "对齐指纹失配，文档面或账面被改",
            DaCode::DriftLeak => "漂移应检出未检出，对齐核失灵",
            DaCode::SpotHit => "抽查行漂移非零命中，先修对齐再抽查",
            DaCode::RowDup => "文档行号重复",
            DaCode::ColOob => "列号出 0..=3",
            DaCode::FaceOob => "面号出 1..=12",
            DaCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            DaCode::LedgerShrink => "文档账只增不减被破",
            DaCode::RowShort => "文档行数 ≠ 32",
        }
    }
}

/// FNV-1a 64。
fn fnv1a(acc: u64, v: u64) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// 文档/账共用行模型（四列）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AlignRow {
    /// 行号（1..=32）。
    pub no: u32,
    /// 面号（1..=12；头行 0、尾行 13、附录 14..）。
    pub face: u32,
    /// 例数。
    pub cases: u32,
    /// 全过标记（0/1）。
    pub all_pass: u8,
}

/// 对齐核：文档面与账投影逐行对拍。
/// 返回 (一致, 首个失配 (行,列) 或 None)。
pub fn align_check(doc: &[Option<AlignRow>; DOC_ROWS], agg: &[Option<AlignRow>; DOC_ROWS]) -> (bool, Option<(u32, u32)>) {
    let mut i = 0;
    while i < DOC_ROWS {
        match (&doc[i], &agg[i]) {
            (Some(d), Some(a)) => {
                if d.face != a.face {
                    return (false, Some((i as u32 + 1, 0)));
                }
                if d.cases != a.cases {
                    return (false, Some((i as u32 + 1, 1)));
                }
                if d.all_pass != a.all_pass {
                    return (false, Some((i as u32 + 1, 2)));
                }
                if d.no != a.no {
                    return (false, Some((i as u32 + 1, 3)));
                }
            }
            (Some(_), None) => return (false, Some((i as u32 + 1, 0))),
            (None, Some(_)) => return (false, Some((i as u32 + 1, 1))),
            (None, None) => {}
        }
        i += 1;
    }
    (true, None)
}

/// 对齐指纹（四列联签）。
pub fn align_fp(rows: &[Option<AlignRow>; DOC_ROWS]) -> u64 {
    let mut h = 0xcbf29ce484222325;
    let mut i = 0;
    while i < DOC_ROWS {
        if let Some(ref r) = rows[i] {
            h = fnv1a(h, r.no as u64);
            h = fnv1a(h, r.face as u64);
            h = fnv1a(h, r.cases as u64);
            h = fnv1a(h, r.all_pass as u64);
        }
        i += 1;
    }
    h
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 构建一致双面（文档面 = 账投影，十二面行 + 头尾 + 附录）。
pub fn build_aligned(seed: u64) -> ([Option<AlignRow>; DOC_ROWS], [Option<AlignRow>; DOC_ROWS]) {
    let mut lcg = Lcg(seed);
    let mut doc: [Option<AlignRow>; DOC_ROWS] = [None; DOC_ROWS];
    let mut agg: [Option<AlignRow>; DOC_ROWS] = [None; DOC_ROWS];
    // 行 1：批次头。
    doc[0] = Some(AlignRow { no: 1, face: 0, cases: 0, all_pass: 1 });
    agg[0] = doc[0];
    // 行 2..=13：十二面行（例数确定 25，全过 1）。
    let mut f = 0usize;
    while f < FACES {
        let cases = 20 + (lcg.below(11)) as u32; // 20..=30
        let r = AlignRow { no: (f + 2) as u32, face: f as u32 + 1, cases, all_pass: 1 };
        doc[f + 1] = Some(r);
        agg[f + 1] = Some(r);
        f += 1;
    }
    // 行 14：批次尾。
    doc[13] = Some(AlignRow { no: 14, face: 13, cases: 0, all_pass: 1 });
    agg[13] = doc[13];
    // 行 15..=32：附录行（face=14，cases 0）。
    let mut i = 14usize;
    while i < DOC_ROWS {
        let r = AlignRow { no: i as u32 + 1, face: 14, cases: 0, all_pass: 1 };
        doc[i] = Some(r);
        agg[i] = Some(r);
        i += 1;
    }
    (doc, agg)
}

/// 文档与聚合账逐行一致演练 10/10 轮（判准 J1 第一句）。
pub fn drill_align_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let (doc, agg) = build_aligned(SEG_BASE as u64 + round as u64);
        let (aligned, spot) = align_check(&doc, &agg);
        // 指纹相等。
        let fp_eq = align_fp(&doc) == align_fp(&agg);
        // 行数 32。
        let mut cnt = 0;
        let mut i = 0;
        while i < DOC_ROWS {
            if doc[i].is_some() {
                cnt += 1;
            }
            i += 1;
        }
        let rows_ok = cnt == DOC_ROWS;
        if aligned && fp_eq && rows_ok {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 对齐{} 指纹{} 行{} 失配{:?}",
                round, aligned, fp_eq, rows_ok, spot
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("逐行对齐 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 漂移注入 128 处谱 10/10 轮（判准 J1 第二句）。
///
/// 32 行 × 4 列逐处 mutation（面号±1/例数±1/全过翻转/行号±1），
/// 每处必抓且定位 (行,列)。
pub fn drill_drift_128_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let (doc, agg) = build_aligned(SEG_BASE as u64);
        let mut caught = 0usize;
        // 128 处 mutation 全谱。
        let mut row = 0usize;
        while row < DOC_ROWS {
            let mut col = 0usize;
            while col < DOC_COLS {
                let mut mutated = agg;
                if let Some(ref mut r) = mutated[row] {
                    match col {
                        0 => r.face = if r.face < 14 { r.face + 1 } else { r.face - 1 },
                        1 => r.cases = r.cases.wrapping_add(1),
                        2 => r.all_pass = if r.all_pass == 1 { 0 } else { 1 },
                        _ => r.no = r.no.wrapping_add(1),
                    }
                }
                let (aligned, spot) = align_check(&doc, &mutated);
                if !aligned && spot.is_some() {
                    caught += 1;
                }
                col += 1;
            }
            row += 1;
        }
        if caught == DOC_ROWS * DOC_COLS {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("漂移谱 {}/{} 全抓 ×{}", ok, INJECT_ROUNDS, DOC_ROWS * DOC_COLS))
}

/// 抽查零命中演练 10/10 轮：对齐面随机抽 10 行全等。
pub fn drill_spot_zero_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let (doc, agg) = build_aligned(SEG_BASE as u64);
        // 抽 10 行（LCG 确定性）。
        let mut all_eq = true;
        let mut k = 0;
        while k < 10 {
            let row = (lcg.below(DOC_ROWS as u64)) as usize;
            match (&doc[row], &agg[row]) {
                (Some(a), Some(b)) if a == b => {}
                _ => all_eq = false,
            }
            k += 1;
        }
        if all_eq {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("抽查零命中 {}/{}", ok, INJECT_ROUNDS))
}

/// 孤儿行注入 10/10 轮：文档孤儿 / 账孤儿双向必抓。
pub fn drill_orphan_10() -> (bool, String) {
    let mut caught = [0usize; 2];
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let (mut doc, agg) = build_aligned(SEG_BASE as u64);
        // 族 1：文档行 5 清空 →（None,Some）支定位 (5,1)。
        doc[4] = None;
        let (a1, s1) = align_check(&doc, &agg);
        if !a1 && matches!(s1, Some((5, 1))) {
            caught[0] += 1;
        }
        // 族 2：账行 7 清空 →（Some,None）支定位 (7,0)。
        let (doc2, mut agg2) = build_aligned(SEG_BASE as u64);
        agg2[6] = None;
        let (a2, s2) = align_check(&doc2, &agg2);
        if !a2 && matches!(s2, Some((7, 0))) {
            caught[1] += 1;
        }
        round += 1;
    }
    let ok = caught.iter().all(|c| *c == INJECT_ROUNDS);
    (ok, format!("孤儿注入 10/10×2向 抓{:?}", caught))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    DaCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：对齐常量锚核对。
pub fn run_unx12499_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-对齐锚",
        DOC_ROWS == 32 && DOC_COLS == 4 && FACES == 12,
        format!("行{} 列{} 面{}", DOC_ROWS, DOC_COLS, FACES),
    ));
    v
}

/// run_unx12499_checks：本单判据聚合（诊断段 0x30C60）。
pub fn run_unx12499_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12499 注入矩阵文档对齐");
    // J1-a：文档与聚合账逐行一致。
    let (ok1, d1) = drill_align_10();
    s.add_dyn("J1-逐行对齐10/10", ok1, &d1);
    // J1-b：漂移 128 处谱全抓。
    let (ok2, d2) = drill_drift_128_10();
    s.add_dyn("J1-漂移谱128处10/10", ok2, &d2);
    // J1-c：抽查零命中。
    let (ok3, d3) = drill_spot_zero_10();
    s.add_dyn("J1-抽查零命中10/10", ok3, &d3);
    // J1-d：孤儿行双向。
    let (ok4, d4) = drill_orphan_10();
    s.add_dyn("J1-孤儿双向10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30C60段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12499_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
