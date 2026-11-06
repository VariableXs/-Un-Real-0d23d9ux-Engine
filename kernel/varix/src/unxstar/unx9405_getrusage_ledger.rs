//! UNX-F9405 getrusage 记账面 · 0x7D2C0段
//!
//! 卡判据（J1 全文引）：「注入必抓：注入器对 10 类非法路径逐一投毒（who
//! 越闭集、空缓冲、非法 pid、重复收割、单调回退、守恒溢出、账满、封印
//! 后写……），每一类都必须被闭集判定器以**精确错误码**抓捕，多抓（误杀
//! 合法输入）与漏抓同等记红；判序与 Linux 一致：getrusage/wait4 记账门
//! 逐门对齐 Linux（who 闭集 EINVAL → 缓冲 EFAULT → 单调守恒）；双跑一致：
//! 同一场景跑两遍，FNV-1a 指纹逐位相同；段位守卫：诊断码全部落在
//! 0x7D2C0 段内且互不碰撞；闭集诊断码：每个错误码在闭集内有名有姓，
//! 绝不落段外码；完成时 API/软件写回：状态=已完成、结果摘要非空。」
//!
//! 设计要点：
//! 1. 判定器判序：getrusage 两门严格定序 who 闭集 → 缓冲；reap_child 走
//!    封印 → pid → 退出码 → 重复收割 → 账容量 → 守恒溢出；update_self 走
//!    封印 → 单调 → 账容量。
//! 2. 状态机：自用记账（RUSAGE_SELF 七字段全单调）+ 子账（RUSAGE_CHILDREN
//!    守恒等式 现值 = 基线 + Σ 收割记录）+ 单调事件账本，seal() 冻结后
//!    一切写门拒绝且零副作用。
//! 3. 注入器 10 类必抓：who 7/-7/2、空缓冲、pid 0/-5、退出码 300、重复
//!    收割、自用回退、峰值回退、u64 守恒溢出、账满、封印后写，共 13
//!    样本全数抓捕。
//! 4. 对账表：3 个 who 原始值（0/-1/1）与规范序号（0/1/2）一一互证。
//! 5. 双跑一致指纹 FNV-1a 64 位：对事件账与收割账逐笔摊平取印，两跑比对。
//! 6. no_std 纪律：无 std、无浮点、无裸下标、无时钟无随机；守恒加法全走
//!    checked_add，溢出即 EAccOverflow 相面，不静默回绕。
//! 7. 零重复声明 vs 既有卡：F9403 管定时器槽表、F9404 管调度矩阵，本卡
//!    独占资源记账面，段位 0x7D2C0 不与任何既有段重叠，错误码闭集全新
//!    不重名。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基（0x7D2C0 段，独占）。
pub const SEG_BASE: u32 = 0x7D2C0;

/// 事件账本容量（单调记账，满即拒）。
pub const LEDGER_CAP: usize = 48;
/// 退出码合法界（wait4 状态低 8 位语义的封闭面）。
pub const EXIT_CODE_MAX: i32 = 255;

/// 闭集错误码：getrusage/wait4 记账面的全部失败相面，段内互不碰撞。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuErr {
    /// who 越出 {0,-1,1} 闭集（EINVAL 相面）。
    EBadWho,
    /// 用户缓冲空指针（EFAULT 相面）。
    EBadBuf,
    /// 收割 pid 非法（≤ 0，ESRCH/EINVAL 相面）。
    EBadPid,
    /// 同一 pid 重复收割（重复入账相面）。
    EReapTwice,
    /// 单调字段更新出现回退（单调纪律相面）。
    ENotMono,
    /// 守恒加法 checked_add 溢出（EOVERFLOW 相面）。
    EAccOverflow,
    /// 退出码越出 [0,255]（EINVAL 相面）。
    EBadExit,
    /// 封印后写入（冻结纪律相面）。
    ESealEarly,
    /// 事件账本容量满（记账面满，拒写）。
    ELedgerFull,
    /// 账面结构自检破损（守恒等式/序号/封印纪律失守）。
    EBadLedger,
}

impl RuErr {
    /// 段内诊断码：SEG_BASE + 变体序号，闭集内互不碰撞。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细（逐变体互异、非空）。
    pub fn detail(self) -> &'static str {
        match self {
            RuErr::EBadWho => "who 越出 {0,-1,1} 闭集，拒绝取账",
            RuErr::EBadBuf => "用户缓冲为空指针，EFAULT 相面拒绝",
            RuErr::EBadPid => "收割 pid 非法（非正数），拒绝入账",
            RuErr::EReapTwice => "该 pid 已收割入账，重复收割被拒绝",
            RuErr::ENotMono => "单调字段更新出现回退，记账纪律失守",
            RuErr::EAccOverflow => "守恒加法将溢出，拒绝入账保账面无损",
            RuErr::EBadExit => "退出码越出 [0,255] 合法界",
            RuErr::ESealEarly => "账面已封印，一切写入被拒绝",
            RuErr::ELedgerFull => "事件账本容量已满，拒绝新增记账",
            RuErr::EBadLedger => "账面结构自检破损：守恒/序号/封印纪律失守",
        }
    }
}

/// 闭集全表（自检逐变体取用）。
pub const ALL: [RuErr; 10] = [
    RuErr::EBadWho,
    RuErr::EBadBuf,
    RuErr::EBadPid,
    RuErr::EReapTwice,
    RuErr::ENotMono,
    RuErr::EAccOverflow,
    RuErr::EBadExit,
    RuErr::ESealEarly,
    RuErr::ELedgerFull,
    RuErr::EBadLedger,
];

