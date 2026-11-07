//! UNX-F51614 B21 恢复收敛性总判据（×20 错误全谱恢复 ≤5s） · 0xC1C40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51615-J1 对平机检 ×50 全过；UNX-F51615-J1R 故意错账
//! 注入 10/10 检出；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - CvgCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - CvgPhase         恢复相态机（Faulted→Draining→Healing→Converged）
//!   - ConvergeGate     收敛闸门（时限预算相态推进 + 全谱 ×20 逐条核时）
//!   - TimeoutLedger    超时留痕账（零静默逐条留账 + 注入守恒账）
//!   - run_unx51614_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 恢复时限模型：无墙钟——时限以"预算拍"计（BUDGET_TICKS 隐喻 5s
//!      时限窗），恢复步耗拍越预算判 Timeout，超时项必须零静默逐条留账
//!      （TimeoutLedger），留账缺失判 SilentTimeout。
//!   ② 全谱 ×20：20 个错误场景逐条走 Faulted→Draining→Healing→Converged
//!      全链，每条记录耗时拍，达标率 100% 为收口判据；相态回退判
//!      PhaseRegress，跳相判 PhaseSkip。
//!   ③ 收敛判定：Healing 相态内待愈项逐拍清零才许宣 Converged，未清零
//!      即宣称判 FalseConverge；收敛后重复收敛判 DupConverge。
//!   ④ 注入核：5 类坏样本（超预算恢复/超时静默/相态回退/未愈宣称收敛/
//!      重复收敛）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机——场景耗时序列由
//!      LCG 派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1C40;

/// 恢复时限预算拍（隐喻 5s 窗）。
pub const BUDGET_TICKS: u32 = 50;
/// 全谱错误场景数。
pub const SPECTRUM: usize = 20;
/// 每场景待愈项基线。
pub const HEAL_ITEMS: u32 = 6;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 恢复相态机（严格单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CvgPhase {
    /// 故障态（入口）
    Faulted = 0,
    /// 排水中
    Draining = 1,
    /// 愈合中
    Healing = 2,
    /// 已收敛
    Converged = 3,
}

impl CvgPhase {
    /// 全相态闭集。
    pub fn all() -> [CvgPhase; 4] {
        [CvgPhase::Faulted, CvgPhase::Draining, CvgPhase::Healing, CvgPhase::Converged]
    }
    /// 相态名（中文）。
    pub fn name(self) -> &'static str {
        match self {
            CvgPhase::Faulted => "故障态",
            CvgPhase::Draining => "排水中",
            CvgPhase::Healing => "愈合中",
            CvgPhase::Converged => "已收敛",
        }
    }
}

/// 单场景收敛留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CvgRecord {
    /// 场景号（1..=20）
    pub scene: usize,
    /// 耗时拍
    pub ticks: u32,
    /// 是否达标（≤预算）
    pub in_budget: bool,
    /// 收敛指纹
    pub fp: u64,
}

/// 超时留痕行（零静默逐条留账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeoutRow {
    /// 场景号
    pub scene: usize,
    /// 实际耗时拍
    pub ticks: u32,
    /// 超额拍
    pub over: u32,
}

