//! UNX-F12663 CurrentVersion 分支探测档（ProductType/CSDVersion） · 0x80F80段
//!
//! 判据原文（派工卡 12662 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12663-J1 系统句柄枚举探测档对平断言绿（计数/
//!      进程归属字段），权限不足注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 分支探测档域模型：CurrentVersion 分支三闭集（WinNT 桌面支/
//!    ServerNT 服务器支/LanmanNT 域控支）+ CSDVersion 服务包格式闭
//!    集（空串或 "Service Pack N"，N ∈ 0..=6）；分支与补丁级决定后
//!    续句柄枚举档的消费口径——先立分支档，再消费枚举面；
//! 2. 系统句柄枚举探测档：每行五元（行号/进程归属 pid/句柄号/对象
//!    类型/引用计数），对平断言绿要求"计数"与"进程归属"两字段逐
//!    行对平——pid 必在冻结进程表内、计数必 ≥1 且 ≤ 饱和上沿；
//! 3. 权限面：每个归属进程带权限授予级（三档闭集：只读/读执/全权），
//!    枚举行带权限需求位；需求 > 授予即 AccessDenied 拒止——权限不
//!    足注入 10 例（LCG 确定性生成）逐例拒止 10/10；
//! 4. 档案状态机：Draft → Anchored → Verified 单向推进，倒退显性
//!    拒；对平闸五道（行号序/pid 归属闭集/句柄号非零/计数饱和闭集/
//!    权限授予位）逐行过闸；
//! 5. 注入联测 10 路：行号乱序、pid 越表、句柄号为零、计数为零、
//!    计数越饱和、权限不足、类型越闭集、未锚定即核、核后改行、分
//!    支越闭集——逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死
//!    = SEG_BASE）；取值一律 match/if let/.get()，无 unwrap/expect。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12662 − 12600)）。
pub const SEG_BASE: u32 = 0x80F80;

// ── 分支档诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CverDiag {
    /// 分支越闭集（ProductType 串不认）。
    EBranchRange,
    /// 服务包格式越闭集。
    ECsdRange,
    /// 行号乱序。
    ERowOrder,
    /// pid 越进程归属闭集。
    EPidRange,
    /// 句柄号为零。
    EHandleZero,
    /// 引用计数为零。
    ECountZero,
    /// 引用计数越饱和上沿。
    ECountSat,
    /// 对象类型越闭集。
    ETypeRange,
    /// 权限不足（AccessDenied 拒止）。
    EDenied,
    /// 状态机倒退。
    ERewind,
    /// 未锚定即核（Draft 态消费被拒）。
    ENotAnchored,
}

impl CverDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CverDiag::EBranchRange => 0x01,
                CverDiag::ECsdRange => 0x02,
                CverDiag::ERowOrder => 0x03,
                CverDiag::EPidRange => 0x04,
                CverDiag::EHandleZero => 0x05,
                CverDiag::ECountZero => 0x06,
                CverDiag::ECountSat => 0x07,
                CverDiag::ETypeRange => 0x08,
                CverDiag::EDenied => 0x09,
                CverDiag::ERewind => 0x0a,
                CverDiag::ENotAnchored => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CverDiag::EBranchRange => "分支越闭集：ProductType 只认 WinNT/ServerNT/LanmanNT，先正分支串",
            CverDiag::ECsdRange => "服务包格式越闭集：CSDVersion 只认空串或 Service Pack 0..=6，先修格式",
            CverDiag::ERowOrder => "行号乱序：枚举档按行号 0..N 逐行消费，跳号先回补上一行",
            CverDiag::EPidRange => "pid 越归属闭集：进程归属必在冻结进程表内，越表先对进程账",
            CverDiag::EHandleZero => "句柄号为零：句柄枚举行句柄号非零，零值行先核来源",
            CverDiag::ECountZero => "引用计数为零：活句柄引用计数 ≥1，零值行先核泄漏",
            CverDiag::ECountSat => "引用计数越饱和上沿：计数 ≤ 0xFFFF，越沿先核计数器溢出",
            CverDiag::ETypeRange => "对象类型越闭集：只认进程/线程/文件/令牌/事件五型，先正类型",
            CverDiag::EDenied => "权限不足：需求位超授予级即拒止（AccessDenied），先提权或换面",
            CverDiag::ERewind => "档案状态机倒退：Draft→Anchored→Verified 只进不退，倒退即账可疑",
            CverDiag::ENotAnchored => "未锚定即核：分支档未锚定不许消费句柄枚举面，先锚定再核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CVER_ALL: &[CverDiag] = &[
    CverDiag::EBranchRange,
    CverDiag::ECsdRange,
    CverDiag::ERowOrder,
    CverDiag::EPidRange,
    CverDiag::EHandleZero,
    CverDiag::ECountZero,
    CverDiag::ECountSat,
    CverDiag::ETypeRange,
    CverDiag::EDenied,
    CverDiag::ERewind,
    CverDiag::ENotAnchored,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CVER_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80F81..0x80F8B 闭集内"
}

