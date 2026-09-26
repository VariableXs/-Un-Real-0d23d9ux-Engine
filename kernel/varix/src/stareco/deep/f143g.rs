//! 深化层四 · F143 星徽与品牌资产包（2026-09-27 深化批次四 · g 层）。
//!
//! 资产打包清单规划（按用途选集）、明暗配对校验、第三方使用审批
//! 流状态机、违例工单生命周期、品牌色板 token 导出。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 打包清单规划：用途 → 必需文件集（覆盖核对，缺件点名）
// ---------------------------------------------------------------------------

pub const USE_PRODUCT: u8 = 1;
pub const USE_WEB: u8 = 2;
pub const USE_MERCH: u8 = 4;

/// 必需集：产品=logo+wordmark；网页=+favicon+og；商品=+矢量源。
pub fn required_assets(use_bits: u8) -> alloc::vec::Vec<&'static str> {
    let mut v: alloc::vec::Vec<&'static str> = alloc::vec!["logo.svg", "wordmark.svg"];
    if use_bits & USE_WEB != 0 {
        v.push("favicon.ico");
        v.push("og-image.png");
    }
    if use_bits & USE_MERCH != 0 {
        v.push("logo-vector.ai");
    }
    v
}

/// 打包核对：返回缺失清单（空 = 打包齐备）。
pub fn bundle_gaps(use_bits: u8, have: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    required_assets(use_bits).iter().filter(|r| !have.contains(r)).copied().collect()
}

// ---------------------------------------------------------------------------
// 明暗配对校验：语义资产必须有 -light/-dark 对
// ---------------------------------------------------------------------------