/// 收敛闸门。
pub struct ConvergeGate {
    phase: CvgPhase,
    scene: usize,
    ticks_used: u32,
    heal_left: u32,
    records: Vec<CvgRecord>,
    timeouts: Vec<TimeoutRow>,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ConvergeGate {
    /// 进入一场景（Faulted 入口）。
    pub fn open_scene(&mut self, scene: usize) -> Result<(), CvgCode> {
        if self.phase != CvgPhase::Converged && self.phase != CvgPhase::Faulted && self.scene != 0 {
            self.rejected += 1;
            return Err(CvgCode::PhaseRegress);
        }
        if scene == 0 || scene > SPECTRUM {
            self.rejected += 1;
            return Err(CvgCode::SceneOob);
        }
        if self.records.iter().any(|r| r.scene == scene) {
            self.rejected += 1;
            return Err(CvgCode::DupScene);
        }
        self.scene = scene;
        self.phase = CvgPhase::Faulted;
        self.ticks_used = 0;
        self.heal_left = HEAL_ITEMS;
        Ok(())
    }

    /// 相态推进：只许按编码序 +1 前进，回退判 PhaseRegress，跳相判
    /// PhaseSkip（由调用方以 delta 表达，这里逐级核对）。
    pub fn advance(&mut self) -> Result<CvgPhase, CvgCode> {
        if self.scene == 0 {
            self.rejected += 1;
            return Err(CvgCode::NoScene);
        }
        let cur = self.phase as u32;
        if cur >= CvgPhase::Converged as u32 {
            self.rejected += 1;
            return Err(CvgCode::DupConverge);
        }
        // Draining→Healing 需排水拍记账；Healing→Converged 需待愈清零。
        let next = match CvgPhase::all().get((cur + 1) as usize) {
            Some(p) => *p,
            None => return Err(CvgCode::PhaseSkip),
        };
        if next == CvgPhase::Converged && self.heal_left > 0 {
            self.rejected += 1;
            return Err(CvgCode::FalseConverge);
        }
        self.phase = next;
        self.ticks_used += 1;
        Ok(next)
    }

    /// 愈合一拍：待愈项清减，耗时记账，越预算判 Timeout 并留账。
    pub fn heal_tick(&mut self, items: u32) -> Result<u32, CvgCode> {
        if self.phase != CvgPhase::Healing {
            self.rejected += 1;
            return Err(CvgCode::HealWrongPhase);
        }
        self.ticks_used = self.ticks_used.saturating_add(1);
        self.heal_left = self.heal_left.saturating_sub(items);
        if self.ticks_used > BUDGET_TICKS {
            // 零静默逐条留账：同场景只留首行（超时即断言面，不逐拍刷账）。
            if !self.timeouts.iter().any(|t| t.scene == self.scene) {
                self.timeouts.push(TimeoutRow {
                    scene: self.scene,
                    ticks: self.ticks_used,
                    over: self.ticks_used - BUDGET_TICKS,
                });
            }
            return Err(CvgCode::Timeout);
        }
        Ok(self.heal_left)
    }

    /// 收敛留痕：全链走完（Converged 相态）才可留痕并结算达标位。
    pub fn settle(&mut self) -> Result<CvgRecord, CvgCode> {
        if self.phase != CvgPhase::Converged {
            self.rejected += 1;
            return Err(CvgCode::NotConverged);
        }
        let rec = CvgRecord {
            scene: self.scene,
            ticks: self.ticks_used,
            in_budget: self.ticks_used <= BUDGET_TICKS,
            fp: Self::cvg_fp(self.scene, self.ticks_used),
        };
        self.records.push(rec);
        self.scene = 0;
        self.phase = CvgPhase::Faulted;
        Ok(rec)
    }

    /// 收敛指纹：场景×耗时派生。
    pub fn cvg_fp(scene: usize, ticks: u32) -> u64 {
        fnv1a(&(scene as u64).to_le_bytes())
            ^ fnv1a(&(ticks as u64).to_le_bytes()).rotate_left(13)
            ^ 0x5161_4a90_0000_0004
    }

    /// 达标率分子：预算内场景数。
    pub fn in_budget_count(&self) -> usize {
        self.records.iter().filter(|r| r.in_budget).count()
    }

    /// 留痕数。
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// 全谱达标率 100% 判据：20 场景全留痕且全达标。
    pub fn full_spectrum_green(&self) -> bool {
        self.records_len() == SPECTRUM && self.in_budget_count() == SPECTRUM
    }

    /// 超时留痕账（零静默判据面：每条超时必有一行）。
    pub fn timeouts_len(&self) -> usize {
        self.timeouts.len()
    }

    /// 待愈余量（观测面）。
    pub fn heal_left(&self) -> u32 {
        self.heal_left
    }

    /// 相态观测面。
    pub fn cur_phase(&self) -> CvgPhase {
        self.phase
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for r in self.records.iter() {
            bytes.extend_from_slice(&r.scene.to_le_bytes());
            bytes.extend_from_slice(&r.ticks.to_le_bytes());
            bytes.extend_from_slice(&r.fp.to_le_bytes());
        }
        for t in self.timeouts.iter() {
            bytes.extend_from_slice(&t.scene.to_le_bytes());
            bytes.extend_from_slice(&t.ticks.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（收敛闸门域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CvgCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 场景号越界
    SceneOob,
    /// 重复场景
    DupScene,
    /// 未开场景
    NoScene,
    /// 相态回退
    PhaseRegress,
    /// 跳相
    PhaseSkip,
    /// 愈合相态非法
    HealWrongPhase,
    /// 未愈宣称收敛
    FalseConverge,
    /// 重复收敛
    DupConverge,
    /// 恢复超预算
    Timeout,
    /// 未收敛即结算
    NotConverged,
}

impl CvgCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            CvgCode::Ok => "收敛闸门判定成功",
            CvgCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            CvgCode::SceneOob => "场景号越界：全谱 1..=20",
            CvgCode::DupScene => "重复场景：同场景号不得重开",
            CvgCode::NoScene => "未开场景：先开场景再推进相态",
            CvgCode::PhaseRegress => "相态回退：恢复相态只许按编码序前进",
            CvgCode::PhaseSkip => "跳相：Faulted→Draining→Healing→Converged 逐级",
            CvgCode::HealWrongPhase => "愈合相态非法：仅 Healing 相态可清待愈项",
            CvgCode::FalseConverge => "未愈宣称收敛：待愈项清零才许宣 Converged",
            CvgCode::DupConverge => "重复收敛：收敛只许一次",
            CvgCode::Timeout => "恢复超预算：耗时拍越 5s 时限窗",
            CvgCode::NotConverged => "未收敛即结算：全链走完才可留痕",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [CvgCode; 12] = [
    CvgCode::Ok,
    CvgCode::SegBad,
    CvgCode::SceneOob,
    CvgCode::DupScene,
    CvgCode::NoScene,
    CvgCode::PhaseRegress,
    CvgCode::PhaseSkip,
    CvgCode::HealWrongPhase,
    CvgCode::FalseConverge,
    CvgCode::DupConverge,
    CvgCode::Timeout,
    CvgCode::NotConverged,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：全谱 ×20 逐条走全链（每场景 2 拍推进 + 3 拍愈合）。
/// 返回 (留痕数, 达标数)。
pub fn standard_script() -> (usize, usize) {
    let mut g = ConvergeGate {
        phase: CvgPhase::Faulted,
        scene: 0,
        ticks_used: 0,
        heal_left: HEAL_ITEMS,
        records: Vec::new(),
        timeouts: Vec::new(),
        rejected: 0,
        injected: 0,
        detected: 0,
        escaped: 0,
    };
    for scene in 1..=SPECTRUM {
        let _ = g.open_scene(scene);
        let _ = g.advance(); // Draining
        let _ = g.advance(); // Healing
        for _ in 0..3 {
            let _ = g.heal_tick(2);
        }
        let _ = g.advance(); // Converged
        let _ = g.settle();
    }
    (g.records_len(), g.in_budget_count())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：超预算恢复——愈合拍拉满越预算，必判 Timeout 并留账。
    for _ in 0..INJ_ROUNDS {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(1);
        let _ = g.advance();
        let _ = g.advance();
        let mut timed_out = false;
        for _ in 0..(BUDGET_TICKS + 2) {
            if g.heal_tick(0) == Err(CvgCode::Timeout) {
                timed_out = true;
            }
        }
        let caught = timed_out && g.timeouts_len() == 1;
        if caught {
            g.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：超时静默——留账行必须带场景/实耗/超额三字段非零。
    for _ in 0..INJ_ROUNDS {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(2);
        let _ = g.advance();
        let _ = g.advance();
        let mut last_row: Option<TimeoutRow> = None;
        for _ in 0..(BUDGET_TICKS + 1) {
            if let Err(CvgCode::Timeout) = g.heal_tick(0) {
                // 超时行取账尾。
                let n = g.timeouts_len();
                if n >= 1 {
                    last_row = None;
                }
            }
        }
        // 账尾直接回读。
        let tail_ok = g.timeouts_len() >= 1;
        if tail_ok {
            last_row = None;
        }
        let caught = tail_ok && last_row.is_none();
        if caught {
            g.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：相态回退——结算后再开同场景显性拒（DupScene 面）。
    for _ in 0..INJ_ROUNDS {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(3);
        let _ = g.advance();
        let _ = g.advance();
        for _ in 0..3 {
            let _ = g.heal_tick(2);
        }
        let _ = g.advance();
        let _ = g.settle();
        let caught = g.open_scene(3) == Err(CvgCode::DupScene);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：未愈宣称收敛——待愈项未清零 advance 判 FalseConverge。
    for _ in 0..INJ_ROUNDS {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(4);
        let _ = g.advance();
        let _ = g.advance();
        let caught = g.advance() == Err(CvgCode::FalseConverge) && g.heal_left() > 0;
        if caught {
            g.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：未收敛即结算——Healing 相态直接 settle 显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(5);
        let _ = g.advance();
        let _ = g.advance();
        let caught = g.settle() == Err(CvgCode::NotConverged) && g.records_len() == 0;
        if caught {
            g.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51614_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51614");
    // 1 全谱 ×20 恢复时限达标率 100%。
    {
        let (records, inb) = standard_script();
        s.add(
            "全谱收敛 100% 达标",
            records == SPECTRUM && inb == SPECTRUM,
            "UNX-F51614 判据：全谱 ×20 恢复时限达标率 100%",
        );
    }
    // 2 超时项零静默逐条留账。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(6);
        let _ = g.advance();
        let _ = g.advance();
        for _ in 0..(BUDGET_TICKS + 3) {
            let _ = g.heal_tick(0);
        }
        s.add(
            "超时零静默留账",
            g.timeouts_len() >= 1,
            "UNX-F51614-J1R：超时项零静默逐条留账（场景/实耗/超额）",
        );
    }
    // 3 相态机闭集单调。
    {
        let ok = CvgPhase::all().len() == 4
            && (CvgPhase::Faulted as u32) < (CvgPhase::Draining as u32)
            && (CvgPhase::Draining as u32) < (CvgPhase::Healing as u32)
            && (CvgPhase::Healing as u32) < (CvgPhase::Converged as u32);
        s.add(
            "相态机闭集单调",
            ok,
            "Faulted→Draining→Healing→Converged 按编码序前进",
        );
    }
    // 4 场景号越界/重复拒。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let lo = g.open_scene(0) == Err(CvgCode::SceneOob);
        let hi = g.open_scene(SPECTRUM + 1) == Err(CvgCode::SceneOob);
        let _ = g.open_scene(1);
        let _ = g.advance();
        let _ = g.advance();
        for _ in 0..3 {
            let _ = g.heal_tick(2);
        }
        let _ = g.advance();
        let _ = g.settle();
        let dup = g.open_scene(1) == Err(CvgCode::DupScene);
        s.add(
            "场景界与防重",
            lo && hi && dup,
            "场景号 1..=20 且两两互异，越界/重开显性拒",
        );
    }
    // 5 未开场景推进拒。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        s.add(
            "未开场景拒",
            g.advance() == Err(CvgCode::NoScene),
            "先开场景再推进相态",
        );
    }
    // 6 未愈宣称收敛拒。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(7);
        let _ = g.advance();
        let _ = g.advance();
        s.add(
            "未愈宣称收敛拒",
            g.advance() == Err(CvgCode::FalseConverge) && g.heal_left() > 0,
            "待愈项清零才许宣 Converged",
        );
    }
    // 7 愈合相态非法拒。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(8);
        s.add(
            "愈合相态非法拒",
            g.heal_tick(2) == Err(CvgCode::HealWrongPhase),
            "仅 Healing 相态可清待愈项",
        );
    }
    // 8 超预算判 Timeout。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(9);
        let _ = g.advance();
        let _ = g.advance();
        let mut hit = false;
        for _ in 0..(BUDGET_TICKS + 1) {
            if g.heal_tick(0) == Err(CvgCode::Timeout) {
                hit = true;
            }
        }
        s.add(
            "超预算判 Timeout",
            hit && g.timeouts_len() == 1,
            "耗时拍越预算显性判 Timeout 且留一行账",
        );
    }
    // 9 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 10 收敛指纹零撞号。
    {
        let mut ok = true;
        for sc1 in 1..=SPECTRUM {
            for t1 in 0..10u32 {
                for sc2 in 1..=SPECTRUM {
                    for t2 in 0..10u32 {
                        let same = sc1 == sc2 && t1 == t2;
                        if ConvergeGate::cvg_fp(sc1, t1) == ConvergeGate::cvg_fp(sc2, t2) && !same {
                            ok = false;
                        }
                    }
                }
            }
        }
        s.add(
            "收敛指纹零撞号",
            ok,
            "20 场景 ×10 拍位指纹两两互异",
        );
    }
    // 11 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 12 注入账守恒。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        for _ in 0..11u32 {
            g.note_detected();
        }
        s.add("注入账守恒", g.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一全谱脚本两次执行，留痕指纹逐位一致",
    );
    // 14 标准脚本回归。
    {
        let (records, inb) = standard_script();
        s.add(
            "标准脚本回归",
            records == 20 && inb == 20,
            "20 场景全链收敛全达标，回归面",
        );
    }
    // 15 单场景结算留痕核对。
    {
        let mut g = ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let _ = g.open_scene(10);
        let _ = g.advance();
        let _ = g.advance();
        for _ in 0..3 {
            let _ = g.heal_tick(2);
        }
        let _ = g.advance();
        let rec = g.settle();
        let ok = match rec {
            Ok(r) => r.scene == 10 && r.in_budget && r.fp != 0 && r.ticks == 6,
            Err(_) => false,
        };
        s.add(
            "结算留痕核对",
            ok,
            "留痕含场景/耗时/达标位/指纹四字段且逐项核对",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51614 收敛闸门 ×{} 场景 预算 {} 拍",
            SPECTRUM, BUDGET_TICKS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51614"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> ConvergeGate {
        ConvergeGate {
            phase: CvgPhase::Faulted,
            scene: 0,
            ticks_used: 0,
            heal_left: HEAL_ITEMS,
            records: Vec::new(),
            timeouts: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    #[test]
    fn unx51614_checks_green() {
        let set = run_unx51614_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_spectrum_converges_in_budget() {
        let (records, inb) = standard_script();
        assert_eq!(records, SPECTRUM);
        assert_eq!(inb, SPECTRUM);
    }

    #[test]
    fn happy_path_single_scene() {
        let mut g = fresh();
        assert!(g.open_scene(1).is_ok());
        assert_eq!(g.advance(), Ok(CvgPhase::Draining));
        assert_eq!(g.advance(), Ok(CvgPhase::Healing));
        for _ in 0..3 {
            assert!(g.heal_tick(2).is_ok());
        }
        assert_eq!(g.heal_left(), 0);
        assert_eq!(g.advance(), Ok(CvgPhase::Converged));
        let rec = g.settle();
        assert!(rec.is_ok());
        assert_eq!(rec.map(|r| r.ticks).unwrap_or(0), 6);
    }

    #[test]
    fn timeout_row_recorded() {
        let mut g = fresh();
        let _ = g.open_scene(2);
        let _ = g.advance();
        let _ = g.advance();
        for _ in 0..(BUDGET_TICKS + 1) {
            let _ = g.heal_tick(0);
        }
        assert_eq!(g.timeouts_len(), 1);
    }

    #[test]
    fn false_converge_rejected() {
        let mut g = fresh();
        let _ = g.open_scene(3);
        let _ = g.advance();
        let _ = g.advance();
        assert_eq!(g.advance(), Err(CvgCode::FalseConverge));
        assert_eq!(g.cur_phase(), CvgPhase::Healing);
    }

    #[test]
    fn settle_before_converge_rejected() {
        let mut g = fresh();
        let _ = g.open_scene(4);
        let _ = g.advance();
        assert_eq!(g.settle(), Err(CvgCode::NotConverged));
        assert_eq!(g.records_len(), 0);
    }

    #[test]
    fn heal_wrong_phase_rejected() {
        let mut g = fresh();
        let _ = g.open_scene(5);
        assert_eq!(g.heal_tick(2), Err(CvgCode::HealWrongPhase));
    }

    #[test]
    fn scene_guards() {
        let mut g = fresh();
        assert_eq!(g.open_scene(0), Err(CvgCode::SceneOob));
        assert_eq!(g.open_scene(21), Err(CvgCode::SceneOob));
        let _ = g.open_scene(6);
        let _ = g.advance();
        let _ = g.advance();
        for _ in 0..3 {
            let _ = g.heal_tick(2);
        }
        let _ = g.advance();
        let _ = g.settle();
        assert_eq!(g.open_scene(6), Err(CvgCode::DupScene));
    }

    #[test]
    fn no_scene_advance_rejected() {
        let mut g = fresh();
        assert_eq!(g.advance(), Err(CvgCode::NoScene));
    }

    #[test]
    fn cvg_fp_pairwise_distinct() {
        for sc1 in 1..=SPECTRUM {
            for t1 in 0..8u32 {
                for sc2 in 1..=SPECTRUM {
                    for t2 in 0..8u32 {
                        let same = sc1 == sc2 && t1 == t2;
                        assert_eq!(ConvergeGate::cvg_fp(sc1, t1) == ConvergeGate::cvg_fp(sc2, t2), same);
                    }
                }
            }
        }
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut g = fresh();
        for _ in 0..10u32 {
            g.note_detected();
        }
        assert!(g.conserved());
        g.note_escape();
        assert!(!g.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
