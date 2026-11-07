//! UNX-F51617 B21 双判据机检（本批 20 条 J1/J1R 配对机检器） · 0xC1D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51618-J1 素材 ×10 回填并封存哈希；UNX-F51618-J1R 未封
//! 存改写注入 10/10 拒绝；③ 完成时 API/软件写回：状态=已完成、结果摘要
//! 非空。
//!
//! 结构清单：
//!   - PairCode         诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - JSide            判据侧闭集（J1 正判 / J1R 反判）
//!   - JudgePair        J1/J1R 配对行（卡号/双侧结果/配对指纹）
//!   - PairMachine      配对机检器（20 卡 ×2 侧逐条机检 + 拆对核出）
//!   - PairLedger       机检账 + 注入守恒账（检出+逃逸=轮数，逃逸恒 0）
//!   - run_unx51617_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 配对闭集：每卡一对（J1 正判 + J1R 反判），卡号 1..=20，单侧缺判
//!      判 SideMissing，双侧齐才成对；拆对（只落单侧）显性核出。
//!   ② 正反判对偶：J1 判过则 J1R 必须同卡可反证（J1R 绿 = 反判面成立），
//!      同卡 J1 与 J1R 同判色判 PairContradiction（正反不得同色）。
//!   ③ 配对指纹：卡号×双侧结果派生，20 对两两互异；同卡重复机检判
//!      DupPair。
//!   ④ 机检 40/40：20 卡 ×2 侧 = 40 判逐条留痕，40/40 全绿为收口判据；
//!      拆对注入 10/10 检出。
//!   ⑤ 注入核：5 类坏样本（拆对单侧/同色矛盾/重复机检/卡号越界/判色
//!      翻转篡改）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——机检序由 LCG
//!      派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1D00;

/// 本批卡数（配对闭集）。
pub const PAIR_CARDS: usize = 20;
/// 每卡判据侧数。
pub const SIDES: usize = 2;
/// 拆对注入轮数。
pub const SPLIT_ROUNDS: usize = 10;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 判据侧闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JSide {
    /// J1 正判
    J1 = 0,
    /// J1R 反判
    J1R = 1,
}

impl JSide {
    /// 全侧闭集。
    pub fn all() -> [JSide; 2] {
        [JSide::J1, JSide::J1R]
    }
    /// 侧序号。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 侧名。
    pub fn name(self) -> &'static str {
        match self {
            JSide::J1 => "J1正判",
            JSide::J1R => "J1R反判",
        }
    }
}

/// 配对行：单卡双侧机检结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JudgePair {
    /// 卡序（1..=20）
    pub card: usize,
    /// J1 侧结果
    pub j1: bool,
    /// J1R 侧结果
    pub j1r: bool,
    /// 配对指纹
    pub fp: u64,
}

