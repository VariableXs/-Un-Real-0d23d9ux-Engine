//! UNX-F11059 外部对照面回归纳入协议（常态化清单）（0x1e760 段）
//!
//! 常态化纳入协议：外部对照面收尾后，16 项协议条目
//! （8 对照族 × 用例集/判据集两项）逐项立账并激活纳入，
//! 每项分配确定性周期槽（cadence 1..4）。清单闭合判据：
//! activated == 总项数（零未纳项），槽位两两互异且与
//! 闭环函数一致，激活后冻结不可撤项。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11059 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11059 - 11000) * 0x20;

/// 清单空 —— 激活前必须先立账
pub const NP_EMPTY: u32 = SEG_BASE + 0x01;
/// 条目号越界 —— 16 项编号 0..16
pub const NP_RANGE: u32 = SEG_BASE + 0x02;
/// 未立先激 —— 只有 registered 态可激活
pub const NP_NOREG: u32 = SEG_BASE + 0x03;
/// 重复激活 —— 一项只激活一次
pub const NP_DUP: u32 = SEG_BASE + 0x04;
/// 槽位冲突 —— 两项不得占用同一周期槽
pub const NP_SLOTS: u32 = SEG_BASE + 0x05;
/// 清单已冻结 —— freeze 后禁止再激活
pub const NP_FROZEN: u32 = SEG_BASE + 0x06;

/// 对照族数
pub const FAMILIES: usize = 8;
/// 每族协议项数
pub const ITEMS_PER_FAM: usize = 2;
/// 协议项总数
pub const ITEMS_TOTAL: usize = FAMILIES * ITEMS_PER_FAM;
/// 常态化清单标签
pub const PROTO_TAG: u64 = 0x6e70_726f_746f_304a;

/// 确定性槽位函数：项号 → 周期槽 1..16（两两互异）
pub fn slot_fn(no: usize) -> u32 {
    // 乘法置换：slot = (no*7+3) % 16 + 1，对 no∈0..16 两两互异
    ((no as u32).wrapping_mul(7).wrapping_add(3) % 16) + 1
}

/// 单条协议项
#[derive(Clone, Debug)]
pub struct ProtoItem {
    /// 项号（0x12000 段起编）
    pub pid: u32,
    /// 项名
    pub name: String,
    /// 所属族号
    pub fam: u8,
    /// 是否用例集项（否则为判据集项）
    pub is_cases: bool,
    /// 立账标记
    pub registered: bool,
    /// 激活标记
    pub activated: bool,
    /// 周期槽位（0 = 未分配）
    pub slot: u32,
}

/// 常态化清单总账
#[derive(Clone, Debug)]
pub struct ProtoLedger {
    /// 16 条协议项
    pub items: Vec<ProtoItem>,
    /// 已立账项数
    pub registered_n: usize,
    /// 已激活项数
    pub activated_n: usize,
    /// 槽位占用位图（bit0..15 对应槽 1..16）
    pub slot_mask: u32,
    /// 冻结标记
    pub frozen: bool,
}

impl ProtoLedger {
    /// 立账：16 项全部登记（未激活）
    pub fn new() -> ProtoLedger {
        let mut lg = ProtoLedger {
            items: Vec::new(),
            registered_n: 0, activated_n: 0, slot_mask: 0, frozen: false,
        };
        for fam in 0..FAMILIES {
            for half in 0..ITEMS_PER_FAM {
                let no = fam * ITEMS_PER_FAM + half;
                lg.items.push(ProtoItem {
                    pid: 0x12000 + no as u32,
                    name: "proto_item".to_string(),
                    fam: fam as u8,
                    is_cases: half == 0,
                    registered: true,
                    activated: false,
                    slot: 0,
                });
            }
        }
        lg.registered_n = ITEMS_TOTAL;
        lg
    }

    /// 激活一项：分配确定性槽位并检查槽位冲突
    pub fn activate(&mut self, no: usize) -> Result<u32, u32> {
        if self.frozen { return Err(NP_FROZEN); }
        if self.items.is_empty() { return Err(NP_EMPTY); }
        if no >= self.items.len() { return Err(NP_RANGE); }
        let it = &mut self.items[no];
        if !it.registered { return Err(NP_NOREG); }
        if it.activated { return Err(NP_DUP); }
        let slot = slot_fn(no);
        if self.slot_mask & (1u32 << (slot - 1)) != 0 {
            return Err(NP_SLOTS);
        }
        it.activated = true;
        it.slot = slot;
        self.slot_mask |= 1u32 << (slot - 1);
        self.activated_n += 1;
        Ok(slot)
    }

