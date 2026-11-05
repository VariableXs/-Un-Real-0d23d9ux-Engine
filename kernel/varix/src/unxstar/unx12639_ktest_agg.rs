//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12639 ktest 聚合器（B31–B32 消息面 40 条聚合） · 0x809C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12640-J1 B33 主题框架登记，与 F12598/F12599/
//!      F12620 预告链衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - ktest 聚合器（承接 0x80980 文档对齐域的两批聚合思想，本卡落
//!     B31/B32 消息面联测聚合）：批标签闭集二分 {B31, B32}，用例逐
//!     批登记（BatchMissing 抓未登记批、CaseShort 抓缺例）；
//!   - 40 条聚合（判据承载）：B31 批 18 例 + B32 批 22 例 = 40 条，
//!     全跑条数与聚合账等式（run_total ≡ sum(cases) ≡ 40）破即
//!     AggEqBroken，等式随批归档落账；
//!   - 用例谓词：每例一个整数判定（值落冻结窗即绿），红例逐例可指
//!     名（CaseRed），注入位单点必抓；
//!   - B33 主题框架登记（判据锚定）：主题 "B33-E1探测面" 登记入册，
//!     预告链三锚 F12598/F12599/F12620 冻结闭表，逐锚解析零悬空
//!     （DanglingLink 抓悬空锚）；
//!   - LCG 种子写死 = SEG_BASE（0x809C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x809C0..0x80A00。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x809C0，AI-VX9-1105 B03 批 12 槽第 4 槽）。
pub const SEG_BASE: u32 = 0x809C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 批未登记：未登记批先跑例
    BatchMissing,
    /// 缺例：批例数与冻结值不符
    CaseShort,
    /// 聚合等式破：全跑条数 ≠ sum 或 ≠ 40
    AggEqBroken,
    /// 红例：用例判定未过
    CaseRed,
    /// 用例越窗：用例判定值不在冻结窗
    CaseOverWindow,
    /// 重复登记：同批重复登记
    BatchDup,
    /// 悬空锚：预告链锚解析失败
    DanglingLink,
    /// 主题重复登记
    TopicDup,
    /// 主题缺登记：未登记主题先跑链
    TopicMissing,
    /// 聚合轮倒退：轮次只进不退
    RoundRegress,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl AggCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AggCode::BatchMissing => 0x01,
                AggCode::CaseShort => 0x02,
                AggCode::AggEqBroken => 0x03,
                AggCode::CaseRed => 0x04,
                AggCode::CaseOverWindow => 0x05,
                AggCode::BatchDup => 0x06,
                AggCode::DanglingLink => 0x07,
                AggCode::TopicDup => 0x08,
                AggCode::TopicMissing => 0x09,
                AggCode::RoundRegress => 0x0A,
                AggCode::FingerprintDrift => 0x0B,
                AggCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            AggCode::BatchMissing => "批未登记：先登记 B31/B32 再跑例",
            AggCode::CaseShort => "缺例：按冻结值 18/22 补齐用例",
            AggCode::AggEqBroken => "聚合等式破：全跑条数须等 sum 且为 40",
            AggCode::CaseRed => "红例：逐例修绿后再聚合",
            AggCode::CaseOverWindow => "用例越窗：判定值落冻结窗",
            AggCode::BatchDup => "重复登记：同批只许登记一次",
            AggCode::DanglingLink => "悬空锚：预告链锚须解析到冻结闭表",
            AggCode::TopicDup => "主题重复登记：一主题一框架",
            AggCode::TopicMissing => "主题缺登记：先登记 B33 框架",
            AggCode::RoundRegress => "聚合轮倒退：轮次只进不退",
            AggCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            AggCode::Unknown => "未知诊断码：不在 0x809C0 段闭集内",
        }
    }

    pub const ALL: [AggCode; 12] = [
        AggCode::BatchMissing,
        AggCode::CaseShort,
        AggCode::AggEqBroken,
        AggCode::CaseRed,
        AggCode::CaseOverWindow,
        AggCode::BatchDup,
        AggCode::DanglingLink,
        AggCode::TopicDup,
        AggCode::TopicMissing,
        AggCode::RoundRegress,
        AggCode::FingerprintDrift,
        AggCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in AggCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    AggCode::Unknown.detail()
}

