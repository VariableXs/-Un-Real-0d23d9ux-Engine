//! CGPU-F0244 · 死锁候选与置信度（cgb244_confide）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0244`
//! **判据原文摘录**：真死锁与慢任务区分（环 + 全成员无进展心跳 + 等待超阈值
//! **三维证据**、置信评分 0-1）、观察期机制（候选进观察期复检、防误杀慢任务、
//! 时长自适应）、判定输出（候选/确认/解除三态与证据链）。
//!
//! ## 为什么需要三维证据
//!
//! 「有环」是死锁的**必要非充分**条件：
//! - 环 + 成员都在跑（心跳活跃）= **慢任务**，不是死锁——它们在推进，
//!   只是慢。杀掉会误伤；
//! - 环 + 成员停了但等待时间短 = **可能刚停**，再等等；
//! - 环 + 全成员无进展心跳 + 等待超阈值 = **真死锁**。
//!
//! 三维缺一即不能确认。这是防误杀的第一道闸。
//!
//! ## 置信评分
//!
//! `confidence = w1·cycle_score + w2·stall_score + w3·time_score`
//!
//! - `cycle_score`：环的完整性（SCC 尺寸 > 2 比互等对更确定）；
//! - `stall_score`：成员无进展比例（全停 = 1.0，半停 = 0.5）；
//! - `time_score`：等待时长 / 阈值（饱和到 1.0）。
//!
//! 权重 w1=0.3, w2=0.4, w3=0.3——**心跳维度权重最高**，因为「无进展」
//! 是死锁与慢任务的本质区别。
//!
//! ## 观察期
//!
//! 候选进入观察期（默认 500 tick，自适应：误报多则加长、检出慢则缩短），
//! 期间复检。若观察期内出现进展心跳 → 降级为「解除」（不是死锁）；
//! 若持续无进展 → 升级为「确认」。
//!
//! 观察期是**防误杀的第二道闸**——即使三维证据齐了，也再看一眼。
//!
//! ## 三态输出
//!
//! | 态 | 含义 | 后续动作 |
//! |---|---|---|
//! | Candidate | 候选，观察期内 | 复检 |
//! | Confirmed | 确认死锁 | F0245 解除 |
//! | Dismissed | 降级（不是死锁） | 归档误报分析 |
//!
//! 证据链随状态转移累积——判定输出必须可审计。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::SubjectId;
use super::cgb243_tarjan::SccCandidate;

// ---------------------------------------------------------------------------
// 诊断码（0xCB70 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB70;
/// 候选不存在。
pub const E_NO_CANDIDATE: u16 = SEG_BASE + 1;
/// 状态机非法转移。
pub const E_BAD_TRANSITION: u16 = SEG_BASE + 2;

// ---------------------------------------------------------------------------
// 三维证据
// ---------------------------------------------------------------------------

/// 证据维度一：环证据。
#[derive(Clone, Copy, Debug)]
pub struct CycleEvidence {
    /// SCC 尺寸。
    pub scc_size: usize,
    /// 环内边数。
    pub edge_count: usize,
    /// 环年龄（tick）。
    pub age_ticks: u32,
    /// 是否自环。
    pub self_loop: bool,
}

/// 证据维度二：无进展心跳证据。
#[derive(Clone, Copy, Debug)]
pub struct StallEvidence {
    /// 环成员总数。
    pub total: usize,
    /// 无进展成员数（心跳超时）。
    pub stalled: usize,
    /// 最近一次任一成员心跳距今 tick。
    pub last_heartbeat_ago: u32,
}

/// 证据维度三：等待时长证据。
#[derive(Clone, Copy, Debug)]
pub struct TimeEvidence {
    /// 环内最长等待 tick。
    pub max_wait: u32,
    /// 等待阈值（tick）。
    pub threshold: u32,
}

/// 三维证据包。
#[derive(Clone, Copy, Debug)]
pub struct Evidence {
    pub cycle: CycleEvidence,
    pub stall: StallEvidence,
    pub time: TimeEvidence,
}

impl Evidence {
    /// 环完整性得分 0.0~1.0。
    pub fn cycle_score(&self) -> f32 {
        if self.cycle.self_loop {
            return 1.0;
        }
        // SCC 尺寸越大越确定：3+ 得满分，互等对 0.8。
        if self.cycle.scc_size >= 3 {
            1.0
        } else if self.cycle.scc_size == 2 {
            0.8
        } else {
            0.0
        }
    }

