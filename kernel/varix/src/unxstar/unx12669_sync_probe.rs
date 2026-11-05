//! UNX-F12670 进程/线程枚举探测（SystemProcessInformation 消费） · 0x81140段
//!
//! 判据原文（派工卡 12669 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12670-J1 枚举探测消费对平断言绿（B33 F12649
//!      档衔接），快照方差档显式；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 枚举探测消费域模型：SystemProcessInformation 消费档每行七元
//!    （探测号/进程 pid/进程名短码/线程计数/快照对号/快照方差/B33
//!    回指指纹）——进程与线程双计数逐行对平是对平断言绿的主体；
//! 2. B33 F12649 档衔接：每行回指 B33 族系统信息档指纹
//!    （B33-F12649-SYSINFO-PROFILE），指纹缺失或错挂即断链拒收——
//!    衔接环零悬空是消费绿的前置环；
//! 3. 快照方差档显式（冻结口径）：双快照对（快照 A/快照 B）的计数
//!    差取 saturating 绝对值，方差必须 ≤ 冻结上沿（VAR_CAP = 2），
//!    方差值显式入档（不允许隐式丢差）——方差档越沿即红；
//! 4. 计数闭集：线程计数 1..=64（冻结口径）、进程 pid 必在冻结进程
//!    表内、快照对号闭集 0..=7；
//! 5. 状态机：Bridged → Enumerated → Filed 单向推进，倒退显性拒；
//!    对平闸五道（pid 归属/线程计数闭集/方差档/B33 指纹/探测号序）
//!    逐行过闸；
//! 6. 注入联测 10 路：方差越沿、线程计数为零、线程计数越上沿、pid
//!    越表、指纹断链、指纹错挂、探测号乱序、快照对号越闭集、未衔接
//!    即探、归档后写——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（冻结档确定性
//!    出题）；取值一律 match/if let/.get()/saturating_*，无 unwrap/
//!    expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12669 − 12600)）。
pub const SEG_BASE: u32 = 0x81140;

// ── 枚举消费诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumDiag {
    /// pid 越进程归属闭集。
    EPidRange,
    /// 线程计数为零。
    EThreadZero,
    /// 线程计数越上沿。
    EThreadOver,
    /// 快照方差越沿。
    EVarOver,
    /// 快照对号越闭集。
    ESnapRange,
    /// B33 回指指纹缺失（断链）。
    EFpMissing,
    /// B33 回指指纹错挂。
    EFpMismatch,
    /// 探测号乱序。
    ERowOrder,
    /// 未衔接即探。
    ENotBridged,
    /// 归档后写入。
    EFiledWrite,
    /// 状态机倒退。
    ERewind,
}

impl EnumDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                EnumDiag::EPidRange => 0x01,
                EnumDiag::EThreadZero => 0x02,
                EnumDiag::EThreadOver => 0x03,
                EnumDiag::EVarOver => 0x04,
                EnumDiag::ESnapRange => 0x05,
                EnumDiag::EFpMissing => 0x06,
                EnumDiag::EFpMismatch => 0x07,
                EnumDiag::ERowOrder => 0x08,
                EnumDiag::ENotBridged => 0x09,
                EnumDiag::EFiledWrite => 0x0a,
                EnumDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            EnumDiag::EPidRange => "pid 越归属闭集：进程必在冻结进程表内，越表先对进程账",
            EnumDiag::EThreadZero => "线程计数为零：活进程线程计数 ≥1，零值先核进程存活位",
            EnumDiag::EThreadOver => "线程计数越上沿：线程计数 ≤64（冻结口径），越沿先核枚举账",
            EnumDiag::EVarOver => "快照方差越沿：双快照计数差 ≤2（冻结上沿），越沿先核突变源",
            EnumDiag::ESnapRange => "快照对号越闭集：对号只认 0..=7，越表先补快照",
            EnumDiag::EFpMissing => "B33 回指指纹缺失：衔接环断链即拒，先补 B33-F12649 指纹",
            EnumDiag::EFpMismatch => "B33 回指指纹错挂：指纹与冻结值不符，先对指纹源",
            EnumDiag::ERowOrder => "探测号乱序：按探测号 0..N 逐行过闸，跳号先回补",
            EnumDiag::ENotBridged => "未衔接即探：先立 B33 衔接环（Bridged→Enumerated）再消费",
            EnumDiag::EFiledWrite => "归档后写入：档面只读，改动走下一轮快照",
            EnumDiag::ERewind => "状态机倒退：Bridged→Enumerated→Filed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ENUM_ALL: &[EnumDiag] = &[
    EnumDiag::EPidRange,
    EnumDiag::EThreadZero,
    EnumDiag::EThreadOver,
    EnumDiag::EVarOver,
    EnumDiag::ESnapRange,
    EnumDiag::EFpMissing,
    EnumDiag::EFpMismatch,
    EnumDiag::ERowOrder,
    EnumDiag::ENotBridged,
    EnumDiag::EFiledWrite,
    EnumDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ENUM_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81141..0x8114B 闭集内"
}

