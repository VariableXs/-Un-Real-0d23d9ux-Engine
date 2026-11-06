//! UNX-F9406 prctl option 错误面 · 0x7D300段
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：收库会话自领单（与 VX11 既有 9001~9402 卡面零重复：F9406 聚焦
//!       prctl 选项闭集与逐选项参数判序错误面，既有卡未涉足该面）。
//!
//!   UNX-F9406-J1 prctl 选项闭集注入判据过：未知 option 全 EOptUnknown 格过，
//!   已知十类选项零误伤；逐选项参数判序与 Linux 一致（SET_NAME 含 NUL ≤16 /
//!   SET_DUMPABLE ∈{0,1} / SET_PDEATHSIG ∈[0,64] / SET_TIMERSLACK ≥0 /
//!   GET_* 须空参）格过；no_new_privs 单向 0→1 格过（1→0 拒止 EPrivDrop）；
//!   注入必抓格过；双跑一致（确定性）格过；段位守卫格过；闭集诊断码格过。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 设计要点：
//!   1) 真实判定器：prctl option 闭集（SET_NAME=15/GET_NAME=16/
//!      SET_NO_NEW_PRIVS=38/GET_NO_NEW_PRIVS=39/SET_DUMPABLE=4/GET_DUMPABLE=3/
//!      SET_PDEATHSIG=1/GET_PDEATHSIG=2/SET_TIMERSLACK=29/GET_TIMERSLACK=30），
//!      判序与 Linux prctl 一致：选项闭集（EOptUnknown）→ 逐选项参数有效性。
//!   2) 逐选项参数判序：SET_NAME 名字节含 NUL ≤16（越者 ENameTooLong，内嵌
//!      NUL 合法但截断记账）；SET_DUMPABLE ∈{0,1}（EBadDumpable）；
//!      SET_PDEATHSIG ∈[0,64]（EBadSig）；SET_TIMERSLACK ≥0（ENegSlack）；
//!      GET_* 须空参 arg2==0（EGetWithArg，Linux 语义 GET 选项不取值）。
//!   3) 真实状态机：TaskAttrs {name ≤15字节, no_new_privs 两态单向 0→1,
//!      dumpable ∈{0,1}, pdeathsig ∈[0,64], timerslack_ns}；Phase 单调
//!      Fresh→Live→Sealed 不回退；name-set/get 往返恰等。
//!   4) 注入器 12 类违约（未知 99/-1、名 17 字节、dumpable=2、sig=65、
//!      slack<0、GET 带参、NNP 1→0、NNP=2、封账后写、账本满、内嵌 NUL
//!      记账正向），断言必抓 12/12（精确诊断码匹配）。
//!   5) 对账：RECON_TABLE 十行与 Linux 原始编号一一对号；段位守卫
//!      0x7D300 步进 0x40 内不越界。
//!   6) 双跑一致：同一输入序列跑两遍，FNV-1a 64 位指纹逐位相等。
//!   7) no_std 纪律：无 unsafe/unwrap/expect/panic!/assert!/浮点/裸下标；
//!      取值走 match/if let/get/iter/checked_*/saturating_*。

#![allow(dead_code)]

use alloc::string::{String, ToString}; use alloc::vec::Vec; use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7D300;

// ════════════════════════ Linux prctl 原始编号（闭集） ════════════════════════

pub const PR_SET_PDEATHSIG: i32 = 1;
pub const PR_GET_PDEATHSIG: i32 = 2;
pub const PR_GET_DUMPABLE: i32 = 3;
pub const PR_SET_DUMPABLE: i32 = 4;
pub const PR_SET_NAME: i32 = 15;
pub const PR_GET_NAME: i32 = 16;
pub const PR_SET_TIMERSLACK: i32 = 29;
pub const PR_GET_TIMERSLACK: i32 = 30;
pub const PR_SET_NO_NEW_PRIVS: i32 = 38;
pub const PR_GET_NO_NEW_PRIVS: i32 = 39;

/// task comm 名槽字节容量（含 NUL，Linux TASK_COMM_LEN=16）。
pub const COMM_MAX_BYTES: usize = 16;
/// pdeathsig 信号上界（0..=64，64 = RT 信号墙上沿）。
pub const SIG_MAX: i64 = 64;

// ════════════════════════ 闭集诊断码 ════════════════════════

/// 闭集诊断码：本卡全部错误位面，超出闭集即违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrctlErr {
    /// 未知 option（不在十元闭集）。
    EOptUnknown,
    /// SET_NAME 名字节含 NUL > 16。
    ENameTooLong,
    /// SET_DUMPABLE arg2 ∉ {0,1}。
    EBadDumpable,
    /// SET_PDEATHSIG signal ∉ [0,64]。
    EBadSig,
    /// SET_TIMERSLACK arg2 < 0。
    ENegSlack,
    /// GET_* 选项带非空参（arg2 != 0）。
    EGetWithArg,
    /// SET_NO_NEW_PRIVS arg2 ∉ {0,1}。
    EBadNnp,
    /// no_new_privs 单向违序：1→0。
    EPrivDrop,
    /// 封账后写。
    ESealed,
    /// 账本满（容量守恒被触）。
    ELedgerFull,
}

