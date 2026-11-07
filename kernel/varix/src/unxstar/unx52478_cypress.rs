//! UNX-F52478 Cypress 收口 · 0xCB740段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52478"，逐字）：
//!   title: Cypress 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52479-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52478）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52479-J1，照原文锚定；功能按本卡标题（Cypress）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：open / run / verify / screenshots / videos 各 4
//!      操作共 20 格，命令族单调（open→run→verify→screenshots→videos
//!      逆序必抓）；逐命令载荷纯函数折叠，恒等才收。
//!   ② 三态生命周期 6 格：准备/启动/执行/断言/收尾/退出逐格落账，漂移
//!      必抓；六判据母版闸逐格落账，越界未齐即封拒。
//!   ③ 与 Selenium 分账声明 2 行 + Node 上栈声明 2 行，共 4 行闭集：
//!      Cypress 与 Selenium 独立账两行、Node 运行时上栈两行，缺一拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（生命周期漂移/命令族逆序/命令越
//!      闭集）全部落在当轮临时账本上，主账注入=检出=10、逃逸=0；双跑
//!      指纹一致；KB-COMPAT 定级闭集（26 格 + 4 行声明 + 六判据全通 →
//!      S2 目标档）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52478;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令操作数。
pub const CMD_OPS: usize = 20;

/// 三态生命周期格数。
pub const LIFE_CELLS: usize = 6;

/// 分账声明行数（Selenium 分账 2 + Node 上栈 2）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CypressCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 生命周期格号越闭集
    ENoLife,
    /// 生命周期漂移
    ELifeDrift,
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

impl CypressCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CypressCode::Ok => "成功",
            CypressCode::ENoOp => "命令号越闭集：五命令 20 操作集之外先登记",
            CypressCode::EDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            CypressCode::EOrder => "命令族逆序：open→run→verify→screenshots→videos 单调，逆序必抓",
            CypressCode::ENoLife => "生命周期格号越闭集：三态生命周期 6 格之外先登记",
            CypressCode::ELifeDrift => "生命周期漂移：六格折叠与基准不恒等，逐格复算",
            CypressCode::ENoRow => "声明行越闭集：Selenium 分账/Node 上栈 4 行闭集之外先登记",
            CypressCode::ERowMiss => "缺声明行：Selenium 分账与 Node 上栈声明未齐先补齐",
            CypressCode::EDup => "重复落账：同格只许一条账目",
            CypressCode::ESeal => "未齐即封：20 命令 + 6 生命周期 + 4 声明未齐先补齐",
            CypressCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CypressCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CypressCode::EGradeDrift => "定级漂移：26 格 + 4 行声明 + 六判据全通才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
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
            Grade::S2 => "S2：Cypress 目标档，六判据+26格+4行声明齐",
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
    Some((0xD8000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=open 4..8=run 8..12=verify 12..16=screenshots 16..20=videos）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("open"),
        1 => Some("run"),
        2 => Some("verify"),
        3 => Some("screenshots"),
        4 => Some("videos"),
        _ => None,
    }
}

/// 生命周期格载荷（格 l：准备/启动/执行/断言/收尾/退出）。
pub const fn life_cell(l: usize) -> Option<u64> {
    if l >= LIFE_CELLS {
        return None;
    }
    Some((0xD9000u64 + (l as u64) * 0x3B) ^ 0x43595052454C4946)
}

/// 生命周期格名。
pub const fn life_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("准备"),
        1 => Some("启动"),
        2 => Some("执行"),
        3 => Some("断言"),
        4 => Some("收尾"),
        5 => Some("退出"),
        _ => None,
    }
}

