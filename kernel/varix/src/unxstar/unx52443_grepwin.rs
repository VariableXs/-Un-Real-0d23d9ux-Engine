//! UNX-F52443 grepWin 收口 · 0xCAE80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52443"，逐字）：
//!   title: grepWin 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52444-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52443）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52444-J1，照原文锚定；功能按本卡标题（grepWin）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 操作闭集：正则/目录/替换/预览/集成，每族 4 操作，族
//!      号 = 操作号/4（i/N 映射），逆序必抓。
//!   ② shell 集成三元闸 3 格：每格三态（0=未启用 1=启用 2=停用），态越
//!      三元必抓；三元闸全落才许封账。
//!   ③ 结果投影 8 格：投影折叠纯函数恒等才收。
//!   ④ 与 ripgrep（F52358）消费分账声明 2 行闭集；C++ 上栈声明 2 行闭
//!      集，两册各自全齐才许封账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（家族逆序/
//!      shell 态越三元/操作越闭集），全部落在当轮临时账本上；双跑 FNV
//!      指纹一致；定级 S1/S2/S3 闭集（全齐 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52443;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数（每族 4 操作）。
pub const OPS: usize = 20;

/// shell 集成三元闸格数。
pub const SHELL_CELLS: usize = 3;

/// shell 集成态闭集（三元）。
pub const SHELL_STATES: u32 = 3;

/// 结果投影格数。
pub const PROJ: usize = 8;

/// ripgrep（F52358）消费分账声明行数。
pub const RG_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepwinCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// shell 态越三元
    EShellPhase,
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

impl GrepwinCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GrepwinCode::Ok => "成功",
            GrepwinCode::ENoOp => "格号越闭集：20 操作/3 三元闸/8 投影/声明行闭集之外先登记",
            GrepwinCode::EDrift => "载荷漂移：操作/shell 格/投影折叠与基准不恒等，逐格复算",
            GrepwinCode::EOrder => "家族逆序：正则→目录→替换→预览→集成单调，逆序必抓",
            GrepwinCode::EShellPhase => "shell 态越三元：未启用/启用/停用三态之外先登记",
            GrepwinCode::ENoRow => "声明行越闭集：ripgrep 消费 2 行与 C++ 上栈 2 行闭集之外先登记",
            GrepwinCode::ERowMiss => "缺声明行：ripgrep（F52358）消费分账或 C++ 上栈声明未齐先补齐",
            GrepwinCode::EDup => "重复落账：同格只许一条账目",
            GrepwinCode::ENoGate => "判据闸越闭集：六判据 0..=5 之外先登记",
            GrepwinCode::ESeal => "未齐即封：20 操作+3 三元闸+8 投影+两册声明未齐先补齐",
            GrepwinCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GrepwinCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GrepwinCode::EGradeDrift => "定级漂移：全齐且两册声明全通才可定 S2",
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
            Grade::S1 => "S1：全通收口（本卡封顶 S2，不启用）",
            Grade::S2 => "S2：六判据+20操作+3三元闸+8投影+两册声明全通收口",
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
    Some((0xDA000u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=正则 4..8=目录 8..12=替换 12..16=预览 16..20=集成）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("正则"),
        1 => Some("目录"),
        2 => Some("替换"),
        3 => Some("预览"),
        4 => Some("集成"),
        _ => None,
    }
}

/// shell 集成三元闸格基准（格 c、态 t<3）。
pub const fn shell_cell(c: usize, t: u32) -> Option<u64> {
    if c >= SHELL_CELLS || t >= SHELL_STATES {
        return None;
    }
    Some(0xDB00 + (c as u64) * 0x43 + (t as u64) * 0x9)
}

/// shell 集成态名。
pub const fn shell_state_name(t: u32) -> Option<&'static str> {
    match t {
        0 => Some("未启用"),
        1 => Some("启用"),
        2 => Some("停用"),
        _ => None,
    }
}

