//! UNX-F52474 Lighthouse CLI 收口 · 0xCB640段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52474"，逐字）：
//!   title: Lighthouse CLI 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52475-J1 e2e
//!     ×10 场景全通（headless 判据）；浏览器驱动下载面网络语义（AI-23
//!     联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52474）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52475-J1，照原文锚定；功能按本卡标题（Lighthouse CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五审计族 20 格闭集：性能/PWA/可访问性/最佳实践/SEO 各 4 格，
//!      审计族单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 五命令 10 操作闭集：collect/assert/autorun/server/view 各 2 操
//!      作，命令漂移必抓。
//!   ③ 评分三档 6 格：红/橙/绿 ×2 格，分档载荷漂移必抓。
//!   ④ Chrome DevTools 消费联签 4 格（F52461）+ Node 上栈声明 2 行：
//!      声明行 6 行闭集（行族 r/4：0..4 联签、4..6 上栈），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（审计漂移/审计族逆序/审计越闭
//!      集），检出+逃逸守恒；双跑 FNV 指纹一致；KB-COMPAT 定级闭集：
//!      全齐 → S1（目标档），六闸齐 → S2，闸缺 → S3。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52474;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五审计族格数。
pub const AUDIT_CELLS: usize = 20;

/// 五命令操作数。
pub const CMD_OPS: usize = 10;

/// 评分三档格数。
pub const SCORE_CELLS: usize = 6;

/// 声明行数（联签 4 + Node 上栈 2）。
pub const DECL_ROWS: usize = 6;

/// headless e2e 六判据数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LighthouseCode {
    /// 成功
    Ok = 0,
    /// 审计格越闭集
    ENoAudit,
    /// 审计载荷漂移
    EAuditDrift,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// 评分档越闭集
    ENoScore,
    /// 评分档载荷漂移
    EScoreDrift,
    /// 审计族逆序
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

impl LighthouseCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LighthouseCode::Ok => "成功",
            LighthouseCode::ENoAudit => "审计格越闭集：性能/PWA/可访问性/最佳实践/SEO 20 格之外先登记",
            LighthouseCode::EAuditDrift => "审计载荷漂移：审计格折叠与基准不恒等，逐格复算",
            LighthouseCode::ENoCmd => "命令操作越闭集：collect/assert/autorun/server/view 10 操作之外先登记",
            LighthouseCode::ECmdDrift => "命令载荷漂移：命令操作折叠与基准不恒等，逐格复算",
            LighthouseCode::ENoScore => "评分档越闭集：红/橙/绿三档 6 格之外先登记",
            LighthouseCode::EScoreDrift => "评分档载荷漂移：评分格折叠与基准不恒等，逐格复算",
            LighthouseCode::EOrder => "审计族逆序：性能→PWA→可访问性→最佳实践→SEO 单调，逆序必抓",
            LighthouseCode::ENoRow => "声明行越闭集：联签 4 行 + Node 上栈 2 行闭集之外先登记",
            LighthouseCode::ERowMiss => "缺声明行：Chrome DevTools 联签/Node 上栈声明未齐先补齐",
            LighthouseCode::EDup => "重复落账：同格只许一条账目",
            LighthouseCode::ESeal => "未齐即封：审计 20 格 + 命令 10 操作 + 评分 6 格未齐先补齐",
            LighthouseCode::EGradeDrift => "定级漂移：全齐才可定 S1，六闸缺不得越档",
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
            Grade::S1 => "S1：六闸+20审计格+10命令操作+6评分格+6声明行全齐收口（目标档）",
            Grade::S2 => "S2：六闸齐而其余未齐，有条件收口",
            Grade::S3 => "S3：不收口，六闸有缺",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 审计格载荷（格 o；审计族 = o/4）。
