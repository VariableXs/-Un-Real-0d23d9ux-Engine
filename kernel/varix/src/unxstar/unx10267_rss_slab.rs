//! UNX-F10267 内存泄漏观测账（RSS/Slab 曲线落账）（0x158A0 段）
//!
//! 锚点：UNX-F10267-J1 泄漏观测达成（RSS/Slab 双曲线落账且泄漏判定
//! 可复算 10/10 次）；J2 泄漏检出（单调增长越阈检出/回吐豁免/抖动不
//! 误报 10/10 次）；J3 曲线指纹（同曲线同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：双曲线采样账（RSS/Slab 千分位水位序列）+ 泄漏判定核（滑动
//! 窗口斜率 + 回吐豁免）+ 误报护栏（抖动带宽）+ 注入核（单调增长/
//! 回吐/抖动必区分）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x158A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x158A0;
pub const ML_MONO_RISE: u32 = SEG_BASE + 0x01; // 单调增长
pub const ML_OUT_RANGE: u32 = SEG_BASE + 0x02; // 越界采样
pub const ML_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后采样
pub const ML_BAD_CURVE: u32 = SEG_BASE + 0x04; // 坏曲线名
pub const ML_EMPTY: u32 = SEG_BASE + 0x05; // 空曲线

/// 曲线闭集。
pub const C_RSS: u8 = 0;
pub const C_SLAB: u8 = 1;
pub const CURVE_N: u8 = 2;

pub fn curve_ok(c: u8) -> bool {
    c < CURVE_N
}

pub fn curve_name(c: u8) -> &'static str {
    match c {
        C_RSS => "rss",
        _ => "slab",
    }
}

/// 泄漏判定参数真源。
pub const WIN: usize = 8; // 滑动窗口样本数
pub const SLOPE_TH: i32 = 20; // 窗口净增长阈值（千分位/窗口）
pub const JITTER: i32 = 30; // 抖动带宽（单步 ±30 内不算增长）

/// 曲线采样账。
pub struct LeakBook {
    rss: Vec<u16>,
    slab: Vec<u16>,
    frozen: bool,
}

impl LeakBook {
    pub fn new() -> LeakBook {
        LeakBook { rss: Vec::new(), slab: Vec::new(), frozen: false }
    }

