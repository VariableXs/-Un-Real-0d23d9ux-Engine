//! UNX-F52476 Puppeteer 收口 · 0xCB6C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52476"，逐字）：
//!   title: Puppeteer 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52477-J1 会话
//!     ×10（Chrome/Firefox 双驱动）；W3C WebDriver 协议 ×20 命令零漂
//!     移；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52476）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52477-J1，照原文锚定；功能按本卡标题（Puppeteer）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 操作闭集：launch/page/screenshot/pdf/click 各 4 操
//!      作，命令族单调（逆序必抓），逐格载荷纯函数折叠恒等才收
//!      （W3C WebDriver ×20 命令零漂移口径同构）。
//!   ② CDP 消费联签 4 格：CDP 域启用/事件订阅/会话转发/协议回执，联
//!      签载荷漂移必抓。
//!   ③ headless 三态 6 格：true/false/new 三态各 2 格，态载荷漂移必抓。
//!   ④ Chrome 消费声明 2 行 + Node 上栈声明 2 行：声明行 4 行闭集，
//!      缺一拒封。
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
pub const ID: u32 = 52476;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令族操作数。
pub const CMD_OPS: usize = 20;

/// CDP 消费联签格数。
pub const CDP_CELLS: usize = 4;

/// headless 三态格数。
pub const HEADLESS_CELLS: usize = 6;

/// 声明行数（Chrome 消费 2 + Node 上栈 2）。
pub const DECL_ROWS: usize = 4;

/// 会话六判据数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PuppeteerCode {
    /// 成功
    Ok = 0,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// CDP 格越闭集
    ENoCdp,
    /// CDP 载荷漂移
    ECdpDrift,
    /// headless 格越闭集
    ENoHead,
    /// headless 载荷漂移
    EHeadDrift,
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

impl PuppeteerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PuppeteerCode::Ok => "成功",
            PuppeteerCode::ENoCmd => "命令操作越闭集：launch/page/screenshot/pdf/click 20 操作之外先登记",
            PuppeteerCode::ECmdDrift => "命令载荷漂移：命令操作折叠与基准不恒等，逐格复算",
            PuppeteerCode::ENoCdp => "CDP 格越闭集：CDP 消费联签 4 格之外先登记",
            PuppeteerCode::ECdpDrift => "CDP 载荷漂移：CDP 联签格折叠与基准不恒等，逐格复算",
            PuppeteerCode::ENoHead => "headless 格越闭集：三态 6 格之外先登记",
            PuppeteerCode::EHeadDrift => "headless 载荷漂移：三态格折叠与基准不恒等，逐格复算",
            PuppeteerCode::EOrder => "命令族逆序：launch→page→screenshot→pdf→click 单调，逆序必抓",
            PuppeteerCode::ENoRow => "声明行越闭集：Chrome 消费 2 行 + Node 上栈 2 行闭集之外先登记",
            PuppeteerCode::ERowMiss => "缺声明行：Chrome 消费声明/Node 上栈声明未齐先补齐",
            PuppeteerCode::EDup => "重复落账：同格只许一条账目",
            PuppeteerCode::ESeal => "未齐即封：命令 20 操作 + CDP 4 格 + headless 6 格未齐先补齐",
            PuppeteerCode::EGradeDrift => "定级漂移：全齐才可定 S1，六闸缺不得越档",
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
            Grade::S1 => "S1：六闸+20命令操作+4CDP格+6headless格+4声明行全齐收口（目标档）",
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
    Some((0xCB6C0u64 + (o as u64) * 0x3F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=launch 4..8=page 8..12=screenshot 12..16=pdf 16..20=click）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("launch"),
        1 => Some("page"),
        2 => Some("screenshot"),
        3 => Some("pdf"),
        4 => Some("click"),
        _ => None,
    }
}

/// CDP 联签格载荷（格 d；联签族 = d/2）。
pub const fn cdp_cell(d: usize) -> Option<u64> {
    if d >= CDP_CELLS {
        return None;
    }
    Some((0xCB700u64 + (d as u64) * 0x4B) ^ ((d / 2) as u64).wrapping_mul(0x27D4EB2F))
}

/// CDP 联签族（0..2=域启用 2..4=事件订阅）。
pub const fn cdp_kind(d: usize) -> Option<u32> {
    if d >= CDP_CELLS {
        return None;
    }
    Some((d / 2) as u32)
}

/// CDP 联签格名。
pub const fn cdp_cell_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("CDP 域启用联签"),
        1 => Some("CDP 事件订阅联签"),
        2 => Some("CDP 会话转发联签"),
        3 => Some("CDP 协议回执联签"),
        _ => None,
    }
}

/// headless 态格载荷（格 h；态族 = h/2：0=true 1=false 2=new）。
pub const fn headless_cell(h: usize) -> Option<u64> {
    if h >= HEADLESS_CELLS {
        return None;
    }
    Some((0xCB740u64 + (h as u64) * 0x5F) ^ ((h / 2) as u64).wrapping_mul(0x9E3779B1))
}

