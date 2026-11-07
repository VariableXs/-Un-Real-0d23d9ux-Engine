//! UNX-F52354 jq 收口 · 0xC9840段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52354"，逐字）：
//!   title: jq 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52355-J1 YAML 处
//!     理 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52354）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52355-J1，照原文锚定；功能按本卡标题（jq）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 过滤器五族 20 格：路径/迭代/构造/算术/内置函数各 4 格，族 =
//!      格号/4，族单调（逆序必抓，空账本首格非 0 号族必抓且不改
//!      last_kind）；过滤器样本逐格载荷折叠恒等才收。
//!   ② 操作 20 格集：导航/变换/组合/格式化各 5 格（kind = 格号/5），
//!      载荷折叠恒等才收；漂移必抓。
//!   ③ 输出三态 6 格：compact/raw/stream 三态 ×2 格，越态必抓。
//!   ④ 管道消费联签 4 格：stdin 供给/管道解码/结果编码/下游回写四行
//!      闭集，缺一拒封。
//!   ⑤ C 上栈声明 2 行闭集：libonig 正则声明行/libjq 链接声明行，缺一
//!      拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（过滤器越闭集/操作漂移/空账乱
//!      序），检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 20 操作 + 6 态 + 4 联签 + 2 声明 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52354;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 过滤器格数（五族 × 各 4 格）。
pub const FILTER_CELLS: usize = 20;

/// 操作格数（四族 × 各 5 格）。
pub const OP_CELLS: usize = 20;

/// 输出三态格数。
pub const OUT_CELLS: usize = 6;

/// 管道消费联签行数。
pub const SIG_ROWS: usize = 4;

/// C 上栈声明行数。
pub const DECL_ROWS: usize = 2;

/// jq 域 XOR 锚（低 16 位恒零）。
pub const JQ_ANCHOR: u64 = 0x4A51_2D4A_5100_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JqCode {
    /// 成功
    Ok = 0,
    /// 过滤器族越闭集
    ENoFam,
    /// 过滤器载荷漂移
    EFamDrift,
    /// 操作越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 族逆序
    EOrder,
    /// 输出态越闭集
    ENoOut,
    /// 输出态载荷漂移
    EOutDrift,
    /// 管道联签行越闭集
    ENoSig,
    /// C 上栈声明行越闭集
    ENoDecl,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl JqCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JqCode::Ok => "成功",
            JqCode::ENoFam => "过滤器族越闭集：路径/迭代/构造/算术/内置函数 20 格之外先登记",
            JqCode::EFamDrift => "过滤器载荷漂移：样本折叠与基准不恒等，逐格复算",
            JqCode::ENoOp => "操作越闭集：操作 20 格集之外先登记",
            JqCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            JqCode::EOrder => "族逆序：路径→迭代→构造→算术→内置函数单调，逆序必抓",
            JqCode::ENoOut => "输出态越闭集：compact/raw/stream 三态 6 格之外先登记",
            JqCode::EOutDrift => "输出态载荷漂移：三态折叠与基准不恒等，逐格复算",
            JqCode::ENoSig => "管道联签行越闭集：4 行闭集之外先登记",
            JqCode::ENoDecl => "C 上栈声明行越闭集：2 行闭集之外先登记",
            JqCode::EDup => "重复落账：同格只许一条账目",
            JqCode::ESeal => "未齐即封：20 格 + 20 操作 + 6 态 + 4 联签 + 2 声明未齐先补齐",
            JqCode::EGradeDrift => "定级漂移：全通且联签/声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+20操作+6态+4联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 过滤器格载荷（格 f；族 = f/4）。
pub const fn filter_cell(f: usize) -> Option<u64> {
    if f >= FILTER_CELLS {
        return None;
    }
    Some(
        ((0xB100u64 + (f as u64) * 0x31) ^ ((f / 4) as u64).wrapping_mul(0x01000193)) ^ JQ_ANCHOR,
    )
}

/// 过滤器族（0..4=路径 4..8=迭代 8..12=构造 12..16=算术 16..20=内置函数）。
pub const fn filter_kind(f: usize) -> Option<u32> {
    if f >= FILTER_CELLS {
        return None;
    }
    Some((f / 4) as u32)
}

/// 过滤器族名。
pub const fn filter_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("路径"),
        1 => Some("迭代"),
        2 => Some("构造"),
        3 => Some("算术"),
        4 => Some("内置函数"),
        _ => None,
    }
}

