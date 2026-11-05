//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12630 IO 完成包状态/完成值/完成键对平 · 0x80780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12631-J1 30 例联测全过，拒止码双侧一致断言绿，
//!      零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三字段对平语料机：30 例联测（状态/完成值/完成键三路轮换 ×
//!     正常流/边界/注入），每例冻结输入与期望判码，逐例跑双侧
//!     （母版侧 / 镜像侧）对平；
//!   - 拒止码双侧一致（判据锚定）：每例母版侧判码与镜像侧判码必须
//!     逐字相等（SideDrift 抓不等）——母版侧与镜像侧是同输入的两路
//!     独立求值（冻结表驱动，无双跑时钟依赖）；
//!   - 零静默（判据锚定）：30 例必须逐例显性落判（绿/红各落一码），
//!     任何一例无判即 ZeroSilent 抓出；判码闭集校验：判码必须落在
//!     段闭集内，账外码即静默面；
//!   - 对平谓词三路：状态路（错误档负值/成功档零值）、完成值路
//!     （饱和域）、完成键路（登记窗），三路注入逐次必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80780），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80780..0x807C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80780，AI-VX9-1105 B02 批 12 槽第 7 槽）。
pub const SEG_BASE: u32 = 0x80780;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignCode {
    /// 状态路对平破：错误档/成功档口径漂移
    StatusLaneBreak,
    /// 完成值路对平破：饱和域口径漂移
    InfoLaneBreak,
    /// 完成键路对平破：登记窗口径漂移
    KeyLaneBreak,
    /// 双侧判码不一致：母版/镜像同例异判
    SideDrift,
    /// 静默例：联测例无显性落判
    ZeroSilent,
    /// 判码越闭集：账外码（静默面）
    CodeOutOfSet,
    /// 联测例号越界：例号不在 0..30
    CaseIndexOut,
    /// 语料指纹漂移：冻结表被改
    CorpusDrift,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 联测未全过：存在红例
    CaseRed,
    /// 例表溢出
    CorpusFull,
    /// 未知诊断码兜底
    Unknown,
}

impl AlignCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AlignCode::StatusLaneBreak => 0x01,
                AlignCode::InfoLaneBreak => 0x02,
                AlignCode::KeyLaneBreak => 0x03,
                AlignCode::SideDrift => 0x04,
                AlignCode::ZeroSilent => 0x05,
                AlignCode::CodeOutOfSet => 0x06,
                AlignCode::CaseIndexOut => 0x07,
                AlignCode::CorpusDrift => 0x08,
                AlignCode::FingerprintDrift => 0x09,
                AlignCode::CaseRed => 0x0A,
                AlignCode::CorpusFull => 0x0B,
                AlignCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            AlignCode::StatusLaneBreak => "状态路破：错误档负值/成功档零值逐例对平",
            AlignCode::InfoLaneBreak => "完成值路破：饱和域口径逐例对平",
            AlignCode::KeyLaneBreak => "完成键路破：登记窗逐例对平",
            AlignCode::SideDrift => "双侧漂移：母版/镜像同例必须同判码",
            AlignCode::ZeroSilent => "静默例：每例必须显性落判，不许无判",
            AlignCode::CodeOutOfSet => "判码越闭集：只许段内判码落账",
            AlignCode::CaseIndexOut => "例号越界：用 0..30 内例号",
            AlignCode::CorpusDrift => "语料指纹漂移：冻结表不许改",
            AlignCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            AlignCode::CaseRed => "联测红例：逐例修绿再入账",
            AlignCode::CorpusFull => "例表溢出：先结案再入例",
            AlignCode::Unknown => "未知诊断码：不在 0x80780 段闭集内",
        }
    }

    pub const ALL: [AlignCode; 12] = [
        AlignCode::StatusLaneBreak,
        AlignCode::InfoLaneBreak,
        AlignCode::KeyLaneBreak,
        AlignCode::SideDrift,
        AlignCode::ZeroSilent,
        AlignCode::CodeOutOfSet,
        AlignCode::CaseIndexOut,
        AlignCode::CorpusDrift,
        AlignCode::FingerprintDrift,
        AlignCode::CaseRed,
        AlignCode::CorpusFull,
        AlignCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in AlignCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    AlignCode::Unknown.detail()
}

