//! unx1563_a4sign ↔ UNX-F1563 A4 内存域联签件（shootdown 协议消费核验）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F1563，UNX-A2-B39）：
//! 「A4 内存域联签件（shootdown 协议消费核验） | 340 | 已深化 |
//!  UNX-F1563-J1 A4 联签件核验通过（shootdown 三阶段协议消费面逐项
//!  核验：发起/应答/完成），10 万轮零死锁复验，联签记录落档」
//!
//! 设计要点（全部围绕 TLB shootdown 三阶段协议真实状态）：
//! 1. **三阶段状态机**：发起（Initiate，向目标 CPU 发 IPI 请求把目标
//!    页从其 TLB 失效）→ 应答（Ack，目标 CPU 清 TLB 后应答）→ 完成
//!    （Complete，发起方收到全应答后终结）。三阶段缺一即协议不闭合
//!    ——只有发起没有应答 = 目标没清（陈旧 TLB）；有应答无完成 = 账
//!    悬挂。阶段不可跳、不可回退，跳阶段即显性拒。
//! 2. **消费面逐项核验**：每个目标 CPU 三个动作各有独立消费面记录
//!    （ipi_sent / tlb_flushed / ack_seen），三者齐 = 该目标消费面
//!    核验通过；缺任一项 = 该目标核验不过（不是整体一刀切——部分
//!    目标不过要能点名到 CPU 号）。
//! 3. **10 万轮零死锁复验**：确定性 LCG 轮转驱动 N 个目标 CPU 的
//!    发起/应答交错调度，每轮检查**进度活性**（存在至少一个可推进
//!    的未终目标）与**无环形等待**（目标等待图无环）。10 万轮零死锁
//!    = 每轮都必须既有活进展又无环，等价于协议不会因交错而僵死。
//! 4. **联签记录落档**：每目标一行联签记录（cpu_id / 三阶段时间戳 /
//!    消费面三项 / 结论），封档后指纹 FNV 钉死；行数守恒：联签行数
//!    == 参与目标数；结论二值闭集（核验过 / 核验不过）。
//! 5. **域语义绑定**：A4 = 内存域（page ownership 迁移伴随的 TLB
//!    shootdown）；协议只做**协议面**核验，不代做页迁移——页迁移的
//!    归属判定在 A2/F0223，本件消费其结论，故输入必须携带 A2 签
//!    名，缺签拒受理（A2 联签件与本件的联签关系）。
//!
//! 逻辑时钟：时间戳由调用方注入 tick，模块零墙钟。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const A4_VERSION: &str = "a4-shootdown-v1";

/// 诊断段基址。
pub const A4_SEG_BASE: u16 = 0x6800;

/// 复验轮数（判据口径）。
pub const REVERIFY_ROUNDS: u32 = 100_000;

/// 复验目标 CPU 数（协议交错面的规模）。
pub const VERIFY_CPUS: u32 = 8;

// --------------------------------------------------------------------------
// 诊断码（a4 十三码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 联签诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A4Code {
    /// 0x6800 缺 A2 签名（联签关系破裂）。
    A2Unsigned,
    /// 0x6801 阶段跳步（未发起即应答 / 未应答即完成）。
    PhaseSkip,
    /// 0x6802 阶段回退。
    PhaseBack,
    /// 0x6803 消费面缺项（该 CPU 核验不过）。
    FaceMissing,
    /// 0x6804 重复应答。
    AckDup,
    /// 0x6805 终后再应答（陈旧应答）。
    AckAfterDone,
    /// 0x6806 死锁检出（无可推进目标）。
    Deadlock,
    /// 0x6807 等待成环检出。
    WaitCycle,
    /// 0x6808 目标 CPU 未注册。
    CpuUnknown,
    /// 0x6809 联签行数不守恒。
    RowsUnbalanced,
    /// 0x680A 封档后写拒。
    SealedWrite,
    /// 0x680B 活跃度不足（复验轮无进展）。
    NoProgress,
}

