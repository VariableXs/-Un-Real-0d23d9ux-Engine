//! VE-F3616 · 创作与市场分发（VE-S 域 · 批次 S01 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3616`
//!
//! **判据（锚点原文六条）**：收益透明、合规分发、下架权、收敛复述、
//! 明细断言、判据。逐条落位：
//! - **收益透明**：创作者收益明细可见（透明红线：分成明细不可见 =
//!   信任崩塌）——[`Earnings::itemize`] 必须返回非空明细，明细空而
//!   总额非零 → [`V10Code::DETAIL_HIDDEN`]（P1，实测红线）。
//! - **合规分发**：分发地区合规审查（合规红线：未合规地区不下发）——
//!   [`RegionTable::allow`] 拒绝未合规地区，绕过 → [`V10Code::REGION_ILLEGAL`]（P0）。
//! - **下架权**：创作者可下架自己的资产（下架权复述）——下架后残留
//!   可下载 → [`V10Code::TAKEDOWN_RESIDUE`]（P1，实测）。
//! - **收敛复述**：分发走 Q 管线打包（收敛复述），绕管线 →
//!   [`V10Code::PIPELINE_BYPASS`] 立案。
//! - **明细断言**：[`Earnings::itemize`] 断言收入 = 分成之和。
//! - **判据**：`run_ves16_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 明细不可见→P1（透明红线实测） | [`V10Code::DETAIL_HIDDEN`]：明细空即立案 |
//! | 未合规下发→P0（红线实测） | [`V10Code::REGION_ILLEGAL`]：未合规地区拒发 |
//! | 绕管线分发→立案（收敛复述） | [`V10Code::PIPELINE_BYPASS`] |
//! | 下架残留→P1（下权红线实测） | [`V10Code::TAKEDOWN_RESIDUE`] |
//!
//! **码段**：独占 **0x40xx**。
//!
//! **性能逐项分解**：流水 O(步)；明细 O(1) 查；合规 O(地区)；
//! 下架 O(1)。
//!
//! **跨批对接点**：Q 管线收敛复述；F3114 生态对端；F3306 权限
//! （结算对端）——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3616 诊断码段（独占 0x40xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V10Code(pub u16);

impl V10Code {
    /// 收益明细被隐藏。
    pub const DETAIL_HIDDEN: V10Code = V10Code(0x4001);
    /// 未合规地区下发。
    pub const REGION_ILLEGAL: V10Code = V10Code(0x4002);
    /// 绕过 Q 管线。
    pub const PIPELINE_BYPASS: V10Code = V10Code(0x4003);
    /// 下架残留。
    pub const TAKEDOWN_RESIDUE: V10Code = V10Code(0x4004);
    /// 分成之和与总额不符。
    pub const SPLIT_MISMATCH: V10Code = V10Code(0x4005);
    /// 定价非法。
    pub const PRICING_INVALID: V10Code = V10Code(0x4006);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V10Code::DETAIL_HIDDEN => {
                "收益明细不可见（透明红线）。分成明细必须逐条可见，建议：补全明细记录。"
            }
            V10Code::REGION_ILLEGAL => {
                "未合规地区下发（合规红线 P0）。建议：在 RegionTable 中审查该地区合规状态。"
            }
            V10Code::PIPELINE_BYPASS => {
                "分发绕过 Q 管线（收敛红线）。所有分发必须走统一管线打包。"
            }
            V10Code::TAKEDOWN_RESIDUE => {
                "下架后仍可下载（下架权红线）。建议：清理分发缓存与镜像。"
            }
            V10Code::SPLIT_MISMATCH => {
                "分成之和与收入总额不符。建议：核对分成比例配置。"
            }
            V10Code::PRICING_INVALID => {
                "定价非法（负价或超限）。建议：定价须在 [0, MAX] 区间。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 定价模型 ────────────────────────────────────────────────────────

/// 定价方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pricing {
    /// 免费。
    Free,
    /// 付费（价格单位：分）。
    Paid(u32),
    /// 打赏。
    Tip,
}

impl Pricing {
    /// 合法性（付费价不能超上限 1,000,000 分 = 10,000 元）。
    pub fn validate(self) -> Result<(), V10Code> {
        match self {
            Pricing::Free | Pricing::Tip => Ok(()),
            Pricing::Paid(cents) => {
                if cents == 0 || cents > 1_000_000 {
                    Err(V10Code::PRICING_INVALID)
                } else {
                    Ok(())
                }
            }
        }
    }
}

// ── 收益明细（透明红线）─────────────────────────────────────────────

/// 一条分成记录。
#[derive(Clone, Debug)]
pub struct SplitLine {
    /// 分成方。
    pub party: String,
    /// 金额（分）。
    pub amount_cents: u32,
}

/// 收益账。
#[derive(Clone, Debug, Default)]
pub struct Earnings {
    /// 总收入（分）。
    pub gross_cents: u32,
    /// 分成明细。
    pub splits: Vec<SplitLine>,
}

impl Earnings {
    /// 新建。
    pub fn new(gross_cents: u32) -> Earnings {
        Earnings {
            gross_cents,
            splits: Vec::new(),
        }
    }