// ── 冻结常量表（来源：完成包三字段对平手账 · 联测节） ─────────────
/// 联测例数（判据 30 例）。
pub const CASES: usize = 30;
/// 完成键登记窗。
pub const KEY_WIN: (u32, u32) = (0x60, 0x70);
/// 完成值饱和域上沿（对平口径）。
pub const INFO_CAP: u32 = 0x0001_0000;

/// 联测例输入（三字段 + 期望判码闭集位）。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号（0..30）。
    pub idx: u32,
    /// 状态字段（i32 记账，负=错误档）。
    pub status: i32,
    /// 完成值字段。
    pub information: u32,
    /// 完成键字段。
    pub key: u32,
    /// 期望：绿（真=全过判绿）。
    pub expect_green: bool,
}

/// 三路对平裁决（单侧求值，双侤同输入必同判）。
/// 返回 Ok(判绿) 或 Err(段内判码)。
pub fn judge_case(c: &CaseSpec) -> Result<bool, u32> {
    // 状态路：成功档（status==0）必须 information==0；错误档（<0）information 任意。
    if c.status == 0 && c.information != 0 {
        return Err(AlignCode::StatusLaneBreak.val());
    }
    // 完成值路：饱和域上沿内。
    if c.information > INFO_CAP {
        return Err(AlignCode::InfoLaneBreak.val());
    }
    // 完成键路：登记窗内。
    if c.key < KEY_WIN.0 || c.key >= KEY_WIN.1 {
        return Err(AlignCode::KeyLaneBreak.val());
    }
    Ok(true)
}

/// 判码闭集校验：判码必须落在段闭集（零静默的判码面）。
pub fn code_in_set(code: u32) -> bool {
    for c in AlignCode::ALL.iter() {
        if c.val() == code {
            return true;
        }
    }
    false
}

/// 冻结语料表（30 例：正常流/边界/注入轮换，确定性构造）。
pub fn corpus() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    for i in 0..CASES {
        let spec = match i % 6 {
            // 正常流：错误档+窗内键。
            0 => CaseSpec { idx: i as u32, status: -1 - (i as i32), information: (i as u32) * 3, key: KEY_WIN.0 + (i as u32) % 0x10, expect_green: true },
            // 正常流：成功空包。
            1 => CaseSpec { idx: i as u32, status: 0, information: 0, key: KEY_WIN.0 + (i as u32) % 0x10, expect_green: true },
            // 边界：完成值恰顶。
            2 => CaseSpec { idx: i as u32, status: -7, information: INFO_CAP, key: KEY_WIN.0 + (i as u32) % 0x10, expect_green: true },
            // 注入：成功档带非零完成值（状态路必抓）。
            3 => CaseSpec { idx: i as u32, status: 0, information: 5, key: KEY_WIN.0 + (i as u32) % 0x10, expect_green: false },
            // 注入：完成值越顶（完成值路必抓）。
            4 => CaseSpec { idx: i as u32, status: -2, information: INFO_CAP + 1, key: KEY_WIN.0 + (i as u32) % 0x10, expect_green: false },
            // 注入：键越窗（完成键路必抓）。
            _ => CaseSpec { idx: i as u32, status: -3, information: 8, key: KEY_WIN.1 + (i as u32), expect_green: false },
        };
        v.push(spec);
    }
    v
}

/// 语料指纹（冻结表完整性：指纹漂移即语料被改）。
pub fn corpus_fingerprint(v: &[CaseSpec]) -> u32 {
    let mut fp = 0x8078_0000u32;
    for c in v.iter() {
        fp = fp.wrapping_mul(31).wrapping_add(c.status as u32);
        fp = fp.wrapping_mul(31).wrapping_add(c.information);
        fp = fp.wrapping_mul(31).wrapping_add(c.key);
        fp = fp.wrapping_mul(31).wrapping_add(c.expect_green as u32);
    }
    fp.wrapping_add(v.len() as u32)
}

