//! UNX-F11074 F 型回归段（0x1e940 段）
//!
//! B01–B08 地基判据在此重放：每条判据登记 (criterion, 批次, 通过位)，
//! 每个批次在通过后录制基线指纹（只折判据号与通过位）；重放时按同样
//! 顺序重算，逐批与录制基线比对。地基语义要求八个批次全覆盖、十席
//! 全通过、八条基线逐位一致。
//!
//! 不变式：pass + fail == 行数；sealed 基线数 == 8；replayed 必须等于
//! recorded。冻结后登记/封印/篡改全以 FT_FROZEN 拒止。
//!
//! 结构：FoundationRow（地基判据行）/ BaselineDigest（批次基线）/
//! FoundationLedger（八批总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11074 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e940;

/// 空账：判据号为 0、批次无行或基线缺失
pub const FT_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：批次不在 B01..=B08
pub const FT_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：判据号或批次基线重复
pub const FT_DUP: u32 = SEG_BASE + 0x03;
/// 失配：重放指纹与录制基线不一致、或存在红行
pub const FT_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：不足十席即申请重放
pub const FT_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的登记/封印/篡改
pub const FT_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x74 标记 11074 号卡
pub const FT_TAG: u64 = 0x1e80_74;

/// 批次下界（B01）
const BATCH_LO: u8 = 1;
/// 批次上界（B08）
const BATCH_HI: u8 = 8;
/// 本段固定十席
const SEATS: usize = 10;

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

/// 地基判据行
#[derive(Clone, Debug)]
pub struct FoundationRow {
    /// 判据号（1..=10）
    pub criterion: u16,
    /// 批次 B01..=B08
    pub batch: u8,
    /// 通过位
    pub pass: bool,
}

impl FoundationRow {
    /// 新建判据行
    pub fn new(criterion: u16, batch: u8, pass: bool) -> FoundationRow {
        FoundationRow { criterion, batch, pass }
    }

    /// 单行指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.criterion);
        h = fnv_byte(h, self.batch);
        fnv_byte(h, if self.pass { 1 } else { 0 })
    }
}

/// 单批基线
#[derive(Clone, Debug)]
pub struct BaselineDigest {
    /// 所属批次
    pub batch: u8,
    /// 封印时录制值
    pub recorded: u64,
    /// 最近一次重放值
    pub replayed: u64,
}

impl BaselineDigest {
    /// 基线是否一致
    pub fn consistent(&self) -> bool {
        self.recorded != 0 && self.recorded == self.replayed
    }

    /// 基线指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_byte(h, self.batch);
        h = fnv_u64(h, self.recorded);
        fnv_u64(h, self.replayed)
    }
}

fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut k = 0u32;
    while k < 8 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// B01–B08 地基总账
#[derive(Clone, Debug)]
pub struct FoundationLedger {
    /// 判据行
    pub rows: Vec<FoundationRow>,
    /// 批次基线
    pub baselines: Vec<BaselineDigest>,
    /// 最近一次重放过批数
    pub verified: u16,
    frozen: Option<u64>,
}

impl FoundationLedger {
    /// 空总账
    pub fn new() -> FoundationLedger {
        FoundationLedger { rows: Vec::new(), baselines: Vec::new(), verified: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(FT_FROZEN);
        }
        Ok(())
    }

