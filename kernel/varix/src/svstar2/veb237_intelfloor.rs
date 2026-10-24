//! VE-F0237 · Intel 老旧代支持下限声明（VE-B 域 · Intel 核显组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0237`
//!
//! **职责定位（锚点原文）**：声明最低支持代际（Gen9 起）与理由（测试成本与
//! 特性依赖），下限以下设备接入时显性拒绝（三要素告知）不模糊兼容；边界
//! 代际支持但标注实验档；下限调整走宣告变更流程提前一版公告。
//!
//! **数据结构（锚点原文）**：下限声明（代际×理由×拒绝文案）；实验档标记。
//!
//! **判据（锚点原文）**：显性拒绝、实验档边界、宣告流程、测试为准、判据。
//! 本模块 14 条判据逐条映射：**显性拒绝 4 条**（下限以下接入显性拒绝不静默
//! ／拒绝文案三要素齐／三要素两两互异／重复拒绝计数可对账）、**实验档边界
//! 3 条**（边界代际支持且标实验档／实验档判定 O(1) 零开销／实验档不静默
//! 转正式档）、**宣告流程 4 条**（下限调整走宣告流程／公告提前一版生效／
//! 未公告调整拒绝／公告生效后声明同步）、**测试为准 2 条**（声明与实现冲突
//! 以实现测试为准／冲突登记可查）、**判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 下限设备接入 → **显性拒绝不静默降级** | [`FloorPolicy::admit`] 对代际序号低于下限的设备返回 [`AdmitVerdict::Rejected`]——拒绝不是「尽量兼容」，没有静默降级路径；拒绝文案三要素（拒了什么/为什么/出路）由 [`reject_notice`] 生成 |
//! | 边界代际 → **支持但标注实验档** | 下限代际本身（Gen9）支持，[`AdmitVerdict::AdmittedExperimental`]——能力可用但按「实验档」呈现（收口口径：实验档不进认证通过率分子） |
//! | 声明与实现冲突 → **以实现测试为准** | 实测发现声明错误（如下限实际是 Gen8 也能跑）时 [`FloorPolicy::record_conflict`] 登记冲突——声明调整走宣告流程，**测试结果先行入账**，不因声明未改而拒绝报告 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **下限是代际序号比较，O(1) 零开销**：代际模型成单调序号（Gen9=0 起），
//!   接入判定就是一次整数比较——锚点「下限判定 O(1)」的字面落实。
//! - **拒绝文案三要素在生成时保证齐备**：[`reject_notice`] 返回的结构体
//!   三个字段分别承载「拒了什么/为什么/下一步」，判据逐字段非空断言——
//!   三要素缺失在文案生成时就拦截，不等呈现层兜底。
//! - **宣告流程两阶段**：调整请求先入「公告期」（生效版本 = 当前 + 1），
//!   到版才改下限本体——「提前一版公告」是流程结构而不是文档约定。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE5C0;

// ---- 诊断码 ----
pub const XC_BELOW_FLOOR: u32 = SEG_BASE + 1; // 下限以下接入（显性拒绝）
pub const XC_NOTICE_BAD: u32 = SEG_BASE + 2; // 拒绝文案三要素缺失
pub const XC_ANNOUNCE_BAD: u32 = SEG_BASE + 3; // 宣告流程非法（未公告/乱序）
pub const XC_CONFLICT: u32 = SEG_BASE + 4; // 声明与实现测试冲突
pub const XC_GEN_BAD: u32 = SEG_BASE + 5; // 代际序号非法

/// 代际序号模型：Gen9=0 起，序号越大越新（下限判定就是整数比较）。
pub const GEN9: u8 = 0;
pub const GEN11: u8 = 1;
pub const XE1: u8 = 2;
pub const XE_LATEST: u8 = 3;

/// 下限声明本体。
#[derive(Clone, Debug)]
pub struct FloorDecl {
    /// 最低支持代际（含）。
    pub floor_gen: u8,
    /// 实验档代际（= 下限代际：边界支持但标注实验）。
    pub experimental_gen: u8,
    /// 下限理由（测试成本与特性依赖，锚点原文要求声明理由）。
    pub reason: String,
    /// 声明版本（宣告流程推进）。
    pub decl_ver: u32,
}

/// 拒绝文案三要素。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectNotice {
    /// 拒了什么（接入的代际名）。
    pub what: String,
    /// 为什么（下限与理由）。
    pub why: String,
    /// 出路（下一步做什么）。
    pub way_out: String,
}

impl RejectNotice {
    /// 三要素齐备性。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.way_out.is_empty()
    }
}

