//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15621 · IConnectionPointContainer 面谱 · 0x675C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B22）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15621-J1 容器面谱成文（FindConnectionPoint/
//!     EnumConnectionPoints 双核 10/10 次），幽灵 IID 注入（未声明
//!     IID 仍返连接点）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 连接点池：IID 声明闭集（事件源 IID 表写死），Find 只对声明
//!     IID 返连接点，未声明 = 幽灵 IID 注入，如实拒；
//!   - 双核分账：FindConnectionPoint（按 IID 查点）/ 
//!     EnumConnectionPoints（全点枚举闭环）各 10/10 轮；
//!   - 连接点内部状态机：Empty（无订阅）→ Advised（有订阅槽）→
//!     Unadvised（退订）；槽位守恒：advised − unadvised = 在订数；
//!   - 枚举守恒：EnumConnectionPoints 产出数 = 声明 IID 数；
//!   - 幽灵注入面：查未声明 IID 10/10 必拒；退订未订槽 10/10 必拒；
//!   - LCG 种子写死 = SEG_BASE（0x675C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x675C0，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x675C0;

// ── IID 声明闭集（写死：典型 OLE 事件源 IID 池） ─────────────────
pub const IID_CPIDATA: usize = 0;
pub const IID_CPIEVENTS: usize = 1;
pub const IID_CPISHOWN: usize = 2;
pub const IID_CPISHUTDOWN: usize = 3;
pub const IID_COUNT: usize = 4;
pub const IID_NAMES: [&str; IID_COUNT] = [
    "iid:cpidata", "iid:cpievents", "iid:cpishown", "iid:cpishutdown",
];

// ── 双核闭集 ─────────────────────────────────────────────────────
pub const CORE_FIND: usize = 0;
pub const CORE_ENUM: usize = 1;
pub const CORE_COUNT: usize = 2;
pub const CORE_NAMES: [&str; CORE_COUNT] = ["FindConnectionPoint", "EnumConnectionPoints"];

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpcCode {
    /// IID 越声明闭集
    IidOutOfRange,
    /// 双核越闭集
    CoreOutOfRange,
    /// 幽灵 IID：未声明仍查点（如实拒）
    GhostIid,
    /// 重复声明 IID
    DuplicateIid,
    /// 订阅槽越界
    SlotOutOfRange,
    /// 退订未订槽（非法迁移）
    UnadviseNotAdvised,
    /// 订阅槽守恒破坏
    SlotConservationBroken,
    /// 枚举产出与声明不守恒
    EnumConservationBroken,
    /// 幽灵注入漏抓
    GhostLeak,
    /// 双跑不一致
    NotReproducible,
}