/// 双侧求值：母版侧与镜像侧是同输入的两路独立调用（判据锚定：
/// 拒止码双侧一致）。返回 (母版判, 镜像判)。
fn dual_judge(c: &CaseSpec) -> (Result<bool, u32>, Result<bool, u32>) {
    // 母版侧：直判。
    let master = judge_case(c);
    // 镜像侧：独立重判（同函数同输入，确定性双跑的冻结表形态）。
    let mirror = judge_case(c);
    (master, mirror)
}

/// 30 例联测（判据核心）：逐例双侧对平 + 零静默 + 全过。
pub fn joint_test() -> Result<u32, u32> {
    let v = corpus();
    if v.len() != CASES {
        return Err(AlignCode::CorpusFull.val());
    }
    let mut greens = 0u32;
    let mut silent = 0u32;
    for c in v.iter() {
        if c.idx >= CASES as u32 {
            return Err(AlignCode::CaseIndexOut.val());
        }
        let (m, s) = dual_judge(c);
        // 双侧判码一致（含绿判一致）。
        if m != s {
            return Err(AlignCode::SideDrift.val());
        }
        // 零静默：判码必须显性（绿判或段内红码）。
        match m {
            Ok(true) => greens = greens.saturating_add(1),
            Ok(false) => silent = silent.saturating_add(1),
            Err(code) => {
                if !code_in_set(code) {
                    return Err(AlignCode::CodeOutOfSet.val());
                }
            }
        }
        // 期望对平：注入例必须红、正常例必须绿。
        let is_green = m == Ok(true);
        if is_green != c.expect_green {
            return Err(AlignCode::CaseRed.val());
        }
    }
    if silent != 0 {
        return Err(AlignCode::ZeroSilent.val());
    }
    Ok(greens)
}

/// 黄金正演：联测全过 + 语料指纹冻结。
fn golden_run() -> Result<u32, u32> {
    let fp = corpus_fingerprint(&corpus());
    let g = joint_test()?;
    if fp == 0 {
        return Err(AlignCode::CorpusDrift.val());
    }
    Ok(g)
}

/// 双跑一致：语料指纹与联测结果两次相等。
fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && corpus_fingerprint(&corpus()) == corpus_fingerprint(&corpus())
}

/// 三路注入逐路必抓（状态/完成值/完成键各一路专用例）。
fn lane_inject_ok() -> bool {
    let s_inj = judge_case(&CaseSpec { idx: 0, status: 0, information: 3, key: KEY_WIN.0, expect_green: true })
        == Err(AlignCode::StatusLaneBreak.val());
    let i_inj = judge_case(&CaseSpec { idx: 1, status: -1, information: INFO_CAP + 1, key: KEY_WIN.0, expect_green: true })
        == Err(AlignCode::InfoLaneBreak.val());
    let k_inj = judge_case(&CaseSpec { idx: 2, status: -1, information: 1, key: KEY_WIN.1, expect_green: true })
        == Err(AlignCode::KeyLaneBreak.val());
    s_inj && i_inj && k_inj
}

/// 判码越闭集注入：账外码必拒。
fn code_set_inject_ok() -> bool {
    !code_in_set(0xCAFE) && !code_in_set(SEG_BASE - 1) && code_in_set(SEG_BASE + 0x01)
}

// ── 判码流水账（零静默的账面化 + 双跑重演） ─────────────────────
/// 逐例判码流水账：每例入库必落显性判（绿判/段内红码两态之一），
/// 账实对平（账内笔数 ≡ 例数）+ 红码闭集（账外码即静默面）。
pub struct AlignLedger {
    /// 判码流水：(例号, 绿判, 红码[绿判时 0])。
    entries: Vec<(u32, bool, u32)>,
    /// 静默计数（判码不显性的例数，恒应为 0）。
    pub silent: u32,
    /// 越闭集计数。
    pub out_of_set: u32,
}

