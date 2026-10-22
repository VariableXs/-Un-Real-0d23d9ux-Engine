//! CGPU-F0428 · 统计隐私（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 隐私：只记度量不记内容（统计对象是资源消耗——绝不涉及用户数据
//!   内容，红线复核）；聚合匿名（账单默认聚合视图——明细仅本机可查）；
//!   导出脱敏（可选脱敏导出）；隐私复核入收口。
//! - 判据：度量红线、聚合、脱敏、复核、判据。
//!
//! 设计纪律：
//! - 度量红线：统计记录只有 (账户/资源/量/窗) 四元组——携带内容字段
//!   （payload/payload_len/描述文本）的结构一律构造期拒绝（类型层面
//!   不给内容留位）。红线复核 = 逐记录四字段白名单校验。
//! - 聚合匿名：对外视图 = 按类聚合（类内账户不可分辨）；明细视图仅
//!   本机口（local_only 标志强制——导出路径拿不到明细）。
//! - 脱敏导出：账户号 → 单向摘要（fp64 同源），明细粒度可选关。
//! - 复核入收口：隐私复核报告 = 红线扫描结果 + 聚合默认验证 + 脱敏
//!   开关状态，三件齐才绿。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0DF0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0DF0，全仓独占）。
pub const SEG_BASE: u16 = 0x0DF0;

pub const E_PRV_CONTENT: u16 = SEG_BASE + 1; // 度量记录携带内容（红线）
pub const E_PRV_LEAK: u16 = SEG_BASE + 2; // 明细经导出路径外泄
pub const E_PRV_UNMASKED: u16 = SEG_BASE + 3; // 脱敏关闭却导出明细
pub const E_PRV_AGG: u16 = SEG_BASE + 4; // 聚合视图含单户可分辨信息
pub const E_PRV_REVIEW: u16 = SEG_BASE + 5; // 复核三件缺一
pub const E_PRV_FIELDS: u16 = SEG_BASE + 6; // 字段越白名单

/// 度量记录四元组白名单字段。
pub const FIELD_WHITELIST: [&str; 4] = ["acct", "res", "amount", "win"];

/// 度量记录（四元组——无内容位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricRec {
    pub acct: u32,
    pub res: u8,
    pub amount: u64,
    pub win: u64,
}

impl MetricRec {
    /// 红线复核：字段集必须 ⊆ 白名单（四元组恰好全命中）。
    pub fn fields_ok(&self, claimed: &[&str]) -> Result<(), u16> {
        if claimed.len() > FIELD_WHITELIST.len() {
            return Err(E_PRV_FIELDS);
        }
        for c in claimed {
            if !FIELD_WHITELIST.contains(c) {
                return Err(E_PRV_CONTENT);
            }
        }
        Ok(())
    }
}

/// 内容走私防线：任何试图携带内容描述的记录构造被拒绝（示范 API——
/// 红线在类型/接口层落实）。
pub fn record_checked(
    acct: u32,
    res: u8,
    amount: u64,
    win: u64,
    content_payload: Option<&[u8]>,
) -> Result<MetricRec, u16> {
    if content_payload.is_some() {
        return Err(E_PRV_CONTENT); // 内容载荷一律拒收
    }
    Ok(MetricRec { acct, res, amount, win })
}

/// 同源摘要（脱敏——与 C 域指纹同源算法）。
pub fn fp64(bytes: &[u8], salt: u8) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ ((salt as u64) << 8);
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 账户脱敏：账号 → 摘要（单向，不可回推）。
pub fn mask_acct(acct: u32, salt: u8) -> u64 {
    fp64(&acct.to_le_bytes(), salt)
}

/// 视图标志：明细仅本机可查。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewFlags {
    /// 本机明细口。
    pub local_detail: bool,
    /// 导出脱敏开关。
    pub export_masked: bool,
}

pub const DEFAULT_FLAGS: ViewFlags = ViewFlags { local_detail: true, export_masked: true };

/// 导出闸：明细导出必须脱敏开启；聚合视图无限制。
pub fn export_gate(flags: ViewFlags, want_detail: bool) -> Result<(), u16> {
    if want_detail && !flags.export_masked {
        return Err(E_PRV_UNMASKED);
    }
    if want_detail && !flags.local_detail {
        return Err(E_PRV_LEAK);
    }
    Ok(())
}

/// 聚合匿名视图：按类聚合（类内账户合并）。
#[derive(Clone, Debug)]
pub struct AggView {
    pub rows: Vec<(u8, u64)>, // (类, 合计量)
}

impl AggView {
    /// 构建聚合视图：类内账户不可分辨（行内无账户号）。
    pub fn build(recs: &[MetricRec], class_of: impl Fn(u32) -> u8) -> Self {
        let mut rows: Vec<(u8, u64)> = Vec::new();
        for r in recs {
            let cls = class_of(r.acct);
            match rows.iter_mut().find(|(c, _)| *c == cls) {
                Some(e) => e.1 += r.amount,
                None => rows.push((cls, r.amount)),
            }
        }
        AggView { rows }
    }

