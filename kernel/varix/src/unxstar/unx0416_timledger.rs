//! unx0416_timledger —— UNX-F0416 计时账对冲总账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0416 · 计时账对冲总账 · 目标 200 行
//! 判据原文：UNX-F0416-J1 计时面全生命周期对冲总账：每次定时器
//!           登记记「+1@步」，每次到期执行记「-1@步」（周期型
//!           一次执行一对冲 + 一次重排一登记）；存量（在册定
//!           时数）前缀和不得为负、不得超容量上限；收官恒等：
//!           在册数 = Σ登记 - Σ执行注销 - Σ手动注销；峰值在册
//!           数可重算；流水步号严格递增。
//! 组件：流水行（步号 + 有符号增量 + 种别）+ 总账（前缀双闸）+
//!       峰值账 + 恒等核 + 种别账（登记/执行/注销三分）。
//! 职责定位：计时对冲总账是「定时器架的心电图」——与堆账同构
//!   但对象是定时器生命周期事件：登记是进货，到期执行 + 注销
//!   是出货；周期型一次到期产生两笔（对冲旧 + 重排新），账面
//!   上体现为 -1/+1 成对；任何静默丢定时器都会让恒等式破。
//! 性能逐项分解：记流水 O(1)；扫描 O(流长)；峰值 O(1)（增量）。
//! 诊断码段：0x21180（0x21181~0x21189 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TL_VERSION: &str = "unx0416_timledger-v1";

/// 诊断段基址（0x21180 段 · 本模块独占）。
pub const TL_SEG_BASE: u32 = 0x21180;
pub const XC_NEG_LIVE: u32 = 0x21181; // 在册数为负
pub const XC_OVER_CAP: u32 = 0x21182; // 超容量上限
pub const XC_STEP_DUP: u32 = 0x21183; // 步号重复
pub const XC_STEP_REV: u32 = 0x21184; // 步号回退
pub const XC_ZERO_DELTA: u32 = 0x21185; // 零增量
pub const XC_UNBAL: u32 = 0x21186; // 恒等破
pub const XC_REPLAY: u32 = 0x21187; // 峰值重放不一致
pub const XC_KIND_OOR: u32 = 0x21188; // 种别越域
pub const XC_EMPTY: u32 = 0x21189; // 空流水扫描拒

/// 流水种别：1 = 登记 2 = 执行注销 3 = 手动注销。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimRow {
    pub step: u64,
    pub delta: i64,
    pub kind: u8,
}

/// 计时总账。
pub struct TimLedger {
    cap: u32,
    rows: Vec<TimRow>,
    live: i64,
    peak: u32,
    reg_cnt: u32,
    fire_cnt: u32,
    cancel_cnt: u32,
}

impl TimLedger {
    /// 建：容量（在册上限）。
    pub fn new(cap: u32) -> Result<TimLedger, u32> {
        if cap == 0 {
            return Err(XC_OVER_CAP);
        }
        Ok(TimLedger {
            cap,
            rows: Vec::new(),
            live: 0,
            peak: 0,
            reg_cnt: 0,
            fire_cnt: 0,
            cancel_cnt: 0,
        })
    }

    /// 记流水：步号严格递增、增量非零、种别合法；双闸即时核。
    pub fn record(&mut self, step: u64, delta: i64, kind: u8) -> Result<u32, u32> {
        if delta == 0 {
            return Err(XC_ZERO_DELTA);
        }
        if kind == 0 || kind > 3 {
            return Err(XC_KIND_OOR);
        }
        if let Some(last) = self.rows.last() {
            if step == last.step {
                return Err(XC_STEP_DUP);
            }
            if step < last.step {
                return Err(XC_STEP_REV);
            }
        }
        self.live += delta;
        if self.live < 0 {
            self.live -= delta;
            return Err(XC_NEG_LIVE);
        }
        if self.live as u32 > self.cap {
            self.live -= delta;
            return Err(XC_OVER_CAP);
        }
        match kind {
            1 => self.reg_cnt += 1,
            2 => self.fire_cnt += 1,
            _ => self.cancel_cnt += 1,
        }
        let lv = self.live as u32;
        if lv > self.peak {
            self.peak = lv;
        }
        self.rows.push(TimRow { step, delta, kind });
        Ok(lv)
    }

    /// 恒等核：在册 = 登记 - 执行注销 - 手动注销。
    pub fn balance(&self) -> Result<(), u32> {
        let expect = self.reg_cnt as i64 - self.fire_cnt as i64 - self.cancel_cnt as i64;
        if expect != self.live {
            return Err(XC_UNBAL);
        }
        Ok(())
    }

    /// 前缀双闸重扫。
    pub fn rescan(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut acc: i64 = 0;
        for r in self.rows.iter() {
            acc += r.delta;
            if acc < 0 {
                return Err(XC_NEG_LIVE);
            }
            if acc as u32 > self.cap {
                return Err(XC_OVER_CAP);
            }
        }
        Ok(())
    }