impl PrctlErr {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PrctlErr::EOptUnknown => "未知 option，不在十元闭集",
            PrctlErr::ENameTooLong => "SET_NAME 名字节含 NUL 超 16",
            PrctlErr::EBadDumpable => "SET_DUMPABLE 取值不在 {0,1}",
            PrctlErr::EBadSig => "SET_PDEATHSIG 信号号越界 [0,64]",
            PrctlErr::ENegSlack => "SET_TIMERSLACK 取负值",
            PrctlErr::EGetWithArg => "GET 选项须空参，arg2 非 0",
            PrctlErr::EBadNnp => "SET_NO_NEW_PRIVS 取值不在 {0,1}",
            PrctlErr::EPrivDrop => "no_new_privs 单向违序 1→0",
            PrctlErr::ESealed => "封账后禁写",
            PrctlErr::ELedgerFull => "账本容量触顶",
        }
    }
    pub const ALL: [PrctlErr; 10] = [
        PrctlErr::EOptUnknown,
        PrctlErr::ENameTooLong,
        PrctlErr::EBadDumpable,
        PrctlErr::EBadSig,
        PrctlErr::ENegSlack,
        PrctlErr::EGetWithArg,
        PrctlErr::EBadNnp,
        PrctlErr::EPrivDrop,
        PrctlErr::ESealed,
        PrctlErr::ELedgerFull,
    ];
}

// ════════════════════════ 域模型：prctl 选项闭集 ════════════════════════

/// prctl 选项闭集（kernel/sys.c 语义，判序入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrctlOption {
    SetName,
    GetName,
    SetNoNewPrivs,
    GetNoNewPrivs,
    SetDumpable,
    GetDumpable,
    SetPdeathsig,
    GetPdeathsig,
    SetTimerslack,
    GetTimerslack,
}

impl PrctlOption {
    pub fn from_raw(raw: i32) -> Option<PrctlOption> {
        match raw {
            PR_SET_NAME => Some(PrctlOption::SetName),
            PR_GET_NAME => Some(PrctlOption::GetName),
            PR_SET_NO_NEW_PRIVS => Some(PrctlOption::SetNoNewPrivs),
            PR_GET_NO_NEW_PRIVS => Some(PrctlOption::GetNoNewPrivs),
            PR_SET_DUMPABLE => Some(PrctlOption::SetDumpable),
            PR_GET_DUMPABLE => Some(PrctlOption::GetDumpable),
            PR_SET_PDEATHSIG => Some(PrctlOption::SetPdeathsig),
            PR_GET_PDEATHSIG => Some(PrctlOption::GetPdeathsig),
            PR_SET_TIMERSLACK => Some(PrctlOption::SetTimerslack),
            PR_GET_TIMERSLACK => Some(PrctlOption::GetTimerslack),
            _ => None,
        }
    }
    pub fn idx(self) -> usize {
        match self {
            PrctlOption::SetName => 0,
            PrctlOption::GetName => 1,
            PrctlOption::SetNoNewPrivs => 2,
            PrctlOption::GetNoNewPrivs => 3,
            PrctlOption::SetDumpable => 4,
            PrctlOption::GetDumpable => 5,
            PrctlOption::SetPdeathsig => 6,
            PrctlOption::GetPdeathsig => 7,
            PrctlOption::SetTimerslack => 8,
            PrctlOption::GetTimerslack => 9,
        }
    }
    /// GET_* 选项须空参（Linux 语义：GET 不取值）。
    pub fn is_get(self) -> bool {
        match self {
            PrctlOption::GetName
            | PrctlOption::GetNoNewPrivs
            | PrctlOption::GetDumpable
            | PrctlOption::GetPdeathsig
            | PrctlOption::GetTimerslack => true,
            _ => false,
        }
    }
}

/// 单次成功调用的产出（GET_* 的 ret 为读回值，SET_* 为 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OptOutcome {
    pub opt: PrctlOption,
    pub ret: i64,
}

/// 任务属性（prctl 的真实落点）。
#[derive(Clone, Debug)]
pub struct TaskAttrs {
    /// comm 名（不含 NUL，≤ COMM_MAX_BYTES-1 字节）。
    pub name: String,
    /// no_new_privs 两态，只能 0→1 单向。
    pub no_new_privs: u8,
    /// dumpable ∈ {0,1}。
    pub dumpable: u8,
    /// pdeathsig ∈ [0,64]。
    pub pdeathsig: u8,
    /// timerslack 纳秒（≥0）。
    pub timerslack_ns: u64,
}

/// Phase 状态机：Fresh → Live → Sealed，单调不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Fresh,
    Live,
    Sealed,
}

