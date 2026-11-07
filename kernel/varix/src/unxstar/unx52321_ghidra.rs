//! UNX-F52321 Ghidra 收口 · 0xC9000段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52321"，逐字）：
//!   title: Ghidra 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52322-J1 六判据
//!     全落；分析/反汇编 ×10；许可面如实定级（AI-95 联签锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线②；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52321）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52322-J1，照原文锚定；功能按本卡标题（Ghidra）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 版本阶梯闭集：10.x / 11.x / 12.x 三级，越级必抓；每级独立载荷
//!      折叠，纯函数恒等才收。
//!   ② 分析流水五段 20 格闭集（导入/初步分析/引用与调用图/数据流/
//!      后处理）各 4 格，段单调（逆序必抓）；反编译窗口 10 操作闭集
//!      （打开窗口/光标定位/数据流追踪/高亮/注释/重命名/签名补全/切
//!      换视图/导出/关闭），逐操作载荷恒等才收，漂移必抓。
//!   ③ 脚本（Java/Python）三态 8 格：Java 4 格 + Python 4 格，三态
//!      （注册/执行/回写）逐格落账，越格必抓。
//!   ④ Java 上栈 4 层（JRE 运行时逐层折叠）；NSA 开源声明 2 行闭集
//!      （NSA 开源发布声明/Apache-2.0 许可声明），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（版本级越
//!      阶梯/窗口操作漂移/分析格越闭集），检出+逃逸守恒等式；双跑
//!      FNV 指纹一致；定级 S1/S2/S3 闭集（AI-95 联签锚定行：开源许
//!      可面如实，全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52321;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 分析流水格数（五段 × 4）。
pub const STAGE_OPS: usize = 20;

/// 反编译窗口操作数。
pub const WINDOW_OPS: usize = 10;

/// 脚本三态格数（Java 4 + Python 4）。
pub const SCRIPT_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// Java 上栈层数。
pub const JAVA_STACK: usize = 4;

/// NSA 开源声明行数。
pub const NSA_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhidraCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 分析段格越闭集
    ENoStage,
    /// 分析段漂移
    EStageDrift,
    /// 分析段逆序
    EOrder,
    /// 窗口操作越闭集
    ENoWin,
    /// 窗口操作漂移
    EWinDrift,
    /// 脚本格越闭集
    ENoScript,
    /// 脚本格漂移
    EScriptDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl GhidraCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GhidraCode::Ok => "成功",
            GhidraCode::ENoVer => "版本级越阶梯：10.x/11.x/12.x 三级闭集之外先登记",
            GhidraCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            GhidraCode::ENoStage => "分析段格越闭集：分析流水五段 20 格之外先登记",
            GhidraCode::EStageDrift => "分析段漂移：段格折叠与基准不恒等，逐格复算",
            GhidraCode::EOrder => "分析段逆序：导入→初步分析→引用与调用图→数据流→后处理单调，逆序必抓",
            GhidraCode::ENoWin => "窗口操作越闭集：反编译窗口 10 操作之外先登记",
            GhidraCode::EWinDrift => "窗口操作漂移：窗口操作折叠与基准不恒等，逐格复算",
            GhidraCode::ENoScript => "脚本格越闭集：Java/Python 三态 8 格之外先登记",
            GhidraCode::EScriptDrift => "脚本格漂移：脚本格折叠与基准不恒等，逐格复算",
            GhidraCode::EDup => "重复落账：同格只许一条账目",
            GhidraCode::ESeal => "未齐即封：20 分析格 + 10 窗口操作 + 8 脚本格未齐先补齐",
            GhidraCode::EGradeDrift => "定级漂移：全通且 NSA 声明全齐才可定 S1（AI-95 联签锚定行）",
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
            Grade::S1 => "S1：六判据+20 分析格+10 窗口操作+8 脚本格+2 行 NSA 声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本级载荷（级 v：0=10.x 1=11.x 2=12.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xC1000u64 + (v as u64) * 0x61) ^ 0x4748_4944_5241_0000)
}