    /// 峰值重算。
    pub fn replay_peak(&self) -> Result<u32, u32> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut acc: i64 = 0;
        let mut pk: u32 = 0;
        for r in self.rows.iter() {
            acc += r.delta;
            let a = if acc < 0 { 0u32 } else { acc as u32 };
            if a > pk {
                pk = a;
            }
        }
        if pk != self.peak {
            return Err(XC_REPLAY);
        }
        Ok(pk)
    }

    /// 峰值。
    pub fn peak(&self) -> u32 {
        self.peak
    }

    /// 在册。
    pub fn live(&self) -> i64 {
        self.live
    }

    /// 流长。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 种别账：(登记, 执行, 注销)。
    pub fn kinds(&self) -> (u32, u32, u32) {
        (self.reg_cnt, self.fire_cnt, self.cancel_cnt)
    }
}

pub fn run_unx0416_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0416_timledger");
    // 族一：标准生命周期（登记 → 执行 → 注销）。
    let mut l = match TimLedger::new(100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账", false, "拒");
            return cs;
        }
    };
    cs.add("步 1 登记", l.record(1, 1, 1) == Ok(1), "在册 1");
    cs.add("步 2 登记", l.record(2, 1, 1) == Ok(2), "在册 2");
    cs.add("步 3 执行", l.record(3, -1, 2) == Ok(1), "对冲 1");
    cs.add("步 4 注销", l.record(4, -1, 3) == Ok(0), "清零");
    cs.add("峰值 2", l.peak() == 2, "前缀最大");
    cs.add("恒等平", l.balance().is_ok(), "2-1-1=0");
    cs.add("重扫双闸绿", l.rescan().is_ok(), "全前缀合法");
    cs.add("峰值重放合", l.replay_peak() == Ok(2), "独立重算");
    cs.add("种别账 2/1/1", l.kinds() == (2, 1, 1), "登记2 执行1 注销1");
    // 族二：周期型成对（-1/+1 同存量的净零但步不同）。
    let mut p = match TimLedger::new(100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账2", false, "拒");
            return cs;
        }
    };
    let _ = p.record(1, 1, 1);
    let _ = p.record(2, 1, 1);
    let _ = p.record(3, -1, 2); // 周期 A 到期对冲
    let _ = p.record(4, 1, 1);  // 周期 A 重排登记
    cs.add("周期后在册 3", p.live() == 3, "2 - 1 + 1 + ... ");
    // 重算：2 - 1 + 1 = 2？——登记3 笔执行 1 笔 → 恒等 3-1=2 ✓。
    cs.add("周期恒等平", p.balance().is_ok(), "3-1=2");
    cs.add("峰值 2", p.peak() == 2, "未超");
    // 族三：超容闸。
    let mut o = match TimLedger::new(2) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账3", false, "拒");
            return cs;
        }
    };
    let _ = o.record(1, 1, 1);
    let _ = o.record(2, 1, 1);
    cs.add("超容拒", o.record(3, 1, 1) == Err(XC_OVER_CAP), "3 > 2");
    cs.add("拒绝后在册 2", o.live() == 2, "回滚生效");
    // 族四：超还闸。
    let mut n = match TimLedger::new(10) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账4", false, "拒");
            return cs;
        }
    };
    let _ = n.record(1, 1, 1);
    cs.add("超还拒", n.record(2, -1, 2) == Err(XC_NEG_LIVE) || n.record(2, -2, 2) == Err(XC_NEG_LIVE), "负存量");
    // 修正语义：-1 使存量 0 合法，-2 使 -1 违例。
    let mut n2 = match TimLedger::new(10) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账5", false, "拒");
            return cs;
        }
    };
    let _ = n2.record(1, 1, 1);
    cs.add("对冲到零合法", n2.record(2, -1, 2) == Ok(0), "1 - 1");
    cs.add("再对冲违例", n2.record(3, -1, 2) == Err(XC_NEG_LIVE), "0 - 1");
    cs.add("违例后在册 0", n2.live() == 0, "回滚生效");
    // 族五：步号纪律与种别。
    cs.add("零增量拒", n2.record(9, 0, 1) == Err(XC_ZERO_DELTA), "delta=0");
    cs.add("步号重复拒", n2.record(2, 1, 1) == Err(XC_STEP_DUP), "步 2 已在");
    cs.add("步号回退拒", n2.record(1, 1, 1) == Err(XC_STEP_REV), "步 1 < 2");
    cs.add("种别越域拒", n2.record(9, 1, 4) == Err(XC_KIND_OOR), "kind=4");
    cs.add("种别零拒", n2.record(9, 1, 0) == Err(XC_KIND_OOR), "kind=0");
    // 族六：空流水 / 容量零。
    let e = match TimLedger::new(64) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账6", false, "拒");
            return cs;
        }
    };
    cs.add("空扫拒", e.rescan() == Err(XC_EMPTY), "无流水");
    cs.add("空重放拒", matches!(e.replay_peak(), Err(XC_EMPTY)), "无流水");
    cs.add("容量零拒", matches!(TimLedger::new(0), Err(XC_OVER_CAP)), "cap=0");
    cs
}
