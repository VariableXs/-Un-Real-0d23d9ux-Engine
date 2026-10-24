//! VE-F0071 · 呈现失败分类与恢复（VE-A 域 · A03b 同步与呈现组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0071`
//!
//! **职责定位（锚点原文）**：失败按类别矩阵处置：尺寸失效（out of date）→
//! 重建交换链、次优（suboptimal）→ 记录继续、设备丢失 → 上报上层走设备
//! 恢复、未知错误 → 保守重建并留证据；恢复过程界面不可见中间态。
//!
//! **数据结构（锚点原文）**：失败分类矩阵（类别 × 处置）；重建记录；证据
//! 留档。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 重建连续失败 → 升级处置（换格式或换呈现模式）有上限；
//! - 恢复期间新请求 → 排队不丢弃；
//! - 分类未知 → 保守处置加原始码留档。
//!
//! **性能逐项分解（锚点原文）**：分类 O(1) 查表；重建 O(surface) 一次性；
//! 排队 O(1)。
//!
//! **跨批对接点（锚点原文）**：上游 F0067/F0068；下游 F0103 降级通知联动。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面；恢复通知三要素。
//!
//! **判据（锚点原文）**：分类矩阵、升级处置、中间态不可见、证据留档、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE8A1 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PRECOV_VERSION: &str = "vea71_precov-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE8A1;

// ---- 诊断码 ----
pub const XC_UPGRADE_CAP: u32 = SEG_BASE + 1; // 升级处置达上限
pub const XC_QUEUE: u32 = SEG_BASE + 2; // 恢复期请求排队（记账非错误）
pub const XC_UNKNOWN: u32 = SEG_BASE + 3; // 未知错误（保守处置 + 留档）
pub const XC_NO_SURFACE: u32 = SEG_BASE + 4; // surface 未登记

/// 失败类别（分类矩阵的行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// 尺寸失效（out of date）。
    OutOfDate = 1,
    /// 次优（suboptimal，能用但性能受损）。
    Suboptimal = 2,
    /// 设备丢失。
    DeviceLost = 3,
    /// 未知错误（原始码留档）。
    Unknown = 4,
}

/// 处置动作（分类矩阵的列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 重建交换链。
    RebuildChain,
    /// 记录继续（不重建）。
    LogAndContinue,
    /// 上报上层走设备恢复。
    EscalateDevice,
    /// 保守重建 + 原始码留档。
    ConservativeRebuild,
}

/// 分类矩阵（类别 → 处置，查表 O(1)）。
pub fn classify(kind: FailKind) -> Action {
    match kind {
        FailKind::OutOfDate => Action::RebuildChain,
        FailKind::Suboptimal => Action::LogAndContinue,
        FailKind::DeviceLost => Action::EscalateDevice,
        FailKind::Unknown => Action::ConservativeRebuild,
    }
}

/// 重建记录。
#[derive(Clone, Copy, Debug)]
pub struct RebuildRec {
    pub surface_id: u32,
    /// 重建次数（升级处置的晋升依据）。
    pub attempts: u32,
    /// 当前处置档位（0=重建交换链，1=换格式，2=换呈现模式=上限）。
    pub level: u8,
    /// 是否恢复完成（中间态不可见的判定字段）。
    pub settled: bool,
}

/// 恢复期排队的请求。
#[derive(Clone, Copy, Debug)]
pub struct QueuedReq {
    pub surface_id: u32,
    pub seq: u64,
}

/// 呈现失败恢复器。
pub struct PresentRecovery {
    recs: Vec<RebuildRec>,
    cap: usize,
    last_code: u32,
    /// 证据留档（未知错误的原始码环形 32 条）。
    evidence: Vec<(u32, u32)>, // (surface_id, raw_code)
    /// 恢复期排队。
    queue: Vec<QueuedReq>,
    queue_seq: u64,
    /// 升级处置上限（档位 2 = 换呈现模式，再失败显性报上限码）。
    upgrade_cap: u8,
    /// 次优记录数（LogAndContinue 的留痕计数）。
    suboptimal_logged: u32,
}

impl PresentRecovery {
    pub fn new(cap: usize) -> PresentRecovery {
        PresentRecovery {
            recs: Vec::new(),
            cap,
            last_code: 0,
            evidence: Vec::new(),
            queue: Vec::new(),
            queue_seq: 0,
            upgrade_cap: 2,
            suboptimal_logged: 0,
        }
    }

    pub fn register_surface(&mut self, surface_id: u32) -> bool {
        surface_id != 0
            && !self.recs.iter().any(|r| r.surface_id == surface_id)
            && self.recs.len() < self.cap
            && {
                self.recs.push(RebuildRec { surface_id, attempts: 0, level: 0, settled: true });
                true
            }
    }

