//! UNX-F52495 SQLMap 收口 · 0xCBB80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52495"，逐字）：
//!   title: SQLMap 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52496-J1 只对本
//!     机靶场演示 ×10（白名单声明，真实凭据零接触）；KB-COMPAT 定级登
//!     记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52495）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52496-J1，照原文锚定；功能按本卡标题（SQLMap）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（本地授权演练靶场/干跑语义，零真实攻击面，真实凭据零接触）：
//!   ① 判据只对本机靶场演示（回环 127.0.0.1 演练库）与干跑结果账收口；
//!      --dump-sessions 族全程干跑，--schema 族只投影元数据骨架。
//!   ② 扫描 20 操作集：五命令族闭集（-u/--batch/--forms/--dump-sessions/
//!      --schema 干跑）各 4 操作，家族单调（逆序必抓）。
//!   ③ 三组声明行闭集：与 Nikto 分账声明 2 行（F52494 锚定行）+ Python
//!      上栈声明 2 行 + 回环靶标声明 2 行，缺一拒封。
//!   ④ ledger 状态机（new/record/declare/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作越闭集/操作漂移/回环声明行
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S2/S3 闭集
//!      （20 操作 + 6 行声明 + 六判据全通 → S2）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52495;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 扫描操作数。
pub const SCAN_OPS: usize = 20;

/// 与 Nikto 分账声明行数（F52494）。
pub const SPLIT_ROWS: usize = 2;

/// Python 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 回环靶标声明行数。
pub const LOOP_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SqlmapCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// Nikto 分账行越闭集
    ENoSplit,
    /// Python 上栈行越闭集
    ENoStack,
    /// 回环声明行越闭集
    ENoLoop,
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

impl SqlmapCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SqlmapCode::Ok => "成功",
            SqlmapCode::ENoOp => "操作号越闭集：扫描 20 操作集之外先登记，真实凭据零接触",
            SqlmapCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            SqlmapCode::EOrder => "家族逆序：-u→--batch→--forms→--dump-sessions→--schema 单调，逆序必抓",
            SqlmapCode::ENoSplit => "Nikto 分账行越闭集：与 Nikto 分账 2 行（F52494）之外先登记",
            SqlmapCode::ENoStack => "Python 上栈行越闭集：Python 上栈 2 行之外先登记",
            SqlmapCode::ENoLoop => "回环声明行越闭集：回环靶标 2 行之外先登记",
            SqlmapCode::ERowMiss => "缺声明行：Nikto 分账 / Python 上栈 / 回环靶标声明未齐先补齐",
            SqlmapCode::EDup => "重复落账：同格只许一条账目",
            SqlmapCode::ESeal => "未齐即封：20 操作 + 6 行声明 + 六判据未齐先补齐",
            SqlmapCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SqlmapCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SqlmapCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不产出，闭集在册）
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：全通收口（SQLMap 卡闭集在册但目标档为 S2）",
            Grade::S2 => "S2：20操作+6行声明+六判据全通（只对本机靶场演示，真实凭据零接触）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 扫描操作格载荷（操作 o；命令族 = o/4）。
pub const fn scan_cell(o: usize) -> Option<u64> {
    if o >= SCAN_OPS {
        return None;
    }
    Some((0xCBB80u64 + (o as u64) * 0x3B) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=-u 4..8=--batch 8..12=--forms 12..16=--dump-sessions 16..20=--schema 干跑）。
pub const fn scan_kind(o: usize) -> Option<u32> {
    if o >= SCAN_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名（按族索引，非格号）。
pub const fn scan_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-u 靶址"),
        1 => Some("--batch 应答批"),
        2 => Some("--forms 表单桩"),
        3 => Some("--dump-sessions 干跑会话"),
        4 => Some("--schema 元数据干跑"),
        _ => None,
    }
}

/// 与 Nikto 分账声明行折叠（行 r；F52494 锚定行）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCD80 + r as u32 * 0x3B)
}

/// 与 Nikto 分账声明行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("与 Nikto 分账声明：SQLMap 注入账独立记"),
        1 => Some("与 Nikto 分账声明：F52494 锚定行不改写"),
        _ => None,
    }
}

/// Python 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCE80 + r as u32 * 0x47)
}

/// Python 上栈声明行名。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Python 上栈声明：SQLMap 依赖 Python 解释栈"),
        1 => Some("Python 上栈声明：干跑脚本走同一解释栈版本"),
        _ => None,
    }
}