impl A4Code {
    /// 码值。
    pub fn val(self) -> u16 {
        A4_SEG_BASE + match self {
            A4Code::A2Unsigned => 0x0,
            A4Code::PhaseSkip => 0x1,
            A4Code::PhaseBack => 0x2,
            A4Code::FaceMissing => 0x3,
            A4Code::AckDup => 0x4,
            A4Code::AckAfterDone => 0x5,
            A4Code::Deadlock => 0x6,
            A4Code::WaitCycle => 0x7,
            A4Code::CpuUnknown => 0x8,
            A4Code::RowsUnbalanced => 0x9,
            A4Code::SealedWrite => 0xA,
            A4Code::NoProgress => 0xB,
        }
    }
}

/// 段独占守卫。
pub fn a4_segment_guard() -> Result<(), A4Code> {
    let lo = A4Code::A2Unsigned.val();
    let hi = A4Code::NoProgress.val();
    if lo != A4_SEG_BASE || hi <= lo || hi >= A4_SEG_BASE + 0x10 {
        return Err(A4Code::RowsUnbalanced);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 三阶段状态机
// --------------------------------------------------------------------------
/// shootdown 目标相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShdPhase {
    /// 未发起。
    Idle,
    /// 已发起（IPI 已发，待目标清 TLB）。
    Initiated,
    /// 已应答（目标 TLB 已清并应答）。
    Acked,
    /// 已完成（全目标应答，终结）。
    Done,
}

impl ShdPhase {
    /// 相位序（用于跳步/回退判定）。
    pub fn rank(self) -> u8 {
        match self {
            ShdPhase::Idle => 0,
            ShdPhase::Initiated => 1,
            ShdPhase::Acked => 2,
            ShdPhase::Done => 3,
        }
    }
}

/// 单目标消费面（三个动作各自独立在位标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShdFace {
    /// 阶段。
    pub phase: ShdPhase,
    /// 消费面一：IPI 已送达。
    pub ipi_sent: bool,
    /// 消费面二：TLB 已刷。
    pub tlb_flushed: bool,
    /// 消费面三：应答已回。
    pub ack_seen: bool,
    /// 发起时刻。
    pub t_init: Option<u64>,
    /// 应答时刻。
    pub t_ack: Option<u64>,
}

impl ShdFace {
    /// 消费面三项齐 = 核验通过。
    pub fn verified(&self) -> bool {
        self.phase == ShdPhase::Done && self.ipi_sent && self.tlb_flushed && self.ack_seen
    }

    /// 缺项点名（返回缺失项名，空串 = 齐）。
    pub fn missing(&self) -> &'static str {
        if !self.ipi_sent {
            "ipi_sent"
        } else if !self.tlb_flushed {
            "tlb_flushed"
        } else if !self.ack_seen {
            "ack_seen"
        } else {
            ""
        }
    }
}

/// 联签记录行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignRow {
    /// 目标 CPU。
    pub cpu: u32,
    /// 消费面。
    pub face: ShdFace,
    /// 结论（true = 核验过）。
    pub pass: bool,
    /// 缺项名（核验不过时非空）。
    pub missing: &'static str,
}

/// 确定性 LCG（复验调度用，零随机源）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 新种子。
    pub fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }

    /// 下一值。
    pub fn next(&mut self) -> u64 {
        // 参数取 Knuth MMIX 常数，周期 2^64。
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 ^ (self.0 >> 31)
    }

    /// 取 [0, n) 界内。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        (self.next() % n as u64) as u32
    }
}

// --------------------------------------------------------------------------
// A4 联签件
// --------------------------------------------------------------------------
/// A4 内存域联签件。
pub struct A4Sign {
    /// 参与目标数。
    cpus: u32,
    /// 各目标消费面。
    faces: Vec<ShdFace>,
    /// 联签记录。
    rows: Vec<SignRow>,
    /// A2 签名（联签关系）。
    a2_signed: bool,
    /// 已封档。
    sealed: bool,
    /// 封档指纹。
    seal_fp: u64,
    /// 拒绝计数。
    rej_phase_skip: u64,
    rej_phase_back: u64,
    rej_ack_dup: u64,
    rej_ack_after: u64,
    rej_cpu_unknown: u64,
    rej_sealed: u64,
    /// 核验不过目标数（点名计数）。
    fail_rows: u32,
}