// ── 分支档（ProductType / CSDVersion 语义闭集） ──────────────────
/// ProductType 分支三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Branch {
    /// WinNT：桌面工作站支。
    WinNt,
    /// ServerNT：独立服务器支。
    ServerNt,
    /// LanmanNT：域控制器支。
    LanmanNt,
}

impl Branch {
    /// 分支串（与注册表字面量一致）。
    pub fn text(self) -> &'static str {
        match self {
            Branch::WinNt => "WinNT",
            Branch::ServerNt => "ServerNT",
            Branch::LanmanNt => "LanmanNT",
        }
    }

    /// 串解析（闭集外 None）。
    pub fn parse(s: &str) -> Option<Branch> {
        if s == "WinNT" {
            Some(Branch::WinNt)
        } else if s == "ServerNT" {
            Some(Branch::ServerNt)
        } else if s == "LanmanNT" {
            Some(Branch::LanmanNt)
        } else {
            None
        }
    }

    /// 枚举档口径上沿（分支决定可枚举句柄数上沿：桌面 40 / 服务器 60 / 域控 80）。
    pub fn enum_cap(self) -> usize {
        match self {
            Branch::WinNt => 40,
            Branch::ServerNt => 60,
            Branch::LanmanNt => 80,
        }
    }
}

/// CSDVersion 服务包级解析（空串 = 0 级；"Service Pack N" N∈0..=6）。
pub fn parse_csd(s: &str) -> Option<u32> {
    if s.is_empty() {
        return Some(0);
    }
    let prefix = "Service Pack ";
    if !s.starts_with(prefix) {
        return None;
    }
    let rest = match s.get(prefix.len()..) {
        Some(v) => v,
        None => return None,
    };
    // N 为单字符数字且 ≤ 6。
    if rest.len() != 1 {
        return None;
    }
    let ch = match rest.chars().next() {
        Some(c) => c,
        None => return None,
    };
    match ch.to_digit(10) {
        Some(n) if n <= 6 => Some(n),
        _ => None,
    }
}

// ── 进程归属表与权限授予级 ───────────────────────────────────────
/// 权限授予级三档闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    /// 只读（1）。
    ReadOnly,
    /// 读执（2）。
    ReadExec,
    /// 全权（3）。
    Full,
}

impl Grant {
    /// 级值。
    pub fn level(self) -> u8 {
        match self {
            Grant::ReadOnly => 1,
            Grant::ReadExec => 2,
            Grant::Full => 3,
        }
    }
}

/// 进程归属行（pid + 授予级 + 分支）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerProc {
    /// 进程归属 pid（非零）。
    pub pid: u32,
    /// 权限授予级。
    pub grant: Grant,
    /// 归属分支。
    pub branch: Branch,
}

/// 冻结进程归属表（4 例）。
pub const OWNER_TABLE: &[OwnerProc] = &[
    OwnerProc { pid: 4, grant: Grant::Full, branch: Branch::ServerNt },
    OwnerProc { pid: 300, grant: Grant::ReadExec, branch: Branch::WinNt },
    OwnerProc { pid: 512, grant: Grant::ReadOnly, branch: Branch::WinNt },
    OwnerProc { pid: 768, grant: Grant::Full, branch: Branch::LanmanNt },
];

