//! veb297_mliwkv —— VE-F0297 Mali 规避表。
//! 锚点（CoRun Unxreal 主册）：VE-F0297 · Mali 规避表 · 目标 300 行
//! 判据原文：VE-F0297-J1 规避表账：问题号登记（缺陷号 + 波
//!           及代际集 + 规避动作）；命中判定（当前代 ∈ 波及
//!           集 → 命中）；动作分发（命中 → 返回规避动作码）；
//!           代际豁免（不在波及集 → 豁免直通）；规避成本标
//!           注（每动作带成本 0~3）；成本合计核（当前代命
//!           中的规避总成本）；表容上限（64 条，满拒）；重号
//!           拒（缺陷号唯一）。
//! 组件：缺陷账 + 波及集 + 动作分发 + 成本账。
//! 职责定位：规避表是「Mali 的病历与药方」——波及集是传染
//!           范围：出了范围的代不用吃药；动作带成本是药价：
//!           好药贵，吃之前算总账；重号拒是病历的编号规则：
//!           一个缺陷一份病历。
//! 性能逐项分解：命中 O(条数)；合计 O(命中)。
//! 诊断码段：0x31340（0x31341~0x31349 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const WK_VERSION: &str = "veb297_mliwkv-v1";

/// 诊断段基址（0x31340 段 · 本模块独占）。
pub const WK_SEG_BASE: u32 = 0x31340;
pub const XC_MAGIC: u32 = 0x31341; // 账魔数破
pub const XC_DUP: u32 = 0x31342; // 缺陷号重
pub const XC_FULL: u32 = 0x31343; // 表满
pub const XC_COST: u32 = 0x31344; // 成本越界

/// 账魔数。
const MAGIC: u32 = 0x3134_1BC8;

/// 表容。
pub const WK_CAP: usize = 64;

/// 规避条目。
pub struct WkRow {
    /// 缺陷号。
    pub bug: u32,
    /// 波及代际位集（bit i = 代 i 受波及）。
    pub gens: u32,
    /// 规避动作码。
    pub action: u32,
    /// 成本 0~3。
    pub cost: u32,
}

/// 规避表。
pub struct WkTab {
    rows: Vec<WkRow>,
    magic: u32,
}

impl WkTab {
    /// 建。
    pub fn new() -> WkTab {
        WkTab { rows: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记。
    pub fn add(&mut self, bug: u32, gens: u32, action: u32, cost: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.rows.len() >= WK_CAP {
            return Err(XC_FULL);
        }
        if cost > 3 {
            return Err(XC_COST);
        }
        for r in self.rows.iter() {
            if r.bug == bug {
                return Err(XC_DUP);
            }
        }
        self.rows.push(WkRow { bug, gens, action, cost });
        Ok(())
    }

    /// 命中判定：当前代是否在任何条的波及集。
    pub fn hit(&self, gen: u32) -> bool {
        for r in self.rows.iter() {
            if gen < 32 && (r.gens >> gen) & 1 == 1 {
                return true;
            }
        }
        false
    }

    /// 动作分发：返回命中的全部动作码（登记序）。
    pub fn actions(&self, gen: u32) -> Vec<u32> {
        let mut v = Vec::new();
        for r in self.rows.iter() {
            if gen < 32 && (r.gens >> gen) & 1 == 1 {
                v.push(r.action);
            }
        }
        v
    }

    /// 命中条数。
    pub fn hit_count(&self, gen: u32) -> u32 {
        let mut n = 0;
        for r in self.rows.iter() {
            if gen < 32 && (r.gens >> gen) & 1 == 1 {
                n += 1;
            }
        }
        n
    }

    /// 成本合计：当前代命中动作的成本和。
    pub fn cost_total(&self, gen: u32) -> u32 {
        let mut t = 0;
        for r in self.rows.iter() {
            if gen < 32 && (r.gens >> gen) & 1 == 1 {
                t += r.cost;
            }
        }
        t
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

pub fn run_veb297_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0297");
    let mut t = WkTab::new();
    // 族零：登记门。
    s.add("成本 4 拒", t.add(1, 0b11, 10, 4) == Err(XC_COST), ">3");
    s.add("登 1", t.add(1, 0b011, 10, 1).is_ok(), "代 0,1");
    s.add("重号拒", t.add(1, 0b100, 20, 0) == Err(XC_DUP), "缺一号一病历");
    s.add("登 2", t.add(2, 0b100, 21, 2).is_ok(), "代 2");
    s.add("登 3", t.add(3, 0b101, 22, 3).is_ok(), "代 0,2");
    s.add("表 3 条", t.len() == 3, "三病历");
    // 族一：命中判定。
    s.add("代 0 命中", t.hit(0), "1+3 波及");
    s.add("代 1 命中", t.hit(1), "1 波及");
    s.add("代 2 命中", t.hit(2), "2+3 波及");
    s.add("代 3 豁免", t.hit(3) == false, "无人波及");
    s.add("代 31 豁免", t.hit(31) == false, "远代");
    // 族二：动作分发。
    let a0 = t.actions(0);
    s.add("代 0 动作 2", a0.len() == 2 && a0[0] == 10 && a0[1] == 22, "登记序");
    let a2 = t.actions(2);
    s.add("代 2 动作 2", a2.len() == 2 && a2[0] == 21 && a2[1] == 22, "序核");
    s.add("代 3 无动作", t.actions(3).is_empty(), "豁免直通");
    // 族三：命中计数。
    s.add("代 0 计 2", t.hit_count(0) == 2, "两条");
    s.add("代 1 计 1", t.hit_count(1) == 1, "一条");
    s.add("代 3 计 0", t.hit_count(3) == 0, "零条");
    // 族四：成本合计。
    s.add("代 0 成本 4", t.cost_total(0) == 4, "1+3");
    s.add("代 1 成本 1", t.cost_total(1) == 1, "单条");
    s.add("代 2 成本 5", t.cost_total(2) == 5, "2+3");
    s.add("代 3 成本 0", t.cost_total(3) == 0, "不吃药");
    // 族五：表容。
    let mut f = WkTab::new();
    let mut full_ok = true;
    for i in 0..WK_CAP as u32 {
        if f.add(i, 1 << (i % 32), i, 0).is_err() {
            full_ok = false;
        }
    }
    s.add("满表 64", full_ok && f.len() == WK_CAP, "定容");
    s.add("65 条拒", f.add(999, 1, 1, 0) == Err(XC_FULL), "溢出");
    s.add("魔数核", WK_SEG_BASE == 0x31340, "段基址");
    s
}