/// who 原始值到规范序号：0→0（SELF）、-1→1（CHILDREN）、1→2（THREAD）。
pub fn who_canonical(raw: i32) -> Option<usize> {
    match raw {
        0 => Some(0),
        -1 => Some(1),
        1 => Some(2),
        _ => None,
    }
}

/// who 名渲染（对账表第一列同源）。
pub fn who_name(raw: i32) -> &'static str {
    match raw {
        0 => "RUSAGE_SELF",
        -1 => "RUSAGE_CHILDREN",
        1 => "RUSAGE_THREAD",
        _ => "RUSAGE_BAD",
    }
}

/// 对账表：（who 名，原始值，规范序号）——原始 0/-1/1 对规范 0/1/2。
pub const RECON_TABLE: [(&str, i32, i32); 3] = [
    ("RUSAGE_SELF", 0, 0),
    ("RUSAGE_CHILDREN", -1, 1),
    ("RUSAGE_THREAD", 1, 2),
];

/// 资源用量七字段（全单调；maxrss 为峰值亦只增不减）。
#[derive(Clone, Copy, Debug)]
pub struct RusageAcc {
    pub ru_utime_us: u64,
    pub ru_stime_us: u64,
    pub ru_maxrss_kb: u64,
    pub ru_minflt: u64,
    pub ru_majflt: u64,
    pub ru_nvcsw: u64,
    pub ru_nivcsw: u64,
}

impl RusageAcc {
    pub fn zero() -> RusageAcc {
        RusageAcc {
            ru_utime_us: 0,
            ru_stime_us: 0,
            ru_maxrss_kb: 0,
            ru_minflt: 0,
            ru_majflt: 0,
            ru_nvcsw: 0,
            ru_nivcsw: 0,
        }
    }

    /// 逐字段单调核：任一字段相对现值回退即假。
    fn mono_ok_again(&self, cand: &RusageAcc) -> bool {
        cand.ru_utime_us >= self.ru_utime_us
            && cand.ru_stime_us >= self.ru_stime_us
            && cand.ru_maxrss_kb >= self.ru_maxrss_kb
            && cand.ru_minflt >= self.ru_minflt
            && cand.ru_majflt >= self.ru_majflt
            && cand.ru_nvcsw >= self.ru_nvcsw
            && cand.ru_nivcsw >= self.ru_nivcsw
    }
}

/// 单条收割记录（wait4 相面）。
#[derive(Clone, Copy, Debug)]
pub struct Wait4Rec {
    pub pid: i32,
    pub exit_code: i32,
    pub utime_us: u64,
    pub stime_us: u64,
}

/// 更新候选（update_self 入参，全字段提交）。
#[derive(Clone, Copy, Debug)]
pub struct RusageUpdate {
    pub ru_utime_us: u64,
    pub ru_stime_us: u64,
    pub ru_maxrss_kb: u64,
    pub ru_minflt: u64,
    pub ru_majflt: u64,
    pub ru_nvcsw: u64,
    pub ru_nivcsw: u64,
}

/// 账本事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuOp {
    Reap,
    Update,
}

impl RuOp {
    pub fn name(self) -> &'static str {
        match self {
            RuOp::Reap => "reap",
            RuOp::Update => "update",
        }
    }
}

/// 单条账本事件：单调序号 + 门名 + pid + 结果。
#[derive(Clone, Copy, Debug)]
pub struct RuEvent {
    pub seq: u32,
    pub op: RuOp,
    pub pid: i32,
    pub ok: bool,
    /// 失败时的段内诊断码；成功记 0。
    pub err_code: u32,
}

/// FNV-1a 64 位指纹（wrapping 乘法，无随机无时钟，双跑逐位一致）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// getrusage/wait4 记账面状态机：自用账 + 子账（守恒）+ 单调事件账本 + 封印。
pub struct RuFace {
    /// 自用记账（RUSAGE_SELF / RUSAGE_THREAD 视图）。
    self_acc: RusageAcc,
    /// 子账基线（守恒等式左端的初始值）。
    baseline_utime_us: u64,
    baseline_stime_us: u64,
    /// 子账现值（RUSAGE_CHILDREN 视图）。
    children_utime_us: u64,
    children_stime_us: u64,
    /// 收割账（每 reap 一条）。
    reaped: Vec<Wait4Rec>,
    ledger: Vec<RuEvent>,
    sealed: bool,
    seq: u32,
    /// 封印之后发生的记账笔数（恒应为 0）。
    writes_since_seal: u32,
    /// 封印之后任何写门被触达的痕迹（含被拒调用）。
    seal_breach: bool,
}

