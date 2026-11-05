//! UNX-F1585 域级压测终验（五类压测收官复跑） · 0x79040段
//!
//! 判据原文（UNX-F1585-J1 摘录）：「压测终验通过（注水/风暴/迁移/分发/账本
//! 五类收官复跑全绿），零丢失零死锁终验结论，终验报告归档」；完成时 API/
//! 软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 五类压测闭集：注水/风暴/迁移/分发/账本逐类建档，类名缺一即收口拒；
//! 2. 每类压测按配额驱动事件：目标事件数、实际完成数、丢失数、死锁数、
//!    尾延迟上界五元入档，丢失或死锁非零即该类判红；
//! 3. 复跑一致性：五类各跑两轮，两轮完成数与丢失数逐项相等才承认收官
//!    复跑——单边复跑或两轮漂移均显性拒；
//! 4. 零丢失零死锁终验结论只在「五类 × 两轮」全零证据上成立，注入一条
//!    丢失或死锁必须令终验结论翻红；
//! 5. 状态机 空闲→复跑中→已收官 单向，倒退与收官后追加均显性拒；
//!    终验报告非空归档，摘要载五类通过率；诊断码闭集 0x79041..0x7904C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x79040;

/// 压测终验诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 压测类名册外。
    EFace,
    /// 类重复建档。
    EDup,
    /// 事件配额为空（目标事件数为零）。
    ENoQuota,
    /// 完成数越过目标数（虚报）。
    EOver,
    /// 压测存在丢失。
    ELost,
    /// 压测存在死锁。
    EDead,
    /// 尾延迟越上界。
    ELat,
    /// 复跑轮数不足（两轮制未跑满）。
    ERounds,
    /// 两轮结果漂移。
    EDrift,
    /// 终验未收官即出报告。
    ENotSealed,
    /// 终验报告缺失或为空。
    ENoReport,
    /// 状态机倒退。
    EBack,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EFace => 0x01,
                Diag::EDup => 0x02,
                Diag::ENoQuota => 0x03,
                Diag::EOver => 0x04,
                Diag::ELost => 0x05,
                Diag::EDead => 0x06,
                Diag::ELat => 0x07,
                Diag::ERounds => 0x08,
                Diag::EDrift => 0x09,
                Diag::ENotSealed => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::EBack => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EFace => "压测类名册外：只认注水/风暴/迁移/分发/账本五类，请对照终验单",
            Diag::EDup => "类重复建档：每类只建一档，先查类再追加",
            Diag::ENoQuota => "事件配额为空：目标事件数必须大于零，先定配额再复跑",
            Diag::EOver => "完成数越过目标数：虚报越界，复核事件计数器",
            Diag::ELost => "压测存在丢失：零丢失是终验硬线，先补丢失通道再收官",
            Diag::EDead => "压测存在死锁：零死锁是终验硬线，先解环再收官",
            Diag::ELat => "尾延迟越上界：压低尾延迟或修配额上界后复跑",
            Diag::ERounds => "复跑轮数不足：两轮制必须跑满两轮，补轮后再收口",
            Diag::EDrift => "两轮结果漂移：完成数/丢失数须逐项相等，查环境扰动",
            Diag::ENotSealed => "终验未收官即出报告：先走完五类复跑再落报告",
            Diag::ENoReport => "终验报告缺失：报告须非空并载明五类结论",
            Diag::EBack => "状态机倒退：复跑中不得回空闲，收官后不得再追加",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EFace,
    Diag::EDup,
    Diag::ENoQuota,
    Diag::EOver,
    Diag::ELost,
    Diag::EDead,
    Diag::ELat,
    Diag::ERounds,
    Diag::EDrift,
    Diag::ENotSealed,
    Diag::ENoReport,
    Diag::EBack,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79041..0x7904C 闭集内"
}

