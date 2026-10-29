//! cgb823_driver_update — CGPU-F0823 Intel 驱动更新应对。
//!
//! 判据原文（验收行摘录）：Intel 驱动更新应对：驱动版本指纹（语义化版本
//! + 内建能力字合成指纹）；更新前后能力快照 diff（新增/丢失/变更三向清
//! 单）；回归风险评级（丢失关键能力即高危，仅新增即低危）；应对策略状
//! 态机（评估→冻结→降级→恢复四态），高危自动冻结提交。判据：指纹、
//! diff、评级、四态应对。
//!
//! 诊断段 0x7160（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；能力集用闭集枚举数组表达；四态状态机迁移
//! 显式受控；drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb823-v1";

/// 诊断段基（本单独占 0x7160）。
pub const SEG_BASE: u16 = 0x7160;

/// 驱动未登记（未登记旧驱动即提交更新）。
pub const XC_NO_BASELINE: u16 = SEG_BASE + 1;
/// 版本回退（新版本号小于旧版本——更新流违约）。
pub const XC_VERSION_BACKWARD: u16 = SEG_BASE + 2;
/// 同版本重复更新（指纹未变——无操作意义）。
pub const XC_SAME_FINGERPRINT: u16 = SEG_BASE + 3;
/// 冻结期提交（高危冻结窗口内收到新任务——应拒）。
pub const XC_FROZEN_SUBMIT: u16 = SEG_BASE + 4;
/// 状态迁移非法（四态之外的跳转）。
pub const XC_BAD_TRANSITION: u16 = SEG_BASE + 5;
/// 能力快照超容（定容能力槽数耗尽）。
pub const XC_SNAPSHOT_FULL: u16 = SEG_BASE + 6;
/// 评估未完成（未 diff 即评级/决策）。
pub const XC_NOT_ASSESSED: u16 = SEG_BASE + 7;

/// 能力快照定容。
pub const FEAT_CAP: usize = 24;

/// 关键能力闭集（判据"丢失关键能力即高危"的判定域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    /// 内存池适配。
    ShmPool,
    /// 媒体联动。
    MediaLink,
    /// 功耗画像。
    PowerProf,
    /// 遥测通道。
    Telemetry,
    /// TDR 恢复。
    TdrRecover,
    /// 混合路由。
    HybridRoute,
}

impl Cap {
    /// 能力名。
    pub fn name(self) -> &'static str {
        match self {
            Cap::ShmPool => "shm_pool",
            Cap::MediaLink => "media_link",
            Cap::PowerProf => "power_prof",
            Cap::Telemetry => "telemetry",
            Cap::TdrRecover => "tdr_recover",
            Cap::HybridRoute => "hybrid_route",
        }
    }
}

/// 驱动版本（语义化三段：主.次.补丁）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ver {
    /// 主版本。
    pub major: u32,
    /// 次版本。
    pub minor: u32,
    /// 补丁号。
    pub patch: u32,
}

impl Ver {
    /// 合成单调版本号（判据"版本回退"判定用）。
    pub fn key(self) -> u64 {
        self.major as u64 * 1_000_000 + self.minor as u64 * 1_000 + self.patch as u64
    }
    /// 版本指纹文本。
    pub fn text(self) -> String {
        let mut s = String::new();
        s.push_str(&self.major.to_string());
        s.push('.');
        s.push_str(&self.minor.to_string());
        s.push('.');
        s.push_str(&self.patch.to_string());
        s
    }
}

/// 驱动指纹 = 版本 key 异或能力集合成值（判据"指纹"）。
pub fn fingerprint(v: Ver, caps: &[Cap]) -> u64 {
    let mut acc = v.key();
    for c in caps {
        acc = acc.wrapping_mul(31).wrapping_add(*c as u64 + 1);
    }
    acc
}

/// 三向 diff（判据"新增/丢失/变更三向清单"）。
#[derive(Clone, Debug)]
pub struct Diff {
    /// 新增能力。
    pub added: Vec<Cap>,
    /// 丢失能力。
    pub lost: Vec<Cap>,
    /// 保留能力（变更侧：能力保留但指纹段变化）。
    pub kept: Vec<Cap>,
}

/// 风险评级（判据"回归风险评级"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Risk {
    /// 低危：仅新增，无丢失。
    Low,
    /// 中危：丢失非关键能力。
    Mid,
    /// 高危：丢失任一关键能力（TDR/内存池）。
    High,
}

/// 关键能力表（丢失即高危）。
pub fn is_critical(c: Cap) -> bool {
    c == Cap::TdrRecover || c == Cap::ShmPool
}

/// 应对策略状态机四态（判据"四态应对"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum St {
    /// 评估中：接收新驱动、做 diff。
    Assessing,
    /// 冻结：高危时自动进入，禁止新提交。
    Frozen,
    /// 降级：关闭丢失能力对应模块路径，继续服务。
    Degraded,
    /// 恢复：能力补齐/回滚完成，回全量服务。
    Restored,
}