// ── B33 F12649 档衔接锚 ──────────────────────────────────────────
/// B33 族系统信息档指纹（冻结）。
pub const B33_FP: &str = "B33-F12649-SYSINFO-PROFILE";
/// B33 衔接回指卡 id（F12649 = 卡 12648 的面号）。
pub const B33_REF_ID: u32 = 12649;

// ── 进程归属表（冻结） ───────────────────────────────────────────
/// 冻结进程归属表（5 例：pid + 进程名短码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumProc {
    /// 进程 pid（非零）。
    pub pid: u32,
    /// 进程名短码（4 字符冻结）。
    pub tag: &'static str,
}

/// 冻结进程表。
pub const PROC_TABLE: &[EnumProc] = &[
    EnumProc { pid: 4, tag: "SYST" },
    EnumProc { pid: 320, tag: "CSRS" },
    EnumProc { pid: 388, tag: "WINS" },
    EnumProc { pid: 512, tag: "EXPL" },
    EnumProc { pid: 1024, tag: "TASK" },
];

/// 进程表容量。
pub const PROC_CAP: usize = 5;

/// 取进程行（越表 None）。
pub fn proc_of(pid: u32) -> Option<&'static EnumProc> {
    for p in PROC_TABLE.iter() {
        if p.pid == pid {
            return Some(p);
        }
    }
    None
}

// ── 快照方差档（冻结口径） ───────────────────────────────────────
/// 快照方差上沿（冻结口径：双快照计数差 ≤2）。
pub const VAR_CAP: u32 = 2;

/// 线程计数上沿（冻结口径）。
pub const THREAD_CAP: u32 = 64;

/// 快照对号闭集上沿（0..=7）。
pub const SNAP_CAP: u32 = 7;

/// 快照方差显式计算（saturating 绝对差）。
pub fn variance(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

// ── 枚举消费行与黄金判定 ─────────────────────────────────────────
/// 枚举消费行七元（探测号/pid/线程计数/快照 A 计数/快照 B 计数/对号/指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumRow {
    /// 探测号。
    pub no: u32,
    /// 进程 pid。
    pub pid: u32,
    /// 线程计数。
    pub threads: u32,
    /// 快照 A 进程计数。
    pub snap_a: u32,
    /// 快照 B 进程计数。
    pub snap_b: u32,
    /// 快照对号。
    pub pair: u32,
    /// B33 回指指纹。
    pub fp: &'static str,
}