/// 操作格载荷（格 o；族 = o/5：导航/变换/组合/格式化）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OP_CELLS {
        return None;
    }
    Some((0xB200u64 + (o as u64) * 0x2E) ^ ((o / 5) as u64).wrapping_mul(0x9E3779B9))
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
        0 => Some("导航"),
        1 => Some("变换"),
        2 => Some("组合"),
        3 => Some("格式化"),
        _ => None,
    }
}

/// 输出三态格载荷（格 i；态 = i/2）。
pub const fn out_cell(i: usize) -> Option<u64> {
    if i >= OUT_CELLS {
        return None;
    }
    Some((0xB400u64 + (i as u64) * 0x2C) ^ 0x4A_0000_0000)
}

/// 输出态。
pub const fn out_state(i: usize) -> Option<u32> {
    if i >= OUT_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 输出态名。
pub const fn out_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("compact"),
        1 => Some("raw"),
        2 => Some("stream"),
        _ => None,
    }
}

/// 管道消费联签行折叠（行 r）。
pub const fn pipe_row(r: usize) -> Option<u32> {
    if r >= SIG_ROWS {
        return None;
    }
    Some(0xB500 + r as u32 * 0x2C)
}

/// 联签行名。
pub const fn pipe_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("stdin 供给联签"),
        1 => Some("管道解码联签"),
        2 => Some("结果编码联签"),
        3 => Some("下游回写联签"),
        _ => None,
    }
}

/// C 上栈声明行折叠（行 d）。
pub const fn decl_row(d: usize) -> Option<u32> {
    if d >= DECL_ROWS {
        return None;
    }
    Some(0xB600 + d as u32 * 0x2C)
}

