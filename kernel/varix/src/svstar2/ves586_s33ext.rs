//! VE-F4352 · 一致性扩展点汇总（U08 测试与资产组）
//! 职责：一致性扩展点汇总——扩展点四类（自定义规则 / 自定义审查器 / 自
//! 定义场景触达 / 自定义验证器）+ 扩展点契约（每点含冻结接口 + 示例 + 沙
//! 箱三件）+ 破坏性变更零容忍（走双版期——十四章开放性口径落地）+ 越过
//! 接口拒绝加载 + 扩展崩溃沙箱隔离 + 接口破坏版本拦截 + 扩展产物同受无
//! 障碍门禁（域本色）。
//! 段位：0x4340（续表 idx586）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4340;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    /// 越过接口（拒绝加载）。
    BypassInterface = SEG_BASE,
    /// 扩展崩溃（沙箱隔离）。
    Crashed = SEG_BASE + 0x01,
    /// 接口破坏（版本拦截——双版期）。
    InterfaceBroken = SEG_BASE + 0x02,
    /// 契约三件缺（示例/沙箱缺一拒载）。
    ContractGap = SEG_BASE + 0x03,
}

/// 四类扩展点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtKind {
    CustomRule,
    CustomLinter,
    CustomTouch,
    CustomVerifier,
}

/// 扩展点契约卡。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtCard {
    pub kind: ExtKind,
    pub id: u32,
    /// 冻结接口版本（扩展编译目标版本）。
    pub iface_ver: u32,
    /// 平台当前接口版本。
    pub platform_ver: u32,
    /// 契约三件：示例 / 沙箱。
    pub has_example: bool,
    pub has_sandbox: bool,
    /// 沙箱隔离态。
    pub quarantined: bool,
    /// 无障碍门禁通过（域色——扩展产物同受门禁）。
    pub a11y_pass: bool,
    pub loaded: bool,
}

/// 双版期宽限（破坏性变更后旧接口允许并存版本数）。
pub const DUAL_VERSION_SPAN: u32 = 2;

/// 扩展台。
#[derive(Default)]
pub struct ExtDesk {
    pub cards: Vec<ExtCard>,
    pub blocks: u32,
    pub quarantines: u32,
}

impl ExtDesk {
    pub fn new() -> Self {
        ExtDesk { ..Default::default() }
    }

    /// 扩展加载：契约三件齐 + 接口版本在双版期内 + 无障碍门禁过才载入。
    pub fn load(&mut self, c: ExtCard) -> Result<(), ExCode> {
        if !c.has_example || !c.has_sandbox {
            return Err(ExCode::ContractGap);
        }
        if !c.a11y_pass {
            self.blocks += 1;
            return Err(ExCode::InterfaceBroken); // 无障碍门禁不过 = 域色拦截
        }
        if c.iface_ver + DUAL_VERSION_SPAN < c.platform_ver {
            self.blocks += 1;
            return Err(ExCode::InterfaceBroken);
        }
        let mut x = c;
        x.loaded = true;
        if let Some(old) = self.cards.iter_mut().find(|x| x.id == c.id) {
            *old = x;
        } else {
            self.cards.push(x);
        }
        Ok(())
    }

    /// 越过接口检测：扩展声明的接口版本高于平台 = 伪造（拒绝加载）。
    pub fn bypass_check(&self, c: &ExtCard) -> Result<(), ExCode> {
        if c.iface_ver > c.platform_ver {
            Err(ExCode::BypassInterface)
        } else {
            Ok(())
        }
    }

    /// 扩展崩溃沙箱隔离（本体不拖垮——隔离计数）。
    pub fn crash(&mut self, id: u32) -> Result<(), ExCode> {
        let c = self.cards.iter_mut().find(|c| c.id == id).ok_or(ExCode::Crashed)?;
        c.quarantined = true;
        c.loaded = false;
        self.quarantines += 1;
        Ok(())
    }

    /// 四类扩展点齐备断言（载入且未隔离口径）。
    pub fn all_kinds(&self) -> bool {
        (0..4).all(|i| {
            self.cards
                .iter()
                .any(|c| c.loaded && !c.quarantined && c.kind as usize == i)
        })
    }