impl CpcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CpcCode::IidOutOfRange => "IID 越声明闭集：4 源冻结",
            CpcCode::CoreOutOfRange => "双核越闭集：Find/Enum 冻结",
            CpcCode::GhostIid => "幽灵 IID：未声明仍返连接点，如实拒",
            CpcCode::DuplicateIid => "重复声明 IID：池唯一性红线",
            CpcCode::SlotOutOfRange => "订阅槽越界：先比长度再下标",
            CpcCode::UnadviseNotAdvised => "退订未订槽：非法迁移",
            CpcCode::SlotConservationBroken => "订阅槽守恒破坏：订−退≠在订",
            CpcCode::EnumConservationBroken => "枚举产出与声明不守恒",
            CpcCode::GhostLeak => "幽灵注入漏抓：检出 10/10 红线",
            CpcCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CpcCode; 10] = [
        CpcCode::IidOutOfRange,
        CpcCode::CoreOutOfRange,
        CpcCode::GhostIid,
        CpcCode::DuplicateIid,
        CpcCode::SlotOutOfRange,
        CpcCode::UnadviseNotAdvised,
        CpcCode::SlotConservationBroken,
        CpcCode::EnumConservationBroken,
        CpcCode::GhostLeak,
        CpcCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CpcCode::ALL.len() {
        if CpcCode::ALL[i].val() == code {
            return CpcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x675C0 段闭集内"
}

// ── 连接点状态机 ─────────────────────────────────────────────────
/// 三态闭集：Empty → Advised → Unadvised（可再 Advised 回环）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpState {
    Empty,
    Advised,
    Unadvised,
}

/// 合法边：Empty→Advised（订）；Advised→Unadvised（退）；
/// Unadvised→Advised（再订）；其余（Empty→Unadvised 跳步退订）
/// 非法。
pub fn cp_can_move(from: CpState, to: CpState) -> bool {
    match (from, to) {
        (CpState::Empty, CpState::Advised) => true,
        (CpState::Advised, CpState::Unadvised) => true,
        (CpState::Unadvised, CpState::Advised) => true,
        _ => false,
    }
}

/// 连接点记录。
#[derive(Clone, Debug)]
pub struct ConnPoint {
    pub iid: usize,
    pub slots: Vec<CpState>,
    pub advised_total: u32,
    pub unadvised_total: u32,
}

impl ConnPoint {
    pub fn live(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i] == CpState::Advised {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 槽守恒：advised_total − unadvised_total = live。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.advised_total < self.unadvised_total {
            return Err(CpcCode::SlotConservationBroken.val());
        }
        if self.advised_total - self.unadvised_total != self.live() {
            return Err(CpcCode::SlotConservationBroken.val());
        }
        Ok(())
    }
}

// ── 容器面谱账 ───────────────────────────────────────────────────
pub struct CpcContainer {
    pub declared: [bool; IID_COUNT],
    pub points: Vec<ConnPoint>,
    pub find_calls: u32,
    pub enum_calls: u32,
}

impl CpcContainer {
    pub fn new() -> CpcContainer {
        CpcContainer {
            declared: [false; IID_COUNT],
            points: Vec::new(),
            find_calls: 0,
            enum_calls: 0,
        }
    }

    /// 声明连接点（重复声明拒）。
    pub fn declare(&mut self, iid: usize) -> Result<usize, u32> {
        if iid >= IID_COUNT {
            return Err(CpcCode::IidOutOfRange.val());
        }
        if self.declared[iid] {
            return Err(CpcCode::DuplicateIid.val());
        }
        self.declared[iid] = true;
        self.points.push(ConnPoint {
            iid,
            slots: Vec::new(),
            advised_total: 0,
            unadvised_total: 0,
        });
        Ok(self.points.len() - 1)
    }

    fn find_point(&self, iid: usize) -> Option<usize> {
        let mut i = 0;
        while i < self.points.len() {
            if self.points[i].iid == iid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// FindConnectionPoint：只对声明 IID 返点；未声明 = 幽灵注入拒。
    pub fn find_connection_point(&mut self, iid: usize) -> Result<usize, u32> {
        if iid >= IID_COUNT {
            return Err(CpcCode::IidOutOfRange.val());
        }
        self.find_calls += 1;
        if !self.declared[iid] {
            return Err(CpcCode::GhostIid.val());
        }
        match self.find_point(iid) {
            Some(i) => Ok(i),
            None => Err(CpcCode::GhostIid.val()),
        }
    }

    /// EnumConnectionPoints：产出全部已声明点（守恒=声明数）。
    pub fn enum_connection_points(&mut self) -> Result<Vec<usize>, u32> {
        self.enum_calls += 1;
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.points.len() {
            v.push(i);
            i += 1;
        }
        // 守恒对账：产出数 = 声明数。
        let mut declared_n = 0;
        let mut f = 0;
        while f < IID_COUNT {
            if self.declared[f] {
                declared_n += 1;
            }
            f += 1;
        }
        if v.len() as u32 != declared_n {
            return Err(CpcCode::EnumConservationBroken.val());
        }
        Ok(v)
    }

    /// Advise：订阅（槽追加，Empty/Unadvised → Advised）。
    pub fn advise(&mut self, pid: usize) -> Result<usize, u32> {
        if pid >= self.points.len() {
            return Err(CpcCode::SlotOutOfRange.val());
        }
        let st = if self.points[pid].slots.len() == 0 {
            CpState::Empty
        } else {
            self.points[pid].slots[self.points[pid].slots.len() - 1]
        };
        if !cp_can_move(st, CpState::Advised) {
            return Err(CpcCode::UnadviseNotAdvised.val());
        }
        self.points[pid].slots.push(CpState::Advised);
        self.points[pid].advised_total += 1;
        Ok(self.points[pid].slots.len() - 1)
    }

    /// Unadvise：退订（仅 Advised 槽可退）。
    pub fn unadvise(&mut self, pid: usize, slot: usize) -> Result<(), u32> {
        if pid >= self.points.len() {
            return Err(CpcCode::SlotOutOfRange.val());
        }
        if slot >= self.points[pid].slots.len() {
            return Err(CpcCode::SlotOutOfRange.val());
        }
        if self.points[pid].slots[slot] != CpState::Advised {
            return Err(CpcCode::UnadviseNotAdvised.val());
        }
        self.points[pid].slots[slot] = CpState::Unadvised;
        self.points[pid].unadvised_total += 1;
        Ok(())
    }

    /// 全点守恒。
    pub fn conserve_all(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.points.len() {
            self.points[i].conserve()?;
            i += 1;
        }
        Ok(())
    }

    /// 双核判据正演：10/10 轮。
    pub fn judge_run(&mut self) -> Result<(u32, u32), u32> {
        let mut find_ok = 0;
        let mut enum_ok = 0;
        // 声明全部 4 IID。
        let mut f = 0;
        while f < IID_COUNT {
            self.declare(f)?;
            f += 1;
        }
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let iid = r as usize % IID_COUNT;
            // Find 核。
            if self.find_connection_point(iid).is_ok() {
                find_ok += 1;
            }
            // Enum 核。
            let ev = self.enum_connection_points()?;
            if ev.len() == IID_COUNT {
                enum_ok += 1;
            }
            // 订/退闭环（槽守恒）。
            let pid = self.find_connection_point(iid)?;
            let s = self.advise(pid)?;
            self.unadvise(pid, s)?;
            r += 1;
        }
        self.conserve_all()?;
        Ok((find_ok, enum_ok))
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_CORE_OK: u32 = 10;
pub const GOLDEN_GHOST_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut c = CpcContainer::new();
    let (f, e) = c.judge_run()?;
    if f != GOLDEN_CORE_OK || e != GOLDEN_CORE_OK {
        return Err(CpcCode::EnumConservationBroken.val());
    }
    // 幽灵 IID 注入 10/10：声明池外的伪 IID 查点必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c2 = CpcContainer::new();
        let _ = c2.declare(IID_CPIDATA);
        let ghost = IID_COUNT + 1; // 池外伪 IID
        if let Err(e) = c2.find_connection_point(ghost) {
            if e == CpcCode::IidOutOfRange.val() {
                caught += 1;
            }
        }
        // 声明池内但未声明的 IID 也必拒（幽灵语义二型）。
        if let Err(e) = c2.find_connection_point(IID_CPISHUTDOWN) {
            if e == CpcCode::GhostIid.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_GHOST_CAUGHT {
        return Err(CpcCode::GhostLeak.val());
    }
    Ok((f, e, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：幽灵 IID（未声明仍查点）×10 必拒。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c = CpcContainer::new();
        let _ = c.declare(IID_CPIEVENTS);
        if let Err(e) = c.find_connection_point(IID_CPISHOWN) {
            if e == CpcCode::GhostIid.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：重复声明注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut c1 = CpcContainer::new();
    let _ = c1.declare(IID_CPIDATA);
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = c1.declare(IID_CPIDATA) {
            if e == CpcCode::DuplicateIid.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：退订未订槽注入 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c2 = CpcContainer::new();
        let _ = c2.declare(IID_CPISHUTDOWN);
        let pid = c2.find_connection_point(IID_CPISHUTDOWN).ok();
        if let Some(p) = pid {
            if let Err(e) = c2.unadvise(p, 0) {
                // 空槽退订：槽越界拒。
                if e == CpcCode::SlotOutOfRange.val() {
                    ledger.caught[2] += 1;
                }
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法容器必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut c = CpcContainer::new();
    let _ = c.declare(IID_CPIDATA)?;
    let pid = c.find_connection_point(IID_CPIDATA)?;
    let s = c.advise(pid)?;
    let _ = c.advise(pid)?; // 再订
    c.unadvise(pid, s)?;
    let _ = c.enum_connection_points()?;
    c.conserve_all()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CpcCode::ALL.len() {
        let v = CpcCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：双核 10/10 + 幽灵注入 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-双核10/10+幽灵IID10/10检出",
        matches!(g, Ok((10, 10, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 连接点边闭集：跳步退订（Empty→Unadvised）恒非法。
    let closed = !cp_can_move(CpState::Empty, CpState::Unadvised)
        && cp_can_move(CpState::Unadvised, CpState::Advised);
    v.push(("J1-订阅边闭集", closed, String::from("E→A→U→A 闭集")));
    // 4. IID 名点验：4 声明源非空名。
    let names_ok = {
        let mut ok = true;
        let mut f = 0;
        while f < IID_COUNT {
            if IID_NAMES[f].len() == 0 {
                ok = false;
            }
            f += 1;
        }
        ok
    };
    v.push(("J1-IID名点验", names_ok, format!("{} 源", IID_COUNT)));
    // 5. 反向判据：合法容器必须全过。
    v.push(("J1-反向探针合法容器", matches!(reverse_probe(), Ok(())), String::from("订/退/枚全过")));
    // 6. 槽守恒反向：人为破坏账面必拒。
    let mut bad = CpcContainer::new();
    let _ = bad.declare(IID_CPIDATA);
    let p = bad.find_connection_point(IID_CPIDATA).ok();
    if let Some(pp) = p {
        let _ = bad.advise(pp);
        bad.points[pp].advised_total = 0; // 人为漏记
    }
    let rev = if let Err(e) = bad.conserve_all() {
        e == CpcCode::SlotConservationBroken.val()
    } else {
        false
    };
    v.push(("J1-槽守恒反向", rev, String::from("订−退=在订")));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xC0);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x675C0 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15621_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15621 IConnectionPointContainer 面谱");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