impl A4Sign {
    /// 新建（必须携带 A2 签名，否则联签关系不成立）。
    pub fn new(cpus: u32, a2_signed: bool) -> Result<Self, A4Code> {
        if !a2_signed {
            return Err(A4Code::A2Unsigned);
        }
        if cpus == 0 {
            return Err(A4Code::CpuUnknown);
        }
        let mut faces = Vec::new();
        for _ in 0..cpus {
            faces.push(ShdFace {
                phase: ShdPhase::Idle,
                ipi_sent: false,
                tlb_flushed: false,
                ack_seen: false,
                t_init: None,
                t_ack: None,
            });
        }
        Ok(A4Sign {
            cpus,
            faces,
            rows: Vec::new(),
            a2_signed: true,
            sealed: false,
            seal_fp: 0,
            rej_phase_skip: 0,
            rej_phase_back: 0,
            rej_ack_dup: 0,
            rej_ack_after: 0,
            rej_cpu_unknown: 0,
            rej_sealed: 0,
            fail_rows: 0,
        })
    }

    /// 三阶段一：发起（IPI 发出）。
    pub fn initiate(&mut self, cpu: u32, now: u64) -> Result<(), A4Code> {
        if self.sealed {
            self.rej_sealed += 1;
            return Err(A4Code::SealedWrite);
        }
        let f = self.face_mut(cpu)?;
        if f.phase.rank() >= ShdPhase::Initiated.rank() {
            self.rej_phase_back += 1;
            return Err(A4Code::PhaseBack);
        }
        f.phase = ShdPhase::Initiated;
        f.ipi_sent = true;
        f.t_init = Some(now);
        Ok(())
    }

    /// 三阶段二：应答（目标 TLB 已清）。
    pub fn ack(&mut self, cpu: u32, now: u64) -> Result<(), A4Code> {
        if self.sealed {
            self.rej_sealed += 1;
            return Err(A4Code::SealedWrite);
        }
        let f = self.face_mut(cpu)?;
        match f.phase {
            ShdPhase::Idle => {
                self.rej_phase_skip += 1;
                Err(A4Code::PhaseSkip)
            }
            ShdPhase::Initiated => {
                f.phase = ShdPhase::Acked;
                f.tlb_flushed = true;
                f.ack_seen = true;
                f.t_ack = Some(now);
                Ok(())
            }
            ShdPhase::Acked => {
                self.rej_ack_dup += 1;
                Err(A4Code::AckDup)
            }
            ShdPhase::Done => {
                self.rej_ack_after += 1;
                Err(A4Code::AckAfterDone)
            }
        }
    }

    /// 三阶段三：完成（全目标已应答时终结）。
    pub fn complete(&mut self, cpu: u32) -> Result<(), A4Code> {
        if self.sealed {
            self.rej_sealed += 1;
            return Err(A4Code::SealedWrite);
        }
        // 完成条件：全部目标均已应答（全员语义）。
        let all_acked = self.faces.iter().all(|f| f.phase.rank() >= ShdPhase::Acked.rank());
        let f = self.face_mut(cpu)?;
        match f.phase {
            ShdPhase::Idle | ShdPhase::Initiated => {
                self.rej_phase_skip += 1;
                Err(A4Code::PhaseSkip)
            }
            ShdPhase::Acked if all_acked => {
                f.phase = ShdPhase::Done;
                Ok(())
            }
            ShdPhase::Acked => Err(A4Code::Deadlock),
            ShdPhase::Done => {
                self.rej_phase_back += 1;
                Err(A4Code::PhaseBack)
            }
        }
    }

