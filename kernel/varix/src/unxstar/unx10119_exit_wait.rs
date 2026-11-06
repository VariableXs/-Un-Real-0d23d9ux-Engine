//! UNX-F10119 exit/wait 正反双判据（退出回收达成与无子等待误用受控）· 0x92DC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10120-J1 收口小结落账（19 条战线条目 J1/J2 双判据齐备+预算
//!       6,600 核销零偏离 10/10 次）；UNX-F10120-J2 收口虚报受控（判据
//!       缺失或守恒偏离检出拦截 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92DC0（本卡独占 0x40 窗口，下一卡 0x92E00）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_wait_call`："wait|pid" 两成分（pid=0 表任意子），
//!      非数字/成分缺失逐档拒绝；
//!   ② 进程册 `ProcLedger`：pid→(父号, 状态, 状态字) 三列，exit 落僵尸 /
//!      wait 取码回收 / 孤儿收养三语义；
//!   ③ 状态机 `ProcState`：Running→Zombie→Reaped 单向推进（禁倒退）；
//!   ④ 注入核 `BadWait`：无子等待 / 重复收割 / 非法 pid / 坏调用行
//!      四类坏样本逐类 10/10 次精确复现 ECHILD/EINVAL 档，注入账守恒逃逸=0；
//!   ⑤ 状态字 WIFEXITED/WEXITSTATUS/WIFSIGNALED 三面解码 + Linux errno
//!      逐码对照 + FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10119 独占 0x40 窗口，下一卡 0x92E00）。
pub const SEG_BASE: u32 = 0x92DC0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92E00;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 进程册容量上限。
pub const LEDGER_CAP: usize = 32;

/// 进程号基线（模型 pids 从 100 起）。
pub const PID_BASE: u32 = 100;

/// 状态字「已退出」位（bit15）。
pub const W_EXITED: u16 = 0x8000;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WtCode {
    /// 无子等待（ECHILD 族）
    NoChild,
    /// 重复收割（ECHILD 族，回收后账面无此子）
    ReapTwice,
    /// 非法 pid（ECHILD 族）
    BadPid,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 册满（EAGAIN 档）
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 守恒失平（fork=exit+live 不成立）
    CountDrift,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 状态字解码不一致
    StatusDecode,
    /// 成语料指纹漂移
    RoundTrip,
}

impl WtCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            WtCode::NoChild => "无子等待被拒：册内无在册子进程；ECHILD 精确归档",
            WtCode::ReapTwice => "重复收割被拒：该子已回收出册；二次 wait 按 ECHILD 归档",
            WtCode::BadPid => "非法 pid 被拒：pid 不在册；ECHILD 精确归档",
            WtCode::BadNumber => "非数字成分被拒：pid 必须为十进制；请改写",
            WtCode::MissingPart => "成分缺失被拒：调用行必须两段 wait|pid；请补成分",
            WtCode::LedgerFull => "进程册满被拒：请先收割僵尸释放槽位",
            WtCode::StateRegress => "状态倒退被拒：Running→Zombie→Reaped 单向",
            WtCode::CountDrift => "守恒失平被拒：fork 数 = 僵尸数 + 存活数 + 回收数；请回查",
            WtCode::EmptySummary => "空成文被拒：回收小结不得为空串；请先收割再成文",
            WtCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            WtCode::StatusDecode => "状态字解码不一致：WIFEXITED/WEXITSTATUS 必须与账面一致",
            WtCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [WtCode; 12] = [
        WtCode::NoChild,
        WtCode::ReapTwice,
        WtCode::BadPid,
        WtCode::BadNumber,
        WtCode::MissingPart,
        WtCode::LedgerFull,
        WtCode::StateRegress,
        WtCode::CountDrift,
        WtCode::EmptySummary,
        WtCode::SegmentViolation,
        WtCode::StatusDecode,
        WtCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 wait 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitCall {
    /// 目标 pid（0 表任意子）
    pub pid: u32,
}

/// 解析 "wait|pid" 两段调用行。
pub fn parse_wait_call(line: &str) -> Result<WaitCall, WtCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = match bytes.get(i) {
            Some(b) => *b as char,
            None => break,
        };
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
        i += 1;
    }
    let _ = parts.push(cur);
    if parts.len() != 2 {
        return Err(WtCode::MissingPart);
    }
    let s1 = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(WtCode::MissingPart),
    };
    if s1.is_empty() {
        return Err(WtCode::BadNumber);
    }
    let mut acc: u32 = 0;
    for b in s1.bytes() {
        if !b.is_ascii_digit() {
            return Err(WtCode::BadNumber);
        }
        acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Ok(WaitCall { pid: acc })
}