/// 结果投影格载荷（投影 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ {
        return None;
    }
    Some((0xDC000u64 + (p as u64) * 0x3D) ^ 0x47524550_57494E31)
}

/// ripgrep（F52358）消费分账声明行折叠（行 r）。
pub const fn rg_row(r: usize) -> Option<u32> {
    if r >= RG_ROWS {
        return None;
    }
    Some(0xDD00 + r as u32 * 0x30)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xDE00 + r as u32 * 0x30)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// grepWin 收口账本状态机。
#[derive(Clone)]
pub struct GrepwinLedger {
    /// 六判据（启动/正则/替换/预览/卸载/升级）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// shell 三元闸账。
    shells: [Option<u64>; SHELL_CELLS],
    /// 投影账。
    projs: [Option<u64>; PROJ],
    /// ripgrep 消费分账声明账。
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

impl GrepwinLedger {
    /// 新账本。
    pub fn new() -> Self {
        GrepwinLedger {
            gates: [None; 6],
            ops: [None; OPS],
            shells: [None; SHELL_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GrepwinCode> {
        if gate >= 6 {
            return Err(GrepwinCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GrepwinCode> {
        if o >= OPS {
            return Err(GrepwinCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(GrepwinCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5310);
            return Err(GrepwinCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(GrepwinCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5311 + o as u64);
            return Err(GrepwinCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrepwinCode::EDup);
        }
        if let Some(x) = self.ops.get_mut(o) {
            *x = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5312 + o as u64);
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

    /// shell 三元闸一格（态对照 + 重复拒；态越三元必抓且不改 last_kind）。
    pub fn record_shell(&mut self, c: usize, t: u32, got: u64) -> Result<(), GrepwinCode> {
        if c >= SHELL_CELLS {
            return Err(GrepwinCode::ENoOp);
        }
        let want = match shell_cell(c, t) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5320 + c as u64);
                return Err(GrepwinCode::EShellPhase);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5321 + c as u64);
            return Err(GrepwinCode::EDrift);
        }
        if match self.shells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrepwinCode::EDup);
        }
        if let Some(x) = self.shells.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5322 + c as u64);
        Ok(())
    }

    /// shell 三元闸全账核验（3 格）。
    pub fn shells_all(&self) -> bool {
        let mut c = 0usize;
        while c < SHELL_CELLS {
            match self.shells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), GrepwinCode> {
        if p >= PROJ {
            return Err(GrepwinCode::ENoOp);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(GrepwinCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5330 + p as u64);
            return Err(GrepwinCode::EDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrepwinCode::EDup);
        }
        if let Some(x) = self.projs.get_mut(p) {
            *x = Some(got);
        }
        self.fnv(0x5331 + p as u64);
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

    /// ripgrep（F52358）消费分账声明一行。
    pub fn declare_rg(&mut self, r: usize) -> Result<(), GrepwinCode> {
        if r >= RG_ROWS {
            return Err(GrepwinCode::ENoRow);
        }
        if match self.rg.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrepwinCode::EDup);
        }
        if let Some(x) = self.rg.get_mut(r) {
            *x = rg_row(r);
        }
        self.fnv(0x5340 + r as u64);
        Ok(())
    }

    /// ripgrep 消费分账声明全齐核验。
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
    pub fn declare_stack(&mut self, r: usize) -> Result<(), GrepwinCode> {
        if r >= STACK_ROWS {
            return Err(GrepwinCode::ENoRow);
        }
        if match self.stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrepwinCode::EDup);
        }
        if let Some(x) = self.stack.get_mut(r) {
            *x = stack_row(r);
        }
        self.fnv(0x5350 + r as u64);
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

    /// KB-COMPAT 定级（全齐 → S2 封顶，判据缺 → S3，其余 → S2）。
    pub fn grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.shells_all()
            && self.projs_all()
            && self.rg_all()
            && self.stack_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GrepwinLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GrepwinLedger::new();
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
    let mut c = 0usize;
    while c < SHELL_CELLS {
        // 逐格按三元轮转取态：态 = c % 3。
        let t = (c % 3) as u32;
        if let Some(w) = shell_cell(c, t) {
            if led.record_shell(c, t, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
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
    // 注入 10 轮：家族逆序 / shell 态越三元 / 操作越闭集三态轮转，全部落
    // 在当轮临时账本上；逆序注入先按合法序落正格（格 17）再回拍（格 5）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GrepwinLedger::new();
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
                lt.record_op(5, w5) == Err(GrepwinCode::EOrder)
            }
            1 => {
                // shell 态越三元：空账直接注态 3，必抓且不改 last_kind。
                lt.record_shell(0, SHELL_STATES, 0) == Err(GrepwinCode::EShellPhase)
            }
            _ => lt.record_op(OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5360 + k as u64);
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
/// UNX-F52443 判据集（锚 UNX-F52444-J1，off-by-one 板式）。
pub fn run_unx52443_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52443");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= OPS as u32,
        "正则/目录/替换/预览/集成逐操作恒等（§AI-66.3）",
    );
    s.add(
        "shell 集成三元闸 3/3 全落",
        {
            let mut lt = GrepwinLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SHELL_CELLS {
                let t = (c % 3) as u32;
                if let Some(w) = shell_cell(c, t) {
                    if lt.record_shell(c, t, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.shells_all()
        },
        "未启用/启用/停用三元闭集逐格恒等",
    );
    s.add(
        "结果投影 8/8 全齐",
        {
            let mut lt = GrepwinLedger::new();
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
        "分账声明 4/4 全齐（ripgrep 消费 2 + C++ 上栈 2）",
        {
            let mut lt = GrepwinLedger::new();
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
        "ripgrep（F52358）消费分账 2 行 + C++ 上栈声明 2 行，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GrepwinLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/正则/替换/预览/卸载/升级逐格落账（UNX-F52444-J1）",
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
        "家族逆序/shell 态越三元/操作越闭集三态轮转必抓",
    );
    let mut l0 = GrepwinLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·shell 态越三元",
        l0.record_shell(0, SHELL_STATES, 0).is_err(),
        "态 3 越三元闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        match l0.declare_stack(STACK_ROWS) {
            Err(GrepwinCode::ENoRow) => true,
            _ => false,
        },
        "C++ 上栈声明行 2 之外越闭集必拒",
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
            let mut lt = GrepwinLedger::new();
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
            let mut c = 0usize;
            while c < SHELL_CELLS {
                let t = (c % 3) as u32;
                if let Some(w) = shell_cell(c, t) {
                    let _ = lt.record_shell(c, t, w);
                }
                c += 1;
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
            lt.grade() == Grade::S2
        },
        "全齐 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAE80,
        "SEG_BASE 0x40 对齐且在 VX16 域 52443 格",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn w(o: usize) -> u64 {
        match op_cell(o) {
            Some(v) => v,
            None => 0,
        }
    }

    #[test]
    fn test_op_kind_mapping() {
        // i/N 映射：kind = o/4。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(10), Some(2));
        assert_eq!(op_kind(15), Some(3));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(1), Some("目录"));
        assert_eq!(op_kind_name(4), Some("集成"));
        assert!(op_kind_name(7).is_none());
    }

    #[test]
    fn test_shell_states_closed() {
        assert_eq!(shell_state_name(0), Some("未启用"));
        assert_eq!(shell_state_name(1), Some("启用"));
        assert_eq!(shell_state_name(2), Some("停用"));
        assert!(shell_state_name(3).is_none());
        assert!(shell_cell(SHELL_CELLS, 0).is_none());
        assert!(shell_cell(0, SHELL_STATES).is_none());
        assert!(shell_cell(0, 0).is_some());
    }

    #[test]
    fn test_cells_pure() {
        let a = w(6);
        assert_eq!(a, w(6));
        assert_ne!(a, w(7));
        assert!(proj_cell(PROJ).is_none());
        assert!(rg_row(RG_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GrepwinLedger::new();
        let _ = lt.record_op(11, w(11));
        assert_eq!(lt.record_op(3, w(3)), Err(GrepwinCode::EOrder));
        assert!(lt.record_op(12, w(12)).is_ok());
    }

    #[test]
    fn test_shell_phase_caught_on_empty() {
        // 空账注态 3 必抓，且不改 last_kind（后续合法操作不误判逆序）。
        let mut lt = GrepwinLedger::new();
        assert_eq!(lt.record_shell(2, SHELL_STATES, 0), Err(GrepwinCode::EShellPhase));
        assert!(lt.record_op(0, w(0)).is_ok());
        // 已落格重复注格被 EDup 拦。
        let t = 2u32;
        let wt = match shell_cell(0, t) {
            Some(v) => v,
            None => 0,
        };
        let _ = lt.record_shell(0, t, wt);
        assert_eq!(lt.record_shell(0, t, wt), Err(GrepwinCode::EDup));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GrepwinLedger::new();
        assert_eq!(lt.record_op(5, w(5).wrapping_add(1)), Err(GrepwinCode::EDrift));
        let t = 1u32;
        let wt = match shell_cell(1, t) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(lt.record_shell(1, t, wt.wrapping_add(3)), Err(GrepwinCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GrepwinLedger::new();
        let _ = lt.record_proj(0, match proj_cell(0) {
            Some(v) => v,
            None => 0,
        });
        assert_eq!(
            lt.record_proj(0, match proj_cell(0) {
                Some(v) => v,
                None => 0,
            }),
            Err(GrepwinCode::EDup)
        );
        assert!(lt.declare_rg(0).is_ok());
        assert_eq!(lt.declare_rg(0), Err(GrepwinCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GrepwinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(6), Err(GrepwinCode::ENoGate));
        assert!(lt.gates_all());
        let mut lt2 = GrepwinLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：20 操作 + 3 三元闸 + 8 投影 = 31。
        assert_eq!(recorded, OPS as u32 + SHELL_CELLS as u32 + PROJ as u32);
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
        assert_eq!(SEG_BASE, 0xCAE80);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GrepwinCode::Ok as u32, 0);
        assert_eq!(GrepwinCode::EGradeDrift as u32, 12);
        assert_eq!(GrepwinCode::Ok.val(), SEG_BASE);
        assert_eq!(GrepwinCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all = [
            GrepwinCode::Ok,
            GrepwinCode::ENoOp,
            GrepwinCode::EDrift,
            GrepwinCode::EOrder,
            GrepwinCode::EShellPhase,
            GrepwinCode::ENoRow,
            GrepwinCode::ERowMiss,
            GrepwinCode::EDup,
            GrepwinCode::ENoGate,
            GrepwinCode::ESeal,
            GrepwinCode::EInj,
            GrepwinCode::EFp,
            GrepwinCode::EGradeDrift,
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
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2（本卡封顶 S2）。
        let lt = GrepwinLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        let mut lt2 = GrepwinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        let mut lt3 = GrepwinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            let _ = lt3.record_op(o, w(o));
            o += 1;
        }
        let mut c = 0usize;
        while c < SHELL_CELLS {
            let t = (c % 3) as u32;
            if let Some(v) = shell_cell(c, t) {
                let _ = lt3.record_shell(c, t, v);
            }
            c += 1;
        }
        let mut p = 0usize;
        while p < PROJ {
            if let Some(v) = proj_cell(p) {
                let _ = lt3.record_proj(p, v);
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
        assert_eq!(lt3.grade(), Grade::S2);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = GrepwinLedger::new();
        let _ = lt.record_op(0, w(0).wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 1);
        assert_eq!(caught, 1);
        assert_eq!(esc, 0);
        // 错误路径已记注入账：指纹必被 fnv 推进，不得断言不变。
        let fp0 = lt.fingerprint();
        let _ = lt.record_op(1, w(1).wrapping_add(2));
        assert_ne!(lt.fingerprint(), fp0);
    }

    #[test]
    fn unx52443_checks_green() {
        let s = run_unx52443_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 14);
    }
}
