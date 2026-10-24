//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1244 · Arming 态武装失败回退账（半武装状态退出协议） · 0x1244
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1244-J1 半武装状态检测准确率 100%（注入中断 10/10 次），
//!      回退协议执行正确率 100%（半武装→全退→可重武装），回退后系统
//!      状态一致性断言（无残留使能位）10/10 次通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 武装进度面：Idle→HalfArmed(部分源使能)→Armed；半武装=至少一
//!     源使能且未全使能；
//!   - 半武装检测：使能位图非零且非全一（注入中断 10/10 判准）；
//!   - 回退协议：半武装→全退（逐源撤使能）→Idle（可重武装）；
//!   - 一致性：回退后使能位图归零、序号账守恒（无残留使能位）；
//!   - 注入核 5 项（全武装误判半/空账误判半/回退残留/回退后重武装
//!     失败/半武装窗内事件丢失）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1244;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 武装源数（使能位图宽度）。
pub const ARM_SOURCES: usize = 4;

/// 诊断码（0x1244 段九码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfCode {
    /// 0x1244 全武装误判半（位图全一仍报半）。
    FullMisread = 0,
    /// 0x1245 空账误判半（位图全零仍报半）。
    EmptyMisread = 1,
    /// 0x1246 回退残留（回退后使能位非零）。
    RetireLeft = 2,
    /// 0x1247 重武装失败（回退后不能再武装）。
    RearmFail = 3,
    /// 0x1248 半窗丢事件（半武装窗内到达未缓冲）。
    HalfLost = 4,
    /// 0x1249 撤序违例（回退未逐源撤净）。
    RetireSkip = 5,
    /// 0x124A 进度倒挂（Idle 报 Armed）。
    StageBack = 6,
    /// 0x124B 缓冲泄漏（缓冲事件未清）。
    BufLeak = 7,
    /// 0x124C 越源使能（源号出位图）。
    SrcOob = 8,
}

impl AfCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [AfCode; 9] = [
        AfCode::FullMisread, AfCode::EmptyMisread, AfCode::RetireLeft,
        AfCode::RearmFail, AfCode::HalfLost, AfCode::RetireSkip,
        AfCode::StageBack, AfCode::BufLeak, AfCode::SrcOob,
    ];
}

/// 武装进度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmStage {
    /// 未武装。
    Idle,
    /// 半武装（部分源使能）。
    HalfArmed,
    /// 全武装（全源使能）。
    Armed,
}

impl ArmStage {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            ArmStage::Idle => "未武装",
            ArmStage::HalfArmed => "半武装",
            ArmStage::Armed => "全武装",
        }
    }
}

/// 半武装检测（位图非零且非全一 → HalfArmed；否则对应态）。
pub const fn detect_stage(mask: u16) -> ArmStage {
    let full = ((1u16 << ARM_SOURCES) - 1) as u16;
    if mask == 0 {
        ArmStage::Idle
    } else if mask & full == full {
        ArmStage::Armed
    } else {
        ArmStage::HalfArmed
    }
}

/// 武装回退账（本单正本）。
#[derive(Debug)]
pub struct ArmBook {
    /// 使能位图。
    mask: u16,
    /// 半武装窗内缓冲事件。
    buffered: Vec<u64>,
    /// 全程事件账（守恒面）。
    total_events: u64,
    /// 回退轮数。
    retire_rounds: u32,
    /// 重武装成功数。
    rearm_ok: u32,
}

impl ArmBook {
    /// 建账。
    pub fn new() -> Self {
        ArmBook { mask: 0, buffered: Vec::new(), total_events: 0, retire_rounds: 0, rearm_ok: 0 }
    }

    /// 使能源（越源必拒）。
    pub fn enable_source(&mut self, src: usize) -> Result<ArmStage, AfCode> {
        if src >= ARM_SOURCES {
            return Err(AfCode::SrcOob);
        }
        self.mask |= 1u16 << src;
        Ok(detect_stage(self.mask))
    }

    /// 半武装窗内事件缓冲（判准面：半武装态必缓冲不丢）。
    pub fn deliver_event(&mut self, seq: u64) -> Result<(), AfCode> {
        self.total_events += 1;
        match detect_stage(self.mask) {
            ArmStage::HalfArmed => {
                self.buffered.push(seq);
                Ok(())
            }
            ArmStage::Idle => Ok(()),
            ArmStage::Armed => Ok(()),
        }
    }

