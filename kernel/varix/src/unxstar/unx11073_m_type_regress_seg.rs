//! UNX-F11073 M 型回归段（0x1e920 段）
//!
//! B09–B20 机制主体判据在此重放：每行登记一个机制 (mech, 批次, 入口数,
//! 已演练入口数)，演练不得超出入口数；只有全部入口被演练、且十二个
//! 批次逐一被点亮，机制判据才算通过。
//!
//! 不变式：exercised ≤ entries；覆盖率 = 已覆盖行 / 总行；十二批次
//! 位图必须全真；冻结后登记/演练/重挂全以 MT_FROZEN 拒止。
//!
//! 结构：MechRow（机制行）/ MechLedger（十二批机制账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11073 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e920;

/// 空账：机制号/入口数为 0、或零步演练
pub const MT_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：批次不在 B09..=B20、演练超入口
pub const MT_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：机制号重复登记
pub const MT_DUP: u32 = SEG_BASE + 0x03;
/// 失配：存在未覆盖机制
pub const MT_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：不足十二行、或批次位图有洞
pub const MT_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：机制账冻结后的登记/演练
pub const MT_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x73 标记 11073 号卡
pub const MT_TAG: u64 = 0x1e80_73;

/// 十二批编号下界（B09）
const BATCH_LO: u8 = 9;
/// 十二批编号上界（B20）
const BATCH_HI: u8 = 20;
/// 本段固定十二席
const SEATS: usize = 12;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 单个机制行
#[derive(Clone, Debug)]
pub struct MechRow {
    /// 机制号（1..=12）
    pub mech: u8,
    /// 所属批次 B09..=B20
    pub class: u8,
    /// 入口总数
    pub entries: u8,
    /// 已演练入口数
    pub exercised: u8,
}

impl MechRow {
    /// 新建未演练机制行
    pub fn new(mech: u8, class: u8, entries: u8) -> MechRow {
        MechRow { mech, class, entries, exercised: 0 }
    }

    /// 是否全入口覆盖
    pub fn covered(&self) -> bool {
        self.entries > 0 && self.exercised >= self.entries
    }

    /// 覆盖率（百分比，入口为 0 时返回 0）
    pub fn progress(&self) -> u32 {
        if self.entries == 0 {
            return 0;
        }
        (self.exercised as u32)
            .wrapping_mul(100)
            .wrapping_div(self.entries as u32)
    }

    /// 机制行指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_byte(h, self.mech);
        h = fnv_byte(h, self.class);
        h = fnv_byte(h, self.entries);
        fnv_byte(h, self.exercised)
    }
}

/// B09–B20 机制总账
#[derive(Clone, Debug)]
pub struct MechLedger {
    /// 机制行
    pub rows: Vec<MechRow>,
    /// 十二批次点亮位图（下标 class-9）
    pub batches: [bool; 12],
    /// 最近一次重放覆盖数
    pub verified: u16,
    frozen: Option<u64>,
}

impl MechLedger {
    /// 空机制账
    pub fn new() -> MechLedger {
        MechLedger { rows: Vec::new(), batches: [false; 12], verified: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(MT_FROZEN);
        }
        Ok(())
    }

    fn batch_index(class: u8) -> Option<usize> {
        if class >= BATCH_LO && class <= BATCH_HI {
            Some((class - BATCH_LO) as usize)
        } else {
            None
        }
    }