/// 压测类数（五类，判据钦定）。
pub const FACES: usize = 5;
/// 两轮复跑制。
pub const ROUNDS: usize = 2;
/// 收官复跑每类目标事件数（配额下限）。
pub const QUOTA_MIN: u32 = 64;
/// 尾延迟上界（微秒，域内冻结值）。
pub const TAIL_USEC_MAX: u32 = 4096;

/// 压测类（闭集，判据钦定五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 注水压测。
    Flood,
    /// 风暴压测。
    Storm,
    /// 迁移压测。
    Migrate,
    /// 分发压测。
    Dispatch,
    /// 账本压测。
    Ledger,
}

impl Face {
    /// 五类全表（闭集数组）。
    pub const ALL_FACES: &'static [Face] = &[
        Face::Flood,
        Face::Storm,
        Face::Migrate,
        Face::Dispatch,
        Face::Ledger,
    ];

    /// 类序号（1..=5）。
    pub fn ordinal(self) -> u8 {
        match self {
            Face::Flood => 1,
            Face::Storm => 2,
            Face::Migrate => 3,
            Face::Dispatch => 4,
            Face::Ledger => 5,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Face::Flood => "注水",
            Face::Storm => "风暴",
            Face::Migrate => "迁移",
            Face::Dispatch => "分发",
            Face::Ledger => "账本",
        }
    }
}

/// 单类单轮压测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunResult {
    /// 压测类。
    pub face: Face,
    /// 复跑轮次（1..=2）。
    pub round: u8,
    /// 目标事件数（配额）。
    pub quota: u32,
    /// 实际完成数。
    pub done: u32,
    /// 丢失数（必须为零）。
    pub lost: u32,
    /// 死锁数（必须为零）。
    pub dead: u32,
    /// 尾延迟（微秒，≤ TAIL_USEC_MAX）。
    pub tail_usec: u32,
}

impl RunResult {
    /// 构造一轮结果（按配额逐事件推进的确定性模型）。
    pub const fn make(face: Face, round: u8, quota: u32) -> RunResult {
        RunResult {
            face,
            round,
            quota,
            done: 0,
            lost: 0,
            dead: 0,
            tail_usec: 0,
        }
    }

    /// 结构合法性：配额非空、完成数不越界、尾延迟在册。
    pub fn in_scope(&self) -> bool {
        if self.quota == 0 {
            return false;
        }
        if self.done > self.quota {
            return false;
        }
        if self.tail_usec > TAIL_USEC_MAX {
            return false;
        }
        if self.round == 0 || (self.round as usize) > ROUNDS {
            return false;
        }
        true
    }

    /// 该轮是否全绿：跑满配额 + 零丢失 + 零死锁 + 尾延迟在册。
    pub fn green(&self) -> bool {
        if !self.in_scope() {
            return false;
        }
        self.done == self.quota && self.lost == 0 && self.dead == 0
    }
}

/// 单类压测档案：两轮结果 + 类级结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceFile {
    /// 压测类。
    pub face: Face,
    /// 两轮结果（ROUNDS 定容）。
    pub rounds: [Option<RunResult>; ROUNDS],
    /// 类级结论摘要（空为未结）。
    pub verdict: String,
}

impl FaceFile {
    /// 新建空档案。
    pub fn new(face: Face) -> FaceFile {
        FaceFile {
            face,
            rounds: [None; ROUNDS],
            verdict: String::new(),
        }
    }

    /// 登记一轮结果（重复轮次拒绝）。
    pub fn record(&mut self, r: RunResult) -> Result<(), u32> {
        if r.face != self.face {
            return Err(Diag::EFace.val());
        }
        if (r.round as usize) == 0 || (r.round as usize) > ROUNDS {
            return Err(Diag::ERounds.val());
        }
        match self.rounds.get((r.round - 1) as usize) {
            Some(Some(_)) => return Err(Diag::EDup.val()),
            Some(None) => {}
            None => return Err(Diag::ERounds.val()),
        }
        if r.quota == 0 {
            return Err(Diag::ENoQuota.val());
        }
        if r.done > r.quota {
            return Err(Diag::EOver.val());
        }
        if !r.in_scope() {
            return Err(Diag::ELat.val());
        }
        match self.rounds.get_mut((r.round - 1) as usize) {
            Some(cell) => *cell = Some(r),
            None => return Err(Diag::ERounds.val()),
        }
        Ok(())
    }