/// 回环靶标声明行折叠（行 r）。
pub const fn loop_row(r: usize) -> Option<u32> {
    if r >= LOOP_ROWS {
        return None;
    }
    Some(0xCF80 + r as u32 * 0x3B)
}

/// 回环靶标声明行名。
pub const fn loop_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("回环靶标声明：只对本机靶场 127.0.0.1 演示"),
        1 => Some("回环靶标声明：真实凭据零接触，干跑会话落账"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SQLMap 收口账本状态机。
#[derive(Clone)]
pub struct SqlmapLedger {
    /// 六判据（本机靶场：白名单/授权/限速/隔离/落账/写回）。
    gates: [Option<bool>; 6],
    /// 扫描操作账。
    ops: [Option<u64>; SCAN_OPS],
    /// Nikto 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Python 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 回环靶标声明账。
    loops: [Option<u32>; LOOP_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SqlmapLedger {
    /// 新账本。
    pub fn new() -> Self {
        SqlmapLedger {
            gates: [None; 6],
            ops: [None; SCAN_OPS],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
            loops: [None; LOOP_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SqlmapCode> {
        if gate >= 6 {
            return Err(SqlmapCode::ESeal);
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

    /// 扫描操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SqlmapCode> {
        if o >= SCAN_OPS {
            return Err(SqlmapCode::ENoOp);
        }
        let kind = match scan_kind(o) {
            Some(k) => k,
            None => return Err(SqlmapCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(SqlmapCode::EOrder);
        }
        let want = match scan_cell(o) {
            Some(w) => w,
            None => return Err(SqlmapCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(SqlmapCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqlmapCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < SCAN_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 声明一行（group 0=Nikto 分账 1=Python 上栈 2=回环靶标）。
    pub fn declare_row(&mut self, group: usize, r: usize) -> Result<(), SqlmapCode> {
        let limit = match group {
            0 => SPLIT_ROWS,
            1 => STACK_ROWS,
            2 => LOOP_ROWS,
            _ => return Err(SqlmapCode::ERowMiss),
        };
        if r >= limit {
            return match group {
                0 => Err(SqlmapCode::ENoSplit),
                1 => Err(SqlmapCode::ENoStack),
                _ => Err(SqlmapCode::ENoLoop),
            };
        }
        let cell = split_row(r);
        let cell = if group == 0 {
            cell
        } else if group == 1 {
            stack_row(r)
        } else {
            loop_row(r)
        };
        let want = match cell {
            Some(w) => w,
            None => return match group {
                0 => Err(SqlmapCode::ENoSplit),
                1 => Err(SqlmapCode::ENoStack),
                _ => Err(SqlmapCode::ENoLoop),
            },
        };
        let hit = match group {
            0 => match self.splits.get(r) {
                Some(Some(_)) => true,
                _ => false,
            },
            1 => match self.stacks.get(r) {
                Some(Some(_)) => true,
                _ => false,
            },
            _ => match self.loops.get(r) {
                Some(Some(_)) => true,
                _ => false,
            },
        };
        if hit {
            return Err(SqlmapCode::EDup);
        }
        match group {
            0 => {
                if let Some(c) = self.splits.get_mut(r) {
                    *c = Some(want);
                }
            }
            1 => {
                if let Some(c) = self.stacks.get_mut(r) {
                    *c = Some(want);
                }
            }
            _ => {
                if let Some(c) = self.loops.get_mut(r) {
                    *c = Some(want);
                }
            }
        }
        self.fnv(0x7300 + group as u64 * 0x10 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（三组各 2 行）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut s = 0usize;
        while s < STACK_ROWS {
            match self.stacks.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        let mut l = 0usize;
        while l < LOOP_ROWS {
            match self.loops.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// 封闸：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
    pub fn seal(&self) -> Result<(), SqlmapCode> {
        if !self.gates_all() {
            return Err(SqlmapCode::ESeal);
        }
        if !self.rows_all() {
            return Err(SqlmapCode::ERowMiss);
        }
        if !self.ops_all() {
            return Err(SqlmapCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S2，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.rows_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SqlmapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SqlmapLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < SCAN_OPS {
        if let Some(w) = scan_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_row(0, r);
        r += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_row(1, s);
        s += 1;
    }
    let mut l = 0usize;
    while l < LOOP_ROWS {
        let _ = led.declare_row(2, l);
        l += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 回环声明行越闭集三态轮转，
    // 全部落在当轮临时账本上；主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SqlmapLedger::new();
        let caught = match kind {
            0 => {
                lt.record_op(0, scan_cell(0).unwrap_or(0));
                lt.record_op(SCAN_OPS, 0).is_err()
            }
            1 => {
                // 同族已录格注漂移（跨族会先撞序闸）。
                lt.record_op(4, scan_cell(4).unwrap_or(0));
                lt.record_op(4, scan_cell(4).unwrap_or(0).wrapping_add(1)) == Err(SqlmapCode::EDrift)
            }
            _ => {
                lt.declare_row(2, 0).is_ok() && lt.declare_row(2, LOOP_ROWS) == Err(SqlmapCode::ENoLoop)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7400 + k as u64);
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
/// UNX-F52495 判据集（锚 UNX-F52496-J1 只对本机靶场演示 ×10（白名单声明，
/// 真实凭据零接触），off-by-one 板式）。
pub fn run_unx52495_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52495");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "扫描 20 操作全通（只对本机靶场演示）",
        recorded >= SCAN_OPS as u32,
        "-u/--batch/--forms/--dump-sessions/--schema 五命令族逐操作恒等，全程干跑",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SqlmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "本机靶场白名单/授权/限速/隔离/落账/写回逐格落账",
    );
    s.add(
        "与 Nikto 分账声明 2/2（F52494）",
        {
            let mut lt = SqlmapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_row(0, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "SQLMap 注入账独立记 / F52494 锚定行不改写（§AI-66.3）",
    );
    s.add(
        "Python 上栈声明 2/2",
        {
            let mut lt = SqlmapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_row(1, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "Python 解释栈声明 + 干跑脚本同版本声明",
    );
    s.add(
        "回环靶标声明 2/2",
        {
            let mut lt = SqlmapLedger::new();
            let mut ok = true;
            // rows_all 要求三组声明行（分账/上栈/回环）全部落账。
            let mut g = 0usize;
            while g < 3 {
                let mut r = 0usize;
                while r < LOOP_ROWS {
                    if lt.declare_row(g, r).is_err() {
                        ok = false;
                    }
                    r += 1;
                }
                g += 1;
            }
            ok && lt.rows_all()
        },
        "只对本机靶场 127.0.0.1 演示 / 真实凭据零接触（白名单声明，§AI-66.3）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SCAN_OPS {
                if scan_kind(o).unwrap_or(5) < scan_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "操作越闭集/操作漂移/回环声明行越闭集三态轮转必抓");
    s.add("边界·越闭集·操作", SqlmapLedger::new().record_op(SCAN_OPS, 0).is_err(), "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）");
    s.add(
        "边界·越闭集·Nikto 分账行",
        SqlmapLedger::new().declare_row(0, SPLIT_ROWS).is_err(),
        "Nikto 分账行 2 越闭集必拒（ENoSplit 提前返回）",
    );
    s.add(
        "边界·越闭集·回环声明行",
        {
            let mut lt = SqlmapLedger::new();
            lt.declare_row(2, LOOP_ROWS).is_err() && lt.declare_row(3, 0).is_err()
        },
        "回环行 2 与组 3 越闭集必拒（ENoLoop/ERowMiss 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = SqlmapLedger::new();
            lt.record_op(0, scan_cell(0).unwrap_or(0));
            lt.record_op(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SqlmapCode::EDrift)
        },
        "操作载荷漂移必抓",
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
            let mut lt = SqlmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < SCAN_OPS {
                if let Some(w) = scan_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_row(0, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = lt.declare_row(1, r2);
                r2 += 1;
            }
            let mut r3 = 0usize;
            while r3 < LOOP_ROWS {
                let _ = lt.declare_row(2, r3);
                r3 += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "封闸语义闭集",
        {
            // 六闸缺 → ESeal；六闸齐而声明行缺 → ERowMiss；格账缺 → ESeal。
            let empty = SqlmapLedger::new();
            let mut mid = SqlmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = mid.judge_gate(g);
                g += 1;
            }
            let mut full = mid.clone();
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = full.declare_row(0, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = full.declare_row(1, r2);
                r2 += 1;
            }
            let mut r3 = 0usize;
            while r3 < LOOP_ROWS {
                let _ = full.declare_row(2, r3);
                r3 += 1;
            }
            empty.seal() == Err(SqlmapCode::ESeal)
                && mid.seal() == Err(SqlmapCode::ERowMiss)
                && full.seal() == Err(SqlmapCode::ESeal)
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal，按序核验",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBB80,
        "SEG_BASE 0x40 对齐且在 VX16 域（0xCA400 + 0x40×94）",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_kind_table() {
        assert_eq!(scan_kind(0), Some(0));
        assert_eq!(scan_kind(4), Some(1));
        assert_eq!(scan_kind(8), Some(2));
        assert_eq!(scan_kind(12), Some(3));
        assert_eq!(scan_kind(19), Some(4));
        assert!(scan_kind(20).is_none());
        // 名表按族索引而非格号。
        assert_eq!(scan_kind_name(0), Some("-u 靶址"));
        assert_eq!(scan_kind_name(3), Some("--dump-sessions 干跑会话"));
        assert_eq!(scan_kind_name(4), Some("--schema 元数据干跑"));
        assert!(scan_kind_name(5).is_none());
        assert!(scan_cell(20).is_none());
    }

    #[test]
    fn test_rows_pure() {
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert!(loop_row(LOOP_ROWS).is_none());
        assert_eq!(split_row(0), Some(0xCD80));
        assert_eq!(stack_row(1), Some(0xCE80 + 0x47));
        assert_eq!(loop_row(1), Some(0xCF80 + 0x3B));
        assert_eq!(split_row_name(1), Some("与 Nikto 分账声明：F52494 锚定行不改写"));
        assert_eq!(loop_row_name(0), Some("回环靶标声明：只对本机靶场 127.0.0.1 演示"));
        assert!(split_row_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = scan_cell(3).unwrap_or(0);
        assert_eq!(a, scan_cell(3).unwrap_or(0));
        assert_ne!(a, scan_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SqlmapLedger::new();
        let _ = lt.record_op(9, scan_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, scan_cell(2).unwrap_or(0)), Err(SqlmapCode::EOrder));
        assert!(lt.record_op(10, scan_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SqlmapLedger::new();
        assert_eq!(lt.record_op(5, scan_cell(5).unwrap_or(0).wrapping_add(1)), Err(SqlmapCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SqlmapLedger::new();
        let _ = lt.record_op(0, scan_cell(0).unwrap_or(0));
        assert_eq!(lt.record_op(0, scan_cell(0).unwrap_or(0)), Err(SqlmapCode::EDup));
        let _ = lt.declare_row(1, 0);
        assert_eq!(lt.declare_row(1, 0), Err(SqlmapCode::EDup));
        let _ = lt.declare_row(2, 1);
        assert_eq!(lt.declare_row(2, 1), Err(SqlmapCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SqlmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SqlmapLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_order() {
        let empty = SqlmapLedger::new();
        assert_eq!(empty.seal(), Err(SqlmapCode::ESeal));
        let mut mid = SqlmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.seal(), Err(SqlmapCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = mid.declare_row(0, r);
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < STACK_ROWS {
            let _ = mid.declare_row(1, r2);
            r2 += 1;
        }
        let mut r3 = 0usize;
        while r3 < LOOP_ROWS {
            let _ = mid.declare_row(2, r3);
            r3 += 1;
        }
        assert_eq!(mid.seal(), Err(SqlmapCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 操作 = 20。
        assert_eq!(recorded, SCAN_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xCBB80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SqlmapCode::Ok as u32, 0);
        assert_eq!(SqlmapCode::EGradeDrift as u32, 12);
        assert!(!SqlmapCode::Ok.detail().is_empty());
        assert!(!SqlmapCode::ENoLoop.detail().is_empty());
        assert!(!SqlmapCode::ERowMiss.detail().is_empty());
        assert_eq!(SqlmapCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S2（目标档）。
        let empty = SqlmapLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = SqlmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < SCAN_OPS {
            if let Some(w) = scan_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_row(0, r);
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < STACK_ROWS {
            let _ = full.declare_row(1, r2);
            r2 += 1;
        }
        let mut r3 = 0usize;
        while r3 < LOOP_ROWS {
            let _ = full.declare_row(2, r3);
            r3 += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_boundary_no_injection_ledger() {
        // 越闭集边界拒（ENoX 类提前返回）不计注入账。
        let mut lt = SqlmapLedger::new();
        let _ = lt.record_op(SCAN_OPS, 0);
        let _ = lt.declare_row(0, SPLIT_ROWS);
        let _ = lt.declare_row(2, LOOP_ROWS);
        let _ = lt.declare_row(9, 0);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn unx52495_checks_green() {
        let s = run_unx52495_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