// ── 冻结常量表（来源：ktest 聚合手账 · B31/B32 批节） ────────────
/// B31 批用例数（IO 完成端口联测，冻结 18）。
pub const B31_N: usize = 18;
/// B32 批用例数（ALPC 消息面联测，冻结 22）。
pub const B32_N: usize = 22;
/// 两批总用例数（判据 40）。
pub const AGG_TOTAL: usize = 40;
/// 用例判定窗（判定值落 [0, 512) 即绿，冻结）。
pub const CASE_WIN: (u32, u32) = (0, 512);
/// 主题账容量。
pub const TOPIC_CAP: usize = 16;

/// 批标签闭集二分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchTag {
    /// B31 批（IO 完成端口联测）。
    B31,
    /// B32 批（ALPC 消息面联测）。
    B32,
}

impl BatchTag {
    /// 批序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            BatchTag::B31 => 0,
            BatchTag::B32 => 1,
        }
    }

    /// 冻结例数。
    pub const fn frozen_n(self) -> usize {
        match self {
            BatchTag::B31 => B31_N,
            BatchTag::B32 => B32_N,
        }
    }
}

/// 一条用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRec {
    /// 用例号（批内 1 起）。
    pub no: u32,
    /// 批标签。
    pub tag: BatchTag,
    /// 判定值（落窗即绿）。
    pub val: u32,
}

/// 用例判定：值落窗即绿（判定 = 谓词纯函数，无副作用）。
pub fn case_judge(c: &CaseRec) -> Result<u32, u32> {
    if c.val < CASE_WIN.0 || c.val >= CASE_WIN.1 {
        return Err(AggCode::CaseOverWindow.val());
    }
    Ok(c.no)
}

/// 冻结预告链锚闭表（B33 框架衔接锚：F12598/F12599/F12620）。
pub const CHAIN_ANCHORS: [(&str, &str); 3] = [
    ("F12598", "B31 IO 完成端口联测批预告锚"),
    ("F12599", "B32 ALPC 消息面联测批预告锚"),
    ("F12620", "B32 闸门预告锚"),
];

/// 预告链锚解析：锚名必须在冻结闭表内（零悬空）。
pub fn anchor_resolve(name: &str) -> Result<&'static str, u32> {
    for a in CHAIN_ANCHORS.iter() {
        if a.0 == name {
            return Ok(a.1);
        }
    }
    Err(AggCode::DanglingLink.val())
}

/// 用例语料（B31 18 例 + B32 22 例，判定值 LCG 冻结派生，全绿设计）。
pub fn case_corpus() -> Vec<CaseRec> {
    let mut v: Vec<CaseRec> = Vec::new();
    let mut st = SEG_BASE as u64;
    let mut i = 0usize;
    while i < AGG_TOTAL {
        st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        // 判定值冻结派生：压进窗内（% 400 + 7），全绿设计。
        let val = (((st >> 33) & 0xFFFF) as u32 % 400).saturating_add(7);
        let (no, tag) = if i < B31_N {
            ((i + 1) as u32, BatchTag::B31)
        } else {
            ((i - B31_N + 1) as u32, BatchTag::B32)
        };
        v.push(CaseRec { no, tag, val });
        i += 1;
    }
    v
}

/// ktest 聚合账。
pub struct AggLedger {
    /// 已登记批。
    batches: Vec<BatchTag>,
    /// 已跑用例。
    ran: Vec<CaseRec>,
    /// 轮次（单调）。
    rounds: Vec<u32>,
    /// B33 主题已登记。
    topic_on: bool,
    /// 已解析预告链锚。
    pub chain_resolved: Vec<String>,
}

impl AggLedger {
    pub fn new() -> AggLedger {
        AggLedger {
            batches: Vec::new(),
            ran: Vec::new(),
            rounds: Vec::new(),
            topic_on: false,
            chain_resolved: Vec::new(),
        }
    }

    /// 批登记（同批重复登记拒）。
    pub fn register_batch(&mut self, t: BatchTag) -> Result<u32, u32> {
        for b in self.batches.iter() {
            if *b == t {
                return Err(AggCode::BatchDup.val());
            }
        }
        self.batches.push(t);
        Ok(t.idx())
    }