/// 账本记录（仅成功调用入账）。
#[derive(Clone, Copy, Debug)]
pub struct OptionRec {
    pub step: u64,
    pub opt_idx: u8,
    pub arg2: i64,
    /// SET_NAME 内嵌 NUL 截断记账。
    pub nul_trunc: bool,
}

// ════════════════════════ 状态机：PrctlState ════════════════════════

pub const LEDGER_CAP: usize = 32;

/// prctl 状态机 + 记账。判序：封账 → 选项闭集 → 账容 → 逐选项参数 → 生效。
pub struct PrctlState {
    attrs: TaskAttrs,
    phase: Phase,
    recs: Vec<OptionRec>,
    cap: usize,
    step: u64,
    sealed: bool,
}

impl PrctlState {
    pub fn new() -> Self {
        PrctlState {
            attrs: TaskAttrs {
                name: String::new(),
                no_new_privs: 0,
                dumpable: 1,
                pdeathsig: 0,
                timerslack_ns: 50_000,
            },
            phase: Phase::Fresh,
            recs: Vec::new(),
            cap: LEDGER_CAP,
            step: 0,
            sealed: false,
        }
    }

    pub fn attrs(&self) -> &TaskAttrs {
        &self.attrs
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn recs(&self) -> &[OptionRec] {
        &self.recs
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// prctl 判定主入口。`name` 仅 SET_NAME 选项使用（其余传 ""）；
    /// SET_NAME 的 arg2 是用户态缓冲指针（模型内不校验，长度以 name
    /// 字节含 NUL 记账）。
    pub fn prctl(&mut self, opt: i32, arg2: i64, name: &str) -> Result<OptOutcome, u32> {
        // 判序 ①：封账后禁写。
        if self.sealed {
            return Err(PrctlErr::ESealed.val());
        }
        // 判序 ②：选项闭集。
        let po = match PrctlOption::from_raw(opt) {
            Some(p) => p,
            None => return Err(PrctlErr::EOptUnknown.val()),
        };
        // 判序 ③：账本容量守恒。
        if self.recs.len() >= self.cap {
            return Err(PrctlErr::ELedgerFull.val());
        }
        let mut nul_trunc = false;
        // 判序 ④：逐选项参数有效性（与 Linux 判序一致）。
        let outcome = match po {
            PrctlOption::SetName => {
                let bytes = name.as_bytes();
                let total = bytes.len().saturating_add(1); // 含 NUL
                if total > COMM_MAX_BYTES {
                    return Err(PrctlErr::ENameTooLong.val());
                }
                // 内嵌 NUL：合法，可见名截断于首个 NUL 并记账。
                let (stored, trunc) = match bytes.iter().position(|&b| b == 0) {
                    Some(p) => (
                        match bytes.get(..p).map(core::str::from_utf8) {
                            Some(Ok(s)) => s,
                            _ => "",
                        },
                        true,
                    ),
                    None => (name, false),
                };
                self.attrs.name = String::from(stored);
                nul_trunc = trunc;
                OptOutcome {
                    opt: po,
                    ret: stored.len() as i64,
                }
            }
            PrctlOption::GetName => {
                if arg2 != 0 {
                    return Err(PrctlErr::EGetWithArg.val());
                }
                OptOutcome {
                    opt: po,
                    ret: self.attrs.name.as_bytes().len() as i64,
                }
            }
            PrctlOption::SetNoNewPrivs => {
                if arg2 != 0 && arg2 != 1 {
                    return Err(PrctlErr::EBadNnp.val());
                }
                // 单向 0→1：1→0 拒止（EPrivDrop）。
                if arg2 == 0 && self.attrs.no_new_privs == 1 {
                    return Err(PrctlErr::EPrivDrop.val());
                }
                self.attrs.no_new_privs = arg2 as u8;
                OptOutcome { opt: po, ret: 0 }
            }
            PrctlOption::GetNoNewPrivs => {
                if arg2 != 0 {
                    return Err(PrctlErr::EGetWithArg.val());
                }
                OptOutcome {
                    opt: po,
                    ret: self.attrs.no_new_privs as i64,
                }
            }
            PrctlOption::SetDumpable => {
                if arg2 != 0 && arg2 != 1 {
                    return Err(PrctlErr::EBadDumpable.val());
                }
                self.attrs.dumpable = arg2 as u8;
                OptOutcome { opt: po, ret: 0 }
            }
            PrctlOption::GetDumpable => {
                if arg2 != 0 {
                    return Err(PrctlErr::EGetWithArg.val());
                }
                OptOutcome {
                    opt: po,
                    ret: self.attrs.dumpable as i64,
                }
            }
            PrctlOption::SetPdeathsig => {
                if arg2 < 0 || arg2 > SIG_MAX {
                    return Err(PrctlErr::EBadSig.val());
                }
                self.attrs.pdeathsig = arg2 as u8;
                OptOutcome { opt: po, ret: 0 }
            }
            PrctlOption::GetPdeathsig => {
                if arg2 != 0 {
                    return Err(PrctlErr::EGetWithArg.val());
                }
                OptOutcome {
                    opt: po,
                    ret: self.attrs.pdeathsig as i64,
                }
            }
            PrctlOption::SetTimerslack => {
                if arg2 < 0 {
                    return Err(PrctlErr::ENegSlack.val());
                }
                self.attrs.timerslack_ns = arg2 as u64;
                OptOutcome { opt: po, ret: 0 }
            }
            PrctlOption::GetTimerslack => {
                if arg2 != 0 {
                    return Err(PrctlErr::EGetWithArg.val());
                }
                OptOutcome {
                    opt: po,
                    ret: self.attrs.timerslack_ns.min(i64::MAX as u64) as i64,
                }
            }
        };
        // 生效：记账 + Phase 单调推进 Fresh→Live。
        self.recs.push(OptionRec {
            step: self.step,
            opt_idx: po.idx() as u8,
            arg2,
            nul_trunc,
        });
        self.step = self.step.saturating_add(1);
        if let Phase::Fresh = self.phase {
            self.phase = Phase::Live;
        }
        Ok(outcome)
    }

    /// 封账：Phase 单调推进到 Sealed；重复封账拒止。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(PrctlErr::ESealed.val());
        }
        self.sealed = true;
        self.phase = Phase::Sealed;
        Ok(self.recs.len())
    }

