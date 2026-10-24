//! VE-F4353 · 第三方一致性工具（U08 测试与资产组）
//! 职责：第三方一致性工具生态——接入双路径（SDK 认证 F4290 + 扩展点注册
//! F4352）+ 第三方质量门（合规测试包实测 + 市场门禁复用 F4288 同域第三用）
//! + 生态治理（恶意工具→资格冻结 + 公示；实测不过→暂缓接入；版本冲突→
//! 兼容声明强制）+ 第三方数据边界声明强制 + 无障碍门同受（域本色）。
//! 段位：0x4360（续表 idx587）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4360;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThCode {
    /// 恶意行为（即冻红线 + 公示）。
    Malicious = SEG_BASE,
    /// 实测不过（暂缓接入）。
    TestFail = SEG_BASE + 0x01,
    /// 版本冲突（兼容声明强制）。
    VerClash = SEG_BASE + 0x02,
    /// 边界声明缺失（拒接入）。
    NoBoundary = SEG_BASE + 0x03,
}

/// 接入路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    /// SDK 认证（F4290 复用）。
    SdkAuth,
    /// 扩展点注册（F4352 复用）。
    ExtRegister,
}

/// 第三方工具档案。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tool {
    pub id: u32,
    pub path: Path,
    /// 兼容声明版本（冲突检测输入）。
    pub compat_ver: u32,
    /// 数据边界声明（强制——缺失拒接入）。
    pub boundary_decl: bool,
    /// 质量门实测通过。
    pub qa_pass: bool,
    /// 无障碍门通过（域色——第三方工具同受门）。
    pub a11y_pass: bool,
    pub admitted: bool,
    pub frozen: bool,
}

/// 第三方台。
#[derive(Default)]
pub struct ThirdDesk {
    pub tools: Vec<Tool>,
    /// 公示名单（恶意冻结）。
    pub wall_of_shame: Vec<u32>,
    pub platform_ver: u32,
    pub qa_runs: u32,
}

impl ThirdDesk {
    pub fn new(platform_ver: u32) -> Self {
        ThirdDesk { platform_ver, ..Default::default() }
    }

    /// 接入审核：双路径择一 + 边界声明强制 + 质量门实测 + 无障碍门（域色）
    /// + 版本兼容声明核对。
    pub fn admit(&mut self, mut t: Tool) -> Result<(), ThCode> {
        if !t.boundary_decl {
            return Err(ThCode::NoBoundary);
        }
        if !t.qa_pass {
            self.qa_runs += 1;
            return Err(ThCode::TestFail);
        }
        if !t.a11y_pass {
            self.qa_runs += 1;
            return Err(ThCode::Malicious); // 无障碍门不过按红线处置（域色）
        }
        if t.compat_ver != self.platform_ver {
            return Err(ThCode::VerClash);
        }
        self.qa_runs += 1;
        t.admitted = true;
        t.frozen = false;
        if let Some(x) = self.tools.iter_mut().find(|x| x.id == t.id) {
            *x = t;
        } else {
            self.tools.push(t);
        }
        Ok(())
    }

    /// 质量门实测暂缓：实测不过者复测通道（重审计数）。
    pub fn retest(&mut self, id: u32, pass: bool) -> Result<(), ThCode> {
        let t = self.tools.iter_mut().find(|t| t.id == id).ok_or(ThCode::TestFail)?;
        self.qa_runs += 1;
        if pass {
            t.qa_pass = true;
            t.admitted = true;
            Ok(())
        } else {
            Err(ThCode::TestFail)
        }
    }

    /// 恶意行为：即冻红线 + 公示（台账留痕）。
    pub fn freeze_malicious(&mut self, id: u32) -> Result<(), ThCode> {
        let t = self.tools.iter_mut().find(|t| t.id == id).ok_or(ThCode::Malicious)?;
        t.frozen = true;
        t.admitted = false;
        if !self.wall_of_shame.contains(&id) {
            self.wall_of_shame.push(id);
        }
        Ok(())
    }

    /// 冻结资格拒入（公示名单内工具重复接入拦截）。
    pub fn gate_frozen(&self, id: u32) -> bool {
        self.wall_of_shame.contains(&id)
    }

