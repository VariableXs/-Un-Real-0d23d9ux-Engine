//! UNX-F11076 失配归因与修复回执账（0x1e980 段）
//!
//! 回归失配逐例登记 (case, seg, cause, repro)，其中 cause 四类：1 规格 /
//! 2 实现 / 3 环境 / 4 抖动（Flake）。每例的修复必须附一张回执
//! (case, owner, before, after, attempts)，before/after 为该例判据指纹；
//! 回执只有 after != before 且 attempts 落在 1..=3 内才算有效，闭合时
//! 逐例销账。
//!
//! 不变式：回执与失配一一对应；闭合账要求 cases 全 closed、receipts 全
//! changed 且 closed；冻结后开例/挂回执/销账/重开一律 MR_FROZEN 拒止。
//!
//! 结构：MismatchCase（失配例）/ RepairReceipt（修复回执）/
//! RepairLedger（归因账 + 闭合 + 审计 + 指纹）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11076 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e980;

/// 空账：编号为 0 的失配例、无主回执、无回执可销
pub const MR_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：段号不在回归窗口、cause 不在 1..=4、repro 不在 1..=5
pub const MR_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：失配例重复开账、同例重复挂回执、重复销账
pub const MR_DUP: u32 = SEG_BASE + 0x03;
/// 失配：before == after，修复未改变判据指纹
pub const MR_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：attempts 越界、例数不足、仍有未销账项
pub const MR_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：回执账冻结后的开例/挂账/销账/重开
pub const MR_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x76 标记 11076 号卡
pub const MR_TAG: u64 = 0x1e80_76;

/// 样本失配例数
const SEATS: usize = 10;
/// 回归段窗口下界（F11064 段基址）
const SEG_LO: u32 = 0x1e800;
/// 回归段窗口上界（本卡段基址）
const SEG_HI: u32 = 0x1e980;
/// 复现次数下界
const REPRO_LO: u16 = 1;
/// 复现次数上界
const REPRO_HI: u16 = 5;
/// 单例修复尝试上限
const ATTEMPT_MAX: u8 = 3;
/// 回执责任人编号下界
const OWNER_LO: u16 = 0x40;

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

fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut k = 0u32;
    while k < 8 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 失配归因四类
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// 规格判据本身写错
    Spec,
    /// 实现与规格不符
    Impl,
    /// 环境/时序导致
    Env,
    /// 抖动（Flake），需复现确认
    Flake,
}

impl Cause {
    /// 由落账码还原归因
    pub fn from_code(code: u8) -> Option<Cause> {
        match code {
            1 => Some(Cause::Spec),
            2 => Some(Cause::Impl),
            3 => Some(Cause::Env),
            4 => Some(Cause::Flake),
            _ => None,
        }
    }

    /// 归因落账码（1..=4）
    pub fn code(self) -> u8 {
        match self {
            Cause::Spec => 1,
            Cause::Impl => 2,
            Cause::Env => 3,
            Cause::Flake => 4,
        }
    }

    /// 归因短标签（供动态账引用）
    pub fn label(self) -> &'static str {
        match self {
            Cause::Spec => "spec",
            Cause::Impl => "impl",
            Cause::Env => "env",
            Cause::Flake => "flake",
        }
    }
}

/// 单条失配例：案例号、所属段、归因、复现次数、是否仍开放
#[derive(Clone, Debug)]
pub struct MismatchCase {
    /// 失配例编号（1 起）
    pub case: u16,
    /// 所属回归段基址
    pub seg: u32,
    /// 归因落账码 1..=4
    pub cause: u8,
    /// 复现次数 1..=5
    pub repro: u16,
    /// 是否尚未销账
    pub open: bool,
}

impl MismatchCase {
    /// 开一条失配例：编号非零、段在窗口内、归因四类、复现次数有界
    pub fn new(case: u16, seg: u32, cause: u8, repro: u16) -> Result<Self, u32> {
        if case == 0 {
            return Err(MR_EMPTY);
        }
        if seg < SEG_LO || seg > SEG_HI {
            return Err(MR_RANGE);
        }
        if Cause::from_code(cause).is_none() {
            return Err(MR_RANGE);
        }
        if repro < REPRO_LO || repro > REPRO_HI {
            return Err(MR_RANGE);
        }
        Ok(Self {
            case,
            seg,
            cause,
            repro,
            open: true,
        })
    }