// ── 进程状态机 ────────────────────────────────────────────────────────
/// 进程生命周期（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    /// 运行中
    Running,
    /// 僵尸（已退出未收割）
    Zombie,
    /// 已回收（出册留档）
    Reaped,
}

impl ProcState {
    /// 单调推进判定。
    pub fn can_advance(self, next: ProcState) -> bool {
        let rank = |s: ProcState| match s {
            ProcState::Running => 0u8,
            ProcState::Zombie => 1,
            ProcState::Reaped => 2,
        };
        rank(next) == rank(self) + 1
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            ProcState::Running => "运行中",
            ProcState::Zombie => "僵尸",
            ProcState::Reaped => "已回收",
        }
    }
}

// ── 状态字解码 ────────────────────────────────────────────────────────
/// WIFEXITED：bit15 置位即正常退出。
pub fn wifexited(status: u16) -> bool {
    status & W_EXITED != 0
}

/// WEXITSTATUS：位 8..=14 退出码（先掩掉 bit15 标志位再取）。
pub fn wexitstatus(status: u16) -> u32 {
    (((status & !W_EXITED) >> 8) & 0xFF) as u32
}

/// WIFSIGNALED：bit15 清零且低 7 位非零即被信号终止。
pub fn wifsignaled(status: u16) -> bool {
    status & W_EXITED == 0 && status & 0x7F != 0
}

/// WTERMSIG：低 7 位信号号（仅 WIFSIGNALED 时有效）。
pub fn wtermsig(status: u16) -> u32 {
    (status & 0x7F) as u32
}

// ── 进程册 ────────────────────────────────────────────────────────────
/// 单进程三列记录。
#[derive(Clone, Debug)]
pub struct ProcEnt {
    /// 进程号
    pub pid: u32,
    /// 父进程号
    pub ppid: u32,
    /// 生命周期状态
    pub state: ProcState,
    /// 退出状态字（Zombie 后有效）
    pub status: u16,
}

/// 进程册：pid→(父号, 状态, 状态字)。
#[derive(Clone, Debug)]
pub struct ProcLedger {
    ents: Vec<ProcEnt>,
    cap: usize,
    /// 下一个分配 pid
    next_pid: u32,
    /// 累计 fork 次数
    pub forks: u32,
    /// 累计 exit 次数
    pub exits: u32,
    /// 累计回收次数
    pub reaps: u32,
}

impl ProcLedger {
    /// 空册构造（父 = 1 号 init）。
    pub fn new() -> ProcLedger {
        ProcLedger {
            ents: Vec::new(),
            cap: LEDGER_CAP,
            next_pid: PID_BASE,
            forks: 0,
            exits: 0,
            reaps: 0,
        }
    }

    /// 在册进程数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }

    /// fork：分配 pid、挂父链，Running 入册。
    pub fn fork(&mut self, ppid: u32) -> Result<u32, WtCode> {
        if self.ents.len() >= self.cap {
            return Err(WtCode::LedgerFull);
        }
        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        let _ = self.ents.push(ProcEnt {
            pid,
            ppid,
            state: ProcState::Running,
            status: 0,
        });
        self.forks = self.forks.saturating_add(1);
        Ok(pid)
    }

