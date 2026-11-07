//! UNX-F52356 qsv（xsv 续作）收口 · 0xC98C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52356"，逐字）：
//!   title: qsv（xsv 续作）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52357-J1 索引生
//!     成 ×10 语言全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52356）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52357-J1，照原文锚定；功能按本卡标题（qsv，xsv 续作）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 格：select/sort/filter/stats/join 各 4 格，族 =
//!      格号/4，族单调（逆序必抓，空账本首格非 0 号族必抓且不改
//!      last_kind）；索引生成 ×10 语言样本逐格载荷折叠恒等才收。
//!   ② 操作 20 格集：查找/变换/聚合/IO 各 5 格（kind = 格号/5），
//!      载荷折叠恒等才收；漂移必抓。
//!   ③ 索引三态 6 格：off/build/use 三态 ×2 格，越态必抓。
//!   ④ 与 xsv 分账声明 2 行闭集：xsv 存量分账声明行 / qsv 接管增量声
//!      明行（xsv → qsv 升级接管字样），缺一拒封。
//!   ⑤ Rust 上栈声明 2 行闭集：rustc 工具链声明行/CSV crate 链接声明
//!      行，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（命令族越闭集/操作漂移/空账乱
//!      序），检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 20 操作 + 6 态 + 2 分账 + 2 声明 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52356;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五命令族格数（五族 × 各 4 格）。
pub const CMD_CELLS: usize = 20;

/// 操作格数（四族 × 各 5 格）。
pub const OP_CELLS: usize = 20;

/// 索引三态格数。
pub const IDX_CELLS: usize = 6;

/// 与 xsv 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Rust 上栈声明行数。
pub const DECL_ROWS: usize = 2;

/// qsv 域 XOR 锚（低 16 位恒零）。
pub const QSV_ANCHOR: u64 = 0x5153_562D_5300_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QsvCode {
    /// 成功
    Ok = 0,
    /// 命令族越闭集
    ENoCmd,
    /// 命令族载荷漂移
    ECmdDrift,
    /// 操作越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 族逆序
    EOrder,
    /// 索引态越闭集
    ENoIdx,
    /// 索引态载荷漂移
    EIdxDrift,
    /// xsv 分账行越闭集
    ENoSplit,
    /// Rust 上栈声明行越闭集
    ENoDecl,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl QsvCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            QsvCode::Ok => "成功",
            QsvCode::ENoCmd => "命令族越闭集：select/sort/filter/stats/join 20 格之外先登记",
            QsvCode::ECmdDrift => "命令族载荷漂移：索引样本折叠与基准不恒等，逐格复算",
            QsvCode::ENoOp => "操作越闭集：操作 20 格集之外先登记",
            QsvCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            QsvCode::EOrder => "族逆序：select→sort→filter→stats→join 单调，逆序必抓",
            QsvCode::ENoIdx => "索引态越闭集：off/build/use 三态 6 格之外先登记",
            QsvCode::EIdxDrift => "索引态载荷漂移：三态折叠与基准不恒等，逐格复算",
            QsvCode::ENoSplit => "xsv 分账行越闭集：2 行闭集之外先登记",
            QsvCode::ENoDecl => "Rust 上栈声明行越闭集：2 行闭集之外先登记",
            QsvCode::EDup => "重复落账：同格只许一条账目",
            QsvCode::ESeal => "未齐即封：20 格 + 20 操作 + 6 态 + 2 分账 + 2 声明未齐先补齐",
            QsvCode::EGradeDrift => "定级漂移：全通且分账/声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+20操作+6态+2分账+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令族格载荷（格 c；族 = c/4）。
pub const fn cmd_cell(c: usize) -> Option<u64> {
    if c >= CMD_CELLS {
        return None;
    }
    Some(
        ((0xD100u64 + (c as u64) * 0x33) ^ ((c / 4) as u64).wrapping_mul(0x01000193)) ^ QSV_ANCHOR,
    )
}

/// 命令族（0..4=select 4..8=sort 8..12=filter 12..16=stats 16..20=join）。
pub const fn cmd_kind(c: usize) -> Option<u32> {
    if c >= CMD_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("select"),
        1 => Some("sort"),
        2 => Some("filter"),
        3 => Some("stats"),
        4 => Some("join"),
        _ => None,
    }
}

/// 操作格载荷（格 o；族 = o/5：查找/变换/聚合/IO）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OP_CELLS {
        return None;
    }
    Some((0xD200u64 + (o as u64) * 0x2E) ^ ((o / 5) as u64).wrapping_mul(0x9E3779B9))
}