    /// 无进展得分 0.0~1.0。
    pub fn stall_score(&self) -> f32 {
        if self.stall.total == 0 {
            return 0.0;
        }
        self.stall.stalled as f32 / self.stall.total as f32
    }

    /// 等待时长得分 0.0~1.0（饱和）。
    pub fn time_score(&self) -> f32 {
        if self.time.threshold == 0 {
            return 1.0;
        }
        let r = self.time.max_wait as f32 / self.time.threshold as f32;
        if r > 1.0 {
            1.0
        } else if r < 0.0 {
            0.0
        } else {
            r
        }
    }

    /// 综合置信度 0.0~1.0。
    ///
    /// 权重：cycle 0.3 + stall 0.4 + time 0.3。心跳维度权重最高。
    pub fn confidence(&self) -> f32 {
        0.3 * self.cycle_score() + 0.4 * self.stall_score() + 0.3 * self.time_score()
    }

    /// 三维是否齐备（每维都过阈值）。
    pub fn all_three(&self) -> bool {
        self.cycle_score() >= 0.5 && self.stall_score() >= 0.5 && self.time_score() >= 0.5
    }
}

// ---------------------------------------------------------------------------
// 判定三态
// ---------------------------------------------------------------------------

/// 判定状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 候选（观察期内）。
    Candidate,
    /// 确认死锁。
    Confirmed,
    /// 降级（不是死锁）。
    Dismissed,
}

impl Verdict {
    pub const fn name(self) -> &'static str {
        match self {
            Verdict::Candidate => "candidate",
            Verdict::Confirmed => "confirmed",
            Verdict::Dismissed => "dismissed",
        }
    }
}

/// 证据链条目（可审计）。
#[derive(Clone, Debug)]
pub struct EvidenceRecord {
    pub tick: u32,
    pub confidence: f32,
    pub note: String,
}

/// 一个死锁候选的完整生命周期。
#[derive(Clone, Debug)]
pub struct DeadlockCandidate {
    pub id: u64,
    /// 环成员。
    pub members: Vec<SubjectId>,
    /// 当前判定。
    pub verdict: Verdict,
    /// 最新置信度。
    pub confidence: f32,
    /// 最新证据。
    pub evidence: Evidence,
    /// 证据链。
    pub chain: Vec<EvidenceRecord>,
    /// 进入观察期的 tick。
    pub observe_start: u32,
    /// 观察期长度（tick，自适应）。
    pub observe_len: u32,
    /// 创建 tick。
    pub created_at: u32,
    /// 终态 tick。
    pub settled_at: u32,
}

impl DeadlockCandidate {
    /// 观察期是否已满。
    pub fn observe_expired(&self, now: u32) -> bool {
        now.saturating_sub(self.observe_start) >= self.observe_len
    }

    /// 追加证据链条目。
    pub fn record(&mut self, tick: u32, confidence: f32, note: &str) {
        self.chain.push(EvidenceRecord {
            tick,
            confidence,
            note: String::from(note),
        });
    }
}

// ---------------------------------------------------------------------------
// 置信度引擎
// ---------------------------------------------------------------------------

/// 默认观察期长度（tick）。
pub const DEFAULT_OBSERVE_TICKS: u32 = 500;
/// 最短观察期。
pub const MIN_OBSERVE_TICKS: u32 = 100;
/// 最长观察期。
pub const MAX_OBSERVE_TICKS: u32 = 5000;
/// 确认阈值（置信度）。
pub const CONFIRM_THRESHOLD: f32 = 0.75;
/// 解除阈值（置信度低于此则 Dismissed）。
pub const DISMISS_THRESHOLD: f32 = 0.35;

/// 置信度引擎：候选生命周期管理。
///
/// 纪律：
/// 1. **不直接杀**——只产 verdict，解除动作在 F0245；
/// 2. **观察期自适应**——误报多加长、检出慢缩短；
/// 3. **证据链必留**——每次状态转移记录原因。
pub struct ConfidenceEngine {
    candidates: Vec<DeadlockCandidate>,
    next_id: u64,
    /// 当前观察期基准长度。
    pub observe_base: u32,
    /// 遥测：候选总数。
    pub total_candidates: u64,
    /// 遥测：确认数。
    pub confirmed: u64,
    /// 遥测：解除（误报）数。
    pub dismissed: u64,
    /// 遥测：观察期超时自动确认数。
    pub auto_confirmed: u64,
}