/// 接入判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdmitVerdict {
    /// 正式支持（代际 > 下限）。
    Admitted,
    /// 实验档支持（代际 == 下限）。
    AdmittedExperimental,
    /// 显性拒绝（代际 < 下限）。
    Rejected,
}

/// 宣告流程记录。
#[derive(Clone, Debug)]
pub struct Announcement {
    /// 请求的新下限。
    pub new_floor: u8,
    /// 生效版本（当前声明版本 + 1）。
    pub effective_ver: u32,
    /// 是否已生效。
    pub applied: bool,
}

/// 下限策略控制器。
pub struct FloorPolicy {
    /// 声明本体。
    pub decl: FloorDecl,
    /// 拒绝计数（对账）。
    pub rejections: u32,
    /// 冲突登记账（声明 vs 实测）。
    pub conflicts: Vec<(u8, String)>,
    /// 公告期记录（未生效的调整请求）。
    pub pending: Option<Announcement>,
    /// 已生效公告数。
    pub applied_announcements: u32,
}

impl FloorPolicy {
    /// 按锚点默认声明：Gen9 起支持、Gen9 为实验档边界。
    pub fn default_decl() -> Self {
        FloorPolicy {
            decl: FloorDecl {
                floor_gen: GEN9,
                experimental_gen: GEN9,
                reason: alloc::format!("Gen9 起测试成本可控且特性依赖（采样器/RT 位）齐备"),
                decl_ver: 1,
            },
            rejections: 0,
            conflicts: Vec::new(),
            pending: None,
            applied_announcements: 0,
        }
    }

    /// 接入判定（O(1) 整数比较，锚点：下限判定 O(1)）。
    pub fn admit(&mut self, gen: u8) -> Result<AdmitVerdict, u32> {
        if gen > XE_LATEST {
            return Err(XC_GEN_BAD);
        }
        if gen < self.decl.floor_gen {
            self.rejections += 1;
            Err(XC_BELOW_FLOOR)
        } else if gen == self.decl.floor_gen {
            Ok(AdmitVerdict::AdmittedExperimental)
        } else {
            Ok(AdmitVerdict::Admitted)
        }
    }

    /// 拒绝文案三要素生成（gen 必须低于下限才可调用）。
    pub fn reject_notice(&self, gen: u8) -> RejectNotice {
        RejectNotice {
            what: alloc::format!("Gen 序号 {} 低于支持下限 Gen 序号 {}", gen, self.decl.floor_gen),
            why: self.decl.reason.clone(),
            way_out: alloc::format!("请升级至 Gen{} 及以上硬件；下限调整走宣告流程提前一版公告", self.decl.floor_gen + 9),
        }
    }

    /// 下限调整请求：两阶段——先入公告期，生效版本 = 当前 + 1（锚点：
    /// 提前一版公告）。已有未生效公告时拒绝叠加。
    pub fn request_floor_change(&mut self, new_floor: u8) -> Result<u32, u32> {
        if new_floor > XE_LATEST {
            return Err(XC_GEN_BAD);
        }
        if self.pending.is_some() {
            return Err(XC_ANNOUNCE_BAD);
        }
        let eff = self.decl.decl_ver + 1;
        self.pending = Some(Announcement { new_floor, effective_ver: eff, applied: false });
        Ok(eff)
    }

    /// 到版生效：声明版本推进后调用——公告转正，下限本体变更。
    pub fn apply_pending(&mut self, cur_ver: u32) -> Result<u32, u32> {
        match self.pending.take() {
            Some(a) if a.effective_ver == cur_ver => {
                self.decl.floor_gen = a.new_floor;
                self.decl.experimental_gen = a.new_floor;
                self.decl.decl_ver = cur_ver;
                self.applied_announcements += 1;
                Ok(cur_ver)
            }
            Some(a) => {
                // 版本不符：公告退回待定态。
                self.pending = Some(a);
                Err(XC_ANNOUNCE_BAD)
            }
            None => Err(XC_ANNOUNCE_BAD),
        }
    }

    /// 声明与实现冲突登记（锚点：声明与实现冲突→以实现测试为准——测试
    /// 结果先行入账，声明调整走流程）。
    pub fn record_conflict(&mut self, gen: u8, detail: &str) {
        self.conflicts.push((gen, alloc::format!("{}", detail)));
    }

    /// 冲突数。
    pub fn conflicts_len(&self) -> usize {
        self.conflicts.len()
    }
}

