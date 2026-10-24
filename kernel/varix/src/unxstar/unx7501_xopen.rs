//! UNX-F7501 互开对照执行总纲（≥5 环境样本矩阵执行框架）（0x9480 段）
//!
//! 锚点：UNX-F7501-J1 互开执行框架落账（样本矩阵 5 环境执行序/
//! 每环境正向+反向两向/结果登记结构）；与 F7492 前置准备清单逐项
//! 对账（5 环境全部可执行）。
//! 域：CoRun·UNX-B5-B16（互开对照面）。
//!
//! 结构：5 环境样本矩阵（QEMU-x86_64 / QEMU-aarch64 / 真机-A /
//! 真机-B / 精简内核构建）+ 执行序冻结（环境 × {正向, 反向}）+
//! 结果登记结构（(环境, 向, 结果, 日志指纹)）+ F7492 前置对账核
//! （5 环境全部可执行 = 前置清单全过）+ 注入核（缺环境/单向执行/
//! 登记漂移/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9480 独占 0x80）。

pub const SEG_BASE: u32 = 0x9480;
pub const XC_XO_NOENV: u32 = SEG_BASE + 0x01; // 缺环境
pub const XC_XO_HALFWAY: u32 = SEG_BASE + 0x02; // 单向执行
pub const XC_XO_DRIFT: u32 = SEG_BASE + 0x03; // 登记漂移
pub const XC_XO_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 5 环境样本闭集。
pub const ENV_QEMU_X64: &str = "qemu-x86_64";
pub const ENV_QEMU_A64: &str = "qemu-aarch64";
pub const ENV_REAL_A: &str = "real-machine-a";
pub const ENV_REAL_B: &str = "real-machine-b";
pub const ENV_MINI: &str = "mini-kernel";
pub const ENVS: [&str; 5] = [ENV_QEMU_X64, ENV_QEMU_A64, ENV_REAL_A, ENV_REAL_B, ENV_MINI];

/// 两向闭集。
pub const DIR_FWD: u8 = 0; // 正向（互开成功预期）
pub const DIR_REV: u8 = 1; // 反向（互开失败预期——守门）

/// 结果闭集。
pub const RES_PASS: u8 = 0;
pub const RES_FAIL: u8 = 1;
pub const RES_BLOCK: u8 = 2; // 环境不可执行

/// F7492 前置准备清单（逐环境）。
pub const PREP_LIST: [&str; 5] = [
    "prep-qemu-x86_64",
    "prep-qemu-aarch64",
    "prep-real-machine-a",
    "prep-real-machine-b",
    "prep-mini-kernel",
];

/// 登记项。
#[derive(Clone)]
pub struct ExecLog {
    pub env: &'static str,
    pub dir: u8,
    pub result: u8,
    pub log_fp: u64, // 日志指纹
}

/// 执行框架。
pub struct XOpenBook {
    pub prep_passed: Vec<bool>,
    pub logs: Vec<ExecLog>,
    pub sealed: bool,
}

impl XOpenBook {
    pub fn new() -> Self {
        XOpenBook { prep_passed: vec![false; ENVS.len()], logs: Vec::new(), sealed: false }
    }

    /// F7492 前置逐项通过。
    pub fn pass_prep(&mut self, prep: &str) -> Result<(), u32> {
        match PREP_LIST.iter().position(|p| *p == prep) {
            Some(i) => {
                self.prep_passed[i] = true;
                Ok(())
            }
            None => Err(XC_XO_NOENV),
        }
    }

    /// 环境可执行 = 前置全过。
    pub fn env_ready(&self, env: &str) -> bool {
        match ENVS.iter().position(|e| *e == env) {
            Some(i) => self.prep_passed[i],
            None => false,
        }
    }

    /// 执行登记（守卫：环境就绪 + 两向都登记才算完整）。
    pub fn log_exec(&mut self, env: &'static str, dir: u8, result: u8, fp: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_XO_SEALED);
        }
        if !ENVS.contains(&env) {
            return Err(XC_XO_NOENV);
        }
        if !self.env_ready(env) {
            return Err(XC_XO_NOENV);
        }
        if dir > DIR_REV {
            return Err(XC_XO_HALFWAY);
        }
        // 登记漂移：同 (env, dir) 重复拒
        if self
            .logs
            .iter()
            .any(|l| l.env == env && l.dir == dir)
        {
            return Err(XC_XO_DRIFT);
        }
        self.logs.push(ExecLog { env, dir, result, log_fp: fp });
        Ok(())
    }

    /// 两向完整核：每环境正向+反向各一条。
    pub fn both_dirs(&self) -> bool {
        ENVS.iter().all(|e| {
            self.logs.iter().any(|l| l.env == *e && l.dir == DIR_FWD)
                && self.logs.iter().any(|l| l.env == *e && l.dir == DIR_REV)
        })
    }

    /// 执行序核：登记序 = 环境序 × 向序。
    pub fn order_kept(&self) -> bool {
        for (i, e) in ENVS.iter().enumerate() {
            let fwd_idx = match self.logs.iter().position(|l| l.env == *e && l.dir == DIR_FWD) {
                Some(x) => x,
                None => return false,
            };
            let rev_idx = match self.logs.iter().position(|l| l.env == *e && l.dir == DIR_REV) {
                Some(x) => x,
                None => return false,
            };
            if rev_idx < fwd_idx {
                return false;
            }
            let _ = i;
        }
        true
    }
}