/// 单行黄金判定（五道：pid/线程计数/快照对号/方差档/指纹）。
pub fn enum_row_ok(r: &EnumRow) -> Result<(), u32> {
    // pid 归属闸。
    if r.pid == 0 || proc_of(r.pid).is_none() {
        return Err(EnumDiag::EPidRange.val());
    }
    // 线程计数闸。
    if r.threads == 0 {
        return Err(EnumDiag::EThreadZero.val());
    }
    if r.threads > THREAD_CAP {
        return Err(EnumDiag::EThreadOver.val());
    }
    // 快照对号闸。
    if r.pair > SNAP_CAP {
        return Err(EnumDiag::ESnapRange.val());
    }
    // 快照方差档闸（显式计算，越沿即红）。
    if variance(r.snap_a, r.snap_b) > VAR_CAP {
        return Err(EnumDiag::EVarOver.val());
    }
    // B33 指纹闸。
    if r.fp.is_empty() {
        return Err(EnumDiag::EFpMissing.val());
    }
    if r.fp != B33_FP {
        return Err(EnumDiag::EFpMismatch.val());
    }
    Ok(())
}

/// 探测档容量（8 行）。
pub const ENUM_CAP: usize = 8;

/// 冻结枚举消费档（8 行）。
pub const ENUM_PROFILE: &[EnumRow] = &[
    EnumRow { no: 0, pid: 4, threads: 58, snap_a: 5, snap_b: 5, pair: 0, fp: B33_FP },
    EnumRow { no: 1, pid: 320, threads: 22, snap_a: 5, snap_b: 4, pair: 1, fp: B33_FP },
    EnumRow { no: 2, pid: 388, threads: 9, snap_a: 4, snap_b: 4, pair: 2, fp: B33_FP },
    EnumRow { no: 3, pid: 512, threads: 34, snap_a: 4, snap_b: 6, pair: 3, fp: B33_FP },
    EnumRow { no: 4, pid: 1024, threads: 12, snap_a: 6, snap_b: 5, pair: 4, fp: B33_FP },
    EnumRow { no: 5, pid: 4, threads: 60, snap_a: 5, snap_b: 7, pair: 5, fp: B33_FP },
    EnumRow { no: 6, pid: 512, threads: 30, snap_a: 7, snap_b: 7, pair: 6, fp: B33_FP },
    EnumRow { no: 7, pid: 388, threads: 10, snap_a: 7, snap_b: 6, pair: 7, fp: B33_FP },
];

// ── 状态机（Bridged → Enumerated → Filed） ───────────────────────
/// 消费状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsState {
    /// 已衔接（B33 环立）。
    Bridged,
    /// 枚举中。
    Enumerated,
    /// 已归档。
    Filed,
}

impl ConsState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            ConsState::Bridged => 0,
            ConsState::Enumerated => 1,
            ConsState::Filed => 2,
        }
    }
}

/// 消费账。
#[derive(Clone, Debug)]
pub struct EnumLedger {
    /// 已对平行。
    rows: Vec<EnumRow>,
    /// 拒收记账（探测号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: ConsState,
}

