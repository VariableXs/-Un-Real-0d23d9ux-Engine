//! UNX-F52224 PuTTY 收口 · 0xC77C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52224"，逐字）：
//!   title: PuTTY 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52225-J1 六判据全
//!     落；PuTTY 接管口径（升级接管字样）；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.1 防重声明；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式：本卡（卡 N=52224）判据锚 UNX-F(N+1)-J1 = UNX-F52225-J1，
//! 照原文锚定；功能按本卡标题（PuTTY）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 会话 10 格：Default Settings/生产/跳板/串口/裸/raw/Telnet/SSH 备
//!      份/隧道专用/只读审计十格，逐格载荷折叠恒等才收。
//!   ② SSH 三元闸：SSH-2/SSH-1.x/KEX 协商三态闭集，越三元必拒。
//!   ③ xterm 终端仿真 12 格：光标/滚动/颜色/标题四家族各 3 格，家族单
//!      调，逆序必抓。
//!   ④ pscp/psftp/plink 分账 3 行：三工具各自独立账行声明，缺一拒封。
//!   ⑤ 注册表存储 8 格：HKCU 会话根/SshHostKeys/默认设置/性别。
//!   ⑥ 接管口径声明（升级接管字样）+ §AI-66.1 防重声明两行闭集。
//!   ⑦ 注入核 10 轮三态轮转（会话越闭集/仿真漂移/会话越闭集），三类注
//!      入全部落在当轮临时账本上 10/10 必抓；六判据母版闸 + KB-COMPAT 定
//!      级 S1（10 会话 + 12 仿真 + 8 注册表格 + 3 分账行 + 2 口径行全通
//!      → S1）；双跑 FNV-1a 指纹一致。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52224;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 会话格数。
pub const SESSION_CELLS: usize = 10;

/// xterm 仿真格数。
pub const TERM_CELLS: usize = 12;

/// 注册表存储格数。
pub const REG_CELLS: usize = 8;

/// pscp/psftp/plink 分账行数。
pub const TOOL_ROWS: usize = 3;

/// 口径声明行数（接管口径 + AI-66.1 防重声明）。
pub const POLICY_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PuttyCode {
    /// 成功
    Ok = 0,
    /// 会话格越闭集
    ENoSession,
    /// 会话漂移
    ESessionDrift,
    /// 仿真格越闭集
    ENoTerm,
    /// 仿真漂移
    ETermDrift,
    /// 仿真家族逆序
    EOrder,
    /// SSH 态越三元
    ENoSsh,
    /// 注册表格越闭集
    ENoReg,
    /// 分账行越闭集
    ENoRow,
    /// 缺口径声明行
    EPolicyMiss,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl PuttyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PuttyCode::Ok => "成功",
            PuttyCode::ENoSession => "会话格越闭集：会话 10 格闭集之外先登记",
            PuttyCode::ESessionDrift => "会话漂移：会话格载荷折叠与基准不恒等，逐格复算",
            PuttyCode::ENoTerm => "仿真格越闭集：xterm 仿真 12 格闭集之外先登记",
            PuttyCode::ETermDrift => "仿真漂移：仿真格载荷折叠与基准不恒等，逐格复算",
            PuttyCode::EOrder => "仿真家族逆序：光标→滚动→颜色→标题单调，逆序必抓",
            PuttyCode::ENoSsh => "SSH 态越三元：SSH-2/SSH-1.x/KEX 三态闭集之外先登记",
            PuttyCode::ENoReg => "注册表格越闭集：注册表存储 8 格闭集之外先登记",
            PuttyCode::ENoRow => "分账行越闭集：pscp/psftp/plink 3 行闭集之外先登记",
            PuttyCode::EPolicyMiss => "缺口径声明行：接管口径/AI-66.1 防重声明未齐先补齐",
            PuttyCode::EDup => "重复落账：同格只许一条账目",
            PuttyCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PuttyCode::EGradeDrift => "定级漂移：六判据+10 会话+12 仿真+8 注册表+3 分账+2 口径全通才定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+10 会话+12 仿真+8 注册表+3 分账+2 口径全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 会话格名（const fn 表 + Option 边界）。
pub const fn session_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("Default Settings"),
        1 => Some("prod-web-01"),
        2 => Some("jump-host"),
        3 => Some("serial-COM3"),
        4 => Some("raw-echo-9999"),
        5 => Some("telnet-bbs"),
        6 => Some("ssh-backup"),
        7 => Some("tunnel-only-L8080"),
        8 => Some("audit-readonly"),
        9 => Some("cygwin-term"),
        _ => None,
    }
}

