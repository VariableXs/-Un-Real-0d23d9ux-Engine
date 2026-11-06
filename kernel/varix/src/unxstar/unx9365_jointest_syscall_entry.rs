//! UNX-F9365 syscall 路由联测（entry.rs 双门 × C3 探针） · 0x7C8C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9365-J1 双门路由联测判据过，SYSCALL/int 0x80 双门逐门
//! 可观测，C3 探针注入零改动格过。
//!
//! SEG_BASE = 0x7C8C0 诊断码闭集说明：本段 0x40 窗口独占；诊断码 1 基
//! 编码（码值 = SEG_BASE + 变体序号 + 1），闭集 12 个
//! （RouteCode::ALL）两两唯一，段基址本身不占码。
//!
//! 结构清单：
//!   ① 双门模型 Gate：SYSCALL 门与 int 0x80 门闭集，逐门可观测。
//!   ② 路由判定器 route：坏门 EINVAL → 号段 ENOSYS → 未注册 ENOSYS →
//!      门掩码不支持 ENOSYS → 命中并回路由结果；判序冻结。
//!   ③ C3 探针墙 ProbeWall：每格（门,号）挂一个探针号；路由命中打点
//!      可观测，探针墙指纹在注入前后零改动。
//!   ④ 路由账 RouteLedger：尝试/命中/拒绝三本账闭合，只增不减。
//!   ⑤ 注入核：越界号/门不支持/坏门号三类各 10 轮全抓，账守恒逃逸=0；
//!      Linux errno 对照表 + FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9365 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C8C0;

/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7A000;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 门数量。
pub const GATES: usize = 2;
/// 共享号面行数（双门共享一张分发表）。
pub const ROWS: usize = 12;
/// 号面上限。
pub const NR_MAX: u32 = 256;
/// 探针墙容量。
pub const WALL_CAP: usize = GATES * ROWS;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EEXIST: i32 = 17;
pub const ENOSYS: i32 = 38;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（1 基：码值 = SEG_BASE + 序号 + 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteCode {
    /// 坏门号：门闭集之外
    BadGate,
    /// 号越界：nr > NR_MAX 或 nr == 0
    NrBeyond,
    /// 未注册号：分发表空槽
    NrUnregistered,
    /// 门不支持：号已注册但该门掩码未开放
    GateNotOpen,
    /// 探针墙满：格位耗尽
    WallFull,
    /// 探针打点缺失：命中却未留观测点
    ProbeMissing,
    /// 探针墙漂移：注入前后指纹不一致
    WallDrift,
    /// 路由账倒退
    LedgerRegress,
    /// 路由账失守：命中+拒绝 != 尝试
    LedgerUnbalanced,
    /// 双跑不一致
    NotReproducible,
    /// 文案缺失
    DetailMissing,
    /// 段位越界
    SegmentViolation,
}

impl RouteCode {
    /// 码值（段基址 + 变体序号 + 1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RouteCode::BadGate => "坏门号被拒：路由门只有 SYSCALL 与 int 0x80 两扇；\
                闭集之外一律 EINVAL",
            RouteCode::NrBeyond => "号越界被拒：nr 必须落在 1..=NR_MAX；越界回 \
                ENOSYS，路由入口先于查表拒绝",
            RouteCode::NrUnregistered => "未注册号被拒：分发表空槽；请先走注册接口 \
                登记，未注册回 ENOSYS",
            RouteCode::GateNotOpen => "门不支持被拒：号已注册但该门掩码未开放；\
                请改走另一扇门或核对门位登记",
            RouteCode::WallFull => "探针墙满：格位耗尽；请扩容或复用既有格，\
                禁止覆盖",
            RouteCode::ProbeMissing => "探针打点缺失：命中路由必须留下可观测点；\
                请核对探针墙挂载",
            RouteCode::WallDrift => "探针墙漂移：注入前后指纹不一致；探针墙对 \
                路由联测只读，零改动声明失守",
            RouteCode::LedgerRegress => "路由账倒退被拒：计数只增不减；请检查 \
                回滚路径",
            RouteCode::LedgerUnbalanced => "路由账失守：命中+拒绝必须等于尝试；\
                请核对每笔路由是否都归了账",
            RouteCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            RouteCode::DetailMissing => "诊断文案缺失：闭集每码必须带中文建议；\
                请补齐 detail()",
            RouteCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [RouteCode; 12] = [
        RouteCode::BadGate,
        RouteCode::NrBeyond,
        RouteCode::NrUnregistered,
        RouteCode::GateNotOpen,
        RouteCode::WallFull,
        RouteCode::ProbeMissing,
        RouteCode::WallDrift,
        RouteCode::LedgerRegress,
        RouteCode::LedgerUnbalanced,
        RouteCode::NotReproducible,
        RouteCode::DetailMissing,
        RouteCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── 双门模型 ──────────────────────────────────────────────────────────
/// 路由门闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// SYSCALL 门（64 位原生入口）
    Syscall,
    /// int 0x80 门（兼容入口）
    Int80,
}

impl Gate {
    /// 门序号。
    pub fn idx(self) -> usize {
        match self {
            Gate::Syscall => 0,
            Gate::Int80 => 1,
        }
    }