    /// 归因是否已写明
    pub fn attributed(&self) -> bool {
        Cause::from_code(self.cause).is_some()
            && self.repro >= REPRO_LO
            && self.repro <= REPRO_HI
            && self.seg >= SEG_LO
            && self.seg <= SEG_HI
    }

    /// 折入失配例
    pub fn digest(&self, h0: u64) -> u64 {
        let mut h = fnv_u16(h0, self.case);
        h = fnv_u32(h, self.seg);
        h = fnv_byte(h, self.cause);
        h = fnv_u16(h, self.repro);
        fnv_byte(h, if self.open { 1 } else { 0 })
    }
}

/// 修复回执：案例号、责任人、修复前后指纹、尝试次数、是否已销
#[derive(Clone, Debug)]
pub struct RepairReceipt {
    /// 对应的失配例编号
    pub case: u16,
    /// 修复责任人（>= 0x40）
    pub owner: u16,
    /// 修复前判据指纹
    pub before: u64,
    /// 修复后判据指纹
    pub after: u64,
    /// 修复尝试次数 1..=3
    pub attempts: u8,
    /// 是否已随例销账
    pub closed: bool,
}

impl RepairReceipt {
    /// 建一张回执：指纹必须真的变化，尝试次数有界，责任人非空
    pub fn new(
        case: u16,
        owner: u16,
        before: u64,
        after: u64,
        attempts: u8,
    ) -> Result<Self, u32> {
        if case == 0 {
            return Err(MR_EMPTY);
        }
        if owner < OWNER_LO {
            return Err(MR_EMPTY);
        }
        if attempts == 0 || attempts > ATTEMPT_MAX {
            return Err(MR_SHORT);
        }
        if before == after {
            return Err(MR_MISMATCH);
        }
        Ok(Self {
            case,
            owner,
            before,
            after,
            attempts,
            closed: false,
        })
    }

    /// 修复是否留下指纹差异
    pub fn changed(&self) -> bool {
        self.after != self.before
    }

    /// 回执是否有效：有差异、尝试有界、责任人非空
    pub fn valid(&self) -> bool {
        self.changed() && self.attempts >= 1 && self.attempts <= ATTEMPT_MAX && self.owner >= OWNER_LO
    }

    /// 折入回执
    pub fn digest(&self, h0: u64) -> u64 {
        let mut h = fnv_u16(h0, self.case);
        h = fnv_u16(h, self.owner);
        h = fnv_u64(h, self.before);
        h = fnv_u64(h, self.after);
        h = fnv_byte(h, self.attempts);
        fnv_byte(h, if self.closed { 1 } else { 0 })
    }
}

/// 归因与修复回执总账
#[derive(Clone, Debug)]
pub struct RepairLedger {
    /// 失配例（按 case 升序）
    pub cases: Vec<MismatchCase>,
    /// 回执（与 cases 一一对应，同序）
    pub receipts: Vec<RepairReceipt>,
    /// 冻结指纹
    pub frozen: Option<u64>,
}

impl RepairLedger {
    /// 空账
    pub fn new() -> Self {
        Self {
            cases: Vec::new(),
            receipts: Vec::new(),
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            Err(MR_FROZEN)
        } else {
            Ok(())
        }
    }

