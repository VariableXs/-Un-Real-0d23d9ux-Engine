//! VE-F0128 · 崩溃恢复后会话连续性（VE-A 域 · A06 环境与系统集成组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0128`
//!
//! **职责定位（锚点原文）**：渲染崩溃恢复后的会话连续（恢复后场景状态/用户进度/
//! 窗口布局三保留），连续性的分级承诺（什么保证恢复什么只保证不崩）；含连续性的
//! 分级实测。
//!
//! **铁律**：
//! 1. **三保留**——场景/进度/布局。
//! 2. **分级承诺**——什么保证恢复什么只保证不崩。
//! 3. **恢复对拍**——承诺与实测一致。
//! 4. **进度红线**——进度丢失阻断级。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C6（会话连续性）。
pub const SEG_BASE: u16 = 0xE8C6;

/// 进度丢失（红线）。
pub const XC_PROGRESS_LOST: u16 = SEG_BASE | 0x01;
/// 状态失真。
pub const XC_STATE_SKEW: u16 = SEG_BASE | 0x02;
/// 分级虚标。
pub const XC_PROMISE_LIE: u16 = SEG_BASE | 0x03;
/// 恢复失败。
pub const XC_RECOVER_FAIL: u16 = SEG_BASE | 0x04;

/// 分级承诺。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseLevel {
    /// 保证恢复。
    Guaranteed,
    /// 只保证不崩。
    SurviveOnly,
}

/// 三保留单元。
#[derive(Clone, Copy, Debug)]
pub struct PreservedItem {
    /// 0=场景 1=进度 2=布局。
    pub kind: u8,
    /// 指纹。
    pub fingerprint: u32,
    pub promise: PromiseLevel,
}

/// 会话连续性管理器。
pub struct SessionContinuity {
    items: Vec<PreservedItem>,
    last_code: u16,
    /// 恢复次数。
    recoveries: u32,
    /// 对拍次数。
    checks: u32,
    /// 进度红线触发。
    blockers: u32,
}

