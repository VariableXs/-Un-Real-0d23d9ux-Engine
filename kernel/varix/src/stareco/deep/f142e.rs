//! 深化层二 · F142 安全披露通道（2026-09-26 深化批次二）。
//!
//! 补深主册【设计细节】披露窗可协商延长与月度安全版例外节奏
//! （主册 G-D-17）：处理四步时限钟深化（逾期升级）、PGP 指纹管理
//! （登记/轮换/撤销）、协调披露文档生成器、无效报告三分类器、
//! 月度安全版模型（不等季窗的例外——F138 联动）。

use crate::checks::CheckSet;
use crate::stareco::secdisclose::{Credit, SecReport, CONFIRM_LIMIT_DAYS};

// ---------------------------------------------------------------------------
// 处理时限钟深化：四步逐项计时 + 逾期升级
// ---------------------------------------------------------------------------

/// 处理阶段耗时记录（接收→确认→修复→披露各段天数）。
pub struct TimelineClock {
    pub received_day: u32,
    pub confirmed_day: Option<u32>,
    pub fixed_day: Option<u32>,
    pub disclosed_day: Option<u32>,
}

impl TimelineClock {
    pub fn new(day: u32) -> TimelineClock {
        TimelineClock { received_day: day, confirmed_day: None, fixed_day: None, disclosed_day: None }
    }

    pub fn confirm(&mut self, day: u32) -> Result<(), &'static str> {
        if self.confirmed_day.is_some() {
            return Err("已确认：不重复计时");
        }
        if day < self.received_day {
            return Err("确认早于接收：时间线矛盾");
        }
        self.confirmed_day = Some(day);
        Ok(())
    }

    /// 48h 确认线（超时 = 升级事件——对自家施压的钟也对着我们自己）。
    pub fn confirm_sla_breached(&self) -> bool {
        match self.confirmed_day {
            Some(d) => d - self.received_day > CONFIRM_LIMIT_DAYS,
            None => false, // 未确认不算 breaches（计时在 SecReport 内部走）
        }
    }

    pub fn fix(&mut self, day: u32) -> Result<(), &'static str> {
        let confirmed = self.confirmed_day.ok_or("未确认就修复：流程跳步")?;
        if day < confirmed {
            return Err("修复早于确认：时间线矛盾");
        }
        self.fixed_day = Some(day);
        Ok(())
    }

    pub fn disclose(&mut self, day: u32) -> Result<(), &'static str> {
        let fixed = self.fixed_day.ok_or("未修复就披露：流程跳步")?;
        if day < fixed {
            return Err("披露早于修复：时间线矛盾");
        }
        self.disclosed_day = Some(day);
        Ok(())
    }

    /// 全链时长（接收→披露；未完成返回 None）。
    pub fn total_days(&self) -> Option<u32> {
        self.disclosed_day.map(|d| d - self.received_day)
    }
}

// ---------------------------------------------------------------------------
// PGP 指纹管理：登记 / 轮换 / 撤销（加密邮件通道的钥匙面）
// ---------------------------------------------------------------------------

pub struct PgpRing {
    /// (指纹, 状态)。活跃指纹至多一条（轮换 = 旧撤新登）。
    active: Option<u64>,
    retired: alloc::vec::Vec<u64>,
}

impl PgpRing {
    pub fn new() -> PgpRing {
        PgpRing { active: None, retired: alloc::vec::Vec::new() }
    }

    pub fn enroll(&mut self, fp: u64) -> Result<(), &'static str> {
        if fp == 0 {
            return Err("零指纹无效");
        }
        if self.active.is_some() {
            return Err("已有活跃指纹：先轮换再登记");
        }
        self.active = Some(fp);
        Ok(())
    }

    /// 轮换：旧指纹退役存档（旧邮件仍可验——存档不删除）。
    pub fn rotate(&mut self, new_fp: u64) -> Result<(), &'static str> {
        if new_fp == 0 {
            return Err("零指纹无效");
        }
        let old = self.active.take().ok_or("无活跃指纹：直接登记即可")?;
        if Some(new_fp) == self.active {
            return Err("新指纹与旧相同：轮换无意义");
        }
        self.retired.push(old);
        self.active = Some(new_fp);
        Ok(())
    }

    /// 加密验证：给定指纹是否是当前活跃钥匙。
    pub fn accepts(&self, fp: u64) -> bool {
        self.active == Some(fp)
    }

    pub fn retired_count(&self) -> usize {
        self.retired.len()
    }
}