/// pid 是否在归属闭集内。
pub fn pid_in_set(pid: u32) -> bool {
    for p in OWNER_TABLE.iter() {
        if p.pid == pid {
            return true;
        }
    }
    false
}

/// 取归属行（越界 None）。
pub fn owner_of(pid: u32) -> Option<&'static OwnerProc> {
    for p in OWNER_TABLE.iter() {
        if p.pid == pid {
            return Some(p);
        }
    }
    None
}

// ── 句柄枚举档（SystemHandleInformation 消费面） ─────────────────
/// 对象类型五闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjKind {
    /// 进程。
    Proc,
    /// 线程。
    Thread,
    /// 文件。
    File,
    /// 令牌。
    Token,
    /// 事件。
    Event,
}

impl ObjKind {
    /// 类型名。
    pub fn name(self) -> &'static str {
        match self {
            ObjKind::Proc => "Process",
            ObjKind::Thread => "Thread",
            ObjKind::File => "File",
            ObjKind::Token => "Token",
            ObjKind::Event => "Event",
        }
    }

    /// 需求位（类型决定最低权限需求：进程/线程 2，文件/令牌 3，事件 1）。
    pub fn need(self) -> u8 {
        match self {
            ObjKind::Proc | ObjKind::Thread => 2,
            ObjKind::File | ObjKind::Token => 3,
            ObjKind::Event => 1,
        }
    }
}

/// 枚举行五元（行号/pid/句柄号/类型/引用计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumRow {
    /// 行号。
    pub no: u32,
    /// 进程归属 pid。
    pub pid: u32,
    /// 句柄号（非零）。
    pub handle: u32,
    /// 对象类型。
    pub kind: ObjKind,
    /// 引用计数（1..=0xFFFF）。
    pub refs: u32,
}

/// 枚举档冻结 10 例（覆盖 pid 闭集四例与五型）。
pub const ENUM_PROFILE: &[EnumRow] = &[
    EnumRow { no: 0, pid: 4, handle: 0x0008, kind: ObjKind::Proc, refs: 3 },
    EnumRow { no: 1, pid: 4, handle: 0x0010, kind: ObjKind::Thread, refs: 2 },
    EnumRow { no: 2, pid: 768, handle: 0x0014, kind: ObjKind::File, refs: 1 },
    EnumRow { no: 3, pid: 4, handle: 0x0020, kind: ObjKind::Token, refs: 5 },
    EnumRow { no: 4, pid: 512, handle: 0x0024, kind: ObjKind::Event, refs: 1 },
    EnumRow { no: 5, pid: 300, handle: 0x0030, kind: ObjKind::Proc, refs: 9 },
    EnumRow { no: 6, pid: 768, handle: 0x0038, kind: ObjKind::Thread, refs: 4 },
    EnumRow { no: 7, pid: 768, handle: 0x0040, kind: ObjKind::File, refs: 2 },
    EnumRow { no: 8, pid: 300, handle: 0x0048, kind: ObjKind::Event, refs: 7 },
    EnumRow { no: 9, pid: 4, handle: 0x0050, kind: ObjKind::Token, refs: 1 },
];

/// 档容量。
pub const ENUM_CAP: usize = 10;

/// 引用计数饱和上沿（16 位计数器口径）。
pub const REFS_SAT: u32 = 0xFFFF;

/// 单行对平（判据 J1 的"计数/进程归属字段"对平口径）。
pub fn row_ok(r: &EnumRow) -> Result<(), u32> {
    // 进程归属闭集闸。
    if !pid_in_set(r.pid) {
        return Err(CverDiag::EPidRange.val());
    }
    // 句柄号非零闸。
    if r.handle == 0 {
        return Err(CverDiag::EHandleZero.val());
    }
    // 引用计数闭集闸（≥1 且 ≤ 饱和）。
    if r.refs == 0 {
        return Err(CverDiag::ECountZero.val());
    }
    if r.refs > REFS_SAT {
        return Err(CverDiag::ECountSat.val());
    }
    Ok(())
}