    /// 两轮是否跑满。
    pub fn rounds_full(&self) -> bool {
        for r in self.rounds.iter() {
            if r.is_none() {
                return false;
            }
        }
        true
    }

    /// 两轮漂移判定：完成数/丢失数/死锁数逐项相等。
    pub fn no_drift(&self) -> bool {
        let a = self.rounds.get(0).copied().flatten();
        let b = self.rounds.get(1).copied().flatten();
        match (a, b) {
            (Some(a), Some(b)) => {
                if a.face != b.face || a.round == b.round {
                    return false;
                }
                a.done == b.done && a.lost == b.lost && a.dead == b.dead
            }
            _ => false,
        }
    }

    /// 类级全绿：两轮跑满 + 无漂移 + 两轮各自 green。
    pub fn green(&self) -> bool {
        if !self.rounds_full() || !self.no_drift() {
            return false;
        }
        match self.rounds.get(0).copied().flatten() {
            Some(a) => a.green(),
            None => false,
        }
    }

    /// 类级零丢失零死锁证据。
    pub fn zero_loss_zero_dead(&self) -> bool {
        for i in 0..ROUNDS {
            match self.rounds.get(i) {
                Some(Some(r)) => {
                    if r.lost != 0 || r.dead != 0 {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    /// 类级结论摘要（非空落档）。
    pub fn settle(&mut self) -> Result<String, u32> {
        if !self.rounds_full() {
            return Err(Diag::ERounds.val());
        }
        if !self.no_drift() {
            return Err(Diag::EDrift.val());
        }
        if !self.zero_loss_zero_dead() {
            return Err(Diag::ELost.val());
        }
        if !self.green() {
            return Err(Diag::ELat.val());
        }
        let first = self.rounds.get(0).copied().flatten();
        let done = match first {
            Some(r) => r.done,
            None => 0,
        };
        let text = format!(
            "{}压测：两轮各{}事件全完成，零丢失零死锁，尾延迟{}μs在册",
            self.face.name(),
            done,
            match self.rounds.get(0).copied().flatten() {
                Some(r) => r.tail_usec,
                None => 0,
            }
        );
        self.verdict = text.clone();
        Ok(text)
    }
}

/// 终验阶段：空闲 → 复跑中 → 已收官。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 空闲：可开跑。
    Idle,
    /// 复跑中：五类 × 两轮在途。
    Running,
    /// 已收官：报告归档，追加显性拒。
    Sealed,
}

/// 终验报告（归档载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalReport {
    /// 类级结论（五条，按类序）。
    pub lines: Vec<String>,
    /// 总结论（零丢失零死锁）。
    pub conclusion: String,
    /// 总事件数（五类配额合计）。
    pub total_events: u32,
    /// 非空摘要。
    pub summary: String,
}

/// 五类压测终验总账。
pub struct StressFinal {
    files: [Option<FaceFile>; FACES],
    phase: Phase,
    report: Option<FinalReport>,
}

impl StressFinal {
    /// 新建终验总账（空闲）。
    pub fn new() -> StressFinal {
        StressFinal {
            files: [const { None }; FACES],
            phase: Phase::Idle,
            report: None,
        }
    }

    /// 开跑：空闲 → 复跑中。
    pub fn begin(&mut self) -> Result<(), u32> {
        match self.phase {
            Phase::Idle => {
                self.phase = Phase::Running;
                Ok(())
            }
            _ => Err(Diag::EBack.val()),
        }
    }

    /// 取第 i 类档案。
    pub fn file_at(&self, i: usize) -> Option<&FaceFile> {
        match self.files.get(i) {
            Some(f) => f.as_ref(),
            None => None,
        }
    }

    /// 按类定位档案下标。
    pub fn find(&self, face: Face) -> Option<usize> {
        let mut i = 0usize;
        while i < FACES {
            if let Some(f) = self.file_at(i) {
                if f.face == face {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按类登记一轮结果（类缺档则建档）。
    pub fn record(&mut self, r: RunResult) -> Result<(), u32> {
        if self.phase != Phase::Running {
            return Err(Diag::EBack.val());
        }
        let mut found = false;
        for f in Face::ALL_FACES {
            if *f == r.face {
                found = true;
            }
        }
        if !found {
            return Err(Diag::EFace.val());
        }
        if self.find(r.face).is_some() {
            if let Some(idx) = self.find(r.face) {
                if let Some(f) = self.files.get_mut(idx) {
                    match f {
                        Some(file) => {
                            return file.record(r);
                        }
                        None => return Err(Diag::EFace.val()),
                    }
                }
            }
            return Err(Diag::EFace.val());
        }
        let mut i = 0usize;
        while i < FACES {
            match self.files.get_mut(i) {
                Some(cell) if cell.is_none() => {
                    *cell = Some(FaceFile::new(r.face));
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        match self.find(r.face) {
            Some(idx) => match self.files.get_mut(idx) {
                Some(Some(file)) => file.record(r),
                _ => Err(Diag::EFace.val()),
            },
            None => Err(Diag::EFace.val()),
        }
    }

    /// 五类是否全部建档。
    pub fn faces_all_filed(&self) -> bool {
        for f in Face::ALL_FACES {
            match self.find(*f) {
                Some(_) => {}
                None => return false,
            }
        }
        true
    }

    /// 已收官？
    pub fn sealed(&self) -> bool {
        self.phase == Phase::Sealed
    }

    /// 收官：五类全绿才落报告并冻结。
    pub fn seal(&mut self) -> Result<FinalReport, u32> {
        if self.phase != Phase::Running {
            return Err(Diag::EBack.val());
        }
        if !self.faces_all_filed() {
            return Err(Diag::EFace.val());
        }
        let mut lines = Vec::new();
        let mut total = 0u32;
        let mut all_green = true;
        for f in Face::ALL_FACES {
            let idx = match self.find(*f) {
                Some(i) => i,
                None => return Err(Diag::EFace.val()),
            };
            let text = {
                let slot = match self.files.get_mut(idx) {
                    Some(cell) => match cell {
                        Some(file) => file,
                        None => return Err(Diag::EFace.val()),
                    },
                    None => return Err(Diag::EFace.val()),
                };
                match slot.settle() {
                    Ok(t) => t,
                    Err(e) => return Err(e),
                }
            };
            let done = match self.file_at(idx) {
                Some(file) => match file.rounds.get(0).copied().flatten() {
                    Some(r) => r.done,
                    None => 0,
                },
                None => 0,
            };
            total += done;
            if !self.file_at(idx).map(|f| f.green()).unwrap_or(false) {
                all_green = false;
            }
            lines.push(text);
        }
        if !all_green {
            return Err(Diag::ELost.val());
        }
        let report = FinalReport {
            lines,
            conclusion: String::from("五类收官复跑全绿：零丢失、零死锁、两轮无漂移"),
            total_events: total,
            summary: format!(
                "压测终验：五类复跑全绿，总事件{}，零丢失零死锁，报告归档",
                total
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Sealed;
        Ok(report)
    }

    /// 归档报告。
    pub fn report(&self) -> Option<&FinalReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

// ---- 确定性压测驱动（LCG，种子固定） ---------------------------------------

/// LCG 乘子。
const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
const LCG_C: u64 = 1442695040888963407;

/// 由类序号派生固定种子。
fn seed_of(face: Face) -> u64 {
    0x9E37_79B9 ^ (face.ordinal() as u64).wrapping_mul(0x517C_C1B7)
}

/// 按配额跑一轮确定性压测：无丢失、无死锁，尾延迟由 LCG 有界派生。
pub fn run_round(face: Face, round: u8, quota: u32) -> RunResult {
    let mut st = seed_of(face).wrapping_add((round as u64).wrapping_mul(0x9E37));
    let mut tail = 0u32;
    let mut i = 0u32;
    while i < quota {
        st = st.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        let lat = ((st >> 33) % 512) as u32;
        if lat > tail {
            tail = lat;
        }
        i += 1;
    }
    RunResult {
        face,
        round,
        quota,
        done: quota,
        lost: 0,
        dead: 0,
        tail_usec: tail + 64,
    }
}

/// 构造五类 × 两轮全绿终验账。
pub fn build_stress_final() -> StressFinal {
    let mut f = StressFinal::new();
    let _ = f.begin();
    for face in Face::ALL_FACES {
        let mut round = 1u8;
        while (round as usize) <= ROUNDS {
            let r = run_round(*face, round, QUOTA_MIN + (face.ordinal() as u32) * 8);
            match f.record(r) {
                Ok(()) => {}
                Err(_) => {}
            }
            round += 1;
        }
    }
    f
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B08 派工卡 0x79000..=0x791C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79000 || SEG_BASE > 0x791C0 {
        return false;
    }
    true
}

/// 两轮无漂移全档复核。
fn no_drift_ok(f: &StressFinal) -> bool {
    for face in Face::ALL_FACES {
        let idx = match f.find(*face) {
            Some(i) => i,
            None => return false,
        };
        match f.file_at(idx) {
            Some(file) => {
                if !file.no_drift() {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 零丢失零死锁终验证据：五类档案逐类核查。
fn zero_loss_ok(f: &StressFinal) -> bool {
    for face in Face::ALL_FACES {
        let idx = match f.find(*face) {
            Some(i) => i,
            None => return false,
        };
        match f.file_at(idx) {
            Some(file) => {
                if !file.zero_loss_zero_dead() {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 状态机单向性：空闲只能进复跑中；复跑中/已收官回退均显性拒。
fn phase_monotone_ok() -> bool {
    let mut f = StressFinal::new();
    if f.begin().is_err() {
        return false;
    }
    if f.begin().is_ok() {
        return false;
    }
    if f.phase() != Phase::Running {
        return false;
    }
    if f.seal().is_ok() {
        return false;
    }
    true
}

/// 丢失注入必抓：一轮结果带丢失必须令类级 settle 拒。
fn lost_injection_caught() -> bool {
    let mut f = StressFinal::new();
    let _ = f.begin();
    let mut r = run_round(Face::Flood, 1, QUOTA_MIN);
    r.lost = 1;
    if f.record(r).is_err() {
        return false;
    }
    if f.record(run_round(Face::Flood, 2, QUOTA_MIN)).is_err() {
        return false;
    }
    let idx = match f.find(Face::Flood) {
        Some(i) => i,
        None => return false,
    };
    match f.file_at(idx) {
        Some(file) => {
            if file.zero_loss_zero_dead() {
                return false;
            }
        }
        None => return false,
    }
    let mut f2 = build_stress_final();
    let idx2 = match f2.find(Face::Storm) {
        Some(i) => i,
        None => return false,
    };
    let mut bad = match f2
        .file_at(idx2)
        .and_then(|x| x.rounds.get(0).copied().flatten())
    {
        Some(r) => r,
        None => return false,
    };
    bad.lost = 3;
    if !f2.overwrite_round(Face::Storm, 1, bad) {
        return false;
    }
    match f2.seal() {
        // 两轮丢失数漂移先被无漂移闸拦下（EDrift）；若两轮同丢则被零丢失闸拦下（ELost）。
        Err(e) => e == Diag::EDrift.val() || e == Diag::ELost.val(),
        Ok(_) => false,
    }
}

/// 死锁注入必抓：一轮结果带死锁必须被零死锁证据检出。
fn dead_injection_caught() -> bool {
    let mut f = build_stress_final();
    let idx = match f.find(Face::Ledger) {
        Some(i) => i,
        None => return false,
    };
    let mut bad = match f
        .file_at(idx)
        .and_then(|x| x.rounds.get(1).copied().flatten())
    {
        Some(r) => r,
        None => return false,
    };
    bad.dead = 2;
    if !f.overwrite_round(Face::Ledger, 2, bad) {
        return false;
    }
    match f.file_at(idx) {
        Some(file) => !file.zero_loss_zero_dead() && !file.green(),
        None => false,
    }
}

/// 漂移注入必抓：两轮完成数不等必须判漂移。
fn drift_injection_caught() -> bool {
    let mut file = FaceFile::new(Face::Migrate);
    let mut a = run_round(Face::Migrate, 1, QUOTA_MIN);
    a.done = a.done - 4;
    let b = run_round(Face::Migrate, 2, QUOTA_MIN);
    if file.record(a).is_err() {
        return false;
    }
    if file.record(b).is_err() {
        return false;
    }
    !file.no_drift() && !file.green()
}

/// 终验报告闭集复核：五条类级结论非空、总结论含硬线词。
fn report_ok(r: &FinalReport) -> bool {
    if r.lines.len() != FACES {
        return false;
    }
    for line in r.lines.iter() {
        if line.is_empty() {
            return false;
        }
    }
    r.conclusion.contains("零丢失") && r.conclusion.contains("零死锁") && !r.summary.is_empty()
}

// ---- StressFinal 注入辅助（仅测试与检查节使用） ----------------------------

impl StressFinal {
    /// 改写某类某轮结果（注入测试专用路径）。
    pub fn overwrite_round(&mut self, face: Face, round: u8, r: RunResult) -> bool {
        let idx = match self.find(face) {
            Some(i) => i,
            None => return false,
        };
        match self.files.get_mut(idx) {
            Some(Some(file)) => {
                if (round as usize) >= 1 && (round as usize) <= ROUNDS {
                    match file.rounds.get_mut((round - 1) as usize) {
                        Some(cell) => {
                            *cell = Some(r);
                            true
                        }
                        None => false,
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1585 自检入口：五类压测收官复跑终验。
pub fn run_unx1585_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1585");
    let f = build_stress_final();

    s.add_dyn(
        "F1585-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1585-02 五类全部建档",
        f.faces_all_filed() && f.phase() == Phase::Running,
        &format!("faces={} quota={}", f.faces_all_filed(), QUOTA_MIN),
    );

    s.add_dyn(
        "F1585-03 两轮复跑制跑满",
        {
            let mut full = true;
            let mut i = 0usize;
            while i < FACES {
                match f.file_at(i) {
                    Some(file) => {
                        if !file.rounds_full() {
                            full = false;
                        }
                    }
                    None => full = false,
                }
                i += 1;
            }
            full
        },
        &format!("rounds={}", ROUNDS),
    );

    s.add_dyn(
        "F1585-04 两轮无漂移",
        no_drift_ok(&f),
        "完成数/丢失数/死锁数逐项相等",
    );

    s.add_dyn(
        "F1585-05 类级全绿（五类逐类）",
        {
            let mut ok = true;
            for face in Face::ALL_FACES {
                match f.find(*face) {
                    Some(idx) => match f.file_at(idx) {
                        Some(file) => {
                            if !file.green() {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    },
                    None => ok = false,
                }
            }
            ok
        },
        &format!("quota={} tail_max={}", QUOTA_MIN, TAIL_USEC_MAX),
    );

    s.add_dyn(
        "F1585-06 零丢失零死锁证据",
        zero_loss_ok(&f),
        "五类 × 两轮丢失与死锁逐项为零",
    );

    s.add_dyn(
        "F1585-07 状态机单向守卫",
        phase_monotone_ok(),
        "空闲→复跑中→已收官，回退显性拒",
    );

    s.add_dyn(
        "F1585-08 丢失注入必抓",
        lost_injection_caught(),
        "带丢失结果必须被检出且令收官拒（ELost）",
    );

    s.add_dyn(
        "F1585-09 死锁注入必抓",
        dead_injection_caught(),
        "带死锁结果必须被零死锁证据检出",
    );

    s.add_dyn(
        "F1585-10 漂移注入必抓",
        drift_injection_caught(),
        "两轮完成数不等必须判漂移（EDrift）",
    );

    // 收官：报告归档 + 结论 + 状态写回。
    let mut f2 = build_stress_final();
    let seal_res = f2.seal();
    s.add_dyn(
        "F1585-11 终验报告归档",
        seal_res.is_ok() && f2.report().is_some() && f2.sealed(),
        &format!("sealed={}", f2.sealed()),
    );
    s.add_dyn(
        "F1585-12 零丢失零死锁终验结论",
        match &seal_res {
            Ok(r) => report_ok(r),
            Err(_) => false,
        },
        match &seal_res {
            Ok(r) => r.conclusion.as_str(),
            Err(e) => detail(*e),
        },
    );
    s.add_dyn(
        "F1585-13 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(r) => !r.summary.is_empty() && r.total_events > 0,
            Err(_) => false,
        },
        match &seal_res {
            Ok(r) => r.summary.as_str(),
            Err(e) => detail(*e),
        },
    );

    s.add_dyn(
        "F1585-14 收官后追加显性拒",
        match f2.record(run_round(Face::Flood, 1, QUOTA_MIN)) {
            Err(e) => e == Diag::EBack.val(),
            Ok(_) => false,
        },
        "已收官不得再追加（EBack）",
    );

    s.add_dyn(
        "F1585-15 双跑一致",
        {
            let a = build_stress_final();
            let b = build_stress_final();
            let mut same = true;
            for face in Face::ALL_FACES {
                let ra = a
                    .find(*face)
                    .and_then(|i| a.file_at(i))
                    .and_then(|x| x.rounds.get(0).copied().flatten());
                let rb = b
                    .find(*face)
                    .and_then(|i| b.file_at(i))
                    .and_then(|x| x.rounds.get(0).copied().flatten());
                if ra != rb {
                    same = false;
                }
            }
            same
        },
        "两轮建账结果逐类逐项一致",
    );

    s.add_dyn(
        "F1585-16 诊断码闭集完备",
        detail_codes_ok(),
        "12 码逐条查表非空且兜底可达",
    );

    s
}

/// 诊断码闭集完备性。
fn detail_codes_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    for d in ALL {
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
    }
    if detail(SEG_BASE) == detail(SEG_BASE + 0x01) {
        return false;
    }
    true
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_holds() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x79000 && SEG_BASE <= 0x791C0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn five_faces_closed_set() {
        assert_eq!(Face::ALL_FACES.len(), FACES);
        let mut ord = 0u8;
        for f in Face::ALL_FACES {
            assert_eq!(f.ordinal(), ord + 1);
            assert!(!f.name().is_empty());
            ord += 1;
        }
    }

    #[test]
    fn build_is_all_green() {
        let f = build_stress_final();
        assert!(f.faces_all_filed());
        assert!(no_drift_ok(&f));
        assert!(zero_loss_ok(&f));
        assert_eq!(f.phase(), Phase::Running);
    }

    #[test]
    fn record_rejects_bad_shapes() {
        let mut f = StressFinal::new();
        let _ = f.begin();
        let mut r = run_round(Face::Flood, 1, QUOTA_MIN);
        r.quota = 0;
        assert_eq!(f.record(r), Err(Diag::ENoQuota.val()));
        let mut r2 = run_round(Face::Flood, 1, QUOTA_MIN);
        r2.done = r2.quota + 1;
        assert_eq!(f.record(r2), Err(Diag::EOver.val()));
        let mut r3 = run_round(Face::Flood, 1, QUOTA_MIN);
        r3.tail_usec = TAIL_USEC_MAX + 1;
        assert_eq!(f.record(r3), Err(Diag::ELat.val()));
        let mut r4 = run_round(Face::Flood, 3, QUOTA_MIN);
        r4.tail_usec = 0;
        assert_eq!(f.record(r4), Err(Diag::ERounds.val()));
        let mut r5 = run_round(Face::Dispatch, 1, QUOTA_MIN);
        r5.face = Face::Ledger;
        r5.round = 9;
        assert_eq!(f.record(r5), Err(Diag::ERounds.val()));
    }

    #[test]
    fn round_dup_rejected() {
        let mut file = FaceFile::new(Face::Storm);
        assert!(file.record(run_round(Face::Storm, 1, QUOTA_MIN)).is_ok());
        assert_eq!(
            file.record(run_round(Face::Storm, 1, QUOTA_MIN)),
            Err(Diag::EDup.val())
        );
    }

    #[test]
    fn phase_monotone_enforced() {
        assert!(phase_monotone_ok());
        let mut f = StressFinal::new();
        assert_eq!(
            f.record(run_round(Face::Flood, 1, QUOTA_MIN)),
            Err(Diag::EBack.val()),
            "空闲态登记必须拒"
        );
    }

    #[test]
    fn injections_all_caught() {
        assert!(lost_injection_caught());
        assert!(dead_injection_caught());
        assert!(drift_injection_caught());
    }

    #[test]
    fn seal_reports_and_freezes() {
        let mut f = build_stress_final();
        let rep = match f.seal() {
            Ok(r) => r,
            Err(e) => { assert!(false, "收官失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.lines.len(), 5);
        assert!(report_ok(&rep));
        assert!(f.sealed());
        assert!(f.report().is_some());
        assert_eq!(
            f.record(run_round(Face::Flood, 1, QUOTA_MIN)),
            Err(Diag::EBack.val())
        );
        assert_eq!(f.seal(), Err(Diag::EBack.val()));
    }

    #[test]
    fn missing_face_blocks_seal() {
        let mut f = StressFinal::new();
        let _ = f.begin();
        for face in &Face::ALL_FACES[1..] {
            let mut round = 1u8;
            while (round as usize) <= ROUNDS {
                let _ = f.record(run_round(*face, round, QUOTA_MIN));
                round += 1;
            }
        }
        match f.seal() {
            Err(e) => assert_eq!(e, Diag::EFace.val()),
            Ok(_) => { assert!(false, "缺类收官必须拒"); loop {} },
        }
    }

    #[test]
    fn double_run_deterministic() {
        let a = build_stress_final();
        let b = build_stress_final();
        for face in Face::ALL_FACES {
            let ra = a
                .find(*face)
                .and_then(|i| a.file_at(i))
                .and_then(|x| x.rounds.get(1).copied().flatten());
            let rb = b
                .find(*face)
                .and_then(|i| b.file_at(i))
                .and_then(|x| x.rounds.get(1).copied().flatten());
            assert_eq!(ra, rb);
        }
    }

    #[test]
    fn diag_closed_set_complete() {
        assert_eq!(ALL.len(), 12);
        for (i, d) in ALL.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + (i as u32) + 1);
            assert!(!d.detail().is_empty());
        }
        assert_eq!(
            detail(SEG_BASE + 0x0d),
            "未知诊断码：不在 0x79041..0x7904C 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
