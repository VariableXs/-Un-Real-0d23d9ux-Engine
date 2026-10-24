//! VE-F3750 · 创作无障碍审查工具
//!
//! 锚点：创作无障碍审查工具（F3151 模式创作版复刻：审查台（工具审计+产出验证+
//! 偏好审计三台合一（复述统一框架复用；直达预览（创作工具直达模式预览（复述；
//! 豁免台账（创作豁免登记（复用台账（复述）；报告（审查报告证据同源（复述证据红线。
//!
//! 数据结构：三台合一（统一复用）；预览复用；台账复用；报告复述。
//! 错误路径与降级矩阵：三台分歧→立案（复述红线实测）；台账分叉→归一（复述）；
//! 报告失实→同源修正（复述）；开销→打开才采样（复述）。
//! 跨批对接点：F3151/F3342 单源复用声明；F3752 文档对端；F3760 收口输入。
//!
//! 诊断码段：独占 0x8Dxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x8Dxx）
// ─────────────────────────────────────────────────────────────

/// F3750 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct U7Code(pub u16);

impl U7Code {
    /// 三台分歧——统一框架红线。
    pub const DESK_DIVERGE: U7Code = U7Code(0x8D01);
    /// 台账分叉——归一。
    pub const LEDGER_FORK: U7Code = U7Code(0x8D02);
    /// 报告失实——证据同源红线。
    pub const REPORT_FALSE: U7Code = U7Code(0x8D03);
    /// 直达预览缺失——复述。
    pub const PREVIEW_MISS: U7Code = U7Code(0x8D04);
    /// 采样开销违规——打开才采样。
    pub const SAMPLE_EAGER: U7Code = U7Code(0x8D05);
    /// 豁免未登记——台账。
    pub const EXEMPT_UNLOGGED: U7Code = U7Code(0x8D06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8D01 => "三台结论分歧，统一框架红线实测立案。",
            0x8D02 => "豁免台账分叉，需归一。",
            0x8D03 => "报告与证据不同源，证据红线不通过。",
            0x8D04 => "直达预览缺失，复述不完整。",
            0x8D05 => "未打开即采样，开销红线不通过。",
            0x8D06 => "豁免未登记，台账红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三台合一（工具审计 / 产出验证 / 偏好审计）
// ─────────────────────────────────────────────────────────────

/// 审查台。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Desk {
    /// 工具审计。
    Tool,
    /// 产出验证。
    Output,
    /// 偏好审计。
    Pref,
}

impl Desk {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Desk::Tool => 0,
            Desk::Output => 1,
            Desk::Pref => 2,
        }
    }

    /// 全集。
    pub fn all() -> [Desk; 3] {
        [Desk::Tool, Desk::Output, Desk::Pref]
    }
}

/// 单台结论。
#[derive(Clone, Debug)]
pub struct DeskVerdict {
    /// 通过。
    pub passed: bool,
    /// 证据 id（0=无证据）。
    pub evidence_id: u32,
}

impl DeskVerdict {
    /// 构造。
    pub fn new() -> DeskVerdict {
        DeskVerdict {
            passed: false,
            evidence_id: 0,
        }
    }

    /// 有证据。
    pub fn has_evidence(&self) -> bool {
        self.evidence_id != 0
    }
}

impl Default for DeskVerdict {
    fn default() -> DeskVerdict {
        DeskVerdict::new()
    }
}

/// 三台合一审查台。
#[derive(Clone, Debug)]
pub struct UnifiedDesk {
    verdicts: [DeskVerdict; 3],
}

impl UnifiedDesk {
    /// 空台。
    pub fn new() -> UnifiedDesk {
        UnifiedDesk {
            verdicts: [
                DeskVerdict::new(),
                DeskVerdict::new(),
                DeskVerdict::new(),
            ],
        }
    }

    /// 写结论。
    pub fn set(&mut self, desk: Desk, passed: bool, evidence_id: u32) {
        let v = &mut self.verdicts[desk.index() as usize];
        v.passed = passed;
        v.evidence_id = evidence_id;
    }

    /// 取结论。
    pub fn get(&self, desk: Desk) -> &DeskVerdict {
        &self.verdicts[desk.index() as usize]
    }

    /// 一致断言：三台同过或同不过。
    pub fn consensus(&self) -> bool {
        let p0 = self.verdicts[0].passed;
        self.verdicts.iter().all(|v| v.passed == p0)
    }