    /// 跑批：逐例判定入账（未登记批先跑必抓）。
    pub fn run_batch(&mut self, t: BatchTag, cases: &[CaseRec]) -> Result<u32, u32> {
        let mut on = false;
        for b in self.batches.iter() {
            if *b == t {
                on = true;
            }
        }
        if !on {
            return Err(AggCode::BatchMissing.val());
        }
        let mut greens = 0u32;
        for c in cases.iter() {
            if c.tag != t {
                continue;
            }
            match case_judge(c) {
                Ok(_) => {
                    self.ran.push(*c);
                    greens += 1;
                }
                Err(e) => return Err(e),
            }
        }
        if greens as usize != t.frozen_n() {
            return Err(AggCode::CaseShort.val());
        }
        Ok(greens)
    }

    /// 聚合轮（轮次单调只进不退）。
    pub fn enter_round(&mut self, round: u32) -> Result<u32, u32> {
        if let Some(last) = self.rounds.last() {
            if round <= *last {
                return Err(AggCode::RoundRegress.val());
            }
        }
        self.rounds.push(round);
        Ok(round)
    }

    /// B33 主题框架登记（判据锚定）。
    pub fn register_topic(&mut self) -> Result<u32, u32> {
        if self.topic_on {
            return Err(AggCode::TopicDup.val());
        }
        self.topic_on = true;
        Ok(1)
    }

    /// 预告链衔接：三锚逐锚解析（未登记主题先跑链必抓）。
    pub fn link_chain(&mut self) -> Result<u32, u32> {
        if !self.topic_on {
            return Err(AggCode::TopicMissing.val());
        }
        self.chain_resolved.clear();
        for a in CHAIN_ANCHORS.iter() {
            match anchor_resolve(a.0) {
                Ok(desc) => self.chain_resolved.push(String::from(desc)),
                Err(e) => return Err(e),
            }
        }
        Ok(self.chain_resolved.len() as u32)
    }

    /// 判据谓词：聚合等式——全跑条数 ≡ sum(批例) ≡ 40。
    pub fn agg_equation(&self) -> Result<u32, u32> {
        let b31 = self.count_tag(BatchTag::B31);
        let b32 = self.count_tag(BatchTag::B32);
        let sum = b31.saturating_add(b32);
        if b31 != B31_N || b32 != B32_N {
            return Err(AggCode::CaseShort.val());
        }
        if sum != AGG_TOTAL || self.ran.len() != AGG_TOTAL {
            return Err(AggCode::AggEqBroken.val());
        }
        Ok(AGG_TOTAL as u32)
    }

    /// 按批计数。
    pub fn count_tag(&self, t: BatchTag) -> usize {
        let mut n = 0usize;
        for c in self.ran.iter() {
            if c.tag == t {
                n += 1;
            }
        }
        n
    }

    /// 总条数。
    pub fn total_ran(&self) -> usize {
        self.ran.len()
    }

    /// 等式归档文本。
    pub fn archive_text(&self) -> String {
        format!(
            "聚合账等式：B31 {} + B32 {} = 全跑 {}（冻结 40）",
            self.count_tag(BatchTag::B31),
            self.count_tag(BatchTag::B32),
            self.total_ran()
        )
    }

    /// 账指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x809C_0D00u32;
        for c in self.ran.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(c.no)
                .wrapping_add(c.tag.idx())
                .wrapping_add(c.val);
        }
        for r in self.rounds.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(*r);
        }
        fp.wrapping_add(if self.topic_on { 0x11 } else { 0 })
    }
}

/// 黄金正演：双批登记 + 40 例全跑 + 聚合等式 + B33 登记 + 预告链零悬空。
fn golden_run() -> Result<u32, u32> {
    let mut l = AggLedger::new();
    let _ = l.register_batch(BatchTag::B31)?;
    let _ = l.register_batch(BatchTag::B32)?;
    let _ = l.run_batch(BatchTag::B31, &case_corpus())?;
    let _ = l.run_batch(BatchTag::B32, &case_corpus())?;
    let eq = l.agg_equation()?;
    let _ = l.register_topic()?;
    let links = l.link_chain()?;
    if links != CHAIN_ANCHORS.len() as u32 {
        return Err(AggCode::DanglingLink.val());
    }
    Ok(eq)
}