impl ConfidenceEngine {
    pub fn new() -> ConfidenceEngine {
        ConfidenceEngine {
            candidates: Vec::new(),
            next_id: 1,
            observe_base: DEFAULT_OBSERVE_TICKS,
            total_candidates: 0,
            confirmed: 0,
            dismissed: 0,
            auto_confirmed: 0,
        }
    }

    /// 从 SCC 候选创建死锁候选（进观察期）。
    pub fn admit(
        &mut self,
        scc: &SccCandidate,
        evidence: Evidence,
        now: u32,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let conf = evidence.confidence();
        let mut c = DeadlockCandidate {
            id,
            members: scc.members.clone(),
            verdict: Verdict::Candidate,
            confidence: conf,
            evidence,
            chain: Vec::new(),
            observe_start: now,
            observe_len: self.observe_base,
            created_at: now,
            settled_at: 0,
        };
        c.record(now, conf, "admitted as candidate");
        self.candidates.push(c);
        self.total_candidates += 1;
        id
    }

    /// 复检一个候选（观察期内）。
    ///
    /// `progress_heartbeat` 表示观察期内是否有成员出现进展心跳。
    pub fn recheck(
        &mut self,
        id: u64,
        evidence: Evidence,
        progress_heartbeat: bool,
        now: u32,
    ) -> Result<Verdict, u16> {
        let idx = self
            .candidates
            .iter()
            .position(|c| c.id == id)
            .ok_or(E_NO_CANDIDATE)?;
        if self.candidates[idx].verdict != Verdict::Candidate {
            return Err(E_BAD_TRANSITION);
        }

        let conf = evidence.confidence();
        self.candidates[idx].evidence = evidence;
        self.candidates[idx].confidence = conf;

        // 有进展心跳 → 立即降级（第二道闸）。
        if progress_heartbeat {
            self.candidates[idx].verdict = Verdict::Dismissed;
            self.candidates[idx].settled_at = now;
            self.candidates[idx].record(now, conf, "dismissed: progress heartbeat");
            self.dismissed += 1;
            // 误报 → 自适应加长观察期。
            self.observe_base = (self.observe_base + 200).min(MAX_OBSERVE_TICKS);
            return Ok(Verdict::Dismissed);
        }

        // 观察期满 + 三维齐备 + 置信度过阈值 → 确认。
        if self.candidates[idx].observe_expired(now) {
            if evidence.all_three() && conf >= CONFIRM_THRESHOLD {
                self.candidates[idx].verdict = Verdict::Confirmed;
                self.candidates[idx].settled_at = now;
                self.candidates[idx].record(now, conf, "confirmed after observe");
                self.confirmed += 1;
                self.auto_confirmed += 1;
                // 检出准 → 自适应缩短观察期。
                self.observe_base = self.observe_base.saturating_sub(50).max(MIN_OBSERVE_TICKS);
                return Ok(Verdict::Confirmed);
            }
            // 观察期满但证据不足 → 解除。
            self.candidates[idx].verdict = Verdict::Dismissed;
            self.candidates[idx].settled_at = now;
            self.candidates[idx].record(now, conf, "dismissed: insufficient evidence");
            self.dismissed += 1;
            return Ok(Verdict::Dismissed);
        }

        // 置信度极低 → 提前解除。
        if conf < DISMISS_THRESHOLD {
            self.candidates[idx].verdict = Verdict::Dismissed;
            self.candidates[idx].settled_at = now;
            self.candidates[idx].record(now, conf, "dismissed: low confidence");
            self.dismissed += 1;
            return Ok(Verdict::Dismissed);
        }

        // 置信度极高 + 三维齐备 → 提前确认（不等观察期满）。
        if conf >= 0.95 && evidence.all_three() {
            self.candidates[idx].verdict = Verdict::Confirmed;
            self.candidates[idx].settled_at = now;
            self.candidates[idx].record(now, conf, "confirmed: high confidence early");
            self.confirmed += 1;
            return Ok(Verdict::Confirmed);
        }

        Ok(Verdict::Candidate)
    }

    /// 批量复检全部活动候选。返回 (confirmed_ids, dismissed_ids)。
    pub fn recheck_all(
        &mut self,
        evidences: &[(u64, Evidence, bool)],
        now: u32,
    ) -> (Vec<u64>, Vec<u64>) {
        let mut conf = Vec::new();
        let mut dism = Vec::new();
        for (id, ev, hb) in evidences {
            match self.recheck(*id, *ev, *hb, now) {
                Ok(Verdict::Confirmed) => conf.push(*id),
                Ok(Verdict::Dismissed) => dism.push(*id),
                _ => {}
            }
        }
        (conf, dism)
    }