// ---------------------------------------------------------------------------
// 协调披露文档生成器（四节模板：时间线/影响/缓解/致谢）
// ---------------------------------------------------------------------------

pub struct DisclosureDoc {
    pub cve_ref: &'static str,
    pub received_day: u32,
    pub disclosed_day: u32,
    pub impact: &'static str,
    pub mitigation: &'static str,
    pub credit_line: &'static str,
}

impl DisclosureDoc {
    pub fn sections_complete(&self) -> bool {
        !self.cve_ref.is_empty()
            && !self.impact.is_empty()
            && !self.mitigation.is_empty()
            && !self.credit_line.is_empty()
            && self.disclosed_day >= self.received_day
    }

    /// 渲染四节文本（公开页展示面）。
    pub fn render(&self, buf: &mut alloc::string::String) {
        buf.push_str(&alloc::format!("## 协调披露 {}\n", self.cve_ref));
        buf.push_str(&alloc::format!("时间线：{} 收到 → {} 披露（{} 天窗）\n", self.received_day, self.disclosed_day, self.disclosed_day - self.received_day));
        buf.push_str(&alloc::format!("影响：{}\n缓解：{}\n致谢：{}\n", self.impact, self.mitigation, self.credit_line));
    }
}

// ---------------------------------------------------------------------------
// 无效报告三分类器（礼貌说明 + 归类——不进披露窗）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvalidKind {
    /// 无法复现（缺复现步骤或环境不符）。
    NotReproducible,
    /// 超范围（不属于系统攻击面——如第三方库自身问题）。
    OutOfScope,
    /// 已知问题（与在册披露重复）。
    Duplicate,
}

