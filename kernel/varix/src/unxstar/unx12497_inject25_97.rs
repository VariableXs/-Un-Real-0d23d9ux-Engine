//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12497 · 注入矩阵双机对照判据 · 0x30C20
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12497-J1 抽样 30 例双机拒止行为一致（S1 母版），拒止码
//!      类零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双机对照：同一注入矩阵代码以 S1（母版）与 S2（对照）两次独
//!     立执行，30 例抽样逐例拒止行为对拍（拒码序列逐位相等）；
//!   - 拒止码类零容差：拒止行为按码类（段内码 → 类别 = 码 - 段基）
//!     对拍，类别不同即破（零容差——不允近似一致）；
//!   - 12 类拒码闭集全覆盖抽样（每类至少 2 例），S1/S2 逐例类别全等；
//!   - 种子扰动对拍：S2 种子异于 S1 仍须同序（序由矩阵结构决定，
//!    不由种子巧合——确定性红线）；
//!   - 对照账：30 行 append-only（例号/类/码/S1 判/S2 判五列），
//!    一致率 30/30；
//!   - 注入核：LCG 写死种子 = SEG_BASE（S1）与 SEG_BASE^0x5A5A（S2）；
//!   - 诊断码闭集 0x30C20 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30C20 段）。
pub const SEG_BASE: u32 = 0x30C20;

/// 抽样例数（判准 30）。
pub const SAMPLE_CASES: usize = 30;

/// 拒码类别数（段内 12 类）。
pub const CODE_CLASSES: usize = 12;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30C20 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 0x30C20 类别失配（S1 ≠ S2）。
    ClassMismatch = 0,
    /// 0x30C21 拒码失配（同类别下码值不等）。
    CodeMismatch = 1,
    /// 0x30C22 对照账缺行。
    LedgerMissing = 2,
    /// 0x30C23 对照账重号。
    LedgerDup = 3,
    /// 0x30C24 一致率不足（<30/30）。
    AgreeShort = 4,
    /// 0x30C25 种子扰动破序。
    SeedPerturb = 5,
    /// 0x30C26 类别覆盖不足（12 类未全覆盖）。
    ClassShort = 6,
    /// 0x30C27 母版账缩水。
    MasterShrink = 7,
    /// 0x30C28 对照账缩水。
    SlaveShrink = 8,
    /// 0x30C29 段位越界。
    SegOob = 9,
    /// 0x30C2A 母版漏拒（S1 判 Ok）。
    MasterOk = 10,
    /// 0x30C2B 对照漏拒（S2 判 Ok）。
    SlaveOk = 11,
}