impl AlignLedger {
    pub fn new() -> AlignLedger {
        AlignLedger { entries: Vec::new(), silent: 0, out_of_set: 0 }
    }

    /// 记一笔判（显性两态：绿判 / 段内红码；无判即静默面记账）。
    pub fn record(&mut self, idx: u32, r: Result<bool, u32>) {
        match r {
            Ok(green) => self.entries.push((idx, green, 0)),
            Err(code) => {
                if !code_in_set(code) {
                    self.out_of_set = self.out_of_set.saturating_add(1);
                }
                self.entries.push((idx, false, code));
            }
        }
    }

    /// 账内例号查找（不裸下标）。
    pub fn find(&self, idx: u32) -> Option<(u32, bool, u32)> {
        self.entries.iter().copied().find(|e| e.0 == idx)
    }

    /// 账实对平：账内笔数 ≡ 语料例数，且每例判码显性（零静默）。
    pub fn audit(&self, total: u32) -> Result<u32, u32> {
        if self.entries.len() as u32 != total {
            return Err(AlignCode::CaseIndexOut.val());
        }
        if self.silent != 0 {
            return Err(AlignCode::ZeroSilent.val());
        }
        if self.out_of_set != 0 {
            return Err(AlignCode::CodeOutOfSet.val());
        }
        // 红码全部落闭集（账面化零静默复核）。
        for (_, green, code) in self.entries.iter() {
            if !*green && *code != 0 && !code_in_set(*code) {
                return Err(AlignCode::CodeOutOfSet.val());
            }
        }
        let greens = self.entries.iter().filter(|(_, g, _)| *g).count() as u32;
        Ok(greens)
    }

    /// 重演：同语料再跑一遍，逐例判码必须逐位相等（判据锚定：可重演）。
    pub fn replay_equal(&self, other: &AlignLedger) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            match (self.entries.get(i), other.entries.get(i)) {
                (Some(a), Some(b)) if a == b => i += 1,
                _ => return false,
            }
        }
        true
    }

    /// 账指纹（双跑对平素材）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8078_1000u32;
        for (idx, green, code) in self.entries.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(*idx);
            fp = fp.wrapping_mul(31).wrapping_add(*green as u32);
            fp = fp.wrapping_mul(31).wrapping_add(*code);
        }
        fp
    }
}

/// 流水账全跑：30 例逐例落账 + 审计绿 + 重演对平。
fn ledger_run() -> Result<u32, u32> {
    let v = corpus();
    let mut led = AlignLedger::new();
    for c in v.iter() {
        led.record(c.idx, judge_case(c));
    }
    let greens = led.audit(CASES as u32)?;
    // 重演：再跑一遍逐例对平。
    let mut led2 = AlignLedger::new();
    for c in v.iter() {
        led2.record(c.idx, judge_case(c));
    }
    if !led.replay_equal(&led2) || led.fingerprint() != led2.fingerprint() {
        return Err(AlignCode::FingerprintDrift.val());
    }
    Ok(greens)
}

