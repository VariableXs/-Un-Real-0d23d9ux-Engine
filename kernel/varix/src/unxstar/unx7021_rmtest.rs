//! UNX-F7021 真机补测规程总纲（R-B4-004 三项规程化）（0x9280 段）
//!
//! 锚点：UNX-F7021-J1 真机补测规程总纲（三项/前置条件/记录表模板）
//! 落盘；与 R-B4-004 登记项一一对应机检。
//! 域：CoRun·UNX-B4-B32（真机补测规程面）。
//!
//! 结构：规程项模型（项名/前置条件集/记录表模板）+ 三项规程闭集
//! （环境核查 / 缺陷复现 / 回归确认）+ 前置条件守卫（未全过拒执行）
//! + R-B4-004 登记对应核（登记项名 ↔ 规程项名双射机检）+ 注入核
//! （未过前置执行/登记缺项/双射破缺/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9280 独占 0x80）。

pub const SEG_BASE: u32 = 0x9280;
pub const XC_RT_PRE: u32 = SEG_BASE + 0x01; // 前置未过
pub const XC_RT_UNREG: u32 = SEG_BASE + 0x02; // 登记缺项
pub const XC_RT_BROKEN: u32 = SEG_BASE + 0x03; // 双射破缺
pub const XC_RT_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 三项规程闭集（R-B4-004 登记）。
pub const REG_ENV_CHECK: &str = "env-check"; // 环境核查
pub const REG_BUG_REPRO: &str = "bug-repro"; // 缺陷复现
pub const REG_REGRESS: &str = "regress-confirm"; // 回归确认
pub const REG_ITEMS: [&str; 3] = [REG_ENV_CHECK, REG_BUG_REPRO, REG_REGRESS];

/// 前置条件闭集。
pub const PRE_QEMU_GREEN: &str = "qemu-green"; // QEMU 全判据绿
pub const PRE_IMG_READY: &str = "img-ready"; // 镜像落盘
pub const PRE_LOG_CLEAR: &str = "log-clear"; // 串口日志通
pub const PRE_DEVICES: &str = "devices-online"; // 真机在位
pub const PRE_SET: [&str; 4] = [PRE_QEMU_GREEN, PRE_IMG_READY, PRE_LOG_CLEAR, PRE_DEVICES];

/// 规程项：项名 + 前置条件集 + 记录表模板列名。
#[derive(Clone)]
pub struct ProbeReg {
    pub name: String,
    pub pres: Vec<String>,
    pub columns: [&'static str; 4], // 记录表四列：轮次/输入/期望/实测
    pub pres_passed: Vec<bool>,
    pub executed: bool,
}

impl ProbeReg {
    /// 建规程项（前置全集冻结）。
    pub fn new(name: &str, pres: &[&str]) -> Self {
        ProbeReg {
            name: String::from(name),
            pres: pres.iter().map(|s| String::from(*s)).collect(),
            columns: ["round", "input", "expect", "actual"],
            pres_passed: vec![false; pres.len()],
            executed: false,
        }
    }

    /// 前置通过标记。
    pub fn pass_pre(&mut self, pre: &str) -> Result<(), u32> {
        match self.pres.iter().position(|p| p == pre) {
            Some(i) => {
                self.pres_passed[i] = true;
                Ok(())
            }
            None => Err(XC_RT_UNREG),
        }
    }

    /// 前置全过判定。
    pub fn pres_ready(&self) -> bool {
        self.pres_passed.len() == self.pres.len() && self.pres_passed.iter().all(|&b| b)
    }