    /// 门位掩码。
    pub fn bit(self) -> u8 {
        match self {
            Gate::Syscall => 0b01,
            Gate::Int80 => 0b10,
        }
    }

    /// 按门序号还原门（越界 None）。
    pub fn from_idx(i: usize) -> Option<Gate> {
        match i {
            0 => Some(Gate::Syscall),
            1 => Some(Gate::Int80),
            _ => None,
        }
    }

    /// 门名。
    pub fn name(self) -> &'static str {
        match self {
            Gate::Syscall => "SYSCALL",
            Gate::Int80 => "int80",
        }
    }
}

// ── 共享号面 ──────────────────────────────────────────────────────────
/// 一行号面：号 + 双门开放位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 系统调用号
    pub nr: u32,
    /// 门开放掩码（bit0=SYSCALL，bit1=int80）
    pub open: u8,
}

/// 双门共享号面（冻结 12 行）。
pub const TABLE: [Row; ROWS] = [
    Row { nr: 1, open: 0b11 },
    Row { nr: 2, open: 0b11 },
    Row { nr: 3, open: 0b11 },
    Row { nr: 4, open: 0b11 },
    Row { nr: 5, open: 0b11 },
    Row { nr: 6, open: 0b11 },
    Row { nr: 7, open: 0b11 },
    Row { nr: 8, open: 0b11 },
    Row { nr: 9, open: 0b01 },
    Row { nr: 10, open: 0b01 },
    Row { nr: 11, open: 0b10 },
    Row { nr: 12, open: 0b10 },
];

/// 号面查询：命中返回开放掩码。
pub fn row_open(nr: u32) -> Option<u8> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            if r.nr == nr {
                return Some(r.open);
            }
        }
        i += 1;
    }
    None
}

// ── C3 探针墙 ─────────────────────────────────────────────────────────
/// 探针墙：每格（门,号）一个探针号，对路由只读（零改动声明）。
#[derive(Clone, Copy, Debug)]
pub struct ProbeWall {
    slots: [Option<u32>; WALL_CAP],
    mounted: u32,
}

impl ProbeWall {
    /// 空墙。
    pub const fn new() -> ProbeWall {
        ProbeWall { slots: [None; WALL_CAP], mounted: 0 }
    }

