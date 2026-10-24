//! VE-F4252 · 验证安全（U03 一致性验证平台）
//! 职责：平台攻击面三审（报告注入/基准投毒/闸门绕过企图）+ 审计留痕三道
//! + 投毒溯源链 + 绕过无障碍闸企图按最高缺陷处理（域色红线实测）。
//! 段位：0x1D60（续表 idx486）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1D60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 报告注入检出（恶意发现项文本）。
    InjectHit = SEG_BASE,
    /// 基准投毒（伪造基准）。
    PoisonHit = SEG_BASE + 0x01,
    /// 闸门绕过企图（最高级告警）。
    BypassHit = SEG_BASE + 0x02,
    /// 审计链断。
    AuditBroken = SEG_BASE + 0x03,
}

/// 攻击面类别。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtkFace {
    ReportInject = 0,
    BaselinePoison = 1,
    GateBypass = 2,
}
pub const ATK_N: usize = 3;

/// 注入特征：发现项文本黑名单。
pub const INJECT_MARKS: [&str; 4] = ["</script>", "${}", "\x1b[", "union select"];

/// 审计事件（三道留痕之一）。
#[derive(Clone, Debug)]
pub struct AuditEv {
    pub face: AtkFace,
    pub actor: u8,
    pub verdict_blocked: bool,
    pub trace: u64, // 溯源链指纹
}

impl AuditEv {
    pub fn chain_hash(&self, prev: u64) -> u64 {
        let mut h = prev ^ (self.face as u64).wrapping_mul(0x9E37_79B9);
        h ^= self.actor as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
        h ^ self.trace
    }
}

/// 安全册：三审 + 审计链。
pub struct SecBook {
    pub events: Vec<AuditEv>,
    pub chain: u64,
    /// 最高告警计数（绕过企图）。
    pub max_alerts: u32,
    /// 双签校验表（基准指纹→已签）。
    pub baseline_fps: Vec<(u64, bool)>,
}

impl SecBook {
    pub fn new() -> Self {
        SecBook { events: Vec::new(), chain: 0x5EED_CAFE, max_alerts: 0, baseline_fps: Vec::new() }
    }

    /// 一审：报告注入检出。
    pub fn scan_inject(&mut self, text: &str) -> bool {
        let hit = INJECT_MARKS.iter().any(|m| text.contains(m));
        if hit {
            self.record(AtkFace::ReportInject, 9, true);
        }
        hit
    }

    /// 二审：基准投毒——指纹须在双签表且已签，否则拦截+溯源。
    pub fn scan_poison(&mut self, fp: u64) -> bool {
        let signed = self.baseline_fps.iter().any(|(f, s)| *f == fp && *s);
        if !signed {
            self.record(AtkFace::BaselinePoison, 9, true);
        }
        !signed
    }

    /// 基准指纹登记（双签后入表）。
    pub fn register_baseline(&mut self, fp: u64, signed: bool) {
        self.baseline_fps.push((fp, signed));
    }

    /// 三审：闸门绕过企图——最高级告警（域色红线：绕无障碍闸按最高缺陷）。
    pub fn scan_bypass(&mut self, target: &str) -> bool {
        let is_a11y = target.contains("a11y");
        self.record(AtkFace::GateBypass, 9, true);
        self.max_alerts += 1;
        is_a11y
    }

    /// 审计留痕（链式哈希，断链可查）。
    pub fn record(&mut self, face: AtkFace, actor: u8, blocked: bool) -> u64 {
        let ev = AuditEv { face, actor, verdict_blocked: blocked, trace: self.events.len() as u64 + 1 };
        self.chain = ev.chain_hash(self.chain);
        self.events.push(ev);
        self.chain
    }

    /// 审计链完整性：逐事件重放校验。
    pub fn audit_intact(&self) -> bool {
        let mut h = 0x5EED_CAFE;
        for ev in &self.events {
            h = ev.chain_hash(h);
        }
        h == self.chain
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C486-01..12。
pub fn run_ves486_checks() -> CheckSet {
    let mut set = CheckSet::new("ves486");

    let mut sb = SecBook::new();
    chk(&mut set, "C486-01", sb.scan_inject("normal finding text") == false, "正常文本放行");
    chk(&mut set, "C486-02", sb.scan_inject("evil </script> payload"), "注入检出并留痕");
    chk(&mut set, "C486-03", sb.events.len() == 1 && sb.events[0].verdict_blocked, "注入审计阻断留痕");

    sb.register_baseline(0xAAAA, true);
    sb.register_baseline(0xBBBB, false);
    chk(&mut set, "C486-04", sb.scan_poison(0xAAAA) == false, "已双签基准放行");
    chk(&mut set, "C486-05", sb.scan_poison(0xBBBB), "未签基准=投毒拦截");
    chk(&mut set, "C486-06", sb.scan_poison(0xCCCC), "未知指纹=投毒拦截+溯源");

    let a11y_bypass = sb.scan_bypass("gate:a11y");
    chk(&mut set, "C486-07", a11y_bypass, "无障碍闸绕过企图识别");
    chk(&mut set, "C486-08", sb.max_alerts == 3, "三次企图三次最高级告警");

    let _ = sb.scan_inject("second ${} injection");
    chk(&mut set, "C486-09", sb.audit_intact(), "审计链重放一致（三道留痕）");
    chk(&mut set, "C486-10", sb.events.iter().all(|e| e.verdict_blocked), "全部攻击面事件阻断处置");

    // 溯源：按 face 过滤链
    let injects = sb.events.iter().filter(|e| e.face == AtkFace::ReportInject).count();
    chk(&mut set, "C486-11", injects == 2, "溯源链按面过滤精确");
    chk(&mut set, "C486-12", sb.chain != 0 && sb.events.last().unwrap().trace == 4, "链尾序号连续");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_three_faces() {
        let mut sb = SecBook::new();
        assert!(!sb.scan_inject("ok"));
        assert!(sb.scan_inject("x \x1b[31m"));
        sb.register_baseline(7, true);
        assert!(!sb.scan_poison(7));
        assert!(sb.scan_poison(8));
        assert!(sb.audit_intact());
    }
}