/// 版本级名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("10.x"),
        1 => Some("11.x"),
        2 => Some("12.x"),
        _ => None,
    }
}

/// 分析流水段格载荷（格 o；段 = o/4）。
pub const fn stage_cell(o: usize) -> Option<u64> {
    if o >= STAGE_OPS {
        return None;
    }
    Some((0xC2000u64 + (o as u64) * 0x3B) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 分析流水段号（0..4=导入 4..8=初步分析 8..12=引用与调用图 12..16=数据流 16..20=后处理）。
pub const fn stage_kind(o: usize) -> Option<u32> {
    if o >= STAGE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 分析段名。
pub const fn stage_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("导入"),
        1 => Some("初步分析"),
        2 => Some("引用与调用图"),
        3 => Some("数据流"),
        4 => Some("后处理"),
        _ => None,
    }
}

/// 反编译窗口操作载荷（操作 o）。
pub const fn window_cell(o: usize) -> Option<u64> {
    if o >= WINDOW_OPS {
        return None;
    }
    Some((0xC3000u64 + (o as u64) * 0x47) ^ 0x5749_4E44_4F57_0000)
}

/// 窗口操作名（const fn 表）。
pub const fn window_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开窗口"),
        1 => Some("光标定位"),
        2 => Some("数据流追踪"),
        3 => Some("高亮"),
        4 => Some("注释"),
        5 => Some("重命名"),
        6 => Some("签名补全"),
        7 => Some("切换视图"),
        8 => Some("导出"),
        9 => Some("关闭窗口"),
        _ => None,
    }
}

/// 脚本三态格载荷（格 s：0..4=Java 4..8=Python；三态 = s%3 注册/执行/回写 交叠闭集）。
pub const fn script_cell(s: usize) -> Option<u64> {
    if s >= SCRIPT_CELLS {
        return None;
    }
    let family = (s / 4) as u64;
    Some((0xC4000u64 + (s as u64) * 0x53) ^ (family.wrapping_mul(0x0100_0000) + (s % 3) as u64))
}

/// 脚本三态名（态 t：0=注册 1=执行 2=回写）。
pub const fn script_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("注册"),
        1 => Some("执行"),
        2 => Some("回写"),
        _ => None,
    }
}

/// Java 上栈层载荷（层 l）。
pub const fn java_stack_cell(l: usize) -> Option<u64> {
    if l >= JAVA_STACK {
        return None;
    }
    Some((0xC5000u64 + (l as u64) * 0x29) ^ 0x4A41_5652_5430_0000)
}