    /// 不变量：账本容量守恒、step 单调、名长含 NUL ≤16、两态域 ∈{0,1}、
    /// pdeathsig ∈[0,64]、Phase 单调（有账必不 Fresh、封账必 Sealed）。
    pub fn invariant_ok(&self) -> bool {
        if self.recs.len() > self.cap {
            return false;
        }
        if self.attrs.name.as_bytes().len().saturating_add(1) > COMM_MAX_BYTES {
            return false;
        }
        if self.attrs.no_new_privs > 1 || self.attrs.dumpable > 1 {
            return false;
        }
        if self.attrs.pdeathsig as i64 > SIG_MAX {
            return false;
        }
        let mut prev: Option<u64> = None;
        for r in self.recs.iter() {
            if r.opt_idx as usize >= 10 {
                return false;
            }
            if let Some(p) = prev {
                if r.step <= p {
                    return false;
                }
            }
            prev = Some(r.step);
        }
        match self.phase {
            Phase::Fresh if !self.recs.is_empty() => return false,
            Phase::Sealed if !self.sealed => return false,
            _ => {}
        }
        true
    }
}

// ════════════════════════ 注入器 ════════════════════════

/// 注入违约样本闭集：每类必抓（精确诊断码匹配），共 12 类。
pub struct Injector;

/// 16 字节名样本（含 NUL 共 17 字节 → 必越界）。
const NAME16: &str = "aaaaaaaaaaaaaaaa";

impl Injector {
    /// 12 类注入样本，逐一构造并断言必抓。返回 (抓到数, 总数)。
    pub fn run_all(st: &mut PrctlState) -> (usize, usize) {
        let mut caught = 0usize;
        let total = 12usize;
        // ① 未知 option 99 → EOptUnknown。
        if st.prctl(99, 0, "") == Err(PrctlErr::EOptUnknown.val()) {
            caught += 1;
        }
        // ② 未知 option -1 → EOptUnknown。
        if st.prctl(-1, 0, "") == Err(PrctlErr::EOptUnknown.val()) {
            caught += 1;
        }
        // ③ 名 16 字节（含 NUL 17 > 16）→ ENameTooLong。
        if st.prctl(PR_SET_NAME, 0, NAME16) == Err(PrctlErr::ENameTooLong.val()) {
            caught += 1;
        }
        // ④ dumpable = 2 → EBadDumpable。
        if st.prctl(PR_SET_DUMPABLE, 2, "") == Err(PrctlErr::EBadDumpable.val()) {
            caught += 1;
        }
        // ⑤ pdeathsig = 65 → EBadSig。
        if st.prctl(PR_SET_PDEATHSIG, 65, "") == Err(PrctlErr::EBadSig.val()) {
            caught += 1;
        }
        // ⑥ timerslack < 0 → ENegSlack。
        if st.prctl(PR_SET_TIMERSLACK, -1, "") == Err(PrctlErr::ENegSlack.val()) {
            caught += 1;
        }
        // ⑦ GET 带参 → EGetWithArg。
        if st.prctl(PR_GET_NAME, 7, "") == Err(PrctlErr::EGetWithArg.val()) {
            caught += 1;
        }
        // ⑧ NNP 单向违序：0→1 合法，1→0 → EPrivDrop。
        if st.prctl(PR_SET_NO_NEW_PRIVS, 1, "").is_ok()
            && st.prctl(PR_SET_NO_NEW_PRIVS, 0, "") == Err(PrctlErr::EPrivDrop.val())
        {
            caught += 1;
        }
        // ⑨ NNP = 2 → EBadNnp。
        if st.prctl(PR_SET_NO_NEW_PRIVS, 2, "") == Err(PrctlErr::EBadNnp.val()) {
            caught += 1;
        }
        // ⑩ 内嵌 NUL：合法但截断记账（正向语义，零误伤验证）。
        let ok_nul = st.prctl(PR_SET_NAME, 0, "va\0rix");
        let trunc_ok = match ok_nul {
            Ok(o) => {
                let rec_trunc = match st.recs.last() {
                    Some(r) => r.nul_trunc,
                    None => false,
                };
                o.ret == 2 && rec_trunc && st.attrs().name == "va"
            }
            Err(_) => false,
        };
        if trunc_ok {
            caught += 1;
        }
        // ⑪ 账本满：以合法 GET_TIMERSLACK 灌满后下一次写 → ELedgerFull。
        while st.recs.len() < st.cap {
            let _ = st.prctl(PR_GET_TIMERSLACK, 0, "");
        }
        if st.prctl(PR_GET_NAME, 0, "") == Err(PrctlErr::ELedgerFull.val()) {
            caught += 1;
        }
        // ⑫ 封账后写 → ESealed。
        let _ = st.seal();
        if st.prctl(PR_SET_DUMPABLE, 1, "") == Err(PrctlErr::ESealed.val()) {
            caught += 1;
        }
        (caught, total)
    }
}