    /// 全量激活
    pub fn activate_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.items.len() {
            self.activate(no)?;
        }
        Ok(self.activated_n)
    }

    /// 撤项（仅未激活项可撤，负向验证用）
    pub fn retract(&mut self, no: usize) -> Result<(), u32> {
        if no >= self.items.len() { return Err(NP_RANGE); }
        if self.items[no].activated { return Err(NP_DUP); }
        self.items[no].registered = false;
        self.registered_n -= 1;
        Ok(())
    }

    /// 按族统计激活数
    pub fn activated_fam(&self, fam: u8) -> usize {
        self.items.iter().filter(|it| it.fam == fam && it.activated).count()
    }

    /// 清单闭合：16/16 激活且槽位全占
    pub fn closed(&self) -> bool {
        self.activated_n == ITEMS_TOTAL
            && self.slot_mask == 0xffff
            && self.registered_n == ITEMS_TOTAL
    }

    /// 槽位闸：存在重复占用即 NP_SLOTS
    pub fn slot_gate(&self) -> Result<(), u32> {
        let mut seen = 0u32;
        for it in &self.items {
            if it.activated {
                if seen & (1u32 << (it.slot - 1)) != 0 {
                    return Err(NP_SLOTS);
                }
                seen |= 1u32 << (it.slot - 1);
            }
        }
        Ok(())
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PROTO_TAG;
        for it in &self.items {
            h = (h ^ (it.pid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (it.slot as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (it.activated as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ (self.slot_mask as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结清单
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：16 项全激活清单闭合
pub fn sample_activated() -> (ProtoLedger, bool) {
    let mut lg = ProtoLedger::new();
    let _ = lg.activate_all();
    let ok = lg.closed();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：重复激活 / 撤项后再激活 / 越界 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = ProtoLedger::new();
    // 注入 1：重复激活 → NP_DUP 拒止
    let _ = lg.activate(0);
    let i1 = matches!(lg.activate(0), Err(NP_DUP));
    // 注入 2：撤项后再激活 → NP_NOREG 拒止
    let _ = lg.retract(1);
    let i2 = matches!(lg.activate(1), Err(NP_NOREG));
    // 注入 3：项号越界 → NP_RANGE 拒止
    let i3 = matches!(lg.activate(ITEMS_TOTAL), Err(NP_RANGE));
    // 注入 4：冻结后激活 → NP_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.activate(2), Err(NP_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 纳入协议逐项立账，常态化清单闭合
pub fn run_unx11059_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11059");
    let (lg, ok) = sample_activated();

    s.add("J1-逐项激活", ok && lg.activated_n == ITEMS_TOTAL, "16 项协议条目逐项激活纳入");
    s.add("J1-清单闭合", lg.closed() && lg.slot_gate().is_ok(), "清单闭合零未纳项槽位全占");
    s.add("J1-立账齐备", lg.registered_n == ITEMS_TOTAL, "16 项全部立账");
    s.add("J2-槽位互异", lg.slot_mask == 0xffff, "16 槽位两两互异全占位图");
    s.add("J2-槽位闭环", (0..ITEMS_TOTAL).all(|no| slot_fn(no) >= 1 && slot_fn(no) <= 16), "槽位函数闭环取值 1..16");
    s.add("J2-槽账一致", lg.items.iter().all(|it| !it.activated || it.slot == slot_fn((it.pid - 0x12000) as usize)), "激活槽位与闭环函数一致");
    s.add("J3-族覆盖", (0..FAMILIES as u8).all(|f| lg.activated_fam(f) == ITEMS_PER_FAM), "八族每族 2/2 项激活");
    s.add("J3-双项齐备", lg.items.iter().filter(|it| it.is_cases).count() == FAMILIES, "用例集项 8 条齐备");
    s.add("J3-编号连续", lg.items[0].pid == 0x12000 && lg.items[ITEMS_TOTAL - 1].pid == 0x1200F, "项号 0x12000 段起编连续");
    s.add("J4-重复拒激", inject_four()[0], "重复激活被 NP_DUP 拒止");
    s.add("J4-未立拒激", inject_four()[1], "撤项后激活被 NP_NOREG 拒止");
    s.add("J5-越界拒激", inject_four()[2], "项号越界被 NP_RANGE 拒止");
    s.add("J5-冻结拒激", inject_four()[3], "冻结后激活被 NP_FROZEN 拒止");
    s.add("J6-空账拒激", ProtoLedger { items: Vec::new(), registered_n: 0, activated_n: 0, slot_mask: 0, frozen: false }.activate(0).is_err(), "空账激活被 NP_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-协议动态",
        ok,
        &format!("激活 {}/{}，槽位图 {:#x}，fold={:#x}", lg.activated_n, ITEMS_TOTAL, lg.slot_mask, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11059_routine_protocol_flow() {
        let mut lg = ProtoLedger::new();
        // 全量激活
        assert_eq!(lg.activate_all().unwrap_or(0), ITEMS_TOTAL);
        assert!(lg.closed());
        assert_eq!(lg.activated_n, ITEMS_TOTAL);
        // 槽位互异抽查
        assert_eq!(slot_fn(0), 4);
        assert_ne!(slot_fn(0), slot_fn(1));
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11059_checks_green() {
        let s = run_unx11059_checks();
        assert!(s.all_passed(), "UNX-F11059 判据未全绿");
    }
}