impl SessionContinuity {
    pub fn new() -> Self {
        SessionContinuity {
            items: Vec::new(),
            last_code: 0,
            recoveries: 0,
            checks: 0,
            blockers: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 保存三保留。
    pub fn save(&mut self, kind: u8, fingerprint: u32, promise: PromiseLevel) -> Result<(), u16> {
        if kind > 2 {
            self.last_code = XC_STATE_SKEW;
            return Err(XC_STATE_SKEW);
        }
        self.items.retain(|i| i.kind != kind);
        self.items.push(PreservedItem {
            kind,
            fingerprint,
            promise,
        });
        self.last_code = 0;
        Ok(())
    }

    /// 恢复（分级执行）。
    pub fn restore(&mut self) -> Result<Vec<u8>, u16> {
        let mut restored = Vec::new();
        for item in &self.items {
            match item.promise {
                PromiseLevel::Guaranteed => {
                    // 保证恢复：必须还原。
                    restored.push(item.kind);
                }
                PromiseLevel::SurviveOnly => {
                    // 只保证不崩：可不还原但不崩。
                    restored.push(item.kind);
                }
            }
        }
        self.recoveries += 1;
        // 进度（kind=1）必须在 Guaranteed 里。
        let progress = self.items.iter().find(|i| i.kind == 1);
        match progress {
            Some(p) if p.promise == PromiseLevel::Guaranteed => {}
            Some(_) => {
                self.blockers += 1;
                self.last_code = XC_PROGRESS_LOST;
                return Err(XC_PROGRESS_LOST);
            }
            None => {
                self.blockers += 1;
                self.last_code = XC_PROGRESS_LOST;
                return Err(XC_PROGRESS_LOST);
            }
        }
        self.last_code = 0;
        Ok(restored)
    }

    /// 恢复对拍（承诺与实测一致）。
    pub fn verify_promise(&mut self, kind: u8, measured_fp: u32) -> Result<(), u16> {
        self.checks += 1;
        match self.items.iter().find(|i| i.kind == kind) {
            Some(item) => {
                if item.promise == PromiseLevel::Guaranteed && item.fingerprint != measured_fp {
                    self.last_code = XC_STATE_SKEW;
                    return Err(XC_STATE_SKEW);
                }
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_PROMISE_LIE;
                Err(XC_PROMISE_LIE)
            }
        }
    }

    /// 分级虚标检测。
    pub fn verify_grades(&mut self) -> Result<(), u16> {
        // 进度必须 Guaranteed。
        for item in &self.items {
            if item.kind == 1 && item.promise != PromiseLevel::Guaranteed {
                self.last_code = XC_PROMISE_LIE;
                return Err(XC_PROMISE_LIE);
            }
        }
        self.last_code = 0;
        Ok(())
    }

    pub fn recoveries(&self) -> u32 {
        self.recoveries
    }

    pub fn checks(&self) -> u32 {
        self.checks
    }

    pub fn blockers(&self) -> u32 {
        self.blockers
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

impl Default for SessionContinuity {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：三保留、分级承诺、恢复对拍、进度红线。
pub fn run_vea128_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn full() -> SessionContinuity {
        let mut s = SessionContinuity::new();
        let _ = s.save(0, 0xAA, PromiseLevel::Guaranteed); // 场景
        let _ = s.save(1, 0xBB, PromiseLevel::Guaranteed); // 进度
        let _ = s.save(2, 0xCC, PromiseLevel::SurviveOnly); // 布局
        s
    }

    let mut set = CheckSet::new("vea128_session");

    // C128-01 三保留
    {
        let s = full();
        chk(&mut set, "C128-01 三保留", s.item_count() == 3, "保留缺");
    }

    // C128-02 分级承诺
    {
        let mut s = full();
        chk(
            &mut set,
            "C128-02 分级",
            s.verify_grades().is_ok(),
            "分级虚标",
        );
    }

    // C128-03 恢复成功
    {
        let mut s = full();
        let r = s.restore();
        chk(&mut set, "C128-03 恢复", r.is_ok() && r.unwrap().len() == 3, "恢复失败");
    }

    // C128-04 进度红线
    {
        let mut s = SessionContinuity::new();
        let _ = s.save(1, 0xBB, PromiseLevel::SurviveOnly); // 进度只保不崩 = 违例
        let r = s.restore();
        chk(
            &mut set,
            "C128-04 进度红线",
            r.is_err() && s.blockers() == 1,
            "红线被破",
        );
    }

    // C128-05 无进度阻断
    {
        let mut s = SessionContinuity::new();
        let _ = s.save(0, 0xAA, PromiseLevel::Guaranteed);
        let r = s.restore();
        chk(
            &mut set,
            "C128-05 无进度阻断",
            r.is_err() && s.last_code() == XC_PROGRESS_LOST,
            "无进度被放",
        );
    }

    // C128-06 对拍一致
    {
        let mut s = full();
        let r = s.verify_promise(0, 0xAA);
        chk(&mut set, "C128-06 对拍", r.is_ok() && s.checks() == 1, "对拍失败");
    }

    // C128-07 对拍失真检出
    {
        let mut s = full();
        let r = s.verify_promise(0, 0x99); // 指纹不符
        chk(
            &mut set,
            "C128-07 失真检出",
            r.is_err() && s.last_code() == XC_STATE_SKEW,
            "失真漏检",
        );
    }

    // C128-08 分级虚标检出
    {
        let mut s = SessionContinuity::new();
        let _ = s.save(1, 0xBB, PromiseLevel::SurviveOnly);
        let r = s.verify_grades();
        chk(
            &mut set,
            "C128-08 虚标检出",
            r.is_err() && s.last_code() == XC_PROMISE_LIE,
            "虚标漏检",
        );
    }

    // C128-09 SurviveOnly 不对拍
    {
        let mut s = full();
        let r = s.verify_promise(2, 0x99); // 布局 SurviveOnly 不校验指纹
        chk(&mut set, "C128-09 不对拍", r.is_ok(), "过度对拍");
    }

    // C128-10 覆盖保存
    {
        let mut s = full();
        let _ = s.save(0, 0xDD, PromiseLevel::Guaranteed);
        chk(&mut set, "C128-10 覆盖", s.item_count() == 3, "未覆盖");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea128_all_green() {
        let s = run_vea128_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