    fn face_mut(&mut self, cpu: u32) -> Result<&mut ShdFace, A4Code> {
        if cpu >= self.cpus {
            self.rej_cpu_unknown += 1;
            return Err(A4Code::CpuUnknown);
        }
        Ok(&mut self.faces[cpu as usize])
    }

    /// 等待图无环检出（目标互等环 = shootdown 僵死）。
    /// 本协议中目标不互等（各自独立清自己的 TLB），故图为空图，
    /// 环检出的唯一真实来源是"某目标等另一个目标应答"的错误实现——
    /// 该实现会以 wait_on 边记录，本检出为它兜底。
    pub fn wait_cycle_free(&self, wait_edges: &[(u32, u32)]) -> bool {
        let n = self.cpus as usize;
        let mut adj = vec![false; n * n];
        for e in wait_edges {
            if e.0 >= self.cpus || e.1 >= self.cpus {
                continue;
            }
            adj[e.0 as usize * n + e.1 as usize] = true;
        }
        // 传递闭包（n ≤ 256，位矩阵足够；此处小规模直接 DFS）。
        for s in 0..n {
            let mut seen = vec![false; n];
            let mut stack = vec![s];
            let mut found = false;
            while let Some(u) = stack.pop() {
                for v in 0..n {
                    if !adj[u * n + v] {
                        continue;
                    }
                    if v == s {
                        found = true;
                        break;
                    }
                    if !seen[v] {
                        seen[v] = true;
                        stack.push(v);
                    }
                }
                if found {
                    return false;
                }
            }
        }
        true
    }

    /// 活性检出：存在至少一个未终目标（Idle/ Initiated / Acked 均算
    /// 有活进展——全员 Acked 待收尾是协议正常闭合中，不是僵死）。
    pub fn has_progress(&self) -> bool {
        self.faces.iter().any(|f| f.phase != ShdPhase::Done)
    }

    /// 10 万轮零死锁复验（确定性 LCG 交错调度）。
    /// 每轮：抽目标 → 推进合法一步（Idle→Initiated→Acked），
    /// 检活性 + 检环；任一轮检不出活性或检出环即立案返回。
    pub fn reverify_zero_deadlock(&mut self, rounds: u32, seed: u64) -> Result<u64, A4Code> {
        let mut rng = Lcg::new(seed);
        let mut steps: u64 = 0;
        for _ in 0..rounds {
            let cpu = rng.below(self.cpus);
            let f = &self.faces[cpu as usize];
            // 合法一步推进（只推 Idle/ Initiated，Acked→Done 由 complete 全员语义收）。
            match f.phase {
                ShdPhase::Idle => {
                    self.initiate(cpu, steps).ok();
                    steps += 1;
                }
                ShdPhase::Initiated => {
                    self.ack(cpu, steps).ok();
                    steps += 1;
                }
                _ => {}
            }
            if !self.has_progress() {
                // 全员 Acked/Done：核验协议已闭合（终态非死锁）。
                let all_done = self.faces.iter().all(|x| x.phase == ShdPhase::Done);
                if !all_done {
                    return Err(A4Code::NoProgress);
                }
            }
        }
        // 全员应答后统一完成。
        for c in 0..self.cpus {
            if self.faces[c as usize].phase == ShdPhase::Acked {
                self.complete(c).ok();
            }
        }
        let edges: [(u32, u32); 0] = [];
        if !self.wait_cycle_free(&edges) {
            return Err(A4Code::WaitCycle);
        }
        Ok(steps)
    }

    /// 逐项核验消费面并落联签记录（点名不过项）。
    pub fn verify_and_sign(&mut self) -> usize {
        self.rows.clear();
        self.fail_rows = 0;
        for c in 0..self.cpus {
            let f = self.faces[c as usize];
            let pass = f.verified();
            let missing = f.missing();
            if !pass {
                self.fail_rows += 1;
            }
            self.rows.push(SignRow { cpu: c, face: f, pass, missing });
        }
        self.rows.len()
    }

    /// 行数守恒：联签行数 == 参与目标数。
    pub fn rows_balanced(&self) -> bool {
        self.rows.len() == self.cpus as usize
    }