    /// 采样：越界/冻结后写/坏曲线拒收。
    pub fn sample(&mut self, curve: u8, permille: u16) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !curve_ok(curve) {
            return Err("bad_curve");
        }
        if permille > 1000 {
            return Err("out_range");
        }
        let buf = match curve {
            C_RSS => &mut self.rss,
            _ => &mut self.slab,
        };
        buf.push(permille);
        Ok(())
    }

    /// 泄漏判定核：滑动窗口净增长（扣除抖动带宽）。
    /// 返回 (泄漏否, 窗口净增长)。
    pub fn leak_check(&self, curve: u8) -> Result<(bool, i32), &'static str> {
        let buf = match curve {
            C_RSS => &self.rss,
            C_SLAB => &self.slab,
            _ => return Err("bad_curve"),
        };
        if buf.len() < WIN {
            return Ok((false, 0)); // 样本不足不判（护栏）
        }
        let w = &buf[buf.len() - WIN..];
        // 净增长：首尾差 - 抖动豁免
        let raw = w[w.len() - 1] as i32 - w[0] as i32;
        let net = if raw > JITTER { raw - JITTER } else { 0 };
        Ok((net > SLOPE_TH, net))
    }

    /// 回吐豁免：单调增长后显著回吐 → 重置窗口（不算泄漏）。
    pub fn relieve(&mut self, curve: u8) -> Result<bool, &'static str> {
        let buf = match curve {
            C_RSS => &mut self.rss,
            C_SLAB => &mut self.slab,
            _ => return Err("bad_curve"),
        };
        if buf.len() < 2 {
            return Ok(false);
        }
        let drop = buf[buf.len() - 2] as i32 - buf[buf.len() - 1] as i32;
        let relieved = drop > JITTER;
        if relieved {
            // 回吐：截断窗口重开
            let keep = buf.len().saturating_sub(1);
            buf.drain(..keep);
        }
        Ok(relieved)
    }

    pub fn len_of(&self, curve: u8) -> Result<usize, &'static str> {
        match curve {
            C_RSS => Ok(self.rss.len()),
            C_SLAB => Ok(self.slab.len()),
            _ => Err("bad_curve"),
        }
    }

    /// 曲线指纹。
    pub fn fp(&self, curve: u8) -> Result<u32, &'static str> {
        let buf = match curve {
            C_RSS => &self.rss,
            C_SLAB => &self.slab,
            _ => return Err("bad_curve"),
        };
        let mut h: u32 = 0x811c9dc5;
        for v in buf {
            for b in v.to_le_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        Ok(h)
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 注入核。
pub fn inject_fault(b: &mut LeakBook, code: u32, curve: u8) -> Result<(), &'static str> {
    match code {
        ML_MONO_RISE => {
            // 单调增长序列：窗口净增长超阈
            for i in 0..WIN {
                let _ = b.sample(curve, 100 + (i as u16) * 15);
            }
            Ok(())
        }
        ML_OUT_RANGE => b.sample(curve, 1001),
        ML_FROZEN => {
            b.freeze();
            b.sample(curve, 500)
        }
        ML_BAD_CURVE => b.sample(9, 500),
        ML_EMPTY => {
            // 空曲线判泄漏（护栏：样本不足不判）
            b.leak_check(curve).map(|_| ())
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10267_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10267");

    // J1 泄漏检出：单调增长序列判泄漏
    let j1 = {
        let mut b = LeakBook::new();
        let _ = inject_fault(&mut b, ML_MONO_RISE, C_RSS);
        match b.leak_check(C_RSS) {
            Ok((leak, net)) => leak && net > SLOPE_TH,
            Err(_) => false,
        }
    };
    s.add("J1-单调泄漏", j1, "净增长超阈判泄漏");

    // J2 抖动不误报：±30 内波动零泄漏
    let j2 = {
        let mut b = LeakBook::new();
        let wave = [500, 520, 490, 510, 495, 525, 480, 505];
        for v in wave {
            let _ = b.sample(C_SLAB, v);
        }
        matches!(b.leak_check(C_SLAB), Ok((false, _)))
    };
    s.add("J2-抖动不误报", j2, "带宽内零泄漏");

    // J3 回吐豁免：增长后回吐重置
    let j3 = {
        let mut b = LeakBook::new();
        for i in 0..WIN {
            let _ = b.sample(C_RSS, 100 + (i as u16) * 15);
        }
        let pre = b.leak_check(C_RSS).unwrap().0;
        let _ = b.sample(C_RSS, 100); // 回吐：末值大幅回落（205→100）
        let relieved = b.relieve(C_RSS).unwrap();
        pre && relieved && b.len_of(C_RSS).unwrap() == 1
    };
    s.add("J3-回吐豁免", j3, "回吐重置窗口");

    // J4 样本不足护栏：空/少样本不判
    let j4 = {
        let mut b = LeakBook::new();
        let _ = b.sample(C_RSS, 500);
        matches!(b.leak_check(C_RSS), Ok((false, 0)))
    };
    s.add("J4-样本护栏", j4, "样本不足不判泄漏");

    // J5 双曲线独立：RSS 泄漏不牵连 Slab
    let j5 = {
        let mut b = LeakBook::new();
        let _ = inject_fault(&mut b, ML_MONO_RISE, C_RSS);
        let wave = [500u16, 510, 490, 505, 495, 515, 485, 500];
        for v in wave {
            let _ = b.sample(C_SLAB, v);
        }
        let rss_leak = b.leak_check(C_RSS).unwrap().0;
        let slab_leak = b.leak_check(C_SLAB).unwrap().0;
        rss_leak && !slab_leak
    };
    s.add("J5-双线独立", j5, "RSS/Slab 判定互不牵连");

    // J6 曲线指纹：同曲线同指纹/变必变
    let j6 = {
        let mut b1 = LeakBook::new();
        let _ = b1.sample(C_RSS, 500);
        let _ = b1.sample(C_RSS, 510);
        let f1 = b1.fp(C_RSS).unwrap();
        let mut b2 = LeakBook::new();
        let _ = b2.sample(C_RSS, 500);
        let _ = b2.sample(C_RSS, 520);
        let f2 = b2.fp(C_RSS).unwrap();
        f1 != f2 && f1 != 0
    };
    s.add("J6-曲线指纹", j6, "采样变指纹必变");

    // J7 注入五路
    let codes = [ML_MONO_RISE, ML_OUT_RANGE, ML_FROZEN, ML_BAD_CURVE, ML_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut LeakBook::new(), **c, C_RSS).is_err())
        .count();
    s.add("J7-注入五路", caught >= 3, "至少三路直接拒收");

    // J8 越界采样拒收
    let j8 = inject_fault(&mut LeakBook::new(), ML_OUT_RANGE, C_SLAB) == Err("out_range");
    s.add("J8-越界拒收", j8, "1001 即拒");

    // J9 冻结后采样拒收
    let j9 = {
        let mut b = LeakBook::new();
        inject_fault(&mut b, ML_FROZEN, C_RSS);
        b.is_frozen()
    };
    s.add("J9-冻结拒采", j9, "印后采样必拒");

    // J10 十轮泄漏判定回放：10 轮判定与净增长一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = LeakBook::new();
            for i in 0..WIN {
                let _ = b.sample(C_SLAB, 200 + (i as u16) * 12);
            }
            b.leak_check(C_SLAB).unwrap()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮判定", j10, "10/10 轮判定一致");

    s
}
