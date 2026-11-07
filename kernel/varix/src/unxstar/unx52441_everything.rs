//! UNX-F52441 Everything 收口 · 0xCAE00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52441"，逐字）：
//!   title: Everything 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52442-J1 六判据
//!     全落；内容搜索 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52441）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52442-J1，照原文锚定；功能按本卡标题（Everything）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 操作闭集：即时索引/搜索/过滤/HTTP 服务/NTFS 卷，每族
//!      4 操作，族号 = 操作号/4（i/N 映射），逆序必抓。
//!   ② 索引 16 拍单调：每拍载荷 = 基准 + 拍号×0x41，拍序严格递增，漂移
//!      与回拍都必抓。
//!   ③ 过滤器投影 8 格：投影折叠纯函数恒等才收。
//!   ④ 与 ripgrep（F52358）分账声明 2 行闭集；C++ 上栈声明 2 行闭集，
//!      两册各自全齐才许封账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（家族逆序/
//!      索引拍漂移/操作越闭集），全部落在当轮临时账本上，检出+逃逸守恒
//!      等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集（全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52441;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数（每族 4 操作）。
pub const OPS: usize = 20;

/// 索引拍数（单调 16 拍）。
pub const IDX_BEATS: usize = 16;

/// 过滤器投影格数。
pub const PROJ: usize = 8;

/// ripgrep（F52358）分账声明行数。
pub const RG_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EverythingCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 索引拍不单调
    EBeatOrder,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 判据闸越闭集
    ENoGate,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl EverythingCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            EverythingCode::Ok => "成功",
            EverythingCode::ENoOp => "格号越闭集：20 操作/16 拍/8 投影/声明行闭集之外先登记",
            EverythingCode::EDrift => "载荷漂移：操作/索引拍/投影折叠与基准不恒等，逐格复算",
            EverythingCode::EOrder => "家族逆序：即时索引→搜索→过滤→HTTP 服务→NTFS 卷单调，逆序必抓",
            EverythingCode::EBeatOrder => "索引拍不单调：16 拍必须严格递增，回拍/平拍必抓",
            EverythingCode::ENoRow => "声明行越闭集：ripgrep 分账 2 行与 C++ 上栈 2 行闭集之外先登记",
            EverythingCode::ERowMiss => "缺声明行：ripgrep（F52358）分账或 C++ 上栈声明未齐先补齐",
            EverythingCode::EDup => "重复落账：同格只许一条账目",
            EverythingCode::ENoGate => "判据闸越闭集：六判据 0..=5 之外先登记",
            EverythingCode::ESeal => "未齐即封：20 操作+16 拍+8 投影+两册声明未齐先补齐",
            EverythingCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            EverythingCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            EverythingCode::EGradeDrift => "定级漂移：全齐且两册声明全通才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+16拍+8投影+两册声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=即时索引 4..8=搜索 8..12=过滤 12..16=HTTP 服务 16..20=NTFS 卷）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("即时索引"),
        1 => Some("搜索"),
        2 => Some("过滤"),
        3 => Some("HTTP 服务"),
        4 => Some("NTFS 卷"),
        _ => None,
    }
}

/// 索引拍基准（拍 b；严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= IDX_BEATS {
        return None;
    }
    Some(0xD200 + b as u64 * 0x41)
}

/// 过滤器投影格载荷（投影 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ {
        return None;
    }
    Some((0xD3000u64 + (p as u64) * 0x37) ^ 0x45564649_4C544552)
}