/// 校验规则：清单中凡出现 logo-light 必有 logo-dark（反之亦然）。
pub fn theme_pair_violations(names: &[&'static str]) -> alloc::vec::Vec<&'static str> {
    let mut bad: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for n in names {
        if let Some(stem) = n.strip_suffix("-light") {
            let pair = alloc::format!("{stem}-dark");
            let ok = names.iter().any(|m| *m == pair.as_str());
            if !ok && !bad.contains(n) {
                bad.push(n);
            }
        }
        if let Some(stem) = n.strip_suffix("-dark") {
            let pair = alloc::format!("{stem}-light");
            let ok = names.iter().any(|m| *m == pair.as_str());
            if !ok && !bad.contains(n) {
                bad.push(n);
            }
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// 第三方使用审批流：Submitted→Reviewed→{Approved|Rejected(带理由)}
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApprovalStage {
    Submitted,
    Reviewed,
    Approved,
    Rejected,
}

pub struct ApprovalCase {
    pub applicant: &'static str,
    pub stage: ApprovalStage,
    pub reject_reason: Option<&'static str>,
}

impl ApprovalCase {
    pub fn advance(&mut self, to: ApprovalStage) -> Result<(), &'static str> {
        let legal = match (self.stage, to) {
            (ApprovalStage::Submitted, ApprovalStage::Reviewed)
            | (ApprovalStage::Reviewed, ApprovalStage::Approved)
            | (ApprovalStage::Reviewed, ApprovalStage::Rejected) => true,
            _ => false,
        };
        if !legal {
            return Err("非法审批迁移");
        }
        if to == ApprovalStage::Rejected && self.reject_reason.is_none() {
            return Err("拒绝必须带理由：零静默");
        }
        self.stage = to;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 违例工单：发现→整改期(14天)→复核{已整改|升级处置}
// ---------------------------------------------------------------------------

pub const REMEDIATION_DAYS: u32 = 14;

pub struct ViolationTicket {
    pub subject: &'static str,
    pub found_day: u32,
    pub remediated: Option<bool>,
}

impl ViolationTicket {
    /// 逾期：超 14 天未整改。
    pub fn overdue(&self, today: u32) -> bool {
        self.remediated.is_none() && today > self.found_day + REMEDIATION_DAYS
    }

    pub fn mark(&mut self, ok: bool) -> Result<(), &'static str> {
        if self.remediated.is_some() {
            return Err("复核结论只记一次");
        }
        self.remediated = Some(ok);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 品牌色板 token 导出：确定性行（名,hex,用途）
// ---------------------------------------------------------------------------

pub fn palette_export(colors: &[(&'static str, &'static str, &'static str)]) -> alloc::vec::Vec<alloc::string::String> {
    let mut out = alloc::vec![alloc::string::String::from("# name;hex;usage")];
    // 名字序（确定性）。
    let mut sorted: alloc::vec::Vec<_> = colors.iter().collect();
    for i in 1..sorted.len() {
        let k = sorted[i];
        let mut j = i;
        while j > 0 && sorted[j - 1].0 > k.0 {
            sorted[j] = sorted[j - 1];
            j -= 1;
        }
        sorted[j] = k;
    }
    out.extend(sorted.iter().map(|(n, h, u)| alloc::format!("{n};{h};{u}")));
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F143G_TAG: &str = "stareco-F143-deep4";

pub fn run_f143_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F143G_TAG);

    // 打包规划
    set.add(
        "f143g bundle product",
        bundle_gaps(USE_PRODUCT, &["logo.svg", "wordmark.svg"]).is_empty(),
        "产品用途齐备",
    );
    set.add(
        "f143g bundle web gaps",
        bundle_gaps(USE_WEB, &["logo.svg", "wordmark.svg"]) == alloc::vec!["favicon.ico", "og-image.png"],
        "网页缺件点名",
    );
    set.add(
        "f143g bundle merch",
        bundle_gaps(USE_MERCH | USE_WEB, &["logo.svg", "wordmark.svg", "favicon.ico"])
            == alloc::vec!["og-image.png", "logo-vector.ai"],
        "组合用途全量核对",
    );

    // 明暗配对
    let names = ["logo-light", "logo-dark", "mark-light"];
    set.add(
        "f143g pair violations",
        theme_pair_violations(&names) == alloc::vec!["mark-light"],
        "缺对点名",
    );
    set.add(
        "f143g pair clean",
        theme_pair_violations(&["x-light", "x-dark"]).is_empty(),
        "成对通过",
    );

    // 审批流
    let mut a = ApprovalCase { applicant: "acme", stage: ApprovalStage::Submitted, reject_reason: None };
    set.add("f143g skip approve", a.advance(ApprovalStage::Approved).is_err(), "跳过评审拒绝");
    let _ = a.advance(ApprovalStage::Reviewed);
    set.add(
        "f143g reject no reason",
        a.advance(ApprovalStage::Rejected).is_err(),
        "无理由拒绝拦截",
    );
    a.reject_reason = Some("将徽标用于商品售卖");
    set.add("f143g reject ok", a.advance(ApprovalStage::Rejected).is_ok(), "带理由拒绝放行");

    // 违例工单
    let mut v = ViolationTicket { subject: "site-x", found_day: 100, remediated: None };
    set.add("f143g overdue", !v.overdue(114) && v.overdue(115), "整改 14 天线");
    set.add("f143g mark once", v.mark(true).is_ok() && v.mark(false).is_err(), "复核只记一次");
    set.add("f143g no overdue after mark", !v.overdue(200), "已复核不再逾期");

    // 色板导出
    let pal = palette_export(&[
        ("star-blue", "#3A7BD5", "主色"),
        ("abyss", "#101418", "深底"),
    ]);
    set.add(
        "f143g palette",
        pal[0] == "# name;hex;usage" && pal[1] == "abyss;#101418;深底" && pal[2] == "star-blue;#3A7BD5;主色",
        "表头+名字序",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn required_sets_monotonic() {
        // 用途位叠加只会扩清单不会缩。
        let p = required_assets(USE_PRODUCT);
        let w = required_assets(USE_PRODUCT | USE_WEB);
        assert!(w.len() > p.len());
        assert!(w.starts_with(&p));
    }

    #[test]
    fn approve_happy_path() {
        let mut a = ApprovalCase { applicant: "x", stage: ApprovalStage::Submitted, reject_reason: None };
        let _ = a.advance(ApprovalStage::Reviewed);
        assert!(a.advance(ApprovalStage::Approved).is_ok());
        assert_eq!(a.stage, ApprovalStage::Approved);
    }
}