    fn case_index(&self, case: u16) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].case == case {
                return Ok(k);
            }
            k += 1;
        }
        Err(MR_EMPTY)
    }

    fn receipt_index(&self, case: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.receipts.len() {
            if self.receipts[k].case == case {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 开一条失配例；编号 1 起、段在窗口内、归因/repro 有界、不得重号
    pub fn open_case(&mut self, case: u16, seg: u32, cause: u8, repro: u16) -> Result<(), u32> {
        self.guard()?;
        let row = MismatchCase::new(case, seg, cause, repro)?;
        if self.case_index(case).is_ok() {
            return Err(MR_DUP);
        }
        self.cases.push(row);
        Ok(())
    }

    /// 挂修复回执：例必须存在且未销、回执不得重号、指纹必须变化
    pub fn attach(
        &mut self,
        case: u16,
        owner: u16,
        before: u64,
        after: u64,
        attempts: u8,
    ) -> Result<(), u32> {
        self.guard()?;
        let idx = self.case_index(case)?;
        if self.receipt_index(case).is_some() {
            return Err(MR_DUP);
        }
        if !self.cases[idx].open {
            return Err(MR_SHORT);
        }
        let row = RepairReceipt::new(case, owner, before, after, attempts)?;
        self.receipts.push(row);
        Ok(())
    }

    /// 销账：例与回执都要在，回执指纹必须有差异且尚未销过
    pub fn close(&mut self, case: u16) -> Result<(), u32> {
        self.guard()?;
        let idx = self.case_index(case)?;
        let ridx = match self.receipt_index(case) {
            Some(v) => v,
            None => return Err(MR_EMPTY),
        };
        if !self.receipts[ridx].changed() {
            return Err(MR_MISMATCH);
        }
        if self.receipts[ridx].closed {
            return Err(MR_DUP);
        }
        self.receipts[ridx].closed = true;
        self.cases[idx].open = false;
        Ok(())
    }

    /// 重开一例并改写复现次数（用于漂移复核，不可在冻结后使用）
    pub fn reopen(&mut self, case: u16, repro: u16) -> Result<(), u32> {
        self.guard()?;
        if repro < REPRO_LO || repro > REPRO_HI {
            return Err(MR_RANGE);
        }
        let idx = self.case_index(case)?;
        self.cases[idx].repro = repro;
        self.cases[idx].open = true;
        Ok(())
    }

    /// 归因覆盖数：段、归因、复现三面都写明的例数
    pub fn attributed(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].attributed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 四类归因计数（spec/impl/env/flake）
    pub fn cause_counts(&self) -> [u16; 4] {
        let mut out = [0u16; 4];
        let mut k = 0usize;
        while k < self.cases.len() {
            let code = self.cases[k].cause;
            if code >= 1 && code <= 4 {
                let slot = (code as usize).wrapping_sub(1);
                out[slot] = out[slot].wrapping_add(1);
            }
            k += 1;
        }
        out
    }

    /// 全部回执都留下了指纹差异
    pub fn all_changed(&self) -> bool {
        if self.receipts.is_empty() {
            return false;
        }
        let mut k = 0usize;
        while k < self.receipts.len() {
            if !self.receipts[k].changed() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 闭合：例数==回执数、逐例一一配对、例已销、回执已销且有差异
    pub fn closure(&self) -> bool {
        if self.cases.is_empty() || self.cases.len() != self.receipts.len() {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].open {
                return false;
            }
            let ridx = match self.receipt_index(self.cases[k].case) {
                Some(v) => v,
                None => return false,
            };
            if !self.receipts[ridx].closed || !self.receipts[ridx].changed() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 全账审计：例数、回执配对、指纹差异、归因、闭合
    pub fn audit(&self) -> Result<u16, u32> {
        self.guard()?;
        if self.cases.len() < SEATS {
            return Err(MR_SHORT);
        }
        if self.receipts.len() != self.cases.len() {
            return Err(MR_SHORT);
        }
        if !self.all_changed() {
            return Err(MR_MISMATCH);
        }
        if self.attributed() != self.cases.len() {
            return Err(MR_MISMATCH);
        }
        if !self.closure() {
            return Err(MR_SHORT);
        }
        Ok(self.cases.len() as u16)
    }

    /// 归因账指纹（只折内容，冻结位不参与，保证冻结前后可比）
    pub fn fold(&self) -> u64 {
        let mut h = MR_TAG;
        h = fnv_u16(h, self.cases.len() as u16);
        h = fnv_u16(h, self.receipts.len() as u16);
        let mut k = 0usize;
        while k < self.cases.len() {
            h = self.cases[k].digest(h);
            k += 1;
        }
        k = 0;
        while k < self.receipts.len() {
            h = self.receipts[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结归因账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零编号开例 / 归因越界 / 重复开例 / 无回执销账
pub fn inject_four() -> [bool; 4] {
    let empty = matches!(
        RepairLedger::new().open_case(0, SEG_LO, 1, 1),
        Err(MR_EMPTY)
    );
    let range = matches!(
        RepairLedger::new().open_case(1, SEG_LO, 9, 1),
        Err(MR_RANGE)
    );
    let mut dup_led = RepairLedger::new();
    let dup = dup_led.open_case(1, SEG_LO, 2, 1).is_ok()
        && matches!(dup_led.open_case(1, SEG_LO, 2, 1), Err(MR_DUP));
    let mut short_led = RepairLedger::new();
    let short = short_led.open_case(2, SEG_LO, 3, 1).is_ok()
        && matches!(short_led.close(2), Err(MR_EMPTY));
    [empty, range, dup, short]
}

/// 构造十例十回执并全部销账的样本账
fn sample_ledger() -> Result<RepairLedger, u32> {
    let mut led = RepairLedger::new();
    let mut k = 0u16;
    while (k as usize) < SEATS {
        let case = k.wrapping_add(1);
        let seg = SEG_LO.wrapping_add((k as u32).wrapping_mul(0x20));
        let cause = (k.wrapping_rem(4) as u8).wrapping_add(1);
        let repro = k.wrapping_rem(5).wrapping_add(1);
        led.open_case(case, seg, cause, repro)?;
        k = k.wrapping_add(1);
    }
    k = 0;
    while (k as usize) < SEATS {
        let case = k.wrapping_add(1);
        let owner = OWNER_LO.wrapping_add(k.wrapping_mul(3));
        let before = 0x1000u64
            .wrapping_mul(case as u64)
            .wrapping_add(0x111);
        let after = before ^ 0x5a5a_00c3u64;
        let attempts = (k.wrapping_rem(3) as u8).wrapping_add(1);
        led.attach(case, owner, before, after, attempts)?;
        k = k.wrapping_add(1);
    }
    k = 0;
    while (k as usize) < SEATS {
        led.close(k.wrapping_add(1))?;
        k = k.wrapping_add(1);
    }
    Ok(led)
}

/// UNX-F11076 判据入口
pub fn run_unx11076_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11076");
    let led = match sample_ledger() {
        Ok(v) => v,
        Err(_) => RepairLedger::new(),
    };
    let green = led.audit().unwrap_or(0);
    let cc = led.cause_counts();
    let cause_sum = cc
        .iter()
        .fold(0u16, |a, b| a.wrapping_add(*b));
    s.add(
        "J1a-失配逐例归因",
        green == 10 && led.cases.len() == SEATS && led.attributed() == SEATS,
        "十例失配必须段、归因、复现三面写明",
    );
    s.add(
        "J1b-回执账闭合",
        led.closure()
            && led.receipts.len() == SEATS
            && led.receipts.iter().all(|r| r.closed && r.changed()),
        "十张回执逐例销账，before/after 必须留下差异",
    );
    let mut paired = led.cases.len() == led.receipts.len();
    let mut k = 0usize;
    while k < led.cases.len() {
        if led.receipts[k].case != led.cases[k].case || led.cases[k].open {
            paired = false;
        }
        k += 1;
    }
    s.add(
        "J2-例账守恒",
        paired && cause_sum == 10 && led.attributed() == 10,
        "例数==回执数==十，归因计数求和为十",
    );
    let inj = inject_four();
    s.add("J3a-零例拒止", inj[0], "case=0 开例必须 MR_EMPTY");
    s.add("J3b-归因越界拒止", inj[1], "cause=9 必须 MR_RANGE");
    s.add("J3c-重复开例拒止", inj[2], "同号二次开例必须 MR_DUP");
    s.add("J3d-无回执销账拒", inj[3], "未挂回执直接销账必须 MR_EMPTY");
    let mut fz = match sample_ledger() {
        Ok(v) => v,
        Err(_) => RepairLedger::new(),
    };
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.open_case(11, SEG_LO, 1, 1), Err(MR_FROZEN))
        && matches!(fz.attach(1, OWNER_LO, 1, 2, 1), Err(MR_FROZEN))
        && matches!(fz.close(1), Err(MR_FROZEN))
        && matches!(fz.reopen(1, 2), Err(MR_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后开例/挂账/销账/重开全拒且指纹不变",
    );
    let fp_a = led.clone().fold();
    let mut drift = match sample_ledger() {
        Ok(v) => v,
        Err(_) => RepairLedger::new(),
    };
    let reopened = drift.reopen(2, 5).is_ok();
    s.add(
        "J5-指纹敏感性",
        reopened && drift.fold() != fp_a && matches!(drift.audit(), Err(MR_SHORT)),
        "重开改写复现次数必须改变 fold 并破坏闭合",
    );
    let mismatch = {
        let mut g = RepairLedger::new();
        g.open_case(1, SEG_LO, 1, 1).is_ok()
            && matches!(g.attach(1, OWNER_LO, 0x55, 0x55, 1), Err(MR_MISMATCH))
    };
    let attempts = {
        let mut g = RepairLedger::new();
        g.open_case(1, SEG_LO, 1, 1).is_ok()
            && matches!(g.attach(1, OWNER_LO, 0x55, 0x56, 0), Err(MR_SHORT))
            && matches!(g.attach(1, OWNER_LO, 0x55, 0x56, 4), Err(MR_SHORT))
    };
    let seg_range = matches!(
        RepairLedger::new().open_case(1, 0x1e700, 1, 1),
        Err(MR_RANGE)
    );
    let repro_range = matches!(
        RepairLedger::new().open_case(1, SEG_LO, 1, 6),
        Err(MR_RANGE)
    );
    let no_case = matches!(
        RepairLedger::new().attach(9, OWNER_LO, 1, 2, 1),
        Err(MR_EMPTY)
    );
    s.add(
        "J6-范围与空账拒止",
        mismatch && attempts && seg_range && repro_range && no_case,
        "指纹零差异/尝试越界/段越界/复现越界/无例挂账五路都必须拒",
    );
    let mut order = led.cases.len() == SEATS && led.receipts.len() == SEATS;
    k = 1;
    while k < led.cases.len() {
        if led.cases[k].case <= led.cases[k - 1].case {
            order = false;
        }
        if led.receipts[k].case != led.cases[k].case {
            order = false;
        }
        k += 1;
    }
    s.add(
        "J7-唯一与序不变式",
        order && led.cases[0].case == 1 && led.cases[9].case == 10,
        "例号 1..=10 唯一递增，回执逐位同序",
    );
    let mut partial = RepairLedger::new();
    let mut partial_ok = true;
    k = 0;
    while k < 4 {
        let case = (k as u16).wrapping_add(1);
        let seg = SEG_LO.wrapping_add((k as u32).wrapping_mul(0x20));
        partial_ok = partial_ok && partial.open_case(case, seg, 1, 1).is_ok();
        k += 1;
    }
    let mut tampered = match sample_ledger() {
        Ok(v) => v,
        Err(_) => RepairLedger::new(),
    };
    tampered.receipts[0].after = tampered.receipts[0].before;
    let tamper_hit = matches!(tampered.audit(), Err(MR_MISMATCH));
    s.add(
        "J8-半成品不得全绿",
        partial_ok && partial.cases.len() == 4 && matches!(partial.audit(), Err(MR_SHORT))
            && tamper_hit,
        "四例半账必须 MR_SHORT，回执指纹被抹平必须 MR_MISMATCH",
    );
    let receipts_ok = led.receipts.len() == SEATS
        && led.receipts.iter().all(|r| {
            r.valid() && r.owner >= OWNER_LO && r.after != r.before && r.closed
        });
    let closed_n = {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < led.cases.len() {
            if !led.cases[i].open {
                n += 1;
            }
            i += 1;
        }
        n
    };
    s.add(
        "J10-回执字段可核",
        receipts_ok && closed_n == SEATS && led.receipts[3].attempts == 1,
        "回执责任人/尝试/差异/销账位逐张可核，十例全销",
    );
    s.add_dyn(
        "J9-动态归因账",
        green == 10 && led.cases.len() == SEATS,
        &format!(
            "green={} spec={} impl={} env={} flake={} label={}",
            green,
            cc[0],
            cc[1],
            cc[2],
            cc[3],
            Cause::Impl.label()
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        led.fold() != 0 && fp_a == led.fold(),
        &format!("fold={:016x} tag={:x}", led.fold(), MR_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11076_flow() {
        let mut led = sample_ledger().unwrap_or(RepairLedger::new());
        assert_eq!(led.audit().unwrap_or(0), 10);
        assert!(led.closure());
        assert_eq!(led.cause_counts(), [3, 3, 2, 2]);
        assert!(led.receipts.iter().all(|r| r.changed() && r.closed));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11076_checks_green() {
        assert!(run_unx11076_checks().all_passed(), "UNX-F11076 判据未全绿");
    }
}