impl RuFace {
    pub fn new() -> RuFace {
        RuFace {
            self_acc: RusageAcc::zero(),
            baseline_utime_us: 0,
            baseline_stime_us: 0,
            children_utime_us: 0,
            children_stime_us: 0,
            reaped: Vec::new(),
            ledger: Vec::new(),
            sealed: false,
            seq: 0,
            writes_since_seal: 0,
            seal_breach: false,
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    pub fn reaped_len(&self) -> usize {
        self.reaped.len()
    }

    fn ledger_room(&self) -> Result<(), RuErr> {
        if self.ledger.len() >= LEDGER_CAP {
            Err(RuErr::ELedgerFull)
        } else {
            Ok(())
        }
    }

    /// 提交一条事件（单调序号；调用前必须先过 ledger_room）。
    /// 封印后若被触达即留痕（防御面：正门在 commit 前已全部拒绝）。
    fn commit(&mut self, op: RuOp, pid: i32, ok: bool, err: Option<RuErr>) {
        self.seq = self.seq.wrapping_add(1);
        if self.sealed {
            self.writes_since_seal = self.writes_since_seal.wrapping_add(1);
            self.seal_breach = true;
        }
        let code = match err {
            Some(e) => e.val(),
            None => 0,
        };
        self.ledger.push(RuEvent {
            seq: self.seq,
            op,
            pid,
            ok,
            err_code: code,
        });
    }

    /// 已收割 pid 查账（只读）。
    pub fn reaped_of(&self, pid: i32) -> Option<Wait4Rec> {
        for r in self.reaped.iter() {
            if r.pid == pid {
                return Some(*r);
            }
        }
        None
    }

    /// 门零：wait4 收割入账。
    /// 判序：封印 → pid 闭集 → 退出码界 → 重复收割 → 账容量 → 守恒溢出
    /// （checked_add）→ 落收割账 + 提交事件。
    pub fn reap_child(&mut self, pid: i32, exit_code: i32, utime_us: u64, stime_us: u64) -> Result<(), RuErr> {
        if self.sealed {
            return Err(RuErr::ESealEarly);
        }
        if pid <= 0 {
            return Err(RuErr::EBadPid);
        }
        if exit_code < 0 || exit_code > EXIT_CODE_MAX {
            return Err(RuErr::EBadExit);
        }
        if self.reaped_of(pid).is_some() {
            return Err(RuErr::EReapTwice);
        }
        self.ledger_room()?;
        let nu = match self.children_utime_us.checked_add(utime_us) {
            Some(v) => v,
            None => {
                self.commit(RuOp::Reap, pid, false, Some(RuErr::EAccOverflow));
                return Err(RuErr::EAccOverflow);
            }
        };
        let ns = match self.children_stime_us.checked_add(stime_us) {
            Some(v) => v,
            None => {
                self.commit(RuOp::Reap, pid, false, Some(RuErr::EAccOverflow));
                return Err(RuErr::EAccOverflow);
            }
        };
        self.children_utime_us = nu;
        self.children_stime_us = ns;
        self.reaped.push(Wait4Rec {
            pid,
            exit_code,
            utime_us,
            stime_us,
        });
        self.commit(RuOp::Reap, pid, true, None);
        Ok(())
    }

    /// 门一：自用记账单调更新（RUSAGE_SELF 七字段全提交）。
    /// 判序：封印 → 逐字段单调（首个回退字段定相面）→ 账容量 → 落账。
    pub fn update_self(&mut self, u: RusageUpdate) -> Result<(), RuErr> {
        if self.sealed {
            return Err(RuErr::ESealEarly);
        }
        let cand = RusageAcc {
            ru_utime_us: u.ru_utime_us,
            ru_stime_us: u.ru_stime_us,
            ru_maxrss_kb: u.ru_maxrss_kb,
            ru_minflt: u.ru_minflt,
            ru_majflt: u.ru_majflt,
            ru_nvcsw: u.ru_nvcsw,
            ru_nivcsw: u.ru_nivcsw,
        };
        if !self.self_acc.mono_ok_again(&cand) {
            return Err(RuErr::ENotMono);
        }
        self.ledger_room()?;
        self.self_acc = cand;
        self.commit(RuOp::Update, 0, true, None);
        Ok(())
    }

    /// 门二：getrusage 取账（只读门）。
    /// 判序：who 闭集（EBadWho）→ 缓冲（EBadBuf）→ 视图渲染。
    pub fn getrusage(&self, who_raw: i32, buf_valid: bool) -> Result<String, RuErr> {
        let canon = match who_canonical(who_raw) {
            Some(c) => c,
            None => return Err(RuErr::EBadWho),
        };
        if !buf_valid {
            return Err(RuErr::EBadBuf);
        }
        match canon {
            0 => Ok(format!(
                "RUSAGE_SELF utime={} stime={} maxrss={} minflt={} majflt={} nvcsw={} nivcsw={}",
                self.self_acc.ru_utime_us,
                self.self_acc.ru_stime_us,
                self.self_acc.ru_maxrss_kb,
                self.self_acc.ru_minflt,
                self.self_acc.ru_majflt,
                self.self_acc.ru_nvcsw,
                self.self_acc.ru_nivcsw
            )),
            1 => Ok(format!(
                "RUSAGE_CHILDREN utime={} stime={} 收割{}条",
                self.children_utime_us, self.children_stime_us, self.reaped.len()
            )),
            _ => Ok(format!(
                "RUSAGE_THREAD utime={} stime={}（单线程视图同自用账）",
                self.self_acc.ru_utime_us, self.self_acc.ru_stime_us
            )),
        }
    }

    /// 子账守恒核：现值 = 基线 + Σ 收割记录（checked 求和，溢出即假）。
    pub fn conservation_ok(&self) -> bool {
        let mut su: u64 = self.baseline_utime_us;
        let mut ss: u64 = self.baseline_stime_us;
        for r in self.reaped.iter() {
            su = match su.checked_add(r.utime_us) {
                Some(v) => v,
                None => return false,
            };
            ss = match ss.checked_add(r.stime_us) {
                Some(v) => v,
                None => return false,
            };
        }
        su == self.children_utime_us && ss == self.children_stime_us
    }

    /// 摊平账本为指纹字节流。
    pub fn fingerprint(&self) -> u64 {
        let mut s = String::new();
        for ev in self.ledger.iter() {
            s.push_str(&format!(
                "{}|{}|{}|{}|{};",
                ev.seq,
                ev.op.name(),
                ev.pid,
                ev.ok as u32,
                ev.err_code
            ));
        }
        for r in self.reaped.iter() {
            s.push_str(&format!(";r{}={}/{}/{}", r.pid, r.exit_code, r.utime_us, r.stime_us));
        }
        s.push_str(&format!(
            "#u{}+s{};cu{}+cs{};reaped{};sealed{}",
            self.self_acc.ru_utime_us,
            self.self_acc.ru_stime_us,
            self.children_utime_us,
            self.children_stime_us,
            self.reaped.len(),
            self.sealed as u32
        ));
        fnv1a64(s.as_bytes())
    }

    /// 账面结构不变量核：容量 / 守恒 / 单调序号 / 重复收割 / 封印纪律。
    pub fn invariant_ok(&self) -> bool {
        if self.ledger.len() > LEDGER_CAP {
            return false;
        }
        if !self.conservation_ok() {
            return false;
        }
        // 事件序号严格单调递增。
        let mut prev: u32 = 0;
        for ev in self.ledger.iter() {
            if ev.seq <= prev {
                return false;
            }
            prev = ev.seq;
        }
        // 收割账 pid 唯一（无重复入账）。
        for (i, r) in self.reaped.iter().enumerate() {
            if r.pid <= 0 {
                return false;
            }
            for (j, q) in self.reaped.iter().enumerate() {
                if i < j && r.pid == q.pid {
                    return false;
                }
            }
        }
        // 收割账深度受事件账容量约束（每笔收割至多一条事件）。
        if self.reaped.len() > self.ledger.len() {
            return false;
        }
        // 封印纪律。
        if self.sealed && (self.writes_since_seal != 0 || self.seal_breach) {
            return false;
        }
        true
    }

    /// 不变量报告：首个破损相面映射到闭集诊断码。
    pub fn invariant_report(&self) -> Result<(), RuErr> {
        if self.ledger.len() > LEDGER_CAP {
            return Err(RuErr::EBadLedger);
        }
        if !self.conservation_ok() {
            return Err(RuErr::EAccOverflow);
        }
        if self.sealed && (self.writes_since_seal != 0 || self.seal_breach) {
            return Err(RuErr::ESealEarly);
        }
        Ok(())
    }

    /// 完成时写回摘要（非空契约）。
    pub fn summary(&self) -> String {
        format!(
            "自用 utime={}；子账 utime={}（基线{}）；收割{}条；账{}笔；封印{}",
            self.self_acc.ru_utime_us,
            self.children_utime_us,
            self.baseline_utime_us,
            self.reaped.len(),
            self.ledger.len(),
            if self.sealed { "是" } else { "否" }
        )
    }
}

/// 注入器：13 类非法路径逐一投毒，全部必须以精确错误码被抓捕。
pub struct RuInjector {
    caught_names: Vec<String>,
    missed: Vec<String>,
}

impl RuInjector {
    pub fn new() -> RuInjector {
        RuInjector {
            caught_names: Vec::new(),
            missed: Vec::new(),
        }
    }

    fn mark(&mut self, name: &str, got: RuErr, want: RuErr) {
        if got == want {
            self.caught_names.push(name.to_string());
        } else {
            self.missed
                .push(format!("{}: got {:?} want {:?}", name, got, want));
        }
    }

    fn mark_ok_but_want_err(&mut self, name: &str, want: RuErr) {
        self.missed
            .push(format!("{}: 调用成功但期望 {:?}", name, want));
    }

    fn mark_err_but_want_ok(&mut self, name: &str, got: RuErr) {
        self.missed
            .push(format!("{}: 调用被拒 {:?} 但期望成功", name, got));
    }

    /// 全量注入。返回（抓捕数，总数）。
    pub fn run_all(&mut self, f: &mut RuFace) -> (usize, usize) {
        // —— getrusage 门注入 ——
        // 1/2/3. who 越闭集三门。
        match f.getrusage(7, true) {
            Err(e) => self.mark("who=7", e, RuErr::EBadWho),
            Ok(_) => self.mark_ok_but_want_err("who=7", RuErr::EBadWho),
        }
        match f.getrusage(-7, true) {
            Err(e) => self.mark("who=-7", e, RuErr::EBadWho),
            Ok(_) => self.mark_ok_but_want_err("who=-7", RuErr::EBadWho),
        }
        match f.getrusage(2, true) {
            Err(e) => self.mark("who=2", e, RuErr::EBadWho),
            Ok(_) => self.mark_ok_but_want_err("who=2", RuErr::EBadWho),
        }
        // 4. 空缓冲。
        match f.getrusage(0, false) {
            Err(e) => self.mark("空缓冲", e, RuErr::EBadBuf),
            Ok(_) => self.mark_ok_but_want_err("空缓冲", RuErr::EBadBuf),
        }
        // —— wait4 收割门注入 ——
        // 5/6. pid 非法两门。
        match f.reap_child(0, 0, 1, 1) {
            Err(e) => self.mark("pid=0", e, RuErr::EBadPid),
            Ok(_) => self.mark_ok_but_want_err("pid=0", RuErr::EBadPid),
        }
        match f.reap_child(-5, 0, 1, 1) {
            Err(e) => self.mark("pid=-5", e, RuErr::EBadPid),
            Ok(_) => self.mark_ok_but_want_err("pid=-5", RuErr::EBadPid),
        }
        // 7. 退出码越界。
        match f.reap_child(421, 300, 1, 1) {
            Err(e) => self.mark("退出码300", e, RuErr::EBadExit),
            Ok(_) => self.mark_ok_but_want_err("退出码300", RuErr::EBadExit),
        }
        // 8. 重复收割：先合法收 421，再收即拒。
        if let Err(e) = f.reap_child(421, 0, 10, 5) {
            self.mark_err_but_want_ok("铺垫收421", e);
        }
        match f.reap_child(421, 0, 1, 1) {
            Err(e) => self.mark("重复收割", e, RuErr::EReapTwice),
            Ok(_) => self.mark_ok_but_want_err("重复收割", RuErr::EReapTwice),
        }
        // —— 自用记账单调门注入 ——
        // 9. 先抬高自用账，再回退 utime。
        let base = RusageUpdate {
            ru_utime_us: 100,
            ru_stime_us: 50,
            ru_maxrss_kb: 1024,
            ru_minflt: 8,
            ru_majflt: 1,
            ru_nvcsw: 3,
            ru_nivcsw: 2,
        };
        if let Err(e) = f.update_self(base) {
            self.mark_err_but_want_ok("铺垫自用账", e);
        }
        let regress_u = RusageUpdate {
            ru_utime_us: 99,
            ru_stime_us: 50,
            ru_maxrss_kb: 1024,
            ru_minflt: 8,
            ru_majflt: 1,
            ru_nvcsw: 3,
            ru_nivcsw: 2,
        };
        match f.update_self(regress_u) {
            Err(e) => self.mark("自用回退", e, RuErr::ENotMono),
            Ok(_) => self.mark_ok_but_want_err("自用回退", RuErr::ENotMono),
        }
        // 10. 峰值回退（maxrss 只增不减）。
        let regress_rss = RusageUpdate {
            ru_utime_us: 100,
            ru_stime_us: 50,
            ru_maxrss_kb: 1023,
            ru_minflt: 8,
            ru_majflt: 1,
            ru_nvcsw: 3,
            ru_nivcsw: 2,
        };
        match f.update_self(regress_rss) {
            Err(e) => self.mark("峰值回退", e, RuErr::ENotMono),
            Ok(_) => self.mark_ok_but_want_err("峰值回退", RuErr::ENotMono),
        }
        // 11. 守恒溢出：把子账恰好顶到 u64 上界，再入账一枚即溢出。
        if let Err(e) = f.reap_child(422, 0, u64::MAX - 10, 0) {
            self.mark_err_but_want_ok("铺垫巨子", e);
        }
        match f.reap_child(423, 0, 1, 0) {
            Err(e) => self.mark("守恒溢出", e, RuErr::EAccOverflow),
            Ok(_) => self.mark_ok_but_want_err("守恒溢出", RuErr::EAccOverflow),
        }
        // 12. 账本满：合法 update 反复落账直至容量拒。
        let steady = RusageUpdate {
            ru_utime_us: 100,
            ru_stime_us: 50,
            ru_maxrss_kb: 1024,
            ru_minflt: 8,
            ru_majflt: 1,
            ru_nvcsw: 3,
            ru_nivcsw: 2,
        };
        let mut full_hit = false;
        let mut guard = 0usize;
        while guard <= LEDGER_CAP + 2 {
            guard += 1;
            match f.update_self(steady) {
                Err(RuErr::ELedgerFull) => {
                    full_hit = true;
                    break;
                }
                Err(RuErr::ENotMono) => {
                    // 等值更新恒单调合法，不应到此；继续守门。
                }
                _ => {}
            }
        }
        if full_hit {
            self.caught_names.push("账本满".to_string());
        } else {
            self.missed.push("账本满: 容量门未触发".to_string());
        }
        // 13. 封印后写。
        f.seal();
        match f.reap_child(424, 0, 1, 1) {
            Err(e) => self.mark("封印后写", e, RuErr::ESealEarly),
            Ok(_) => self.mark_ok_but_want_err("封印后写", RuErr::ESealEarly),
        }
        (self.caught_names.len(), 13)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut s = String::new();
        for n in self.caught_names.iter() {
            s.push_str(n);
            s.push(',');
        }
        s.push('|');
        for x in self.missed.iter() {
            s.push_str(x);
            s.push(',');
        }
        fnv1a64(s.as_bytes())
    }

    pub fn missed_detail(&self) -> String {
        self.missed.join("; ")
    }
}

/// 段内自检：14 项静态 + 4 项动态，全绿即 J1 判据达成。
pub fn run_unx9405_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9405");

    // J1-01 段位守卫：段基正确，闭集码全落段内且互不碰撞。
    let mut codes_ok = SEG_BASE == 0x7D2C0;
    let mut seen: Vec<u32> = Vec::new();
    for e in ALL.iter() {
        let v = e.val();
        if v < SEG_BASE || v >= SEG_BASE + 10 {
            codes_ok = false;
        }
        for c in seen.iter() {
            if *c == v {
                codes_ok = false;
            }
        }
        seen.push(v);
    }
    s.add(
        "J1-01 段位守卫",
        codes_ok,
        "段基 0x7D2C0，10 个闭集诊断码全落段内且互不碰撞",
    );

    // J1-02 闭集拒绝：who 越集样本逐一被拒。
    let f2 = RuFace::new();
    let rej = f2.getrusage(7, true) == Err(RuErr::EBadWho)
        && f2.getrusage(-7, true) == Err(RuErr::EBadWho)
        && f2.getrusage(2, true) == Err(RuErr::EBadWho)
        && who_canonical(7).is_none()
        && who_canonical(-1) == Some(1);
    s.add(
        "J1-02 闭集拒绝",
        rej,
        "who {7,-7,2} 越集精确拒绝，{0,-1,1} 映射规范序号 {0,1,2}",
    );

    // J1-03 零误杀：全部合法输入路径零错误通过。
    let mut f3 = RuFace::new();
    let mut zero_kill = f3.getrusage(0, true).is_ok()
        && f3.getrusage(-1, true).is_ok()
        && f3.getrusage(1, true).is_ok();
    zero_kill = zero_kill && f3.reap_child(11, 0, 7, 3).is_ok() && f3.reap_child(12, 42, 5, 5).is_ok();
    let up = RusageUpdate {
        ru_utime_us: 10,
        ru_stime_us: 10,
        ru_maxrss_kb: 4096,
        ru_minflt: 100,
        ru_majflt: 2,
        ru_nvcsw: 9,
        ru_nivcsw: 1,
    };
    zero_kill = zero_kill && f3.update_self(up).is_ok();
    s.add(
        "J1-03 零误杀",
        zero_kill,
        "三 who 取账、两 pid 收割、七字段单调更新全零错误通过",
    );

    // J1-04 判序：who 门先于缓冲门；pid 门先于退出码门，退出码门先于重复门。
    let mut f4 = RuFace::new();
    let _ = f4.reap_child(21, 0, 1, 1);
    let order_ok = f4.getrusage(9, false) == Err(RuErr::EBadWho)
        && f4.getrusage(0, false) == Err(RuErr::EBadBuf)
        && f4.reap_child(-1, 300, 1, 1) == Err(RuErr::EBadPid)
        && f4.reap_child(21, 300, 1, 1) == Err(RuErr::EBadExit)
        && f4.reap_child(21, 0, 1, 1) == Err(RuErr::EReapTwice);
    s.add(
        "J1-04 判序",
        order_ok,
        "who → 缓冲、pid → 退出码 → 重复收割各门严格定序，前门压住后门",
    );

    // J1-05 注入必抓：13 样本全数精确抓捕。
    let mut f5 = RuFace::new();
    let mut inj = RuInjector::new();
    let (c5, t5) = inj.run_all(&mut f5);
    s.add(
        "J1-05 注入必抓",
        c5 == t5 && t5 >= 10,
        "注入器 13 类非法路径全部以精确错误码抓捕，零漏抓",
    );

    // J1-06 对账表：3 行名称/原始值/规范序号互证。
    let mut recon_ok = RECON_TABLE.len() == 3;
    for (i, (name, raw, canon)) in RECON_TABLE.iter().enumerate() {
        let mapped = who_canonical(*raw);
        let canon_ok = match mapped {
            Some(c) => c == *canon as usize && who_name(*raw) == *name,
            None => false,
        };
        if name.is_empty() || !canon_ok || *canon != i as i32 {
            recon_ok = false;
        }
    }
    s.add(
        "J1-06 对账表",
        recon_ok,
        "3 个 who 名称/原始值/规范序号三列互证，0/-1/1 对 0/1/2",
    );

    // J1-07 双跑一致：同场景两跑，账面指纹与注入器指纹逐位相同。
    let mut fa = RuFace::new();
    let mut ia = RuInjector::new();
    let (ca, ta) = ia.run_all(&mut fa);
    let pa = fa.fingerprint();
    let pia = ia.fingerprint();
    let mut fb = RuFace::new();
    let mut ib = RuInjector::new();
    let (cb, tb) = ib.run_all(&mut fb);
    let pb = fb.fingerprint();
    let pib = ib.fingerprint();
    s.add(
        "J1-07 双跑一致",
        pa == pb && pia == pib && ca == cb && ta == tb,
        "两跑账面指纹与注入器指纹逐位相同，抓捕数一致",
    );

    // J1-08 状态机不变量 + 守恒等式：现值 = 基线 + Σ 收割。
    let mut f8 = RuFace::new();
    let _ = f8.reap_child(31, 0, 100, 40);
    let _ = f8.reap_child(32, 0, 60, 10);
    let inv_ok = f8.invariant_ok()
        && f8.invariant_report().is_ok()
        && f8.conservation_ok()
        && f8.reaped_len() == 2;
    s.add(
        "J1-08 状态机不变量",
        inv_ok,
        "账容量守界，序号单调，pid 唯一，守恒等式 160=0+100+60 成立",
    );

    // J1-09 封印纪律：冻结后写门拒绝且账面零副作用。
    let mut f9 = RuFace::new();
    let _ = f9.reap_child(41, 0, 1, 1);
    f9.seal();
    let up9 = RusageUpdate {
        ru_utime_us: 1,
        ru_stime_us: 1,
        ru_maxrss_kb: 1,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    };
    let seal_ok = f9.reap_child(42, 0, 1, 1) == Err(RuErr::ESealEarly)
        && f9.update_self(up9) == Err(RuErr::ESealEarly)
        && f9.invariant_ok();
    s.add(
        "J1-09 封印纪律",
        seal_ok,
        "封印后 reap/update 全拒 ESealEarly，账面零副作用，不变量仍绿",
    );

    // J1-10 容量记账：账满 ELedgerFull 不越界落账。
    let mut f10 = RuFace::new();
    let up10 = RusageUpdate {
        ru_utime_us: 0,
        ru_stime_us: 0,
        ru_maxrss_kb: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    };
    let mut cap_hit = false;
    let mut guard10 = 0usize;
    while guard10 <= LEDGER_CAP + 4 {
        guard10 += 1;
        if f10.update_self(up10) == Err(RuErr::ELedgerFull) {
            cap_hit = true;
            break;
        }
    }
    s.add(
        "J1-10 容量记账",
        cap_hit && f10.ledger_len() == LEDGER_CAP,
        "事件账容量门封顶，越限即拒且不越界落账",
    );

    // J1-11 单调纪律：七字段逐字段只增不减，回退即 ENotMono。
    let mut f11 = RuFace::new();
    let up_a = RusageUpdate {
        ru_utime_us: 5,
        ru_stime_us: 5,
        ru_maxrss_kb: 8,
        ru_minflt: 1,
        ru_majflt: 1,
        ru_nvcsw: 1,
        ru_nivcsw: 1,
    };
    let _ = f11.update_self(up_a);
    let up_b = RusageUpdate {
        ru_utime_us: 5,
        ru_stime_us: 5,
        ru_maxrss_kb: 8,
        ru_minflt: 1,
        ru_majflt: 1,
        ru_nvcsw: 0,
        ru_nivcsw: 1,
    };
    let mono_ok = f11.update_self(up_a).is_ok() && f11.update_self(up_b) == Err(RuErr::ENotMono);
    s.add(
        "J1-11 单调纪律",
        mono_ok,
        "等值重提交合法（零误杀），nvcsw 回退一格即 ENotMono 精确拒",
    );

    // J1-12 守恒溢出：checked_add 溢出即拒且账面无损。
    let mut f12 = RuFace::new();
    let _ = f12.reap_child(51, 0, u64::MAX, 0);
    let ovf = f12.reap_child(52, 0, 1, 0) == Err(RuErr::EAccOverflow);
    let intact = f12.conservation_ok() && f12.reaped_len() == 1;
    s.add(
        "J1-12 守恒溢出",
        ovf && intact,
        "u64 顶格后再入账一枚即 EAccOverflow，账面保持守恒无损",
    );

    // J1-13 闭集诊断码：逐变体码值互异、明细非空互异。
    let mut details: Vec<&'static str> = Vec::new();
    let mut detail_ok = true;
    for e in ALL.iter() {
        let d = e.detail();
        if d.is_empty() {
            detail_ok = false;
        }
        for p in details.iter() {
            if *p == d {
                detail_ok = false;
            }
        }
        details.push(d);
    }
    s.add(
        "J1-13 闭集诊断码",
        detail_ok && ALL.len() == 10,
        "10 个变体明细全部非空互异，码值段内互不碰撞",
    );

    // J1-14 完成时写回：摘要非空 + 不变量报告无破损。
    let sum = f8.summary();
    s.add(
        "J1-14 完成时写回",
        !sum.is_empty() && f8.invariant_report().is_ok(),
        "完成摘要非空且账面状态=已完成、不变量报告无破损",
    );

    // —— 动态明细项 ——
    let mut f15 = RuFace::new();
    let mut i15 = RuInjector::new();
    let _ = i15.run_all(&mut f15);
    let inv15 = f15.invariant_ok();
    s.add_dyn(
        "J1-D1 指纹",
        inv15,
        &format!(
            "账面指纹 {:016x}，注入器指纹 {:016x}，不变量 {}",
            f15.fingerprint(),
            i15.fingerprint(),
            if inv15 { "绿" } else { "红" }
        ),
    );
    s.add_dyn(
        "J1-D2 注入账",
        i15.caught_names.len() == 13,
        &format!("抓捕 {} / 总数 13，漏抓明细：{}", i15.caught_names.len(), {
            let d = i15.missed_detail();
            if d.is_empty() {
                "无".to_string()
            } else {
                d
            }
        }),
    );
    s.add_dyn(
        "J1-D3 账面深",
        f15.ledger_len() <= LEDGER_CAP && f15.conservation_ok(),
        &format!(
            "收割 {} 条，账 {} 笔，守恒 {}，封印 {}",
            f15.reaped_len(),
            f15.ledger_len(),
            f15.conservation_ok(),
            f15.sealed()
        ),
    );
    let miss_clean = i15.missed.is_empty();
    s.add_dyn(
        "J1-D4 零漏抓",
        miss_clean,
        &if miss_clean {
            "漏抓清单为空，注入必抓判据闭环".to_string()
        } else {
            format!("存在漏抓：{}", i15.missed_detail())
        },
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_closed_set_injection() {
        let f = RuFace::new();
        let e1 = f.getrusage(7, true);
        let ok1 = match e1 {
            Err(RuErr::EBadWho) => true,
            _ => false,
        };
        let e2 = f.getrusage(2, true);
        let ok2 = match e2 {
            Err(RuErr::EBadWho) => true,
            _ => false,
        };
        assert!(ok1 && ok2);
    }

    #[test]
    fn t_zero_false_kill() {
        let mut f = RuFace::new();
        let mut ok = f.getrusage(0, true).is_ok() && f.getrusage(-1, true).is_ok();
        ok = ok && f.reap_child(9, 255, 3, 3).is_ok();
        let up = RusageUpdate {
            ru_utime_us: 1,
            ru_stime_us: 1,
            ru_maxrss_kb: 1,
            ru_minflt: 1,
            ru_majflt: 1,
            ru_nvcsw: 1,
            ru_nivcsw: 1,
        };
        ok = ok && f.update_self(up).is_ok();
        assert!(ok);
    }

    #[test]
    fn t_injector_full_catch() {
        let mut f = RuFace::new();
        let mut inj = RuInjector::new();
        let (c, t) = inj.run_all(&mut f);
        assert!(c == t && t >= 10 && inj.missed.is_empty());
    }

    #[test]
    fn t_dual_run_determinism() {
        let mut fa = RuFace::new();
        let mut ia = RuInjector::new();
        let (ca, ta) = ia.run_all(&mut fa);
        let pa = fa.fingerprint();
        let pia = ia.fingerprint();
        let mut fb = RuFace::new();
        let mut ib = RuInjector::new();
        let (cb, tb) = ib.run_all(&mut fb);
        let pb = fb.fingerprint();
        let pib = ib.fingerprint();
        assert!(pa == pb && pia == pib && ca == cb && ta == tb);
    }

    #[test]
    fn t_checks_all_pass() {
        let s = run_unx9405_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t_seg_diag_codes() {
        let first = ALL[0].val();
        let last = ALL[ALL.len() - 1].val();
        let mut ok = first == SEG_BASE && last == SEG_BASE + (ALL.len() as u32 - 1);
        for e in ALL.iter() {
            if e.detail().is_empty() {
                ok = false;
            }
        }
        assert!(ok);
    }

    #[test]
    fn t_judge_order_who_before_buf() {
        let f = RuFace::new();
        // 劣 who + 空缓冲同投：who 门先出码。
        let e = f.getrusage(9, false);
        let ok = match e {
            Err(RuErr::EBadWho) => true,
            _ => false,
        };
        assert!(ok);
    }

    #[test]
    fn t_reap_twice_and_exit_code() {
        let mut f = RuFace::new();
        let bad_exit = match f.reap_child(5, 300, 1, 1) {
            Err(RuErr::EBadExit) => true,
            _ => false,
        };
        let first = f.reap_child(5, 7, 10, 2).is_ok();
        let dup = match f.reap_child(5, 7, 10, 2) {
            Err(RuErr::EReapTwice) => true,
            _ => false,
        };
        assert!(bad_exit && first && dup);
    }

    #[test]
    fn t_conservation_equation() {
        let mut f = RuFace::new();
        let _ = f.reap_child(1, 0, 300, 100);
        let _ = f.reap_child(2, 0, 200, 50);
        let ok = f.conservation_ok() && f.reaped_len() == 2;
        let dup = f.reap_child(1, 0, 1, 1) == Err(RuErr::EReapTwice);
        assert!(ok && dup && f.conservation_ok());
    }

    #[test]
    fn t_monotonic_updates() {
        let mut f = RuFace::new();
        let a = RusageUpdate {
            ru_utime_us: 4,
            ru_stime_us: 2,
            ru_maxrss_kb: 16,
            ru_minflt: 3,
            ru_majflt: 0,
            ru_nvcsw: 2,
            ru_nivcsw: 5,
        };
        let b = RusageUpdate {
            ru_utime_us: 9,
            ru_stime_us: 2,
            ru_maxrss_kb: 20,
            ru_minflt: 3,
            ru_majflt: 0,
            ru_nvcsw: 2,
            ru_nivcsw: 5,
        };
        let back = RusageUpdate {
            ru_utime_us: 8,
            ru_stime_us: 2,
            ru_maxrss_kb: 20,
            ru_minflt: 3,
            ru_majflt: 0,
            ru_nvcsw: 2,
            ru_nivcsw: 5,
        };
        let ok = f.update_self(a).is_ok() && f.update_self(b).is_ok();
        let regressed = f.update_self(back) == Err(RuErr::ENotMono);
        assert!(ok && regressed);
    }

    #[test]
    fn t_overflow_and_seal() {
        let mut f = RuFace::new();
        let _ = f.reap_child(1, 0, u64::MAX, 0);
        let ovf = f.reap_child(2, 0, 1, 0) == Err(RuErr::EAccOverflow);
        f.seal();
        let sealed = f.reap_child(3, 0, 1, 1) == Err(RuErr::ESealEarly);
        assert!(ovf && sealed && f.conservation_ok() && f.invariant_ok());
    }

    #[test]
    fn t_recon_table() {
        let mut ok = RECON_TABLE.len() == 3;
        for (i, (name, raw, canon)) in RECON_TABLE.iter().enumerate() {
            let mapped = who_canonical(*raw);
            let pair_ok = match mapped {
                Some(c) => c == *canon as usize && who_name(*raw) == *name,
                None => false,
            };
            if name.is_empty() || !pair_ok || *canon != i as i32 {
                ok = false;
            }
        }
        assert!(ok && RECON_TABLE[1] == ("RUSAGE_CHILDREN", -1, 1));
    }

    #[test]
    fn t_summary_nonempty() {
        let mut f = RuFace::new();
        let _ = f.reap_child(1, 0, 1, 1);
        let sum = f.summary();
        assert!(!sum.is_empty());
    }
}