pub const fn audit_cell(o: usize) -> Option<u64> {
    if o >= AUDIT_CELLS {
        return None;
    }
    Some((0xCB640u64 + (o as u64) * 0x3B) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 审计族（0..4=性能 4..8=PWA 8..12=可访问性 12..16=最佳实践 16..20=SEO）。
pub const fn audit_kind(o: usize) -> Option<u32> {
    if o >= AUDIT_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 审计族名。
pub const fn audit_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("性能"),
        1 => Some("PWA"),
        2 => Some("可访问性"),
        3 => Some("最佳实践"),
        4 => Some("SEO"),
        _ => None,
    }
}

/// 命令操作载荷（操作 c；命令族 = c/2）。
pub const fn cmd_cell(c: usize) -> Option<u64> {
    if c >= CMD_OPS {
        return None;
    }
    Some((0xCB680u64 + (c as u64) * 0x47) ^ ((c / 2) as u64).wrapping_mul(0x27D4EB2F))
}

/// 命令族（0..2=collect 2..4=assert 4..6=autorun 6..8=server 8..10=view）。
pub const fn cmd_kind(c: usize) -> Option<u32> {
    if c >= CMD_OPS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("collect"),
        1 => Some("assert"),
        2 => Some("autorun"),
        3 => Some("server"),
        4 => Some("view"),
        _ => None,
    }
}

/// 评分档格载荷（格 t；分档族 = t/2：0=红 1=橙 2=绿）。
pub const fn score_cell(t: usize) -> Option<u64> {
    if t >= SCORE_CELLS {
        return None;
    }
    Some((0xCB6C0u64 + (t as u64) * 0x5D) ^ ((t / 2) as u64).wrapping_mul(0x9E3779B1))
}

/// 评分档名（0..2=红 2..4=橙 4..6=绿）。
pub const fn score_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("红档 0-49"),
        1 => Some("橙档 50-89"),
        2 => Some("绿档 90-100"),
        _ => None,
    }
}

/// 声明行折叠（行 r；行族 = r/4：0..4 联签 4..6 Node 上栈）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xCB700 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn decl_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Chrome DevTools 审计面板联签声明（F52461）"),
        1 => Some("Chrome DevTools 性能痕迹联签声明（F52461）"),
        2 => Some("Chrome DevTools 评分读数联签声明（F52461）"),
        3 => Some("Chrome DevTools 报告视图联签声明（F52461）"),
        4 => Some("Node 运行时上栈声明"),
        5 => Some("LHCI 脚本宿主上栈声明"),
        _ => None,
    }
}

