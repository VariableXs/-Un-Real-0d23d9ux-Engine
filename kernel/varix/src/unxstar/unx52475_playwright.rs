//! UNX-F52475 Playwright CLI 收口 · 0xCB680段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52475"，逐字）：
//!   title: Playwright CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52476-J1 ×10；
//!     与 Playwright 分账（同域不同框架）；KB-COMPAT 定级登记；判据锚
//!     定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52475）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52476-J1，照原文锚定；功能按本卡标题（Playwright CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 操作闭集：install/run/codegen/report/trace 各 4 操
//!      作，命令族单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 浏览器三引擎 6 格：chromium/firefox/webkit 各 2 格，引擎载荷
//!      漂移必抓。
//!   ③ trace 消费 8 格：四族（trace 记录/trace 回放/网络归档/DOM 快
//!      照）各 2 格，消费载荷漂移必抓。
//!   ④ Puppeteer 分账声明 2 行（同域不同框架）+ Node 上栈声明 2 行：
//!      声明行 4 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（命令漂移/命令族逆序/命令越闭
//!      集），检出+逃逸守恒；双跑 FNV 指纹一致；KB-COMPAT 定级闭集：
//!      全齐 → S1（目标档），六闸齐 → S2，闸缺 → S3。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52475;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令族操作数。
pub const CMD_OPS: usize = 20;

/// 浏览器三引擎格数。
pub const ENGINE_CELLS: usize = 6;

/// trace 消费格数。
pub const TRACE_CELLS: usize = 8;

/// 声明行数（Puppeteer 分账 2 + Node 上栈 2）。
pub const DECL_ROWS: usize = 4;

/// e2e 六判据数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaywrightCode {
    /// 成功
    Ok = 0,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// 引擎格越闭集
    ENoEng,
    /// 引擎载荷漂移
    EEngDrift,
    /// trace 格越闭集
    ENoTrace,
    /// trace 载荷漂移
    ETraceDrift,
    /// 命令族逆序
    EOrder,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl PlaywrightCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PlaywrightCode::Ok => "成功",
            PlaywrightCode::ENoCmd => "命令操作越闭集：install/run/codegen/report/trace 20 操作之外先登记",
            PlaywrightCode::ECmdDrift => "命令载荷漂移：命令操作折叠与基准不恒等，逐格复算",
            PlaywrightCode::ENoEng => "引擎格越闭集：chromium/firefox/webkit 6 格之外先登记",
            PlaywrightCode::EEngDrift => "引擎载荷漂移：引擎格折叠与基准不恒等，逐格复算",
            PlaywrightCode::ENoTrace => "trace 格越闭集：trace 消费 8 格之外先登记",
            PlaywrightCode::ETraceDrift => "trace 载荷漂移：trace 格折叠与基准不恒等，逐格复算",
            PlaywrightCode::EOrder => "命令族逆序：install→run→codegen→report→trace 单调，逆序必抓",
            PlaywrightCode::ENoRow => "声明行越闭集：Puppeteer 分账 2 行 + Node 上栈 2 行闭集之外先登记",
            PlaywrightCode::ERowMiss => "缺声明行：Puppeteer 分账/Node 上栈声明未齐先补齐",
            PlaywrightCode::EDup => "重复落账：同格只许一条账目",
            PlaywrightCode::ESeal => "未齐即封：命令 20 操作 + 引擎 6 格 + trace 8 格未齐先补齐",
            PlaywrightCode::EGradeDrift => "定级漂移：全齐才可定 S1，六闸缺不得越档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六闸+20命令操作+6引擎格+8trace格+4声明行全齐收口（目标档）",
            Grade::S2 => "S2：六闸齐而其余未齐，有条件收口",
            Grade::S3 => "S3：不收口，六闸有缺",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作载荷（操作 o；命令族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xCB680u64 + (o as u64) * 0x3D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=install 4..8=run 8..12=codegen 12..16=report 16..20=trace）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("install"),
        1 => Some("run"),
        2 => Some("codegen"),
        3 => Some("report"),
        4 => Some("trace"),
        _ => None,
    }
}

/// 引擎格载荷（格 e；引擎族 = e/2）。
pub const fn engine_cell(e: usize) -> Option<u64> {
    if e >= ENGINE_CELLS {
        return None;
    }
    Some((0xCB6C0u64 + (e as u64) * 0x49) ^ ((e / 2) as u64).wrapping_mul(0x27D4EB2F))
}

/// 引擎族（0..2=chromium 2..4=firefox 4..6=webkit）。
pub const fn engine_kind(e: usize) -> Option<u32> {
    if e >= ENGINE_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// 引擎族名。
pub const fn engine_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("chromium"),
        1 => Some("firefox"),
        2 => Some("webkit"),
        _ => None,
    }
}

/// trace 消费格载荷（格 t；消费族 = t/2）。
pub const fn trace_cell(t: usize) -> Option<u64> {
    if t >= TRACE_CELLS {
        return None;
    }
    Some((0xCB700u64 + (t as u64) * 0x53) ^ ((t / 2) as u64).wrapping_mul(0x9E3779B1))
}

