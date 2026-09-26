//! F144 「Crafted for VARIX」徽标计划 · 完整设计（STAR I 主册 G-D-19）。
//!
//! **判据（主册）**：首批授予演练 3 应用全流程；签名验真双向通过；
//! 撤销路径实测。
//!
//! **设计要点（主册）**：授予三前提（控件宪章默认合规 B-1103 + 无
//! 障碍五判据 F141 + 三要素文案审查）；审核内容四查（合规/无障碍/
//! 文案/隐私声明）；申请免费（质量导向不收费——写进条款）；徽标三
//! 形态（横版/方版/单色）；审核 SLA 14 天；有效期随系统大版本（复检
//! 制）；撤销机制（质量回退即撤）+通知+申诉；伪造 → 签名验真拦截
//! +F142 报告；授予册数据进季报（F149）。
//!
//! 本模块是徽标计划的**授予核**：四查门禁、签名授予与双向验真
//! （ebase::fnv1a64 指纹链）、撤销与复检宽限、SLA 计时、授予册
//! （append-only——季报数据源）。

use crate::checks::CheckSet;
use crate::stareco::ebase::{fnv1a64, SeqLedger, TraceId};

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 审核 SLA（天）。
pub const REVIEW_SLA_DAYS: u32 = 14;
/// 标准升级后的复检宽限（天）：一季。
pub const GRACE_DAYS: u32 = 90;

/// 徽标三形态。
pub const BADGE_FORMS: [&str; 3] = ["horizontal", "square", "mono"];

// ---------------------------------------------------------------------------
// 四查门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReviewInput {
    /// ①合规：控件宪章默认合规（B-1103）。
    pub charter_compliant: bool,
    /// ②无障碍：F141 五判据全绿。
    pub a11y_green: bool,
    /// ③文案：三要素审查通过。
    pub copy_three_parts: bool,
    /// ④隐私声明在册。
    pub privacy_declared: bool,
}

impl ReviewInput {
    pub fn four_checks_pass(&self) -> bool {
        self.charter_compliant && self.a11y_green && self.copy_three_parts && self.privacy_declared
    }
}

/// 申请条款：免费（质量导向不收费——条款常量，渲染层从此取）。
pub const FREE_CLAUSE: &str = "申请永久免费：徽标只看质量，不收费、不排序、不加权。";

// ---------------------------------------------------------------------------
// 授予册与验真
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeState {
    Granted,
    /// 质量回退 → 撤销（可申诉）。
    Revoked,
    /// 标准升级 → 复检宽限期。
    GracePeriod,
}

#[derive(Clone, Copy, Debug)]
pub struct Grant {
    pub app: &'static str,
    pub state: BadgeState,
    /// 授予日 / 撤销日 / 宽限满日。
    pub granted_day: u32,
    pub revoked_day: u32,
    pub grace_end_day: u32,
    /// 版本指纹（授予时的应用版本——复检锚）。
    pub version_fp: u64,
    /// 授予签名 = fnv1a64(app ^ version_fp ^ granted_day ^ 授予密钥表)。
    pub signature: u64,
}

/// 授予密钥（演示表——真签名通道走内核 ksha256/kvault，见完成报告
/// 边界声明）。
const GRANT_KEY: u64 = 0x5EC5_1BAD_6E00_0001;

pub struct BadgeOffice {
    grants: [Option<Grant>; 16],
    count: usize,
    ledger: SeqLedger,
    seq: crate::stareco::ebase::SeqAlloc,
    /// 伪造拦截计数（验真双向的反向面）。
    pub forgeries_blocked: u32,
}

impl BadgeOffice {
    pub fn new() -> BadgeOffice {
        BadgeOffice {
            grants: [None; 16],
            count: 0,
            ledger: SeqLedger::new(),
            seq: crate::stareco::ebase::SeqAlloc::new(),
            forgeries_blocked: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 授予：四查全过才签发（SLA 由流程计时——本层记录申请日）。
    pub fn grant(&mut self, day: u32, app: &'static str, review: &ReviewInput) -> Result<TraceId, &'static str> {
        if !review.four_checks_pass() {
            return Err("四查未过：合规/无障碍/文案/隐私声明缺一不可");
        }
        if self.count >= 16 {
            return Err("grant book full");
        }
        let s = self.seq.take(day);
        let id = TraceId::new("CRAFT", day, s);
        if !id.is_valid() {
            return Err("bad grant id");
        }
        let version_fp = fnv1a64(app.as_bytes());
        let signature = fnv1a64(&version_fp.to_le_bytes())
            ^ fnv1a64(&(day as u64).to_le_bytes())
            ^ GRANT_KEY;
        self.grants[self.count] = Some(Grant {
            app,
            state: BadgeState::Granted,
            granted_day: day,
            revoked_day: 0,
            grace_end_day: 0,
            version_fp,
            signature,
        });
        self.count += 1;
        self.ledger.append(signature);
        Ok(id)
    }

    /// 验真（正向）：签名与册内重算一致 → 真徽标。
    pub fn verify(&self, app: &str, signature: u64) -> bool {
        self.grants[..self.count]
            .iter()
            .flatten()
            .any(|g| {
                g.app == app
                    && g.signature == signature
                    && g.state == BadgeState::Granted
                    && signature
                        == fnv1a64(&g.version_fp.to_le_bytes())
                            ^ fnv1a64(&(g.granted_day as u64).to_le_bytes())
                            ^ GRANT_KEY
            })
    }

    /// 验伪（反向）：伪造签名必被拦 + 计数（F142 报告由调用方路由）。
    pub fn reject_forgery(&mut self, app: &str, signature: u64) -> bool {
        if !self.verify(app, signature) {
            self.forgeries_blocked += 1;
            true
        } else {
            false
        }
    }

    /// 撤销：质量回退即撤（+通知语义由调用方渲染；申诉走 F148）。
    pub fn revoke(&mut self, app: &str, day: u32) -> Result<(), &'static str> {
        let g = self.grants[..self.count].iter_mut().flatten().find(|g| g.app == app).ok_or("unknown app")?;
        if g.state != BadgeState::Granted {
            return Err("not granted or already revoked");
        }
        g.state = BadgeState::Revoked;
        g.revoked_day = day;
        Ok(())
    }

    /// 标准升级 → 已授徽标宽限一季复检。
    pub fn open_grace(&mut self, app: &str, day: u32) -> Result<(), &'static str> {
        let g = self.grants[..self.count].iter_mut().flatten().find(|g| g.app == app).ok_or("unknown app")?;
        if g.state != BadgeState::Granted {
            return Err("grace only from granted");
        }
        g.state = BadgeState::GracePeriod;
        g.grace_end_day = day + GRACE_DAYS;
        Ok(())
    }

    /// 宽限期满未复检 → 撤（保鲜活）。
    pub fn grace_expired(&self, app: &str, today: u32) -> bool {
        self.grants[..self.count]
            .iter()
            .flatten()
            .any(|g| g.app == app && g.state == BadgeState::GracePeriod && today > g.grace_end_day)
    }

    /// 授予册（季报 F149 数据源）。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.verify()
    }