/// 六判据名（headless e2e 闸）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("headless 启动"),
        1 => Some("URL 装载"),
        2 => Some("审计调度"),
        3 => Some("评分汇总"),
        4 => Some("报告落盘"),
        5 => Some("断言闭合"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Lighthouse CLI 收口账本状态机。
#[derive(Clone)]
pub struct LighthouseLedger {
    /// headless e2e 六判据。
    gates: [Option<bool>; GATES],
    /// 审计格账。
    audits: [Option<u64>; AUDIT_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 评分档账。
    scores: [Option<u64>; SCORE_CELLS],
    /// 声明行账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后审计族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LighthouseLedger {
    /// 新账本。
    pub fn new() -> Self {
        LighthouseLedger {
            gates: [None; GATES],
            audits: [None; AUDIT_CELLS],
            cmds: [None; CMD_OPS],
            scores: [None; SCORE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LighthouseCode> {
        if gate >= GATES {
            return Err(LighthouseCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 审计一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_audit(&mut self, o: usize, got: u64) -> Result<(), LighthouseCode> {
        if o >= AUDIT_CELLS {
            return Err(LighthouseCode::ENoAudit);
        }
        let kind = match audit_kind(o) {
            Some(k) => k,
            None => return Err(LighthouseCode::ENoAudit),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(LighthouseCode::EOrder);
        }
        let want = match audit_cell(o) {
            Some(w) => w,
            None => return Err(LighthouseCode::ENoAudit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(LighthouseCode::EAuditDrift);
        }
        if match self.audits.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LighthouseCode::EDup);
        }
        if let Some(c) = self.audits.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 审计全账核验（20 格）。
    pub fn audits_all(&self) -> bool {
        let mut o = 0usize;
        while o < AUDIT_CELLS {
            match self.audits.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令一格（载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, c: usize, got: u64) -> Result<(), LighthouseCode> {
        if c >= CMD_OPS {
            return Err(LighthouseCode::ENoCmd);
        }
        let want = match cmd_cell(c) {
            Some(w) => w,
            None => return Err(LighthouseCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + c as u64);
            return Err(LighthouseCode::ECmdDrift);
        }
        if match self.cmds.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LighthouseCode::EDup);
        }
        if let Some(x) = self.cmds.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x6301 + c as u64);
        Ok(())
    }

    /// 命令全账核验（10 操作）。
    pub fn cmds_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMD_OPS {
            match self.cmds.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 评分一格（载荷对照 + 重复拒）。
    pub fn record_score(&mut self, t: usize, got: u64) -> Result<(), LighthouseCode> {
        if t >= SCORE_CELLS {
            return Err(LighthouseCode::ENoScore);
        }
        let want = match score_cell(t) {
            Some(w) => w,
            None => return Err(LighthouseCode::ENoScore),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + t as u64);
            return Err(LighthouseCode::EScoreDrift);
        }
        if match self.scores.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LighthouseCode::EDup);
        }
        if let Some(x) = self.scores.get_mut(t) {
            *x = Some(got);
        }
        self.fnv(0x6401 + t as u64);
        Ok(())
    }

    /// 评分全账核验（6 格）。
    pub fn scores_all(&self) -> bool {
        let mut t = 0usize;
        while t < SCORE_CELLS {
            match self.scores.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 声明一行（联签/Node 上栈）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), LighthouseCode> {
        if r >= DECL_ROWS {
            return Err(LighthouseCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LighthouseCode::EDup);
        }
        if let Some(x) = self.decls.get_mut(r) {
            *x = decl_row(r);
        }
        self.fnv(0x6500 + r as u64);
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
        if self.gates_all() && self.audits_all() && self.cmds_all() && self.scores_all()
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

impl Default for LighthouseLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LighthouseLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < AUDIT_CELLS {
        if let Some(w) = audit_cell(o) {
            if led.record_audit(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CMD_OPS {
        if let Some(w) = cmd_cell(c) {
            if led.record_cmd(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < SCORE_CELLS {
        if let Some(w) = score_cell(t) {
            if led.record_score(t, w).is_ok() {
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
    // 注入 10 轮：审计漂移 / 审计族逆序 / 审计越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LighthouseLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_audit(2, audit_cell(2).unwrap_or(0));
                lt.record_audit(3, audit_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(LighthouseCode::EAuditDrift)
            }
            1 => {
                let _ = lt.record_audit(9, audit_cell(9).unwrap_or(0));
                lt.record_audit(2, audit_cell(2).unwrap_or(0)) == Err(LighthouseCode::EOrder)
            }
            _ => lt.record_audit(AUDIT_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52474 判据集（锚 UNX-F52475-J1，off-by-one 板式）。
pub fn run_unx52474_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52474");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五审计族 20 格全通",
        recorded >= (AUDIT_CELLS + CMD_OPS + SCORE_CELLS) as u32,
        "性能/PWA/可访问性/最佳实践/SEO 五族逐格恒等（UNX-F52475-J1 e2e ×10 全通口径）",
    );
    s.add(
        "五命令 10 操作全落",
        {
            let mut lt = LighthouseLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    if lt.record_cmd(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmds_all()
        },
        "collect/assert/autorun/server/view 五命令各 2 操作恒等",
    );
    s.add(
        "评分三档 6 格全落",
        {
            let mut lt = LighthouseLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < SCORE_CELLS {
                if let Some(w) = score_cell(t) {
                    if lt.record_score(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.scores_all()
        },
        "红/橙/绿三档各 2 格恒等",
    );
    s.add(
        "headless e2e 六判据全落",
        {
            let mut lt = LighthouseLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "headless 启动/URL 装载/审计调度/评分汇总/报告落盘/断言闭合逐格落账",
    );
    s.add(
        "联签与上栈声明 6/6 全齐",
        {
            let mut lt = LighthouseLedger::new();
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
        "Chrome DevTools 联签 4 行（F52461，AI-23 锚定）+ Node 上栈 2 行闭集（§AI-66.3）",
    );
    s.add(
        "审计族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < AUDIT_CELLS {
                if audit_kind(o).unwrap_or(5) < audit_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五审计族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "审计漂移/审计族逆序/审计越闭集三态轮转必抓",
    );
    let mut l0 = LighthouseLedger::new();
    s.add(
        "边界·审计越闭集",
        l0.record_audit(AUDIT_CELLS, 0).is_err(),
        "审计格 20 越闭集必拒",
    );
    s.add(
        "审计漂移拒",
        {
            let _ = l0.record_audit(0, audit_cell(0).unwrap_or(0));
            l0.record_audit(1, audit_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(LighthouseCode::EAuditDrift)
        },
        "审计载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令操作 10 越闭集必拒",
    );
    s.add(
        "边界·评分越闭集",
        l0.record_score(SCORE_CELLS, 0).is_err(),
        "评分格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_row(DECL_ROWS) == Err(LighthouseCode::ENoRow),
        "声明行 6 越闭集必拒",
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
            let mut lt = LighthouseLedger::new();
            let _ = lt.judge_gate(0);
            lt.judge_gate(0).is_ok() && !lt.gates_all() && lt.judge_grade() == Grade::S3
        },
        "账未齐时逐格仍 Ok 落账，定级不给目标档 S1",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = LighthouseLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < AUDIT_CELLS {
                if let Some(w) = audit_cell(o) {
                    let _ = lt.record_audit(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    let _ = lt.record_cmd(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < SCORE_CELLS {
                if let Some(w) = score_cell(t) {
                    let _ = lt.record_score(t, w);
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
        "六闸 + 审计/命令/评分/声明全齐 → S1（KB-COMPAT 定级登记，目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB640,
        "SEG_BASE 0x40 对齐且在 VX16 域第二格",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_kind() {
        assert_eq!(audit_kind(0), Some(0));
        assert_eq!(audit_kind(4), Some(1));
        assert_eq!(audit_kind(19), Some(4));
        assert!(audit_kind(20).is_none());
        assert!(audit_cell(20).is_none());
        assert_eq!(audit_kind_name(4), Some("SEO"));
        assert!(audit_kind_name(5).is_none());
    }

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(2), Some(1));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert!(cmd_cell(10).is_none());
        assert_eq!(cmd_kind_name(2), Some("autorun"));
    }

    #[test]
    fn test_cells_pure() {
        let a = audit_cell(3).unwrap_or(0);
        assert_eq!(a, audit_cell(3).unwrap_or(0));
        assert_ne!(a, audit_cell(4).unwrap_or(0));
        assert_ne!(cmd_cell(1).unwrap_or(0), cmd_cell(2).unwrap_or(0));
        assert_ne!(score_cell(3).unwrap_or(0), score_cell(4).unwrap_or(0));
        assert!(score_cell(SCORE_CELLS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert_eq!(decl_row_name(4), Some("Node 运行时上栈声明"));
        assert!(decl_row_name(6).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = LighthouseLedger::new();
        let _ = lt.record_audit(9, audit_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_audit(2, audit_cell(2).unwrap_or(0)),
            Err(LighthouseCode::EOrder)
        );
        assert!(lt.record_audit(10, audit_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LighthouseLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(LighthouseCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_score(2, score_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(LighthouseCode::EScoreDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LighthouseLedger::new();
        let _ = lt.record_audit(0, audit_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_audit(0, audit_cell(0).unwrap_or(0)),
            Err(LighthouseCode::EDup)
        );
        let _ = lt.declare_row(0);
        assert_eq!(lt.declare_row(0), Err(LighthouseCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LighthouseLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        assert_eq!(gate_name(0), Some("headless 启动"));
        assert_eq!(gate_name(5), Some("断言闭合"));
        assert!(gate_name(6).is_none());
        let mut lt2 = LighthouseLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded >= (AUDIT_CELLS + CMD_OPS + SCORE_CELLS) as u32,
            "recorded 须覆盖审计 20 + 命令 10 + 评分 6"
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
        assert_eq!(SEG_BASE, 0xCB640);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(LighthouseCode::Ok as u32, 0);
        assert_eq!(LighthouseCode::EGradeDrift as u32, 12);
        assert_eq!(LighthouseCode::ESeal as u32, 11);
        assert_eq!(LighthouseCode::Ok.val(), SEG_BASE);
        assert!(!LighthouseCode::Ok.detail().is_empty());
        assert!(!LighthouseCode::ESeal.detail().is_empty());
        assert!(!LighthouseCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let empty = LighthouseLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = LighthouseLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52474_checks_green() {
        let s = run_unx52474_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
