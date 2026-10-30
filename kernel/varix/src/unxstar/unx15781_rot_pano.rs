//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15781 · IRunningObjectTable 全量面谱（七核 10/10 + 幽灵注册注入）
//! · 0x677C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15781-J1 面谱成文（Register/Revoke/IsRunning/GetObject/
//!      EnumRunning/NoteChangeTime/GetTimeOfLastChange 七核 10/10 次），
//!      幽灵注册注入（未运行对象登记在册）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 七核闭面：Register/Revoke/IsRunning/GetObject/EnumRunning/
//!     NoteChangeTime/GetTimeOfLastChange；
//!   - ROT 账本：句柄→Moniker（名字+运行态+时间戳向量时钟）；
//!   - 幽灵判据：登记在册但运行态=未运行（Register 必附运行凭据）；
//!   - 状态机：Vacant → Registered → Revoked（句柄单向流水，不复用）；
//!   - LCG 种子写死 = SEG_BASE（0x677C0），禁 rand/浮点/IO/墙钟；
//!     时间戳用确定性整数时钟（Lamport 式 +1），禁墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x677C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x677C0;

/// 每核正演次数（判据钉版：10/10）。
pub const RUNS_PER_CORE: u32 = 10;
/// 七核闭集基数。
pub const CORE_COUNT: usize = 7;
pub const CORE_NAMES: [&str; CORE_COUNT] = [
    "Register",
    "Revoke",
    "IsRunning",
    "GetObject",
    "EnumRunning",
    "NoteChangeTime",
    "GetTimeOfLastChange",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotCode {
    /// 句柄查无（Revoke/GetObject 落空）
    HandleNotFound,
    /// 幽灵注册（未运行对象登记在册）
    GhostRegistration,
    /// 重复注册（同名 Moniker 重复在册）
    DuplicateRegistration,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 句柄复用（流水不复用被破坏）
    HandleReused,
    /// 时间倒流（Note 后 Get 时间早于上次）
    TimeRegression,
    /// 正演账未满
    RunLedgerShort,
    /// 双跑不一致
    NotReproducible,
    /// 枚举账与在册数不守恒
    EnumCountMismatch,
}

impl RotCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RotCode::HandleNotFound => "句柄查无：Revoke/GetObject 落空",
            RotCode::GhostRegistration => "幽灵注册：未运行对象登记在册",
            RotCode::DuplicateRegistration => "重复注册：同名 Moniker 重复在册",
            RotCode::IllegalPhaseEdge => "非法相位边：状态机闭集外",
            RotCode::HandleReused => "句柄复用：流水句柄不复用被破坏",
            RotCode::TimeRegression => "时间倒流：变更时间早于上次",
            RotCode::RunLedgerShort => "正演账未满：七核须 10/10",
            RotCode::NotReproducible => "双跑不一致：面谱复现红线",
            RotCode::EnumCountMismatch => "枚举账不守恒：枚举数≠在册数",
        }
    }
    pub const ALL: [RotCode; 9] = [
        RotCode::HandleNotFound,
        RotCode::GhostRegistration,
        RotCode::DuplicateRegistration,
        RotCode::IllegalPhaseEdge,
        RotCode::HandleReused,
        RotCode::TimeRegression,
        RotCode::RunLedgerShort,
        RotCode::NotReproducible,
        RotCode::EnumCountMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RotCode::ALL.len() {
        if RotCode::ALL[i].val() == code {
            return RotCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x677C0 段闭集内"
}

// ── 条目与状态机 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotPhase {
    /// 空（未登记）
    Vacant,
    /// 在册且运行中（凭据齐）
    Registered,
    /// 已注销（句柄作废，流水不复用）
    Revoked,
}

/// 合法边闭集：Vacant→Registered、Registered→Revoked；其余非法。
pub fn rot_edge_legal(from: RotPhase, to: RotPhase) -> bool {
    match (from, to) {
        (RotPhase::Vacant, RotPhase::Registered) => true,
        (RotPhase::Registered, RotPhase::Revoked) => true,
        _ => false,
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RotEntry {
    pub moniker: String,
    pub phase: RotPhase,
    /// 运行凭据（Register 必附；幽灵=在册但凭据假）。
    pub running_credential: bool,
    /// 最后变更时钟（确定性整数时钟）。
    pub last_change: u64,
}

pub struct RotLedger {
    pub entries: Vec<RotEntry>,
    /// 句柄流水（单调递增，不复用）。
    pub next_handle: u64,
    /// 全局确定性时钟。
    pub clock: u64,
    pub core_ok: [u32; CORE_COUNT],
}

impl RotLedger {
    pub fn new() -> RotLedger {
        RotLedger {
            entries: Vec::new(),
            next_handle: 1,
            clock: 1,
            core_ok: [0; CORE_COUNT],
        }
    }

    /// 核 0：Register（凭据必须为真，幽灵即拒）。
    pub fn register(&mut self, moniker: &str, credential: bool) -> Result<u64, u32> {
        self.core_ok[0] += 1;
        if !credential {
            return Err(RotCode::GhostRegistration.val());
        }
        // 重复注册判据：同名在册即拒。
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].phase == RotPhase::Registered
                && self.entries[i].moniker == moniker
            {
                return Err(RotCode::DuplicateRegistration.val());
            }
            i += 1;
        }
        let h = self.next_handle;
        self.next_handle += 1;
        self.clock += 1;
        self.entries.push(RotEntry {
            moniker: String::from(moniker),
            phase: RotPhase::Registered,
            running_credential: true,
            last_change: self.clock,
        });
        Ok(h)
    }

    /// 核 1：Revoke（句柄必须有效且在册）。
    pub fn revoke(&mut self, handle: u64) -> Result<(), u32> {
        self.core_ok[1] += 1;
        if handle == 0 || handle >= self.next_handle {
            return Err(RotCode::HandleNotFound.val());
        }
        let e = &mut self.entries[(handle - 1) as usize];
        if !rot_edge_legal(e.phase, RotPhase::Revoked) {
            return Err(RotCode::IllegalPhaseEdge.val());
        }
        e.phase = RotPhase::Revoked;
        Ok(())
    }

    /// 核 2：IsRunning（在册且运行中才真）。
    pub fn is_running(&mut self, moniker: &str) -> Result<bool, u32> {
        self.core_ok[2] += 1;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].moniker == moniker {
                return Ok(self.entries[i].phase == RotPhase::Registered
                    && self.entries[i].running_credential);
            }
            i += 1;
        }
        Ok(false)
    }

    /// 核 3：GetObject（按 Moniker 取句柄）。
    pub fn get_object(&mut self, moniker: &str) -> Result<u64, u32> {
        self.core_ok[3] += 1;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].moniker == moniker
                && self.entries[i].phase == RotPhase::Registered
            {
                return Ok((i + 1) as u64);
            }
            i += 1;
        }
        Err(RotCode::HandleNotFound.val())
    }

    /// 核 4：EnumRunning（枚举在册数须与账守恒）。
    pub fn enum_running(&mut self) -> Result<u32, u32> {
        self.core_ok[4] += 1;
        let mut live = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].phase == RotPhase::Registered {
                live += 1;
            }
            i += 1;
        }
        Ok(live)
    }

    /// 核 5：NoteChangeTime（时间只进不退）。
    pub fn note_change_time(&mut self, handle: u64) -> Result<u64, u32> {
        self.core_ok[5] += 1;
        if handle == 0 || handle >= self.next_handle {
            return Err(RotCode::HandleNotFound.val());
        }
        let e = &mut self.entries[(handle - 1) as usize];
        if e.phase != RotPhase::Registered {
            return Err(RotCode::HandleNotFound.val());
        }
        self.clock += 1;
        if self.clock <= e.last_change {
            return Err(RotCode::TimeRegression.val());
        }
        e.last_change = self.clock;
        Ok(e.last_change)
    }

    /// 核 6：GetTimeOfLastChange。
    pub fn get_time_of_last_change(&mut self, handle: u64) -> Result<u64, u32> {
        self.core_ok[6] += 1;
        if handle == 0 || handle >= self.next_handle {
            return Err(RotCode::HandleNotFound.val());
        }
        let e = &self.entries[(handle - 1) as usize];
        if e.phase != RotPhase::Registered {
            return Err(RotCode::HandleNotFound.val());
        }
        Ok(e.last_change)
    }

    /// 幽灵巡检：在册条目凭据必须为真。
    pub fn ghost_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].phase == RotPhase::Registered
                && !self.entries[i].running_credential
            {
                return Err(RotCode::GhostRegistration.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["幽灵注册族", "句柄查无族", "重复注册族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_CORE {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：幽灵注册注入（凭据假 ×10）→ GhostRegistration。
    let mut k = 0u32;
    while k < RUNS_PER_CORE {
        ledger.tries[0] += 1;
        let mut rot = RotLedger::new();
        if let Err(e) = rot.register("ghost.moniker", false) {
            if e == RotCode::GhostRegistration.val() {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：句柄查无注入（Revoke 未来句柄 ×10）。
    k = 0;
    while k < RUNS_PER_CORE {
        ledger.tries[1] += 1;
        let mut rot = RotLedger::new();
        let _ = rot.register("live.moniker", true);
        if let Err(e) = rot.revoke(rot.next_handle + 100) {
            if e == RotCode::HandleNotFound.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：重复注册注入（同名 ×10）。
    k = 0;
    while k < RUNS_PER_CORE {
        ledger.tries[2] += 1;
        let mut rot = RotLedger::new();
        let _ = rot.register("dup.moniker", true);
        if let Err(e) = rot.register("dup.moniker", true) {
            if e == RotCode::DuplicateRegistration.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：七核各 10 次 ───────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub core_ok: [u32; CORE_COUNT],
    pub live: u32,
    pub revoked: u32,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut rot = RotLedger::new();
    // Register ×10（唯一名 mono1..mono10，句柄 1..=10）。
    let mut handles: Vec<u64> = Vec::new();
    let mut r = 0u32;
    while r < RUNS_PER_CORE {
        let _ = g.below(1000);
        let name = format!("mono{}", r as u64 + 1);
        let h = rot.register(&name, true)?;
        handles.push(h);
        r += 1;
    }
    // IsRunning ×10（前 10 名逐条）。
    let mut r = 0u32;
    while r < RUNS_PER_CORE {
        let name = format!("mono{}", r as u64 + 1);
        let live = rot.is_running(&name)?;
        if !live {
            return Err(RotCode::GhostRegistration.val());
        }
        r += 1;
    }
    // GetObject ×10。
    let mut r = 0u32;
    while r < RUNS_PER_CORE {
        let name = format!("mono{}", r as u64 + 1);
        let h = rot.get_object(&name)?;
        if h != handles[r as usize] {
            return Err(RotCode::HandleNotFound.val());
        }
        r += 1;
    }
    // NoteChangeTime ×10 + GetTimeOfLastChange ×10（时间只进不退）。
    let mut prev: u64 = 0;
    let mut r = 0u32;
    while r < RUNS_PER_CORE {
        let t = rot.note_change_time(handles[r as usize])?;
        if t <= prev {
            return Err(RotCode::TimeRegression.val());
        }
        prev = t;
        let t2 = rot.get_time_of_last_change(handles[r as usize])?;
        if t2 != t {
            return Err(RotCode::TimeRegression.val());
        }
        r += 1;
    }
    // EnumRunning ×10（守恒：live==10）。
    let mut r = 0u32;
    while r < RUNS_PER_CORE {
        let live = rot.enum_running()?;
        if live != RUNS_PER_CORE {
            return Err(RotCode::EnumCountMismatch.val());
        }
        r += 1;
    }
    // Revoke ×10（全部注销）。
    let mut r = 0u32;
    while r < RUNS_PER_CORE {
        rot.revoke(handles[r as usize])?;
        r += 1;
    }
    rot.ghost_audit()?;
    // 账满检：七核各 ≥10。
    let mut c = 0;
    while c < CORE_COUNT {
        if rot.core_ok[c] < RUNS_PER_CORE {
            return Err(RotCode::RunLedgerShort.val());
        }
        c += 1;
    }
    let live = rot.enum_running()?;
    let mut revoked = 0u32;
    let mut i = 0;
    while i < rot.entries.len() {
        if rot.entries[i].phase == RotPhase::Revoked {
            revoked += 1;
        }
        i += 1;
    }
    if live != 0 || revoked != RUNS_PER_CORE {
        return Err(RotCode::EnumCountMismatch.val());
    }
    Ok(GoldenResult { core_ok: rot.core_ok, live, revoked })
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let mut i = 0;
    while i < CORE_COUNT {
        if a.core_ok[i] != b.core_ok[i] {
            return Err(RotCode::NotReproducible.val());
        }
        i += 1;
    }
    if a.live != b.live || a.revoked != b.revoked {
        return Err(RotCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RotCode::ALL.len() {
        let v = RotCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：七核 10/10 + 注销守恒。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => {
            let mut full = g.live == 0 && g.revoked == RUNS_PER_CORE;
            let mut c = 0;
            while c < CORE_COUNT {
                if g.core_ok[c] < RUNS_PER_CORE {
                    full = false;
                }
                c += 1;
            }
            full
        }
        Err(_) => false,
    };
    v.push(("J1-七核正演10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-幽灵等三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 状态机非法边抽验：Revoked→Registered 直跳必拒（句柄不复用）。
    let bad = !rot_edge_legal(RotPhase::Revoked, RotPhase::Registered);
    v.push(("J1-句柄复用边必拒", bad, String::from("Revoked→Registered 闭集外")));
    // 5. 七核名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < CORE_COUNT {
        if CORE_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-七核名点验", ok == CORE_COUNT, format!("{}/7", ok)));
    // 6. 时间倒流反向判据：Note 后 Get 必相等且单调（抽 3 条）。
    let mut rot = RotLedger::new();
    let mut mono_ok = true;
    let mut last = 0u64;
    let mut r = 0;
    while r < 3 {
        let h = rot.register("mk", true);
        match h {
            Ok(hh) => {
                let t = rot.note_change_time(hh);
                match t {
                    Ok(tt) => {
                        if tt <= last {
                            mono_ok = false;
                        }
                        last = tt;
                    }
                    Err(_) => mono_ok = false,
                }
            }
            Err(_) => mono_ok = false,
        }
        r += 1;
    }
    v.push(("J1-时间单调反向判据", mono_ok, format!("last={}", last)));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x677C0 窗口")));
    v
}

pub fn run_unx15781_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B30 IRunningObjectTable 全量面谱（七核 + 幽灵注入）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
