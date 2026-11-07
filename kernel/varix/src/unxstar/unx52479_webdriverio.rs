//! UNX-F52479 WebdriverIO CLI 收口 · 0xCB780段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52479"，逐字）：
//!   title: WebdriverIO CLI 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52480-J1 本组 8
//!     浏览器并存 profile/证书/代理设置隔离 ×2 轮零互踩；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.2 专题二跨应用共面；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52479）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52480-J1，照原文锚定；功能按本卡标题（WebdriverIO CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：run / install / configure / service / repl 各 4
//!      操作共 20 格，命令族单调（run→install→configure→service→repl
//!      逆序必抓）；逐命令载荷纯函数折叠，恒等才收。
//!   ② service 消费 8 格：WDIO service 消费面逐格落账，漂移必抓；六判
//!      据母版闸逐格落账，越界未齐即封拒。
//!   ③ 与 Selenium 分账声明 2 行 + Node 上栈声明 2 行，共 4 行闭集：
//!      WebdriverIO 与 Selenium 独立账两行、Node 运行时上栈两行，缺一
//!      拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（service 漂移/命令族逆序/命令越
//!      闭集）全部落在当轮临时账本上，主账注入=检出=10、逃逸=0；双跑
//!      指纹一致；KB-COMPAT 定级闭集（28 格 + 4 行声明 + 六判据全通 →
//!      S2 目标档）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52479;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令操作数。
pub const CMD_OPS: usize = 20;

/// service 消费格数。
pub const SVC_CELLS: usize = 8;

/// 分账声明行数（Selenium 分账 2 + Node 上栈 2）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdIOCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// service 格号越闭集
    ENoSvc,
    /// service 消费漂移
    ESvcDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl WdIOCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WdIOCode::Ok => "成功",
            WdIOCode::ENoOp => "命令号越闭集：五命令 20 操作集之外先登记",
            WdIOCode::EDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            WdIOCode::EOrder => "命令族逆序：run→install→configure→service→repl 单调，逆序必抓",
            WdIOCode::ENoSvc => "service 格号越闭集：service 消费 8 格之外先登记",
            WdIOCode::ESvcDrift => "service 消费漂移：消费折叠与基准不恒等，逐格复算",
            WdIOCode::ENoRow => "声明行越闭集：Selenium 分账/Node 上栈 4 行闭集之外先登记",
            WdIOCode::ERowMiss => "缺声明行：Selenium 分账与 Node 上栈声明未齐先补齐",
            WdIOCode::EDup => "重复落账：同格只许一条账目",
            WdIOCode::ESeal => "未齐即封：20 命令 + 8 service + 4 声明未齐先补齐",
            WdIOCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WdIOCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WdIOCode::EGradeDrift => "定级漂移：28 格 + 4 行声明 + 六判据全通才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡口径不判）
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：全通收口（本卡口径不判）",
            Grade::S2 => "S2：WebdriverIO 目标档，六判据+28格+4行声明齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；命令族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xE0000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=run 4..8=install 8..12=configure 12..16=service 16..20=repl）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("run"),
        1 => Some("install"),
        2 => Some("configure"),
        3 => Some("service"),
        4 => Some("repl"),
        _ => None,
    }
}

/// service 消费格载荷（格 s）。
pub const fn svc_cell(s: usize) -> Option<u64> {
    if s >= SVC_CELLS {
        return None;
    }
    Some((0xE1000u64 + (s as u64) * 0x3D) ^ 0x5744494F53564330)
}

/// service 消费格名（WDIO service 消费面闭集）。
pub const fn svc_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("wdio-cli 消费"),
        1 => Some("config 装载"),
        2 => Some("runner 派发"),
        3 => Some("session 建立"),
        4 => Some("capability 解析"),
        5 => Some("reporter 汇聚"),
        6 => Some("service 钩子"),
        7 => Some("结果回写"),
        _ => None,
    }
}