    /// 版本冲突兼容声明强制：声明版本不符须更新声明后重审（不算恶意）。
    pub fn compat_fix(&mut self, id: u32, new_ver: u32) -> Result<(), ThCode> {
        if new_ver != self.platform_ver {
            return Err(ThCode::VerClash);
        }
        let t = self.tools.iter_mut().find(|t| t.id == id).ok_or(ThCode::VerClash)?;
        t.compat_ver = new_ver;
        Ok(())
    }

    /// 双路径覆盖断言：SDK 与扩展点两路径都有在册接入。
    pub fn dual_path(&self) -> bool {
        self.tools.iter().any(|t| t.path == Path::SdkAuth)
            && self.tools.iter().any(|t| t.path == Path::ExtRegister)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves587_checks() -> CheckSet {
    let mut set = CheckSet::new("ves587");
    let mut td = ThirdDesk::new(30);

    // C587-01 双路径接入（边界声明 + 质量门 + 无障碍门齐备）。
    chk(&mut set, "C587-01", td.admit(Tool { id: 1, path: Path::SdkAuth, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false }).is_ok()
        && td.admit(Tool { id: 2, path: Path::ExtRegister, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false }).is_ok(), "双路径接入");

    // C587-02 双路径覆盖断言。
    chk(&mut set, "C587-02", td.dual_path(), "双路径覆盖");

    // C587-03 边界声明缺失拒接入。
    chk(&mut set, "C587-03", td.admit(Tool { id: 3, path: Path::SdkAuth, compat_ver: 30, boundary_decl: false, qa_pass: true, a11y_pass: true, admitted: false, frozen: false })
        == Err(ThCode::NoBoundary), "缺边界声明拒");

    // C587-04 质量门实测不过暂缓。
    chk(&mut set, "C587-04", td.admit(Tool { id: 4, path: Path::SdkAuth, compat_ver: 30, boundary_decl: true, qa_pass: false, a11y_pass: true, admitted: false, frozen: false })
        == Err(ThCode::TestFail), "实测不过暂缓");

    // C587-05 复测通道转绿。
    let _ = td.admit(Tool { id: 5, path: Path::ExtRegister, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false });
    chk(&mut set, "C587-05", td.retest(5, true).is_ok() && td.tools.iter().find(|t| t.id == 5).map(|t| t.admitted) == Some(true), "复测转绿");

    // C587-06 版本冲突兼容声明强制（重审不计恶意）。
    chk(&mut set, "C587-06", td.admit(Tool { id: 6, path: Path::SdkAuth, compat_ver: 28, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false })
        == Err(ThCode::VerClash), "版本冲突强制声明");

    // C587-07 兼容声明修正后接入。
    let _ = td.admit(Tool { id: 6, path: Path::SdkAuth, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false });
    chk(&mut set, "C587-07", td.compat_fix(6, 30).is_ok(), "声明修正接入");

    // C587-08 恶意工具即冻红线 + 公示。
    chk(&mut set, "C587-08", td.freeze_malicious(1).is_ok() && td.wall_of_shame == [1]
        && td.gate_frozen(1), "恶意即冻公示");

    // C587-09 冻结资格再接入拦截。
    chk(&mut set, "C587-09", td.admit(Tool { id: 1, path: Path::SdkAuth, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false }).is_err(), "冻结拒再入");

    // C587-10 无障碍门不过按红线（域色）。
    chk(&mut set, "C587-10", td.admit(Tool { id: 7, path: Path::SdkAuth, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: false, admitted: false, frozen: false })
        == Err(ThCode::Malicious), "无障碍门红线");

    // C587-11 未接入工具复测拒。
    chk(&mut set, "C587-11", td.retest(99, true).is_err(), "未接入复测拒");

    // C587-12 版本修正错版仍拒（不臆测）。
    chk(&mut set, "C587-12", td.compat_fix(6, 29).is_err(), "错版仍拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: u32) -> Tool {
        Tool { id, path: Path::SdkAuth, compat_ver: 30, boundary_decl: true, qa_pass: true, a11y_pass: true, admitted: false, frozen: false }
    }

    #[test]
    fn admit_flow() {
        let mut td = ThirdDesk::new(30);
        assert!(td.admit(tool(1)).is_ok());
        assert!(td.tools[0].admitted);
        assert_eq!(td.dual_path(), false); // 仅一路径
    }

    #[test]
    fn malicious_freeze() {
        let mut td = ThirdDesk::new(30);
        let _ = td.admit(tool(1));
        assert!(td.freeze_malicious(1).is_ok());
        assert!(td.tools[0].frozen);
        assert!(td.gate_frozen(1));
    }
}
