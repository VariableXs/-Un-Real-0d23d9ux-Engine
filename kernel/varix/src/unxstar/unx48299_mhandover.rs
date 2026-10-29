//! unx48299_mhandover — UNX-F48299 M 段交接预告件（B16–B40 续产锚点清单与冻结面状态登记）（诊断段 0xC640）。
//!
//! 判据原文：
//!   J1 锚点清单入册零悬空（全部挂计划或已冻结）；判据锚定 §52.9 同源防悬空口径。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 锚点清单：B16–B40 共 25 个续产锚点，每个锚点必须恰好处于
//!      两种合法状态之一——已挂计划（Planned，带计划号引用）或已冻结
//!      （Frozen，带封印号引用）；无状态、双状态、状态外值皆为悬空。
//!   2. 引用闭合（防悬空）：Planned 的 plan_id 必须落在已登记计划表
//!      内、Frozen 的 seal_id 必须落在已登记封印表内——引用表外号即
//!      悬空（§52.9 同源口径：引用必须可回溯到源表）。
//!   3. 计划表/封印表：两表一次登记只读（计划号 0..P、封印号 0..S），
//!      锚点引用域以表长为界；表空时 Planned 引用必悬空。
//!   4. 清单口径（J1）：25 锚点全部入册、状态合法、引用闭合、锚号
//!      连续无缺——四项全过才零悬空；悬空锚点逐条留痕（锚号+悬因）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC640;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DANGLE: u16 = SEG_BASE + 1;    // 悬空锚点
const E_STATE_BAD: u16 = SEG_BASE + 2; // 锚点状态非法
const E_ANCHOR_MISS: u16 = SEG_BASE + 3;// 清单不齐（25 缺）
const E_ANCHOR_DUP: u16 = SEG_BASE + 4;// 锚号重复
const E_TABLE_BAD: u16 = SEG_BASE + 5; // 计划/封印表未登记或越域
const E_LEDGER_BAD: u16 = SEG_BASE + 6;// 交接账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 锚点总数（B16..B40）。
pub const ANCHOR_COUNT: usize = 25;
/// 起始批号。
pub const ANCHOR_BASE: u32 = 16;

/// 锚点状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    Planned, // 挂计划：plan_id 有效
    Frozen,  // 已冻结：seal_id 有效
}

/// 一条锚点。
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub anchor_no: u32,   // 批号 16..40
    pub state: AnchorState,
    pub plan_id: u32, // Planned 时有效
    pub seal_id: u32, // Frozen 时有效
}

/// 一条悬空账。
#[derive(Clone, Copy, Debug)]
pub struct DangleEntry {
    pub anchor_no: u32,
    pub reason: u16,
}

/// 交接预告枢纽：计划表/封印表登记 + 锚点清单 + 防悬空审计。
pub struct MHandover {
    plans: Vec<&'static str>,  // 计划表（计划名，下标即 plan_id）
    seals: Vec<&'static str>,  // 封印表
    anchors: Vec<Anchor>,
    dangles: Vec<DangleEntry>,
    pub anchor_total: u32,
}

impl MHandover {
    pub fn new() -> MHandover {
        MHandover {
            plans: Vec::new(),
            seals: Vec::new(),
            anchors: Vec::new(),
            dangles: Vec::new(),
            anchor_total: 0,
        }
    }