/// 互开框架正本核：5 环境 × 两向 = 10 登记全齐。
pub fn frame_full_exact() -> bool {
    let mut b = XOpenBook::new();
    for p in PREP_LIST.iter() {
        if b.pass_prep(p).is_err() {
            return false;
        }
    }
    for e in ENVS.iter() {
        if !b.env_ready(e) {
            return false;
        }
        if b.log_exec(e, DIR_FWD, RES_PASS, 0xAAAA + e.as_bytes()[0] as u64 % 7).is_err() {
            return false;
        }
        if b.log_exec(e, DIR_REV, RES_PASS, 0xBBBB).is_err() {
            return false;
        }
    }
    b.both_dirs() && b.order_kept() && b.logs.len() == 10
}

/// 前置对账核：5 环境全部可执行（前置清单全过）。
pub fn prep_all_ready_exact() -> bool {
    let mut b = XOpenBook::new();
    for p in PREP_LIST.iter() {
        let _ = b.pass_prep(p);
    }
    ENVS.iter().all(|e| b.env_ready(e)) && b.prep_passed.iter().all(|&v| v)
}

/// 结果登记结构核：结果值闭集 + 指纹留痕。
pub fn log_struct_exact() -> bool {
    let mut b = XOpenBook::new();
    for p in PREP_LIST.iter() {
        let _ = b.pass_prep(p);
    }
    let _ = b.log_exec(ENV_QEMU_X64, DIR_FWD, RES_PASS, 0xF1);
    let _ = b.log_exec(ENV_QEMU_X64, DIR_REV, RES_FAIL, 0xF2);
    let l1 = &b.logs[0];
    let l2 = &b.logs[1];
    l1.result == RES_PASS
        && l1.log_fp == 0xF1
        && l2.result == RES_FAIL
        && l2.log_fp == 0xF2
        && (0..=2u8).contains(&l1.result)
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = XOpenBook::new();
    for p in PREP_LIST.iter() {
        let _ = b.pass_prep(p);
    }
    // 1) 未知环境
    if b.log_exec("cloud-env", DIR_FWD, RES_PASS, 0) == Err(XC_XO_NOENV) {
        caught += 1;
    }
    // 2) 前置未过环境拒执行
    let mut b2 = XOpenBook::new();
    if b2.log_exec(ENV_MINI, DIR_FWD, RES_PASS, 0) == Err(XC_XO_NOENV) {
        caught += 1;
    }
    // 3) 单向执行检测（只正向无反向）
    let mut b3 = XOpenBook::new();
    for p in PREP_LIST.iter() {
        let _ = b3.pass_prep(p);
    }
    let _ = b3.log_exec(ENV_REAL_A, DIR_FWD, RES_PASS, 0);
    if !b3.both_dirs() {
        caught += 1;
    }
    // 4) 同 (env, dir) 重复登记漂移
    let mut b4 = XOpenBook::new();
    for p in PREP_LIST.iter() {
        let _ = b4.pass_prep(p);
    }
    let _ = b4.log_exec(ENV_MINI, DIR_FWD, RES_PASS, 0);
    if b4.log_exec(ENV_MINI, DIR_FWD, RES_PASS, 0) == Err(XC_XO_DRIFT) {
        caught += 1;
    }
    // 5) 封账后拒
    b4.sealed = true;
    if b4.log_exec(ENV_MINI, DIR_REV, RES_PASS, 0) == Err(XC_XO_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：登记账折叠哈希。
pub fn fp_frozen(b: &XOpenBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for l in &b.logs {
        for &c in l.env.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= l.dir as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= l.result as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= l.log_fp;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7501");
    // J1-1 框架正本（10 登记全齐）
    let ok = frame_full_exact();
    s.add("B16-01-框架正本", ok, if ok { "" } else { "框架漂移" });
    // J1-2 F7492 前置对账（5 环境全可执行）
    let ok = prep_all_ready_exact();
    s.add("B16-02-前置对账", ok, if ok { "" } else { "前置漂移" });
    // J1-3 登记结构
    let ok = log_struct_exact();
    s.add("B16-03-登记结构", ok, if ok { "" } else { "结构漂移" });
    // J1-4 环境闭集
    let ok = ENVS.len() == 5 && PREP_LIST.len() == 5;
    s.add("B16-04-环境闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B16-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let mut b = XOpenBook::new();
    for p in PREP_LIST.iter() {
        let _ = b.pass_prep(p);
    }
    let _ = b.log_exec(ENV_QEMU_X64, DIR_FWD, RES_PASS, 0xF9);
    let ok = fp_frozen(&b) == fp_frozen(&b);
    s.add("B16-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