    /// 双版期边界：恰差 1 版可载 / 差 2 版可载 / 差 3 版拒。
    pub fn dual_span_ok(&self, iface: u32, platform: u32) -> bool {
        iface + DUAL_VERSION_SPAN >= platform && iface <= platform
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves586_checks() -> CheckSet {
    let mut set = CheckSet::new("ves586");
    let mut ed = ExtDesk::new();

    // C586-01 四类扩展点契约齐载入。
    let kinds = [ExtKind::CustomRule, ExtKind::CustomLinter, ExtKind::CustomTouch, ExtKind::CustomVerifier];
    for (i, k) in kinds.iter().enumerate() {
        let c = ExtCard {
            kind: *k, id: i as u32 + 1, iface_ver: 7, platform_ver: 7,
            has_example: true, has_sandbox: true, quarantined: false,
            a11y_pass: true, loaded: false,
        };
        let _ = ed.load(c);
    }
    chk(&mut set, "C586-01", ed.all_kinds() && ed.cards.len() == 4, "四类齐载入");

    // C586-02 契约三件缺一拒载。
    chk(&mut set, "C586-02", ed.load(ExtCard { kind: ExtKind::CustomRule, id: 9, iface_ver: 7, platform_ver: 7, has_example: false, has_sandbox: true, quarantined: false, a11y_pass: true, loaded: false })
        == Err(ExCode::ContractGap), "缺示例拒载");

    // C586-03 越过接口拒绝加载（接口版本伪造高于平台）。
    let fake = ExtCard { kind: ExtKind::CustomVerifier, id: 8, iface_ver: 99, platform_ver: 7, has_example: true, has_sandbox: true, quarantined: false, a11y_pass: true, loaded: false };
    chk(&mut set, "C586-03", ed.bypass_check(&fake) == Err(ExCode::BypassInterface), "越接口拒载");

    // C586-04 双版期边界（差 1/2 版可载·差 3 版拒）。
    chk(&mut set, "C586-04", ed.dual_span_ok(6, 7) && ed.dual_span_ok(5, 7) && !ed.dual_span_ok(4, 7), "双版期边界");

    // C586-05 接口破坏版本拦截（差 3 版拒载计数）。
    chk(&mut set, "C586-05", ed.load(ExtCard { kind: ExtKind::CustomRule, id: 10, iface_ver: 4, platform_ver: 7, has_example: true, has_sandbox: true, quarantined: false, a11y_pass: true, loaded: false })
        == Err(ExCode::InterfaceBroken) && ed.blocks == 1, "破坏版本拦截");

    // C586-06 无障碍门禁拦截（域色——扩展产物同受门禁）。
    chk(&mut set, "C586-06", ed.load(ExtCard { kind: ExtKind::CustomTouch, id: 11, iface_ver: 7, platform_ver: 7, has_example: true, has_sandbox: true, quarantined: false, a11y_pass: false, loaded: false })
        == Err(ExCode::InterfaceBroken) && ed.blocks == 2, "无障碍门禁拦");

    // C586-07 扩展崩溃沙箱隔离（本体不拖垮）。
    chk(&mut set, "C586-07", ed.crash(2).is_ok() && ed.cards[1].quarantined
        && !ed.cards[1].loaded && ed.quarantines == 1, "崩溃沙箱隔离");

    // C586-08 隔离后四类缺一。
    chk(&mut set, "C586-08", !ed.all_kinds(), "隔离后缺类");

    // C586-09 恢复重载（隔离解除）。
    let _ = ed.load(ExtCard { kind: ExtKind::CustomLinter, id: 2, iface_ver: 7, platform_ver: 7, has_example: true, has_sandbox: true, quarantined: false, a11y_pass: true, loaded: false });
    chk(&mut set, "C586-09", ed.all_kinds(), "重载复活");

    // C586-10 同 id 覆盖加载（幂等）。
    let n0 = ed.cards.len();
    let _ = ed.load(ExtCard { kind: ExtKind::CustomRule, id: 1, iface_ver: 7, platform_ver: 7, has_example: true, has_sandbox: true, quarantined: false, a11y_pass: true, loaded: false });
    chk(&mut set, "C586-10", ed.cards.len() == n0, "同 id 幂等");

    // C586-11 未知扩展崩溃拒绝。
    chk(&mut set, "C586-11", ed.crash(99).is_err(), "未知崩溃拒");

    // C586-12 空台四类恒假（不臆测）。
    let ed2 = ExtDesk::new();
    chk(&mut set, "C586-12", !ed2.all_kinds(), "空台恒假");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: u32, iface: u32, platform: u32) -> ExtCard {
        ExtCard { kind: ExtKind::CustomRule, id, iface_ver: iface, platform_ver: platform, has_example: true, has_sandbox: true, quarantined: false, a11y_pass: true, loaded: false }
    }

    #[test]
    fn dual_version_span() {
        let mut ed = ExtDesk::new();
        assert!(ed.load(card(1, 5, 7)).is_ok());  // 差 2 = 双版期内
        assert_eq!(ed.load(card(2, 4, 7)), Err(ExCode::InterfaceBroken)); // 差 3
    }

    #[test]
    fn sandbox_crash_isolate() {
        let mut ed = ExtDesk::new();
        let _ = ed.load(card(1, 7, 7));
        assert!(ed.crash(1).is_ok());
        assert!(ed.cards[0].quarantined);
        assert!(!ed.all_kinds());
    }
}