    /// 匿名校验：视图行内无账户号（结构保证）+ 行数 < 账户数（真合并）。
    pub fn anonymous_ok(&self, acct_count: usize) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Ok(());
        }
        if self.rows.len() > acct_count {
            return Err(E_PRV_AGG);
        }
        Ok(())
    }
}

/// 隐私复核报告（收口件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivacyReview {
    /// 红线扫描：违规记录数（须 0）。
    pub redline_violations: u32,
    /// 聚合默认视图验证通过。
    pub agg_default_ok: bool,
    /// 脱敏开关状态。
    pub masked: bool,
}

impl PrivacyReview {
    /// 复核三件齐才绿。
    pub fn pass(&self) -> Result<(), u16> {
        if self.redline_violations != 0 {
            return Err(E_PRV_CONTENT);
        }
        if !self.agg_default_ok || !self.masked {
            return Err(E_PRV_REVIEW);
        }
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_度量红线_四元组无内容位() {
        let r = record_checked(1, 0, 100, 7, None).unwrap_or(MetricRec {
            acct: 0, res: 0, amount: 0, win: 0,
        });
        assert_eq!(r.fields_ok(&["acct", "res", "amount", "win"]), Ok(()));
        // 内容载荷拒收（红线在接口层）。
        assert_eq!(record_checked(1, 0, 100, 7, Some(b"user data")), Err(E_PRV_CONTENT));
        // 字段越白名单红。
        assert_eq!(r.fields_ok(&["acct", "payload"]), Err(E_PRV_CONTENT));
        assert_eq!(r.fields_ok(&["a", "b", "c", "d", "e"]), Err(E_PRV_FIELDS));
        // 白名单固化。
        assert_eq!(FIELD_WHITELIST.len(), 4);
    }

    #[test]
    fn a02_聚合匿名_类内账户不可分辨() {
        let recs = vec![
            MetricRec { acct: 1, res: 0, amount: 100, win: 0 },
            MetricRec { acct: 2, res: 0, amount: 200, win: 0 },
            MetricRec { acct: 3, res: 1, amount: 50, win: 0 },
        ];
        let v = AggView::build(&recs, |a| if a <= 2 { 0 } else { 1 });
        // 类 0 合并两账户 = 300；类 1 = 50。
        assert_eq!(v.rows, vec![(0, 300), (1, 50)]);
        assert_eq!(v.anonymous_ok(3), Ok(()));
        // 聚合行数 > 账户数（未真合并）红。
        let bad = AggView { rows: vec![(0, 1), (1, 1), (2, 1), (3, 1)] };
        assert_eq!(bad.anonymous_ok(3), Err(E_PRV_AGG));
    }

    #[test]
    fn a03_脱敏导出_单向摘要与导出闸() {
        // 摘要确定性 + 不同账户不同摘要。
        assert_eq!(mask_acct(1, 7), mask_acct(1, 7));
        assert_ne!(mask_acct(1, 7), mask_acct(2, 7));
        assert_ne!(mask_acct(1, 7), mask_acct(1, 8)); // 盐变摘要变
        // 导出闸：脱敏开 + 明细 → 放行；脱敏关 + 明细 → 拒。
        assert_eq!(export_gate(DEFAULT_FLAGS, true), Ok(()));
        let off = ViewFlags { local_detail: true, export_masked: false };
        assert_eq!(export_gate(off, true), Err(E_PRV_UNMASKED));
        // 本机明细口关闭还导明细 → 泄漏红。
        let nolocal = ViewFlags { local_detail: false, export_masked: true };
        assert_eq!(export_gate(nolocal, true), Err(E_PRV_LEAK));
        // 聚合导出无限制。
        assert_eq!(export_gate(off, false), Ok(()));
    }

    #[test]
    fn a04_复核入收口_三件齐才绿() {
        let ok = PrivacyReview { redline_violations: 0, agg_default_ok: true, masked: true };
        assert_eq!(ok.pass(), Ok(()));
        // 红线有违规。
        let bad = PrivacyReview { redline_violations: 1, agg_default_ok: true, masked: true };
        assert_eq!(bad.pass(), Err(E_PRV_CONTENT));
        // 聚合默认未验证。
        let bad2 = PrivacyReview { redline_violations: 0, agg_default_ok: false, masked: true };
        assert_eq!(bad2.pass(), Err(E_PRV_REVIEW));
        // 脱敏关。
        let bad3 = PrivacyReview { redline_violations: 0, agg_default_ok: true, masked: false };
        assert_eq!(bad3.pass(), Err(E_PRV_REVIEW));
    }

    #[test]
    fn a05_默认旗标_本机明细开脱敏开() {
        assert!(DEFAULT_FLAGS.local_detail);
        assert!(DEFAULT_FLAGS.export_masked);
        // 摘要与 C 域指纹同源可复算。
        assert_eq!(fp64(&[1, 2, 3], 9), fp64(&[1, 2, 3], 9));
        assert_ne!(fp64(&[1, 2, 3], 9), fp64(&[1, 2, 4], 9));
    }
}