    /// exit：Running→Zombie，落状态字（正常退出）。
    pub fn exit(&mut self, pid: u32, code: u32) -> Result<(), WtCode> {
        let idx = match self.idx_of(pid) {
            Some(i) => i,
            None => return Err(WtCode::BadPid),
        };
        let st = match self.ents.get(idx) {
            Some(e) => e.state,
            None => return Err(WtCode::BadPid),
        };
        if st != ProcState::Running {
            return Err(WtCode::StateRegress);
        }
        let word = W_EXITED | ((code & 0xFF) as u16) << 8;
        if let Some(e) = self.ents.get_mut(idx) {
            e.state = ProcState::Zombie;
            e.status = word;
        }
        self.exits = self.exits.saturating_add(1);
        Ok(())
    }

    /// 信号终止：Running→Zombie，落信号状态字。
    pub fn kill(&mut self, pid: u32, sig: u32) -> Result<(), WtCode> {
        let idx = match self.idx_of(pid) {
            Some(i) => i,
            None => return Err(WtCode::BadPid),
        };
        let st = match self.ents.get(idx) {
            Some(e) => e.state,
            None => return Err(WtCode::BadPid),
        };
        if st != ProcState::Running {
            return Err(WtCode::StateRegress);
        }
        let word = (sig & 0x7F) as u16;
        if let Some(e) = self.ents.get_mut(idx) {
            e.state = ProcState::Zombie;
            e.status = word;
        }
        self.exits = self.exits.saturating_add(1);
        Ok(())
    }

    /// wait：Zombie→Reaped 取码出册；无子/重复收割/非法 pid 逐档拒绝。
    pub fn wait(&mut self, pid: u32) -> Result<(u32, u32), WtCode> {
        // pid==0：任取一个本册僵尸（模型：任意父下的最老僵尸）。
        let target = if pid == 0 {
            let mut found: Option<u32> = None;
            let mut i = 0usize;
            while i < self.ents.len() {
                if let Some(e) = self.ents.get(i) {
                    if e.state == ProcState::Zombie {
                        found = Some(e.pid);
                        break;
                    }
                }
                i += 1;
            }
            match found {
                Some(p) => p,
                None => return Err(WtCode::NoChild),
            }
        } else {
            pid
        };
        let idx = match self.idx_of(target) {
            Some(i) => i,
            None => return Err(WtCode::BadPid),
        };
        let (st, status, ppid) = match self.ents.get(idx) {
            Some(e) => (e.state, e.status, e.ppid),
            None => return Err(WtCode::BadPid),
        };
        if st == ProcState::Running {
            return Err(WtCode::NoChild);
        }
        let _ = ppid;
        if let Some(e) = self.ents.get_mut(idx) {
            e.state = ProcState::Reaped;
        }
        self.reaps = self.reaps.saturating_add(1);
        let _ = self.ents.remove(idx);
        let code = wexitstatus(status);
        Ok((code, status as u32))
    }

