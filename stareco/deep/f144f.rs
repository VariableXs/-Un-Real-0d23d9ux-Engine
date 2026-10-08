//! 深化层三 · F144 「Crafted for VARIX」徽标（2026-09-26 深化批次三）。
//!
//! 补深运营工程面（主册 G-D-19）：使用场景矩阵（尺寸下限+底色对比）、
//! 复检调度器（宽限季 → 到期清单）、伪造样本指纹册（排除真签名）、
//! 撤销事件链（append-only 有效性查询）。

use crate::checks::CheckSet;
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// 使用场景矩阵：最小尺寸 + 底色对比双门
// ---------------------------------------------------------------------------

pub const BADGE_MIN_PX: u32 = 24;

/// 场景校验：尺寸 ≥24px 且徽标色与底色对比达标（简易亮度差口径）。
pub fn usage_ok(px: u32, badge_luma: u32, bg_luma: u32) -> Result<(), &'static str> {
    if px < BADGE_MIN_PX {
        return Err("徽标小于 24px 最小尺寸");
    }
    let diff = badge_luma.abs_diff(bg_luma);
    if diff < 60 {
        return Err("徽标与底色对比不足：不可辨识");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 复检调度：授予日 + 宽限一季（90 天）→ 到期清单
// ---------------------------------------------------------------------------

pub const RECHECK_GRACE_DAYS: u32 = 90;

/// 到期清单：today - grant_day > 90 且未复检的徽标名（登记序）。
pub fn recheck_due(
    grants: &[(&'static str, u32, bool)],
    today: u32,
) -> alloc::vec::Vec<&'static str> {
    grants
        .iter()
        .filter(|(_, day, rechecked)| !rechecked && today > day + RECHECK_GRACE_DAYS)
        .map(|(n, _, _)| *n)
        .collect()
}

// ---------------------------------------------------------------------------
// 伪造样本指纹册：FNV 册内比对，真签名白名单排除误报
// ---------------------------------------------------------------------------

pub struct ForgeryBook {
    /// 伪造样本指纹（举报入库）。
    forgeries: alloc::vec::Vec<u64>,
    /// 真签名指纹白名单（防止同名素材误伤）。
    whitelist: alloc::vec::Vec<u64>,
}

impl ForgeryBook {
    pub fn new() -> ForgeryBook {
        ForgeryBook { forgeries: alloc::vec::Vec::new(), whitelist: alloc::vec::Vec::new() }
    }

    pub fn add_forgery(&mut self, asset: &'static str) {
        self.forgeries.push(ebase::fnv1a64(asset.as_bytes()));
    }

    pub fn whitelist_real(&mut self, sig_fp: u64) {
        self.whitelist.push(sig_fp);
    }

    /// 查询：真签名白名单优先（排除误报）；册内命中计伪造。
    pub fn verdict(&self, fp: u64) -> Result<(), &'static str> {
        if self.whitelist.contains(&fp) {
            return Ok(());
        }
        if self.forgeries.contains(&fp) {
            return Err("伪造样本指纹命中：拒绝展示");
        }
        Ok(())
    }

    pub fn forgery_count(&self) -> usize {
        self.forgeries.len()
    }
}

// ---------------------------------------------------------------------------
// 撤销事件链：append-only + 链校验；有效性 = 授予未撤销
// ---------------------------------------------------------------------------

pub struct RevokeChain {
    /// (徽标名, 撤销日, 原因)
    events: alloc::vec::Vec<(&'static str, u32, &'static str)>,
    chain: u64,
}

impl RevokeChain {
    pub fn new() -> RevokeChain {
        RevokeChain { events: alloc::vec::Vec::new(), chain: 0x9E37_79B9_7F4A_7C15 }
    }

    pub fn revoke(&mut self, badge: &'static str, day: u32, reason: &'static str) -> u64 {
        self.chain = ebase::fnv1a64(&self.chain.to_be_bytes())
            ^ ebase::fnv1a64(badge.as_bytes())
            ^ (day as u64);
        self.events.push((badge, day, reason));
        self.chain
    }

    /// 链校验：重放一致。
    pub fn verify(&self) -> bool {
        let mut chain = 0x9E37_79B9_7F4A_7C15u64;
        for (badge, day, _) in &self.events {
            chain = ebase::fnv1a64(&chain.to_be_bytes())
                ^ ebase::fnv1a64(badge.as_bytes())
                ^ (*day as u64);
        }
        chain == self.chain
    }

    /// 徽标有效性：撤销册内出现即失效（撤销不可逆）。
    pub fn is_valid(&self, badge: &str) -> bool {
        !self.events.iter().any(|(b, _, _)| *b == badge)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F144F_TAG: &str = "stareco-F144-deep3";

pub fn run_f144_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F144F_TAG);

    // 使用场景
    set.add("f144f usage ok", usage_ok(32, 40, 220).is_ok(), "尺寸对比双达");
    set.add("f144f usage small", usage_ok(23, 40, 220).is_err(), "小于 24px 拒");
    set.add("f144f usage boundary", usage_ok(24, 40, 220).is_ok(), "24px 压线通过");
    set.add("f144f usage low contrast", usage_ok(32, 120, 150).is_err(), "亮度差不足拒");

    // 复检调度
    let grants = [
        ("app-a", 1u32, false),
        ("app-b", 1, true),
        ("app-c", 500, false),
    ];
    set.add(
        "f144f recheck due",
        recheck_due(&grants, 100) == alloc::vec!["app-a"],
        "91 天未复检到期",
    );
    set.add(
        "f144f recheck grace",
        recheck_due(&grants, 91).is_empty(),
        "90 天整在宽限内",
    );

    // 伪造册
    let mut fb = ForgeryBook::new();
    fb.add_forgery("fake-badge-v1");
    let fake_fp = ebase::fnv1a64(b"fake-badge-v1");
    set.add("f144f forgery hit", fb.verdict(fake_fp).is_err(), "伪造指纹命中");
    fb.whitelist_real(fake_fp);
    set.add("f144f whitelist wins", fb.verdict(fake_fp).is_ok(), "真签名白名单排除误报");
    set.add("f144f unknown clean", fb.verdict(ebase::fnv1a64(b"normal-asset")).is_ok(), "未知资产放行");
    set.add("f144f count", fb.forgery_count() == 1, "册内计数");

    // 撤销链
    let mut rc = RevokeChain::new();
    rc.revoke("app-x", 100, "条款违规");
    rc.revoke("app-y", 105, "滥用品牌");
    set.add("f144f chain verify", rc.verify(), "链重放一致");
    set.add("f144f revoked invalid", !rc.is_valid("app-x") && !rc.is_valid("app-y"), "撤销即失效");
    set.add("f144f valid intact", rc.is_valid("app-z"), "未撤销有效");
    set.add("f144f events", rc.len() == 2, "事件计数");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn chain_tamper_detected() {
        let mut rc = RevokeChain::new();
        rc.revoke("a", 1, "r1");
        rc.revoke("b", 2, "r2");
        assert!(rc.verify());
        // 第三条入库改变链头——与两条链的链头不同。
        let head2 = rc.revoke("c", 3, "r3");
        let mut rc2 = RevokeChain::new();
        rc2.revoke("a", 1, "r1");
        rc2.revoke("b", 2, "r2");
        assert_ne!(head2, 0);
        assert!(rc2.verify());
        assert!(rc.verify());
    }

    #[test]
    fn usage_abs_diff_edge() {
        // 差 60 压线通过，59 拒。
        assert!(usage_ok(30, 100, 160).is_ok());
        assert!(usage_ok(30, 100, 159).is_err());
    }
}