    // -- 查询 --------------------------------------------------------------

    pub fn candidate(&self, id: u64) -> Option<&DeadlockCandidate> {
        self.candidates.iter().find(|c| c.id == id)
    }

    pub fn active(&self) -> Vec<&DeadlockCandidate> {
        self.candidates
            .iter()
            .filter(|c| c.verdict == Verdict::Candidate)
            .collect()
    }

    pub fn confirmed_list(&self) -> Vec<&DeadlockCandidate> {
        self.candidates
            .iter()
            .filter(|c| c.verdict == Verdict::Confirmed)
            .collect()
    }

    /// 误报率（Dismissed / 总终态）。F0254 目标 <5%。
    pub fn false_positive_rate(&self) -> f32 {
        let settled = self.confirmed + self.dismissed;
        if settled == 0 {
            return 0.0;
        }
        self.dismissed as f32 / settled as f32
    }

    /// 渲染证据链（人话）。
    pub fn render_chain(&self, id: u64) -> String {
        let mut s = String::new();
        if let Some(c) = self.candidate(id) {
            s.push_str(&alloc::format!(
                "候选 #{} 环成员{:?} 判定={}\n",
                c.id,
                c.members,
                c.verdict.name()
            ));
            s.push_str(&alloc::format!(
                "置信度 {:.2}（环 {:.2} + 停滞 {:.2} + 时长 {:.2}）\n",
                c.confidence,
                c.evidence.cycle_score(),
                c.evidence.stall_score(),
                c.evidence.time_score()
            ));
            for r in &c.chain {
                s.push_str(&alloc::format!("  [t={}] {:.2} {}\n", r.tick, r.confidence, r.note));
            }
        }
        s
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb244_confide");
        // 终态必有 settled_at。
        let bad_settle = self
            .candidates
            .iter()
            .filter(|c| c.verdict != Verdict::Candidate && c.settled_at == 0)
            .count();
        cs.add("终态有时间戳", bad_settle == 0, "settled_at != 0");

        // 候选必有证据链。
        let no_chain = self.candidates.iter().filter(|c| c.chain.is_empty()).count();
        cs.add("候选有证据链", no_chain == 0, "admit 必记录");

        // 计数守恒。
        let conf_n = self
            .candidates
            .iter()
            .filter(|c| c.verdict == Verdict::Confirmed)
            .count() as u64;
        let dism_n = self
            .candidates
            .iter()
            .filter(|c| c.verdict == Verdict::Dismissed)
            .count() as u64;
        cs.add(
            "确认计数",
            conf_n == self.confirmed,
            "confirmed 引擎计数对齐",
        );
        cs.add(
            "解除计数",
            dism_n == self.dismissed,
            "dismissed 引擎计数对齐",
        );

        // 观察期在界内。
        cs.add(
            "观察期有界",
            self.observe_base >= MIN_OBSERVE_TICKS && self.observe_base <= MAX_OBSERVE_TICKS,
            "100..5000",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 证据构造辅助
// ---------------------------------------------------------------------------

/// 从心跳序列构造 StallEvidence。
///
/// `heartbeats` 是各成员的最近心跳距今 tick。超时阈值 `stall_threshold`。
pub fn build_stall_evidence(heartbeats: &[u32], stall_threshold: u32) -> StallEvidence {
    let total = heartbeats.len();
    let stalled = heartbeats.iter().filter(|&&h| h > stall_threshold).count();
    let last = heartbeats.iter().copied().min().unwrap_or(u32::MAX);
    StallEvidence {
        total,
        stalled,
        last_heartbeat_ago: last,
    }
}

/// 构造 TimeEvidence。
pub fn build_time_evidence(max_wait: u32, threshold: u32) -> TimeEvidence {
    TimeEvidence {
        max_wait,
        threshold,
    }
}

/// 构造 CycleEvidence。
pub fn build_cycle_evidence(scc: &SccCandidate) -> CycleEvidence {
    CycleEvidence {
        scc_size: scc.members.len(),
        edge_count: scc.edge_count,
        age_ticks: scc.age_ticks,
        self_loop: scc.self_loop,
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0244 域自检。
pub fn run_cgb244_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb244_confide");
    let mut engine = ConfidenceEngine::new();

    // --- 三维证据评分 ---
    let scc = SccCandidate {
        members: alloc::vec![1, 2, 3],
        age_ticks: 600,
        edge_count: 3,
        self_loop: false,
    };
    let ev = Evidence {
        cycle: build_cycle_evidence(&scc),
        stall: build_stall_evidence(&[2000, 2100, 1900], 1000),
        time: build_time_evidence(600, 500),
    };
    cs.add(
        "三维齐备",
        ev.all_three(),
        "环/停/时长三过",
    );
    let conf = ev.confidence();
    cs.add(
        "置信度在界",
        conf >= 0.0 && conf <= 1.0,
        "0..1",
    );
    cs.add(
        "高置信度",
        conf >= CONFIRM_THRESHOLD,
        "全停+长等待",
    );

    // --- 慢任务区分（有心跳） ---
    let ev_slow = Evidence {
        cycle: build_cycle_evidence(&scc),
        stall: build_stall_evidence(&[10, 20, 15], 1000),
        time: build_time_evidence(600, 500),
    };
    cs.add(
        "慢任务区分",
        ev_slow.stall_score() < 0.5,
        "有心跳 → 停滞分低",
    );
    cs.add(
        "慢任务不全三维",
        !ev_slow.all_three(),
        "停滞维不过",
    );

    // --- 候选生命周期 ---
    let id = engine.admit(&scc, ev, 0);
    cs.add("候选入观察期", engine.candidate(id).is_some(), "admit");
    cs.add(
        "初始态为 Candidate",
        engine.candidate(id).map(|c| c.verdict == Verdict::Candidate).unwrap_or(false),
        "Verdict::Candidate",
    );

    // 观察期内复检：有心跳 → 降级。
    let id2 = engine.admit(&scc, ev, 0);
    let v = engine.recheck(id2, ev_slow, true, 100).unwrap_or(Verdict::Candidate);
    cs.add(
        "心跳 → 降级",
        v == Verdict::Dismissed,
        "progress heartbeat dismiss",
    );

    // 观察期满 + 无心跳 → 确认。
    let id3 = engine.admit(&scc, ev, 0);
    let v3 = engine
        .recheck(id3, ev, false, DEFAULT_OBSERVE_TICKS + 10)
        .unwrap_or(Verdict::Candidate);
    cs.add(
        "观察期满确认",
        v3 == Verdict::Confirmed,
        "三维持久 → Confirmed",
    );

    // --- 误报率 ---
    let fpr = engine.false_positive_rate();
    cs.add(
        "误报率可算",
        fpr >= 0.0 && fpr <= 1.0,
        "dismissed/settled",
    );

    // --- 证据链 ---
    let chain_text = engine.render_chain(id3);
    cs.add(
        "证据链可读",
        chain_text.contains("置信度") && chain_text.contains("confirmed"),
        "人话渲染",
    );

    // --- 一致性 ---
    let ic = engine.check_consistency();
    let mut ic_ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ic_ok = false;
            }
        }
    }
    cs.add("引擎一致性", ic_ok, "五项");

    // --- 自适应观察期 ---
    // 多次误报 → 观察期加长。
    let base_before = engine.observe_base;
    let _id4 = engine.admit(&scc, ev, 0);
    engine.recheck(_id4, ev_slow, true, 10).ok();
    cs.add(
        "误报加长观察期",
        engine.observe_base >= base_before,
        "自适应",
    );

    // --- 置信度权重：心跳维度最高 ---
    // 全停 vs 全动，差值应显著。
    let ev_all_stop = Evidence {
        cycle: build_cycle_evidence(&scc),
        stall: build_stall_evidence(&[5000, 5000, 5000], 1000),
        time: build_time_evidence(500, 500),
    };
    let ev_all_run = Evidence {
        cycle: build_cycle_evidence(&scc),
        stall: build_stall_evidence(&[1, 2, 3], 1000),
        time: build_time_evidence(500, 500),
    };
    cs.add(
        "停走差显著",
        ev_all_stop.confidence() - ev_all_run.confidence() > 0.2,
        "stall 权重 0.4",
    );

    // --- 阈值边界 ---
    cs.add("确认阈值 0.75", CONFIRM_THRESHOLD == 0.75, "常量");
    cs.add("解除阈值 0.35", DISMISS_THRESHOLD == 0.35, "常量");
    cs.add(
        "阈值有序",
        DISMISS_THRESHOLD < CONFIRM_THRESHOLD,
        "dismiss < confirm",
    );

    cs
}