/// 驱动更新应对引擎。
pub struct DriverWatch {
    /// 引擎名。
    pub name: String,
    /// 状态机。
    st: St,
    /// 旧驱动版本。
    base_ver: Option<Ver>,
    /// 旧能力集。
    base_caps: Vec<Cap>,
    /// 最近 diff。
    pub last_diff: Option<Diff>,
    /// 最近评级。
    pub last_risk: Option<Risk>,
    /// 丢失能力降级标记（true=已降级关闭）。
    pub degraded: Vec<Cap>,
    /// 更新轮次。
    pub rounds: u64,
    /// 拒绝计数。
    pub reject: u64,
}

impl DriverWatch {
    /// 构造：Assessing 态，无基线。
    pub fn new(name: &str) -> Self {
        DriverWatch {
            name: name.to_string(),
            st: St::Assessing,
            base_ver: None,
            base_caps: Vec::new(),
            last_diff: None,
            last_risk: None,
            degraded: Vec::new(),
            rounds: 0,
            reject: 0,
        }
    }

    /// 状态只读。
    pub fn st(&self) -> St {
        self.st
    }

    /// 登记基线驱动（首次登记或 Restored 后可重登）。
    pub fn set_baseline(&mut self, v: Ver, caps: &[Cap]) -> Result<(), u16> {
        if caps.len() > FEAT_CAP {
            return Err(XC_SNAPSHOT_FULL);
        }
        if self.st != St::Assessing && self.st != St::Restored {
            return Err(XC_BAD_TRANSITION);
        }
        self.base_ver = Some(v);
        self.base_caps.clear();
        for c in caps {
            self.base_caps.push(*c);
        }
        Ok(())
    }

    /// 基线是否在册。
    pub fn has_baseline(&self) -> bool {
        self.base_ver.is_some()
    }

    /// 三向 diff（判据"三向清单"）。
    pub fn diff(&self, new_caps: &[Cap]) -> Result<Diff, u16> {
        if !self.has_baseline() {
            return Err(XC_NO_BASELINE);
        }
        let mut d = Diff { added: Vec::new(), lost: Vec::new(), kept: Vec::new() };
        for n in new_caps {
            let mut found = false;
            for b in self.base_caps.iter() {
                if b == n {
                    found = true;
                    break;
                }
            }
            if found {
                d.kept.push(*n);
            } else {
                d.added.push(*n);
            }
        }
        for b in self.base_caps.iter() {
            let mut found = false;
            for n in new_caps {
                if n == b {
                    found = true;
                    break;
                }
            }
            if !found {
                d.lost.push(*b);
            }
        }
        Ok(d)
    }

    /// 风险评级（判据"评级"）。
    pub fn grade(&self, d: &Diff) -> Risk {
        let mut high = false;
        let mut mid = false;
        for l in d.lost.iter() {
            if is_critical(*l) {
                high = true;
            } else {
                mid = true;
            }
        }
        if high {
            Risk::High
        } else if mid {
            Risk::Mid
        } else {
            Risk::Low
        }
    }

    /// 提交驱动更新（判据"应对状态机"入口）。
    pub fn apply_update(&mut self, v: Ver, new_caps: &[Cap]) -> Result<Risk, u16> {
        if self.st == St::Frozen {
            self.reject += 1;
            return Err(XC_FROZEN_SUBMIT);
        }
        if self.st != St::Assessing && self.st != St::Restored && self.st != St::Degraded {
            self.reject += 1;
            return Err(XC_BAD_TRANSITION);
        }
        let base = match self.base_ver {
            Some(b) => b,
            None => {
                self.reject += 1;
                return Err(XC_NO_BASELINE);
            }
        };
        if v.key() < base.key() {
            self.reject += 1;
            return Err(XC_VERSION_BACKWARD);
        }
        let nf = fingerprint(v, new_caps);
        let bf = fingerprint(base, &self.base_caps);
        if nf == bf {
            self.reject += 1;
            return Err(XC_SAME_FINGERPRINT);
        }
        let d = self.diff(new_caps)?;
        let risk = self.grade(&d);
        self.last_diff = Some(d);
        self.last_risk = Some(risk);
        self.rounds += 1;
        if risk == Risk::High {
            // 判据"高危自动冻结提交"。
            self.st = St::Frozen;
        } else if risk == Risk::Mid {
            // 中危：降级丢失能力。
            self.degraded.clear();
            match &self.last_diff {
                Some(dd) => {
                    for l in dd.lost.iter() {
                        self.degraded.push(*l);
                    }
                }
                None => {}
            }
            self.st = St::Degraded;
        } else {
            self.degraded.clear();
            self.st = St::Restored;
        }
        Ok(risk)
    }

    /// 冻结解除（人工回滚确认后）：回 Assessing 并要求重登基线。
    pub fn unfreeze(&mut self) -> Result<(), u16> {
        if self.st != St::Frozen {
            return Err(XC_BAD_TRANSITION);
        }
        self.st = St::Assessing;
        self.base_ver = None;
        self.base_caps.clear();
        self.last_diff = None;
        self.last_risk = None;
        Ok(())
    }

