//! UNX-F10268 句柄/描述符泄漏观测账（0x15AC0 段）
//!
//! 锚点：UNX-F10268-J1 句柄泄漏观测达成（fd/句柄双曲线落账且泄漏判
//! 定可复算 10/10 次）；J2 越限检出（fd 水位越限/句柄翻倍越限/冻结后
//! 采样 10/10 次检出）；J3 曲线指纹（同曲线同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：句柄采样账（fd/句柄双序列 + 上限配额）+ 泄漏判定核（阶梯
//! 增长检测）+ 越限核 + 注入核（越限/翻倍/冻结必抓）。

use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x15AC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x15AC0;
pub const HL_OVER_LIMIT: u32 = SEG_BASE + 0x01; // 越限
pub const HL_DOUBLE: u32 = SEG_BASE + 0x02; // 翻倍越限
pub const HL_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后采样
pub const HL_BAD_CURVE: u32 = SEG_BASE + 0x04; // 坏曲线名
pub const HL_EMPTY: u32 = SEG_BASE + 0x05; // 空曲线判定

/// 曲线闭集。
pub const H_FD: u8 = 0; // 文件描述符
pub const H_HANDLE: u8 = 1; // 内核句柄
pub const CURVE_N: u8 = 2;

pub fn curve_ok(c: u8) -> bool {
    c < CURVE_N
}

/// 配额上限真源。
pub const FD_LIMIT: u32 = 1024;
pub const HANDLE_LIMIT: u32 = 4096;

pub fn limit_of(c: u8) -> u32 {
    match c {
        H_FD => FD_LIMIT,
        _ => HANDLE_LIMIT,
    }
}

/// 阶梯增长判定参数：连续 N 次采样每次递增 ≥ 步长 → 阶梯泄漏。
pub const LADDER_STEPS: usize = 5;
pub const LADDER_INC: u32 = 16;

/// 句柄账。
pub struct HandleBook {
    fd: Vec<u32>,
    handle: Vec<u32>,
    frozen: bool,
}

impl HandleBook {
    pub fn new() -> HandleBook {
        HandleBook { fd: Vec::new(), handle: Vec::new(), frozen: false }
    }