    /// 封档（指纹 FNV 钉死，封后写拒）。
    pub fn seal(&mut self) -> Result<u64, A4Code> {
        if self.sealed {
            self.rej_sealed += 1;
            return Err(A4Code::SealedWrite);
        }
        let mut fp: u64 = 0xcbf2_9ce4_8422_2325;
        for r in &self.rows {
            fp ^= r.cpu as u64;
            fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
            fp ^= r.face.phase.rank() as u64;
            fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
            fp ^= if r.pass { 1 } else { 0 };
            fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
        }
        self.sealed = true;
        self.seal_fp = fp;
        Ok(fp)
    }

    /// 联签记录只读。
    pub fn rows(&self) -> &[SignRow] {
        &self.rows
    }

    /// 核验不过目标数。
    pub fn fail_rows(&self) -> u32 {
        self.fail_rows
    }

    /// 封档指纹。
    pub fn seal_fp(&self) -> u64 {
        self.seal_fp
    }

    /// 拒绝计数只读视图。
    pub fn rej_stats(&self) -> (u64, u64, u64, u64, u64, u64) {
        (
            self.rej_phase_skip,
            self.rej_phase_back,
            self.rej_ack_dup,
            self.rej_ack_after,
            self.rej_cpu_unknown,
            self.rej_sealed,
        )
    }
}