    /// 生效徽标数（季报指标）。
    pub fn active(&self) -> usize {
        self.grants[..self.count].iter().flatten().filter(|g| g.state == BadgeState::Granted).count()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F144_TAG: &str = "stareco-F144-craftbadge";

pub fn run_craftbadge_checks() -> CheckSet {
    let mut set = CheckSet::new(F144_TAG);
    let day = 20260926;

    // 条款：免费 + 三形态 + SLA
    set.add("f144 free clause in law", FREE_CLAUSE.contains("免费") && FREE_CLAUSE.contains("不收费"), "quality-only");
    set.add("f144 three forms", BADGE_FORMS == ["horizontal", "square", "mono"], "asset forms");
    set.add("f144 sla 14d", REVIEW_SLA_DAYS == 14, "audit clock");
    set.add("f144 grace one season", GRACE_DAYS == 90, "recheck window");

    // 四查门禁：缺一拒
    let full = ReviewInput { charter_compliant: true, a11y_green: true, copy_three_parts: true, privacy_declared: true };
    let mut office = BadgeOffice::new();
    set.add(
        "f144 four-check gate",
        office.grant(day, "x", &ReviewInput { charter_compliant: false, ..full }).is_err()
            && office.grant(day, "x", &ReviewInput { a11y_green: false, ..full }).is_err()
            && office.grant(day, "x", &ReviewInput { copy_three_parts: false, ..full }).is_err()
            && office.grant(day, "x", &ReviewInput { privacy_declared: false, ..full }).is_err(),
        "each check bites",
    );

    // 首批授予演练 3 应用全流程
    let a = office.grant(day, "notes-plus", &full).expect("grant a");
    let b = office.grant(day, "pixel-doc", &full).expect("grant b");
    let c = office.grant(day, "termx", &full).expect("grant c");
    set.add("f144 first batch 3 granted", office.len() == 3 && a.is_valid() && b != c, "3 apps");

    // 签名验真双向
    let sig_a = office.grants_probe("notes-plus", |g| g.signature);
    set.add("f144 verify genuine passes", office.verify("notes-plus", sig_a), "forward verify");
    set.add("f144 verify forgery blocked", office.reject_forgery("notes-plus", sig_a ^ 1) && office.forgeries_blocked == 1, "reverse verify");
    set.add("f144 genuine not counted as forgery", !office.reject_forgery("notes-plus", sig_a), "no false positive");

    // 撤销路径实测：撤后验真转红
    assert!(office.revoke("pixel-doc", day + 10).is_ok());
    set.add(
        "f144 revoke kills verify",
        !office.verify("pixel-doc", office.grants_probe("pixel-doc", |g| g.signature)) && office.active() == 2,
        "revoked stays revoked",
    );
    set.add("f144 double revoke rejected", office.revoke("pixel-doc", day + 11).is_err(), "idempotent");

    // 复检宽限：升级→宽限→期满未复检即撤语义
    assert!(office.open_grace("termx", day).is_ok());
    set.add("f144 grace active passes", office.grace_expired("termx", day + GRACE_DAYS) == false, "in window");
    set.add("f144 grace expiry detected", office.grace_expired("termx", day + GRACE_DAYS + 1), "out of window");

    // 授予册链完整（季报数据源）
    set.add("f144 grant ledger intact", office.ledger_ok(), "append-only");

    set
}

impl BadgeOffice {
    /// 探针：读指定徽标的字段（检查面，不泄漏内部存储）。
    fn grants_probe(&self, app: &str, f: impl Fn(&Grant) -> u64) -> u64 {
        self.grants[..self.count].iter().flatten().find(|g| g.app == app).map(f).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_revoke_cycle() {
        let mut office = BadgeOffice::new();
        let full = ReviewInput { charter_compliant: true, a11y_green: true, copy_three_parts: true, privacy_declared: true };
        office.grant(20260101, "app", &full).unwrap();
        let sig = office.grants_probe("app", |g| g.signature);
        assert!(office.verify("app", sig));
        office.revoke("app", 20260102).unwrap();
        assert!(!office.verify("app", sig));
        assert_eq!(office.active(), 0);
        assert!(office.ledger_ok());
    }
}