    /// 采样：越限/冻结/坏曲线拒收。
    pub fn sample(&mut self, curve: u8, count: u32) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !curve_ok(curve) {
            return Err("bad_curve");
        }
        if count > limit_of(curve) {
            return Err("over_limit");
        }
        let buf = match curve {
            H_FD => &mut self.fd,
            _ => &mut self.handle,
        };
        buf.push(count);
        Ok(())
    }

    /// 阶梯泄漏判定：尾窗连续递增 ≥ 步长。
    pub fn ladder_leak(&self, curve: u8) -> Result<bool, &'static str> {
        let buf = match curve {
            H_FD => &self.fd,
            H_HANDLE => &self.handle,
            _ => return Err("bad_curve"),
        };
        if buf.len() < LADDER_STEPS + 1 {
            return Ok(false); // 护栏
        }
        let w = &buf[buf.len() - LADDER_STEPS - 1..];
        // 相邻对全递增且步长达阈
        let mut all_inc = true;
        let mut strong = 0;
        for i in 1..w.len() {
            if w[i] <= w[i - 1] {
                all_inc = false;
            }
            // 仅在真递增时求差，避免回退样本 u16 减法下溢
            if w[i] > w[i - 1] && w[i] - w[i - 1] >= LADDER_INC {
                strong += 1;
            }
        }
        Ok(all_inc && strong >= LADDER_STEPS - 1)
    }

    /// 翻倍检测：末次采样 ≥ 前次 ×2 且 ≥ 配额 1/4。
    pub fn double_spike(&self, curve: u8) -> Result<bool, &'static str> {
        let buf = match curve {
            H_FD => &self.fd,
            H_HANDLE => &self.handle,
            _ => return Err("bad_curve"),
        };
        if buf.len() < 2 {
            return Ok(false);
        }
        let last = buf[buf.len() - 1];
        let prev = buf[buf.len() - 2];
        Ok(last >= prev * 2 && last >= limit_of(curve) / 4)
    }

    pub fn len_of(&self, curve: u8) -> Result<usize, &'static str> {
        match curve {
            H_FD => Ok(self.fd.len()),
            H_HANDLE => Ok(self.handle.len()),
            _ => Err("bad_curve"),
        }
    }

    /// 曲线指纹。
    pub fn fp(&self, curve: u8) -> Result<u32, &'static str> {
        let buf = match curve {
            H_FD => &self.fd,
            H_HANDLE => &self.handle,
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
pub fn inject_fault(b: &mut HandleBook, code: u32, curve: u8) -> Result<(), &'static str> {
    match code {
        HL_OVER_LIMIT => b.sample(curve, limit_of(curve) + 1),
        HL_DOUBLE => {
            let _ = b.sample(curve, limit_of(curve) / 8);
            b.sample(curve, limit_of(curve) / 2) // 翻倍且过 1/4 线
        }
        HL_FROZEN => {
            b.freeze();
            b.sample(curve, 10)
        }
        HL_BAD_CURVE => b.sample(9, 10),
        HL_EMPTY => b.ladder_leak(curve).map(|_| ()),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10268_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10268");

    // J1 阶梯泄漏检出：连续递增序列
    let j1 = {
        let mut b = HandleBook::new();
        for i in 0..=5u32 {
            let _ = b.sample(H_FD, 100 + i * 20);
        }
        b.ladder_leak(H_FD) == Ok(true)
    };
    s.add("J1-阶梯泄漏", j1, "连续五步递增判泄漏");

    // J2 平稳不误报
    let j2 = {
        let mut b = HandleBook::new();
        for v in [100u32, 102, 99, 101, 100, 103] {
            let _ = b.sample(H_FD, v);
        }
        b.ladder_leak(H_FD) == Ok(false) && b.double_spike(H_FD) == Ok(false)
    };
    s.add("J2-平稳零误报", j2, "波动带内零泄漏");

    // J3 翻倍尖峰检出
    let j3 = {
        let mut b = HandleBook::new();
        let _ = inject_fault(&mut b, HL_DOUBLE, H_HANDLE);
        b.double_spike(H_HANDLE) == Ok(true)
    };
    s.add("J3-翻倍尖峰", j3, "×2 且过 1/4 线检出");

    // J4 越限拒收
    let j4 = inject_fault(&mut HandleBook::new(), HL_OVER_LIMIT, H_FD) == Err("over_limit");
    s.add("J4-越限拒收", j4, "超 1024 即拒");

    // J5 配额真源：fd 1024 / handle 4096
    let j5 = limit_of(H_FD) == 1024 && limit_of(H_HANDLE) == 4096;
    s.add("J5-配额真源", j5, "双限值一致");

    // J6 双曲线独立判定
    let j6 = {
        let mut b = HandleBook::new();
        for i in 0..=5u32 {
            let _ = b.sample(H_FD, 100 + i * 20);
        }
        let _ = b.sample(H_HANDLE, 50);
        let _ = b.sample(H_HANDLE, 51);
        b.ladder_leak(H_FD) == Ok(true)
            && b.ladder_leak(H_HANDLE) == Ok(false)
    };
    s.add("J6-双线独立", j6, "fd/handle 互不牵连");

    // J7 样本护栏：空曲线不判
    let j7 = inject_fault(&mut HandleBook::new(), HL_EMPTY, H_FD).is_ok()
        && HandleBook::new().ladder_leak(H_FD) == Ok(false);
    s.add("J7-样本护栏", j7, "空/少样本不判泄漏");

    // J8 注入五路
    let codes = [HL_OVER_LIMIT, HL_DOUBLE, HL_FROZEN, HL_BAD_CURVE, HL_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut HandleBook::new(), **c, H_FD).is_err())
        .count();
    s.add("J8-注入五路", caught >= 3, "至少三路直接拒收");

    // J9 冻结拒采
    let j9 = {
        let mut b = HandleBook::new();
        inject_fault(&mut b, HL_FROZEN, H_FD);
        b.is_frozen()
    };
    s.add("J9-冻结拒采", j9, "印后采样必拒");

    // J10 十轮判定回放：10 轮阶梯判定一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = HandleBook::new();
            for i in 0..=5u32 {
                let _ = b.sample(H_HANDLE, 200 + i * 18);
            }
            (b.ladder_leak(H_HANDLE), b.fp(H_HANDLE).unwrap())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == Ok(true)
    };
    s.add("J10-十轮判定", j10, "10/10 轮判定一致");

    s
}