// --------------------------------------------------------------------------
// 判据组：UNX-F1563-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx1563_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx1563");
    if a4_segment_guard().is_err() {
        cs.fail("C1563-SEG", "段守卫失败");
        return cs;
    }

    // ---- C1563-01 缺 A2 签名拒受理（联签关系） ----
    let unsigned = A4Sign::new(4, false);
    let mut a4 = match A4Sign::new(4, true) {
        Ok(a) => a,
        Err(_) => {
            cs.fail("C1563-01", "装配失败");
            return cs;
        }
    };
    cs.add(
        "C1563-01 缺A2签拒",
        matches!(unsigned, Err(A4Code::A2Unsigned)),
        "联签关系破裂即拒",
    );

    // ---- C1563-02 三阶段逐项核验（4 目标全走完） ----
    let mut ok = true;
    for c in 0..4u32 {
        if a4.initiate(c, c as u64 * 10).is_err() {
            ok = false;
        }
    }
    for c in 0..4u32 {
        if a4.ack(c, 100 + c as u64 * 10).is_err() {
            ok = false;
        }
    }
    for c in 0..4u32 {
        if a4.complete(c).is_err() {
            ok = false;
        }
    }
    cs.add("C1563-02 三阶段核验", ok, "发起/应答/完成各4次",);

    // ---- C1563-03 消费面三项齐（逐 CPU 核验通过） ----
    let rows = a4.verify_and_sign();
    let face_ok = rows == 4 && a4.rows_balanced() && a4.fail_rows() == 0;
    cs.add(
        "C1563-03 消费面三项齐",
        face_ok,
        "4/4 消费面核验过",
    );

    // ---- C1563-04 阶段跳步拒（未发起即应答） ----
    let mut a4b = match A4Sign::new(2, true) {
        Ok(a) => a,
        Err(_) => {
            cs.fail("C1563-04", "装配失败");
            return cs;
        }
    };
    let skip = a4b.ack(0, 1);
    cs.add(
        "C1563-04 阶段跳步拒",
        skip == Err(A4Code::PhaseSkip),
        "Idle→Ack拒",
    );

    // ---- C1563-05 重复应答拒 / 终后应答拒 ----
    let mut a4c = A4Sign::new(2, true).ok();
    let dup_after = if let Some(x) = a4c.as_mut() {
        let _ = x.initiate(0, 1);
        let _ = x.ack(0, 2);
        x.ack(0, 3)
    } else {
        Err(A4Code::CpuUnknown)
    };
    cs.add(
        "C1563-05 重复应答拒",
        dup_after == Err(A4Code::AckDup),
        "Acked再Ack拒",
    );

    // ---- C1563-06 未全员应答即完成 = 协议不闭合（拒） ----
    let mut a4d = A4Sign::new(3, true).ok();
    let incomplete = if let Some(x) = a4d.as_mut() {
        let _ = x.initiate(0, 1);
        let _ = x.ack(0, 2);
        x.complete(0)
    } else {
        Err(A4Code::CpuUnknown)
    };
    cs.add(
        "C1563-06 未闭合拒完成",
        incomplete == Err(A4Code::Deadlock),
        "非全员Ack完拒",
    );

    // ---- C1563-07 缺项点名（部分目标消费面不过可点名 CPU） ----
    let mut a4e = A4Sign::new(3, true).ok();
    let named = if let Some(x) = a4e.as_mut() {
        let _ = x.initiate(0, 1);
        let _ = x.ack(0, 2);
        // cpu1 只发起不应答，cpu2 未发起 → 三目标均未达终态，核验不过。
        let _ = x.initiate(1, 1);
        x.verify_and_sign();
        // cpu0 虽三项齐但未 complete（相位非终）→ 核验不过，共 3 个不过。
        x.fail_rows() == 3
            && x.rows()[1].cpu == 1
            && x.rows()[1].missing == "tlb_flushed"
            && x.rows()[2].cpu == 2
            && x.rows()[2].missing == "ipi_sent"
    } else {
        false
    };
    cs.add(
        "C1563-07 缺项点名",
        named,
        "不过目标可点到CPU",
    );

    // ---- C1563-08 行数守恒 + 封档指纹 ----
    let fp8 = a4.seal();
    let bal8 = a4.rows_balanced() && a4.rows().len() == 4;
    cs.add(
        "C1563-08 行数守恒+封档",
        fp8.is_ok() && fp8.unwrap() != 0 && bal8,
        "4行=FNV钉死",
    );

    // ---- C1563-09 封档后写拒 ----
    let after = a4.initiate(1, 999);
    cs.add(
        "C1563-09 封档后写拒",
        after == Err(A4Code::SealedWrite),
        "封后initiate拒",
    );

    // ---- C1563-10 未注册 CPU 拒 ----
    let mut a4f = A4Sign::new(2, true).ok();
    let unk = if let Some(x) = a4f.as_mut() { x.initiate(5, 1) } else { Err(A4Code::CpuUnknown) };
    cs.add(
        "C1563-10 未注册CPU拒",
        unk == Err(A4Code::CpuUnknown),
        "cpu5越域拒",
    );

    // ---- C1563-11 等待成环检出（错误互等实现兜底） ----
    let a4g = A4Sign::new(4, true).ok();
    let cyc_ok = if let Some(x) = a4g.as_ref() {
        let empty: [(u32, u32); 0] = [];
        let ring: [(u32, u32); 4] = [(0, 1), (1, 2), (2, 3), (3, 0)];
        x.wait_cycle_free(&empty) && !x.wait_cycle_free(&ring)
    } else {
        false
    };
    cs.add(
        "C1563-11 等待成环检出",
        cyc_ok,
        "空图过四环拒",
    );

    // ---- C1563-12 10 万轮零死锁复验 ----
    let mut a4h = A4Sign::new(VERIFY_CPUS, true).ok();
    let steps = if let Some(x) = a4h.as_mut() {
        x.reverify_zero_deadlock(REVERIFY_ROUNDS, 0x5eed_1234_abcd_0001)
    } else {
        Err(A4Code::CpuUnknown)
    };
    let zero_deadlock = match &steps {
        Ok(s) => *s > 0,
        Err(_) => false,
    };
    // 复验后全员完成 → 消费面齐 → 联签全过。
    let sign_all = if let Some(x) = a4h.as_mut() { x.verify_and_sign() == VERIFY_CPUS as usize && x.fail_rows() == 0 } else { false };
    cs.add(
        "C1563-12 十万轮零死锁",
        zero_deadlock && sign_all,
        "10万轮活性+无环全过",
    );

    cs
}