    /// 失败入口：分类 → 处置 → 记账。返回处置动作。
    pub fn on_failure(&mut self, surface_id: u32, kind: FailKind, raw_code: u32) -> Result<Action, u32> {
        let idx = match self.recs.iter().position(|r| r.surface_id == surface_id) {
            Some(i) => i,
            None => {
                self.last_code = XC_NO_SURFACE;
                return Err(self.last_code);
            }
        };
        match kind {
            FailKind::OutOfDate => {
                // 重建交换链：连续失败走升级阶梯。
                let r = &mut self.recs[idx];
                r.settled = false; // 中间态（重建中，界面不可见）
                r.attempts += 1;
                if r.attempts % 3 == 0 {
                    if r.level >= self.upgrade_cap {
                        self.last_code = XC_UPGRADE_CAP;
                        r.settled = true;
                        return Err(XC_UPGRADE_CAP);
                    }
                    r.level += 1;
                }
                Ok(Action::RebuildChain)
            }
            FailKind::Suboptimal => {
                self.suboptimal_logged += 1;
                Ok(Action::LogAndContinue)
            }
            FailKind::DeviceLost => {
                self.recs[idx].settled = false;
                Ok(Action::EscalateDevice)
            }
            FailKind::Unknown => {
                // 保守处置 + 原始码留档（证据环形 32 条）。
                if self.evidence.len() >= 32 {
                    self.evidence.remove(0);
                }
                self.evidence.push((surface_id, raw_code));
                self.last_code = XC_UNKNOWN;
                self.recs[idx].settled = false;
                Ok(Action::ConservativeRebuild)
            }
        }
    }

    /// 恢复完成（重建成功回执）：settled 置位，中间态解除。
    pub fn settled_ok(&mut self, surface_id: u32) -> bool {
        match self.recs.iter_mut().find(|r| r.surface_id == surface_id) {
            Some(r) => {
                r.settled = true;
                r.attempts = 0;
                true
            }
            None => false,
        }
    }

    /// 中间态查询：恢复过程界面不可见（true = 处于重建中，调用方应保持
    /// 上一帧呈现而非渲染半新半旧）。
    pub fn in_transition(&self, surface_id: u32) -> bool {
        match self.recs.iter().find(|r| r.surface_id == surface_id) {
            Some(r) => !r.settled,
            None => false,
        }
    }

    /// 恢复期间新请求排队（不丢弃）。
    pub fn enqueue_during_recovery(&mut self, surface_id: u32) -> u64 {
        self.queue_seq += 1;
        self.queue.push(QueuedReq { surface_id, seq: self.queue_seq });
        self.last_code = XC_QUEUE;
        self.queue_seq
    }

    /// 恢复完成后排空队列（按排队序回执）。
    pub fn drain_queue(&mut self) -> Vec<QueuedReq> {
        let out = core::mem::take(&mut self.queue);
        out
    }

    /// 换档位查询（升级处置当前档）。
    pub fn level_of(&self, surface_id: u32) -> Option<u8> {
        self.recs.iter().find(|r| r.surface_id == surface_id).map(|r| r.level)
    }

    pub fn evidence_len(&self) -> usize {
        self.evidence.len()
    }
    pub fn suboptimal_logged(&self) -> u32 {
        self.suboptimal_logged
    }
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 恢复通知三要素。
    pub fn recovery_notice(&self, surface_id: u32) -> String {
        match self.recs.iter().find(|r| r.surface_id == surface_id) {
            Some(r) if !r.settled => alloc::format!(
                "呈现恢复中（发生了什么：交换链重建第 {} 次；为什么：呈现失败触发分类处置；下一步：恢复期间保持上一帧，完成后自动续排）",
                r.attempts
            ),
            Some(_) => alloc::format!("呈现正常"),
            None => alloc::format!("surface 未登记"),
        }
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let trans = self.recs.iter().filter(|r| !r.settled).count();
        alloc::format!(
            "PRECOV surface{} 恢复中{} 排队{} 证据{} 次优{}",
            self.recs.len(),
            trans,
            self.queue.len(),
            self.evidence.len(),
            self.suboptimal_logged
        )
    }
}