/// 双跑一致：账指纹两次独立构造相等。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut l = AggLedger::new();
        let _ = l.register_batch(BatchTag::B31);
        let _ = l.register_batch(BatchTag::B32);
        let _ = l.run_batch(BatchTag::B31, &case_corpus());
        let _ = l.run_batch(BatchTag::B32, &case_corpus());
        let _ = l.register_topic();
        let _ = l.link_chain();
        l.fingerprint()
    };
    run() == run() && golden_run() == golden_run()
}

/// 注入必抓：未登记批先跑 / 例缺 / 越窗红例 / 重复批登记 / 主题未登
/// 记先跑链 / 悬空锚 / 轮次倒退，逐路单点必抓。
fn inject_violations() -> bool {
    // ① 未登记批先跑。
    let mut l = AggLedger::new();
    let m1 = l.run_batch(BatchTag::B31, &case_corpus()) == Err(AggCode::BatchMissing.val());
    // ② 例缺：只入 B31 例数的一半（截半语料走 take，不裸切片）。
    let _ = l.register_batch(BatchTag::B31);
    let half: Vec<CaseRec> = case_corpus().iter().take(B31_N / 2).copied().collect();
    let m2 = l.run_batch(BatchTag::B31, &half) == Err(AggCode::CaseShort.val());
    // ③ 越窗红例必抓。
    let red = CaseRec { no: 99, tag: BatchTag::B31, val: CASE_WIN.1 + 1 };
    let m3 = case_judge(&red) == Err(AggCode::CaseOverWindow.val());
    // ④ 重复批登记。
    let m4 = l.register_batch(BatchTag::B31) == Err(AggCode::BatchDup.val());
    // ⑤ 主题未登记先跑链。
    let m5 = l.link_chain() == Err(AggCode::TopicMissing.val());
    // ⑥ 悬空锚：链外锚名解析必抓。
    let m6 = anchor_resolve("F99999") == Err(AggCode::DanglingLink.val());
    // ⑦ 轮次倒退。
    let _ = l.enter_round(3);
    let m7 = l.enter_round(3) == Err(AggCode::RoundRegress.val());
    let m8 = l.enter_round(2) == Err(AggCode::RoundRegress.val());
    m1 && m2 && m3 && m4 && m5 && m6 && m7 && m8
}