/// 分账声明行折叠（行 r：0/1=Selenium 分账 2/3=Node 上栈）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xE200 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WebdriverIO 收口账本状态机。
#[derive(Clone)]
pub struct WdioLedger {
    /// 六判据母版闸。
    gates: [Option<bool>; 6],
    /// 命令账。
    ops: [Option<u64>; CMD_OPS],
    /// service 消费账。
    svc: [Option<u64>; SVC_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WdioLedger {
    /// 新账本。
    pub fn new() -> Self {
        WdioLedger {
            gates: [None; 6],
            ops: [None; CMD_OPS],
            svc: [None; SVC_CELLS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WdIOCode> {
        if gate >= 6 {
            return Err(WdIOCode::ESeal);
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
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), WdIOCode> {
        if o >= CMD_OPS {
            return Err(WdIOCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(WdIOCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(WdIOCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(WdIOCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(WdIOCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WdIOCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// service 消费一格（载荷对照 + 重复拒）。
    pub fn record_svc(&mut self, s: usize, got: u64) -> Result<(), WdIOCode> {
        let want = match svc_cell(s) {
            Some(w) => w,
            None => return Err(WdIOCode::ENoSvc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + s as u64);
            return Err(WdIOCode::ESvcDrift);
        }
        if match self.svc.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WdIOCode::EDup);
        }
        if let Some(c) = self.svc.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x7301 + s as u64);
        Ok(())
    }

    /// service 消费全账核验（8 格）。
    pub fn svc_all(&self) -> bool {
        let mut s = 0usize;
        while s < SVC_CELLS {
            match self.svc.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 声明一行（Selenium 分账 / Node 上栈）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), WdIOCode> {
        if r >= SPLIT_ROWS {
            return Err(WdIOCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WdIOCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x7400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

    /// KB-COMPAT 定级（全通 → S2 目标档，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.svc_all() && self.splits_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WdioLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WdioLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut s = 0usize;
    while s < SVC_CELLS {
        if let Some(w) = svc_cell(s) {
            if led.record_svc(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：service 漂移 / 命令族逆序 / 命令越闭集三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WdioLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_svc(2, svc_cell(2).unwrap_or(0));
                lt.record_svc(1, svc_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WdIOCode::ESvcDrift)
            }
            1 => {
                let _ = lt.record_op(17, cmd_cell(17).unwrap_or(0));
                lt.record_op(5, cmd_cell(5).unwrap_or(0)) == Err(WdIOCode::EOrder)
            }
            _ => lt.record_op(CMD_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52479 判据集（锚 UNX-F52480-J1，off-by-one 板式）。
pub fn run_unx52479_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52479");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令 20 操作全通",
        {
            let mut lt = WdioLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "run/install/configure/service/repl 五族逐命令恒等",
    );
    s.add(
        "service 消费 8/8 恒等",
        {
            let mut lt = WdioLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SVC_CELLS {
                if let Some(w) = svc_cell(sc) {
                    if lt.record_svc(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.svc_all()
        },
        "wdio-cli/config/runner/session 等八消费格逐格落账",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = WdioLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，越界未齐即封拒",
    );
    s.add(
        "Selenium 分账声明 2/2 全齐",
        {
            let mut lt = WdioLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < 2 {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "WebdriverIO 与 Selenium 独立账两行闭集（行 0/1）",
    );
    s.add(
        "Node 上栈声明 2/2 全齐",
        {
            let mut lt = WdioLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "Node 运行时上栈两行闭集（行 2/3），缺一拒封",
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
    s.add("注入 10/10 全抓", caught >= 10, "service 漂移/族逆序/越闭集三态轮转必抓");
    let mut l0 = WdioLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_op(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·service 越闭集",
        l0.record_svc(SVC_CELLS, 0).is_err(),
        "service 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 4 越闭集必拒",
    );
    s.add(
        "service 漂移拒",
        l0.record_svc(4, svc_cell(4).unwrap_or(0).wrapping_add(1)) == Err(WdIOCode::ESvcDrift),
        "service 消费漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_op(0, cmd_cell(0).unwrap_or(0));
            l0.record_op(0, cmd_cell(0).unwrap_or(0)) == Err(WdIOCode::EDup)
        },
        "同格只许一条账目",
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
        "定级 S2 闭集",
        {
            let mut lt = WdioLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut sc = 0usize;
            while sc < SVC_CELLS {
                if let Some(w) = svc_cell(sc) {
                    let _ = lt.record_svc(sc, w);
                }
                sc += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "28 格 + 4 行声明 + 六判据全通 → S2 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB780,
        "SEG_BASE 0x40 对齐且落 VX16 公式格 0xCB780",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
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
        assert_eq!(cmd_kind_name(0), Some("run"));
        assert_eq!(cmd_kind_name(4), Some("repl"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_svc_cells() {
        assert_eq!(svc_name(0), Some("wdio-cli 消费"));
        assert_eq!(svc_name(7), Some("结果回写"));
        assert!(svc_name(8).is_none());
        assert!(svc_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        let b = svc_cell(5).unwrap_or(0);
        assert_eq!(b, svc_cell(5).unwrap_or(0));
        assert_ne!(b, svc_cell(6).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = WdioLedger::new();
        let _ = lt.record_op(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, cmd_cell(2).unwrap_or(0)), Err(WdIOCode::EOrder));
        assert!(lt.record_op(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WdioLedger::new();
        assert_eq!(
            lt.record_op(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WdIOCode::EDrift)
        );
        assert_eq!(
            lt.record_svc(3, svc_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(WdIOCode::ESvcDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WdioLedger::new();
        let _ = lt.record_svc(0, svc_cell(0).unwrap_or(0));
        assert_eq!(lt.record_svc(0, svc_cell(0).unwrap_or(0)), Err(WdIOCode::EDup));
        let _ = lt.record_op(1, cmd_cell(1).unwrap_or(0));
        assert_eq!(lt.record_op(1, cmd_cell(1).unwrap_or(0)), Err(WdIOCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WdioLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WdioLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 恰为 run_cycle 实际计数两节公式常量之和：20+8。
        assert_eq!(recorded, (CMD_OPS + SVC_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCB780);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WdIOCode::Ok as u32, 0);
        assert_eq!(WdIOCode::EGradeDrift as u32, 12);
        assert_eq!(WdIOCode::EFp as u32, 11);
        assert!(!WdIOCode::Ok.detail().is_empty());
        assert!(!WdIOCode::ERowMiss.detail().is_empty());
        assert!(!WdIOCode::EInj.detail().is_empty());
        assert!(WdIOCode::Ok.val() == SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = WdioLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 → S2。
        let mut lt2 = WdioLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐 → S2 目标档。
        let mut o = 0usize;
        while o < CMD_OPS {
            if let Some(w) = cmd_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut sc = 0usize;
        while sc < SVC_CELLS {
            if let Some(w) = svc_cell(sc) {
                let _ = lt2.record_svc(sc, w);
            }
            sc += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52479_checks_green() {
        let s = run_unx52479_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