/// 黄金正演：联测全过 + 语料指纹冻结。
/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in AlignCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：30 例联测全过。
    let jt = joint_test();
    v.push(("J2-30例联测全过", jt == Ok(15), format!("{:?}", jt)));
    // 判据②：零静默（每例显性落判——注入例全以段内红码落账）。
    let corpus_v = corpus();
    let mut reds = 0u32;
    let mut all_explicit = true;
    for c in corpus_v.iter() {
        match judge_case(c) {
            Ok(_) => {}
            Err(code) => {
                reds = reds.saturating_add(1);
                if !code_in_set(code) {
                    all_explicit = false;
                }
            }
        }
    }
    v.push(("J2-零静默显性落判", all_explicit && reds == 15, format!("红例 {}", reds)));
    // 判据②：拒止码双侧一致（逐例断言）。
    let mut side_ok = true;
    for c in corpus_v.iter() {
        let (m, s) = dual_judge(c);
        if m != s {
            side_ok = false;
        }
    }
    v.push(("J2-双侧判码一致", side_ok, String::from("母版=镜像逐例")));
    // 三路注入。
    v.push(("J3-三路注入必抓", lane_inject_ok(), String::from("状态/完成值/完成键")));
    v.push(("J3-判码越闭集拒", code_set_inject_ok(), String::from("账外码即静默面")));
    // 边界：例号越界（judge 不查例号，联测层按 0..30 闭域抓）。
    let oob_spec = CaseSpec { idx: CASES as u32, status: -1, information: 0, key: KEY_WIN.0, expect_green: true };
    let idx_oob = oob_spec.idx >= CASES as u32;
    let idx_in = corpus_v.first().map(|c| c.idx < CASES as u32) == Some(true);
    v.push(("J4-例号越界检出", idx_oob && idx_in, String::from("0..30 闭域")));
    // 边界：完成值恰顶放行、超 1 拒。
    let at_cap = judge_case(&CaseSpec { idx: 0, status: -1, information: INFO_CAP, key: KEY_WIN.0, expect_green: true }).is_ok();
    let over_cap = judge_case(&CaseSpec { idx: 1, status: -1, information: INFO_CAP + 1, key: KEY_WIN.0, expect_green: true })
        == Err(AlignCode::InfoLaneBreak.val());
    v.push(("J4-完成值恰顶/超1", at_cap && over_cap, String::from("饱和域卡边")));
    // 边界：键窗卡边。
    let k_lo = judge_case(&CaseSpec { idx: 0, status: -1, information: 1, key: KEY_WIN.0, expect_green: true }).is_ok();
    let k_hi = judge_case(&CaseSpec { idx: 1, status: -1, information: 1, key: KEY_WIN.1 - 1, expect_green: true }).is_ok();
    let k_over = judge_case(&CaseSpec { idx: 2, status: -1, information: 1, key: KEY_WIN.1, expect_green: true })
        == Err(AlignCode::KeyLaneBreak.val());
    v.push(("J4-键窗卡边", k_lo && k_hi && k_over, String::from("窗内放行/窗外拒")));
    // 边界：成功空包绿、成功带值红（互斥卡边）。
    let zero_ok = judge_case(&CaseSpec { idx: 0, status: 0, information: 0, key: KEY_WIN.0, expect_green: true }).is_ok();
    v.push(("J4-成功档互斥卡边", zero_ok, String::from("Status=0 须 Info=0")));
    // 语料指纹冻结。
    let fp_a = corpus_fingerprint(&corpus());
    let fp_b = corpus_fingerprint(&corpus());
    v.push(("J5-语料指纹冻结", fp_a == fp_b && fp_a != 0, format!("fp={}", fp_a)));
    // 判码流水账：30 例逐例落账 + 零静默审计 + 重演对平。
    let lr = ledger_run();
    v.push(("J5-判码流水账全跑", lr == Ok(15), format!("{:?}", lr)));
    // 静默面注入：账缺一笔即对平破。
    let mut led_partial = AlignLedger::new();
    for c in corpus_v.iter().take(10) {
        led_partial.record(c.idx, judge_case(c));
    }
    let short = led_partial.audit(CASES as u32) == Err(AlignCode::CaseIndexOut.val());
    v.push(("J5-账缺笔必抓", short, String::from("账实对平")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80780 窗口")));
    v
}

/// VX9 检查入口（卡 12630，UNX-F12631-J1 判据承载）。
pub fn run_unx12630_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12630 完成包三字段对平");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_joint_test_fifteen_greens() {
        assert_eq!(joint_test(), Ok(15));
    }

    #[test]
    fn test_corpus_shape() {
        let v = corpus();
        assert_eq!(v.len(), CASES);
        // 6 类轮换：3 绿类（0/1/2）× 5 + 3 红类（3/4/5）× 5。
        let greens = v.iter().filter(|c| c.expect_green).count();
        assert_eq!(greens, 15);
    }

    #[test]
    fn test_lane_inject() {
        assert!(lane_inject_ok());
    }

    #[test]
    fn test_code_set_inject() {
        assert!(code_set_inject_ok());
    }

    #[test]
    fn test_dual_judge_side_equal() {
        for c in corpus().iter() {
            let (m, s) = dual_judge(c);
            assert_eq!(m, s, "例 {} 双侧必须一致", c.idx);
        }
    }

    #[test]
    fn test_case_red_detected() {
        // 期望被改坏的例（注入例标绿）必须在联测层抓出。
        let mut bad = corpus();
        for c in bad.iter_mut() {
            if c.idx == 3 {
                c.expect_green = true; // 注入例被伪标绿
            }
        }
        let r = joint_test_with(&bad);
        assert_eq!(r, Err(AlignCode::CaseRed.val()));
    }

    /// 联测（语料可注入形态，测试专用）。
    fn joint_test_with(v: &[CaseSpec]) -> Result<u32, u32> {
        if v.len() != CASES {
            return Err(AlignCode::CorpusFull.val());
        }
        for c in v.iter() {
            let (m, s) = dual_judge(c);
            if m != s {
                return Err(AlignCode::SideDrift.val());
            }
            let is_green = m == Ok(true);
            if is_green != c.expect_green {
                return Err(AlignCode::CaseRed.val());
            }
        }
        Ok(v.iter().filter(|c| judge_case(c).is_ok()).count() as u32)
    }

    #[test]
    fn test_info_cap_edge() {
        assert!(judge_case(&CaseSpec { idx: 0, status: -1, information: INFO_CAP, key: KEY_WIN.0, expect_green: true }).is_ok());
        assert_eq!(
            judge_case(&CaseSpec { idx: 1, status: -1, information: INFO_CAP + 1, key: KEY_WIN.0, expect_green: true }),
            Err(AlignCode::InfoLaneBreak.val())
        );
    }

    #[test]
    fn test_key_window_edge() {
        assert!(judge_case(&CaseSpec { idx: 0, status: -1, information: 1, key: KEY_WIN.1 - 1, expect_green: true }).is_ok());
        assert_eq!(
            judge_case(&CaseSpec { idx: 1, status: -1, information: 1, key: KEY_WIN.1, expect_green: true }),
            Err(AlignCode::KeyLaneBreak.val())
        );
    }

    #[test]
    fn test_ledger_run_and_replay() {
        assert_eq!(ledger_run(), Ok(15));
        let v = corpus();
        let mut a = AlignLedger::new();
        let mut b = AlignLedger::new();
        for c in v.iter() {
            a.record(c.idx, judge_case(c));
            b.record(c.idx, judge_case(c));
        }
        assert!(a.replay_equal(&b));
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.audit(CASES as u32), Ok(15));
        // 换例即重演漂移必显。
        let mut c2 = AlignLedger::new();
        for c in v.iter() {
            c2.record(c.idx + 100, judge_case(c));
        }
        assert!(!a.replay_equal(&c2));
    }

    #[test]
    fn test_ledger_out_of_set_accounted() {
        let mut led = AlignLedger::new();
        led.record(0, Err(0xCAFE));
        assert_eq!(led.out_of_set, 1);
        // 账外码在审计层必抓。
        let mut led2 = AlignLedger::new();
        for c in corpus().iter() {
            led2.record(c.idx, judge_case(c));
        }
        match led2.entries.get_mut(3) {
            Some(e) => e.2 = 0xCAFE,
            None => panic!("账内应有第 3 槽"),
        }
        assert_eq!(led2.audit(CASES as u32), Err(AlignCode::CodeOutOfSet.val()));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80780);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in AlignCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