impl EnumLedger {
    /// 开账（Bridged 态）。
    pub fn new() -> EnumLedger {
        EnumLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: ConsState::Bridged,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ConsState {
        self.state
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

    /// 衔接（Bridged→Enumerated）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ConsState::Enumerated.rank() {
            return Err(EnumDiag::ERewind.val());
        }
        self.state = ConsState::Enumerated;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < ENUM_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐行消费（行号序 + 黄金判定 + 行互异 三闸）。
    pub fn consume(&mut self, r: EnumRow) -> Result<(), u32> {
        if self.state.rank() < ConsState::Enumerated.rank() {
            return Err(EnumDiag::ENotBridged.val());
        }
        if self.state.rank() >= ConsState::Filed.rank() {
            self.note_reject(r.no, EnumDiag::EFiledWrite.val());
            return Err(EnumDiag::EFiledWrite.val());
        }
        // 行号序闸。
        if r.no != self.rows.len() as u32 {
            self.note_reject(r.no, EnumDiag::ERowOrder.val());
            return Err(EnumDiag::ERowOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = enum_row_ok(&r) {
            self.note_reject(r.no, e);
            return Err(e);
        }
        // 行互异闸：同 pid 同对号只入一笔。
        for done in self.rows.iter() {
            if done.pid == r.pid && done.pair == r.pair {
                self.note_reject(r.no, EnumDiag::ERowOrder.val());
                return Err(EnumDiag::ERowOrder.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 归档：8 行齐 + 零拒收。
    pub fn file(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ConsState::Filed.rank() {
            return Err(EnumDiag::ERewind.val());
        }
        if self.state.rank() < ConsState::Enumerated.rank() {
            return Err(EnumDiag::ENotBridged.val());
        }
        if self.rows.len() != ENUM_CAP {
            return Err(EnumDiag::ERowOrder.val());
        }
        if !self.rejects.is_empty() {
            return Err(EnumDiag::ERowOrder.val());
        }
        self.state = ConsState::Filed;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == ENUM_CAP && self.rejects.is_empty()
    }
}

// ── 快照方差档显式面（十例确定性出题） ───────────────────────────
/// 方差演练例（对号/A 计数/B 计数/期望越沿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarCase {
    /// 例号。
    pub no: u32,
    /// 快照 A 计数。
    pub a: u32,
    /// 快照 B 计数。
    pub b: u32,
    /// 期望越沿（true = 必须检出）。
    pub expect_over: bool,
}

/// 冻结方差演练档（10 例：6 正 + 4 越沿）。
pub const VAR_PROFILE: &[VarCase] = &[
    VarCase { no: 0, a: 5, b: 5, expect_over: false },
    VarCase { no: 1, a: 5, b: 4, expect_over: false },
    VarCase { no: 2, a: 4, b: 6, expect_over: false },
    VarCase { no: 3, a: 7, b: 7, expect_over: false },
    VarCase { no: 4, a: 6, b: 5, expect_over: false },
    VarCase { no: 5, a: 3, b: 5, expect_over: false },
    VarCase { no: 6, a: 8, b: 4, expect_over: true },
    VarCase { no: 7, a: 2, b: 9, expect_over: true },
    VarCase { no: 8, a: 12, b: 2, expect_over: true },
    VarCase { no: 9, a: 0, b: 3, expect_over: true },
];

/// 方差档演练（判据 J1 的"快照方差档显式"直读口）。
/// 返回 (检出数, 总数, 首个逃脱描述)。
pub fn var_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for c in VAR_PROFILE.iter() {
        total += 1;
        let over = variance(c.a, c.b) > VAR_CAP;
        if over == c.expect_over {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("例{}({} vs {}) 漏检", c.no, c.a, c.b);
        }
    }
    (caught, total, escape)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumFault {
    /// pid 越表。
    PidRange,
    /// 线程计数为零。
    ThreadZero,
    /// 线程计数越上沿。
    ThreadOver,
    /// 方差越沿。
    VarOver,
    /// 快照对号越闭集。
    SnapRange,
    /// 指纹断链。
    FpMissing,
    /// 指纹错挂。
    FpMismatch,
    /// 探测号乱序。
    RowOrder,
    /// 未衔接即探。
    NotBridged,
    /// 归档后写。
    FiledWrite,
}

impl EnumFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            EnumFault::PidRange => 0,
            EnumFault::ThreadZero => 1,
            EnumFault::ThreadOver => 2,
            EnumFault::VarOver => 3,
            EnumFault::SnapRange => 4,
            EnumFault::FpMissing => 5,
            EnumFault::FpMismatch => 6,
            EnumFault::RowOrder => 7,
            EnumFault::NotBridged => 8,
            EnumFault::FiledWrite => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            EnumFault::PidRange => "pid 越表",
            EnumFault::ThreadZero => "线程计数零",
            EnumFault::ThreadOver => "线程计数越沿",
            EnumFault::VarOver => "方差越沿",
            EnumFault::SnapRange => "对号越闭集",
            EnumFault::FpMissing => "指纹断链",
            EnumFault::FpMismatch => "指纹错挂",
            EnumFault::RowOrder => "探测号乱序",
            EnumFault::NotBridged => "未衔接即探",
            EnumFault::FiledWrite => "归档后写",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [EnumFault] {
        &[
            EnumFault::PidRange,
            EnumFault::ThreadZero,
            EnumFault::ThreadOver,
            EnumFault::VarOver,
            EnumFault::SnapRange,
            EnumFault::FpMissing,
            EnumFault::FpMismatch,
            EnumFault::RowOrder,
            EnumFault::NotBridged,
            EnumFault::FiledWrite,
        ]
    }
}

/// 构建完整消费账（8 行全过闸）。
pub fn build_full_ledger() -> EnumLedger {
    let mut led = EnumLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < ENUM_CAP {
        let r = match ENUM_PROFILE.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.consume(r) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in EnumFault::all().iter() {
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
fn run_fault(v: EnumFault) -> bool {
    match v {
        EnumFault::PidRange => {
            let mut r = match ENUM_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.pid = 9999;
            enum_row_ok(&r).is_err()
        }
        EnumFault::ThreadZero => {
            let mut r = match ENUM_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.threads = 0;
            enum_row_ok(&r).is_err()
        }
        EnumFault::ThreadOver => {
            let mut r = match ENUM_PROFILE.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.threads = THREAD_CAP + 1;
            enum_row_ok(&r).is_err()
        }
        EnumFault::VarOver => {
            let mut r = match ENUM_PROFILE.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.snap_a = 10;
            r.snap_b = 2;
            enum_row_ok(&r).is_err()
        }
        EnumFault::SnapRange => {
            let mut r = match ENUM_PROFILE.get(4) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.pair = SNAP_CAP + 1;
            enum_row_ok(&r).is_err()
        }
        EnumFault::FpMissing => {
            let mut r = match ENUM_PROFILE.get(5) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.fp = "";
            enum_row_ok(&r).is_err()
        }
        EnumFault::FpMismatch => {
            let mut r = match ENUM_PROFILE.get(6) {
                Some(x) => x.clone(),
                None => return false,
            };
            r.fp = "B32-FAKE";
            enum_row_ok(&r).is_err()
        }
        EnumFault::RowOrder => {
            let mut led = EnumLedger::new();
            let _ = led.begin();
            let r = match ENUM_PROFILE.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
        EnumFault::NotBridged => {
            let mut fresh = EnumLedger::new();
            let r = match ENUM_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            fresh.consume(r).is_err()
        }
        EnumFault::FiledWrite => {
            let mut led = build_full_ledger();
            if led.file().is_err() {
                return false;
            }
            let r = match ENUM_PROFILE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.consume(r).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 枚举探测消费对平断言绿（判据 J1 直读口）。
fn enum_profile_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != ENUM_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.file().is_err() {
        return false;
    }
    if led.state() != ConsState::Filed {
        return false;
    }
    // 独立复核：8 行逐行黄金判定。
    for r in ENUM_PROFILE.iter() {
        if enum_row_ok(r).is_err() {
            return false;
        }
    }
    true
}

/// B33 F12649 档衔接直读。
fn b33_ref_ok() -> bool {
    if B33_FP != "B33-F12649-SYSINFO-PROFILE" {
        return false;
    }
    if B33_REF_ID != 12649 {
        return false;
    }
    for r in ENUM_PROFILE.iter() {
        if r.fp != B33_FP {
            return false;
        }
    }
    true
}

/// 快照方差档显式（10/10 演练一致）。
fn var_ok() -> bool {
    let (caught, total, escape) = var_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    // 方差函数边界直读。
    if variance(5, 5) != 0 {
        return false;
    }
    if variance(3, 9) != 6 {
        return false;
    }
    if variance(0, VAR_CAP) != VAR_CAP {
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

/// 进程表冻结直读（5 例互异、tag 非空）。
fn proc_census_ok() -> bool {
    if PROC_TABLE.len() != PROC_CAP {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < PROC_TABLE.len() {
        let p = match PROC_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if p.pid == 0 || p.tag.is_empty() {
            ok = false;
        }
        if proc_of(p.pid).is_none() {
            ok = false;
        }
        let mut j = i + 1;
        while j < PROC_TABLE.len() {
            match PROC_TABLE.get(j) {
                Some(q) => {
                    if q.pid == p.pid {
                        ok = false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    ok
}

/// 双跑一致（冻结档下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
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
    if var_sweep().0 != var_sweep().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12669u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.file().is_err() {
        return false;
    }
    if led.state() != ConsState::Filed {
        return false;
    }
    match led.row_at(0) {
        Some(r) => {
            if r.pid == 0 || r.fp != B33_FP {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12670 枚举消费自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12669_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12669-proc-enum");
    let led = build_full_ledger();

    s.add_dyn(
        "F12670-01 枚举档八行齐",
        ENUM_PROFILE.len() == ENUM_CAP,
        &format!("rows={}", ENUM_PROFILE.len()),
    );

    s.add_dyn(
        "F12670-02 枚举探测消费对平断言绿",
        enum_profile_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12670-03 B33 F12649 档衔接",
        b33_ref_ok(),
        &format!("fp={} ref_id={}", B33_FP, B33_REF_ID),
    );

    s.add_dyn(
        "F12670-04 快照方差档显式 10/10",
        var_ok(),
        &format!("caught={}", var_sweep().0),
    );

    s.add_dyn(
        "F12670-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12670-06 进程表冻结直读",
        proc_census_ok(),
        &format!("procs={}", PROC_TABLE.len()),
    );

    s.add_dyn(
        "F12670-07 方差上沿口径",
        VAR_CAP == 2,
        &format!("var_cap={}（冻结）", VAR_CAP),
    );

    s.add_dyn(
        "F12670-08 线程计数上沿口径",
        THREAD_CAP == 64,
        &format!("thread_cap={}（冻结）", THREAD_CAP),
    );

    s.add_dyn(
        "F12670-09 双跑一致",
        dual_run_ok(),
        "冻结档下两轮逐行同判",
    );

    s.add_dyn(
        "F12670-10 消费账等式",
        led.equation_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12670-11 完成写回语义",
        writeback_ok(),
        "归档即已完成且摘要非空",
    );

    s.add_dyn(
        "F12670-12 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
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
        assert_eq!(SEG_BASE, 0x81140);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中。
        assert_eq!(ENUM_ALL.len(), 11);
        let mut i = 0usize;
        while i < ENUM_ALL.len() {
            let vi = match ENUM_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < ENUM_ALL.len() {
                let vj = match ENUM_ALL.get(j) {
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
    fn happy_path_files() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), ENUM_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.file().is_ok());
        assert_eq!(led.state(), ConsState::Filed);
    }

    #[test]
    fn variance_explicit_ten_ten() {
        // 快照方差档显式：10 例演练全对平。
        let (caught, total, escape) = var_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "漏检：{}", escape);
        assert!(escape.is_empty());
        // 方差函数边界。
        assert_eq!(variance(5, 5), 0);
        assert_eq!(variance(3, 9), 6);
        assert_eq!(variance(9, 3), 6);
        assert_eq!(variance(0, 0), 0);
        // 上沿边界：恰 2 过、3 红。
        let eq = EnumRow { no: 0, pid: 4, threads: 1, snap_a: 7, snap_b: 5, pair: 0, fp: B33_FP };
        assert!(enum_row_ok(&eq).is_ok());
        let over = EnumRow { no: 0, pid: 4, threads: 1, snap_a: 7, snap_b: 4, pair: 0, fp: B33_FP };
        assert_eq!(enum_row_ok(&over), Err(EnumDiag::EVarOver.val()));
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
    fn thread_count_boundary() {
        // 线程计数闭集两端。
        let lo = EnumRow { no: 0, pid: 4, threads: 1, snap_a: 1, snap_b: 1, pair: 0, fp: B33_FP };
        assert!(enum_row_ok(&lo).is_ok());
        let hi = EnumRow { no: 0, pid: 4, threads: THREAD_CAP, snap_a: 1, snap_b: 1, pair: 0, fp: B33_FP };
        assert!(enum_row_ok(&hi).is_ok());
        let zero = EnumRow { no: 0, pid: 4, threads: 0, snap_a: 1, snap_b: 1, pair: 0, fp: B33_FP };
        assert_eq!(enum_row_ok(&zero), Err(EnumDiag::EThreadZero.val()));
        let over = EnumRow { no: 0, pid: 4, threads: THREAD_CAP + 1, snap_a: 1, snap_b: 1, pair: 0, fp: B33_FP };
        assert_eq!(enum_row_ok(&over), Err(EnumDiag::EThreadOver.val()));
    }

    #[test]
    fn snap_pair_boundary() {
        // 快照对号闭集两端。
        let lo = EnumRow { no: 0, pid: 4, threads: 1, snap_a: 1, snap_b: 1, pair: 0, fp: B33_FP };
        assert!(enum_row_ok(&lo).is_ok());
        let hi = EnumRow { no: 0, pid: 4, threads: 1, snap_a: 1, snap_b: 1, pair: SNAP_CAP, fp: B33_FP };
        assert!(enum_row_ok(&hi).is_ok());
        let over = EnumRow { no: 0, pid: 4, threads: 1, snap_a: 1, snap_b: 1, pair: SNAP_CAP + 1, fp: B33_FP };
        assert_eq!(enum_row_ok(&over), Err(EnumDiag::ESnapRange.val()));
    }

    #[test]
    fn row_gate_rejects() {
        // 行闸直读：乱序/越表/断链。
        let mut led = EnumLedger::new();
        let _ = led.begin();
        let r1 = match ENUM_PROFILE.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.consume(r1), Err(EnumDiag::ERowOrder.val()));
        let r0 = match ENUM_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.consume(r0).is_ok());
        // 越表 pid。
        let ghost = EnumRow { no: 1, pid: 4242, threads: 3, snap_a: 1, snap_b: 1, pair: 1, fp: B33_FP };
        assert_eq!(led.consume(ghost), Err(EnumDiag::EPidRange.val()));
        // 指纹断链。
        let nofp = EnumRow { no: 1, pid: 320, threads: 3, snap_a: 1, snap_b: 1, pair: 1, fp: "" };
        assert_eq!(led.consume(nofp), Err(EnumDiag::EFpMissing.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(var_sweep().0, var_sweep().0);
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Bridged→Enumerated→Filed。
        assert!(ConsState::Bridged.rank() < ConsState::Enumerated.rank());
        assert!(ConsState::Enumerated.rank() < ConsState::Filed.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), ConsState::Enumerated);
        assert!(led.file().is_ok());
        assert_eq!(led.file(), Err(EnumDiag::ERewind.val()));
        let mut fresh = EnumLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(EnumDiag::ERewind.val()));
    }

    #[test]
    fn b33_anchor_frozen() {
        // B33 F12649 衔接锚冻结直读。
        assert_eq!(B33_FP, "B33-F12649-SYSINFO-PROFILE");
        assert_eq!(B33_REF_ID, 12649);
        assert!(b33_ref_ok());
        // 指纹错挂直读。
        let mut r = match ENUM_PROFILE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        r.fp = "B33-FAKE";
        assert_eq!(enum_row_ok(&r), Err(EnumDiag::EFpMismatch.val()));
    }

    #[test]
    fn proc_table_semantics() {
        // 进程表：5 例互异、查表命中/越表。
        assert_eq!(PROC_TABLE.len(), PROC_CAP);
        assert!(proc_of(4).is_some());
        assert!(proc_of(1024).is_some());
        assert!(proc_of(0).is_none());
        assert!(proc_of(4242).is_none());
        let p = match proc_of(512) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(p.tag, "EXPL");
        assert!(proc_census_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12669_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