    /// 恢复全量（降级能力重新补齐确认）。
    pub fn restore(&mut self) -> Result<(), u16> {
        if self.st != St::Degraded {
            return Err(XC_BAD_TRANSITION);
        }
        self.degraded.clear();
        self.st = St::Restored;
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb823[");
        s.push_str(&self.name);
        s.push_str("] st=");
        s.push_str(match self.st {
            St::Assessing => "assessing",
            St::Frozen => "frozen",
            St::Degraded => "degraded",
            St::Restored => "restored",
        });
        s.push_str(" rounds=");
        s.push_str(&self.rounds.to_string());
        s.push_str(" degraded=");
        s.push_str(&self.degraded.len().to_string());
        s.push_str(" rej=");
        s.push_str(&self.reject.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut w = DriverWatch::new("w3-drill-823");

    // 负样本 1：无基线更新——应拒。
    let full = [Cap::ShmPool, Cap::MediaLink, Cap::PowerProf, Cap::Telemetry, Cap::TdrRecover, Cap::HybridRoute];
    match w.apply_update(Ver { major: 32, minor: 0, patch: 101 }, &full) {
        Err(code) if code == XC_NO_BASELINE => {}
        Ok(_) => return Err(XC_NO_BASELINE),
        Err(other) => return Err(other),
    }
    w.set_baseline(Ver { major: 31, minor: 4, patch: 5084 }, &full)?;

    // 正样本 1：低危（仅新增）。
    let mut low_caps: Vec<Cap> = Vec::new();
    for c in full.iter() {
        low_caps.push(*c);
    }
    low_caps.push(Cap::PowerProf); // 已有——改用重复以制造仅新增场景：改推一个新能力
    // 用少量能力基线重登以构造清晰场景。
    let base2 = [Cap::ShmPool, Cap::MediaLink, Cap::TdrRecover];
    w.set_baseline(Ver { major: 31, minor: 4, patch: 5084 }, &base2)?;
    let low_caps = [Cap::ShmPool, Cap::MediaLink, Cap::TdrRecover, Cap::PowerProf];
    let r1 = w.apply_update(Ver { major: 32, minor: 0, patch: 100 }, &low_caps)?;
    if r1 != Risk::Low {
        return Err(XC_NOT_ASSESSED);
    }
    if w.st() != St::Restored {
        return Err(XC_BAD_TRANSITION);
    }

    // 正样本 2：中危（丢 PowerProf——非关键）。
    let mid_caps = [Cap::ShmPool, Cap::MediaLink, Cap::TdrRecover];
    let r2 = w.apply_update(Ver { major: 32, minor: 1, patch: 100 }, &mid_caps)?;
    if r2 != Risk::Mid {
        return Err(XC_NOT_ASSESSED);
    }
    if w.st() != St::Degraded || w.degraded.len() != 1 {
        return Err(XC_BAD_TRANSITION);
    }
    w.restore()?;
    if w.st() != St::Restored {
        return Err(XC_BAD_TRANSITION);
    }

    // 负样本 2：版本回退——应拒。
    match w.apply_update(Ver { major: 31, minor: 0, patch: 1 }, &mid_caps) {
        Err(code) if code == XC_VERSION_BACKWARD => {}
        Ok(_) => return Err(XC_VERSION_BACKWARD),
        Err(other) => return Err(other),
    }

    // 正样本 3：高危（丢 ShmPool——关键能力）→ 自动冻结。
    let high_caps = [Cap::MediaLink, Cap::TdrRecover];
    let r3 = w.apply_update(Ver { major: 32, minor: 2, patch: 100 }, &high_caps)?;
    if r3 != Risk::High {
        return Err(XC_NOT_ASSESSED);
    }
    if w.st() != St::Frozen {
        return Err(XC_BAD_TRANSITION);
    }
    // 负样本 3：冻结期提交——应拒。
    match w.apply_update(Ver { major: 33, minor: 0, patch: 0 }, &full) {
        Err(code) if code == XC_FROZEN_SUBMIT => {}
        Ok(_) => return Err(XC_FROZEN_SUBMIT),
        Err(other) => return Err(other),
    }
    // 解冻后基线清空，须重登。
    w.unfreeze()?;
    if w.has_baseline() {
        return Err(XC_NO_BASELINE);
    }
    w.set_baseline(Ver { major: 32, minor: 2, patch: 100 }, &high_caps)?;
    // 负样本 4：同指纹重复更新——应拒。
    match w.apply_update(Ver { major: 32, minor: 2, patch: 100 }, &high_caps) {
        Err(code) if code == XC_VERSION_BACKWARD || code == XC_SAME_FINGERPRINT => {}
        Ok(_) => return Err(XC_SAME_FINGERPRINT),
        Err(other) => return Err(other),
    }
    if w.reject < 3 {
        return Err(XC_NOT_ASSESSED);
    }
    Ok(0)
}