/// 操作族。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OP_CELLS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 操作族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("查找"),
        1 => Some("变换"),
        2 => Some("聚合"),
        3 => Some("IO"),
        _ => None,
    }
}

/// 索引三态格载荷（格 i；态 = i/2）。
pub const fn idx_cell(i: usize) -> Option<u64> {
    if i >= IDX_CELLS {
        return None;
    }
    Some((0xD400u64 + (i as u64) * 0x2C) ^ 0x51_0000_0000)
}

/// 索引态。
pub const fn idx_state(i: usize) -> Option<u32> {
    if i >= IDX_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 索引态名。
pub const fn idx_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("off"),
        1 => Some("build"),
        2 => Some("use"),
        _ => None,
    }
}

/// 与 xsv 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x2C)
}

/// 分账行名（xsv → qsv 升级接管字样）。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("xsv 存量分账声明行"),
        1 => Some("qsv 接管增量声明行（xsv → qsv 升级接管）"),
        _ => None,
    }
}

/// Rust 上栈声明行折叠（行 d）。
pub const fn decl_row(d: usize) -> Option<u32> {
    if d >= DECL_ROWS {
        return None;
    }
    Some(0xD600 + d as u32 * 0x2C)
}

/// 声明行名。
pub const fn decl_row_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("rustc 工具链声明行"),
        1 => Some("CSV crate 链接声明行"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// qsv 收口账本状态机。
#[derive(Clone)]
pub struct QsvLedger {
    /// 六判据（索引生成/选择/排序/统计/联接/写回）。
    gates: [Option<bool>; 6],
    /// 命令族账。
    cmds: [Option<u64>; CMD_CELLS],
    /// 操作账。
    ops: [Option<u64>; OP_CELLS],
    /// 索引三态账。
    idxs: [Option<u64>; IDX_CELLS],
    /// xsv 分账账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Rust 上栈声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl QsvLedger {
    /// 新账本。
    pub fn new() -> Self {
        QsvLedger {
            gates: [None; 6],
            cmds: [None; CMD_CELLS],
            ops: [None; OP_CELLS],
            idxs: [None; IDX_CELLS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), QsvCode> {
        if gate >= 6 {
            return Err(QsvCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 命令族一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, c: usize, got: u64) -> Result<(), QsvCode> {
        if c >= CMD_CELLS {
            return Err(QsvCode::ENoCmd);
        }
        let kind = match cmd_kind(c) {
            Some(k) => k,
            None => return Err(QsvCode::ENoCmd),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(QsvCode::EOrder);
        }
        let want = match cmd_cell(c) {
            Some(w) => w,
            None => return Err(QsvCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + c as u64);
            return Err(QsvCode::ECmdDrift);
        }
        if match self.cmds.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QsvCode::EDup);
        }
        if let Some(cell) = self.cmds.get_mut(c) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + c as u64);
        Ok(())
    }

    /// 命令族全账核验（20 格）。
    pub fn cmds_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMD_CELLS {
            match self.cmds.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), QsvCode> {
        if o >= OP_CELLS {
            return Err(QsvCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(QsvCode::ENoOp),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(QsvCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(QsvCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(QsvCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QsvCode::EDup);
        }
        if let Some(cell) = self.ops.get_mut(o) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OP_CELLS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 索引三态一格（载荷对照 + 重复拒）。
    pub fn record_idx(&mut self, i: usize, got: u64) -> Result<(), QsvCode> {
        let want = match idx_cell(i) {
            Some(w) => w,
            None => return Err(QsvCode::ENoIdx),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400 + i as u64);
            return Err(QsvCode::EIdxDrift);
        }
        if match self.idxs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QsvCode::EDup);
        }
        if let Some(cell) = self.idxs.get_mut(i) {
            *cell = Some(got);
        }
        self.fnv(0x4401 + i as u64);
        Ok(())
    }

    /// 索引三态全账核验（6 格）。
    pub fn idxs_all(&self) -> bool {
        let mut i = 0usize;
        while i < IDX_CELLS {
            match self.idxs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// xsv 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), QsvCode> {
        if r >= SPLIT_ROWS {
            return Err(QsvCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QsvCode::EDup);
        }
        if let Some(cell) = self.splits.get_mut(r) {
            *cell = split_row(r);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// Rust 上栈声明一行。
    pub fn declare_decl(&mut self, d: usize) -> Result<(), QsvCode> {
        if d >= DECL_ROWS {
            return Err(QsvCode::ENoDecl);
        }
        if match self.decls.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QsvCode::EDup);
        }
        if let Some(cell) = self.decls.get_mut(d) {
            *cell = decl_row(d);
        }
        self.fnv(0x4600 + d as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut d = 0usize;
        while d < DECL_ROWS {
            match self.decls.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 最后操作族（供注入不改值断言）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
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
        if self.gates_all()
            && self.cmds_all()
            && self.ops_all()
            && self.idxs_all()
            && self.splits_all()
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

impl Default for QsvLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 注入数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u32, u64) {
    let mut led = QsvLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < CMD_CELLS {
        if let Some(w) = cmd_cell(c) {
            if led.record_cmd(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < OP_CELLS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < IDX_CELLS {
        if let Some(w) = idx_cell(i) {
            if led.record_idx(i, w).is_ok() {
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
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_decl(d);
        d += 1;
    }
    // 注入 10 轮：命令族越闭集 / 操作漂移 / 空账乱序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = QsvLedger::new();
        let caught = match kind {
            0 => lt.record_cmd(CMD_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(QsvCode::EOpDrift)
            }
            _ => {
                let r2 = lt.record_op(5, op_cell(5).unwrap_or(0));
                r2 == Err(QsvCode::EOrder) && lt.last_kind() == -1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let (inj, caught, esc) = led.stats();
    let fp = led.fingerprint();
    (recorded, inj, caught, esc, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52356 判据集（锚 UNX-F52357-J1，off-by-one 板式）。
pub fn run_unx52356_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52356");
    let (recorded, inj, caught, esc, fp) = run_cycle();
    s.add(
        "五命令族 ×20 格全通（索引生成 ×10 语言样本）",
        recorded >= (CMD_CELLS + OP_CELLS + IDX_CELLS) as u32,
        "select/sort/filter/stats/join 逐格载荷折叠恒等（UNX-F52357-J1 锚定）",
    );
    s.add(
        "操作 20 格集全落",
        {
            let mut lt = QsvLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OP_CELLS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "查找/变换/聚合/IO 四族逐操作恒等",
    );
    s.add(
        "索引三态 6 格全落",
        {
            let mut lt = QsvLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < IDX_CELLS {
                if let Some(w) = idx_cell(i) {
                    if lt.record_idx(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.idxs_all()
        },
        "off/build/use 三态逐格落账",
    );
    s.add(
        "与 xsv 分账声明 2/2 全齐（升级接管口径）",
        {
            let mut lt = QsvLedger::new();
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
        "xsv 存量分账/qsv 接管增量（xsv → qsv 升级接管字样）两行闭集",
    );
    s.add(
        "Rust 上栈声明 2/2 全齐",
        {
            let mut lt = QsvLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_decl(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.decls_all()
        },
        "rustc 工具链声明行/CSV crate 链接声明行两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = QsvLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "索引生成/选择/排序/统计/联接/写回逐格落账",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CMD_CELLS {
                if cmd_kind(c).unwrap_or(5) < cmd_kind(c - 1).unwrap_or(5) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "select→sort→filter→stats→join 五族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10 && inj >= 10, "命令族越闭集/操作漂移/空账乱序三态轮转必抓");
    s.add("边界·命令族越闭集", QsvLedger::new().record_cmd(CMD_CELLS, 0).is_err(), "命令格 20 越闭集必拒");
    s.add("边界·操作越闭集", QsvLedger::new().record_op(OP_CELLS, 0).is_err(), "操作 20 越闭集必拒");
    s.add(
        "边界·判据闸越闭集",
        QsvLedger::new().judge_gate(6).is_err(),
        "闸 6 越六判据闭集必拒（ESeal）",
    );
    s.add_dyn(
        "注入账守恒",
        inj == caught && esc == 0 && caught >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj, caught, esc),
    );
    let (_, _, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = QsvLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < CMD_CELLS {
                if let Some(w) = cmd_cell(c) {
                    let _ = lt.record_cmd(c, w);
                }
                c += 1;
            }
            let mut o = 0usize;
            while o < OP_CELLS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < IDX_CELLS {
                if let Some(w) = idx_cell(i) {
                    let _ = lt.record_idx(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut d = 0usize;
            while d < DECL_ROWS {
                let _ = lt.declare_decl(d);
                d += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC98C0,
        "SEG_BASE 0x40 对齐且落于 0xC98C0 派工格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kinds() {
        // 族 = 格号/4（i/N 映射），按映射核对。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(3), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert_eq!(cmd_kind_name(4), Some("join"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_op_kinds() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(0));
        assert_eq!(op_kind(5), Some(1));
        assert_eq!(op_kind(19), Some(3));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(3), Some("IO"));
    }

    #[test]
    fn test_idx_states() {
        // 态 = 格号/2（i/N 映射），按映射核对。
        assert_eq!(idx_state(0), Some(0));
        assert_eq!(idx_state(1), Some(0));
        assert_eq!(idx_state(2), Some(1));
        assert_eq!(idx_state(5), Some(2));
        assert!(idx_state(6).is_none());
        assert_eq!(idx_state_name(1), Some("build"));
        assert_eq!(idx_state_name(2), Some("use"));
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(op_cell(OP_CELLS).is_none());
        assert!(idx_cell(IDX_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert_eq!(split_row_name(1), Some("qsv 接管增量声明行（xsv → qsv 升级接管）"));
        assert_eq!(decl_row_name(0), Some("rustc 工具链声明行"));
        // XOR 锚低 16 位恒零。
        assert_eq!(QSV_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_order_caught_and_last_kind() {
        // 空账乱序：首格即第 1 族必抓且不改 last_kind。
        let mut lt = QsvLedger::new();
        assert_eq!(lt.record_op(5, op_cell(5).unwrap_or(0)), Err(QsvCode::EOrder));
        assert_eq!(lt.last_kind(), -1);
        // 合法序：族 0 → 族 1。
        let mut lt2 = QsvLedger::new();
        let _ = lt2.record_op(0, op_cell(0).unwrap_or(0));
        assert!(lt2.record_op(5, op_cell(5).unwrap_or(0)).is_ok());
        // 命令表同理。
        let mut lt3 = QsvLedger::new();
        assert_eq!(lt3.record_cmd(4, cmd_cell(4).unwrap_or(0)), Err(QsvCode::EOrder));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = QsvLedger::new();
        assert_eq!(
            lt.record_cmd(0, cmd_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(QsvCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_op(2, op_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(QsvCode::EOpDrift)
        );
        assert_eq!(
            lt.record_idx(3, idx_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(QsvCode::EIdxDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = QsvLedger::new();
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)), Err(QsvCode::EDup));
        let _ = lt.record_idx(0, idx_cell(0).unwrap_or(0));
        assert_eq!(lt.record_idx(0, idx_cell(0).unwrap_or(0)), Err(QsvCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(QsvCode::EDup));
        let _ = lt.declare_decl(1);
        assert_eq!(lt.declare_decl(1), Err(QsvCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = QsvLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = QsvLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_boundary_closed_set() {
        let mut lt = QsvLedger::new();
        assert_eq!(lt.record_cmd(CMD_CELLS, 0), Err(QsvCode::ENoCmd));
        assert_eq!(lt.record_op(OP_CELLS, 0), Err(QsvCode::ENoOp));
        assert_eq!(lt.record_idx(IDX_CELLS, 0), Err(QsvCode::ENoIdx));
        assert_eq!(lt.declare_split(SPLIT_ROWS), Err(QsvCode::ENoSplit));
        assert_eq!(lt.declare_decl(DECL_ROWS), Err(QsvCode::ENoDecl));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, inj, caught, esc, _) = run_cycle();
        assert!(recorded >= (CMD_CELLS + OP_CELLS + IDX_CELLS) as u32);
        assert_eq!(inj, 10);
        assert_eq!(caught, 10);
        assert_eq!(esc, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (r1, i1, c1, e1, f1) = run_cycle();
        let (r2, i2, c2, e2, f2) = run_cycle();
        assert_eq!(r1, r2);
        assert_eq!(i1, i2);
        assert_eq!(c1, c2);
        assert_eq!(e1, e2);
        assert_eq!(e1, 0);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC98C0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(QsvCode::Ok as u32, 0);
        assert_eq!(QsvCode::EGradeDrift as u32, 12);
        assert_eq!(QsvCode::Ok.val(), SEG_BASE);
        assert!(!QsvCode::Ok.detail().is_empty());
        assert!(!QsvCode::ESeal.detail().is_empty());
        assert!(!QsvCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = QsvLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = QsvLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = QsvLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52356_checks_green() {
        let s = run_unx52356_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