    fn index(&self, criterion: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].criterion == criterion {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn baseline_index(&self, batch: u8) -> Option<usize> {
        let mut k = 0usize;
        while k < self.baselines.len() {
            if self.baselines[k].batch == batch {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记判据：0 号 / 越批 / 重号三道闸
    pub fn record(&mut self, row: FoundationRow) -> Result<(), u32> {
        self.guard()?;
        if row.criterion == 0 {
            return Err(FT_EMPTY);
        }
        if row.batch < BATCH_LO || row.batch > BATCH_HI {
            return Err(FT_RANGE);
        }
        if self.index(row.criterion).is_some() {
            return Err(FT_DUP);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 单批指纹：只折本批行的 (criterion, pass)
    pub fn batch_fold(&self, batch: u8) -> u64 {
        let mut h = FT_TAG;
        h = fnv_byte(h, batch);
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch == batch {
                h = fnv_u16(h, self.rows[k].criterion);
                h = fnv_byte(h, if self.rows[k].pass { 1 } else { 0 });
            }
            k += 1;
        }
        h
    }

    /// 封印单批基线
    pub fn seal(&mut self, batch: u8) -> Result<u64, u32> {
        self.guard()?;
        if batch < BATCH_LO || batch > BATCH_HI {
            return Err(FT_RANGE);
        }
        if self.baseline_index(batch).is_some() {
            return Err(FT_DUP);
        }
        let mut count = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch == batch {
                count = count.wrapping_add(1);
            }
            k += 1;
        }
        if count == 0 {
            return Err(FT_EMPTY);
        }
        let recorded = self.batch_fold(batch);
        self.baselines.push(BaselineDigest { batch, recorded, replayed: 0 });
        Ok(recorded)
    }

    /// 重放单批：未封印 FT_EMPTY，指纹漂移 FT_MISMATCH
    pub fn replay(&mut self, batch: u8) -> Result<u64, u32> {
        self.guard()?;
        let idx = match self.baseline_index(batch) {
            Some(i) => i,
            None => return Err(FT_EMPTY),
        };
        let live = self.batch_fold(batch);
        self.baselines[idx].replayed = live;
        if live != self.baselines[idx].recorded {
            return Err(FT_MISMATCH);
        }
        Ok(live)
    }

    /// 八批全量重放
    pub fn replay_all(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.rows.len() < SEATS {
            return Err(FT_SHORT);
        }
        let mut b = BATCH_LO;
        let mut n = 0u16;
        while b <= BATCH_HI {
            self.replay(b)?;
            n = n.wrapping_add(1);
            b = b.wrapping_add(1);
        }
        self.verified = n;
        Ok(n)
    }

    /// 地基语义：八批全覆盖、全通过、基线全一致
    pub fn foundation_semantics(&self) -> bool {
        if self.baselines.len() != BATCH_HI as usize {
            return false;
        }
        let mut b = BATCH_LO;
        while b <= BATCH_HI {
            if self.baseline_index(b).is_none() {
                return false;
            }
            b = b.wrapping_add(1);
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].pass {
                return false;
            }
            k += 1;
        }
        k = 0;
        while k < self.baselines.len() {
            if !self.baselines[k].consistent() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 通过进位统计：(pass, fail)
    pub fn pass_tally(&self) -> (u16, u16) {
        let mut p = 0u16;
        let mut f = 0u16;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].pass {
                p = p.wrapping_add(1);
            } else {
                f = f.wrapping_add(1);
            }
            k += 1;
        }
        (p, f)
    }

    /// 篡改一条判据（重放漂移对照）
    pub fn tamper(&mut self, criterion: u16, pass: bool) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.index(criterion) {
            Some(i) => i,
            None => return Err(FT_EMPTY),
        };
        self.rows[idx].pass = pass;
        Ok(())
    }

    /// 判据号唯一递增
    pub fn ids_ordered(&self) -> bool {
        let mut k = 1usize;
        while k < self.rows.len() {
            if self.rows[k].criterion <= self.rows[k - 1].criterion {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 重放审计：台面数 + 通过数 + 基线一致
    pub fn audit(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.rows.len() < SEATS {
            return Err(FT_SHORT);
        }
        let (pass, fail) = self.pass_tally();
        if fail > 0 {
            return Err(FT_MISMATCH);
        }
        let n = self.replay_all()?;
        if n != BATCH_HI as u16 || !self.foundation_semantics() {
            return Err(FT_MISMATCH);
        }
        Ok(pass)
    }

    /// 总账指纹
    pub fn fold(&self) -> u64 {
        let mut h = FT_TAG;
        h = fnv_u16(h, self.verified);
        let mut k = 0usize;
        while k < self.rows.len() {
            h = self.rows[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.baselines.len() {
            h = self.baselines[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零判据 / 越批 / 重号 / 篡改后重放
pub fn inject_four() -> [bool; 4] {
    let mut l0 = FoundationLedger::new();
    let empty = matches!(l0.record(FoundationRow::new(0, 1, true)), Err(FT_EMPTY));
    let mut l1 = FoundationLedger::new();
    let range = matches!(l1.record(FoundationRow::new(1, 9, true)), Err(FT_RANGE));
    let mut l2 = FoundationLedger::new();
    let first = l2.record(FoundationRow::new(1, 1, true)).is_ok();
    let dup = first && matches!(l2.record(FoundationRow::new(1, 2, true)), Err(FT_DUP));
    let mut l3 = match sample_ledger() {
        Ok(v) => v,
        Err(_) => FoundationLedger::new(),
    };
    let seeded = l3.tamper(1, false).is_ok() && l3.replay(1).is_err();
    let mismatch = seeded && matches!(l3.replay(1), Err(FT_MISMATCH));
    [empty, range, dup, mismatch]
}

/// 构造八批样本（10 行：B01 两行、B08 两行、其余各一行）
fn sample_ledger() -> Result<FoundationLedger, u32> {
    let mut led = FoundationLedger::new();
    let mut criterion = 1u16;
    let mut batch = BATCH_LO;
    while batch <= BATCH_HI {
        let n = if batch == 1 || batch == 8 { 2u8 } else { 1u8 };
        let mut j = 0u8;
        while j < n {
            led.record(FoundationRow::new(criterion, batch, true))?;
            criterion = criterion.wrapping_add(1);
            j = j.wrapping_add(1);
        }
        led.seal(batch)?;
        batch = batch.wrapping_add(1);
    }
    led.replay_all()?;
    Ok(led)
}

/// UNX-F11074 判据入口
pub fn run_unx11074_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11074");
    let mut led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => FoundationLedger::new(),
    };
    let pass = led.audit().unwrap_or(0);
    let (p, f) = led.pass_tally();
    s.add(
        "J1a-F型判据全量重放",
        pass == 10 && led.rows.len() == SEATS && p == 10,
        "B01–B08 十席必须全通过并逐批重放",
    );
    s.add(
        "J1b-地基语义一致",
        led.foundation_semantics() && led.baselines.len() == 8,
        "八批基线齐全且 recorded==replayed",
    );
    s.add(
        "J2-通过守恒",
        p.wrapping_add(f) == led.rows.len() as u16 && f == 0,
        "pass+fail 必须等于行数且 fail 为 0",
    );
    let inj = inject_four();
    s.add("J3a-零判据拒止", inj[0], "criterion=0 登记必须 FT_EMPTY");
    s.add("J3b-越批拒止", inj[1], "B09 登记必须 FT_RANGE");
    s.add("J3c-重号拒止", inj[2], "同判据号二次登记必须 FT_DUP");
    s.add("J3d-篡改重放拒", inj[3], "篡改后重放必须 FT_MISMATCH");
    let mut fz = match sample_ledger() {
        Ok(v) => v,
        Err(_) => FoundationLedger::new(),
    };
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.record(FoundationRow::new(77, 1, true)), Err(FT_FROZEN))
        && matches!(fz.seal(1), Err(FT_FROZEN))
        && matches!(fz.tamper(1, false), Err(FT_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后登记/封印/篡改全拒且指纹不变",
    );
    let fp_a = led.clone().fold();
    let mut b = match sample_ledger() {
        Ok(v) => v,
        Err(_) => FoundationLedger::new(),
    };
    let sens = b.tamper(2, false).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "篡改一条判据必须改变 fold");
    let zero_batch = matches!(
        FoundationLedger::new().record(FoundationRow::new(1, 0, true)),
        Err(FT_RANGE)
    );
    let empty_seal = {
        let mut g = FoundationLedger::new();
        g.record(FoundationRow::new(1, 1, true)).is_ok() && matches!(g.seal(3), Err(FT_EMPTY))
    };
    let unsealed_replay = {
        let mut g = FoundationLedger::new();
        g.record(FoundationRow::new(1, 1, true)).is_ok() && matches!(g.replay(2), Err(FT_EMPTY))
    };
    s.add(
        "J6-范围与空账拒止",
        zero_batch && empty_seal && unsealed_replay,
        "零批/空批封印/未封印重放三路都必须拒",
    );
    let unique = led.ids_ordered() && led.rows[0].criterion == 1;
    let dup_reject = {
        let mut g = FoundationLedger::new();
        g.record(FoundationRow::new(1, 1, true)).is_ok()
            && matches!(g.record(FoundationRow::new(1, 2, true)), Err(FT_DUP))
    };
    s.add(
        "J7-唯一与序不变式",
        unique && dup_reject,
        "判据号唯一递增且重号当场拒",
    );
    let mut partial = match sample_ledger() {
        Ok(v) => v,
        Err(_) => FoundationLedger::new(),
    };
    while partial.rows.len() > 5 {
        partial.rows.pop();
    }
    let negative = partial.rows.len() == 5 && matches!(partial.audit(), Err(FT_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五席半账必须 FT_SHORT，不得伪装成绿",
    );
    let stability = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => FoundationLedger::new(),
        };
        let first = g.replay_all().unwrap_or(0);
        let second = g.replay_all().unwrap_or(0);
        first == 8 && second == 8 && g.foundation_semantics()
    };
    let replay_twice = {
        let mut g = match sample_ledger() {
            Ok(v) => v,
            Err(_) => FoundationLedger::new(),
        };
        let a = g.replay(1).is_ok();
        let b = g.replay(1).is_ok();
        a && b
    };
    s.add(
        "J10-基线可复现",
        stability && replay_twice,
        "八批重复重放必须稳定、单批可重入",
    );
    s.add_dyn(
        "J9-动态账",
        pass == 10 && led.verified == 8,
        &format!(
            "pass={} fail={} 基线={} 重放批={}",
            p,
            f,
            led.baselines.len(),
            led.verified
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0,
        &format!("fold={:016x} tag={:x}", led.fold(), FT_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11074_flow() {
        let mut led = sample_ledger().unwrap_or(FoundationLedger::new());
        assert_eq!(led.audit().unwrap_or(0), 10);
        assert!(led.foundation_semantics());
        assert_eq!(led.baselines.len(), 8);
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11074_checks_green() {
        assert!(run_unx11074_checks().all_passed(), "UNX-F11074 判据未全绿");
    }
}