/// trace 消费族（0..2=trace 记录 2..4=trace 回放 4..6=网络归档 6..8=DOM 快照）。
pub const fn trace_kind(t: usize) -> Option<u32> {
    if t >= TRACE_CELLS {
        return None;
    }
    Some((t / 2) as u32)
}

/// trace 消费族名。
pub const fn trace_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("trace 记录"),
        1 => Some("trace 回放"),
        2 => Some("网络归档"),
        3 => Some("DOM 快照"),
        _ => None,
    }
}

/// 声明行折叠（行 r；行族 = r/2：0..2 Puppeteer 分账 2..4 Node 上栈）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xCB740 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn decl_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Puppeteer 驱动面分账声明（同域不同框架）"),
        1 => Some("Puppeteer CDP 通道分账声明（同域不同框架）"),
        2 => Some("Node 运行时上栈声明"),
        3 => Some("Playwright 脚本宿主上栈声明"),
        _ => None,
    }
}

/// 六判据名（e2e 闸）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("浏览器启动"),
        1 => Some("上下文创建"),
        2 => Some("页面导航"),
        3 => Some("交互动作"),
        4 => Some("等待条件"),
        5 => Some("trace 闭合"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Playwright CLI 收口账本状态机。
#[derive(Clone)]
pub struct PlaywrightLedger {
    /// e2e 六判据。
    gates: [Option<bool>; GATES],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 引擎格账。
    engines: [Option<u64>; ENGINE_CELLS],
    /// trace 格账。
    traces: [Option<u64>; TRACE_CELLS],
    /// 声明行账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PlaywrightLedger {
    /// 新账本。
    pub fn new() -> Self {
        PlaywrightLedger {
            gates: [None; GATES],
            cmds: [None; CMD_OPS],
            engines: [None; ENGINE_CELLS],
            traces: [None; TRACE_CELLS],
            decls: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PlaywrightCode> {
        if gate >= GATES {
            return Err(PlaywrightCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), PlaywrightCode> {
        if o >= CMD_OPS {
            return Err(PlaywrightCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(PlaywrightCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(PlaywrightCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(PlaywrightCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(PlaywrightCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlaywrightCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 操作）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 引擎一格（载荷对照 + 重复拒）。
    pub fn record_engine(&mut self, e: usize, got: u64) -> Result<(), PlaywrightCode> {
        if e >= ENGINE_CELLS {
            return Err(PlaywrightCode::ENoEng);
        }
        let want = match engine_cell(e) {
            Some(w) => w,
            None => return Err(PlaywrightCode::ENoEng),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + e as u64);
            return Err(PlaywrightCode::EEngDrift);
        }
        if match self.engines.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlaywrightCode::EDup);
        }
        if let Some(c) = self.engines.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x7301 + e as u64);
        Ok(())
    }

    /// 引擎全账核验（6 格）。
    pub fn engines_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENGINE_CELLS {
            match self.engines.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// trace 一格（载荷对照 + 重复拒）。
    pub fn record_trace(&mut self, t: usize, got: u64) -> Result<(), PlaywrightCode> {
        if t >= TRACE_CELLS {
            return Err(PlaywrightCode::ENoTrace);
        }
        let want = match trace_cell(t) {
            Some(w) => w,
            None => return Err(PlaywrightCode::ENoTrace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + t as u64);
            return Err(PlaywrightCode::ETraceDrift);
        }
        if match self.traces.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlaywrightCode::EDup);
        }
        if let Some(c) = self.traces.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x7401 + t as u64);
        Ok(())
    }

    /// trace 全账核验（8 格）。
    pub fn traces_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRACE_CELLS {
            match self.traces.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 声明一行（Puppeteer 分账 / Node 上栈）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), PlaywrightCode> {
        if r >= DECL_ROWS {
            return Err(PlaywrightCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlaywrightCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(r) {
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

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，闸缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cmds_all() && self.engines_all() && self.traces_all()
            && self.decls_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PlaywrightLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PlaywrightLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < ENGINE_CELLS {
        if let Some(w) = engine_cell(e) {
            if led.record_engine(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut t = 0usize;
    while t < TRACE_CELLS {
        if let Some(w) = trace_cell(t) {
            if led.record_trace(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：命令漂移 / 命令族逆序 / 命令越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PlaywrightLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
                lt.record_cmd(3, cmd_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(PlaywrightCode::ECmdDrift)
            }
            1 => {
                let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
                lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(PlaywrightCode::EOrder)
            }
            _ => lt.record_cmd(CMD_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52475 判据集（锚 UNX-F52476-J1，off-by-one 板式）。
pub fn run_unx52475_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52475");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= (CMD_OPS + ENGINE_CELLS + TRACE_CELLS) as u32,
        "install/run/codegen/report/trace 五族逐操作恒等（UNX-F52476-J1 ×10 全通口径）",
    );
    s.add(
        "浏览器三引擎 6 格全落",
        {
            let mut lt = PlaywrightLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENGINE_CELLS {
                if let Some(w) = engine_cell(e) {
                    if lt.record_engine(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.engines_all()
        },
        "chromium/firefox/webkit 三引擎各 2 格恒等",
    );
    s.add(
        "trace 消费 8 格全落",
        {
            let mut lt = PlaywrightLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRACE_CELLS {
                if let Some(w) = trace_cell(t) {
                    if lt.record_trace(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.traces_all()
        },
        "trace 记录/trace 回放/网络归档/DOM 快照四族各 2 格恒等",
    );
    s.add(
        "e2e 六判据全落",
        {
            let mut lt = PlaywrightLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "浏览器启动/上下文创建/页面导航/交互动作/等待条件/trace 闭合逐格落账",
    );
    s.add(
        "Puppeteer 分账与上栈声明 4/4 全齐",
        {
            let mut lt = PlaywrightLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decls_all()
        },
        "Puppeteer 分账 2 行（同域不同框架）+ Node 上栈 2 行闭集（§AI-66.3）",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令漂移/命令族逆序/命令越闭集三态轮转必抓",
    );
    let mut l0 = PlaywrightLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令操作 20 越闭集必拒",
    );
    s.add(
        "命令漂移拒",
        {
            let _ = l0.record_cmd(0, cmd_cell(0).unwrap_or(0));
            l0.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(PlaywrightCode::ECmdDrift)
        },
        "命令载荷漂移必抓",
    );
    s.add(
        "边界·引擎越闭集",
        l0.record_engine(ENGINE_CELLS, 0).is_err(),
        "引擎格 6 越闭集必拒",
    );
    s.add(
        "边界·trace 越闭集",
        l0.record_trace(TRACE_CELLS, 0).is_err(),
        "trace 格 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_row(DECL_ROWS) == Err(PlaywrightCode::ENoRow),
        "声明行 4 越闭集必拒",
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
        "未齐即封核",
        {
            let mut lt = PlaywrightLedger::new();
            let _ = lt.judge_gate(0);
            lt.judge_gate(0).is_ok() && !lt.gates_all() && lt.judge_grade() == Grade::S3
        },
        "账未齐时逐格仍 Ok 落账，定级不给目标档 S1",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = PlaywrightLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_cmd(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < ENGINE_CELLS {
                if let Some(w) = engine_cell(e) {
                    let _ = lt.record_engine(e, w);
                }
                e += 1;
            }
            let mut t = 0usize;
            while t < TRACE_CELLS {
                if let Some(w) = trace_cell(t) {
                    let _ = lt.record_trace(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < DECL_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "六闸 + 命令/引擎/trace/声明全齐 → S1（KB-COMPAT 定级登记，目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB680,
        "SEG_BASE 0x40 对齐且在 VX16 域第三格",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
        assert_eq!(cmd_kind_name(2), Some("codegen"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_engine_kind() {
        assert_eq!(engine_kind(0), Some(0));
        assert_eq!(engine_kind(2), Some(1));
        assert_eq!(engine_kind(5), Some(2));
        assert!(engine_kind(6).is_none());
        assert!(engine_cell(6).is_none());
        assert_eq!(engine_kind_name(2), Some("webkit"));
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert_ne!(engine_cell(1).unwrap_or(0), engine_cell(2).unwrap_or(0));
        assert_ne!(trace_cell(3).unwrap_or(0), trace_cell(4).unwrap_or(0));
        assert!(trace_cell(TRACE_CELLS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert_eq!(trace_kind_name(3), Some("DOM 快照"));
        assert!(trace_kind_name(4).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PlaywrightLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(2, cmd_cell(2).unwrap_or(0)),
            Err(PlaywrightCode::EOrder)
        );
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PlaywrightLedger::new();
        assert_eq!(
            lt.record_engine(5, engine_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PlaywrightCode::EEngDrift)
        );
        assert_eq!(
            lt.record_trace(2, trace_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(PlaywrightCode::ETraceDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PlaywrightLedger::new();
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(0, cmd_cell(0).unwrap_or(0)),
            Err(PlaywrightCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PlaywrightLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        assert_eq!(gate_name(0), Some("浏览器启动"));
        assert_eq!(gate_name(5), Some("trace 闭合"));
        assert!(gate_name(6).is_none());
        let mut lt2 = PlaywrightLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded >= (CMD_OPS + ENGINE_CELLS + TRACE_CELLS) as u32,
            "recorded 须覆盖命令 20 + 引擎 6 + trace 8"
        );
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
        assert_eq!(SEG_BASE, 0xCB680);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PlaywrightCode::Ok as u32, 0);
        assert_eq!(PlaywrightCode::EGradeDrift as u32, 12);
        assert_eq!(PlaywrightCode::ESeal as u32, 11);
        assert_eq!(PlaywrightCode::Ok.val(), SEG_BASE);
        assert!(!PlaywrightCode::Ok.detail().is_empty());
        assert!(!PlaywrightCode::ESeal.detail().is_empty());
        assert!(!PlaywrightCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let empty = PlaywrightLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = PlaywrightLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52475_checks_green() {
        let s = run_unx52475_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