    /// 登记计划表（一次登记只读，重复登记拒）。
    pub fn register_plans(&mut self, names: &[&'static str]) -> Result<(), u16> {
        if !self.plans.is_empty() {
            return Err(E_TABLE_BAD);
        }
        if names.is_empty() {
            return Err(E_TABLE_BAD);
        }
        let mut i = 0usize;
        while i < names.len() {
            self.plans.push(names[i]);
            i += 1;
        }
        Ok(())
    }

    /// 登记封印表（同上）。
    pub fn register_seals(&mut self, names: &[&'static str]) -> Result<(), u16> {
        if !self.seals.is_empty() {
            return Err(E_TABLE_BAD);
        }
        if names.is_empty() {
            return Err(E_TABLE_BAD);
        }
        let mut i = 0usize;
        while i < names.len() {
            self.seals.push(names[i]);
            i += 1;
        }
        Ok(())
    }

    /// 挂计划：锚点入册为 Planned（锚号连续、plan_id 引用闭合才收）。
    pub fn plan_anchor(&mut self, anchor_no: u32, plan_id: u32) -> Result<(), u16> {
        self.add_anchor(anchor_no, AnchorState::Planned, plan_id, 0)
    }

    /// 冻结锚点：入册为 Frozen（seal_id 引用闭合才收）。
    pub fn freeze_anchor(&mut self, anchor_no: u32, seal_id: u32) -> Result<(), u16> {
        self.add_anchor(anchor_no, AnchorState::Frozen, 0, seal_id)
    }

    fn add_anchor(
        &mut self,
        anchor_no: u32,
        state: AnchorState,
        plan_id: u32,
        seal_id: u32,
    ) -> Result<(), u16> {
        // 锚号域：ANCHOR_BASE..ANCHOR_BASE+ANCHOR_COUNT。
        if anchor_no < ANCHOR_BASE || anchor_no >= ANCHOR_BASE + ANCHOR_COUNT as u32 {
            return Err(E_ANCHOR_MISS);
        }
        // 锚号唯一。
        let mut i = 0usize;
        while i < self.anchors.len() {
            if self.anchors[i].anchor_no == anchor_no {
                return Err(E_ANCHOR_DUP);
            }
            i += 1;
        }
        // 引用闭合（防悬空的入册侧拦截）。
        match state {
            AnchorState::Planned => {
                if plan_id as usize >= self.plans.len() {
                    return Err(E_DANGLE);
                }
            }
            AnchorState::Frozen => {
                if seal_id as usize >= self.seals.len() {
                    return Err(E_DANGLE);
                }
            }
        }
        self.anchors.push(Anchor { anchor_no, state, plan_id, seal_id });
        self.anchor_total += 1;
        Ok(())
    }

    /// 防悬空审计（J1 主体）：清单齐 25、状态合法、引用逐条闭合、
    /// 锚号连续无缺；悬空逐条留痕。
    pub fn audit_dangling(&mut self) -> Result<(), u16> {
        self.dangles.clear();
        if self.anchors.len() != ANCHOR_COUNT {
            // 缺锚即悬空（清单口径：未入册的锚也是悬空）。
            let mut no = ANCHOR_BASE;
            while no < ANCHOR_BASE + ANCHOR_COUNT as u32 {
                let mut found = false;
                let mut i = 0usize;
                while i < self.anchors.len() {
                    if self.anchors[i].anchor_no == no {
                        found = true;
                        break;
                    }
                    i += 1;
                }
                if !found {
                    self.dangles.push(DangleEntry { anchor_no: no, reason: E_ANCHOR_MISS });
                }
                no += 1;
            }
            if !self.dangles.is_empty() {
                return Err(E_DANGLE);
            }
            return Err(E_ANCHOR_MISS);
        }
        // 状态与引用逐条闭合。
        let mut i = 0usize;
        while i < self.anchors.len() {
            let a = &self.anchors[i];
            match a.state {
                AnchorState::Planned => {
                    if a.plan_id as usize >= self.plans.len() {
                        self.dangles.push(DangleEntry { anchor_no: a.anchor_no, reason: E_DANGLE });
                    }
                }
                AnchorState::Frozen => {
                    if a.seal_id as usize >= self.seals.len() {
                        self.dangles.push(DangleEntry { anchor_no: a.anchor_no, reason: E_DANGLE });
                    }
                }
            }
            i += 1;
        }
        if !self.dangles.is_empty() {
            return Err(E_DANGLE);
        }
        // 锚号连续无缺。
        let mut no = ANCHOR_BASE;
        while no < ANCHOR_BASE + ANCHOR_COUNT as u32 {
            let idx = (no - ANCHOR_BASE) as usize;
            if self.anchors[idx].anchor_no != no {
                return Err(E_ANCHOR_MISS);
            }
            no += 1;
        }
        Ok(())
    }

    /// ×N 覆盖核验。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.anchor_total < want {
            return Err(E_ANCHOR_MISS);
        }
        Ok(())
    }