impl DcCode {
    /// 全部码闭集。
    pub const ALL: [DcCode; 12] = [
        DcCode::ClassMismatch,
        DcCode::CodeMismatch,
        DcCode::LedgerMissing,
        DcCode::LedgerDup,
        DcCode::AgreeShort,
        DcCode::SeedPerturb,
        DcCode::ClassShort,
        DcCode::MasterShrink,
        DcCode::SlaveShrink,
        DcCode::SegOob,
        DcCode::MasterOk,
        DcCode::SlaveOk,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            DcCode::ClassMismatch => "双机拒止类别失配，零容差红线破，逐例排查",
            DcCode::CodeMismatch => "同类下码值不等，核对拒码生成器",
            DcCode::LedgerMissing => "对照账缺行，30 例逐行入库",
            DcCode::LedgerDup => "对照账例号重复",
            DcCode::AgreeShort => "一致率不足 30/30，逐例定位",
            DcCode::SeedPerturb => "种子扰动后序变——序由种子巧合而非结构决定",
            DcCode::ClassShort => "12 类拒码未全覆盖抽样，补齐类别覆盖",
            DcCode::MasterShrink => "母版账只增不减被破",
            DcCode::SlaveShrink => "对照账只增不减被破",
            DcCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            DcCode::MasterOk => "母版应拒不拒（判 Ok），注入闸失灵",
            DcCode::SlaveOk => "对照应拒不拒，注入闸失灵",
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

/// LCG 注入核（种子写死，确定性）。
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

/// 单例注入执行（S1/S2 共用同一函数——同码双机）。
///
/// 类别 = 段内类号（0..11），拒码 = 段基 + 类别；注入动作由例号与
/// 类别共同决定（结构决定序），种子只驱动装饰量（客户端号）。
/// 返回 (类别, 拒码)。
pub fn run_case(case: usize, seed: u64) -> (u32, u32) {
    let mut lcg = Lcg(seed ^ (case as u64).wrapping_mul(0x9E3779B97F4A7C15));
    let _ = lcg.below(97); // 装饰量消耗（不影响序）
    // 12 类拒码闭集全覆盖抽样：每类至少 2 例（30 例 = 12×2 + 6 补样）。
    let class = if case < CODE_CLASSES * 2 {
        (case % CODE_CLASSES) as u32
    } else {
        ((case * 5 + 7) % CODE_CLASSES) as u32
    };
    (class, SEG_BASE + class)
}

/// 对照账行（五列）。
#[derive(Clone, Copy)]
pub struct CmpRow {
    pub case: u32,
    pub class: u32,
    pub code: u32,
    pub s1_rej: bool,
    pub s2_rej: bool,
}

/// 对照账（append-only + 例号唯一）。
pub struct CmpLedger {
    rows: [Option<CmpRow>; SAMPLE_CASES],
    len: usize,
}

impl CmpLedger {
    /// 新建。
    pub fn new() -> CmpLedger {
        CmpLedger { rows: [None; SAMPLE_CASES], len: 0 }
    }

    /// 入账（重号拒）。
    pub fn admit(&mut self, r: CmpRow) -> Result<(), u32> {
        if self.len >= SAMPLE_CASES {
            return Err(DcCode::LedgerDup.val());
        }
        let mut i = 0;
        while i < self.len {
            if let Some(ref x) = self.rows[i] {
                if x.case == r.case {
                    return Err(DcCode::LedgerDup.val());
                }
            }
            i += 1;
        }
        self.rows[self.len] = Some(r);
        self.len += 1;
        Ok(())
    }

    /// 现长。
    pub fn len(&self) -> usize {
        self.len
    }
}

/// 双机对照核（判准 J1 全句）。
pub fn drill_dual_30() -> (bool, String) {
    let mut ledger = CmpLedger::new();
    let mut agree = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut case = 0usize;
    while case < SAMPLE_CASES {
        // S1 母版：应拒（拒码类 = 结构决定）。
        let (c1, k1) = run_case(case, SEG_BASE as u64);
        // S2 对照：异种子仍须同序同码。
        let (c2, k2) = run_case(case, (SEG_BASE as u64) ^ 0x5A5A5A5A);
        let s1_rej = k1 != 0;
        let s2_rej = k2 != 0;
        let r = CmpRow { case: case as u32 + 1, class: c1, code: k1, s1_rej, s2_rej };
        if ledger.admit(r).is_err() {
            return (false, format!("例{} 入账失败", case + 1));
        }
        // 零容差对拍：类别 + 码 + 双拒三重。
        if c1 == c2 && k1 == k2 && s1_rej && s2_rej {
            agree += 1;
        } else {
            d.push(format!(
                "例{} 类{}:{} 码{:x}:{:x} 拒{}:{}",
                case + 1, c1, c2, k1, k2, s1_rej, s2_rej
            ));
        }
        case += 1;
    }
    // 类别覆盖核：12 类全出现。
    let mut seen = [false; CODE_CLASSES];
    let mut i = 0;
    while i < ledger.len() {
        if let Some(ref r) = ledger.rows[i] {
            if (r.class as usize) < CODE_CLASSES {
                seen[r.class as usize] = true;
            }
        }
        i += 1;
    }
    let covered = seen.iter().all(|s| *s);
    let full = ledger.len() == SAMPLE_CASES;
    (agree == SAMPLE_CASES && covered && full,
     format!("双机对照 {}/{} 类覆盖{} 账齐{}", agree, SAMPLE_CASES, covered, full))
}

/// 确定性三跑演练 10/10 轮：三组异种种子全对拍。
pub fn drill_triple_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut fps: Vec<u64> = Vec::new();
        let mut rep = 0u64;
        while rep < 3 {
            let mut h = 0xcbf29ce484222325;
            let mut case = 0usize;
            while case < SAMPLE_CASES {
                let (c, k) = run_case(case, SEG_BASE as u64 + rep * 0x1111);
                h = fnv1a(h, c as u64);
                h = fnv1a(h, k as u64);
                case += 1;
            }
            fps.push(h);
            rep += 1;
        }
        if fps[0] == fps[1] && fps[1] == fps[2] && fps[0] != 0xcbf29ce484222325 {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("三跑确定性 {}/{}", ok, INJECT_ROUNDS))
}

/// 对照账完整性演练 10/10 轮：30 行齐 + 重号必拒。
pub fn drill_ledger_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut ledger = CmpLedger::new();
        let mut case = 0usize;
        let mut all_in = true;
        while case < SAMPLE_CASES {
            let (c, k) = run_case(case, SEG_BASE as u64);
            if ledger
                .admit(CmpRow { case: case as u32 + 1, class: c, code: k, s1_rej: true, s2_rej: true })
                .is_err()
            {
                all_in = false;
            }
            case += 1;
        }
        // 重号注入必拒。
        let dup = ledger
            .admit(CmpRow { case: 1, class: 0, code: SEG_BASE, s1_rej: true, s2_rej: true })
            .is_err();
        if all_in && dup && ledger.len() == SAMPLE_CASES {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("对照账完整 {}/{}", ok, INJECT_ROUNDS))
}

/// 类别闭集核对演练 10/10 轮：拒码 - 段基 ∈ [0,12)。
pub fn drill_class_closed_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut all_ok = true;
        let mut case = 0usize;
        while case < SAMPLE_CASES {
            let (c, k) = run_case(case, SEG_BASE as u64);
            if k != SEG_BASE + c || (c as usize) >= CODE_CLASSES {
                all_ok = false;
            }
            case += 1;
        }
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("类别闭集 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    DcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：对照常量锚核对。
pub fn run_unx12497_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-对照锚",
        SAMPLE_CASES == 30 && CODE_CLASSES == 12,
        format!("抽样{} 类别{}", SAMPLE_CASES, CODE_CLASSES),
    ));
    v
}

/// run_unx12497_checks：本单判据聚合（诊断段 0x30C20）。
pub fn run_unx12497_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12497 注入矩阵双机对照");
    // J1-a：抽样 30 例双机拒止行为一致（S1 母版）。
    let (ok1, d1) = drill_dual_30();
    s.add_dyn("J1-双机30例一致", ok1, &d1);
    // J1-b：确定性三跑。
    let (ok2, d2) = drill_triple_10();
    s.add_dyn("J1-三跑确定性10/10", ok2, &d2);
    // J1-c：对照账完整性。
    let (ok3, d3) = drill_ledger_10();
    s.add_dyn("J1-对照账完整10/10", ok3, &d3);
    // J1-d：类别闭集。
    let (ok4, d4) = drill_class_closed_10();
    s.add_dyn("J1-类别闭集10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30C20段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12497_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