// =====================================================================
// 自检（判据锚点：分类矩阵 / 升级处置 / 中间态不可见 / 证据留档）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0071 域自检。
pub fn run_vea71_checks() -> CheckSet {
    let mut set = CheckSet::new("vea71");

    // C71-01 分类矩阵：四类处置各就各位
    {
        let ok = classify(FailKind::OutOfDate) == Action::RebuildChain
            && classify(FailKind::Suboptimal) == Action::LogAndContinue
            && classify(FailKind::DeviceLost) == Action::EscalateDevice
            && classify(FailKind::Unknown) == Action::ConservativeRebuild;
        chk(&mut set, "C71-01 分类矩阵", ok, "矩阵行错位");
    }

    // C71-02 尺寸失效 → 重建且进入中间态
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let a = p.on_failure(1, FailKind::OutOfDate, 0);
        let ok = a == Ok(Action::RebuildChain) && p.in_transition(1);
        chk(&mut set, "C71-02 重建中间态", ok, "重建未进中间态");
    }

    // C71-03 次优 → 记录继续（不进中间态）
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let a = p.on_failure(1, FailKind::Suboptimal, 0);
        let ok = a == Ok(Action::LogAndContinue) && !p.in_transition(1)
            && p.suboptimal_logged() == 1;
        chk(&mut set, "C71-03 次优继续", ok, "次优误触发重建");
    }

    // C71-04 设备丢失 → 上报升级
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let a = p.on_failure(1, FailKind::DeviceLost, 0);
        let ok = a == Ok(Action::EscalateDevice) && p.in_transition(1);
        chk(&mut set, "C71-04 设备升级", ok, "设备丢失未升级");
    }

    // C71-05 未知错误 → 保守处置 + 原始码留档
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let a = p.on_failure(1, FailKind::Unknown, 0xDEAD);
        let ok = a == Ok(Action::ConservativeRebuild) && p.evidence_len() == 1
            && p.last_code() == XC_UNKNOWN;
        chk(&mut set, "C71-05 证据留档", ok, "未知错误未留档");
    }

    // C71-06 升级处置阶梯：3 次失败升一档
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        for _ in 0..3 {
            let _ = p.on_failure(1, FailKind::OutOfDate, 0);
        }
        let l1 = p.level_of(1);
        for _ in 0..3 {
            let _ = p.on_failure(1, FailKind::OutOfDate, 0);
        }
        let l2 = p.level_of(1);
        let ok = l1 == Some(1) && l2 == Some(2);
        chk(&mut set, "C71-06 升级阶梯", ok, "阶梯未晋升");
    }

    // C71-07 升级处置有上限：达档 2 后第 9 次失败显性报上限
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let mut hit_cap = false;
        for _ in 0..9 {
            if p.on_failure(1, FailKind::OutOfDate, 0) == Err(XC_UPGRADE_CAP) {
                hit_cap = true;
            }
        }
        let ok = hit_cap && p.level_of(1) == Some(2) && p.last_code() == XC_UPGRADE_CAP;
        chk(&mut set, "C71-07 升级上限", ok, "上限未显性");
    }

    // C71-08 恢复完成解除中间态并清零尝试数
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let _ = p.on_failure(1, FailKind::OutOfDate, 0);
        let _ = p.settled_ok(1);
        let ok = !p.in_transition(1);
        chk(&mut set, "C71-08 恢复完成", ok, "完成未解除中间态");
    }

    // C71-09 恢复期请求排队不丢弃
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let _ = p.on_failure(1, FailKind::OutOfDate, 0);
        let s1 = p.enqueue_during_recovery(1);
        let s2 = p.enqueue_during_recovery(1);
        let ok = p.queue_len() == 2 && s2 == s1 + 1;
        let drained = p.drain_queue();
        let ok = ok && drained.len() == 2 && p.queue_len() == 0;
        chk(&mut set, "C71-09 排队不丢", ok, "排队丢失");
    }

    // C71-10 证据环形：超 32 条挤掉最旧
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        for i in 0..40u32 {
            let _ = p.on_failure(1, FailKind::Unknown, i);
        }
        let ok = p.evidence_len() == 32;
        chk(&mut set, "C71-10 证据环形", ok, "证据表未环形");
    }

    // C71-11 未登记 surface 拒绝
    {
        let mut p = PresentRecovery::new(8);
        let ok = p.on_failure(9, FailKind::OutOfDate, 0) == Err(XC_NO_SURFACE);
        chk(&mut set, "C71-11 未登记拒", ok, "未登记被放行");
    }

    // C71-12 恢复通知三要素
    {
        let mut p = PresentRecovery::new(8);
        let _ = p.register_surface(1);
        let _ = p.on_failure(1, FailKind::OutOfDate, 0);
        let n = p.recovery_notice(1);
        let ok = n.contains("发生了什么") && n.contains("为什么") && n.contains("下一步");
        chk(&mut set, "C71-12 通知三要素", ok, "通知缺要素");
    }

    // C71-13 确定性
    {
        let seq = || -> (u32, usize) {
            let mut p = PresentRecovery::new(8);
            let _ = p.register_surface(1);
            let _ = p.on_failure(1, FailKind::Suboptimal, 0);
            let _ = p.on_failure(1, FailKind::Unknown, 7);
            (p.suboptimal_logged(), p.evidence_len())
        };
        chk(&mut set, "C71-13 确定性", seq() == seq(), "两次不一致");
    }

    // C71-14 读屏行五要素
    {
        let p = PresentRecovery::new(8);
        let line = p.status_line();
        let ok = line.contains("surface") && line.contains("恢复中") && line.contains("排队")
            && line.contains("证据") && line.contains("次优");
        chk(&mut set, "C71-14 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea71_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
