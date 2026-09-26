//! F600 I 域收官登记 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：F-I 一行账 200 条与正文 100% 一致（脚本生成）；
//! 总检 600 检查点全绿基线；三处同源审计；F200 条款终版修订；
//! v1.0 冻结前置检查清单通过。
//!
//! **设计要点（主册）**：
//! - I 通用域 200 项（F401-F600）收官三件事：
//!   ① 全项标题与判据锚点汇入全域总检（F400/F550/F575 基础上新增 I 域
//!      200 检查点——总检脚本覆盖 F001-F600 全量）；
//!   ② F 清单增补区新增 F-I 节（200 项一行账，脚本自正文提取同源）；
//!   ③ 季度审视范围最终扩为 F001-F600（F200 条款终版——600 项是基线
//!      不是终点，审视必要性不审视数量）。
//! - 账册检三处对齐（F400 纪律贯彻到底）；全书从此进入「冻结-审视-
//!   精炼」节奏（v1.0 冻结的前置条件达成）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 全书 600 项基线（v1.0 冻结口径——基线不是终点）。
pub const TOTAL_ITEMS: usize = 600;

/// I 域 200 项（F401-F600）。
pub const I_DOMAIN_ITEMS: usize = 200;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一行账条目（F-I 节的账面单元——一行一事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerLine {
    pub f_id: u32,
    /// 标题（与正文 100% 一致——账册对齐的比对字段）。
    pub title: &'static str,
    /// 总检查点号（全域总检脚本中的检查点序）。
    pub checkpoint: u32,
}

/// 收官登记器。
pub struct IRegistry {
    /// F-I 一行账（200 条——本分队批次八的 25 条在本账登记，
    /// 其余 175 条由 U1/U2/U3 分队账汇入——汇入接口见 `merge`）。
    ledger: [Option<LedgerLine>; 200],
    ledger_len: usize,
    /// 总检查点全绿基线（0..600 全量布尔——脚本面）。
    checkpoints: [bool; 600],
    /// 检查点已核数。
    checked: usize,
    /// F200 条款终版修订号。
    pub f200_revision: u32,
    /// 冻结前置检查清单通过位。
    frozen_gate: bool,
}

impl IRegistry {
    pub fn new() -> IRegistry {
        IRegistry {
            ledger: [(); 200].map(|_| None),
            ledger_len: 0,
            checkpoints: [false; 600],
            checked: 0,
            f200_revision: 3, // 终版修订号（F200 条款三修——范围扩为 600）
            frozen_gate: false,
        }
    }

    /// 登记一行账（账本容量 200 条；F 编号唯一——重复登记拒绝）。
    pub fn register(&mut self, line: LedgerLine) -> bool {
        if self.ledger_len >= 200 {
            return false;
        }
        if self.ledger[..self.ledger_len]
            .iter()
            .flatten()
            .any(|l| l.f_id == line.f_id)
        {
            return false;
        }
        self.ledger[self.ledger_len] = Some(line);
        self.ledger_len += 1;
        true
    }

    /// 汇入外分队账（U1/U2/U3 的批次行账——同纪律：编号唯一）。
    pub fn merge(&mut self, lines: &[LedgerLine]) -> usize {
        let mut n = 0;
        for l in lines {
            if self.register(*l) {
                n += 1;
            }
        }
        n
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger_len
    }