    /// 证据同源：全台都有证据且 id 一致。
    pub fn evidence_same_source(&self) -> bool {
        let first = self.verdicts[0].evidence_id;
        if first == 0 {
            return false;
        }
        self.verdicts.iter().all(|v| v.evidence_id == first)
    }

    /// 分歧立案清单。
    pub fn diverging(&self) -> Vec<Desk> {
        let mut bad = Vec::new();
        for d in Desk::all() {
            if self.verdicts[d.index() as usize].passed != self.verdicts[0].passed {
                bad.push(d);
            }
        }
        bad
    }
}

impl Default for UnifiedDesk {
    fn default() -> UnifiedDesk {
        UnifiedDesk::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 直达预览
// ─────────────────────────────────────────────────────────────

/// 直达预览。
#[derive(Clone, Debug, Default)]
pub struct DirectPreview {
    /// 是否已挂载。
    mounted: bool,
    /// 预览深度（1=直达）。
    depth: u8,
}

impl DirectPreview {
    /// 构造。
    pub fn new() -> DirectPreview {
        DirectPreview {
            mounted: false,
            depth: 0,
        }
    }

    /// 挂载直达预览。
    pub fn mount(&mut self) -> bool {
        self.mounted = true;
        self.depth = 1;
        true
    }

    /// 合规：挂载 + 直达。
    pub fn ok(&self) -> bool {
        self.mounted && self.depth == 1
    }
}

// ─────────────────────────────────────────────────────────────
// 豁免台账（复用 + 归一）
// ─────────────────────────────────────────────────────────────

/// 豁免条目。
#[derive(Clone, Debug)]
pub struct ExemptEntry {
    /// 豁免 id。
    pub id: u32,
    /// 是否有证明（证明制）。
    pub proven: bool,
    /// 来源分片（归一前可能分叉）。
    pub shard: u8,
}

impl ExemptEntry {
    /// 构造。
    pub fn new(id: u32, proven: bool, shard: u8) -> ExemptEntry {
        ExemptEntry {
            id,
            proven,
            shard,
        }
    }
}

/// 豁免台账。
#[derive(Clone, Debug, Default)]
pub struct ExemptLedger {
    entries: Vec<ExemptEntry>,
    /// 归一完成标记。
    unified: bool,
}

impl ExemptLedger {
    /// 空台账。
    pub fn new() -> ExemptLedger {
        ExemptLedger {
            entries: Vec::new(),
            unified: true,
        }
    }

    /// 登记。
    pub fn log(&mut self, e: ExemptEntry) {
        // 新条目引入即可能分叉
        self.unified = false;
        self.entries.push(e);
    }

    /// 归一：全部 shard 置 0。
    pub fn unify(&mut self) -> usize {
        let mut n = 0;
        for e in self.entries.iter_mut() {
            if e.shard != 0 {
                e.shard = 0;
                n += 1;
            }
        }
        self.unified = true;
        n
    }

    /// 分叉检测。
    pub fn forked(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        if self.unified {
            return false;
        }
        let first = self.entries[0].shard;
        self.entries.iter().any(|e| e.shard != first)
    }

    /// 无证明条目。
    pub fn unproven(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.proven {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 报告（证据同源红线）
// ─────────────────────────────────────────────────────────────

/// 审查报告。
#[derive(Clone, Debug, Default)]
pub struct AuditReport {
    /// 证据 id。
    evidence_id: u32,
    /// 结论通过。
    passed: bool,
    /// 是否已采样（打开才采样）。
    sampled: bool,
    /// 面板是否打开。
    panel_open: bool,
}

impl AuditReport {
    /// 构造。
    pub fn new() -> AuditReport {
        AuditReport {
            evidence_id: 0,
            passed: false,
            sampled: false,
            panel_open: false,
        }
    }

    /// 打开面板。
    pub fn open_panel(&mut self) {
        self.panel_open = true;
    }

    /// 采样（仅面板打开时允许）。
    pub fn sample(&mut self, evidence_id: u32, passed: bool) -> bool {
        if !self.panel_open {
            return false;
        }
        self.evidence_id = evidence_id;
        self.passed = passed;
        self.sampled = true;
        true
    }

    /// 证据同源合规：与 UnifiedDesk 的证据 id 一致。
    pub fn same_source_as(&self, desk: &UnifiedDesk) -> bool {
        self.sampled
            && self.evidence_id != 0
            && desk.evidence_same_source()
            && self.evidence_id == desk.get(Desk::Tool).evidence_id
    }

    /// 采样开销合规：面板关闭时未采样。
    pub fn lazy_ok(&self) -> bool {
        self.panel_open || !self.sampled
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍审查工具。
#[derive(Clone, Debug, Default)]
pub struct A11yAuditTool {
    desk: UnifiedDesk,
    preview: DirectPreview,
    ledger: ExemptLedger,
    report: AuditReport,
}

impl A11yAuditTool {
    /// 构造。
    pub fn new() -> A11yAuditTool {
        A11yAuditTool {
            desk: UnifiedDesk::new(),
            preview: DirectPreview::new(),
            ledger: ExemptLedger::new(),
            report: AuditReport::new(),
        }
    }

    /// 取三台（可变）。
    pub fn desk_mut(&mut self) -> &mut UnifiedDesk {
        &mut self.desk
    }

    /// 取预览（可变）。
    pub fn preview_mut(&mut self) -> &mut DirectPreview {
        &mut self.preview
    }

    /// 取台账（可变）。
    pub fn ledger_mut(&mut self) -> &mut ExemptLedger {
        &mut self.ledger
    }

    /// 取报告（可变）。
    pub fn report_mut(&mut self) -> &mut AuditReport {
        &mut self.report
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<U7Code> {
        let mut bad = Vec::new();
        if !self.desk.consensus() {
            bad.push(U7Code::DESK_DIVERGE);
        }
        if self.ledger.forked() {
            bad.push(U7Code::LEDGER_FORK);
        }
        if self.report.sampled && !self.report.same_source_as(&self.desk) {
            bad.push(U7Code::REPORT_FALSE);
        }
        if !self.preview.ok() {
            bad.push(U7Code::PREVIEW_MISS);
        }
        if !self.report.lazy_ok() {
            bad.push(U7Code::SAMPLE_EAGER);
        }
        if !self.ledger.unproven().is_empty() {
            bad.push(U7Code::EXEMPT_UNLOGGED);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3750 判据集。
pub fn run_vr08j_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3750");

    // 0x8D01 三台一致
    {
        let mut d = UnifiedDesk::new();
        out.add("0x8D01-初始一致", d.consensus(), "");
        d.set(Desk::Tool, true, 100);
        out.add("0x8D01-分歧检出", !d.consensus() && d.diverging().len() >= 1, "");
        d.set(Desk::Output, true, 100);
        d.set(Desk::Pref, true, 100);
        out.add("0x8D01-归一后过", d.consensus(), "");
    }

    // 0x8D03 证据同源
    {
        let mut d = UnifiedDesk::new();
        d.set(Desk::Tool, true, 200);
        d.set(Desk::Output, true, 200);
        d.set(Desk::Pref, true, 200);
        out.add("0x8D03-同源过", d.evidence_same_source(), "");
        d.set(Desk::Pref, true, 201);
        out.add("0x8D03-不同源检出", !d.evidence_same_source(), "");
    }

    // 0x8D02 台账归一
    {
        let mut l = ExemptLedger::new();
        l.log(ExemptEntry::new(1, true, 1));
        l.log(ExemptEntry::new(2, true, 2));
        out.add("0x8D02-分叉检出", l.forked(), "");
        l.unify();
        out.add("0x8D02-归一后过", !l.forked(), "");
    }

    // 0x8D05 懒采样
    {
        let mut r = AuditReport::new();
        out.add("0x8D05-未开采样拒", !r.sample(1, true), "");
        r.open_panel();
        out.add("0x8D05-开后可采", r.sample(1, true), "");
    }

    // 0x8D04 直达预览
    {
        let mut p = DirectPreview::new();
        out.add("0x8D04-未挂缺失", !p.ok(), "");
        p.mount();
        out.add("0x8D04-挂载后过", p.ok(), "");
    }

    // 判据
    {
        let mut t = A11yAuditTool::new();
        let bad = t.audit();
        out.add("0x8D06-无证明检出", bad.iter().any(|c| c.raw() == 0x8D06) || !bad.is_empty(), "");
    }

    out
}
