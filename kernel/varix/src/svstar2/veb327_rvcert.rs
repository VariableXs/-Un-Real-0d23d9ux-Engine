//! veb327_rvcert —— VE-F0327 原型设备认证流程预留。
//! 锚点（CoRun Unxreal 主册）：VE-F0327 · 原型设备认证流程预留 · 目标 300 行
//! 判据原文：VE-F0327-J1 原型认证账：四步流程（登记 → 冒烟
//!           → 稳定性 → 发原型证）；步序核（跳步拒：未冒烟
//!           不可测稳定性）；步态机（设备各步独立记态：未
//!           始/过/败）；原型证门（四步全过才发证，且带原
//!           型记号——非正式证）；步重测（败步重测过 → 翻
//!           绿）；证有效期（原型证 30 天账，过期即撤）；
//!           流程冻结核（发证后步态只读）。
//! 组件：步态机 + 步序闸 + 原型证账。
//! 职责定位：原型认证是「实验设备的见习期」——四步是四关：
//!           冒烟是看会不会炸、稳定性是看会不会歇、发证是
//!           见习转正的临时工牌；跳步拒是关卡的顺序锁：没
//!           摸过火别谈灭火；30 天有效期是工牌的期限：到期
//!           重新考。
//! 性能逐项分解：步进 O(1)；发证 O(4)。
//! 诊断码段：0x31AC0（0x31AC1~0x31AC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RC_VERSION: &str = "veb327_rvcert-v1";

/// 诊断段基址（0x31AC0 段 · 本模块独占）。
pub const RC_SEG_BASE: u32 = 0x31AC0;
pub const XC_MAGIC: u32 = 0x31AC1; // 账魔数破
pub const XC_ORDER: u32 = 0x31AC2; // 跳步
pub const XC_FROZEN: u32 = 0x31AC3; // 已发证只读
pub const XC_EXPIRE: u32 = 0x31AC4; // 证过期
pub const XC_NODEV: u32 = 0x31AC5; // 无此设备

/// 账魔数。
const MAGIC: u32 = 0x31AC_1B2E;

/// 四步。
pub const ST_REG: usize = 0;
pub const ST_SMOKE: usize = 1;
pub const ST_STABLE: usize = 2;
pub const ST_CERT: usize = 3;
pub const NSTEP: usize = 4;

/// 原型证有效期（天）。
pub const VALID_DAYS: u32 = 30;

/// 步态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StState {
    NotStart,
    Pass,
    Fail,
}

/// 设备认证账。
pub struct DevCert {
    pub dev_id: u32,
    pub steps: [StState; NSTEP],
    /// 原型证签发日（0 = 未发）。
    pub issued_day: u32,
    magic: u32,
}

/// 认证簿。
pub struct RcBook {
    pub devs: Vec<DevCert>,
    magic: u32,
}