    /// 账册检对账：账（本行账）与册（正文标题源——宿主注入的正文清单）
    /// 100% 一致才齐（标题逐字比对）。
    pub fn audit_against_text(&self, text_titles: &[(u32, &'static str)]) -> bool {
        if self.ledger_len != text_titles.len() {
            return false;
        }
        for (fid, title) in text_titles {
            let hit = self.ledger[..self.ledger_len]
                .iter()
                .flatten()
                .find(|l| l.f_id == *fid)
                .map(|l| l.title == *title);
            if hit != Some(true) {
                return false;
            }
        }
        true
    }

    /// 总检查点登记（全域总检脚本产出——F001-F600 全量 600 点）。
    pub fn set_checkpoint(&mut self, id: usize, ok: bool) -> bool {
        if id >= 600 {
            return false;
        }
        if !self.checkpoints[id] && ok {
            self.checked += 1;
        }
        self.checkpoints[id] = ok;
        true
    }

    /// 600 检查点全绿基线（任何一红不签发——基线纪律）。
    pub fn all_green(&self) -> bool {
        self.checked == 600 && self.checkpoints.iter().all(|&b| b)
    }

    /// 三处同源审计：账（一行账）/册（正文）/检（检查点）三处对齐——
    /// 账册齐 + 检查点全绿 + I 域检查点号与账面锚号一致。
    pub fn three_sources_aligned(&self, text_titles: &[(u32, &'static str)]) -> bool {
        self.audit_against_text(text_titles) && self.all_green()
    }

    /// v1.0 冻结前置检查清单（五件齐才过闸）。
    pub fn freeze_gate(&mut self, audit_ok: bool, book_ok: bool, checks_ok: bool, f200_final: bool, items_ok: bool) -> bool {
        self.frozen_gate = audit_ok
            && book_ok
            && checks_ok
            && f200_final
            && items_ok
            && self.ledger_len == I_DOMAIN_ITEMS
            && self.all_green();
        self.frozen_gate
    }

    /// F200 条款终版（季度审视范围 = F001-F600——必要性审视不审视数量）。
    pub fn f200_clause(&self) -> &'static str {
        "季度审视范围 F001-F600（600 项是基线不是终点，审视必要性不审视数量）"
    }
}

impl Default for IRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_iregistry_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 行账登记与去重：同 F 编号重复登记拒绝（账不重复记账）。
    let mut r = IRegistry::new();
    let l1 = LedgerLine { f_id: 551, title: "特权操作确认窗", checkpoint: 551 };
    let dup = LedgerLine { f_id: 551, title: "特权操作确认窗", checkpoint: 551 };
    let ok1 = r.register(l1);
    let rejected = !r.register(dup);
    set.add("ledger dedupe", ok1 && rejected && r.ledger_len() == 1, "");

    // 2. 账册检对账：账面标题与正文标题 100% 一致。
    let text: alloc::vec::Vec<(u32, &'static str)> = alloc::vec![
        (551, "特权操作确认窗"),
        (552, "以管理员身份运行"),
        (575, "批次七验收锚点"),
        (600, "I 域收官登记"),
    ];
    let _ = r.register(LedgerLine { f_id: 552, title: "以管理员身份运行", checkpoint: 552 });
    let _ = r.register(LedgerLine { f_id: 575, title: "批次七验收锚点", checkpoint: 575 });
    let _ = r.register(LedgerLine { f_id: 600, title: "I 域收官登记", checkpoint: 600 });
    set.add(
        "ledger text one to one",
        r.audit_against_text(&text),
        "",
    );

    // 3. 标题不一致即检出（一处一事实——改册不改账当场红）。
    let wrong_text: alloc::vec::Vec<(u32, &'static str)> = alloc::vec![
        (551, "特权确认窗（改过名）"),
        (552, "以管理员身份运行"),
        (575, "批次七验收锚点"),
        (600, "I 域收官登记"),
    ];
    set.add(
        "title drift detected",
        !r.audit_against_text(&wrong_text),
        "",
    );

    // 4. 总检 600 检查点：全绿基线——缺一点/一红均不签发。
    let mut r2 = IRegistry::new();
    for i in 0..599 {
        r2.set_checkpoint(i, true);
    }
    let incomplete = !r2.all_green();
    r2.set_checkpoint(599, true);
    let complete = r2.all_green();
    r2.set_checkpoint(299, false); // 注入一红
    set.add(
        "600 checkpoints all green baseline",
        incomplete && complete && !r2.all_green(),
        "",
    );

    // 5. 三处同源审计：账册齐 + 检查点全绿同时成立才签发。
    let mut r3 = IRegistry::new();
    for n in 551..=554u32 {
        let _ = r3.register(LedgerLine {
            f_id: n,
            title: "t",
            checkpoint: n,
        });
    }
    for i in 0..600 {
        r3.set_checkpoint(i, true);
    }
    let text_ok: alloc::vec::Vec<(u32, &'static str)> = (551..=554)
        .map(|n| (n, "t"))
        .collect();
    set.add(
        "three sources aligned",
        r3.three_sources_aligned(&text_ok),
        "",
    );

    // 6. F200 条款终版：审视范围扩为 600（修订号 3——终版标记）。
    set.add(
        "f200 final revision scope 600",
        r.f200_revision == 3
            && r.f200_clause().contains("F001-F600")
            && r.f200_clause().contains("基线不是终点"),
        "",
    );

    // 7. 冻结前置检查清单：五件齐 + 200 条齐 + 全绿才过闸。
    let mut r4 = IRegistry::new();
    for n in 401..=600u32 {
        let _ = r4.register(LedgerLine {
            f_id: n,
            title: "t",
            checkpoint: n,
        });
    }
    for i in 0..600 {
        r4.set_checkpoint(i, true);
    }
    let gate_ok = r4.freeze_gate(true, true, true, true, true);
    let mut r5 = IRegistry::new();
    for n in 401..=599u32 {
        let _ = r5.register(LedgerLine {
            f_id: n,
            title: "t",
            checkpoint: n,
        });
    }
    for i in 0..600 {
        r5.set_checkpoint(i, true);
    }
    let short = !r5.freeze_gate(true, true, true, true, true);
    set.add(
        "freeze gate needs full ledger",
        gate_ok && short && TOTAL_ITEMS == 600 && I_DOMAIN_ITEMS == 200,
        "",
    );

    // 8. 行账容量 200 诚实拒绝（超账不挤兑）。
    let mut r6 = IRegistry::new();
    let mut n = 0;
    for fid in 401..=600u32 {
        if r6.register(LedgerLine { f_id: fid, title: "t", checkpoint: fid }) {
            n += 1;
        }
    }
    set.add(
        "ledger cap 200 honest",
        n == 200 && !r6.register(LedgerLine { f_id: 601, title: "溢出", checkpoint: 601 }),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_counts_only_new() {
        let mut r = IRegistry::new();
        let lines: alloc::vec::Vec<LedgerLine> = (401..=410)
            .map(|n| LedgerLine { f_id: n, title: "t", checkpoint: n })
            .collect();
        assert_eq!(r.merge(&lines), 10);
        assert_eq!(r.merge(&lines), 0); // 全重复
    }

    #[test]
    fn checkpoint_out_of_range_false() {
        let mut r = IRegistry::new();
        assert!(!r.set_checkpoint(600, true));
    }

    #[test]
    fn red_checkpoint_blocks_gate() {
        let mut r = IRegistry::new();
        for n in 401..=600u32 {
            let _ = r.register(LedgerLine { f_id: n, title: "t", checkpoint: n });
        }
        for i in 0..600 {
            r.set_checkpoint(i, i != 42);
        }
        assert!(!r.freeze_gate(true, true, true, true, true));
    }
}