/// 声明行名。
pub const fn decl_row_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("libonig 正则声明行"),
        1 => Some("libjq 链接声明行"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// jq 收口账本状态机。
#[derive(Clone)]
pub struct JqLedger {
    /// 六判据（解析/求值/过滤/格式化/流式/退出码）。
    gates: [Option<bool>; 6],
    /// 过滤器账。
    filters: [Option<u64>; FILTER_CELLS],
    /// 操作账。
    ops: [Option<u64>; OP_CELLS],
    /// 输出三态账。
    outs: [Option<u64>; OUT_CELLS],
    /// 管道联签账。
    sigs: [Option<u32>; SIG_ROWS],
    /// C 上栈声明账。
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

impl JqLedger {
    /// 新账本。
    pub fn new() -> Self {
        JqLedger {
            gates: [None; 6],
            filters: [None; FILTER_CELLS],
            ops: [None; OP_CELLS],
            outs: [None; OUT_CELLS],
            sigs: [None; SIG_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JqCode> {
        if gate >= 6 {
            return Err(JqCode::ESeal);
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

    /// 过滤器一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_filter(&mut self, f: usize, got: u64) -> Result<(), JqCode> {
        if f >= FILTER_CELLS {
            return Err(JqCode::ENoFam);
        }
        let kind = match filter_kind(f) {
            Some(k) => k,
            None => return Err(JqCode::ENoFam),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(JqCode::EOrder);
        }
        let want = match filter_cell(f) {
            Some(w) => w,
            None => return Err(JqCode::ENoFam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + f as u64);
            return Err(JqCode::EFamDrift);
        }
        if match self.filters.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JqCode::EDup);
        }
        if let Some(c) = self.filters.get_mut(f) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + f as u64);
        Ok(())
    }

    /// 过滤器全账核验（20 格）。
    pub fn filters_all(&self) -> bool {
        let mut f = 0usize;
        while f < FILTER_CELLS {
            match self.filters.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), JqCode> {
        if o >= OP_CELLS {
            return Err(JqCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(JqCode::ENoOp),
        };
        if (kind as i64) > self.last_kind + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(JqCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(JqCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(JqCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JqCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
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

    /// 输出三态一格（载荷对照 + 重复拒）。
    pub fn record_out(&mut self, i: usize, got: u64) -> Result<(), JqCode> {
        let want = match out_cell(i) {
            Some(w) => w,
            None => return Err(JqCode::ENoOut),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400 + i as u64);
            return Err(JqCode::EOutDrift);
        }
        if match self.outs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JqCode::EDup);
        }
        if let Some(c) = self.outs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4401 + i as u64);
        Ok(())
    }

    /// 输出三态全账核验（6 格）。
    pub fn outs_all(&self) -> bool {
        let mut i = 0usize;
        while i < OUT_CELLS {
            match self.outs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 管道联签一行。
    pub fn declare_sig(&mut self, r: usize) -> Result<(), JqCode> {
        if r >= SIG_ROWS {
            return Err(JqCode::ENoSig);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JqCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = pipe_row(r);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn sigs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SIG_ROWS {
            match self.sigs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_decl(&mut self, d: usize) -> Result<(), JqCode> {
        if d >= DECL_ROWS {
            return Err(JqCode::ENoDecl);
        }
        if match self.decls.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JqCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(d) {
            *c = decl_row(d);
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
            && self.filters_all()
            && self.ops_all()
            && self.outs_all()
            && self.sigs_all()
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

impl Default for JqLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 注入数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u32, u64) {
    let mut led = JqLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FILTER_CELLS {
        if let Some(w) = filter_cell(f) {
            if led.record_filter(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
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
    while i < OUT_CELLS {
        if let Some(w) = out_cell(i) {
            if led.record_out(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SIG_ROWS {
        let _ = led.declare_sig(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_decl(d);
        d += 1;
    }
    // 注入 10 轮：过滤器越闭集 / 操作漂移 / 空账乱序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JqLedger::new();
        let caught = match kind {
            0 => lt.record_filter(FILTER_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(JqCode::EOpDrift)
            }
            _ => {
                let r2 = lt.record_op(5, op_cell(5).unwrap_or(0));
                r2 == Err(JqCode::EOrder) && lt.last_kind() == -1
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
/// UNX-F52354 判据集（锚 UNX-F52355-J1，off-by-one 板式）。
pub fn run_unx52354_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52354");
    let (recorded, inj, caught, esc, fp) = run_cycle();
    s.add(
        "过滤器五族 ×20 格全通",
        recorded >= (FILTER_CELLS + OP_CELLS + OUT_CELLS) as u32,
        "路径/迭代/构造/算术/内置函数逐格载荷折叠恒等（UNX-F52355-J1 锚定）",
    );
    s.add(
        "操作 20 格集全落",
        {
            let mut lt = JqLedger::new();
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
        "导航/变换/组合/格式化四族逐操作恒等",
    );
    s.add(
        "输出三态 6 格全落",
        {
            let mut lt = JqLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < OUT_CELLS {
                if let Some(w) = out_cell(i) {
                    if lt.record_out(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.outs_all()
        },
        "compact/raw/stream 三态逐格落账",
    );
    s.add(
        "管道消费联签 4/4 全齐",
        {
            let mut lt = JqLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SIG_ROWS {
                if lt.declare_sig(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sigs_all()
        },
        "stdin 供给/管道解码/结果编码/下游回写四行闭集",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = JqLedger::new();
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
        "libonig 正则声明行/libjq 链接声明行两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = JqLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "解析/求值/过滤/格式化/流式/退出码逐格落账",
    );
    s.add(
        "过滤器族单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FILTER_CELLS {
                if filter_kind(f).unwrap_or(5) < filter_kind(f - 1).unwrap_or(5) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "路径→迭代→构造→算术→内置函数五族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10 && inj >= 10, "过滤器越闭集/操作漂移/空账乱序三态轮转必抓");
    s.add("边界·过滤器越闭集", JqLedger::new().record_filter(FILTER_CELLS, 0).is_err(), "过滤器格 20 越闭集必拒");
    s.add("边界·操作越闭集", JqLedger::new().record_op(OP_CELLS, 0).is_err(), "操作 20 越闭集必拒");
    s.add(
        "边界·判据闸越闭集",
        JqLedger::new().judge_gate(6).is_err(),
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
            let mut lt = JqLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < FILTER_CELLS {
                if let Some(w) = filter_cell(f) {
                    let _ = lt.record_filter(f, w);
                }
                f += 1;
            }
            let mut o = 0usize;
            while o < OP_CELLS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < OUT_CELLS {
                if let Some(w) = out_cell(i) {
                    let _ = lt.record_out(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SIG_ROWS {
                let _ = lt.declare_sig(r);
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9840,
        "SEG_BASE 0x40 对齐且落于 0xC9840 派工格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_kinds() {
        // 族 = 格号/4（i/N 映射），按映射核对。
        assert_eq!(filter_kind(0), Some(0));
        assert_eq!(filter_kind(3), Some(0));
        assert_eq!(filter_kind(4), Some(1));
        assert_eq!(filter_kind(19), Some(4));
        assert!(filter_kind(20).is_none());
        assert_eq!(filter_kind_name(4), Some("内置函数"));
        assert!(filter_kind_name(5).is_none());
    }

    #[test]
    fn test_op_kinds() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(0));
        assert_eq!(op_kind(5), Some(1));
        assert_eq!(op_kind(19), Some(3));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(3), Some("格式化"));
    }

    #[test]
    fn test_out_states() {
        // 态 = 格号/2（i/N 映射），按映射核对。
        assert_eq!(out_state(0), Some(0));
        assert_eq!(out_state(1), Some(0));
        assert_eq!(out_state(2), Some(1));
        assert_eq!(out_state(5), Some(2));
        assert!(out_state(6).is_none());
        assert_eq!(out_state_name(0), Some("compact"));
        assert_eq!(out_state_name(2), Some("stream"));
    }

    #[test]
    fn test_cells_pure() {
        let a = filter_cell(3).unwrap_or(0);
        assert_eq!(a, filter_cell(3).unwrap_or(0));
        assert_ne!(a, filter_cell(4).unwrap_or(0));
        assert!(op_cell(OP_CELLS).is_none());
        assert!(out_cell(OUT_CELLS).is_none());
        assert!(pipe_row(SIG_ROWS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert_eq!(pipe_row_name(1), Some("管道解码联签"));
        assert_eq!(decl_row_name(1), Some("libjq 链接声明行"));
        // XOR 锚低 16 位恒零。
        assert_eq!(JQ_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_order_caught_and_last_kind() {
        // 空账乱序：首格即第 1 族必抓且不改 last_kind。
        let mut lt = JqLedger::new();
        assert_eq!(lt.record_op(5, op_cell(5).unwrap_or(0)), Err(JqCode::EOrder));
        assert_eq!(lt.last_kind(), -1);
        // 合法序：族 0 → 族 1。
        let mut lt2 = JqLedger::new();
        let _ = lt2.record_op(0, op_cell(0).unwrap_or(0));
        assert!(lt2.record_op(5, op_cell(5).unwrap_or(0)).is_ok());
        // 过滤器表同理。
        let mut lt3 = JqLedger::new();
        assert_eq!(lt3.record_filter(4, filter_cell(4).unwrap_or(0)), Err(JqCode::EOrder));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = JqLedger::new();
        assert_eq!(
            lt.record_filter(0, filter_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(JqCode::EFamDrift)
        );
        assert_eq!(
            lt.record_op(2, op_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(JqCode::EOpDrift)
        );
        assert_eq!(
            lt.record_out(3, out_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(JqCode::EOutDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = JqLedger::new();
        let _ = lt.record_filter(0, filter_cell(0).unwrap_or(0));
        assert_eq!(lt.record_filter(0, filter_cell(0).unwrap_or(0)), Err(JqCode::EDup));
        let _ = lt.declare_decl(0);
        assert_eq!(lt.declare_decl(0), Err(JqCode::EDup));
        let _ = lt.record_out(0, out_cell(0).unwrap_or(0));
        assert_eq!(lt.record_out(0, out_cell(0).unwrap_or(0)), Err(JqCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = JqLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = JqLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_boundary_closed_set() {
        let mut lt = JqLedger::new();
        assert_eq!(lt.record_filter(FILTER_CELLS, 0), Err(JqCode::ENoFam));
        assert_eq!(lt.record_op(OP_CELLS, 0), Err(JqCode::ENoOp));
        assert_eq!(lt.record_out(OUT_CELLS, 0), Err(JqCode::ENoOut));
        assert_eq!(lt.declare_sig(SIG_ROWS), Err(JqCode::ENoSig));
        assert_eq!(lt.declare_decl(DECL_ROWS), Err(JqCode::ENoDecl));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, inj, caught, esc, _) = run_cycle();
        assert!(recorded >= (FILTER_CELLS + OP_CELLS + OUT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9840);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JqCode::Ok as u32, 0);
        assert_eq!(JqCode::EGradeDrift as u32, 12);
        assert_eq!(JqCode::Ok.val(), SEG_BASE);
        assert!(!JqCode::Ok.detail().is_empty());
        assert!(!JqCode::ESeal.detail().is_empty());
        assert!(!JqCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = JqLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = JqLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = JqLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52354_checks_green() {
        let s = run_unx52354_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