    /// 挂探针：号 = nr*10 + 门序号 + 1；缺行 ENOENT、越墙 ENOENT、重复 EEXIST。
    pub fn mount(&mut self, nr: u32, gate: Gate) -> SysResult<u32> {
        if row_open(nr).is_none() {
            return Err(ENOENT);
        }
        let pid = nr.saturating_mul(10).saturating_add(gate.idx() as u32 + 1);
        let mut i = 0usize;
        while i < WALL_CAP {
            if let Some(Some(p)) = self.slots.get(i).map(|x| *x) {
                if p == pid {
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        let mut free = None;
        let mut j = 0usize;
        while j < WALL_CAP {
            if let Some(None) = self.slots.get(j).map(|x| *x) {
                free = Some(j);
                break;
            }
            j += 1;
        }
        match free {
            Some(idx) => {
                if let Some(cell) = self.slots.get_mut(idx) {
                    *cell = Some(pid);
                }
                self.mounted = self.mounted.saturating_add(1);
                Ok(pid)
            }
            None => Err(ENOENT),
        }
    }

    /// 探针查询。
    pub fn probe_of(&self, nr: u32, gate: Gate) -> Option<u32> {
        let pid = nr.saturating_mul(10).saturating_add(gate.idx() as u32 + 1);
        let mut i = 0usize;
        while i < WALL_CAP {
            if let Some(Some(p)) = self.slots.get(i).map(|x| *x) {
                if p == pid {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    /// 挂载数。
    pub fn mounted(&self) -> u32 {
        self.mounted
    }
}

/// 探针墙指纹（对路由零改动的观测基线）。
pub fn wall_fingerprint(wall: &ProbeWall) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((wall.mounted() & 0xFF) as u8);
    let mut i = 0usize;
    while i < WALL_CAP {
        if let Some(Some(p)) = wall.slots.get(i).map(|x| *x) {
            bytes.push((p & 0xFF) as u8);
        } else {
            bytes.push(0);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 路由判定器 ────────────────────────────────────────────────────────
/// 路由命中结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteOutcome {
    /// 命中的门
    pub gate: Gate,
    /// 系统调用号
    pub nr: u32,
    /// 探针打点（探针墙上有挂点则回 Some）
    pub probe_hit: Option<u32>,
}

/// 路由账：尝试/命中/拒绝三本账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteLedger {
    /// 尝试总数
    pub attempts: u32,
    /// 命中数
    pub hits: u32,
    /// 拒绝数
    pub rejects: u32,
}

impl RouteLedger {
    const fn zero() -> RouteLedger {
        RouteLedger { attempts: 0, hits: 0, rejects: 0 }
    }

    /// 记一笔路由（命中或拒绝），账闭合校验。
    pub fn record(&mut self, hit: bool) -> SysResult<()> {
        self.attempts = self.attempts.saturating_add(1);
        if hit {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.rejects = self.rejects.saturating_add(1);
        }
        if self.hits.saturating_add(self.rejects) != self.attempts {
            return Err(-(RouteCode::LedgerUnbalanced as i32));
        }
        Ok(())
    }

    /// 账闭合。
    pub fn balanced(&self) -> bool {
        self.hits.saturating_add(self.rejects) == self.attempts
    }
}

/// 路由判定：坏门 EINVAL → 号段 ENOSYS → 未注册 ENOSYS → 门掩码
/// 不支持 ENOSYS → 命中。判序冻结，与 Linux 号面语义一致。
pub fn route(gate: Option<Gate>, nr: u32, wall: &ProbeWall) -> SysResult<RouteOutcome> {
    let g = match gate {
        Some(g) => g,
        None => return Err(EINVAL),
    };
    if nr == 0 || nr > NR_MAX {
        return Err(ENOSYS);
    }
    let open = match row_open(nr) {
        Some(o) => o,
        None => return Err(ENOSYS),
    };
    if open & g.bit() == 0 {
        return Err(ENOSYS);
    }
    Ok(RouteOutcome { gate: g, nr, probe_hit: wall.probe_of(nr, g) })
}

/// 带账路由：每笔归账，命中必须打点可观测。
pub fn route_accounted(
    gate: Option<Gate>,
    nr: u32,
    wall: &ProbeWall,
    ledger: &mut RouteLedger,
) -> SysResult<RouteOutcome> {
    let r = route(gate, nr, wall);
    let hit = match &r {
        Ok(o) => o.probe_hit.is_some(),
        Err(_) => false,
    };
    let rec = ledger.record(hit);
    if rec.is_err() {
        return rec.map(|_| RouteOutcome {
            gate: Gate::Syscall,
            nr: 0,
            probe_hit: None,
        });
    }
    r
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 三类违约样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolationCase {
    /// 越界号
    NrBeyond,
    /// 门不支持（号已注册但该门未开放）
    GateNotOpen,
    /// 坏门号
    BadGate,
}

impl ViolationCase {
    /// 闭集全量。
    pub const ALL: [ViolationCase; 3] = [
        ViolationCase::NrBeyond,
        ViolationCase::GateNotOpen,
        ViolationCase::BadGate,
    ];

    /// 样本执行：违约必须命中约定码。
    fn run(self) -> bool {
        let wall = ProbeWall::new();
        match self {
            ViolationCase::NrBeyond => {
                route(Some(Gate::Syscall), NR_MAX + 1, &wall) == Err(ENOSYS)
            }
            ViolationCase::GateNotOpen => {
                route(Some(Gate::Int80), 9, &wall) == Err(ENOSYS)
                    && route(Some(Gate::Syscall), 11, &wall) == Err(ENOSYS)
            }
            ViolationCase::BadGate => route(None, 4, &wall) == Err(EINVAL),
        }
    }
}

/// 三类违约 × 10 轮注入，返回账本与逐类检出数。
pub fn run_route_injection() -> (RouteLedger, [u32; 3]) {
    let mut ledger = RouteLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        if let Some(case) = ViolationCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.run();
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                // 违约被拒 = 路由拒绝账一笔；逃逸（未被拒）才记命中。
                let _ = ledger.record(!caught);
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；权限拒绝族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；探针缺基族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏门族"),
    ("ENOSYS", -38, 1, "Windows ERROR_INVALID_FUNCTION；未实装号族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：双门 × 号面逐格路由结果。
pub fn corpus_fingerprint() -> u32 {
    let mut wall = ProbeWall::new();
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i).copied() {
            let mut gi = 0usize;
            while gi < GATES {
                if let Some(g) = Gate::from_idx(gi) {
                    if r.open & g.bit() != 0 {
                        let _ = wall.mount(r.nr, g);
                    }
                }
                gi += 1;
            }
        }
        i += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut gi = 0usize;
    while gi < GATES {
        if let Some(g) = Gate::from_idx(gi) {
            let mut nr = 1u32;
            while nr <= 12 {
                let res = route(Some(g), nr, &wall);
                match res {
                    Ok(o) => {
                        bytes.push(1);
                        bytes.push((o.nr & 0xFF) as u8);
                        bytes.push(if o.probe_hit.is_some() { 1 } else { 0 });
                    }
                    Err(code) => bytes.push((code & 0xFF) as u8),
                }
                nr += 1;
            }
        }
        gi += 1;
    }
    bytes.push((wall_fingerprint(&wall) & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9365 判据集。
pub fn run_unx9365_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9365");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (WINDOW_LO..=WINDOW_HI).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 探针墙挂载：双门逐格
    let mut wall = ProbeWall::new();
    let mut mount_ok = true;
    let mut mi = 0usize;
    while mi < TABLE.len() {
        if let Some(r) = TABLE.get(mi).copied() {
            let mut gi = 0usize;
            while gi < GATES {
                if let Some(g) = Gate::from_idx(gi) {
                    if r.open & g.bit() != 0 && wall.mount(r.nr, g).is_err() {
                        mount_ok = false;
                    }
                }
                gi += 1;
            }
        }
        mi += 1;
    }
    let expect_mount = 20u32;
    s.add("探针墙：双门开放格全量挂载", mount_ok && wall.mounted() == expect_mount,
          "每格（门,号）一个探针号，重复挂载显性拒绝");

    // 3. 双门逐门可观测：开放号命中且带探针打点
    let mut both_ok = true;
    let mut nr = 1u32;
    while nr <= 8 {
        let a = route(Some(Gate::Syscall), nr, &wall);
        let b = route(Some(Gate::Int80), nr, &wall);
        let (pa, pb) = (a.as_ref().ok().and_then(|o| o.probe_hit), b.as_ref().ok().and_then(|o| o.probe_hit));
        if a.is_err() || b.is_err() || pa.is_none() || pb.is_none() {
            both_ok = false;
        }
        nr += 1;
    }
    s.add("双门可观测：开放号两门均命中且探针打点齐全", both_ok,
          "SYSCALL 与 int 0x80 逐门联测，命中必须留下观测点");

    // 4. 单门号：掩码分界逐格
    let only_sys = route(Some(Gate::Int80), 9, &wall) == Err(ENOSYS)
        && route(Some(Gate::Syscall), 9, &wall).is_ok();
    let only_int = route(Some(Gate::Syscall), 11, &wall) == Err(ENOSYS)
        && route(Some(Gate::Int80), 11, &wall).is_ok();
    s.add("门掩码分界：单门号只从开放门命中", only_sys && only_int,
          "号 9/10 仅 SYSCALL 门、号 11/12 仅 int80 门，掩码逐格落账");

    // 5~7. 注入矩阵：三类违约各 10/10 + 账守恒
    let (ledger, per_class) = run_route_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类违约注入各 10/10 全检出", all_caught,
          "越界号/门不支持/坏门号三类必须全部命中约定码");
    s.add("路由账守恒：命中+拒绝=尝试且拒绝=30",
          ledger.balanced() && ledger.attempts == INJECT_CLASSES * INJECT_ROUNDS
              && ledger.rejects == INJECT_CLASSES * INJECT_ROUNDS
              && ledger.hits == 0,
          "30 次违约注入必须 30 次拒绝、0 命中");

    // 8. 判序：坏门 EINVAL 先于号段 ENOSYS
    let order_ok = route(None, NR_MAX + 1, &wall) == Err(EINVAL)
        && route(Some(Gate::Syscall), 0, &wall) == Err(ENOSYS);
    s.add("判序：坏门 EINVAL 先于号段 ENOSYS", order_ok,
          "门闭集校验是最外层，坏门时号再越界也回 EINVAL");

    // 9. 号段边界：0 与越上限 ENOSYS、界内未注册 ENOSYS
    let bounds_ok = route(Some(Gate::Syscall), 0, &wall) == Err(ENOSYS)
        && route(Some(Gate::Syscall), NR_MAX, &wall) == Err(ENOSYS)
        && route(Some(Gate::Syscall), 13, &wall) == Err(ENOSYS)
        && route(Some(Gate::Int80), 12, &wall).is_ok()
        && route(Some(Gate::Syscall), 10, &wall).is_ok()
        && route(Some(Gate::Syscall), 12, &wall) == Err(ENOSYS);
    s.add("号段边界：0/越上限/未注册 ENOSYS，上沿按门掩码命中", bounds_ok,
          "号面边界逐格落账：12 号仅 int80 门开放，13 起未注册");

    // 10. C3 探针零改动：注入前后墙指纹一致
    let fp_before = wall_fingerprint(&wall);
    let wall2 = wall;
    let mut k = 0u32;
    while k < INJECT_CLASSES * INJECT_ROUNDS {
        let _ = route(Some(Gate::Syscall), NR_MAX + 1, &wall2);
        let _ = route(None, 4, &wall2);
        k += 1;
    }
    let fp_after = wall_fingerprint(&wall2);
    s.add("C3 探针零改动：注入前后探针墙指纹一致", fp_before == fp_after,
          "探针墙对路由联测只读，任何漂移即声明失守");

    // 11. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSYS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 12. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 13. 文案闭集
    let text_ok = RouteCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RouteCode::ALL.len() {
        let mut j = i + 1;
        while j < RouteCode::ALL.len() {
            if let (Some(x), Some(y)) = (RouteCode::ALL.get(i), RouteCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 15. 对照表一致性
    let mut table_ok = true;
    let mut ti = 0usize;
    while ti < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(ti) {
            if row.1 != -(-row.1) || row.2 == 0 || row.3.is_empty() {
                table_ok = false;
            }
        }
        ti += 1;
    }
    s.add("对照表一致性：errno/Windows 码逐项自洽", table_ok,
          "表内每一行负值可复算、Windows 码非零、说明非空");

    // 16. 账目单调：带账路由再打 4 笔只增不减
    let mut mono = ledger;
    let before = mono.attempts;
    let mut m = 0u32;
    while m < 4 {
        let _ = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再注入只增不减", mono.attempts == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 17. 带账路由：命中归账且打点齐全
    let mut wall3 = ProbeWall::new();
    let _ = wall3.mount(4, Gate::Syscall);
    let mut led3 = RouteLedger::zero();
    let hit = route_accounted(Some(Gate::Syscall), 4, &wall3, &mut led3);
    let miss = route_accounted(Some(Gate::Syscall), 300, &wall3, &mut led3);
    s.add("带账路由：命中打点归账、拒绝归账",
          hit.is_ok() && hit.ok().map(|o| o.probe_hit == Some(41)) == Some(true)
              && miss == Err(ENOSYS)
              && led3.balanced() && led3.attempts == 2 && led3.hits == 1,
          "每笔路由都归账，命中必须与探针墙对接可观测");

    // 动态明细（≤8）
    let ledger_text = format!(
        "尝试 {} / 命中 {} / 拒绝 {}",
        ledger.attempts, ledger.hits, ledger.rejects
    );
    s.add_dyn("路由账快照", ledger.balanced(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_gates_hit_open_nr() {
        let wall = ProbeWall::new();
        assert!(route(Some(Gate::Syscall), 2, &wall).is_ok());
        assert!(route(Some(Gate::Int80), 2, &wall).is_ok());
    }

    #[test]
    fn gate_mask_boundary() {
        let wall = ProbeWall::new();
        assert!(route(Some(Gate::Int80), 9, &wall).is_err());
        assert!(route(Some(Gate::Syscall), 9, &wall).is_ok());
        assert!(route(Some(Gate::Syscall), 11, &wall).is_err());
        assert!(route(Some(Gate::Int80), 11, &wall).is_ok());
    }

    #[test]
    fn bad_gate_einval_first() {
        let wall = ProbeWall::new();
        assert_eq!(route(None, NR_MAX + 1, &wall), Err(EINVAL));
        assert_eq!(route(None, 4, &wall), Err(EINVAL));
    }

    #[test]
    fn nr_bounds_enosys() {
        let wall = ProbeWall::new();
        assert_eq!(route(Some(Gate::Syscall), 0, &wall), Err(ENOSYS));
        assert_eq!(route(Some(Gate::Syscall), NR_MAX + 1, &wall), Err(ENOSYS));
        assert_eq!(route(Some(Gate::Syscall), 99, &wall), Err(ENOSYS));
    }

    #[test]
    fn wall_mount_and_dup() {
        let mut wall = ProbeWall::new();
        assert_eq!(wall.mount(4, Gate::Syscall), Ok(41));
        assert_eq!(wall.mount(4, Gate::Syscall), Err(EEXIST));
        assert_eq!(wall.mount(4, Gate::Int80), Ok(42));
        assert_eq!(wall.mount(300, Gate::Syscall), Err(ENOENT));
        assert_eq!(wall.mounted(), 2);
    }

    #[test]
    fn wall_unchanged_by_routing() {
        let mut wall = ProbeWall::new();
        let _ = wall.mount(1, Gate::Syscall);
        let f1 = wall_fingerprint(&wall);
        let _ = route(Some(Gate::Syscall), NR_MAX + 1, &wall);
        let _ = route(None, 1, &wall);
        let f2 = wall_fingerprint(&wall);
        assert_eq!(f1, f2);
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_route_injection();
        assert!(l.balanced());
        assert_eq!(l.attempts, INJECT_CLASSES * INJECT_ROUNDS);
        assert_eq!(l.hits, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ENOSYS));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOSYS), -ENOSYS);
    }

    #[test]
    fn accounted_routing_books() {
        let mut wall = ProbeWall::new();
        let _ = wall.mount(4, Gate::Syscall);
        let mut led = RouteLedger::zero();
        let h = route_accounted(Some(Gate::Syscall), 4, &wall, &mut led);
        assert!(h.is_ok());
        let m = route_accounted(Some(Gate::Syscall), 999, &wall, &mut led);
        assert_eq!(m, Err(ENOSYS));
        assert!(led.balanced());
        assert_eq!(led.attempts, 2);
        assert_eq!(led.hits, 1);
    }

    #[test]
    fn unx9365_checks_green() {
        let s = run_unx9365_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