    fn index(&self, mech: u8) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].mech == mech {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记机制：0 号 / 越批 / 零入口 / 重号四道闸
    pub fn enroll(&mut self, row: MechRow) -> Result<(), u32> {
        self.guard()?;
        if row.mech == 0 || row.entries == 0 {
            return Err(MT_EMPTY);
        }
        if MechLedger::batch_index(row.class).is_none() {
            return Err(MT_RANGE);
        }
        if self.index(row.mech).is_some() {
            return Err(MT_DUP);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 点亮一个批次（显式登记覆盖面）
    pub fn cover_batch(&mut self, class: u8) -> Result<(), u32> {
        self.guard()?;
        let idx = match MechLedger::batch_index(class) {
            Some(i) => i,
            None => return Err(MT_RANGE),
        };
        self.batches[idx] = true;
        Ok(())
    }

    /// 演练 n 个入口：零步拒、超入口拒
    pub fn exercise(&mut self, mech: u8, n: u8) -> Result<u8, u32> {
        self.guard()?;
        if n == 0 {
            return Err(MT_EMPTY);
        }
        let idx = match self.index(mech) {
            Some(i) => i,
            None => return Err(MT_EMPTY),
        };
        let next = (self.rows[idx].exercised as u16).wrapping_add(n as u16);
        if next > self.rows[idx].entries as u16 {
            return Err(MT_RANGE);
        }
        self.rows[idx].exercised = next as u8;
        let class = self.rows[idx].class;
        if let Some(bi) = MechLedger::batch_index(class) {
            self.batches[bi] = true;
        }
        Ok(self.rows[idx].exercised)
    }

    /// 重挂一个机制（回退一步，供下一轮演练）
    pub fn reopen(&mut self, mech: u8) -> Result<u8, u32> {
        self.guard()?;
        let idx = match self.index(mech) {
            Some(i) => i,
            None => return Err(MT_EMPTY),
        };
        if self.rows[idx].exercised == 0 {
            return Err(MT_EMPTY);
        }
        self.rows[idx].exercised = self.rows[idx].exercised.wrapping_sub(1);
        Ok(self.rows[idx].exercised)
    }

    /// 覆盖统计：(已覆盖, 总数)
    pub fn coverage(&self) -> (u32, u32) {
        let mut covered = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].covered() {
                covered = covered.wrapping_add(1);
            }
            k += 1;
        }
        (covered, self.rows.len() as u32)
    }

    /// 入口总数与已演练总数
    pub fn entry_totals(&self) -> (u32, u32) {
        let mut total = 0u32;
        let mut done = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            total = total.wrapping_add(self.rows[k].entries as u32);
            done = done.wrapping_add(self.rows[k].exercised as u32);
            k += 1;
        }
        (done, total)
    }

    /// 机制语义：批次合法、演练不超入口、行类批次已点亮
    pub fn mechanism_semantics(&self) -> bool {
        let mut k = 0usize;
        while k < self.rows.len() {
            let row = &self.rows[k];
            if MechLedger::batch_index(row.class).is_none() {
                return false;
            }
            if row.entries == 0 || row.exercised > row.entries {
                return false;
            }
            let bi = (row.class - BATCH_LO) as usize;
            if !self.batches[bi] {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 机制号唯一递增
    pub fn ids_ordered(&self) -> bool {
        let mut k = 1usize;
        while k < self.rows.len() {
            if self.rows[k].mech <= self.rows[k - 1].mech {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 重放：不足十二行/批次有洞 MT_SHORT，未覆盖 MT_MISMATCH
    pub fn judge(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.rows.len() < SEATS {
            return Err(MT_SHORT);
        }
        let mut k = 0usize;
        while k < self.batches.len() {
            if !self.batches[k] {
                return Err(MT_SHORT);
            }
            k += 1;
        }
        let mut covered = 0u16;
        k = 0;
        while k < self.rows.len() {
            if self.rows[k].covered() {
                covered = covered.wrapping_add(1);
            }
            k += 1;
        }
        self.verified = covered;
        if covered as usize != self.rows.len() {
            return Err(MT_MISMATCH);
        }
        Ok(covered)
    }

    /// 机制账指纹
    pub fn fold(&self) -> u64 {
        let mut h = MT_TAG;
        let mut k = 0usize;
        while k < self.batches.len() {
            h = fnv_byte(h, if self.batches[k] { 1 } else { 0 });
            k += 1;
        }
        h = fnv_u16(h, self.verified);
        k = 0;
        while k < self.rows.len() {
            h = self.rows[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结机制账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零机制 / 越批 / 重号 / 演练超入口
pub fn inject_four() -> [bool; 4] {
    let mut m0 = MechLedger::new();
    let empty = matches!(m0.enroll(MechRow::new(0, 9, 1)), Err(MT_EMPTY));
    let mut m1 = MechLedger::new();
    let range = matches!(m1.enroll(MechRow::new(1, 8, 1)), Err(MT_RANGE));
    let mut m2 = MechLedger::new();
    let first = m2.enroll(MechRow::new(1, 9, 2)).is_ok();
    let dup = first && matches!(m2.enroll(MechRow::new(1, 10, 2)), Err(MT_DUP));
    let mut m3 = MechLedger::new();
    let seeded = m3.enroll(MechRow::new(1, 9, 2)).is_ok();
    let over = seeded && matches!(m3.exercise(1, 3), Err(MT_RANGE));
    [empty, range, dup, over]
}

/// 构造十二批样本（一行一批、入口全演练）
fn sample_ledger() -> Result<MechLedger, u32> {
    let mut led = MechLedger::new();
    let mut class = BATCH_LO;
    let mut mech = 1u8;
    while class <= BATCH_HI {
        let entries = 2u8.wrapping_add(class % 3);
        led.enroll(MechRow::new(mech, class, entries))?;
        led.exercise(mech, entries)?;
        mech = mech.wrapping_add(1);
        class = class.wrapping_add(1);
    }
    Ok(led)
}

/// UNX-F11073 判据入口
pub fn run_unx11073_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11073");
    let mut led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => MechLedger::new(),
    };
    let hits = led.judge().unwrap_or(0);
    let (covered, total) = led.coverage();
    s.add(
        "J1a-M型判据全量重放",
        hits == 12 && led.rows.len() == SEATS,
        "B09–B20 十二机制必须全部覆盖",
    );
    s.add(
        "J1b-机制语义一致",
        led.mechanism_semantics() && covered == 12 && total == 12,
        "批次合法、演练不超入口、十二位图全真",
    );
    let (done, entries) = led.entry_totals();
    s.add(
        "J2-覆盖守恒",
        done == entries && done > 0 && covered == total,
        "已演练入口必须等于入口总数",
    );
    let inj = inject_four();
    s.add("J3a-零机制拒止", inj[0], "mech=0 登记必须 MT_EMPTY");
    s.add("J3b-越批拒止", inj[1], "B08 登记必须 MT_RANGE");
    s.add("J3c-重号拒止", inj[2], "同机制号二次登记必须 MT_DUP");
    s.add("J3d-超入口拒止", inj[3], "演练 3/2 入口必须 MT_RANGE");
    let mut fz = match sample_ledger() {
        Ok(v) => v,
        Err(_) => MechLedger::new(),
    };
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.exercise(1, 1), Err(MT_FROZEN))
        && matches!(fz.enroll(MechRow::new(20, 20, 1)), Err(MT_FROZEN))
        && matches!(fz.cover_batch(9), Err(MT_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后演练/登记/点批全拒且指纹不变",
    );
    let fp_a = led.clone().fold();
    let mut b = match sample_ledger() {
        Ok(v) => v,
        Err(_) => MechLedger::new(),
    };
    let sens = b.reopen(1).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "重挂一步必须改变 fold");
    let zero_entry = matches!(
        MechLedger::new().enroll(MechRow::new(1, 9, 0)),
        Err(MT_EMPTY)
    );
    let zero_exercise = {
        let mut g = MechLedger::new();
        g.enroll(MechRow::new(1, 9, 2)).is_ok()
            && matches!(g.exercise(1, 0), Err(MT_EMPTY))
    };
    let bad_cover = matches!(MechLedger::new().cover_batch(21), Err(MT_RANGE));
    s.add(
        "J6-范围与空账拒止",
        zero_entry && zero_exercise && bad_cover,
        "零入口/零演练/越批点亮三路都必须拒",
    );
    let unique = led.ids_ordered() && led.rows[0].mech == 1 && led.rows[11].mech == 12;
    let dup_reject = {
        let mut g = MechLedger::new();
        g.enroll(MechRow::new(1, 9, 1)).is_ok()
            && matches!(g.enroll(MechRow::new(1, 9, 1)), Err(MT_DUP))
    };
    s.add(
        "J7-唯一与序不变式",
        unique && dup_reject,
        "机制号 1..=12 唯一递增且重号当场拒",
    );
    let mut partial = match sample_ledger() {
        Ok(v) => v,
        Err(_) => MechLedger::new(),
    };
    while partial.rows.len() > 6 {
        partial.rows.pop();
    }
    let negative = partial.rows.len() == 6 && matches!(partial.judge(), Err(MT_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "六行半账必须 MT_SHORT，不得伪装成绿",
    );
    let reopen_floor = {
        let mut g = MechLedger::new();
        match g.enroll(MechRow::new(1, 9, 2)) {
            Ok(()) => matches!(g.reopen(1), Err(MT_EMPTY)),
            Err(_) => false,
        }
    };
    let full_progress = {
        let mut g = MechLedger::new();
        match g.enroll(MechRow::new(1, 9, 4)) {
            Ok(()) => g.exercise(1, 4).is_ok() && g.rows[0].progress() == 100,
            Err(_) => false,
        }
    };
    s.add(
        "J10-重挂与覆盖率",
        reopen_floor && full_progress,
        "零演练不可重挂、满演练覆盖率必须 100",
    );
    s.add_dyn(
        "J9-动态账",
        hits == 12 && covered == 12,
        &format!("hits={} covered={}/{} 入口={}/{}", hits, covered, total, done, entries),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0,
        &format!("fold={:016x} tag={:x}", led.fold(), MT_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11073_flow() {
        let mut led = sample_ledger().unwrap_or(MechLedger::new());
        assert_eq!(led.judge().unwrap_or(0), 12);
        assert!(led.mechanism_semantics());
        assert_eq!(led.coverage(), (12, 12));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11073_checks_green() {
        assert!(run_unx11073_checks().all_passed(), "UNX-F11073 判据未全绿");
    }
}