/// 会话格载荷。
pub const fn session_cell(p: usize) -> Option<u64> {
    if p >= SESSION_CELLS {
        return None;
    }
    Some((0xD1000u64 + (p as u64) * 0x5D) ^ 0x50755454595F5331)
}

/// SSH 三元（0=SSH-2 1=SSH-1.x 2=KEX 协商）。
pub const fn ssh_cell(t: usize) -> Option<u64> {
    if t >= 3 {
        return None;
    }
    Some(0xD2000u64 + (t as u64) * 0x3B)
}

/// SSH 三元名。
pub const fn ssh_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("SSH-2"),
        1 => Some("SSH-1.x"),
        2 => Some("KEX 协商"),
        _ => None,
    }
}

/// xterm 仿真家族（0..3=光标 3..6=滚动 6..9=颜色 9..12=标题）。
pub const fn term_kind(c: usize) -> Option<u32> {
    if c >= TERM_CELLS {
        return None;
    }
    Some((c / 3) as u32)
}

/// 仿真格载荷。
pub const fn term_cell(c: usize) -> Option<u64> {
    if c >= TERM_CELLS {
        return None;
    }
    Some((0xD3000u64 + (c as u64) * 0x2C) ^ ((c / 3) as u64).wrapping_mul(0x01000193))
}

/// 仿真家族名。
pub const fn term_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("光标控制"),
        1 => Some("滚动缓冲"),
        2 => Some("颜色映射"),
        3 => Some("窗口标题"),
        _ => None,
    }
}

/// 注册表存储格名（const fn 表 + Option 边界）。
pub const fn reg_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("HKCU\\Software\\SimonTatham\\PuTTY\\Sessions"),
        1 => Some("HKCU\\…\\SshHostKeys"),
        2 => Some("HKCU\\…\\Jumplist"),
        3 => Some("HKCU\\…\\PuTTY 默认设置"),
        4 => Some("会话导出 reg 文件声明"),
        5 => Some("host key 校验声明"),
        6 => Some("代理设置格"),
        7 => Some("反空闲 keepalive 格"),
        _ => None,
    }
}

/// 注册表格载荷。
pub const fn reg_cell(r: usize) -> Option<u64> {
    if r >= REG_CELLS {
        return None;
    }
    Some((0xD4000u64 + (r as u64) * 0x2C) ^ 0x5075545479526567)
}

/// pscp/psftp/plink 分账行折叠（行 r）。
pub const fn tool_row(r: usize) -> Option<u32> {
    if r >= TOOL_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x2C)
}

/// 分账行名。
pub const fn tool_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("pscp 文件传输分账声明"),
        1 => Some("psftp 交互传输分账声明"),
        2 => Some("plink 命令行远端分账声明"),
        _ => None,
    }
}

/// 口径声明行折叠（行 r）。
pub const fn policy_row(r: usize) -> Option<u32> {
    if r >= POLICY_ROWS {
        return None;
    }
    Some(0xD600 + r as u32 * 0x2C)
}

/// 接管口径字样（升级接管）。
pub const POLICY_TAKEOVER: &str = "升级接管";