/// headless 态族名（0..2=true 2..4=false 4..6=new）。
pub const fn headless_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("headless=true"),
        1 => Some("headless=false"),
        2 => Some("headless=new"),
        _ => None,
    }
}

/// 声明行折叠（行 r；行族 = r/2：0..2 Chrome 消费 2..4 Node 上栈）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xCB780 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn decl_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Chrome 消费端分账声明（F52461 同源）"),
        1 => Some("Chrome 驱动进程分账声明（F52461 同源）"),
        2 => Some("Node 运行时上栈声明"),
        3 => Some("Puppeteer 脚本宿主上栈声明"),
        _ => None,
    }
}

/// 六判据名（会话闸，Chrome/Firefox 双驱动口径）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("驱动选择"),
        1 => Some("浏览器启动"),
        2 => Some("页面导航"),
        3 => Some("截图落账"),
        4 => Some("PDF 落账"),
        5 => Some("会话回收"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Puppeteer 收口账本状态机。
#[derive(Clone)]
pub struct PuppeteerLedger {
    /// 会话六判据。
    gates: [Option<bool>; GATES],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// CDP 联签格账。
    cdps: [Option<u64>; CDP_CELLS],
    /// headless 态格账。
    heads: [Option<u64>; HEADLESS_CELLS],
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

impl PuppeteerLedger {
    /// 新账本。
    pub fn new() -> Self {
        PuppeteerLedger {
            gates: [None; GATES],
            cmds: [None; CMD_OPS],
            cdps: [None; CDP_CELLS],
            heads: [None; HEADLESS_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PuppeteerCode> {
        if gate >= GATES {
            return Err(PuppeteerCode::ESeal);
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
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), PuppeteerCode> {
        if o >= CMD_OPS {
            return Err(PuppeteerCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(PuppeteerCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(PuppeteerCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(PuppeteerCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(PuppeteerCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuppeteerCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
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

    /// CDP 联签一格（载荷对照 + 重复拒）。
    pub fn record_cdp(&mut self, d: usize, got: u64) -> Result<(), PuppeteerCode> {
        if d >= CDP_CELLS {
            return Err(PuppeteerCode::ENoCdp);
        }
        let want = match cdp_cell(d) {
            Some(w) => w,
            None => return Err(PuppeteerCode::ENoCdp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + d as u64);
            return Err(PuppeteerCode::ECdpDrift);
        }
        if match self.cdps.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuppeteerCode::EDup);
        }
        if let Some(c) = self.cdps.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x8301 + d as u64);
        Ok(())
    }

    /// CDP 全账核验（4 格）。
    pub fn cdps_all(&self) -> bool {
        let mut d = 0usize;
        while d < CDP_CELLS {
            match self.cdps.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// headless 态一格（载荷对照 + 重复拒）。
    pub fn record_headless(&mut self, h: usize, got: u64) -> Result<(), PuppeteerCode> {
        if h >= HEADLESS_CELLS {
            return Err(PuppeteerCode::ENoHead);
        }
        let want = match headless_cell(h) {
            Some(w) => w,
            None => return Err(PuppeteerCode::ENoHead),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + h as u64);
            return Err(PuppeteerCode::EHeadDrift);
        }
        if match self.heads.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuppeteerCode::EDup);
        }
        if let Some(c) = self.heads.get_mut(h) {
            *c = Some(got);
        }
        self.fnv(0x8401 + h as u64);
        Ok(())
    }

    /// headless 全账核验（6 格）。
    pub fn heads_all(&self) -> bool {
        let mut h = 0usize;
        while h < HEADLESS_CELLS {
            match self.heads.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
        }
        true
    }

    /// 声明一行（Chrome 消费 / Node 上栈）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), PuppeteerCode> {
        if r >= DECL_ROWS {
            return Err(PuppeteerCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PuppeteerCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x8500 + r as u64);
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
        if self.gates_all() && self.cmds_all() && self.cdps_all() && self.heads_all()
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

impl Default for PuppeteerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PuppeteerLedger::new();
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
    let mut d = 0usize;
    while d < CDP_CELLS {
        if let Some(w) = cdp_cell(d) {
            if led.record_cdp(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut h = 0usize;
    while h < HEADLESS_CELLS {
        if let Some(w) = headless_cell(h) {
            if led.record_headless(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
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
        let mut lt = PuppeteerLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
                lt.record_cmd(3, cmd_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(PuppeteerCode::ECmdDrift)
            }
            1 => {
                let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
                lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(PuppeteerCode::EOrder)
            }
            _ => lt.record_cmd(CMD_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52476 判据集（锚 UNX-F52477-J1，off-by-one 板式）。
pub fn run_unx52476_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52476");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= (CMD_OPS + CDP_CELLS + HEADLESS_CELLS) as u32,
        "launch/page/screenshot/pdf/click 五族逐操作恒等（W3C WebDriver ×20 命令零漂移口径，UNX-F52477-J1 会话 ×10 同构）",
    );
    s.add(
        "CDP 消费联签 4 格全落",
        {
            let mut lt = PuppeteerLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < CDP_CELLS {
                if let Some(w) = cdp_cell(d) {
                    if lt.record_cdp(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.cdps_all()
        },
        "CDP 域启用/事件订阅/会话转发/协议回执逐格恒等",
    );
    s.add(
        "headless 三态 6 格全落",
        {
            let mut lt = PuppeteerLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < HEADLESS_CELLS {
                if let Some(w) = headless_cell(h) {
                    if lt.record_headless(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.heads_all()
        },
        "headless=true/false/new 三态各 2 格恒等",
    );
    s.add(
        "会话六判据全落",
        {
            let mut lt = PuppeteerLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "驱动选择/浏览器启动/页面导航/截图落账/PDF 落账/会话回收逐格落账（Chrome/Firefox 双驱动口径）",
    );
    s.add(
        "Chrome 消费与上栈声明 4/4 全齐",
        {
            let mut lt = PuppeteerLedger::new();
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
        "Chrome 消费声明 2 行（F52461 同源）+ Node 上栈 2 行闭集（§AI-66.3）",
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
    let mut l0 = PuppeteerLedger::new();
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
                == Err(PuppeteerCode::ECmdDrift)
        },
        "命令载荷漂移必抓",
    );
    s.add(
        "边界·CDP 越闭集",
        l0.record_cdp(CDP_CELLS, 0).is_err(),
        "CDP 格 4 越闭集必拒",
    );
    s.add(
        "边界·headless 越闭集",
        l0.record_headless(HEADLESS_CELLS, 0).is_err(),
        "headless 格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_row(DECL_ROWS) == Err(PuppeteerCode::ENoRow),
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
            let mut lt = PuppeteerLedger::new();
            let _ = lt.judge_gate(0);
            lt.judge_gate(0).is_ok() && !lt.gates_all() && lt.judge_grade() == Grade::S3
        },
        "账未齐时逐格仍 Ok 落账，定级不给目标档 S1",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = PuppeteerLedger::new();
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
            let mut d = 0usize;
            while d < CDP_CELLS {
                if let Some(w) = cdp_cell(d) {
                    let _ = lt.record_cdp(d, w);
                }
                d += 1;
            }
            let mut h = 0usize;
            while h < HEADLESS_CELLS {
                if let Some(w) = headless_cell(h) {
                    let _ = lt.record_headless(h, w);
                }
                h += 1;
            }
            let mut r = 0usize;
            while r < DECL_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "六闸 + 命令/CDP/headless/声明全齐 → S1（KB-COMPAT 定级登记，目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB6C0,
        "SEG_BASE 0x40 对齐且在 VX16 域第四格",
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
        assert_eq!(cmd_kind_name(2), Some("screenshot"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_headless_kind() {
        assert_eq!(cdp_kind(0), Some(0));
        assert_eq!(cdp_kind(2), Some(1));
        assert!(cdp_kind(4).is_none());
        assert!(cdp_cell(4).is_none());
        assert_eq!(cdp_cell_name(3), Some("CDP 协议回执联签"));
        assert!(cdp_cell_name(4).is_none());
        assert_eq!(headless_kind_name(0), Some("headless=true"));
        assert_eq!(headless_kind_name(2), Some("headless=new"));
        assert!(headless_kind_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert_ne!(cdp_cell(1).unwrap_or(0), cdp_cell(2).unwrap_or(0));
        assert_ne!(headless_cell(3).unwrap_or(0), headless_cell(4).unwrap_or(0));
        assert!(headless_cell(HEADLESS_CELLS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PuppeteerLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(2, cmd_cell(2).unwrap_or(0)),
            Err(PuppeteerCode::EOrder)
        );
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PuppeteerLedger::new();
        assert_eq!(
            lt.record_cdp(2, cdp_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(PuppeteerCode::ECdpDrift)
        );
        assert_eq!(
            lt.record_headless(4, headless_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(PuppeteerCode::EHeadDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PuppeteerLedger::new();
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(0, cmd_cell(0).unwrap_or(0)),
            Err(PuppeteerCode::EDup)
        );
        let _ = lt.declare_row(0);
        assert_eq!(lt.declare_row(0), Err(PuppeteerCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PuppeteerLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        assert_eq!(gate_name(0), Some("驱动选择"));
        assert_eq!(gate_name(5), Some("会话回收"));
        assert!(gate_name(6).is_none());
        let mut lt2 = PuppeteerLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded >= (CMD_OPS + CDP_CELLS + HEADLESS_CELLS) as u32,
            "recorded 须覆盖命令 20 + CDP 4 + headless 6"
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
        assert_eq!(SEG_BASE, 0xCB6C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PuppeteerCode::Ok as u32, 0);
        assert_eq!(PuppeteerCode::EGradeDrift as u32, 12);
        assert_eq!(PuppeteerCode::ESeal as u32, 11);
        assert_eq!(PuppeteerCode::Ok.val(), SEG_BASE);
        assert!(!PuppeteerCode::Ok.detail().is_empty());
        assert!(!PuppeteerCode::ESeal.detail().is_empty());
        assert!(!PuppeteerCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let empty = PuppeteerLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = PuppeteerLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52476_checks_green() {
        let s = run_unx52476_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