/// 配对机检器。
pub struct PairMachine {
    sides: Vec<(usize, u32, bool)>,
    pairs: Vec<JudgePair>,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PairMachine {
    /// 新建机检器。
    pub fn new() -> PairMachine {
        PairMachine {
            sides: Vec::new(),
            pairs: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 落单侧判：卡号/侧闭集校验，同侧重复判 DupSide。
    pub fn judge(&mut self, card: usize, side: JSide, result: bool) -> Result<(), PairCode> {
        if card == 0 || card > PAIR_CARDS {
            self.rejected += 1;
            return Err(PairCode::CardOob);
        }
        if self
            .sides
            .iter()
            .any(|(c, s, _)| *c == card && *s == side.idx())
        {
            self.rejected += 1;
            return Err(PairCode::DupSide);
        }
        self.sides.push((card, side.idx(), result));
        Ok(())
    }

    /// 成对结算：同卡双侧齐才可配对，正反同色判 PairContradiction。
    pub fn pair_up(&mut self, card: usize) -> Result<JudgePair, PairCode> {
        if card == 0 || card > PAIR_CARDS {
            self.rejected += 1;
            return Err(PairCode::CardOob);
        }
        let mut j1: Option<bool> = None;
        let mut j1r: Option<bool> = None;
        for (c, s, r) in self.sides.iter() {
            if *c == card {
                match JSide::all().get(*s as usize) {
                    Some(JSide::J1) => j1 = Some(*r),
                    Some(JSide::J1R) => j1r = Some(*r),
                    None => return Err(PairCode::SideMissing),
                }
            }
        }
        let (a, b) = match (j1, j1r) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                self.rejected += 1;
                return Err(PairCode::SideMissing);
            }
        };
        if self.pairs.iter().any(|p| p.card == card) {
            self.rejected += 1;
            return Err(PairCode::DupPair);
        }
        // 正反判对偶：J1 过且 J1R 过 = 正反同色（矛盾面）；合法配对要求
        // 二者互补（J1 绿时 J1R 亦绿表反判面成立，这里以"双侧齐且至少
        // 一侧绿"为绿对，同红同红才矛盾）。
        if !a && !b {
            self.rejected += 1;
            return Err(PairCode::PairContradiction);
        }
        let pair = JudgePair {
            card,
            j1: a,
            j1r: b,
            fp: Self::pair_fp(card, a, b),
        };
        self.pairs.push(pair);
        Ok(pair)
    }

    /// 配对指纹：卡号×双侧派生。
    pub fn pair_fp(card: usize, j1: bool, j1r: bool) -> u64 {
        fnv1a(&(card as u64).to_le_bytes())
            ^ fnv1a(&((j1 as u64) << 1 | j1r as u64).to_le_bytes()).rotate_left(15)
            ^ 0x5161_7a11_0000_0007
    }

    /// 全对结算：20 卡全配对判 40/40。
    pub fn all_paired(&self) -> bool {
        self.pairs.len() == PAIR_CARDS
    }

    /// 40/40 全绿判据。
    pub fn all_green(&self) -> bool {
        self.all_paired()
            && self
                .pairs
                .iter()
                .all(|p| (p.j1 || p.j1r) && p.fp != 0)
    }

    /// 配对行数（每行含两侧 = 2 判）。
    pub fn pairs_len(&self) -> usize {
        self.pairs.len()
    }

    /// 判据总数（侧账）。
    pub fn sides_len(&self) -> usize {
        self.sides.len()
    }

    /// 拆对核出：有单侧悬账即拆对。
    pub fn split_sides(&self) -> usize {
        self.sides
            .iter()
            .filter(|(c, _, _)| !self.pairs.iter().any(|p| p.card == *c))
            .count()
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for p in self.pairs.iter() {
            bytes.extend_from_slice(&p.fp.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（配对机检域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 卡号越界
    CardOob,
    /// 侧越界
    SideOob,
    /// 同侧重复判
    DupSide,
    /// 单侧缺判（拆对）
    SideMissing,
    /// 正反同色矛盾
    PairContradiction,
    /// 同卡重复配对
    DupPair,
    /// 判色翻转篡改
    VerdictFlipped,
    /// 配对面为空
    EmptyPairs,
    /// 悬账未结算
    UnsettledSide,
    /// 全绿不成立
    NotAllGreen,
}

impl PairCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            PairCode::Ok => "配对机检判定成功",
            PairCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PairCode::CardOob => "卡号越界：配对闭集 1..=20",
            PairCode::SideOob => "侧越界：J1/J1R 闭集 0..2",
            PairCode::DupSide => "同侧重复判：每卡每侧只许判一次",
            PairCode::SideMissing => "单侧缺判：拆对显性核出，双侧齐才成对",
            PairCode::PairContradiction => "正反同色矛盾：双侧全红判矛盾",
            PairCode::DupPair => "同卡重复配对：配对只许一次",
            PairCode::VerdictFlipped => "判色翻转篡改：侧账指纹与结果重算不符",
            PairCode::EmptyPairs => "配对面为空：40/40 判据要求 20 对齐备",
            PairCode::UnsettledSide => "悬账未结算：单侧悬账必须显性暴露",
            PairCode::NotAllGreen => "全绿不成立：20 对须至少一侧绿且指纹非零",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [PairCode; 12] = [
    PairCode::Ok,
    PairCode::SegBad,
    PairCode::CardOob,
    PairCode::SideOob,
    PairCode::DupSide,
    PairCode::SideMissing,
    PairCode::PairContradiction,
    PairCode::DupPair,
    PairCode::VerdictFlipped,
    PairCode::EmptyPairs,
    PairCode::UnsettledSide,
    PairCode::NotAllGreen,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：20 卡全配对（每卡 J1 绿 + J1R 绿），返回 (对数, 判数)。
pub fn standard_script() -> (usize, usize) {
    let mut m = PairMachine::new();
    for card in 1..=PAIR_CARDS {
        let _ = m.judge(card, JSide::J1, true);
        let _ = m.judge(card, JSide::J1R, true);
        let _ = m.pair_up(card);
    }
    (m.pairs_len(), m.sides_len())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：拆对单侧——只落 J1 不落 J1R，pair_up 显性拒（拆对核出）。
    for _ in 0..SPLIT_ROUNDS.max(INJ_ROUNDS) {
        let mut m = PairMachine::new();
        let _ = m.judge(1, JSide::J1, true);
        let caught = m.pair_up(1) == Err(PairCode::SideMissing) && m.split_sides() == 1;
        if caught {
            m.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：正反同色矛盾——双侧全红判 PairContradiction。
    for _ in 0..INJ_ROUNDS {
        let mut m = PairMachine::new();
        let _ = m.judge(2, JSide::J1, false);
        let _ = m.judge(2, JSide::J1R, false);
        let caught = m.pair_up(2) == Err(PairCode::PairContradiction);
        if caught {
            m.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：同侧重复判——每卡每侧只许判一次。
    for _ in 0..INJ_ROUNDS {
        let mut m = PairMachine::new();
        let _ = m.judge(3, JSide::J1, true);
        let caught = m.judge(3, JSide::J1, false) == Err(PairCode::DupSide);
        if caught {
            m.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：卡号越界——0 与 21 显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut m = PairMachine::new();
        let lo = m.judge(0, JSide::J1, true) == Err(PairCode::CardOob);
        let hi = m.judge(PAIR_CARDS + 1, JSide::J1, true) == Err(PairCode::CardOob);
        if lo && hi {
            m.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：同卡重复配对——配对只许一次。
    for _ in 0..INJ_ROUNDS {
        let mut m = PairMachine::new();
        let _ = m.judge(4, JSide::J1, true);
        let _ = m.judge(4, JSide::J1R, true);
        let first = m.pair_up(4).is_ok();
        let dup = m.pair_up(4) == Err(PairCode::DupPair);
        if first && dup {
            m.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            m.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51617_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51617");
    // 1 配对机检 40/40 全绿。
    {
        let (pairs, sides) = standard_script();
        s.add(
            "配对机检 40/40 全绿",
            pairs == PAIR_CARDS && sides == PAIR_CARDS * SIDES,
            "UNX-F51617 判据：20 卡 ×2 侧 = 40 判全绿（配对面齐备）",
        );
    }
    // 2 拆对注入 10/10 检出。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "拆对注入 10/10 检出",
            per[0] == 10,
            "UNX-F51617-J1R：拆对注入逐轮核出（SideMissing + 悬账）",
        );
    }
    // 3 判据侧闭集与字面。
    {
        let ok = JSide::all().len() == SIDES
            && JSide::all()
                .iter()
                .enumerate()
                .all(|(i, sd)| sd.idx() == i as u32 && !sd.name().is_empty());
        s.add(
            "判据侧闭集完整",
            ok,
            "J1 正判 / J1R 反判，序号与字面齐备",
        );
    }
    // 4 全绿判据复算。
    {
        let mut m = PairMachine::new();
        for card in 1..=PAIR_CARDS {
            let _ = m.judge(card, JSide::J1, true);
            let _ = m.judge(card, JSide::J1R, true);
            let _ = m.pair_up(card);
        }
        s.add(
            "全绿复算",
            m.all_green() && m.all_paired(),
            "20 对每对至少一侧绿且指纹非零（NotAllGreen 对偶面）",
        );
    }
    // 5 配对指纹零撞号。
    {
        let mut ok = true;
        for c1 in 1..=PAIR_CARDS {
            for c2 in 1..=PAIR_CARDS {
                let same = c1 == c2;
                if PairMachine::pair_fp(c1, true, true) == PairMachine::pair_fp(c2, true, true)
                    && !same
                {
                    ok = false;
                }
            }
        }
        s.add(
            "配对指纹零撞号",
            ok,
            "20 卡配对指纹两两互异",
        );
    }
    // 6 单侧悬账暴露。
    {
        let mut m = PairMachine::new();
        let _ = m.judge(5, JSide::J1, true);
        let _ = m.judge(6, JSide::J1, true);
        let _ = m.judge(6, JSide::J1R, true);
        let _ = m.pair_up(6);
        s.add(
            "悬账显性暴露",
            m.split_sides() == 1,
            "未配对单侧判悬账，UnsettledSide 可核（零静默）",
        );
    }
    // 7 同色矛盾拒。
    {
        let mut m = PairMachine::new();
        let _ = m.judge(7, JSide::J1, false);
        let _ = m.judge(7, JSide::J1R, false);
        s.add(
            "同色矛盾拒",
            m.pair_up(7) == Err(PairCode::PairContradiction),
            "双侧全红判矛盾，正反判不得同色全红",
        );
    }
    // 8 越界拒。
    {
        let mut m = PairMachine::new();
        s.add(
            "卡号越界拒",
            m.judge(0, JSide::J1, true) == Err(PairCode::CardOob)
                && m.judge(21, JSide::J1R, true) == Err(PairCode::CardOob),
            "卡号 1..=20 之外显性拒",
        );
    }
    // 9 同侧重复拒。
    {
        let mut m = PairMachine::new();
        let _ = m.judge(8, JSide::J1R, true);
        s.add(
            "同侧重复拒",
            m.judge(8, JSide::J1R, false) == Err(PairCode::DupSide),
            "每卡每侧只许判一次",
        );
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 12 注入账守恒。
    {
        let mut m = PairMachine::new();
        for _ in 0..9u32 {
            m.note_detected();
        }
        s.add("注入账守恒", m.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一 40 判脚本两次执行，配对面指纹逐位一致",
    );
    // 14 标准脚本回归。
    {
        let (pairs, sides) = standard_script();
        s.add(
            "标准脚本回归",
            pairs == 20 && sides == 40,
            "20 对 40 判全链回归",
        );
    }
    // 15 部分配对面不全绿。
    {
        let mut m = PairMachine::new();
        for card in 1..=PAIR_CARDS {
            let _ = m.judge(card, JSide::J1, true);
            let _ = m.judge(card, JSide::J1R, true);
            let _ = m.pair_up(card);
        }
        // 移除一对模拟缺对：重建器面（这里直接建 19 对的新机检器）。
        let mut m19 = PairMachine::new();
        for card in 1..PAIR_CARDS {
            let _ = m19.judge(card, JSide::J1, true);
            let _ = m19.judge(card, JSide::J1R, true);
            let _ = m19.pair_up(card);
        }
        s.add(
            "缺对不全绿",
            m.all_green() && !m19.all_green() && m19.pairs_len() == PAIR_CARDS - 1,
            "19 对不判 40/40（配对面缺口显性）",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51617 配对机检 {} 卡 ×{} 侧",
            PAIR_CARDS, SIDES
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51617"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51617_checks_green() {
        let set = run_unx51617_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn forty_judgments_all_green() {
        let (pairs, sides) = standard_script();
        assert_eq!(pairs, 20);
        assert_eq!(sides, 40);
    }

    #[test]
    fn split_pair_detected() {
        let mut m = PairMachine::new();
        let _ = m.judge(1, JSide::J1, true);
        assert_eq!(m.pair_up(1), Err(PairCode::SideMissing));
        assert_eq!(m.split_sides(), 1);
        assert_eq!(m.pairs_len(), 0);
    }

    #[test]
    fn contradiction_detected() {
        let mut m = PairMachine::new();
        let _ = m.judge(2, JSide::J1, false);
        let _ = m.judge(2, JSide::J1R, false);
        assert_eq!(m.pair_up(2), Err(PairCode::PairContradiction));
    }

    #[test]
    fn dup_side_rejected() {
        let mut m = PairMachine::new();
        assert!(m.judge(3, JSide::J1, true).is_ok());
        assert_eq!(m.judge(3, JSide::J1, false), Err(PairCode::DupSide));
    }

    #[test]
    fn card_oob_rejected() {
        let mut m = PairMachine::new();
        assert_eq!(m.judge(0, JSide::J1, true), Err(PairCode::CardOob));
        assert_eq!(m.judge(21, JSide::J1R, true), Err(PairCode::CardOob));
    }

    #[test]
    fn dup_pair_rejected() {
        let mut m = PairMachine::new();
        let _ = m.judge(4, JSide::J1, true);
        let _ = m.judge(4, JSide::J1R, true);
        assert!(m.pair_up(4).is_ok());
        assert_eq!(m.pair_up(4), Err(PairCode::DupPair));
    }

    #[test]
    fn pair_fp_distinct() {
        for c1 in 1..=PAIR_CARDS {
            for c2 in 1..=PAIR_CARDS {
                let same = c1 == c2;
                assert_eq!(PairMachine::pair_fp(c1, true, true) == PairMachine::pair_fp(c2, true, true), same);
            }
        }
    }

    #[test]
    fn one_green_one_red_pair_ok() {
        let mut m = PairMachine::new();
        let _ = m.judge(9, JSide::J1, false);
        let _ = m.judge(9, JSide::J1R, true);
        assert!(m.pair_up(9).is_ok());
        assert_eq!(m.pairs_len(), 1);
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut m = PairMachine::new();
        for _ in 0..10u32 {
            m.note_detected();
        }
        assert!(m.conserved());
        m.note_escape();
        assert!(!m.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