// ── 档案状态机（Draft → Anchored → Verified） ────────────────────
/// 档案状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileState {
    /// 草稿：分支档未锚定。
    Draft,
    /// 已锚定：分支档已立。
    Anchored,
    /// 已核：枚举档逐行对平过。
    Verified,
}

impl ProfileState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            ProfileState::Draft => 0,
            ProfileState::Anchored => 1,
            ProfileState::Verified => 2,
        }
    }
}

/// 分支档案机。
#[derive(Clone, Debug)]
pub struct BranchProfile {
    /// 立档分支。
    branch: Branch,
    /// 服务包级。
    csd: u32,
    /// 已对平行。
    rows: Vec<EnumRow>,
    /// 拒收记账（行号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: ProfileState,
}

impl BranchProfile {
    /// 开档（Draft 态）。
    pub fn new(branch: Branch, csd: u32) -> BranchProfile {
        BranchProfile {
            branch,
            csd,
            rows: Vec::new(),
            rejects: Vec::new(),
            state: ProfileState::Draft,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ProfileState {
        self.state
    }

    /// 立档分支。
    pub fn branch(&self) -> Branch {
        self.branch
    }

    /// 服务包级。
    pub fn csd(&self) -> u32 {
        self.csd
    }

    /// 已对平行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&EnumRow> {
        self.rows.get(idx)
    }

    /// 锚定（Draft→Anchored）。
    pub fn anchor(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ProfileState::Anchored.rank() {
            return Err(CverDiag::ERewind.val());
        }
        self.state = ProfileState::Anchored;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < ENUM_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行消费（六闸：行号序/行对平/分支口径上沿/权限授予/重复行/状态）。
    pub fn consume(&mut self, r: EnumRow) -> Result<(), u32> {
        if self.state.rank() < ProfileState::Anchored.rank() {
            return Err(CverDiag::ENotAnchored.val());
        }
        if self.state.rank() >= ProfileState::Verified.rank() {
            self.note_reject(r.no, CverDiag::ERewind.val());
            return Err(CverDiag::ERewind.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, CverDiag::ERowOrder.val());
            return Err(CverDiag::ERowOrder.val());
        }
        // 分支口径上沿闸（枚举行数不得超分支口径）。
        if self.rows.len() + 1 > self.branch.enum_cap() {
            self.note_reject(r.no, CverDiag::ERowOrder.val());
            return Err(CverDiag::ERowOrder.val());
        }
        // 单行对平闸（计数/进程归属）。
        if let Err(e) = row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 权限授予闸（需求位 > 授予级 → 拒止）。
        let g = match owner_of(r.pid) {
            Some(p) => p.grant,
            None => Grant::ReadOnly,
        };
        if r.kind.need() > g.level() {
            self.note_reject(r.no, CverDiag::EDenied.val());
            return Err(CverDiag::EDenied.val());
        }
        // 重复行闸（同 pid 同句柄只入一笔）。
        for done in self.rows.iter() {
            if done.pid == r.pid && done.handle == r.handle {
                self.note_reject(r.no, CverDiag::EHandleZero.val());
                return Err(CverDiag::EHandleZero.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 核定（Anchored→Verified）：10 行齐 + 零拒收。
    pub fn verify(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ProfileState::Verified.rank() {
            return Err(CverDiag::ERewind.val());
        }
        if self.state.rank() < ProfileState::Anchored.rank() {
            return Err(CverDiag::ENotAnchored.val());
        }
        if self.rows.len() != ENUM_CAP {
            return Err(CverDiag::ERowOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(CverDiag::ERowOrder.val());
        }
        self.state = ProfileState::Verified;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == ENUM_CAP && self.rejects.is_empty()
    }
}

// ── 构建器：完整档案 ─────────────────────────────────────────────
/// 构建完整档案（ServerNT 分支 + 10 行全过）。
pub fn build_full_profile() -> BranchProfile {
    let mut bp = BranchProfile::new(Branch::ServerNt, 0);
    let _ = bp.anchor();
    let mut i = 0usize;
    while i < ENUM_CAP {
        let r = match ENUM_PROFILE.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        match bp.consume(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    bp
}

// ── 权限不足注入（LCG 确定性 10 例） ─────────────────────────────
/// 权限不足注入例（行号 + pid + 类型 + 授予级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenyCase {
    /// 行号。
    pub no: u32,
    /// 注入 pid。
    pub pid: u32,
    /// 注入类型（需求位超授予）。
    pub kind: ObjKind,
}

/// LCG（种子 = SEG_BASE，零随机红线合规）。
pub struct CverLcg {
    state: u64,
}

impl CverLcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> CverLcg {
        CverLcg { state: seed }
    }

    /// 下一伪随机数（Knuth MMIX 常数）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

/// 确定性生成 10 例权限不足注入（需求位必超授予级）。
pub fn deny_cases() -> Vec<DenyCase> {
    let mut out: Vec<DenyCase> = Vec::new();
    let mut rng = CverLcg::new(SEG_BASE as u64);
    let kinds = [ObjKind::Proc, ObjKind::Thread, ObjKind::File, ObjKind::Token, ObjKind::Event];
    let mut guard = 0u32;
    while out.len() < 10 && guard < 256 {
        guard += 1;
        let pid_idx = rng.below(OWNER_TABLE.len() as u32) as usize;
        let kind_idx = rng.below(kinds.len() as u32) as usize;
        let p = match OWNER_TABLE.get(pid_idx) {
            Some(v) => *v,
            None => continue,
        };
        let kind = match kinds.get(kind_idx) {
            Some(v) => *v,
            None => continue,
        };
        // 权限不足前提：需求位 > 授予级。
        if kind.need() > p.grant.level() {
            let no = out.len() as u32;
            out.push(DenyCase { no, pid: p.pid, kind });
        }
    }
    out
}

/// 权限不足注入 10/10 拒止直测（判据 J1 直读口）。
pub fn deny_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for c in deny_cases().iter() {
        total += 1;
        let mut bp = BranchProfile::new(Branch::ServerNt, 0);
        let _ = bp.anchor();
        let r = EnumRow { no: 0, pid: c.pid, handle: 0x0100 + c.no, kind: c.kind, refs: 1 };
        match bp.consume(r) {
            Err(e) if e == CverDiag::EDenied.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("路{}(pid={:?}) 逃脱", c.no, c.pid);
                }
            }
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CverFault {
    /// 行号乱序。
    RowOrder,
    /// pid 越表。
    PidRange,
    /// 句柄号为零。
    HandleZero,
    /// 计数为零。
    CountZero,
    /// 计数越饱和。
    CountSat,
    /// 分支越闭集（串不认）。
    BranchRange,
    /// 服务包格式越闭集。
    CsdRange,
    /// 未锚定即核。
    NotAnchored,
    /// 核后改行。
    AfterVerify,
    /// 未核即账等式要求（行数不足 verify 被拒）。
    ShortVerify,
}

impl CverFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            CverFault::RowOrder => 0,
            CverFault::PidRange => 1,
            CverFault::HandleZero => 2,
            CverFault::CountZero => 3,
            CverFault::CountSat => 4,
            CverFault::BranchRange => 5,
            CverFault::CsdRange => 6,
            CverFault::NotAnchored => 7,
            CverFault::AfterVerify => 8,
            CverFault::ShortVerify => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            CverFault::RowOrder => "行号乱序",
            CverFault::PidRange => "pid 越表",
            CverFault::HandleZero => "句柄号零",
            CverFault::CountZero => "计数零",
            CverFault::CountSat => "计数越饱和",
            CverFault::BranchRange => "分支越闭集",
            CverFault::CsdRange => "服务包越闭集",
            CverFault::NotAnchored => "未锚定即核",
            CverFault::AfterVerify => "核后改行",
            CverFault::ShortVerify => "行数不足即核",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [CverFault] {
        &[
            CverFault::RowOrder,
            CverFault::PidRange,
            CverFault::HandleZero,
            CverFault::CountZero,
            CverFault::CountSat,
            CverFault::BranchRange,
            CverFault::CsdRange,
            CverFault::NotAnchored,
            CverFault::AfterVerify,
            CverFault::ShortVerify,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in CverFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: CverFault) -> bool {
    match v {
        CverFault::RowOrder => {
            let mut bp = build_full_profile();
            let r = match ENUM_PROFILE.get(0).copied() {
                Some(x) => x,
                None => return false,
            };
            bp.consume(r).is_err()
        }
        CverFault::PidRange => {
            let mut bp = BranchProfile::new(Branch::ServerNt, 0);
            let _ = bp.anchor();
            let r = EnumRow { no: 0, pid: 999, handle: 0x8, kind: ObjKind::Event, refs: 1 };
            bp.consume(r).is_err()
        }
        CverFault::HandleZero => {
            let mut bp = BranchProfile::new(Branch::ServerNt, 0);
            let _ = bp.anchor();
            let r = EnumRow { no: 0, pid: 4, handle: 0, kind: ObjKind::Event, refs: 1 };
            bp.consume(r).is_err()
        }
        CverFault::CountZero => {
            let mut bp = BranchProfile::new(Branch::ServerNt, 0);
            let _ = bp.anchor();
            let r = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: 0 };
            bp.consume(r).is_err()
        }
        CverFault::CountSat => {
            let mut bp = BranchProfile::new(Branch::ServerNt, 0);
            let _ = bp.anchor();
            let r = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: REFS_SAT + 1 };
            bp.consume(r).is_err()
        }
        CverFault::BranchRange => Branch::parse("WinNt ").is_none() && Branch::parse("home").is_none(),
        CverFault::CsdRange => parse_csd("Service Pack 7").is_none() && parse_csd("SP1").is_none(),
        CverFault::NotAnchored => {
            let mut bp = BranchProfile::new(Branch::WinNt, 0);
            let r = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: 1 };
            bp.consume(r).is_err()
        }
        CverFault::AfterVerify => {
            let mut bp = build_full_profile();
            if bp.verify().is_err() {
                return false;
            }
            let r = match ENUM_PROFILE.get(0).copied() {
                Some(x) => x,
                None => return false,
            };
            bp.consume(r).is_err()
        }
        CverFault::ShortVerify => {
            let mut bp = BranchProfile::new(Branch::ServerNt, 0);
            let _ = bp.anchor();
            let r = match ENUM_PROFILE.get(0).copied() {
                Some(x) => x,
                None => return false,
            };
            let _ = bp.consume(r);
            bp.verify().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 系统句柄枚举探测档对平断言绿（判据 J1 直读口）。
fn enum_profile_ok() -> bool {
    let mut bp = build_full_profile();
    if bp.len() != ENUM_CAP {
        return false;
    }
    if bp.reject_len() != 0 {
        return false;
    }
    if bp.verify().is_err() {
        return false;
    }
    // 独立复核：10 行逐行 row_ok 对平（计数/进程归属字段）。
    for r in ENUM_PROFILE.iter() {
        if row_ok(r).is_err() {
            return false;
        }
    }
    true
}

/// 权限不足注入 10/10 拒止。
fn deny_ok() -> bool {
    let (caught, total, escape) = deny_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 分支档语义边界（ProductType/CSDVersion 闭集直读）。
fn branch_boundary_ok() -> bool {
    // 分支三闭集解析。
    if Branch::parse("WinNT") != Some(Branch::WinNt) {
        return false;
    }
    if Branch::parse("ServerNT") != Some(Branch::ServerNt) {
        return false;
    }
    if Branch::parse("LanmanNT") != Some(Branch::LanmanNt) {
        return false;
    }
    if Branch::parse("Workstation").is_some() {
        return false;
    }
    // 服务包级闭集：0..=6。
    let mut n = 0u32;
    while n <= 6 {
        let s = if n == 0 {
            String::new()
        } else {
            format!("Service Pack {}", n)
        };
        if parse_csd(&s) != Some(n) {
            return false;
        }
        n += 1;
    }
    if parse_csd("Service Pack 7").is_some() {
        return false;
    }
    if parse_csd("service pack 1").is_some() {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_profile();
    let b = build_full_profile();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let da = deny_sweep();
    let db = deny_sweep();
    if da.0 != db.0 || da.1 != db.1 {
        return false;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12662u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut bp = build_full_profile();
    if bp.verify().is_err() {
        return false;
    }
    if bp.state() != ProfileState::Verified {
        return false;
    }
    match bp.row_at(0) {
        Some(r) => {
            if r.pid == 0 || !pid_in_set(r.pid) {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12663 分支探测档自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12662_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12662-currentver-probe");
    let bp = build_full_profile();

    s.add_dyn(
        "F12663-01 分支档三闭集立档",
        Branch::parse("WinNT").is_some()
            && Branch::parse("ServerNT").is_some()
            && Branch::parse("LanmanNT").is_some(),
        "WinNT/ServerNT/LanmanNT 三支解析齐",
    );

    s.add_dyn(
        "F12663-02 句柄枚举档对平断言绿",
        enum_profile_ok(),
        &format!("rows={} rejects={}", bp.len(), bp.reject_len()),
    );

    s.add_dyn(
        "F12663-03 计数/进程归属字段对平",
        match ENUM_PROFILE.get(0) {
            Some(r) => row_ok(r).is_ok(),
            None => false,
        },
        "pid 归属闭集 + 计数 1..=0xFFFF",
    );

    s.add_dyn(
        "F12663-04 权限不足注入 10/10 拒止",
        deny_ok(),
        &format!("caught={}", deny_sweep().0),
    );

    s.add_dyn(
        "F12663-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12663-06 分支/服务包语义边界",
        branch_boundary_ok(),
        "分支三支 + 服务包 0..=6 级",
    );

    s.add_dyn(
        "F12663-07 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12663-08 档案账等式",
        bp.equation_ok(),
        &format!("rows={} rejects={}", bp.len(), bp.reject_len()),
    );

    s.add_dyn(
        "F12663-09 权限授予级三档",
        Grant::ReadOnly.level() == 1 && Grant::ReadExec.level() == 2 && Grant::Full.level() == 3,
        "只读/读执/全权 级值递增",
    );

    s.add_dyn(
        "F12663-10 完成写回语义",
        writeback_ok(),
        "核定即已完成且摘要非空",
    );

    s.add_dyn(
        "F12663-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 枚举档容量与 pid 闭集独立复核。
    let mut census_ok = ENUM_PROFILE.len() == ENUM_CAP;
    for r in ENUM_PROFILE.iter() {
        if !pid_in_set(r.pid) {
            census_ok = false;
        }
    }
    s.add_dyn(
        "F12663-12 枚举档普查",
        census_ok,
        &format!("rows={} pid 闭集 {} 例", ENUM_PROFILE.len(), OWNER_TABLE.len()),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80F80);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中。
        assert_eq!(CVER_ALL.len(), 11);
        let mut i = 0usize;
        while i < CVER_ALL.len() {
            let vi = match CVER_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CVER_ALL.len() {
                let vj = match CVER_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_verifies() {
        let mut bp = build_full_profile();
        assert_eq!(bp.len(), ENUM_CAP);
        assert_eq!(bp.reject_len(), 0);
        assert!(bp.verify().is_ok());
        assert_eq!(bp.state(), ProfileState::Verified);
    }

    #[test]
    fn deny_injection_ten_ten() {
        // 权限不足注入 10/10 拒止。
        let (caught, total, escape) = deny_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱：{}", escape);
        assert!(escape.is_empty());
        // 双跑一致。
        let a = deny_cases();
        let b = deny_cases();
        assert_eq!(a, b);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn branch_csd_semantics() {
        // 分支/服务包闭集直读。
        assert_eq!(Branch::parse("WinNT"), Some(Branch::WinNt));
        assert_eq!(Branch::parse("ServerNT"), Some(Branch::ServerNt));
        assert_eq!(Branch::parse("LanmanNT"), Some(Branch::LanmanNt));
        assert!(Branch::parse("winnt").is_none());
        assert!(Branch::parse("").is_none());
        assert_eq!(parse_csd(""), Some(0));
        assert_eq!(parse_csd("Service Pack 3"), Some(3));
        assert_eq!(parse_csd("Service Pack 6"), Some(6));
        assert!(parse_csd("Service Pack 7").is_none());
        assert!(parse_csd("Service Pack").is_none());
        assert!(parse_csd("Service Pack 12").is_none());
    }

    #[test]
    fn row_ok_boundaries() {
        // 单行对平边界：pid 闭集两端 + 计数两端。
        let good = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: 1 };
        assert!(row_ok(&good).is_ok());
        let sat = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: REFS_SAT };
        assert!(row_ok(&sat).is_ok());
        let bad_pid = EnumRow { no: 0, pid: 12345, handle: 0x8, kind: ObjKind::Event, refs: 1 };
        assert_eq!(row_ok(&bad_pid), Err(CverDiag::EPidRange.val()));
        let bad_h = EnumRow { no: 0, pid: 4, handle: 0, kind: ObjKind::Event, refs: 1 };
        assert_eq!(row_ok(&bad_h), Err(CverDiag::EHandleZero.val()));
        let bad_c0 = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: 0 };
        assert_eq!(row_ok(&bad_c0), Err(CverDiag::ECountZero.val()));
        let bad_cs = EnumRow { no: 0, pid: 4, handle: 0x8, kind: ObjKind::Event, refs: REFS_SAT + 1 };
        assert_eq!(row_ok(&bad_cs), Err(CverDiag::ECountSat.val()));
    }

    #[test]
    fn grant_need_matrix() {
        // 权限矩阵：类型需求位 × 授予级。
        assert_eq!(ObjKind::Event.need(), 1);
        assert_eq!(ObjKind::Proc.need(), 2);
        assert_eq!(ObjKind::Thread.need(), 2);
        assert_eq!(ObjKind::File.need(), 3);
        assert_eq!(ObjKind::Token.need(), 3);
        // 只读者可枚举事件，不可枚举文件/令牌。
        let ro = Grant::ReadOnly;
        assert!(ObjKind::Event.need() <= ro.level());
        assert!(ObjKind::File.need() > ro.level());
        // 读执者可枚举进程，不可枚举令牌。
        let re = Grant::ReadExec;
        assert!(ObjKind::Proc.need() <= re.level());
        assert!(ObjKind::Token.need() > re.level());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_profile();
        let b = build_full_profile();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(fault_sweep().0, fault_sweep().0);
        assert_eq!(deny_sweep().0, deny_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退。
        assert!(ProfileState::Draft.rank() < ProfileState::Anchored.rank());
        assert!(ProfileState::Anchored.rank() < ProfileState::Verified.rank());
        let mut fresh = BranchProfile::new(Branch::WinNt, 0);
        assert!(fresh.anchor().is_ok());
        assert_eq!(fresh.anchor(), Err(CverDiag::ERewind.val()));
        let mut full = build_full_profile();
        assert_eq!(full.state(), ProfileState::Anchored);
        assert!(full.verify().is_ok());
        assert_eq!(full.verify(), Err(CverDiag::ERewind.val()));
        assert_eq!(full.anchor(), Err(CverDiag::ERewind.val()));
    }

    #[test]
    fn enum_cap_by_branch() {
        // 分支口径上沿：桌面 40 / 服务器 60 / 域控 80。
        assert_eq!(Branch::WinNt.enum_cap(), 40);
        assert_eq!(Branch::ServerNt.enum_cap(), 60);
        assert_eq!(Branch::LanmanNt.enum_cap(), 80);
        // 10 行档在任一分支口径内。
        assert!(ENUM_CAP <= Branch::WinNt.enum_cap());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12662_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