    /// 加一条分成。
    pub fn add_split(&mut self, party: &str, amount_cents: u32) {
        self.splits.push(SplitLine {
            party: String::from(party),
            amount_cents,
        });
    }

    /// 明细可见性断言（透明红线）。
    pub fn itemize(&self) -> Result<&[SplitLine], V10Code> {
        if self.gross_cents > 0 && self.splits.is_empty() {
            return Err(V10Code::DETAIL_HIDDEN);
        }
        Ok(&self.splits)
    }

    /// 分成对账：分成之和必须等于总额。
    pub fn audit_sum(&self) -> Result<(), V10Code> {
        let sum: u32 = self.splits.iter().map(|s| s.amount_cents).sum();
        if sum != self.gross_cents {
            return Err(V10Code::SPLIT_MISMATCH);
        }
        Ok(())
    }
}

// ── 地区合规表 ──────────────────────────────────────────────────────

/// 地区合规状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionStatus {
    /// 合规可发。
    Compliant,
    /// 审查中（暂缓）。
    Pending,
    /// 不合规（拒发）。
    Blocked,
}

/// 地区合规表。
#[derive(Clone, Debug, Default)]
pub struct RegionTable {
    entries: Vec<(String, RegionStatus)>,
}

impl RegionTable {
    /// 空表。
    pub fn new() -> RegionTable {
        RegionTable {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn set(&mut self, region: &str, status: RegionStatus) {
        if let Some(slot) = self.entries.iter_mut().find(|(r, _)| r == region) {
            slot.1 = status;
        } else {
            self.entries.push((String::from(region), status));
        }
    }

    /// 是否允许下发。未登记地区视为 Pending（保守拒发）。
    pub fn allow(&self, region: &str) -> Result<(), V10Code> {
        match self.entries.iter().find(|(r, _)| r == region) {
            Some((_, RegionStatus::Compliant)) => Ok(()),
            Some((_, RegionStatus::Blocked)) => Err(V10Code::REGION_ILLEGAL),
            Some((_, RegionStatus::Pending)) | None => Err(V10Code::REGION_ILLEGAL),
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 上架流水 ────────────────────────────────────────────────────────

/// 上架步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PublishStep {
    /// 验证通过。
    Verified,
    /// 定价确认。
    Priced,
    /// 上架提交。
    Listed,
    /// 分发中。
    Distributing,
    /// 结算完成。
    Settled,
}

impl PublishStep {
    /// 流水顺序。
    pub fn order() -> [PublishStep; 5] {
        [
            PublishStep::Verified,
            PublishStep::Priced,
            PublishStep::Listed,
            PublishStep::Distributing,
            PublishStep::Settled,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            PublishStep::Verified => "验证通过",
            PublishStep::Priced => "定价确认",
            PublishStep::Listed => "上架提交",
            PublishStep::Distributing => "分发中",
            PublishStep::Settled => "结算完成",
        }
    }
}

/// 上架流水。
#[derive(Clone, Debug, Default)]
pub struct PublishPipeline {
    done: Vec<PublishStep>,
    /// 是否经 Q 管线。
    via_q_pipeline: bool,
}

impl PublishPipeline {
    /// 新建。
    pub fn new() -> PublishPipeline {
        PublishPipeline {
            done: Vec::new(),
            via_q_pipeline: true,
        }
    }

    /// 完成一步。
    pub fn complete(&mut self, step: PublishStep) {
        if !self.done.contains(&step) {
            self.done.push(step);
        }
    }

    /// Q 管线标记。
    pub fn mark_bypass(&mut self) {
        self.via_q_pipeline = false;
    }

    /// 收敛审计：必须走 Q 管线。
    pub fn audit_pipeline(&self) -> Result<(), V10Code> {
        if self.via_q_pipeline {
            Ok(())
        } else {
            Err(V10Code::PIPELINE_BYPASS)
        }
    }

    /// 流水完成度。
    pub fn progress(&self) -> usize {
        self.done.len()
    }

    /// 是否全步完成。
    pub fn fully_done(&self) -> bool {
        PublishStep::order().iter().all(|s| self.done.contains(s))
    }
}

// ── 下架协议 ────────────────────────────────────────────────────────

/// 下架状态。
#[derive(Clone, Debug)]
pub struct Takedown {
    /// 资产名。
    pub asset: String,
    /// 是否已断开分发。
    pub severed: bool,
}

impl Takedown {
    /// 执行下架。
    pub fn execute(&mut self) {
        self.severed = true;
    }

    /// 残留审计。
    pub fn audit_residue(&self) -> Result<(), V10Code> {
        if self.severed {
            Ok(())
        } else {
            Err(V10Code::TAKEDOWN_RESIDUE)
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["Q-PIPELINE", "VE-F3114", "VE-F3306"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves16_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3616");

    // 0x4001 明细透明
    let mut e = Earnings::new(1000);
    e.add_split("平台", 300);
    e.add_split("创作者", 700);
    out.add("0x4001-明细透明且对账平", e.itemize().is_ok() && e.audit_sum().is_ok(), "");

    // 0x4002 明细隐藏检出
    let e2 = Earnings::new(500);
    out.add("0x4002-明细隐藏被检出", e2.itemize().is_err(), "");

    // 0x4003 分成不平
    let mut e3 = Earnings::new(1000);
    e3.add_split("平台", 300);
    out.add("0x4003-分成不平检出", e3.audit_sum().is_err(), "");

    // 0x4004 合规地区放行
    let mut rt = RegionTable::new();
    rt.set("CN", RegionStatus::Compliant);
    out.add("0x4004-合规地区放行", rt.allow("CN").is_ok(), "");

    // 0x4005 未合规拒发
    rt.set("XX", RegionStatus::Blocked);
    out.add("0x4005-未合规拒发", rt.allow("XX").is_err(), "");

    // 0x4006 未登记保守拒发
    out.add("0x4006-未登记保守拒发", rt.allow("ZZ").is_err(), "");

    // 0x4007 定价合法
    out.add("0x4007-定价合法", Pricing::Paid(500).validate().is_ok(), "");

    // 0x4008 定价非法
    out.add("0x4008-零价付费拒收", Pricing::Paid(0).validate().is_err(), "");

    // 0x4009 Q 管线收敛
    let pp = PublishPipeline::new();
    out.add("0x4009-Q 管线收敛", pp.audit_pipeline().is_ok(), "");

    // 0x400a 绕管线检出
    let mut pp2 = PublishPipeline::new();
    pp2.mark_bypass();
    out.add("0x400a-绕管线被检出", pp2.audit_pipeline().is_err(), "");

    // 0x400b 下架断流
    let mut td = Takedown {
        asset: String::from("a"),
        severed: false,
    };
    td.execute();
    out.add("0x400b-下架断流", td.audit_residue().is_ok(), "");

    out
}