    /// 状态分布核对：Planned 恰 want_p、Frozen 恰 want_f。
    pub fn audit_distribution(&self, want_p: u32, want_f: u32) -> Result<(), u16> {
        let mut p = 0u32;
        let mut f = 0u32;
        let mut i = 0usize;
        while i < self.anchors.len() {
            match self.anchors[i].state {
                AnchorState::Planned => p += 1,
                AnchorState::Frozen => f += 1,
            }
            i += 1;
        }
        if p != want_p || f != want_f {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    pub fn dangles_len(&self) -> usize {
        self.dangles.len()
    }
    pub fn anchors_len(&self) -> usize {
        self.anchors.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.anchor_total != ANCHOR_COUNT as u32 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（锚点清单入册零悬空），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主入册：8 计划 + 17 封印两表登记 → 25 锚点（B16..B31 挂计划、
    //    B32..B40 已冻结）→ 防悬空审计全绿。
    let mut mh = MHandover::new();
    let plans: [&'static str; 8] = ["P16", "P17", "P18", "P19", "P20", "P21", "P22", "P23"];
    let seals: [&'static str; 17] = [
        "S24", "S25", "S26", "S27", "S28", "S29", "S30", "S31", "S32",
        "S33", "S34", "S35", "S36", "S37", "S38", "S39", "S40",
    ];
    mh.register_plans(&plans)?;
    mh.register_seals(&seals)?;
    // B16..B31 挂计划（16 个），B32..B40 已冻结（9 个）。
    let mut no = 16u32;
    while no < 32 {
        mh.plan_anchor(no, no - 16)?;
        no += 1;
    }
    let mut no = 32u32;
    while no < 41 {
        mh.freeze_anchor(no, no - 32)?;
        no += 1;
    }
    mh.coverage(25)?;
    mh.audit_dangling()?;
    mh.audit_distribution(16, 9)?;
    if mh.anchors_len() != 25 {
        return Err(E_ANCHOR_MISS);
    }
    hits += 1;

    // R2 悬空抓红：计划表未登记时挂计划必拒（引用无源）。
    let mut mh = MHandover::new();
    if mh.plan_anchor(16, 0).is_ok() {
        return Err(E_DANGLE);
    }
    if mh.freeze_anchor(32, 0).is_ok() {
        return Err(E_DANGLE);
    }
    if mh.anchor_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 引用越表拒收：plan_id=8 越出 8 长计划表必拒；seal_id=17 越出
    //    17 长封印表必拒。
    let mut mh = MHandover::new();
    let plans: [&'static str; 8] = ["P16", "P17", "P18", "P19", "P20", "P21", "P22", "P23"];
    let seals: [&'static str; 17] = [
        "S24", "S25", "S26", "S27", "S28", "S29", "S30", "S31", "S32",
        "S33", "S34", "S35", "S36", "S37", "S38", "S39", "S40",
    ];
    mh.register_plans(&plans)?;
    mh.register_seals(&seals)?;
    if mh.plan_anchor(16, 8).is_ok() {
        return Err(E_DANGLE);
    }
    if mh.freeze_anchor(32, 17).is_ok() {
        return Err(E_DANGLE);
    }
    if mh.anchor_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 锚号域与唯一性：B15/B41 越域拒、同锚号二挂拒。
    let mut mh = MHandover::new();
    let plans: [&'static str; 2] = ["P16", "P17"];
    mh.register_plans(&plans)?;
    mh.plan_anchor(16, 0)?;
    if mh.plan_anchor(15, 0).is_ok() {
        return Err(E_ANCHOR_MISS);
    }
    if mh.plan_anchor(41, 1).is_ok() {
        return Err(E_ANCHOR_MISS);
    }
    if mh.plan_anchor(16, 1).is_ok() {
        return Err(E_ANCHOR_DUP);
    }
    if mh.anchor_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 缺锚清单抓红：只入 24 锚，audit_dangling 必红且悬空账指认
    //    缺席锚号（未入册的锚也是悬空——清单口径）。
    let mut mh = MHandover::new();
    let plans: [&'static str; 24] = [
        "P16", "P17", "P18", "P19", "P20", "P21", "P22", "P23",
        "P24", "P25", "P26", "P27", "P28", "P29", "P30", "P31",
        "P32", "P33", "P34", "P35", "P36", "P37", "P38", "P39",
    ];
    mh.register_plans(&plans)?;
    mh.register_seals(&["S40"])?;
    let mut no = 16u32;
    while no < 40 {
        mh.plan_anchor(no, no - 16)?;
        no += 1;
    }
    // B40 未入册。
    if mh.audit_dangling().is_ok() {
        return Err(E_DANGLE);
    }
    if mh.dangles_len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 双表重复登记拒：二次 register_plans/register_seals 全拒。
    let mut mh = MHandover::new();
    mh.register_plans(&["P16"])?;
    mh.register_seals(&["S40"])?;
    if mh.register_plans(&["PX"]).is_ok() {
        return Err(E_TABLE_BAD);
    }
    if mh.register_seals(&["SX"]).is_ok() {
        return Err(E_TABLE_BAD);
    }
    hits += 1;

    // R7 空表登记拒：空计划表/空封印表一次登记即拒（表必须有源）。
    let mut mh = MHandover::new();
    if mh.register_plans(&[]).is_ok() {
        return Err(E_TABLE_BAD);
    }
    if mh.register_seals(&[]).is_ok() {
        return Err(E_TABLE_BAD);
    }
    hits += 1;

    // R8 零悬空终态与写回（判据 J2）：R1 段复演后写回断言
    // （空摘要与清单不齐写回拒）。
    let mut mh = MHandover::new();
    let plans: [&'static str; 8] = ["P16", "P17", "P18", "P19", "P20", "P21", "P22", "P23"];
    let seals: [&'static str; 17] = [
        "S24", "S25", "S26", "S27", "S28", "S29", "S30", "S31", "S32",
        "S33", "S34", "S35", "S36", "S37", "S38", "S39", "S40",
    ];
    mh.register_plans(&plans)?;
    mh.register_seals(&seals)?;
    if mh.writeback(true, 0x10).is_ok() {
        return Err(E_LEDGER_BAD); // 清单不齐不得写回
    }
    let mut no = 16u32;
    while no < 32 {
        mh.plan_anchor(no, no - 16)?;
        no += 1;
    }
    let mut no = 32u32;
    while no < 41 {
        mh.freeze_anchor(no, no - 32)?;
        no += 1;
    }
    mh.audit_dangling()?;
    if mh.dangles_len() != 0 {
        return Err(E_DANGLE);
    }
    if mh.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    mh.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