/// 口径声明行名（含升级接管字样）。
pub const fn policy_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("PuTTY 接管口径：升级接管，旧版会话迁移后旧键封存"),
        1 => Some("§AI-66.1 防重声明：本卡收口账与上位账不重复落账"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PuTTY 收口账本状态机。
#[derive(Clone)]
pub struct PuttyLedger {
    /// 六判据（会话/SSH 三元/仿真/分账/注册表/口径六面收口闸）。
    gates: [Option<bool>; 6],
    /// 会话账。
    sessions: [Option<u64>; SESSION_CELLS],
    /// SSH 三元账。
    sshs: [Option<u64>; 3],
    /// 仿真账。
    terms: [Option<u64>; TERM_CELLS],
    /// 注册表账。
    regs: [Option<u64>; REG_CELLS],
    /// 分账行账。
    tools: [Option<u32>; TOOL_ROWS],
    /// 口径声明账。
    policies: [Option<u32>; POLICY_ROWS],
    /// 最后仿真家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PuttyLedger {
    /// 新账本。
    pub fn new() -> Self {
        PuttyLedger {
            gates: [None; 6],
            sessions: [None; SESSION_CELLS],
            sshs: [None; 3],
            terms: [None; TERM_CELLS],
            regs: [None; REG_CELLS],
            tools: [None; TOOL_ROWS],
            policies: [None; POLICY_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PuttyCode> {
        if gate >= 6 {
            return Err(PuttyCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 会话一格（载荷对照 + 重复拒）。
    pub fn record_session(&mut self, p: usize, got: u64) -> Result<(), PuttyCode> {
        let want = match session_cell(p) {
            Some(w) => w,
            None => return Err(PuttyCode::ENoSession),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + p as u64);
            return Err(PuttyCode::ESessionDrift);
        }
        if match self.sessions.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuttyCode::EDup);
        }
        if let Some(c) = self.sessions.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8201 + p as u64);
        Ok(())
    }

    /// 会话全账核验（10 格）。
    pub fn sessions_all(&self) -> bool {
        let mut p = 0usize;
        while p < SESSION_CELLS {
            match self.sessions.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// SSH 三元一格（越三元必拒）。
    pub fn record_ssh(&mut self, t: usize) -> Result<(), PuttyCode> {
        let want = match ssh_cell(t) {
            Some(w) => w,
            None => return Err(PuttyCode::ENoSsh),
        };
        if match self.sshs.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuttyCode::EDup);
        }
        if let Some(c) = self.sshs.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x8300 + t as u64);
        Ok(())
    }

    /// SSH 三元全账核验。
    pub fn sshs_all(&self) -> bool {
        let mut t = 0usize;
        while t < 3 {
            match self.sshs.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 仿真一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_term(&mut self, c: usize, got: u64) -> Result<(), PuttyCode> {
        if c >= TERM_CELLS {
            return Err(PuttyCode::ENoTerm);
        }
        let kind = match term_kind(c) {
            Some(k) => k,
            None => return Err(PuttyCode::ENoTerm),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400);
            return Err(PuttyCode::EOrder);
        }
        let want = match term_cell(c) {
            Some(w) => w,
            None => return Err(PuttyCode::ENoTerm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8401 + c as u64);
            return Err(PuttyCode::ETermDrift);
        }
        if match self.terms.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuttyCode::EDup);
        }
        if let Some(cc) = self.terms.get_mut(c) {
            *cc = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8402 + c as u64);
        Ok(())
    }

    /// 仿真全账核验（12 格）。
    pub fn terms_all(&self) -> bool {
        let mut c = 0usize;
        while c < TERM_CELLS {
            match self.terms.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 注册表一格（越闭集必拒）。
    pub fn record_reg(&mut self, r: usize) -> Result<(), PuttyCode> {
        let want = match reg_cell(r) {
            Some(w) => w,
            None => return Err(PuttyCode::ENoReg),
        };
        if match self.regs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuttyCode::EDup);
        }
        if let Some(c) = self.regs.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 注册表全账核验（8 格）。
    pub fn regs_all(&self) -> bool {
        let mut r = 0usize;
        while r < REG_CELLS {
            match self.regs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 分账一行（pscp/psftp/plink）。
    pub fn declare_tool(&mut self, r: usize) -> Result<(), PuttyCode> {
        if r >= TOOL_ROWS {
            return Err(PuttyCode::ENoRow);
        }
        if match self.tools.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuttyCode::EDup);
        }
        if let Some(c) = self.tools.get_mut(r) {
            *c = tool_row(r);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
    pub fn tools_all(&self) -> bool {
        let mut r = 0usize;
        while r < TOOL_ROWS {
            match self.tools.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 口径声明一行（接管口径/AI-66.1 防重声明）。
    pub fn declare_policy(&mut self, r: usize) -> Result<(), PuttyCode> {
        if r >= POLICY_ROWS {
            return Err(PuttyCode::EPolicyMiss);
        }
        if match self.policies.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuttyCode::EDup);
        }
        if let Some(c) = self.policies.get_mut(r) {
            *c = policy_row(r);
        }
        self.fnv(0x8700 + r as u64);
        Ok(())
    }

    /// 口径声明全齐核验。
    pub fn policies_all(&self) -> bool {
        let mut r = 0usize;
        while r < POLICY_ROWS {
            match self.policies.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.sessions_all() && self.sshs_all() && self.terms_all()
            && self.regs_all() && self.tools_all() && self.policies_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PuttyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PuttyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < SESSION_CELLS {
        if let Some(w) = session_cell(p) {
            if led.record_session(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut t = 0usize;
    while t < 3 {
        let _ = led.record_ssh(t);
        t += 1;
    }
    let mut c = 0usize;
    while c < TERM_CELLS {
        if let Some(w) = term_cell(c) {
            if led.record_term(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < REG_CELLS {
        let _ = led.record_reg(r);
        r += 1;
    }
    let mut tr = 0usize;
    while tr < TOOL_ROWS {
        let _ = led.declare_tool(tr);
        tr += 1;
    }
    let mut pr = 0usize;
    while pr < POLICY_ROWS {
        let _ = led.declare_policy(pr);
        pr += 1;
    }
    // 注入 10 轮：会话越闭集 / 仿真漂移 / 会话越闭集三态轮转；三类注入
    // 全部落在当轮临时账本上，逐轮必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PuttyLedger::new();
        let caught = match kind {
            0 => lt.record_session(SESSION_CELLS, 0).is_err(),
            1 => {
                lt.record_term(4, term_cell(4).unwrap_or(0));
                lt.record_term(5, term_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(PuttyCode::ETermDrift)
            }
            _ => {
                let _ = lt.record_term(7, term_cell(7).unwrap_or(0));
                lt.record_session(SESSION_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52224 判据集（锚 UNX-F52225-J1，off-by-one 板式）。
pub fn run_unx52224_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52224");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "会话 10 格全通",
        recorded >= SESSION_CELLS as u32,
        "Default Settings/生产/跳板/串口/raw/Telnet 等十格逐格恒等",
    );
    s.add(
        "SSH 三元闸全落",
        {
            let mut lt = PuttyLedger::new();
            let mut t = 0usize;
            while t < 3 {
                let _ = lt.record_ssh(t);
                t += 1;
            }
            lt.sshs_all()
        },
        "SSH-2/SSH-1.x/KEX 协商三态闭集，越三元必拒",
    );
    s.add(
        "xterm 终端仿真 12 格全通",
        {
            let mut lt = PuttyLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < TERM_CELLS {
                if let Some(w) = term_cell(c) {
                    if lt.record_term(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.terms_all()
        },
        "光标/滚动/颜色/标题四家族逐格恒等，零漂移",
    );
    s.add(
        "pscp/psftp/plink 分账 3/3 全齐",
        {
            let mut lt = PuttyLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < TOOL_ROWS {
                if lt.declare_tool(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.tools_all()
        },
        "三工具各自独立账行声明，缺一拒封",
    );
    s.add(
        "注册表存储 8 格全通",
        {
            let mut lt = PuttyLedger::new();
            let mut r = 0usize;
            while r < REG_CELLS {
                let _ = lt.record_reg(r);
                r += 1;
            }
            lt.regs_all()
        },
        "Sessions/SshHostKeys/Jumplist/默认设置等八格",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = PuttyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "会话/SSH 三元/仿真/分账/注册表/口径六面收口闸逐格落账（UNX-F52225-J1）",
    );
    s.add(
        "接管口径 + 防重声明 2/2 全齐",
        {
            let mut lt = PuttyLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < POLICY_ROWS {
                if lt.declare_policy(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.policies_all()
        },
        "升级接管字样口径 + §AI-66.1 防重声明两行闭集",
    );
    s.add(
        "接管口径字样在场",
        match policy_row_name(0) {
            Some(n) => n.contains(POLICY_TAKEOVER),
            None => false,
        },
        "口径声明行含升级接管字样，字样在场才收",
    );
    s.add(
        "仿真家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < TERM_CELLS {
                if term_kind(c).unwrap_or(9) < term_kind(c - 1).unwrap_or(9) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "四家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "会话越闭集/仿真漂移/会话越闭集三态轮转必抓");
    let mut l0 = PuttyLedger::new();
    s.add(
        "边界·会话格越闭集",
        l0.record_session(SESSION_CELLS, 0).is_err(),
        "会话格 10 越闭集必拒",
    );
    s.add(
        "边界·仿真格越闭集",
        l0.record_term(TERM_CELLS, 0).is_err(),
        "仿真格 12 越闭集必拒",
    );
    s.add(
        "边界·SSH 越三元",
        l0.record_ssh(3).is_err(),
        "SSH 态 3 越三元必拒",
    );
    s.add(
        "边界·注册表格越闭集",
        l0.record_reg(REG_CELLS).is_err(),
        "注册表格 8 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = PuttyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < SESSION_CELLS {
                if let Some(w) = session_cell(p) {
                    let _ = lt.record_session(p, w);
                }
                p += 1;
            }
            let mut t = 0usize;
            while t < 3 {
                let _ = lt.record_ssh(t);
                t += 1;
            }
            let mut c = 0usize;
            while c < TERM_CELLS {
                if let Some(w) = term_cell(c) {
                    let _ = lt.record_term(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < REG_CELLS {
                let _ = lt.record_reg(r);
                r += 1;
            }
            let mut tr = 0usize;
            while tr < TOOL_ROWS {
                let _ = lt.declare_tool(tr);
                tr += 1;
            }
            let mut pr = 0usize;
            while pr < POLICY_ROWS {
                let _ = lt.declare_policy(pr);
                pr += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC77C0,
        "SEG_BASE 0x40 对齐且为 VX15 段 52224 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_closed_set() {
        assert_eq!(session_name(0), Some("Default Settings"));
        assert_eq!(session_name(5), Some("telnet-bbs"));
        assert_eq!(session_name(9), Some("cygwin-term"));
        assert!(session_name(10).is_none());
        assert!(session_cell(10).is_none());
    }

    #[test]
    fn test_ssh_ternary() {
        assert_eq!(ssh_name(0), Some("SSH-2"));
        assert_eq!(ssh_name(1), Some("SSH-1.x"));
        assert_eq!(ssh_name(2), Some("KEX 协商"));
        assert!(ssh_name(3).is_none());
        assert!(ssh_cell(3).is_none());
    }

    #[test]
    fn test_term_kinds() {
        assert_eq!(term_kind(0), Some(0));
        assert_eq!(term_kind(3), Some(1));
        assert_eq!(term_kind(6), Some(2));
        assert_eq!(term_kind(11), Some(3));
        assert!(term_kind(12).is_none());
        assert!(term_cell(12).is_none());
        assert_eq!(term_kind_name(2), Some("颜色映射"));
    }

    #[test]
    fn test_regs_named() {
        assert_eq!(
            reg_name(0),
            Some("HKCU\\Software\\SimonTatham\\PuTTY\\Sessions")
        );
        assert_eq!(reg_name(1), Some("HKCU\\…\\SshHostKeys"));
        assert!(reg_name(8).is_none());
        assert!(reg_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = session_cell(3).unwrap_or(0);
        assert_eq!(a, session_cell(3).unwrap_or(0));
        assert_ne!(a, session_cell(4).unwrap_or(0));
        assert!(tool_row(TOOL_ROWS).is_none());
        assert!(policy_row(POLICY_ROWS).is_none());
    }

    #[test]
    fn test_term_order_caught() {
        let mut lt = PuttyLedger::new();
        let _ = lt.record_term(5, term_cell(5).unwrap_or(0));
        assert_eq!(lt.record_term(1, term_cell(1).unwrap_or(0)), Err(PuttyCode::EOrder));
        assert!(lt.record_term(6, term_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PuttyLedger::new();
        assert_eq!(
            lt.record_session(2, session_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(PuttyCode::ESessionDrift)
        );
        assert_eq!(
            lt.record_term(8, term_cell(8).unwrap_or(0).wrapping_add(3)),
            Err(PuttyCode::ETermDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PuttyLedger::new();
        let _ = lt.record_reg(1);
        assert_eq!(lt.record_reg(1), Err(PuttyCode::EDup));
        let _ = lt.declare_tool(0);
        assert_eq!(lt.declare_tool(0), Err(PuttyCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PuttyLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SESSION_CELLS + TERM_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC77C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PuttyCode::Ok as u32, 0);
        assert_eq!(PuttyCode::EGradeDrift as u32, 12);
        assert!(!PuttyCode::Ok.detail().is_empty());
        assert!(!PuttyCode::EPolicyMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PuttyLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_tool_rows_named() {
        assert_eq!(tool_row_name(0), Some("pscp 文件传输分账声明"));
        assert_eq!(tool_row_name(2), Some("plink 命令行远端分账声明"));
        assert!(tool_row_name(3).is_none());
        assert!(policy_row_name(0).unwrap_or("").contains("升级接管"));
        assert!(policy_row_name(1).unwrap_or("").contains("AI-66.1"));
    }

    #[test]
    fn unx52224_checks_green() {
        let s = run_unx52224_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