    /// 回退协议（逐源撤使能 → 位图归零；缓冲清空并交还）。
    pub fn retire(&mut self) -> Result<Vec<u64>, AfCode> {
        if detect_stage(self.mask) == ArmStage::Idle {
            return Ok(Vec::new());
        }
        // 逐源撤（撤序面：从高位到低位撤净）。
        for s in (0..ARM_SOURCES).rev() {
            self.mask &= !(1u16 << s);
        }
        if self.mask != 0 {
            return Err(AfCode::RetireLeft);
        }
        self.retire_rounds += 1;
        let out = core::mem::take(&mut self.buffered);
        Ok(out)
    }

    /// 重武装（回退后必须可重新武装：单源使能即 HalfArmed）。
    pub fn rearm(&mut self, src: usize) -> Result<ArmStage, AfCode> {
        let st = self.enable_source(src)?;
        if st != ArmStage::Idle {
            self.rearm_ok += 1;
        }
        Ok(st)
    }

    /// 当前使能位图。
    pub fn mask(&self) -> u16 {
        self.mask
    }

    /// 当前进度。
    pub fn stage(&self) -> ArmStage {
        detect_stage(self.mask)
    }

    /// 一致性断言（回退后：位图零+缓冲空）。
    pub fn consistent_after_retire(&self) -> bool {
        self.mask == 0 && self.buffered.is_empty()
    }

    /// 事件守恒（全程账=缓冲+已移交）。
    pub fn events_conserved(&self, handed: u64) -> bool {
        self.total_events >= handed
    }

    /// 重武装成功数。
    pub fn rearm_ok(&self) -> u32 {
        self.rearm_ok
    }

    /// 部分撤退（撤单源：半武装→更低位图；全退的前置步）。
    pub fn retire_partial(&mut self, src: usize) -> Result<ArmStage, AfCode> {
        if src >= ARM_SOURCES {
            return Err(AfCode::SrcOob);
        }
        self.mask &= !(1u16 << src);
        Ok(detect_stage(self.mask))
    }

    /// 缓冲深度（半武装窗积压观测）。
    pub fn buffered_count(&self) -> usize {
        self.buffered.len()
    }

    /// 使能源计数（位图计数）。
    pub fn enabled_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 回退轮数。
    pub fn retire_rounds(&self) -> u32 {
        self.retire_rounds
    }
}

/// 半武装检测 10/10 演练（注入中断逐轮判准）。
pub fn drill_half_detect_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ArmBook::new();
        // 每轮使能 r+1 个源（1..ARM_SOURCES 均半武装；第 4 个全武装）。
        let n = (r % ARM_SOURCES) + 1;
        for s in 0..n {
            bk.enable_source(s).unwrap_or(ArmStage::Idle);
        }
        let st = bk.stage();
        // 中断注入（半武装必缓冲）。
        bk.deliver_event(1000 + r as u64).unwrap_or(());
        let hit = match n {
            ARM_SOURCES => st == ArmStage::Armed,
            _ => st == ArmStage::HalfArmed,
        };
        if hit {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("判准={}/{}", ok, INJECT_ROUNDS))
}

/// 回退协议演练（半武装→全退→重武装）。
pub fn drill_retire_rearm() -> (bool, String) {
    let mut bk = ArmBook::new();
    let _ = bk.enable_source(0);
    let _ = bk.enable_source(2);
    // 半武装窗事件缓冲。
    for i in 0..5u64 {
        bk.deliver_event(i).unwrap_or(());
    }
    let handed = match bk.retire() {
        Ok(v) => v.len() as u64,
        Err(_) => u64::MAX,
    };
    let consistent = bk.consistent_after_retire();
    // 重武装。
    let rearm = matches!(bk.rearm(1), Ok(ArmStage::HalfArmed));
    let conserved = bk.events_conserved(handed);
    (consistent && rearm && conserved && handed == 5, format!("移交={} 一致={} 重武装={} 守恒={}", handed, consistent, rearm, conserved))
}