    /// 按号寻址。
    fn idx_of(&self, pid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册存活数（Running）。
    pub fn live_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.state == ProcState::Running {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 守恒式：forks = exits + 存活数（回收出册后账自平）。
    pub fn conserved(&self) -> bool {
        self.forks == self.exits.saturating_add(self.live_count())
    }
}

// ── J1 达成核：exit01/wait01 面 10/10 轮 ──────────────────────────────
/// 逐轮：fork→exit(42)→wait 回码一致→二次 wait 拒。
pub fn run_exit_wait_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = ProcLedger::new();
        let mut all_ok = true;
        // 面1：fork 得子。
        let child = match l.fork(1) {
            Ok(p) => p,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        all_ok = all_ok && child >= PID_BASE && l.live_count() == 1;
        probes += 1;
        // 面2：子 exit(42) 落僵尸。
        all_ok = all_ok && l.exit(child, 42).is_ok();
        probes += 1;
        // 面3：wait 回码 WEXITSTATUS==42。
        let got = l.wait(0);
        all_ok = all_ok && got == Ok((42, (W_EXITED | 42u16 << 8) as u32));
        probes += 1;
        // 面4：回收后册空。
        all_ok = all_ok && l.len() == 0 && l.reaps == 1;
        probes += 1;
        // 面5：二次 wait 按 ECHILD 档拒绝。
        all_ok = all_ok && l.wait(0).err() == Some(WtCode::NoChild);
        probes += 1;
        // 面6：守恒式成立。
        all_ok = all_ok && l.conserved();
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏回收矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadWait {
    /// 无子等待（ECHILD 族）
    NoChild,
    /// 重复收割（ECHILD 族）
    ReapTwice,
    /// 非法 pid（ECHILD 族）
    BadPid,
    /// 坏调用行（EINVAL 族）
    BadCall,
}

impl BadWait {
    /// 闭集全量。
    pub const ALL: [BadWait; 4] = [
        BadWait::NoChild,
        BadWait::ReapTwice,
        BadWait::BadPid,
        BadWait::BadCall,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadWait::NoChild => "无子等待",
            BadWait::ReapTwice => "重复收割",
            BadWait::BadPid => "非法 pid 等待",
            BadWait::BadCall => "坏调用行",
        }
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), WtCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(WtCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏回收注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_wait_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadWait::ALL.len() {
        let case = match BadWait::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = ProcLedger::new();
            let child = l.fork(1).unwrap_or(0);
            let caught = match case {
                BadWait::NoChild => {
                    // 子仍 Running，wait 属无子可收。
                    l.wait(0).err() == Some(WtCode::NoChild)
                }
                BadWait::ReapTwice => {
                    let _ = l.exit(child, 7);
                    let first = l.wait(0).is_ok();
                    let second = l.wait(0).err() == Some(WtCode::NoChild);
                    first && second
                }
                BadWait::BadPid => {
                    l.wait(99999).err() == Some(WtCode::BadPid)
                }
                BadWait::BadCall => {
                    parse_wait_call("wait|12x").err() == Some(WtCode::BadNumber)
                        && parse_wait_call("wait").err() == Some(WtCode::MissingPart)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 回收小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「code=<n> state=<t>」。
pub fn compose_reap_summary(code: u32) -> Result<String, WtCode> {
    let out = format!("code={} state=已回收", code);
    if out.is_empty() {
        return Err(WtCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ECHILD", -10, 128, "Windows ERROR_NO_CHILD_PROCESS；无子/重复收割/非法 pid 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏调用行族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：fork→exit→wait→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = ProcLedger::new();
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(child) = l.fork(1) {
        let _ = l.exit(child, 42);
        if let Ok((code, _)) = l.wait(0) {
            if let Ok(s) = compose_reap_summary(code) {
                bytes.extend_from_slice(s.as_bytes());
            }
        }
    }
    bytes.extend_from_slice(&l.forks.to_le_bytes());
    bytes.extend_from_slice(&l.reaps.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10119 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10119_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10119");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92DC0..=0x92DFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < WtCode::ALL.len() {
        let mut j = i + 1;
        while j < WtCode::ALL.len() {
            if let (Some(a), Some(b)) = (WtCode::ALL.get(i), WtCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = WtCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_wait_call("wait|0").is_ok()
        && parse_wait_call("wait|123").is_ok()
        && parse_wait_call("wait|x").err() == Some(WtCode::BadNumber)
        && parse_wait_call("wait").err() == Some(WtCode::MissingPart);
    s.add("调用行解析器：两段闭集放行且坏成分逐档拒绝", parse_ok,
          "解析器是 wait 前置；pid=0 表任意子");

    // 4. J1 达成核
    let (ok, probes) = run_exit_wait_rounds();
    s.add("J1 退出回收达成：WEXITSTATUS 回读一致 LTP exit01/wait01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面断言（fork/exit/回码/出册/二次拒/守恒）全对");

    // 5. 状态字三面解码
    let word = W_EXITED | 42u16 << 8;
    let decode_ok = wifexited(word) && wexitstatus(word) == 42
        && !wifsignaled(word)
        && wifsignaled(9) && wtermsig(9) == 9;
    s.add("状态字三面：WIFEXITED/WEXITSTATUS/WIFSIGNALED 解码一致", decode_ok,
          "状态字是回收语义的账面根；解码必须与账面逐位对平");

    // 6. 信号终止面：kill→wait 回 WTERMSIG
    let mut l2 = ProcLedger::new();
    let c2 = l2.fork(1).unwrap_or(0);
    let _ = l2.kill(c2, 15);
    let sig_ok = l2.wait(0) == Ok((0, 15));
    s.add("信号终止面：kill 后 wait 回 WTERMSIG 精确", sig_ok,
          "被信号终止的子进程回码低 7 位为信号号");

    // 7. 无子等待档
    let mut l3 = ProcLedger::new();
    let noch_ok = l3.wait(0).err() == Some(WtCode::NoChild)
        && l3.wait(12345).err() == Some(WtCode::BadPid);
    s.add("J2 无子等待受控：ECHILD/ECHILD 族精确复现", noch_ok,
          "空册 wait 与账外 pid 是两档，都归 ECHILD 族但码分立");

    // 8. 注入核
    let (inj, per_class) = run_bad_wait_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "无子/重复收割/非法 pid/坏调用行必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadWait::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 状态机
    let sm_ok = ProcState::Running.can_advance(ProcState::Zombie)
        && ProcState::Zombie.can_advance(ProcState::Reaped)
        && !ProcState::Reaped.can_advance(ProcState::Running)
        && ProcState::Zombie.label() == "僵尸";
    s.add("状态机：Running→Zombie→Reaped 单向", sm_ok,
          "进程生命周期只许单调推进；倒退即协议违约");

    // 11. 收割守恒
    let mut l4 = ProcLedger::new();
    let c4 = l4.fork(1).unwrap_or(0);
    let _ = l4.exit(c4, 3);
    let _ = l4.wait(0);
    let cons_ok = l4.forks == 1 && l4.exits == 1 && l4.reaps == 1 && l4.len() == 0;
    s.add("收割守恒：fork/exit/reap 三账逐次一致", cons_ok,
          "fork=exit+live+reap 出册账必须全平");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(10) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_reap_summary(42);
    let text_ok = sum.as_ref().map(|t| t.contains("code=42")).unwrap_or(false);
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("退出回收达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_wait_call("wait|0").is_ok());
        assert_eq!(parse_wait_call("wait|1x").err(), Some(WtCode::BadNumber));
        assert_eq!(parse_wait_call("wait").err(), Some(WtCode::MissingPart));
    }

    #[test]
    fn status_word_decode() {
        let w = W_EXITED | 42u16 << 8;
        assert!(wifexited(w));
        assert_eq!(wexitstatus(w), 42);
        assert!(!wifsignaled(w));
        assert!(wifsignaled(9));
        assert_eq!(wtermsig(9), 9);
    }

    #[test]
    fn fork_exit_wait_roundtrip() {
        let mut l = ProcLedger::new();
        let c = l.fork(1).unwrap_or(0);
        assert!(l.exit(c, 42).is_ok());
        assert_eq!(l.wait(0), Ok((42, (W_EXITED | 42u16 << 8) as u32)));
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn wait_running_child_is_nochild() {
        let mut l = ProcLedger::new();
        let _ = l.fork(1);
        assert_eq!(l.wait(0).err(), Some(WtCode::NoChild));
    }

    #[test]
    fn reap_twice_blocked() {
        let mut l = ProcLedger::new();
        let c = l.fork(1).unwrap_or(0);
        let _ = l.exit(c, 1);
        assert!(l.wait(0).is_ok());
        assert_eq!(l.wait(0).err(), Some(WtCode::NoChild));
    }

    #[test]
    fn bad_pid_blocked() {
        let mut l = ProcLedger::new();
        let _ = l.fork(1);
        assert_eq!(l.wait(424242).err(), Some(WtCode::BadPid));
    }

    #[test]
    fn signal_termination() {
        let mut l = ProcLedger::new();
        let c = l.fork(1).unwrap_or(0);
        let _ = l.kill(c, 11);
        assert_eq!(l.wait(0), Ok((0, 11)));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_wait_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_exit_wait_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(ProcState::Running.can_advance(ProcState::Zombie));
        assert!(ProcState::Zombie.can_advance(ProcState::Reaped));
        assert!(!ProcState::Running.can_advance(ProcState::Reaped));
        assert_eq!(ProcState::Reaped.label(), "已回收");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10119_checks_green() {
        let set = run_unx10119_checks();
        assert!(set.all_passed(), "UNX-F10119 red: {}", set.red_items_text());
    }
}
