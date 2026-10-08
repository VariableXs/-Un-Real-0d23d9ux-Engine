//! 深化层 · F600 I 域收官登记（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F600 节）：
//! ①「总检脚本覆盖 F001-F600 全量 600 检查点」的**四段基线账**——
//!   F001-F200 / F201-F400 / F401-F550 / F551-F600 的覆盖计数与
//!   完整性校验（任何一段缺一点即红）；
//! ②「200 项一行账与正文逐条同源」的**摘要对账引擎**——账行与模拟
//!   正文提取行各自折叠成摘要比对（一处改题当场红）；
//! ③「v1.0 冻结前置检查清单」的**状态机**——三处齐/全绿/四段完整/
//!   候删五项登记在案/F200 终版，五件齐才过闸；
//! ④「F200 条款终版」的**登记账**——修订号与范围双锚对表基础件。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::iregistry::{IRegistry, LedgerLine, I_DOMAIN_ITEMS, TOTAL_ITEMS};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// ① 600 检查点四段基线账
// ---------------------------------------------------------------------------

/// 四段边界（F 编号闭区间——收官口径：200/200/150/50，见 SEGMENT_EXPECT）。
pub const SEGMENTS: [(u32, u32); 4] = [(1, 200), (201, 400), (401, 550), (551, 600)];
/// 每段应有检查点数。
const SEGMENT_EXPECT: [usize; 4] = [200, 200, 150, 50];

/// 段覆盖计数（checkpoint 数组 0 基下标 = F 编号-1）。
pub fn segment_counts(ck: &[bool; TOTAL_ITEMS]) -> [usize; 4] {
    let mut counts = [0usize; 4];
    for (i, &ok) in ck.iter().enumerate() {
        if !ok {
            continue;
        }
        let fid = (i + 1) as u32;
        for (s, &(lo, hi)) in SEGMENTS.iter().enumerate() {
            if fid >= lo && fid <= hi {
                counts[s] += 1;
            }
        }
    }
    counts
}

/// 四段完整性：任何一段缺一点即红。
pub fn segment_complete(counts: &[usize; 4]) -> bool {
    counts.iter().zip(SEGMENT_EXPECT.iter()).all(|(c, e)| c == e)
}

// ---------------------------------------------------------------------------
// ② 一行账摘要对账引擎
// ---------------------------------------------------------------------------

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix_byte(h: &mut u64, b: u8) {
    *h ^= b as u64;
    *h = h.wrapping_mul(FNV_PRIME);
}

/// 单行摘要：F 编号（4 字节小端）+ 标题字节折叠。
pub fn line_digest(f_id: u32, title: &str) -> u64 {
    let mut h = FNV_OFFSET;
    for shift in [0u32, 8, 16, 24] {
        mix_byte(&mut h, (f_id >> shift) as u8);
    }
    for &b in title.as_bytes() {
        mix_byte(&mut h, b);
    }
    h
}