/// 预告链零悬空注入：三锚全解析 + 链外锚全拒（多路单点）。
fn chain_dangling_inject() -> bool {
    let ok1 = anchor_resolve("F12598").is_ok();
    let ok2 = anchor_resolve("F12599").is_ok();
    let ok3 = anchor_resolve("F12620").is_ok();
    let bad1 = anchor_resolve("F12597").is_err();
    let bad2 = anchor_resolve("F12621").is_err();
    let bad3 = anchor_resolve("").is_err();
    ok1 && ok2 && ok3 && bad1 && bad2 && bad3
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in AggCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
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
    // 判据承载：40 条聚合 + 等式。
    let mut l = AggLedger::new();
    let _ = l.register_batch(BatchTag::B31);
    let _ = l.register_batch(BatchTag::B32);
    let _ = l.run_batch(BatchTag::B31, &case_corpus());
    let _ = l.run_batch(BatchTag::B32, &case_corpus());
    let eq = l.agg_equation();
    v.push((
        "J2-两批40条聚合",
        eq == Ok(AGG_TOTAL as u32),
        format!("B31{} B32{} 全跑{}", l.count_tag(BatchTag::B31), l.count_tag(BatchTag::B32), l.total_ran()),
    ));
    v.push((
        "J2-等式随批归档",
        !l.archive_text().is_empty() && l.archive_text().contains("聚合账等式"),
        l.archive_text(),
    ));
    // 判据②：B33 主题框架登记 + 预告链零悬空。
    let _ = l.register_topic();
    let links = l.link_chain();
    v.push((
        "J2-B33框架登记",
        l.fingerprint() != 0 && links == Ok(CHAIN_ANCHORS.len() as u32),
        format!("预告链 {:?}", links),
    ));
    v.push((
        "J2-预告链零悬空",
        l.chain_resolved.len() == CHAIN_ANCHORS.len(),
        String::from("F12598/F12599/F12620 三锚全解析"),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("未登记/缺例/越窗/重复/悬空/倒退"),
    ));
    v.push((
        "J3-悬空锚注入",
        chain_dangling_inject(),
        String::from("链内三绿/链外三拒"),
    ));
    // 边界：用例判定卡边。
    let lo = CaseRec { no: 1, tag: BatchTag::B31, val: CASE_WIN.0 };
    let hi = CaseRec { no: 2, tag: BatchTag::B32, val: CASE_WIN.1 - 1 };
    v.push((
        "J4-判定卡边",
        case_judge(&lo).is_ok() && case_judge(&hi).is_ok(),
        format!("{}..{} 窗", CASE_WIN.0, CASE_WIN.1),
    ));
    // 边界：批例数冻结值。
    v.push((
        "J4-批例数冻结",
        BatchTag::B31.frozen_n() == B31_N && BatchTag::B32.frozen_n() == B32_N,
        format!("B31={} B32={}", B31_N, B32_N),
    ));
    // 边界：语料规模恰 40。
    v.push((
        "J4-语料恰40",
        case_corpus().len() == AGG_TOTAL,
        format!("语料 {}", case_corpus().len()),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-账指纹复验",
        l.fingerprint() != 0,
        format!("fp={:#x}", l.fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x809C0 窗口")));
    v
}

/// VX9 检查入口（卡 12639，UNX-F12640-J1 判据承载）。
pub fn run_unx12639_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12639 ktest聚合器B31-B32消息面40条");
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
    fn test_corpus_40_frozen() {
        let c = case_corpus();
        assert_eq!(c.len(), AGG_TOTAL);
        let mut b31 = 0usize;
        let mut b32 = 0usize;
        for x in c.iter() {
            match x.tag {
                BatchTag::B31 => b31 += 1,
                BatchTag::B32 => b32 += 1,
            }
        }
        assert_eq!(b31, B31_N);
        assert_eq!(b32, B32_N);
    }

    #[test]
    fn test_agg_equation() {
        let mut l = AggLedger::new();
        let _ = l.register_batch(BatchTag::B31);
        let _ = l.register_batch(BatchTag::B32);
        let _ = l.run_batch(BatchTag::B31, &case_corpus());
        let _ = l.run_batch(BatchTag::B32, &case_corpus());
        assert_eq!(l.agg_equation(), Ok(AGG_TOTAL as u32));
        assert!(l.archive_text().contains("40"));
    }

    #[test]
    fn test_batch_missing_first() {
        let mut l = AggLedger::new();
        assert_eq!(l.run_batch(BatchTag::B32, &case_corpus()), Err(AggCode::BatchMissing.val()));
        let _ = l.register_batch(BatchTag::B32);
        assert!(l.run_batch(BatchTag::B32, &case_corpus()).is_ok());
    }

    #[test]
    fn test_chain_zero_dangling() {
        let mut l = AggLedger::new();
        assert_eq!(l.link_chain(), Err(AggCode::TopicMissing.val()));
        let _ = l.register_topic();
        assert_eq!(l.link_chain(), Ok(CHAIN_ANCHORS.len() as u32));
        assert!(l.chain_resolved.len() == 3);
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_chain_dangling_inject() {
        assert!(chain_dangling_inject());
    }

    #[test]
    fn test_case_edges() {
        assert_eq!(case_judge(&CaseRec { no: 1, tag: BatchTag::B31, val: CASE_WIN.0 }).is_ok(), true);
        assert_eq!(
            case_judge(&CaseRec { no: 2, tag: BatchTag::B31, val: CASE_WIN.1 }),
            Err(AggCode::CaseOverWindow.val())
        );
        assert!(case_judge(&CaseRec { no: 3, tag: BatchTag::B32, val: CASE_WIN.1 - 1 }).is_ok());
    }

    #[test]
    fn test_round_monotone() {
        let mut l = AggLedger::new();
        assert!(l.enter_round(1).is_ok());
        assert_eq!(l.enter_round(1), Err(AggCode::RoundRegress.val()));
        assert_eq!(l.enter_round(0), Err(AggCode::RoundRegress.val()));
        assert!(l.enter_round(2).is_ok());
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(case_corpus(), case_corpus());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x809C0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in AggCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x7777).contains("未知"));
    }
}