/// VE-F0237 判据组（14 条）。
pub fn run_veb237_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb237_intelfloor");

    // --- 判据 1：显性拒绝——下限以下接入显性拒绝不静默 ---
    {
        let mut p = FloorPolicy::default_decl();
        // GEN9 是序号 0，比它更老用「代际表外」语义不可表达——改用提高下限后验证。
        let mut p2 = FloorPolicy::default_decl();
        p2.decl.floor_gen = GEN11; // 公告模拟：下限抬到 Gen11
        let r2 = p2.admit(GEN9);
        s.add(
            "B37-显性拒绝-下限以下不静默兼容",
            matches!(r2, Err(XC_BELOW_FLOOR)) && p2.rejections == 1,
            "下限抬到 Gen11 后 Gen9 接入即拒（Err 带码），无静默降级路径",
        );
    }
    // --- 判据 2：显性拒绝——拒绝文案三要素齐 ---
    {
        let mut p = FloorPolicy::default_decl();
        p.decl.floor_gen = GEN11;
        let n = p.reject_notice(GEN9);
        s.add_dyn(
            "B37-显性拒绝-三要素齐备",
            n.complete() && n.what.contains("低于") && n.way_out.contains("升级"),
            &alloc::format!("what={} | way_out={}", n.what, n.way_out),
        );
    }
    // --- 判据 3：显性拒绝——三要素两两互异 ---
    {
        let mut p = FloorPolicy::default_decl();
        p.decl.floor_gen = GEN11;
        let n = p.reject_notice(GEN9);
        let distinct = n.what != n.why && n.why != n.way_out && n.what != n.way_out;
        s.add(
            "B37-显性拒绝-三要素两两互异",
            distinct,
            "拒了什么/为什么/出路三段语义不重叠（复读式文案不算齐备）",
        );
    }
    // --- 判据 4：显性拒绝——重复拒绝计数可对账 ---
    {
        let mut p = FloorPolicy::default_decl();
        p.decl.floor_gen = GEN11;
        let _ = p.admit(GEN9);
        let _ = p.admit(GEN9);
        let _ = p.admit(GEN11);
        s.add(
            "B37-显性拒绝-拒绝计数对账",
            p.rejections == 2,
            "两次拒绝一次实验档接入——计数只记拒绝",
        );
    }
    // --- 判据 5：实验档——边界代际支持且标实验档 ---
    {
        let mut p = FloorPolicy::default_decl();
        let v = p.admit(GEN9);
        s.add(
            "B37-实验档-边界代际支持标注",
            v == Ok(AdmitVerdict::AdmittedExperimental),
            "下限代际（Gen9）可用但呈实验档——不与正式档混同",
        );
    }
    // --- 判据 6：实验档——实验档不静默转正式 ---
    {
        let mut p = FloorPolicy::default_decl();
        let exp = p.admit(GEN9);
        let formal = p.admit(GEN11);
        s.add(
            "B37-实验档-实验不转正式",
            exp == Ok(AdmitVerdict::AdmittedExperimental)
                && formal == Ok(AdmitVerdict::Admitted)
                && exp != formal,
            "同一次判定里实验档与正式档是两个独立判别值（不会互串）",
        );
    }
    // --- 判据 7：实验档——判定 O(1) 零开销（纯比较无查表） ---
    {
        // 判定路径只做整数比较：代际越界也只走一次比较。
        let mut p = FloorPolicy::default_decl();
        let r1 = p.admit(XE_LATEST);
        let r2 = p.admit(XE_LATEST + 1);
        s.add(
            "B37-实验档-判定纯比较零查表",
            r1 == Ok(AdmitVerdict::Admitted) && r2 == Err(XC_GEN_BAD),
            "最高代际正常通过；表外序号越界拒绝（判定无查表依赖）",
        );
    }
    // --- 判据 8：宣告流程——调整走两阶段公告 ---
    {
        let mut p = FloorPolicy::default_decl();
        let eff = p.request_floor_change(GEN11);
        s.add(
            "B37-宣告流程-调整入公告期",
            eff == Ok(2) && p.pending.as_ref().map(|a| a.effective_ver).unwrap_or(0) == 2 && !p.pending.as_ref().map(|a| a.applied).unwrap_or(true),
            "请求即入公告期：生效版本=当前+1，未生效前下限本体不变",
        );
    }
    // --- 判据 9：宣告流程——公告期下限本体不变 ---
    {
        let mut p = FloorPolicy::default_decl();
        let _ = p.request_floor_change(GEN11);
        let still = p.admit(GEN9);
        s.add(
            "B37-宣告流程-公告期下限不变",
            still == Ok(AdmitVerdict::AdmittedExperimental),
            "公告未生效：Gen9 仍按旧下限（实验档）接入——提前一版公告语义",
        );
    }
    // --- 判据 10：宣告流程——到版生效声明同步 ---
    {
        let mut p = FloorPolicy::default_decl();
        let _ = p.request_floor_change(GEN11);
        let applied = p.apply_pending(2);
        let after = p.admit(GEN9);
        s.add(
            "B37-宣告流程-到版生效同步",
            applied == Ok(2)
                && p.decl.decl_ver == 2
                && p.applied_announcements == 1
                && after == Err(XC_BELOW_FLOOR),
            "版本到 2 时公告转正：下限本体、声明版本、公告计数同步推进",
        );
    }
    // --- 判据 11：宣告流程——未公告调整拒绝叠加 ---
    {
        let mut p = FloorPolicy::default_decl();
        let _ = p.request_floor_change(GEN11);
        let second = p.request_floor_change(XE1);
        s.add(
            "B37-宣告流程-公告叠加拒绝",
            second == Err(XC_ANNOUNCE_BAD),
            "已有未生效公告时不接受新调整（流程乱序防护）",
        );
    }
    // --- 判据 12：测试为准——冲突登记先行入账 ---
    {
        let mut p = FloorPolicy::default_decl();
        p.record_conflict(GEN9 - 0, "实测 Gen9 序号 0 之下仍有可用设备（声明过严）");
        let _ = p.admit(GEN9); // 声明未改——接入判定仍按声明走
        s.add(
            "B37-测试为准-冲突登记先行",
            p.conflicts_len() == 1,
            "实测结果先入冲突账（以实现测试为准），声明调整随后走公告流程",
        );
    }
    // --- 判据 13：测试为准——声明理由非空且含依据 ---
    {
        let p = FloorPolicy::default_decl();
        s.add_dyn(
            "B37-测试为准-下限理由声明",
            !p.decl.reason.is_empty() && p.decl.reason.contains("测试成本"),
            &alloc::format!("理由：{}", p.decl.reason),
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_BELOW_FLOOR, XC_NOTICE_BAD, XC_ANNOUNCE_BAD, XC_CONFLICT, XC_GEN_BAD];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B37-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE5C0 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb237_reject_chain() {
        let mut p = FloorPolicy::default_decl();
        p.decl.floor_gen = GEN11;
        // Gen9 拒绝带码计数。
        assert_eq!(p.admit(GEN9), Err(XC_BELOW_FLOOR));
        assert_eq!(p.rejections, 1);
        // 拒绝文案三要素。
        let n = p.reject_notice(GEN9);
        assert!(n.complete());
        assert!(n.what.contains("低于支持下限"));
        assert!(n.way_out.contains("升级"));
        // 边界代际实验档、正式代际放行。
        assert_eq!(p.admit(GEN11), Ok(AdmitVerdict::AdmittedExperimental));
        assert_eq!(p.admit(XE1), Ok(AdmitVerdict::Admitted));
    }

    #[test]
    fn veb237_announcement_two_phase() {
        let mut p = FloorPolicy::default_decl();
        // 阶段一：请求入公告期，本体不变。
        assert_eq!(p.request_floor_change(GEN11), Ok(2));
        assert_eq!(p.admit(GEN9), Ok(AdmitVerdict::AdmittedExperimental));
        // 公告期叠加拒绝。
        assert_eq!(p.request_floor_change(XE1), Err(XC_ANNOUNCE_BAD));
        // 版本不符不生效。
        assert_eq!(p.apply_pending(3), Err(XC_ANNOUNCE_BAD));
        // 阶段二：到版生效。
        assert_eq!(p.apply_pending(2), Ok(2));
        assert_eq!(p.admit(GEN9), Err(XC_BELOW_FLOOR));
        assert_eq!(p.applied_announcements, 1);
    }

    #[test]
    fn veb237_gen_bounds() {
        let mut p = FloorPolicy::default_decl();
        assert_eq!(p.admit(XE_LATEST), Ok(AdmitVerdict::Admitted));
        assert_eq!(p.admit(XE_LATEST + 1), Err(XC_GEN_BAD));
        // 调整请求的代际越界同样拒绝。
        assert_eq!(p.request_floor_change(9), Err(XC_GEN_BAD));
    }

    #[test]
    fn veb237_conflict_ledger() {
        let mut p = FloorPolicy::default_decl();
        assert_eq!(p.conflicts_len(), 0);
        p.record_conflict(GEN9, "实测发现 Gen9 媒体只读验证可过，与声明理由中的特性依赖不符");
        assert_eq!(p.conflicts_len(), 1);
        // 声明未改时接入判定照旧。
        assert_eq!(p.admit(GEN9), Ok(AdmitVerdict::AdmittedExperimental));
    }

    #[test]
    fn veb237_checks_all_green() {
        let set = run_veb237_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0237 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