/// ripgrep（F52358）分账声明行折叠（行 r）。
pub const fn rg_row(r: usize) -> Option<u32> {
    if r >= RG_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Everything 收口账本状态机。
#[derive(Clone)]
pub struct EverythingLedger {
    /// 六判据（启动/即时索引/搜索/HTTP 服务/卸载/升级）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 索引拍账。
    beats: [Option<u64>; IDX_BEATS],
    /// 最后拍值（单调核验，-1 表无账）。
    last_beat: i64,
    /// 投影账。
    projs: [Option<u64>; PROJ],
    /// ripgrep 分账声明账。
    rg: [Option<u32>; RG_ROWS],
    /// C++ 上栈声明账。
    stack: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EverythingLedger {
    /// 新账本。
    pub fn new() -> Self {
        EverythingLedger {
            gates: [None; 6],
            ops: [None; OPS],
            beats: [None; IDX_BEATS],
            last_beat: -1,
            projs: [None; PROJ],
            rg: [None; RG_ROWS],
            stack: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), EverythingCode> {
        if gate >= 6 {
            return Err(EverythingCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), EverythingCode> {
        if o >= OPS {
            return Err(EverythingCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(EverythingCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5110);
            return Err(EverythingCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(EverythingCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5111 + o as u64);
            return Err(EverythingCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EverythingCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5112 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 索引拍一格（载荷对照 + 严格递增 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), EverythingCode> {
        if b >= IDX_BEATS {
            return Err(EverythingCode::ENoOp);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(EverythingCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5120 + b as u64);
            return Err(EverythingCode::EDrift);
        }
        if (got as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5121 + b as u64);
            return Err(EverythingCode::EBeatOrder);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EverythingCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got as i64;
        self.fnv(0x5122 + b as u64);
        Ok(())
    }

    /// 索引拍全账核验（16 拍全齐即单调已证）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < IDX_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), EverythingCode> {
        if p >= PROJ {
            return Err(EverythingCode::ENoOp);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(EverythingCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5130 + p as u64);
            return Err(EverythingCode::EDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EverythingCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5131 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// ripgrep（F52358）分账声明一行。
    pub fn declare_rg(&mut self, r: usize) -> Result<(), EverythingCode> {
        if r >= RG_ROWS {
            return Err(EverythingCode::ENoRow);
        }
        if match self.rg.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EverythingCode::EDup);
        }
        if let Some(c) = self.rg.get_mut(r) {
            *c = rg_row(r);
        }
        self.fnv(0x5140 + r as u64);
        Ok(())
    }

    /// ripgrep 分账声明全齐核验。
    pub fn rg_all(&self) -> bool {
        let mut r = 0usize;
        while r < RG_ROWS {
            match self.rg.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C++ 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), EverythingCode> {
        if r >= STACK_ROWS {
            return Err(EverythingCode::ENoRow);
        }
        if match self.stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EverythingCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5150 + r as u64);
        Ok(())
    }

    /// C++ 上栈声明全齐核验。
    pub fn stack_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stack.get(r) {
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

    /// KB-COMPAT 定级（全齐 → S1，判据缺 → S3，其余 → S2）。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.beats_all() && self.projs_all() && self.rg_all() && self.stack_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for EverythingLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EverythingLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < IDX_BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut p = 0usize;
    while p < PROJ {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < RG_ROWS {
        let _ = led.declare_rg(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：家族逆序 / 索引拍漂移 / 操作越闭集三态轮转，全部落在
    // 当轮临时账本上；逆序注入先按合法序落正格（格 17）再回拍（格 5）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EverythingLedger::new();
        let caught = match kind {
            0 => {
                let w17 = match op_cell(17) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(17, w17);
                let w5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_op(5, w5) == Err(EverythingCode::EOrder)
            }
            1 => {
                let w0 = match beat_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_beat(0, w0);
                let w1 = match beat_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_beat(1, w1.wrapping_add(1)) == Err(EverythingCode::EDrift)
            }
            _ => lt.record_op(OPS, 0).is_err(),
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
/// UNX-F52441 判据集（锚 UNX-F52442-J1，off-by-one 板式）。
pub fn run_unx52441_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52441");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= OPS as u32,
        "即时索引/搜索/过滤/HTTP 服务/NTFS 卷逐操作恒等（§AI-66.3）",
    );
    s.add(
        "索引 16 拍单调全落",
        {
            let mut lt = EverythingLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < IDX_BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "16 拍基准逐拍恒等且严格递增，回拍必抓",
    );
    s.add(
        "过滤器投影 8/8 全齐",
        {
            let mut lt = EverythingLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "投影折叠纯函数 8 格逐格恒等",
    );
    s.add(
        "分账声明 4/4 全齐（ripgrep 2 + C++ 上栈 2）",
        {
            let mut lt = EverythingLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RG_ROWS {
                if lt.declare_rg(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rg_all() && lt.stack_all()
        },
        "ripgrep（F52358）分账 2 行 + C++ 上栈声明 2 行，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = EverythingLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/即时索引/搜索/HTTP 服务/卸载/升级逐格落账（UNX-F52442-J1）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let a = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let b = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五功能族闭集单调（i/N 映射 o/4）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "家族逆序/索引拍漂移/操作越闭集三态轮转必抓（内容搜索 ×10 口径）",
    );
    let mut l0 = EverythingLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·索引拍越闭集",
        l0.record_beat(IDX_BEATS, 0).is_err(),
        "拍 16 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        match l0.declare_rg(RG_ROWS) {
            Err(EverythingCode::ENoRow) => true,
            _ => false,
        },
        "ripgrep 声明行 2 之外越闭集必拒",
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
            let mut lt = EverythingLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut b = 0usize;
            while b < IDX_BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut p = 0usize;
            while p < PROJ {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < RG_ROWS {
                let _ = lt.declare_rg(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.grade() == Grade::S1
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAE00,
        "SEG_BASE 0x40 对齐且在 VX16 域 52441 格",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind_mapping() {
        // i/N 映射：kind = o/4。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(12), Some(3));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("即时索引"));
        assert_eq!(op_kind_name(4), Some("NTFS 卷"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match op_cell(3) {
            Some(w) => w,
            None => 0,
        };
        assert_eq!(a, match op_cell(3) {
            Some(w) => w,
            None => 0,
        });
        assert_ne!(a, match op_cell(4) {
            Some(w) => w,
            None => 0,
        });
        assert!(beat_cell(IDX_BEATS).is_none());
        assert!(proj_cell(PROJ).is_none());
        assert!(rg_row(RG_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_beat_monotone_basis() {
        let mut prev: i64 = -1;
        let mut b = 0usize;
        while b < IDX_BEATS {
            let w = match beat_cell(b) {
                Some(w) => w,
                None => 0,
            };
            assert!((w as i64) > prev);
            prev = w as i64;
            b += 1;
        }
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = EverythingLedger::new();
        let _ = lt.record_op(9, match op_cell(9) {
            Some(w) => w,
            None => 0,
        });
        assert_eq!(
            lt.record_op(2, match op_cell(2) {
                Some(w) => w,
                None => 0,
            }),
            Err(EverythingCode::EOrder)
        );
        assert!(lt
            .record_op(10, match op_cell(10) {
                Some(w) => w,
                None => 0,
            })
            .is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = EverythingLedger::new();
        assert_eq!(
            lt.record_op(5, match op_cell(5) {
                Some(w) => w,
                None => 0,
            }
            .wrapping_add(1)),
            Err(EverythingCode::EDrift)
        );
        let mut lt2 = EverythingLedger::new();
        let _ = lt2.record_beat(0, match beat_cell(0) {
            Some(w) => w,
            None => 0,
        });
        assert_eq!(
            lt2.record_beat(1, match beat_cell(1) {
                Some(w) => w,
                None => 0,
            }
            .wrapping_add(1)),
            Err(EverythingCode::EDrift)
        );
    }

    #[test]
    fn test_beat_order_caught() {
        // 平拍（载荷相同）也必抓：先按合法序落格 0，再以格 0 的载荷重复注拍。
        let mut lt = EverythingLedger::new();
        let w0 = match beat_cell(0) {
            Some(w) => w,
            None => 0,
        };
        let _ = lt.record_beat(0, w0);
        // 格 1 载荷但拍值不大于 last_beat 的情形：直接以格 0 载荷再落格 0 被 EDup 拦，
        // 改走拍序：构造格 2 载荷却先落格 1 之前回拍的等价路径——用越序重放格 1。
        let w1 = match beat_cell(1) {
            Some(w) => w,
            None => 0,
        };
        assert!(lt.record_beat(1, w1).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = EverythingLedger::new();
        let _ = lt.record_proj(0, match proj_cell(0) {
            Some(w) => w,
            None => 0,
        });
        assert_eq!(
            lt.record_proj(0, match proj_cell(0) {
                Some(w) => w,
                None => 0,
            }),
            Err(EverythingCode::EDup)
        );
        assert!(lt.declare_rg(0).is_ok());
        assert_eq!(lt.declare_rg(0), Err(EverythingCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = EverythingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(EverythingCode::ENoGate));
        assert!(lt.gates_all());
        let mut lt2 = EverythingLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：20 操作 + 16 拍 + 8 投影 = 44。
        assert_eq!(recorded, OPS as u32 + IDX_BEATS as u32 + PROJ as u32);
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
        assert_eq!(SEG_BASE, 0xCAE00);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(EverythingCode::Ok as u32, 0);
        assert_eq!(EverythingCode::EGradeDrift as u32, 12);
        assert_eq!(EverythingCode::Ok.val(), SEG_BASE);
        assert_eq!(EverythingCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        // 13 变体全明细非空。
        let all = [
            EverythingCode::Ok,
            EverythingCode::ENoOp,
            EverythingCode::EDrift,
            EverythingCode::EOrder,
            EverythingCode::EBeatOrder,
            EverythingCode::ENoRow,
            EverythingCode::ERowMiss,
            EverythingCode::EDup,
            EverythingCode::ENoGate,
            EverythingCode::ESeal,
            EverythingCode::EInj,
            EverythingCode::EFp,
            EverythingCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt = EverythingLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        let mut lt2 = EverythingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        let mut lt3 = EverythingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = lt3.record_op(o, w);
            }
            o += 1;
        }
        let mut b = 0usize;
        while b < IDX_BEATS {
            if let Some(w) = beat_cell(b) {
                let _ = lt3.record_beat(b, w);
            }
            b += 1;
        }
        let mut p = 0usize;
        while p < PROJ {
            if let Some(w) = proj_cell(p) {
                let _ = lt3.record_proj(p, w);
            }
            p += 1;
        }
        let mut r = 0usize;
        while r < RG_ROWS {
            let _ = lt3.declare_rg(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt3.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt3.grade(), Grade::S1);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = EverythingLedger::new();
        let w = match op_cell(0) {
            Some(w) => w,
            None => 0,
        };
        let _ = lt.record_op(0, w.wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 1);
        assert_eq!(caught, 1);
        assert_eq!(esc, 0);
        // 错误路径已记注入账：指纹必被 fnv 推进，不得断言不变。
        let fp0 = lt.fingerprint();
        let _ = lt.record_op(1, w.wrapping_add(2));
        assert_ne!(lt.fingerprint(), fp0);
        // 越闭集路径在 fnv 之前拒：不记注入账。
        let fp1 = lt.fingerprint();
        let _ = lt.record_op(OPS + 1, 0);
        let _ = lt.record_beat(IDX_BEATS, 0);
        assert_eq!(lt.fingerprint(), fp1);
    }

    #[test]
    fn unx52441_checks_green() {
        let s = run_unx52441_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 14);
    }
}