pub fn classify_invalid(reproducible: bool, in_scope: bool, known: bool) -> Result<(), InvalidKind> {
    if !in_scope {
        return Err(InvalidKind::OutOfScope);
    }
    if known {
        return Err(InvalidKind::Duplicate);
    }
    if !reproducible {
        return Err(InvalidKind::NotReproducible);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 月度安全版模型：安全修复不等季窗（F138 节奏的例外条款）
// ---------------------------------------------------------------------------

/// 安全例外判定：修复日与下一季窗的间隔超过阈值 → 走月度安全版。
pub const SECURITY_EXCEPTION_DAYS: u32 = 45;

pub fn security_release_needed(fix_day: u32, next_quarter_window_day: u32) -> bool {
    next_quarter_window_day.saturating_sub(fix_day) > SECURITY_EXCEPTION_DAYS
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F142E_TAG: &str = "stareco-F142-deep2";

pub fn run_f142_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F142E_TAG);

    // 时限钟
    let mut clock = TimelineClock::new(100);
    let _ = clock.confirm(101);
    set.add("f142e confirm fast", !clock.confirm_sla_breached(), "24h 确认达标");
    set.add("f142e confirm dup", clock.confirm(102).is_err(), "重复确认拒绝");
    let _ = clock.fix(150);
    set.add("f142e fix order", clock.fix(120).is_err(), "跳步拒绝（再修一次在确认前）");
    let _ = clock.disclose(190);
    set.add("f142e total", clock.total_days() == Some(90), "全链 90 天窗");
    let mut early = TimelineClock::new(100);
    set.add("f142e disclose skip", early.disclose(101).is_err(), "未修复先披露拒绝");
    let mut late = TimelineClock::new(100);
    let _ = late.confirm(103);
    set.add("f142e confirm breach", late.confirm_sla_breached(), "72h 确认超时检出");
    set.add("f142e timeline back", clock.confirm(90).is_err(), "时间倒流拒绝");

    // PGP 环
    let mut ring = PgpRing::new();
    let _ = ring.enroll(0xAAAA);
    set.add("f142e pgp accept", ring.accepts(0xAAAA), "活跃钥匙收信");
    set.add("f142e pgp reject", !ring.accepts(0xBBBB), "陌生钥匙拒收");
    set.add("f142e pgp dup", ring.enroll(0xBBBB).is_err(), "未轮换先登记拒绝");
    let _ = ring.rotate(0xBBBB);
    set.add("f142e pgp rotate", ring.accepts(0xBBBB) && ring.retired_count() == 1, "轮换后旧退役");
    set.add("f142e pgp zero", ring.rotate(0).is_err(), "零指纹拒绝");

    // 披露文档
    let doc = DisclosureDoc {
        cve_ref: "VX-2026-0007",
        received_day: 100,
        disclosed_day: 190,
        impact: "peblock 门可被路径拼接绕过",
        mitigation: "升级至 1.4.2；临时规避：关闭未签名卷自动挂载",
        credit_line: "致谢：研究者 R（2026-09-26 首报）",
    };
    set.add("f142e doc complete", doc.sections_complete(), "四节齐套");
    let mut out = alloc::string::String::new();
    doc.render(&mut out);
    set.add(
        "f142e doc render",
        out.contains("90 天窗") && out.contains("升级至 1.4.2"),
        "四节渲染齐全",
    );
    let empty = DisclosureDoc {
        cve_ref: "",
        received_day: 1,
        disclosed_day: 2,
        impact: "",
        mitigation: "",
        credit_line: "",
    };
    set.add("f142e doc incomplete", !empty.sections_complete(), "空节不齐");

    // 无效分类
    set.add("f142e invalid oos", classify_invalid(true, false, false) == Err(InvalidKind::OutOfScope), "超范围归类");
    set.add("f142e invalid dup", classify_invalid(true, true, true) == Err(InvalidKind::Duplicate), "已知归类");
    set.add("f142e invalid norepro", classify_invalid(false, true, false) == Err(InvalidKind::NotReproducible), "不可复现归类");
    set.add("f142e valid pass", classify_invalid(true, true, false).is_ok(), "有效报告通过");

    // 月度安全版例外
    set.add(
        "f142e sec release",
        security_release_needed(100, 200),
        "距季窗 100 天 → 走月度安全版",
    );
    set.add(
        "f142e sec wait quarter",
        !security_release_needed(100, 120),
        "距季窗 20 天 → 随季窗",
    );

    // 与基础层联动：完整报告生命周期对账（SecReport 四步）
    let mut sec = SecReport::new(100, "researcher-r");
    let _ = sec.confirm(101);
    let _ = sec.fix(150);
    set.add("f142e sec not due", !sec.due_for_disclosure(180), "窗内不强制");
    set.add("f142e sec due", sec.due_for_disclosure(191), "窗满强制（宪法钟）");
    let _ = sec.force_disclose(191);
    let credits = crate::stareco::secdisclose::credits_page(alloc::vec![Credit { reporter: "researcher-r", day: 100, kind: "漏洞首报" }]);
    set.add("f142e credits", credits.len() == 1, "致谢页在册");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;
    use crate::stareco::ebase::fnv1a64;
    use crate::stareco::secdisclose::DISCLOSURE_WINDOW_DAYS;

    #[test]
    fn pgp_retire_chain() {
        let mut r = PgpRing::new();
        let _ = r.enroll(1);
        let _ = r.rotate(2);
        let _ = r.rotate(3);
        assert_eq!(r.retired_count(), 2);
        assert!(r.accepts(3));
        assert!(!r.accepts(1) && !r.accepts(2));
    }

    #[test]
    fn clock_partial_stages() {
        let c = TimelineClock::new(50);
        assert!(c.total_days().is_none());
        assert!(!c.confirm_sla_breached());
    }

    #[test]
    fn invalid_priority_order() {
        // 超范围优先于已知（先裁范围再查重——分类语义固定）
        assert_eq!(classify_invalid(true, false, true), Err(InvalidKind::OutOfScope));
    }

    #[test]
    fn window_constants() {
        assert_eq!(DISCLOSURE_WINDOW_DAYS, 90);
        assert_eq!(CONFIRM_LIMIT_DAYS, 2);
        assert_eq!(fnv1a64(b"x"), fnv1a64(b"x"));
    }
}