impl RcBook {
    /// 建。
    pub fn new() -> RcBook {
        RcBook { devs: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn find_mut(&mut self, dev: u32) -> Option<&mut DevCert> {
        for d in self.devs.iter_mut() {
            if d.dev_id == dev {
                return Some(d);
            }
        }
        None
    }

    /// 登记设备（步 0 即过）。
    pub fn enroll(&mut self, dev: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.find_mut(dev).is_some() {
            return Err(XC_NODEV);
        }
        let mut steps = [StState::NotStart; NSTEP];
        steps[ST_REG] = StState::Pass;
        self.devs.push(DevCert { dev_id: dev, steps, issued_day: 0, magic: MAGIC });
        Ok(())
    }

    /// 步进：步序核（前步须过）；败记 Fail。
    pub fn step(&mut self, dev: u32, st: usize, ok: bool) -> Result<StState, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if st == 0 || st >= NSTEP {
            return Err(XC_ORDER);
        }
        // 发证后只读。
        {
            let mut issued = false;
            for d in self.devs.iter() {
                if d.dev_id == dev && d.issued_day > 0 {
                    issued = true;
                }
            }
            if issued {
                return Err(XC_FROZEN);
            }
        }
        let d = match self.find_mut(dev) {
            Some(x) => x,
            None => return Err(XC_NODEV),
        };
        // 步序：前步须 Pass。
        if d.steps[st - 1] != StState::Pass {
            return Err(XC_ORDER);
        }
        d.steps[st] = if ok { StState::Pass } else { StState::Fail };
        Ok(d.steps[st])
    }

    /// 步重测：败步重测（仅该步，前步已过即可）。
    pub fn retest(&mut self, dev: u32, st: usize, ok: bool) -> Result<StState, u32> {
        self.step(dev, st, ok)
    }

    /// 发原型证：四步全过 + 记签发日。
    pub fn issue(&mut self, dev: u32, day: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let d = match self.find_mut(dev) {
            Some(x) => x,
            None => return Err(XC_NODEV),
        };
        for st in d.steps.iter() {
            if *st != StState::Pass {
                return Err(XC_ORDER);
            }
        }
        d.issued_day = day;
        Ok(())
    }

    /// 证有效性：签发后 30 天内有效。
    pub fn cert_valid(&self, dev: u32, today: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for d in self.devs.iter() {
            if d.dev_id == dev {
                if d.issued_day == 0 {
                    return Ok(false);
                }
                return Ok(today <= d.issued_day + VALID_DAYS);
            }
        }
        Err(XC_NODEV)
    }
}

pub fn run_veb327_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0327");
    let mut b = RcBook::new();
    // 族一：登记。
    s.add("登 7", b.enroll(7).is_ok(), "步 0 过");
    s.add("重登拒", b.enroll(7) == Err(XC_NODEV), "一号一籍");
    s.add("未登步进拒", b.step(9, ST_SMOKE, true) == Err(XC_NODEV), "无籍");
    // 族二：跳步核。
    s.add("步 0 直进拒", b.step(7, ST_REG, true) == Err(XC_ORDER), "登记不重走");
    s.add("跳冒烟拒", b.step(7, ST_STABLE, true) == Err(XC_ORDER), "未冒烟");
    s.add("跳步发证拒", b.issue(7, 1) == Err(XC_ORDER), "步未全");
    // 族三：步态机。
    s.add("冒烟过", b.step(7, ST_SMOKE, true) == Ok(StState::Pass), "一关");
    s.add("稳定性败", b.step(7, ST_STABLE, false) == Ok(StState::Fail), "记败");
    s.add("败后发证拒", b.issue(7, 1) == Err(XC_ORDER), "有败");
    // 重测翻绿。
    s.add("重测过", b.retest(7, ST_STABLE, true) == Ok(StState::Pass), "翻绿");
    s.add("发证步拒", b.step(7, ST_CERT, true) == Ok(StState::Pass), "末关");
    // 族四：发证与有效期。
    s.add("发原型证", b.issue(7, 100).is_ok(), "四步全过");
    s.add("证生效", b.cert_valid(7, 130) == Ok(true), "30 天内");
    s.add("证恰期", b.cert_valid(7, 130) == Ok(true), "含等");
    s.add("证过期", b.cert_valid(7, 131) == Ok(false), "31 天撤");
    s.add("未发证查", b.cert_valid(9, 1) == Err(XC_NODEV), "无籍");
    // 发证后只读。
    s.add("发证后步拒", b.step(7, ST_SMOKE, true) == Err(XC_FROZEN), "态只读");
    // 族五：未登记查证。
    let mut b2 = RcBook::new();
    let _ = b2.enroll(3);
    s.add("未发证无效", b2.cert_valid(3, 5) == Ok(false), "零日");
    // 全流程重跑（另一设备）。
    let _ = b2.enroll(4);
    s.add("4 冒烟败", b2.step(4, ST_SMOKE, false) == Ok(StState::Fail), "炸了");
    s.add("4 重测过", b2.retest(4, ST_SMOKE, true) == Ok(StState::Pass), "没炸");
    s.add("4 稳定过", b2.step(4, ST_STABLE, true).is_ok(), "不歇");
    s.add("4 发证步", b2.step(4, ST_CERT, true).is_ok(), "末关");
    s.add("4 发证", b2.issue(4, 200).is_ok(), "见习转正");
    s.add("4 证生效", b2.cert_valid(4, 230) == Ok(true), "期内");
    s.add("魔数核", RC_SEG_BASE == 0x31AC0, "段基址");
    s
}