// ════════════════════════ 双跑一致 / 指纹 ════════════════════════

/// FNV-1a 64 位指纹（确定性）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 对账表条目：与 Linux 原始 prctl 编号一一对号（名, 原始编号, 槽位）。
pub const RECON_TABLE: [(&str, i32, i32); 10] = [
    ("PR_SET_NAME", 15, 0),
    ("PR_GET_NAME", 16, 1),
    ("PR_SET_NO_NEW_PRIVS", 38, 2),
    ("PR_GET_NO_NEW_PRIVS", 39, 3),
    ("PR_SET_DUMPABLE", 4, 4),
    ("PR_GET_DUMPABLE", 3, 5),
    ("PR_SET_PDEATHSIG", 1, 6),
    ("PR_GET_PDEATHSIG", 2, 7),
    ("PR_SET_TIMERSLACK", 29, 8),
    ("PR_GET_TIMERSLACK", 30, 9),
];

// ════════════════════════ 判据聚合 ════════════════════════

pub fn run_unx9406_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9406");
    // J1-① 选项闭集：未知 option 全 EOptUnknown。
    {
        let unknown: [i32; 12] = [-7, -1, 0, 5, 17, 31, 32, 63, 99, 1000, i32::MIN, i32::MAX];
        let mut all_ebad = true;
        for raw in unknown.iter() {
            let mut st = PrctlState::new();
            if st.prctl(*raw, 0, "") != Err(PrctlErr::EOptUnknown.val()) {
                all_ebad = false;
            }
        }
        s.add(
            "J1-01 未知option全EOptUnknown",
            all_ebad,
            "12 个未知 option 全部拒止于闭集",
        );
    }
    // J1-② 已知十类选项零误伤。
    {
        let mut all_ok = true;
        for (name, raw, _) in RECON_TABLE.iter() {
            let _ = name;
            let mut st = PrctlState::new();
            let r = match PrctlOption::from_raw(*raw) {
                Some(po) => match po {
                    PrctlOption::SetName => st.prctl(*raw, 0, "v").is_ok(),
                    PrctlOption::SetNoNewPrivs => st.prctl(*raw, 1, "").is_ok(),
                    PrctlOption::SetDumpable => st.prctl(*raw, 1, "").is_ok(),
                    PrctlOption::SetPdeathsig => st.prctl(*raw, 0, "").is_ok(),
                    PrctlOption::SetTimerslack => st.prctl(*raw, 100, "").is_ok(),
                    _ => st.prctl(*raw, 0, "").is_ok(),
                },
                None => false,
            };
            if !r {
                all_ok = false;
            }
        }
        s.add("J1-02 十类已知选项零误伤", all_ok, "RECON_TABLE 全行绿，闭集内不误伤");
    }
    // J1-③ SET_NAME 越长全抓。
    {
        let mut all_ebad = true;
        for n in [16usize, 17, 20, 64].iter() {
            let mut st = PrctlState::new();
            let mut nm = String::new();
            let mut k = 0usize;
            while k < *n {
                nm.push('a');
                k += 1;
            }
            if st.prctl(PR_SET_NAME, 0, &nm) != Err(PrctlErr::ENameTooLong.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-03 名越16字节全抓", all_ebad, "含 NUL 记账后 16..64 字节全拒");
    }
    // J1-④ 名 15 字节恰容零误伤。
    {
        let mut st = PrctlState::new();
        let mut nm = String::new();
        let mut k = 0usize;
        while k < 15 {
            nm.push('a');
            k += 1;
        }
        let ok = st.prctl(PR_SET_NAME, 0, &nm).is_ok()
            && st.prctl(PR_GET_NAME, 0, "") == Ok(OptOutcome {
                opt: PrctlOption::GetName,
                ret: 15,
            });
        s.add("J1-04 名15字节恰容零误伤", ok, "含 NUL 恰 16，往返 ret=15");
    }
    // J1-⑤ SET_DUMPABLE 越集全抓，{0,1} 零误伤。
    {
        let mut all_ebad = true;
        for v in [-1i64, 2, 7, i64::MIN, i64::MAX].iter() {
            let mut st = PrctlState::new();
            if st.prctl(PR_SET_DUMPABLE, *v, "") != Err(PrctlErr::EBadDumpable.val()) {
                all_ebad = false;
            }
        }
        let mut st = PrctlState::new();
        let zero_one_ok = st.prctl(PR_SET_DUMPABLE, 0, "").is_ok()
            && st.prctl(PR_SET_DUMPABLE, 1, "").is_ok();
        s.add(
            "J1-05 dumpable闭集判序",
            all_ebad && zero_one_ok,
            "越集样本全拒，{0,1} 全绿",
        );
    }
    // J1-⑥ SET_PDEATHSIG 越界全抓，[0,64] 零误伤。
    {
        let mut all_ebad = true;
        for v in [65i64, 255, 4096, i64::MAX].iter() {
            let mut st = PrctlState::new();
            if st.prctl(PR_SET_PDEATHSIG, *v, "") != Err(PrctlErr::EBadSig.val()) {
                all_ebad = false;
            }
        }
        let mut rng_ok = true;
        let mut sig = 0i64;
        while sig <= SIG_MAX {
            // 每个信号号独立状态（避免合法序列灌满账本误判）。
            let mut st = PrctlState::new();
            if st.prctl(PR_SET_PDEATHSIG, sig, "").is_err() {
                rng_ok = false;
            }
            sig += 1;
        }
        s.add("J1-06 pdeathsig判序", all_ebad && rng_ok, "65..全拒，0..=64 全绿");
    }
    // J1-⑦ SET_TIMERSLACK 负值全抓。
    {
        let mut all_ebad = true;
        for v in [-1i64, -100, i64::MIN].iter() {
            let mut st = PrctlState::new();
            if st.prctl(PR_SET_TIMERSLACK, *v, "") != Err(PrctlErr::ENegSlack.val()) {
                all_ebad = false;
            }
        }
        let mut st = PrctlState::new();
        let zero_ok = st.prctl(PR_SET_TIMERSLACK, 0, "").is_ok();
        s.add("J1-07 timerslack非负判序", all_ebad && zero_ok, "负值全拒，0 合法");
    }
    // J1-⑧ GET_* 须空参：带参全抓，空参全绿。
    {
        let gets: [i32; 5] = [PR_GET_NAME, PR_GET_NO_NEW_PRIVS, PR_GET_DUMPABLE, PR_GET_PDEATHSIG, PR_GET_TIMERSLACK];
        let mut all_ebad = true;
        for raw in gets.iter() {
            let mut st = PrctlState::new();
            if st.prctl(*raw, 1, "") != Err(PrctlErr::EGetWithArg.val()) {
                all_ebad = false;
            }
        }
        let mut all_ok = true;
        for raw in gets.iter() {
            let mut st = PrctlState::new();
            if st.prctl(*raw, 0, "").is_err() {
                all_ok = false;
            }
        }
        s.add(
            "J1-08 GET空参判序",
            all_ebad && all_ok,
            "五类 GET 带参全拒，空参全绿（Linux 语义）",
        );
    }
    // J1-⑨ NNP 单向 0→1：1→0 拒止。
    {
        let mut st = PrctlState::new();
        let up_ok = st.prctl(PR_SET_NO_NEW_PRIVS, 1, "").is_ok();
        let drop_rejected = st.prctl(PR_SET_NO_NEW_PRIVS, 0, "") == Err(PrctlErr::EPrivDrop.val());
        let sticky_ok = st.prctl(PR_SET_NO_NEW_PRIVS, 1, "").is_ok();
        let bad_rejected = st.prctl(PR_SET_NO_NEW_PRIVS, 2, "") == Err(PrctlErr::EBadNnp.val());
        s.add(
            "J1-09 NNP单向单调",
            up_ok && drop_rejected && sticky_ok && bad_rejected,
            "0→1 绿、1→0 EPrivDrop、1→1 幂等、2 → EBadNnp",
        );
    }
    // J1-⑩ name-set/get 往返恰等 + 内嵌 NUL 截断记账。
    {
        let mut st = PrctlState::new();
        let _ = st.prctl(PR_SET_NAME, 0, "varix内核");
        let got = st.prctl(PR_GET_NAME, 0, "");
        let round_ok = match got {
            Ok(o) => o.ret == st.attrs().name.as_bytes().len() as i64 && !st.attrs().name.is_empty(),
            Err(_) => false,
        };
        let _ = st.prctl(PR_SET_NAME, 0, "va\0rix");
        let trunc_ok = st.attrs().name == "va"
            && match st.recs.last() {
                Some(r) => r.nul_trunc,
                None => false,
            };
        s.add(
            "J1-10 名往返与NUL记账",
            round_ok && trunc_ok,
            "set/get 往返恰等；内嵌 NUL 截断于首 NUL 且记账",
        );
    }
    // J1-⑪ 注入器 12/12。
    {
        let mut st = PrctlState::new();
        let (caught, total) = Injector::run_all(&mut st);
        s.add(
            "J1-11 注入器12/12必抓",
            caught == total,
            "违约样本全数被闭集诊断码拦下",
        );
    }
    // J1-⑫ 对账表 + 段位守卫。
    {
        let mut ok = true;
        for (name, raw, expect) in RECON_TABLE.iter() {
            let _ = name;
            match PrctlOption::from_raw(*raw) {
                Some(po) => {
                    if po.idx() != *expect as usize {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        let seg_ok = SEG_BASE == 0x7D300
            && PrctlErr::ALL
                .iter()
                .all(|e| e.val() >= SEG_BASE && e.val() < SEG_BASE + 0x40);
        s.add(
            "J1-12 对账表与段位守卫",
            ok && seg_ok,
            "RECON_TABLE 十行对号；0x7D300 段步进 0x40 内不越界",
        );
    }
    // J1-⑬ 双跑一致。
    {
        let run = || -> u64 {
            let mut st = PrctlState::new();
            let mut log = String::new();
            let seq: [(i32, i64, &str); 10] = [
                (PR_SET_NAME, 0, "varix"),
                (PR_GET_NAME, 0, ""),
                (PR_SET_NO_NEW_PRIVS, 1, ""),
                (PR_GET_NO_NEW_PRIVS, 0, ""),
                (PR_SET_DUMPABLE, 1, ""),
                (PR_GET_DUMPABLE, 0, ""),
                (PR_SET_PDEATHSIG, 9, ""),
                (PR_GET_PDEATHSIG, 0, ""),
                (PR_SET_TIMERSLACK, 500, ""),
                (PR_GET_TIMERSLACK, 0, ""),
            ];
            for (opt, arg2, nm) in seq.iter() {
                match st.prctl(*opt, *arg2, nm) {
                    Ok(o) => {
                        let line = format!("{},{};", o.opt.idx(), o.ret);
                        log.push_str(&line);
                    }
                    Err(_) => log.push('E'),
                }
            }
            let _ = st.seal();
            fnv1a(log.as_bytes())
        };
        let a = run();
        let b = run();
        s.add("J1-13 双跑一致", a == b, "同输入双跑指纹相等（确定性）");
    }
    // J1-⑭ 封账纪律 + 不变量守恒。
    {
        let mut st = PrctlState::new();
        let _ = st.prctl(PR_SET_NAME, 0, "ktest");
        let _ = st.prctl(PR_SET_NO_NEW_PRIVS, 1, "");
        let inv_before = st.invariant_ok();
        let sealed = st.seal().is_ok() && st.sealed();
        let write_rejected = st.prctl(PR_SET_NAME, 0, "x") == Err(PrctlErr::ESealed.val());
        s.add(
            "J1-14 封账纪律与不变量",
            inv_before && sealed && write_rejected && st.invariant_ok(),
            "封账前不变量绿、封账置位、封账后拒写",
        );
    }
    // 动态明细（≤8 条）：最关键的三条。
    s.add_dyn("账本深度", ledger_depth_ok(), "成功调用一次账本恰增一格");
    s.add_dyn("封账状态", seal_state_ok(), "封账后 sealed 置位且拒写");
    s.add_dyn("指纹非零", fnv_nonzero(), "FNV 指纹非零，可作账锚");
    s
}

// ════════════════════════ 动态明细辅助 ════════════════════════

fn ledger_depth_ok() -> bool {
    let mut st = PrctlState::new();
    let _ = st.prctl(PR_GET_NAME, 0, "");
    st.recs().len() == 1
}

fn seal_state_ok() -> bool {
    let mut st = PrctlState::new();
    let _ = st.prctl(PR_GET_NAME, 0, "");
    st.seal().is_ok() && st.sealed()
}

fn fnv_nonzero() -> bool {
    fnv1a(&[1u8, 2, 3]) != 0
}

// ════════════════════════ 测试 ════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_unknown_option_closed_set() {
        for raw in [-1i32, 0, 99, 17, i32::MIN] {
            let mut st = PrctlState::new();
            assert_eq!(st.prctl(raw, 0, ""), Err(PrctlErr::EOptUnknown.val()));
        }
    }

    #[test]
    fn t02_known_options_zero_false_kill() {
        for (_, raw, _) in RECON_TABLE.iter() {
            let mut st = PrctlState::new();
            let r = match PrctlOption::from_raw(*raw) {
                Some(po) => match po {
                    PrctlOption::SetName => st.prctl(*raw, 0, "v").is_ok(),
                    PrctlOption::SetNoNewPrivs => st.prctl(*raw, 1, "").is_ok(),
                    PrctlOption::SetDumpable => st.prctl(*raw, 1, "").is_ok(),
                    PrctlOption::SetPdeathsig => st.prctl(*raw, 0, "").is_ok(),
                    PrctlOption::SetTimerslack => st.prctl(*raw, 1, "").is_ok(),
                    _ => st.prctl(*raw, 0, "").is_ok(),
                },
                None => false,
            };
            assert!(r, "raw={} 不得误伤", raw);
        }
    }

    #[test]
    fn t03_name_length_boundary() {
        let mut st = PrctlState::new();
        let mut ok15 = String::new();
        let mut k = 0usize;
        while k < 15 {
            ok15.push('a');
            k += 1;
        }
        assert!(st.prctl(PR_SET_NAME, 0, &ok15).is_ok());
        let mut bad16 = ok15.clone();
        bad16.push('b'); // 16 字节 + NUL = 17 > 16
        assert_eq!(st.prctl(PR_SET_NAME, 0, &bad16), Err(PrctlErr::ENameTooLong.val()));
    }

    #[test]
    fn t04_name_inner_nul_truncated_with_accounting() {
        let mut st = PrctlState::new();
        assert!(st.prctl(PR_SET_NAME, 0, "va\0rix").is_ok());
        assert_eq!(st.attrs().name, "va");
        let trunc = match st.recs.last() {
            Some(r) => r.nul_trunc,
            None => false,
        };
        assert!(trunc, "内嵌 NUL 必须记账");
    }

    #[test]
    fn t05_nnp_monotonic_one_way() {
        let mut st = PrctlState::new();
        assert!(st.prctl(PR_SET_NO_NEW_PRIVS, 1, "").is_ok());
        assert_eq!(
            st.prctl(PR_SET_NO_NEW_PRIVS, 0, ""),
            Err(PrctlErr::EPrivDrop.val())
        );
        assert!(st.prctl(PR_SET_NO_NEW_PRIVS, 1, "").is_ok());
    }

    #[test]
    fn t06_get_requires_null_arg() {
        let mut st = PrctlState::new();
        assert_eq!(st.prctl(PR_GET_DUMPABLE, 3, ""), Err(PrctlErr::EGetWithArg.val()));
        assert!(st.prctl(PR_GET_DUMPABLE, 0, "").is_ok());
    }

    #[test]
    fn t07_injector_full_catch() {
        let mut st = PrctlState::new();
        let (caught, total) = Injector::run_all(&mut st);
        assert_eq!(caught, total);
        assert_eq!(total, 12);
    }

    #[test]
    fn t08_ledger_invariant_and_seal() {
        let mut st = PrctlState::new();
        let _ = st.prctl(PR_SET_NAME, 0, "ktest");
        let _ = st.prctl(PR_SET_PDEATHSIG, 9, "");
        assert!(st.invariant_ok());
        assert!(st.seal().is_ok());
        assert_eq!(
            st.prctl(PR_SET_TIMERSLACK, 1, ""),
            Err(PrctlErr::ESealed.val())
        );
        assert!(st.invariant_ok());
    }

    #[test]
    fn t09_diag_codes_closed_set() {
        // 闭集 10 类，val 单调递增且 detail 非空互异。
        let mut prev = 0u32;
        for (i, e) in PrctlErr::ALL.iter().enumerate() {
            assert!(e.val() > prev || i == 0);
            prev = e.val();
            assert!(!e.detail().is_empty());
        }
        // 明细互异（val 由判别式唯一保证，此处再验文本两两不同）。
        for (i, a) in PrctlErr::ALL.iter().enumerate() {
            for (j, b) in PrctlErr::ALL.iter().enumerate() {
                if i < j {
                    assert!(a.detail() != b.detail(), "诊断明细重复");
                }
            }
        }
        assert_eq!(PrctlErr::ALL.len(), 10);
    }

    #[test]
    fn t10_checks_all_pass() {
        let s = run_unx9406_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t11_dual_run_determinism() {
        let run = || {
            let mut st = PrctlState::new();
            let _ = st.prctl(PR_SET_NAME, 0, "det");
            let _ = st.prctl(PR_SET_NO_NEW_PRIVS, 1, "");
            let _ = st.prctl(PR_SET_TIMERSLACK, 777, "");
            let _ = st.seal();
            (
                st.attrs().name.clone(),
                st.attrs().no_new_privs,
                st.recs().len(),
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn t12_seg_base_and_recon() {
        assert_eq!(SEG_BASE, 0x7D300);
        assert_eq!(RECON_TABLE.len(), 10);
        for (name, raw, idx) in RECON_TABLE.iter() {
            match PrctlOption::from_raw(*raw) {
                Some(po) => assert_eq!(po.idx(), *idx as usize, "{} 槽位错", name),
                None => panic_closed_set(name),
            }
        }
    }

    // 测试辅助：闭集缺失即断言失败（不用 unwrap/expect）。
    fn panic_closed_set(name: &str) {
        assert!(false, "RECON_TABLE 项 {} 不在选项闭集", name);
    }
}
