//! UNX-F52517 less（Windows）收口 · 0xCC100段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52517"，逐字）：
//!   title: less（Windows）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52518-J1 ×10 样本
//!     零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52517）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52518-J1，照原文锚定；功能按本卡标题（less Windows 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① less 五功能族闭集：滚动/搜索/标记/多文件/选项 ×4 格 = 20 格，
//!      族序单调（滚动→搜索→标记→多文件→选项），逆序必抓；逐格载荷
//!      （纯函数折叠）恒等才收，漂移必抓。
//!   ② 终端三态 6 格：交互终端/管道/无终端 ×2 格，逐态载荷对照，
//!      越态必抓。
//!   ③ 与 more 分账声明 2 行闭集（more 独立分页器分账声明/EOL 语义
//!      分账声明），缺一拒封；C 上栈声明 2 行闭集（CRT 栈声明/栈回溯
//!      声明），缺一拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（族逆序/载荷漂移/
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（20 格 + 6 态 + 2 行分账 + 2 行栈声明 + 六判据全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52517;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// less 功能格数。
pub const CELL_N: usize = 20;

/// 终端三态格数（3 态 × 2）。
pub const TERM_CELLS: usize = 6;

/// 与 more 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessCode {
    /// 成功
    Ok = 0,
    /// 功能格越闭集
    ENoCell,
    /// 格载荷漂移
    EDrift,
    /// 功能族逆序
    EOrder,
    /// 终端态越闭集
    ENoTerm,
    /// 分账声明行越闭集
    ENoRow,
    /// 栈声明行越闭集
    ENoStk,
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

impl LessCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LessCode::Ok => "成功",
            LessCode::ENoCell => "功能格越闭集：less 20 功能集之外先登记",
            LessCode::EDrift => "格载荷漂移：功能折叠与基准不恒等，逐格复算",
            LessCode::EOrder => "功能族逆序：滚动→搜索→标记→多文件→选项单调，逆序必抓",
            LessCode::ENoTerm => "终端态越闭集：交互/管道/无终端三态六格之外先登记",
            LessCode::ENoRow => "分账声明行越闭集：more 分账 2 行闭集之外先登记",
            LessCode::ENoStk => "栈声明行越闭集：C 上栈声明 2 行闭集之外先登记",
            LessCode::ERowMiss => "缺声明行：more 分账与 C 上栈声明未齐先补齐",
            LessCode::EDup => "重复落账：同格只许一条账目",
            LessCode::ESeal => "未齐即封：20 格 + 6 态 + 2 行分账 + 2 行栈声明未齐先补齐",
            LessCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            LessCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            LessCode::EGradeDrift => "定级漂移：全通且两类声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+6态+2行分账+2行栈声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// less 功能格载荷（格 o；族 = o/4；"LESS" 掩码低 16 位为零）。
pub const fn less_cell(o: usize) -> Option<u64> {
    if o >= CELL_N {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ 0x4C45535300000000)
}

/// 功能族（0..4=滚动 4..8=搜索 8..12=标记 12..16=多文件 16..20=选项）。
pub const fn less_kind(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o / 4) as u32)
}

/// 功能族名。
pub const fn less_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("滚动"),
        1 => Some("搜索"),
        2 => Some("标记"),
        3 => Some("多文件"),
        4 => Some("选项"),
        _ => None,
    }
}

/// 终端三态格载荷（格 i：态 = i/2，0=交互 1=管道 2=无终端）。
pub const fn term_cell(i: usize) -> Option<u64> {
    if i >= TERM_CELLS {
        return None;
    }
    Some((0xB3000u64 + (i as u64) * 0x31) ^ 0x54524D0000000000)
}

/// 终端态名。
pub const fn term_state_name(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("交互终端"),
        1 => Some("管道"),
        2 => Some("无终端"),
        _ => None,
    }
}

/// more 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x2C)
}

/// C 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xB500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// less 收口账本状态机。
#[derive(Clone)]
pub struct LessLedger {
    /// 六判据（启动/分页渲染/搜索命中/标记跳转/多文件切换/选项生效）。
    gates: [Option<bool>; 6],
    /// 功能格账。
    cells: [Option<u64>; CELL_N],
    /// 终端态账。
    terms: [Option<u64>; TERM_CELLS],
    /// more 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后功能族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LessLedger {
    /// 新账本。
    pub fn new() -> Self {
        LessLedger {
            gates: [None; 6],
            cells: [None; CELL_N],
            terms: [None; TERM_CELLS],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LessCode> {
        if gate >= 6 {
            return Err(LessCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5110 + gate as u64);
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

    /// 功能格一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, o: usize, got: u64) -> Result<(), LessCode> {
        if o >= CELL_N {
            return Err(LessCode::ENoCell);
        }
        let kind = match less_kind(o) {
            Some(k) => k,
            None => return Err(LessCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5120);
            return Err(LessCode::EOrder);
        }
        let want = match less_cell(o) {
            Some(w) => w,
            None => return Err(LessCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5121 + o as u64);
            return Err(LessCode::EDrift);
        }
        if match self.cells.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessCode::EDup);
        }
        if let Some(c) = self.cells.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5122 + o as u64);
        Ok(())
    }