/// 全账摘要：逐行折叠（顺序敏感——账序即正文序）。
pub fn ledger_digest(lines: &[LedgerLine]) -> u64 {
    let mut h = FNV_OFFSET;
    for l in lines {
        h = h.rotate_left(8) ^ line_digest(l.f_id, l.title);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 正文提取模拟：自 (F 编号, 标题) 清单构造账行（脚本同源口径）。
pub fn extract_from_text(text: &[(u32, &'static str)]) -> Vec<LedgerLine> {
    text.iter().map(|&(f_id, title)| LedgerLine { f_id, title, checkpoint: f_id }).collect()
}

// ---------------------------------------------------------------------------
// ③ 冻结前置清单状态机 + 候删五项登记账
// ---------------------------------------------------------------------------

/// 候删登记账（冻结前置之一：候删在案不隐账——F 编号去重）。
pub struct CandidateLedger {
    entries: Vec<(u32, &'static str)>,
}

impl CandidateLedger {
    pub fn new() -> CandidateLedger {
        CandidateLedger { entries: Vec::new() }
    }

    /// 登记候删项（同 F 编号重复登记忽略）。
    pub fn register(&mut self, f_id: u32, reason: &'static str) {
        if !self.registered(f_id) {
            self.entries.push((f_id, reason));
        }
    }

    pub fn registered(&self, f_id: u32) -> bool {
        self.entries.iter().any(|&(id, _)| id == f_id)
    }

    /// 候删五项齐。
    pub fn has_five(&self) -> bool {
        self.entries.len() >= 5
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// v1.0 冻结前置检查清单状态机（五件齐才过闸）。
pub struct FreezeChecklist {
    pub three_sources_ok: bool,
    pub segments_ok: bool,
    pub all_green_ok: bool,
    pub candidates_ok: bool,
    pub f200_ok: bool,
}

impl FreezeChecklist {
    pub fn new() -> FreezeChecklist {
        FreezeChecklist {
            three_sources_ok: false,
            segments_ok: false,
            all_green_ok: false,
            candidates_ok: false,
            f200_ok: false,
        }
    }

    pub fn ready(&self) -> bool {
        self.three_sources_ok
            && self.segments_ok
            && self.all_green_ok
            && self.candidates_ok
            && self.f200_ok
    }
}

// ---------------------------------------------------------------------------
// ④ F200 条款终版登记账
// ---------------------------------------------------------------------------

/// F200 终版登记（修订号 + 审视范围双锚——与基础件对表）。
pub struct F200FinalRecord {
    pub revision: u32,
    pub scope: (u32, u32),
}

impl F200FinalRecord {
    /// 终版：修订号 3，范围 F001-F600。
    pub fn final_version() -> F200FinalRecord {
        F200FinalRecord { revision: 3, scope: (1, TOTAL_ITEMS as u32) }
    }

    /// 与基础件登记值对表。
    pub fn matches_base(&self, reg: &IRegistry) -> bool {
        self.revision == reg.f200_revision && self.scope == (1, 600)
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f600_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 四段基线账：600 点全绿 → 四段计数恰为 200/200/150/50；注一红
    //    （F450 在第三段）→ 段缺即红，且基础件 all_green 同步翻红。
    let mut ck = [true; TOTAL_ITEMS];
    let mut reg1 = IRegistry::new();
    for i in 0..TOTAL_ITEMS {
        let _ = reg1.set_checkpoint(i, true);
    }
    let full = segment_complete(&segment_counts(&ck));
    ck[449] = false;
    let _ = reg1.set_checkpoint(449, false);
    cs.add(
        "segment baseline any gap red",
        full && !segment_complete(&segment_counts(&ck)) && !reg1.all_green(),
        "",
    );

    // 2) 摘要同源对账：账行与正文提取行摘要一致，基础件账册审计同绿。
    let mut reg2 = IRegistry::new();
    let lines: Vec<LedgerLine> = alloc::vec![
        LedgerLine { f_id: 551, title: "特权操作确认窗", checkpoint: 551 },
        LedgerLine { f_id: 552, title: "以管理员身份运行", checkpoint: 552 },
        LedgerLine { f_id: 600, title: "I 域收官登记", checkpoint: 600 },
    ];
    for l in &lines {
        let _ = reg2.register(*l);
    }
    let text: Vec<(u32, &'static str)> = alloc::vec![
        (551, "特权操作确认窗"),
        (552, "以管理员身份运行"),
        (600, "I 域收官登记"),
    ];
    let extracted = extract_from_text(&text);
    cs.add(
        "digest same source aligned",
        ledger_digest(&lines) == ledger_digest(&extracted) && reg2.audit_against_text(&text),
        "",
    );

    // 3) 摘要对账验伪：正文改一处标题——摘要当场对不上（改册不改账即红）。
    let drifted: Vec<(u32, &'static str)> = alloc::vec![
        (551, "特权确认窗（改过名）"),
        (552, "以管理员身份运行"),
        (600, "I 域收官登记"),
    ];
    cs.add(
        "digest drift detected",
        ledger_digest(&lines) != ledger_digest(&extract_from_text(&drifted)),
        "",
    );

    // 4) 候删五项登记账：五项在案、编号去重、未登记项可查。
    let mut cand = CandidateLedger::new();
    cand.register(123, "重复功能");
    cand.register(456, "无人使用");
    cand.register(789, "并入他项");
    cand.register(234, "已由 F100 覆盖");
    cand.register(345, "判据撤并");
    cand.register(123, "重复登记忽略");
    cs.add(
        "five candidates on record",
        cand.has_five() && cand.len() == 5 && !cand.registered(999),
        "",
    );

    // 5) 冻结前置状态机：五件齐才过闸；缺候删一件即不过。
    let mut reg5 = IRegistry::new();
    for fid in 401..=600u32 {
        let _ = reg5.register(LedgerLine { f_id: fid, title: "t", checkpoint: fid });
    }
    for i in 0..TOTAL_ITEMS {
        let _ = reg5.set_checkpoint(i, true);
    }
    let full_ck = [true; TOTAL_ITEMS];
    let text_200: alloc::vec::Vec<(u32, &'static str)> =
        (401..=600u32).map(|n| (n, "t")).collect();
    let mut gate = FreezeChecklist::new();
    gate.three_sources_ok = reg5.three_sources_aligned(&text_200);
    gate.segments_ok = segment_complete(&segment_counts(&full_ck));
    gate.all_green_ok = reg5.all_green();
    gate.f200_ok = F200FinalRecord::final_version().matches_base(&reg5);
    let partial = !gate.ready(); // 候删未挂
    gate.candidates_ok = cand.has_five();
    let ready = gate.ready() && reg5.freeze_gate(true, true, true, true, true);
    cs.add("freeze gate state machine", partial && ready, "");

    // 6) F200 条款终版登记账：修订号 3、范围 F001-F600 与基础件对表一致。
    let rec = F200FinalRecord::final_version();
    cs.add(
        "f200 final record anchored",
        rec.matches_base(&reg5) && rec.scope == (1, 600) && reg5.f200_clause().contains("F001-F600"),
        "",
    );

    // 7) 基础件契约不被深化破坏：600/200 基线常量与汇入去重语义原样。
    let mut reg7 = IRegistry::new();
    let squad: Vec<LedgerLine> = (401..=410u32)
        .map(|n| LedgerLine { f_id: n, title: "t", checkpoint: n })
        .collect();
    let first = reg7.merge(&squad);
    let second = reg7.merge(&squad);
    cs.add(
        "base contract kept",
        first == 10 && second == 0 && TOTAL_ITEMS == 600 && I_DOMAIN_ITEMS == 200,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_deterministic_and_field_sensitive() {
        assert_eq!(line_digest(551, "a"), line_digest(551, "a"));
        assert_ne!(line_digest(551, "a"), line_digest(552, "a"));
        assert_ne!(line_digest(551, "a"), line_digest(551, "b"));
    }

    #[test]
    fn empty_checkpoints_all_segments_red() {
        let ck = [false; TOTAL_ITEMS];
        assert_eq!(segment_counts(&ck), [0, 0, 0, 0]);
        assert!(!segment_complete(&segment_counts(&ck)));
    }

    #[test]
    fn candidates_below_five_not_ready() {
        let mut cand = CandidateLedger::new();
        cand.register(1, "r");
        cand.register(2, "r");
        cand.register(3, "r");
        cand.register(4, "r");
        assert!(!cand.has_five());
        cand.register(5, "r");
        assert!(cand.has_five());
    }
}