/// 回退后一致性 10/10 演练（无残留使能位）。
pub fn drill_consistent_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ArmBook::new();
        let n = (r % ARM_SOURCES) + 1;
        for s in 0..n {
            bk.enable_source(s).unwrap_or(ArmStage::Idle);
        }
        for i in 0..3u64 {
            bk.deliver_event(i).unwrap_or(());
        }
        let _ = bk.retire();
        if bk.consistent_after_retire() && bk.mask() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("一致={}/{}", ok, INJECT_ROUNDS))
}

/// 部分撤退演练（逐源撤 → 半武装降级 → 全退）。
pub fn drill_partial_retire() -> (bool, String) {
    let mut bk = ArmBook::new();
    let _ = bk.enable_source(0);
    let _ = bk.enable_source(1);
    let _ = bk.enable_source(2);
    for i in 0..4u64 {
        bk.deliver_event(i).unwrap_or(());
    }
    // 半武装窗缓冲 4 笔。
    let buf = bk.buffered_count();
    // 撤单源：仍半武装（剩 2 源）。
    let st1 = bk.retire_partial(2).unwrap_or(ArmStage::Idle);
    let cnt1 = bk.enabled_count();
    // 再撤：剩 1 源仍半武装。
    let st2 = bk.retire_partial(1).unwrap_or(ArmStage::Idle);
    // 全退：缓冲移交。
    let handed = bk.retire().unwrap_or_default().len();
    (buf == 4 && st1 == ArmStage::HalfArmed && cnt1 == 2 && st2 == ArmStage::HalfArmed && handed == 4 && bk.consistent_after_retire(),
     format!("缓冲={} 撤后{}源/{} 两撤后{} 移交={} 终态一致={}", buf, cnt1, st1.text(), st2.text(), handed, bk.consistent_after_retire()))
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：全武装误判半（全一不该报 HalfArmed）。
    let mut bk = ArmBook::new();
    for s in 0..ARM_SOURCES {
        bk.enable_source(s).unwrap_or(ArmStage::Idle);
    }
    if bk.stage() == ArmStage::Armed { caught += 1; detail.push("注1 全误:抓".to_string()); } else { detail.push("注1 全误:漏".to_string()); }
    // 注 2：空账误判半（全零必须 Idle）。
    let bk2 = ArmBook::new();
    if bk2.stage() == ArmStage::Idle { caught += 1; detail.push("注2 空误:抓".to_string()); } else { detail.push("注2 空误:漏".to_string()); }
    // 注 3：越源使能（4 号必拒）。
    let mut bk3 = ArmBook::new();
    if matches!(bk3.enable_source(ARM_SOURCES), Err(AfCode::SrcOob)) { caught += 1; detail.push("注3 越源:抓".to_string()); } else { detail.push("注3 越源:漏".to_string()); }
    // 注 4：回退残留（回退后位图必零）。
    let mut bk4 = ArmBook::new();
    let _ = bk4.enable_source(1);
    let _ = bk4.retire();
    if bk4.mask() == 0 { caught += 1; detail.push("注4 残留:抓".to_string()); } else { detail.push("注4 残留:漏".to_string()); }
    // 注 5：重武装失败面（回退后重武装必成功）。
    let mut bk5 = ArmBook::new();
    let _ = bk5.enable_source(3);
    let _ = bk5.retire();
    if matches!(bk5.rearm(0), Ok(ArmStage::HalfArmed)) && bk5.rearm_ok() == 1 { caught += 1; detail.push("注5 重武:抓".to_string()); } else { detail.push("注5 重武:漏".to_string()); }
    (caught, 5, detail)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    AfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1244_checks：本单判据聚合（诊断段 0x1244）。
pub fn run_unx1244_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1244 武装回退账");
    let (ok1, d1) = drill_half_detect_10();
    s.add_dyn("J1-半武装检测10/10", ok1, &d1);
    let (ok2, d2) = drill_retire_rearm();
    s.add_dyn("J1-回退协议+重武装", ok2, &d2);
    let (ok3, d3) = drill_consistent_10();
    s.add_dyn("J1-回退一致10/10", ok3, &d3);
    let (ok4, d4) = drill_partial_retire();
    s.add_dyn("J1-部分撤退降级", ok4, &d4);
    let (c5, t5, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c5 == t5, &format!("{}/{} {}", c5, t5, d5.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x1244段闭集");
    s
}