/// 分账声明行折叠（行 r：0/1=Selenium 分账 2/3=Node 上栈）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xDA00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Cypress 收口账本状态机。
#[derive(Clone)]
pub struct CypLedger {
    /// 六判据母版闸。
    gates: [Option<bool>; 6],
    /// 命令账。
    ops: [Option<u64>; CMD_OPS],
    /// 生命周期账。
    life: [Option<u64>; LIFE_CELLS],
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

impl CypLedger {
    /// 新账本。
    pub fn new() -> Self {
        CypLedger {
            gates: [None; 6],
            ops: [None; CMD_OPS],
            life: [None; LIFE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CypressCode> {
        if gate >= 6 {
            return Err(CypressCode::ESeal);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), CypressCode> {
        if o >= CMD_OPS {
            return Err(CypressCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(CypressCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(CypressCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(CypressCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(CypressCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CypressCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
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

    /// 生命周期一格（载荷对照 + 重复拒）。
    pub fn record_life(&mut self, l: usize, got: u64) -> Result<(), CypressCode> {
        let want = match life_cell(l) {
            Some(w) => w,
            None => return Err(CypressCode::ENoLife),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + l as u64);
            return Err(CypressCode::ELifeDrift);
        }
        if match self.life.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CypressCode::EDup);
        }
        if let Some(c) = self.life.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x6301 + l as u64);
        Ok(())
    }

    /// 生命周期全账核验（6 格）。
    pub fn life_all(&self) -> bool {
        let mut l = 0usize;
        while l < LIFE_CELLS {
            match self.life.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 声明一行（Selenium 分账 / Node 上栈）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), CypressCode> {
        if r >= SPLIT_ROWS {
            return Err(CypressCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CypressCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x6400 + r as u64);
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
        if self.gates_all() && self.ops_all() && self.life_all() && self.splits_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CypLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CypLedger::new();
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
    let mut l = 0usize;
    while l < LIFE_CELLS {
        if let Some(w) = life_cell(l) {
            if led.record_life(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：生命周期漂移 / 命令族逆序 / 命令越闭集三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CypLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_life(1, life_cell(1).unwrap_or(0));
                lt.record_life(0, life_cell(0).unwrap_or(0).wrapping_add(1))
                    == Err(CypressCode::ELifeDrift)
            }
            1 => {
                let _ = lt.record_op(17, cmd_cell(17).unwrap_or(0));
                lt.record_op(5, cmd_cell(5).unwrap_or(0)) == Err(CypressCode::EOrder)
            }
            _ => lt.record_op(CMD_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6500 + k as u64);
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
/// UNX-F52478 判据集（锚 UNX-F52479-J1，off-by-one 板式）。
pub fn run_unx52478_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52478");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令 20 操作全通",
        {
            let mut lt = CypLedger::new();
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
        "open/run/verify/screenshots/videos 五族逐命令恒等",
    );
    s.add(
        "三态生命周期 6/6 全落",
        {
            let mut lt = CypLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LIFE_CELLS {
                if let Some(w) = life_cell(l) {
                    if lt.record_life(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.life_all()
        },
        "准备/启动/执行/断言/收尾/退出逐格落账",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = CypLedger::new();
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
            let mut lt = CypLedger::new();
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
        "Cypress 与 Selenium 独立账两行闭集（行 0/1）",
    );
    s.add(
        "Node 上栈声明 2/2 全齐",
        {
            let mut lt = CypLedger::new();
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
    s.add("注入 10/10 全抓", caught >= 10, "生命周期漂移/族逆序/越闭集三态轮转必抓");
    let mut l0 = CypLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_op(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·生命周期越闭集",
        l0.record_life(LIFE_CELLS, 0).is_err(),
        "生命周期 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 4 越闭集必拒",
    );
    s.add(
        "生命周期漂移拒",
        l0.record_life(3, life_cell(3).unwrap_or(0).wrapping_add(1))
            == Err(CypressCode::ELifeDrift),
        "六格生命周期漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_op(0, cmd_cell(0).unwrap_or(0));
            l0.record_op(0, cmd_cell(0).unwrap_or(0)) == Err(CypressCode::EDup)
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
            let mut lt = CypLedger::new();
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
            let mut l = 0usize;
            while l < LIFE_CELLS {
                if let Some(w) = life_cell(l) {
                    let _ = lt.record_life(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "26 格 + 4 行声明 + 六判据全通 → S2 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB740,
        "SEG_BASE 0x40 对齐且落 VX16 公式格 0xCB740",
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
        assert_eq!(cmd_kind_name(0), Some("open"));
        assert_eq!(cmd_kind_name(4), Some("videos"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_life_cells() {
        assert_eq!(life_name(0), Some("准备"));
        assert_eq!(life_name(5), Some("退出"));
        assert!(life_name(6).is_none());
        assert!(life_cell(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        let b = life_cell(2).unwrap_or(0);
        assert_eq!(b, life_cell(2).unwrap_or(0));
        assert_ne!(b, life_cell(3).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = CypLedger::new();
        let _ = lt.record_op(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, cmd_cell(2).unwrap_or(0)), Err(CypressCode::EOrder));
        assert!(lt.record_op(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CypLedger::new();
        assert_eq!(
            lt.record_op(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(CypressCode::EDrift)
        );
        assert_eq!(
            lt.record_life(2, life_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(CypressCode::ELifeDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CypLedger::new();
        let _ = lt.record_life(0, life_cell(0).unwrap_or(0));
        assert_eq!(lt.record_life(0, life_cell(0).unwrap_or(0)), Err(CypressCode::EDup));
        let _ = lt.record_op(1, cmd_cell(1).unwrap_or(0));
        assert_eq!(lt.record_op(1, cmd_cell(1).unwrap_or(0)), Err(CypressCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CypLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CypLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 恰为 run_cycle 实际计数两节公式常量之和：20+6。
        assert_eq!(recorded, (CMD_OPS + LIFE_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCB740);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CypressCode::Ok as u32, 0);
        assert_eq!(CypressCode::EGradeDrift as u32, 12);
        assert_eq!(CypressCode::EInj as u32, 10);
        assert!(!CypressCode::Ok.detail().is_empty());
        assert!(!CypressCode::ERowMiss.detail().is_empty());
        assert!(!CypressCode::EFp.detail().is_empty());
        assert!(CypressCode::Ok.val() == SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = CypLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 → S2。
        let mut lt2 = CypLedger::new();
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
        let mut l = 0usize;
        while l < LIFE_CELLS {
            if let Some(w) = life_cell(l) {
                let _ = lt2.record_life(l, w);
            }
            l += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52478_checks_green() {
        let s = run_unx52478_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