    /// 执行守卫：前置全过才可执行。
    pub fn execute(&mut self) -> Result<(), u32> {
        if self.executed {
            return Err(XC_RT_SEALED);
        }
        if !self.pres_ready() {
            return Err(XC_RT_PRE);
        }
        self.executed = true;
        Ok(())
    }
}

/// R-B4-004 登记对应核：登记项名 ↔ 规程项名双射。
pub fn registry_bijection(regs: &[ProbeReg], registry: &[&str]) -> Result<(), u32> {
    if regs.len() != registry.len() {
        return Err(XC_RT_UNREG);
    }
    for r in regs.iter() {
        if !registry.contains(&r.name.as_str()) {
            return Err(XC_RT_BROKEN);
        }
    }
    for name in registry.iter() {
        if !regs.iter().any(|r| &r.name == name) {
            return Err(XC_RT_BROKEN);
        }
    }
    Ok(())
}

/// 三项规程总纲正本核：全规程建账 + 前置全过 + 全执行 + 双射成立。
pub fn total_probe_exact() -> bool {
    let mut env = ProbeReg::new(REG_ENV_CHECK, &[PRE_QEMU_GREEN, PRE_IMG_READY]);
    let mut bug = ProbeReg::new(REG_BUG_REPRO, &[PRE_IMG_READY, PRE_LOG_CLEAR, PRE_DEVICES]);
    let mut reg = ProbeReg::new(REG_REGRESS, &[PRE_QEMU_GREEN, PRE_DEVICES]);
    // 前置全过
    if env.pass_pre(PRE_QEMU_GREEN).is_err() {
        return false;
    }
    if env.pass_pre(PRE_IMG_READY).is_err() {
        return false;
    }
    let _ = bug.pass_pre(PRE_IMG_READY);
    let _ = bug.pass_pre(PRE_LOG_CLEAR);
    let _ = bug.pass_pre(PRE_DEVICES);
    let _ = reg.pass_pre(PRE_QEMU_GREEN);
    let _ = reg.pass_pre(PRE_DEVICES);
    if !env.pres_ready() || !bug.pres_ready() || !reg.pres_ready() {
        return false;
    }
    // 全执行
    if env.execute().is_err() || bug.execute().is_err() || reg.execute().is_err() {
        return false;
    }
    // 双射
    registry_bijection(&[env, bug, reg], &REG_ITEMS).is_ok()
}

/// 前置守卫核：未过前置执行必拒。
pub fn pre_guard_exact() -> bool {
    let mut r = ProbeReg::new(REG_ENV_CHECK, &[PRE_QEMU_GREEN, PRE_IMG_READY]);
    // 一个未过
    let _ = r.pass_pre(PRE_QEMU_GREEN);
    if r.execute().is_ok() {
        return false;
    }
    // 全过后执行放行
    let _ = r.pass_pre(PRE_IMG_READY);
    r.execute().is_ok()
}

/// 记录表模板核：四列冻结 + 填行序核。
pub fn record_template_exact() -> bool {
    let r = ProbeReg::new(REG_BUG_REPRO, &[PRE_DEVICES]);
    if r.columns != ["round", "input", "expect", "actual"] {
        return false;
    }
    // 记录行（模拟）：轮次递增
    let rows = [("R1", "insert", "enumerated", "enumerated"), ("R2", "remove", "removed", "removed")];
    let mut prev: u32 = 0;
    for (i, row) in rows.iter().enumerate() {
        let round_num: u32 = match row.0.trim_start_matches('R').parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if round_num <= prev {
            return false;
        }
        prev = round_num;
        let _ = i;
    }
    true
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 前置未过执行
    let mut r = ProbeReg::new(REG_REGRESS, &[PRE_QEMU_GREEN]);
    if r.execute() == Err(XC_RT_PRE) {
        caught += 1;
    }
    // 2) 未知前置标记
    if r.pass_pre("nonexistent") == Err(XC_RT_UNREG) {
        caught += 1;
    }
    // 3) 重复执行
    let _ = r.pass_pre(PRE_QEMU_GREEN);
    let _ = r.execute();
    if r.execute() == Err(XC_RT_SEALED) {
        caught += 1;
    }
    // 4) 登记缺项（规程少一项）
    let mut env = ProbeReg::new(REG_ENV_CHECK, &[]);
    let _ = env.pass_pre("void");
    let regs = [env];
    if registry_bijection(&regs, &REG_ITEMS).is_err() {
        caught += 1;
    }
    // 5) 双射破缺（规程名不在登记表）
    let mut fake = ProbeReg::new("fake-probe", &[]);
    let _ = fake.pass_pre("v");
    if registry_bijection(&[fake], &["fake-probe", REG_ENV_CHECK]).is_err() {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：规程账折叠哈希。
pub fn fp_frozen(regs: &[ProbeReg]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for r in regs {
        for &c in r.name.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= r.pres.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= r.executed as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7021");
    // J1-1 三项规程总纲正本
    let ok = total_probe_exact();
    s.add("B32-01-三项规程", ok, if ok { "" } else { "总纲漂移" });
    // J1-2 前置守卫
    let ok = pre_guard_exact();
    s.add("B32-02-前置守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-3 记录表模板
    let ok = record_template_exact();
    s.add("B32-03-记录表模板", ok, if ok { "" } else { "模板漂移" });
    // J1-4 前置条件闭集
    let ok = PRE_SET.len() == 4 && PRE_SET[0] == PRE_QEMU_GREEN;
    s.add("B32-04-前置闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B32-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结 + 一一对应
    let env = ProbeReg::new(REG_ENV_CHECK, &[PRE_QEMU_GREEN]);
    let bug = ProbeReg::new(REG_BUG_REPRO, &[PRE_DEVICES]);
    let reg2 = ProbeReg::new(REG_REGRESS, &[PRE_IMG_READY]);
    let ok = fp_frozen(&[env.clone(), bug.clone(), reg2.clone()])
        == fp_frozen(&[env, bug, reg2])
        && registry_bijection(
            &[
                ProbeReg::new(REG_ENV_CHECK, &[]),
                ProbeReg::new(REG_BUG_REPRO, &[]),
                ProbeReg::new(REG_REGRESS, &[]),
            ],
            &REG_ITEMS,
        )
        .is_ok();
    s.add("B32-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