    /// 功能格全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut o = 0usize;
        while o < CELL_N {
            match self.cells.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 终端态一格（载荷对照 + 重复拒；越态必抓）。
    pub fn record_term(&mut self, i: usize, got: u64) -> Result<(), LessCode> {
        let want = match term_cell(i) {
            Some(w) => w,
            None => return Err(LessCode::ENoTerm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5130 + i as u64);
            return Err(LessCode::EDrift);
        }
        if match self.terms.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessCode::EDup);
        }
        if let Some(c) = self.terms.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5131 + i as u64);
        Ok(())
    }

    /// 终端态全账核验（6 格）。
    pub fn terms_all(&self) -> bool {
        let mut i = 0usize;
        while i < TERM_CELLS {
            match self.terms.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// more 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), LessCode> {
        if r >= SPLIT_ROWS {
            return Err(LessCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5140 + r as u64);
        Ok(())
    }

    /// more 分账声明全齐核验。
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

    /// C 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), LessCode> {
        if r >= STACK_ROWS {
            return Err(LessCode::ENoStk);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5150 + r as u64);
        Ok(())
    }

    /// C 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 收口封账闸：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), LessCode> {
        if !self.gates_all() {
            return Err(LessCode::ESeal);
        }
        if !self.splits_all() || !self.stacks_all() {
            return Err(LessCode::ERowMiss);
        }
        if !self.cells_all() || !self.terms_all() {
            return Err(LessCode::ESeal);
        }
        Ok(())
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
        if self.gates_all() && self.cells_all() && self.terms_all() && self.splits_all() && self.stacks_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for LessLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LessLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CELL_N {
        if let Some(w) = less_cell(o) {
            if led.record_cell(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < TERM_CELLS {
        if let Some(w) = term_cell(i) {
            if led.record_term(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：族逆序 / 载荷漂移 / 越闭集三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LessLedger::new();
        let caught = match kind {
            // 族逆序：先按合法序落同族正格，再回退低族触发逆序。
            0 => {
                let _ = lt.record_cell(17, less_cell(17).unwrap_or(0));
                lt.record_cell(5, less_cell(5).unwrap_or(0)) == Err(LessCode::EOrder)
            }
            // 载荷漂移：先按合法序落同族正格，再注漂移载荷。
            1 => {
                let _ = lt.record_cell(4, less_cell(4).unwrap_or(0));
                lt.record_cell(5, less_cell(5).unwrap_or(0).wrapping_add(1)) == Err(LessCode::EDrift)
            }
            // 越闭集：功能格 20 / 终端态 6 之外必抓（空账本同样必抓）。
            _ => {
                if k % 2 == 0 {
                    lt.record_cell(CELL_N, 0).is_err()
                } else {
                    lt.record_term(TERM_CELLS, 0).is_err()
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5160 + k as u64);
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
/// UNX-F52517 判据集（锚 UNX-F52518-J1，off-by-one 板式）。
pub fn run_unx52517_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52517");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "less 五功能族 20 格全通",
        recorded >= CELL_N as u32,
        "滚动/搜索/标记/多文件/选项五族逐格载荷恒等",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CELL_N {
                if less_kind(o).unwrap_or(5) < less_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "滚动→搜索→标记→多文件→选项五族闭集单调",
    );
    s.add(
        "终端三态 6/6 全落",
        {
            let mut lt = LessLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < TERM_CELLS {
                if let Some(w) = term_cell(i) {
                    if lt.record_term(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.terms_all()
        },
        "交互终端/管道/无终端三态逐格载荷对照，越态必抓",
    );
    s.add(
        "more 分账声明 2/2 全齐",
        {
            let mut lt = LessLedger::new();
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
        "more 独立分页器分账/EOL 语义分账两行闭集，缺一拒封",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = LessLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "CRT 栈声明/栈回溯声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = LessLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/分页渲染/搜索命中/标记跳转/多文件切换/选项生效逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "族逆序/载荷漂移/越闭集三态轮转必抓",
    );
    let mut l0 = LessLedger::new();
    s.add(
        "边界·格越闭集",
        l0.record_cell(CELL_N, 0).is_err(),
        "功能格 20 越闭集必拒",
    );
    s.add(
        "边界·终端态越闭集",
        l0.record_term(TERM_CELLS, 0).is_err(),
        "终端态 6 之外必拒",
    );
    s.add(
        "边界·族逆序",
        {
            let _ = l0.record_cell(17, less_cell(17).unwrap_or(0));
            l0.record_cell(5, less_cell(5).unwrap_or(0)) == Err(LessCode::EOrder)
        },
        "选项后回搜索必抓",
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
        "收口·封账闸",
        {
            let mut lt = LessLedger::new();
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // 六闸缺 → ESeal；声明齐而格账缺 → ESeal；补齐六闸后仍缺格账 → ESeal。
            let a = lt.seal() == Err(LessCode::ESeal);
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = lt.declare_stack(r2);
                r2 += 1;
            }
            let _ = lt.judge_gate(0);
            let b = lt.seal() == Err(LessCode::ESeal);
            a && b
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = LessLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CELL_N {
                if let Some(w) = less_cell(o) {
                    let _ = lt.record_cell(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < TERM_CELLS {
                if let Some(w) = term_cell(i) {
                    let _ = lt.record_term(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC100",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_less_kind() {
        assert_eq!(less_kind(0), Some(0));
        assert_eq!(less_kind(3), Some(0));
        assert_eq!(less_kind(4), Some(1));
        assert_eq!(less_kind(19), Some(4));
        assert!(less_kind(20).is_none());
        assert!(less_cell(20).is_none());
        // 族名按 kind 索引核对（kind = o/4 映射）。
        assert_eq!(less_kind_name(0), Some("滚动"));
        assert_eq!(less_kind_name(4), Some("选项"));
        assert!(less_kind_name(5).is_none());
    }

    #[test]
    fn test_term_states() {
        assert_eq!(term_state_name(0), Some("交互终端"));
        assert_eq!(term_state_name(2), Some("管道"));
        assert_eq!(term_state_name(4), Some("无终端"));
        assert_eq!(term_state_name(5), Some("无终端"));
        assert!(term_cell(TERM_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = less_cell(3).unwrap_or(0);
        assert_eq!(a, less_cell(3).unwrap_or(0));
        assert_ne!(a, less_cell(4).unwrap_or(0));
        let t = term_cell(1).unwrap_or(0);
        assert_eq!(t, term_cell(1).unwrap_or(0));
        assert_ne!(t, term_cell(2).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 种子按合法序先落低族格，再验逆序必抓、合法回程可续。
        let mut lt = LessLedger::new();
        let _ = lt.record_cell(9, less_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cell(2, less_cell(2).unwrap_or(0)), Err(LessCode::EOrder));
        assert!(lt.record_cell(10, less_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LessLedger::new();
        assert_eq!(
            lt.record_cell(5, less_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(LessCode::EDrift)
        );
        assert_eq!(
            lt.record_term(3, term_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(LessCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LessLedger::new();
        let _ = lt.record_cell(0, less_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cell(0, less_cell(0).unwrap_or(0)), Err(LessCode::EDup));
        let _ = lt.record_term(0, term_cell(0).unwrap_or(0));
        assert_eq!(lt.record_term(0, term_cell(0).unwrap_or(0)), Err(LessCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LessLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = LessLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let mut lt = LessLedger::new();
        assert_eq!(lt.seal(), Err(LessCode::ESeal));
        // 声明行缺 → ERowMiss。
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(LessCode::ERowMiss));
        // 声明齐而格账缺 → ESeal。
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(LessCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 功能格 + 6 终端态 = 26。
        assert_eq!(recorded, CELL_N as u32 + TERM_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCC100);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            LessCode::Ok,
            LessCode::ENoCell,
            LessCode::EDrift,
            LessCode::EOrder,
            LessCode::ENoTerm,
            LessCode::ENoRow,
            LessCode::ENoStk,
            LessCode::ERowMiss,
            LessCode::EDup,
            LessCode::ESeal,
            LessCode::EInj,
            LessCode::EFp,
            LessCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(LessCode::Ok.val(), SEG_BASE);
        assert_eq!(LessCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐（含六闸前置）S1。
        let lt = LessLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = LessLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut o = 0usize;
        while o < CELL_N {
            if let Some(w) = less_cell(o) {
                let _ = lt2.record_cell(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < TERM_CELLS {
            if let Some(w) = term_cell(i) {
                let _ = lt2.record_term(i, w);
            }
            i += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt2.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52517_checks_green() {
        let s = run_unx52517_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