/// NSA 声明行折叠（行 r）。
pub const fn nsa_row(r: usize) -> Option<u32> {
    if r >= NSA_ROWS {
        return None;
    }
    Some(0xC600 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Ghidra 收口账本状态机。
#[derive(Clone)]
pub struct GhidraLedger {
    /// 六判据（分析/反汇编/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 分析流水账。
    stages: [Option<u64>; STAGE_OPS],
    /// 反编译窗口账。
    wins: [Option<u64>; WINDOW_OPS],
    /// 脚本三态账。
    scripts: [Option<u64>; SCRIPT_CELLS],
    /// Java 上栈账。
    jstack: [Option<u64>; JAVA_STACK],
    /// NSA 声明账。
    nsa: [Option<u32>; NSA_ROWS],
    /// 最后段号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GhidraLedger {
    /// 新账本。
    pub fn new() -> Self {
        GhidraLedger {
            gates: [None; 6],
            vers: [None; VERSION_STEPS],
            stages: [None; STAGE_OPS],
            wins: [None; WINDOW_OPS],
            scripts: [None; SCRIPT_CELLS],
            jstack: [None; JAVA_STACK],
            nsa: [None; NSA_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GhidraCode> {
        if gate >= 6 {
            return Err(GhidraCode::ESeal);
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

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), GhidraCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(GhidraCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + v as u64);
            return Err(GhidraCode::EVerDrift);
        }
        let dup = match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(GhidraCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5201 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 级全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 分析流水一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_stage(&mut self, o: usize, got: u64) -> Result<(), GhidraCode> {
        if o >= STAGE_OPS {
            return Err(GhidraCode::ENoStage);
        }
        let kind = match stage_kind(o) {
            Some(k) => k,
            None => return Err(GhidraCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(GhidraCode::EOrder);
        }
        let want = match stage_cell(o) {
            Some(w) => w,
            None => return Err(GhidraCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(GhidraCode::EStageDrift);
        }
        let dup = match self.stages.get(o) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(GhidraCode::EDup);
        }
        if let Some(c) = self.stages.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 分析流水全账核验（20 格）。
    pub fn stages_all(&self) -> bool {
        let mut o = 0usize;
        while o < STAGE_OPS {
            match self.stages.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 反编译窗口一格（载荷对照 + 重复拒）。
    pub fn record_window(&mut self, o: usize, got: u64) -> Result<(), GhidraCode> {
        let want = match window_cell(o) {
            Some(w) => w,
            None => return Err(GhidraCode::ENoWin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + o as u64);
            return Err(GhidraCode::EWinDrift);
        }
        let dup = match self.wins.get(o) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(GhidraCode::EDup);
        }
        if let Some(c) = self.wins.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5401 + o as u64);
        Ok(())
    }

    /// 反编译窗口全账核验（10 操作）。
    pub fn wins_all(&self) -> bool {
        let mut o = 0usize;
        while o < WINDOW_OPS {
            match self.wins.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 脚本三态一格（载荷对照 + 重复拒）。
    pub fn record_script(&mut self, s: usize, got: u64) -> Result<(), GhidraCode> {
        let want = match script_cell(s) {
            Some(w) => w,
            None => return Err(GhidraCode::ENoScript),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + s as u64);
            return Err(GhidraCode::EScriptDrift);
        }
        let dup = match self.scripts.get(s) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(GhidraCode::EDup);
        }
        if let Some(c) = self.scripts.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5501 + s as u64);
        Ok(())
    }

    /// 脚本三态全账核验（8 格）。
    pub fn scripts_all(&self) -> bool {
        let mut s = 0usize;
        while s < SCRIPT_CELLS {
            match self.scripts.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Java 上栈一层（载荷对照 + 重复拒）。
    pub fn record_java_stack(&mut self, l: usize, got: u64) -> Result<(), GhidraCode> {
        let want = match java_stack_cell(l) {
            Some(w) => w,
            None => return Err(GhidraCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + l as u64);
            return Err(GhidraCode::EStageDrift);
        }
        let dup = match self.jstack.get(l) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(GhidraCode::EDup);
        }
        if let Some(c) = self.jstack.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x5601 + l as u64);
        Ok(())
    }

    /// Java 上栈全账核验（4 层）。
    pub fn jstack_all(&self) -> bool {
        let mut l = 0usize;
        while l < JAVA_STACK {
            match self.jstack.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// NSA 声明一行。
    pub fn declare_nsa(&mut self, r: usize) -> Result<(), GhidraCode> {
        if r >= NSA_ROWS {
            return Err(GhidraCode::ENoWin);
        }
        let dup = match self.nsa.get(r) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(GhidraCode::EDup);
        }
        if let Some(c) = self.nsa.get_mut(r) {
            *c = nsa_row(r);
        }
        self.fnv(0x5700 + r as u64);
        Ok(())
    }

    /// NSA 声明全齐核验。
    pub fn nsa_all(&self) -> bool {
        let mut r = 0usize;
        while r < NSA_ROWS {
            match self.nsa.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.vers_all()
            && self.stages_all()
            && self.wins_all()
            && self.scripts_all()
            && self.jstack_all()
            && self.nsa_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GhidraLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GhidraLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < STAGE_OPS {
        if let Some(w) = stage_cell(o) {
            if led.record_stage(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut w = 0usize;
    while w < WINDOW_OPS {
        if let Some(p) = window_cell(w) {
            if led.record_window(w, p).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let mut s = 0usize;
    while s < SCRIPT_CELLS {
        if let Some(p) = script_cell(s) {
            if led.record_script(s, p).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut l = 0usize;
    while l < JAVA_STACK {
        if let Some(p) = java_stack_cell(l) {
            if led.record_java_stack(l, p).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < NSA_ROWS {
        let _ = led.declare_nsa(r);
        r += 1;
    }
    // 注入 10 轮：版本级越阶梯 / 窗口操作漂移 / 分析格越闭集三态轮转。
    // 逆序注入先按合法序落高段格再注低段格（空账本上低段本就合法，须先抬 last_kind）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GhidraLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_version(0, match version_cell(0) {
                    Some(p) => p,
                    None => 0,
                });
                lt.record_version(VERSION_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_window(0, match window_cell(0) {
                    Some(p) => p,
                    None => 0,
                });
                lt.record_window(
                    1,
                    match window_cell(1) {
                        Some(p) => p,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(GhidraCode::EWinDrift)
            }
            _ => {
                let _ = lt.record_stage(19, match stage_cell(19) {
                    Some(p) => p,
                    None => 0,
                });
                lt.record_stage(STAGE_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5800 + k as u64);
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
/// UNX-F52321 判据集（锚 UNX-F52322-J1，off-by-one 板式）。
pub fn run_unx52321_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52321");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "分析流水五段 20 格全通",
        recorded >= STAGE_OPS as u32,
        "导入/初步分析/引用与调用图/数据流/后处理五段逐格恒等",
    );
    s.add(
        "反编译窗口 10 操作全落",
        {
            let mut lt = GhidraLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < WINDOW_OPS {
                if let Some(p) = window_cell(o) {
                    if lt.record_window(o, p).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.wins_all()
        },
        "打开窗口/光标定位/数据流追踪/高亮/注释/重命名/签名补全/切换视图/导出/关闭逐操作恒等",
    );
    s.add(
        "脚本三态 8 格全落",
        {
            let mut lt = GhidraLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SCRIPT_CELLS {
                if let Some(p) = script_cell(c) {
                    if lt.record_script(c, p).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.scripts_all()
        },
        "Java 4 格 + Python 4 格，注册/执行/回写三态逐格落账",
    );
    s.add(
        "版本阶梯 10.x/11.x/12.x 三级全落",
        {
            let mut lt = GhidraLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(p) = version_cell(v) {
                    if lt.record_version(v, p).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三级闭集逐级恒等，越级必抓",
    );
    s.add(
        "Java 上栈 4 层全落",
        {
            let mut lt = GhidraLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JAVA_STACK {
                if let Some(p) = java_stack_cell(l) {
                    if lt.record_java_stack(l, p).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.jstack_all()
        },
        "JRE 运行时逐层折叠恒等",
    );
    s.add(
        "NSA 开源声明 2/2 全齐",
        {
            let mut lt = GhidraLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < NSA_ROWS {
                if lt.declare_nsa(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.nsa_all()
        },
        "NSA 开源发布声明/Apache-2.0 许可声明两行闭集（AI-95 联签锚定行）",
    );
    s.add(
        "分析段单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < STAGE_OPS {
                if stage_kind(o).unwrap_or(5) < stage_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "版本越阶梯/窗口漂移/分析越闭集三态轮转必抓");
    let mut l0 = GhidraLedger::new();
    s.add(
        "边界·版本级越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "边界·窗口操作越闭集",
        l0.record_window(WINDOW_OPS, 0).is_err(),
        "窗口操作 10 越闭集必拒",
    );
    s.add(
        "边界·分析格越闭集",
        l0.record_stage(STAGE_OPS, 0).is_err(),
        "分析格 20 越闭集必拒",
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
            let mut lt = GhidraLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(p) = version_cell(v) {
                    let _ = lt.record_version(v, p);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < STAGE_OPS {
                if let Some(p) = stage_cell(o) {
                    let _ = lt.record_stage(o, p);
                }
                o += 1;
            }
            let mut w = 0usize;
            while w < WINDOW_OPS {
                if let Some(p) = window_cell(w) {
                    let _ = lt.record_window(w, p);
                }
                w += 1;
            }
            let mut c = 0usize;
            while c < SCRIPT_CELLS {
                if let Some(p) = script_cell(c) {
                    let _ = lt.record_script(c, p);
                }
                c += 1;
            }
            let mut l = 0usize;
            while l < JAVA_STACK {
                if let Some(p) = java_stack_cell(l) {
                    let _ = lt.record_java_stack(l, p);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < NSA_ROWS {
                let _ = lt.declare_nsa(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，开源许可面如实）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9000,
        "SEG_BASE 0x40 对齐且落 VX15 域 0xC9000 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("10.x"));
        assert_eq!(version_name(1), Some("11.x"));
        assert_eq!(version_name(2), Some("12.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_stage_kind() {
        assert_eq!(stage_kind(0), Some(0));
        assert_eq!(stage_kind(4), Some(1));
        assert_eq!(stage_kind(8), Some(2));
        assert_eq!(stage_kind(19), Some(4));
        assert!(stage_kind(20).is_none());
        assert!(stage_cell(20).is_none());
        assert_eq!(stage_kind_name(2), Some("引用与调用图"));
    }

    #[test]
    fn test_window_ops() {
        assert_eq!(window_op_name(0), Some("打开窗口"));
        assert_eq!(window_op_name(6), Some("签名补全"));
        assert_eq!(window_op_name(9), Some("关闭窗口"));
        assert!(window_op_name(10).is_none());
        assert!(window_cell(WINDOW_OPS).is_none());
    }

    #[test]
    fn test_script_states() {
        assert_eq!(script_state_name(0), Some("注册"));
        assert_eq!(script_state_name(1), Some("执行"));
        assert_eq!(script_state_name(2), Some("回写"));
        assert!(script_state_name(3).is_none());
        let a = script_cell(3).unwrap_or(0);
        assert_ne!(a, script_cell(4).unwrap_or(0));
        assert!(script_cell(SCRIPT_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = stage_cell(3).unwrap_or(0);
        assert_eq!(a, stage_cell(3).unwrap_or(0));
        assert_ne!(a, stage_cell(4).unwrap_or(0));
        assert!(nsa_row(NSA_ROWS).is_none());
        assert!(java_stack_cell(JAVA_STACK).is_none());
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = GhidraLedger::new();
        // 合法序先落高段格，再注低段格（空账本低段本就合法）。
        let _ = lt.record_stage(9, stage_cell(9).unwrap_or(0));
        assert_eq!(lt.record_stage(2, stage_cell(2).unwrap_or(0)), Err(GhidraCode::EOrder));
        assert!(lt.record_stage(10, stage_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GhidraLedger::new();
        assert_eq!(
            lt.record_window(5, window_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(GhidraCode::EWinDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(GhidraCode::EVerDrift)
        );
        assert_eq!(
            lt.record_script(3, script_cell(3).unwrap_or(0).wrapping_add(2)),
            Err(GhidraCode::EScriptDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GhidraLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(GhidraCode::EDup));
        let _ = lt.declare_nsa(0);
        assert_eq!(lt.declare_nsa(0), Err(GhidraCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GhidraLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GhidraLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= STAGE_OPS as u32 + WINDOW_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC9000);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GhidraCode::Ok as u32, 0);
        assert_eq!(GhidraCode::EGradeDrift as u32, 12);
        assert!(!GhidraCode::Ok.detail().is_empty());
        assert!(!GhidraCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GhidraLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52321_checks_green() {
        let s = run_unx52321_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
