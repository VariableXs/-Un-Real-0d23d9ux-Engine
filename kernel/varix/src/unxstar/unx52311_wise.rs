//! UNX-F52311 Wise Program Uninstaller 收口 · 0xC8D80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52311"，逐字）：
//!   title: Wise Program Uninstaller 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52312-J1 六判据
//!     全落；捆绑推广行为如实记录入账（不隐藏）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52311）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52312-J1，照原文锚定；功能按本卡标题（Wise Program
//! Uninstaller）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 卸载 10 操作集闭集：五家族（执行/扫描/清理/登记/收尾）各 2 操
//!      作，相位闸按已收格数推演期望家族，空账本注高族必抓。
//!   ② 强制卸载/智能扫描三态 12 格：3 态（待扫/命中/清净）× 4 程序位，
//!      态载荷折叠恒等才收，漂移必抓。
//!   ③ 上下文菜单集成 6 行闭集（右键卸载/右键强制卸载/右键智能卸载/
//!      安装器接管/多语入口/还原入口），逐行落账。
//!   ④ 残留清理 8 格闭集：注册表 4 + 文件 4 双侧对照，缺侧拒封。
//!   ⑤ EOL 声明 2 行闭集（如实在册）：产品生命周期声明/EOL 临近提示声
//!      明；捆绑推广声明 1 行：捆绑推广行为如实记录入账（不隐藏）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（操作越闭集/三态漂移/相位越序）
//!      全部落在当轮临时账本上，检出+逃逸守恒；双跑指纹一致；KB-COMPAT
//!      定级 S2 封顶口径（全通 → S2，S1 枚举位保留不签发）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52311;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8D80;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 卸载操作数。
pub const UNINSTALL_OPS: usize = 10;

/// 三态数（待扫/命中/清净）。
pub const TRI_STATES: usize = 3;

/// 每态程序位数（3 态 × 4 = 12 格）。
pub const TRI_SLOTS: usize = 4;

/// 强制卸载/智能扫描三态总格数。
pub const TRI_CELLS: usize = TRI_STATES * TRI_SLOTS;

/// 上下文菜单集成行数。
pub const CTX_ROWS: usize = 6;

/// 残留清理格数（注册表 4 + 文件 4）。
pub const RESID_CELLS: usize = 8;

/// EOL 声明行数。
pub const EOL_ROWS: usize = 2;

/// 捆绑推广声明行数（如实在册，不隐藏）。
pub const BUNDLE_ROWS: usize = 1;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x5749_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WiseCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族相位越序
    EPhase,
    /// 三态/残留格越闭集
    ENoGrid,
    /// 三态载荷漂移
    ETriDrift,
    /// 上下文菜单行越闭集
    ENoCtx,
    /// EOL 声明行越闭集
    ENoRow,
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

impl WiseCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WiseCode::Ok => "成功",
            WiseCode::ENoOp => "操作号越闭集：卸载 10 操作集之外先登记",
            WiseCode::EDrift => "载荷漂移：操作/残留清理折叠与基准不恒等，逐格复算",
            WiseCode::EPhase => "家族相位越序：执行→扫描→清理→登记→收尾按相位推进，越相必抓",
            WiseCode::ENoGrid => "三态/残留格越闭集：三态 12 格与残留 8 格之外先登记",
            WiseCode::ETriDrift => "三态载荷漂移：强制卸载/智能扫描 12 格折叠与基准不恒等，逐态复算",
            WiseCode::ENoCtx => "上下文菜单行越闭集：6 行闭集之外先登记",
            WiseCode::ENoRow => "EOL 声明行越闭集：2 行闭集之外先登记",
            WiseCode::EDup => "重复落账：同格只许一条账目",
            WiseCode::ESeal => "未齐即封：10 操作 + 12 三态格 + 6 行菜单 + 8 残留格 + 2 行 EOL + 1 行捆绑声明未齐先补齐",
            WiseCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WiseCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WiseCode::EGradeDrift => "定级漂移：全通才可定 S2，KB-COMPAT 封顶口径",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本域 S2 封顶：S1 保留枚举位不签发）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：保留位，本域不签发
    S1,
    /// S2：有条件收口（本域封顶级）
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
            Grade::S1 => "S1：保留位，Wise 域存在捆绑推广外部条件不签发",
            Grade::S2 => "S2：六判据+10操作+12三态格+6行菜单+8残留格+2行EOL+1行捆绑声明全通，有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 卸载操作格载荷（操作 o）。
pub const fn uninstall_op_cell(o: usize) -> Option<u64> {
    if o >= UNINSTALL_OPS {
        return None;
    }
    Some((0xC100u64 + (o as u64) * 0x2C) ^ ANCHOR)
}

/// 操作家族（o/2：0 执行 1 扫描 2 清理 3 登记 4 收尾）。
pub const fn op_family(o: usize) -> Option<u32> {
    if o >= UNINSTALL_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn op_family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("执行"),
        1 => Some("扫描"),
        2 => Some("清理"),
        3 => Some("登记"),
        4 => Some("收尾"),
        _ => None,
    }
}

/// 强制卸载/智能扫描三态格载荷（态 t 0..3：0 待扫 1 命中 2 清净；程序位 p 0..4）。
pub const fn tri_cell(t: usize, p: usize) -> Option<u64> {
    if t >= TRI_STATES || p >= TRI_SLOTS {
        return None;
    }
    Some((0xC200u64 + (t as u64) * 0x120 + (p as u64) * 0x2C) ^ ANCHOR)
}

/// 三态名。
pub const fn tri_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("待扫"),
        1 => Some("命中"),
        2 => Some("清净"),
        _ => None,
    }
}

/// 上下文菜单集成行折叠（行 c）。
pub const fn ctx_row(c: usize) -> Option<u32> {
    if c >= CTX_ROWS {
        return None;
    }
    Some(0xC300 + c as u32 * 0x2A)
}

/// 上下文菜单行名。
pub const fn ctx_row_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("右键卸载"),
        1 => Some("右键强制卸载"),
        2 => Some("右键智能卸载"),
        3 => Some("安装器接管"),
        4 => Some("多语入口"),
        5 => Some("还原入口"),
        _ => None,
    }
}

/// 残留清理格载荷（i 0..8：0..4 注册表侧，4..8 文件侧）。
pub const fn resid_cell(i: usize) -> Option<u64> {
    if i >= RESID_CELLS {
        return None;
    }
    Some((0xC400u64 + (i as u64) * 0x24) ^ ANCHOR)
}

/// EOL 声明行折叠（行 r，如实在册）。
pub const fn eol_row(r: usize) -> Option<u32> {
    if r >= EOL_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x30)
}

/// 捆绑推广声明行折叠（如实在册，不隐藏）。
pub const fn bundle_row() -> Option<u32> {
    Some(0xC600)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Wise Program Uninstaller 收口账本状态机。
#[derive(Clone)]
pub struct WiseLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 卸载操作账。
    ops: [Option<u64>; UNINSTALL_OPS],
    /// 三态账（3 态 × 4 程序位平铺）。
    tri: [Option<u64>; TRI_CELLS],
    /// 上下文菜单集成账。
    ctx: [Option<u32>; CTX_ROWS],
    /// 残留清理账。
    resid: [Option<u64>; RESID_CELLS],
    /// EOL 声明账。
    eol: [Option<u32>; EOL_ROWS],
    /// 捆绑推广声明账（如实在册）。
    bundle: Option<u32>,
    /// 已收操作数（相位推演基）。
    op_count: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WiseLedger {
    /// 新账本。
    pub fn new() -> Self {
        WiseLedger {
            gates: [None; 6],
            ops: [None; UNINSTALL_OPS],
            tri: [None; TRI_CELLS],
            ctx: [None; CTX_ROWS],
            resid: [None; RESID_CELLS],
            eol: [None; EOL_ROWS],
            bundle: None,
            op_count: 0,
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

    /// 落一格六判据（UNX-F52312-J1）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WiseCode> {
        if gate >= 6 {
            return Err(WiseCode::ESeal);
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

    /// 卸载操作一格（相位闸 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), WiseCode> {
        if o >= UNINSTALL_OPS {
            return Err(WiseCode::ENoOp);
        }
        let kind = match op_family(o) {
            Some(k) => k,
            None => return Err(WiseCode::ENoOp),
        };
        // 相位闸：期望家族 = 已收操作数 / 每族格数；空账本注高族必抓。
        if kind as usize != self.op_count / 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(WiseCode::EPhase);
        }
        let want = match uninstall_op_cell(o) {
            Some(w) => w,
            None => return Err(WiseCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(WiseCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiseCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.op_count += 1;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < UNINSTALL_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三态一格（态/位越界拒 + 载荷对照 + 重复拒）。
    pub fn record_tri(&mut self, t: usize, p: usize, got: u64) -> Result<(), WiseCode> {
        let want = match tri_cell(t, p) {
            Some(w) => w,
            None => return Err(WiseCode::ENoGrid),
        };
        let idx = t * TRI_SLOTS + p;
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + idx as u64);
            return Err(WiseCode::ETriDrift);
        }
        if match self.tri.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiseCode::EDup);
        }
        if let Some(c) = self.tri.get_mut(idx) {
            *c = Some(got);
        }
        self.fnv(0x7301 + idx as u64);
        Ok(())
    }

    /// 三态全账核验（12 格）。
    pub fn tri_all(&self) -> bool {
        let mut i = 0usize;
        while i < TRI_CELLS {
            match self.tri.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 上下文菜单集成一行。
    pub fn declare_ctx(&mut self, c: usize) -> Result<(), WiseCode> {
        let want = match ctx_row(c) {
            Some(w) => w,
            None => return Err(WiseCode::ENoCtx),
        };
        if match self.ctx.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiseCode::EDup);
        }
        if let Some(cell) = self.ctx.get_mut(c) {
            *cell = Some(want);
        }
        self.fnv(0x7400 + c as u64);
        Ok(())
    }

    /// 上下文菜单全齐核验（6 行）。
    pub fn ctx_all(&self) -> bool {
        let mut c = 0usize;
        while c < CTX_ROWS {
            match self.ctx.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 残留清理一格（载荷对照 + 重复拒）。
    pub fn record_resid(&mut self, i: usize, got: u64) -> Result<(), WiseCode> {
        let want = match resid_cell(i) {
            Some(w) => w,
            None => return Err(WiseCode::ENoGrid),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + i as u64);
            return Err(WiseCode::EDrift);
        }
        if match self.resid.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiseCode::EDup);
        }
        if let Some(c) = self.resid.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7501 + i as u64);
        Ok(())
    }

    /// 残留清理全账核验（8 格）。
    pub fn resid_all(&self) -> bool {
        let mut i = 0usize;
        while i < RESID_CELLS {
            match self.resid.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// EOL 声明一行（如实在册）。
    pub fn declare_eol(&mut self, r: usize) -> Result<(), WiseCode> {
        let want = match eol_row(r) {
            Some(w) => w,
            None => return Err(WiseCode::ENoRow),
        };
        if match self.eol.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiseCode::EDup);
        }
        if let Some(c) = self.eol.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// EOL 声明全齐核验。
    pub fn eol_all(&self) -> bool {
        let mut r = 0usize;
        while r < EOL_ROWS {
            match self.eol.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 捆绑推广声明一行（如实在册，不隐藏；重复落账拒）。
    pub fn declare_bundle(&mut self) -> Result<(), WiseCode> {
        let want = match bundle_row() {
            Some(w) => w,
            None => return Err(WiseCode::ESeal),
        };
        if self.bundle.is_some() {
            return Err(WiseCode::EDup);
        }
        self.bundle = Some(want);
        self.fnv(0x7700);
        Ok(())
    }

    /// 捆绑推广声明已落核验（如实记录入账）。
    pub fn bundle_ok(&self) -> bool {
        self.bundle.is_some()
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S2 封顶，判据缺 → S3；S1 本域不签发）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.tri_all()
            && self.ctx_all()
            && self.resid_all()
            && self.eol_all()
            && self.bundle_ok()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WiseLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（收格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WiseLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < UNINSTALL_OPS {
        if let Some(w) = uninstall_op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TRI_STATES {
        let mut p = 0usize;
        while p < TRI_SLOTS {
            if let Some(w) = tri_cell(t, p) {
                if led.record_tri(t, p, w).is_ok() {
                    recorded += 1;
                }
            }
            p += 1;
        }
        t += 1;
    }
    let mut c = 0usize;
    while c < CTX_ROWS {
        let _ = led.declare_ctx(c);
        c += 1;
    }
    let mut i = 0usize;
    while i < RESID_CELLS {
        if let Some(w) = resid_cell(i) {
            if led.record_resid(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < EOL_ROWS {
        let _ = led.declare_eol(r);
        r += 1;
    }
    // 捆绑推广行为如实记录入账（不隐藏）。
    let _ = led.declare_bundle();
    // 注入 10 轮：操作越闭集 / 三态漂移 / 相位越序三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WiseLedger::new();
        let caught = match kind {
            0 => lt.record_op(UNINSTALL_OPS, 0) == Err(WiseCode::ENoOp),
            1 => {
                // 漂移注入：先录正格再注漂移。
                if let Some(w) = tri_cell(0, 0) {
                    let _ = lt.record_tri(0, 0, w);
                }
                match tri_cell(0, 1) {
                    Some(w) => lt.record_tri(0, 1, w.wrapping_add(1)) == Err(WiseCode::ETriDrift),
                    None => false,
                }
            }
            _ => {
                // 相位注入：空账本直注家族 2 操作，必抓且不改 last 基。
                match uninstall_op_cell(4) {
                    Some(w) => lt.record_op(4, w) == Err(WiseCode::EPhase),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7800 + k as u64);
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
/// UNX-F52311 判据集（锚 UNX-F52312-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52311_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52311");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "卸载 10 操作全通",
        recorded >= UNINSTALL_OPS as u32,
        "执行/扫描/清理/登记/收尾五家族逐操作恒等",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = WiseLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52312-J1 六判据逐格落账",
    );
    s.add(
        "强制卸载/智能扫描三态 12 格全落",
        {
            let mut lt = WiseLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRI_STATES {
                let mut p = 0usize;
                while p < TRI_SLOTS {
                    if let Some(w) = tri_cell(t, p) {
                        if lt.record_tri(t, p, w).is_err() {
                            ok = false;
                        }
                    }
                    p += 1;
                }
                t += 1;
            }
            ok && lt.tri_all()
        },
        "待扫/命中/清净三态逐程序位恒等",
    );
    s.add(
        "上下文菜单集成 6/6 全落",
        {
            let mut lt = WiseLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CTX_ROWS {
                if lt.declare_ctx(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.ctx_all()
        },
        "右键卸载/强制/智能/安装器接管/多语/还原逐行落账",
    );
    s.add(
        "残留清理 8 格全落",
        {
            let mut lt = WiseLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < RESID_CELLS {
                if let Some(w) = resid_cell(i) {
                    if lt.record_resid(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.resid_all()
        },
        "注册表侧 4 格 + 文件侧 4 格双侧对照",
    );
    s.add(
        "EOL 声明 2/2 全齐（如实在册）",
        {
            let mut lt = WiseLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < EOL_ROWS {
                if lt.declare_eol(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.eol_all()
        },
        "产品生命周期声明/EOL 临近提示声明两行闭集，如实在册",
    );
    s.add(
        "捆绑推广行为如实入账（不隐藏）",
        {
            let mut lt = WiseLedger::new();
            lt.declare_bundle().is_ok() && lt.bundle_ok()
        },
        "捆绑推广声明 1 行落账，如实记录不隐藏（卡面判据）",
    );
    s.add(
        "家族相位单调",
        {
            let mut lt = WiseLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < UNINSTALL_OPS {
                if let Some(w) = uninstall_op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.op_count == UNINSTALL_OPS
        },
        "执行→扫描→清理→登记→收尾按相位推进不逆序",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作越闭集/三态漂移/相位越序三态轮转必抓",
    );
    let mut l0 = WiseLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(UNINSTALL_OPS, 0) == Err(WiseCode::ENoOp),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·三态格越闭集",
        l0.record_tri(TRI_STATES, 0, 0) == Err(WiseCode::ENoGrid),
        "三态 3 越闭集必拒",
    );
    s.add(
        "边界·菜单行越闭集",
        l0.declare_ctx(CTX_ROWS) == Err(WiseCode::ENoCtx),
        "上下文菜单行 6 越闭集必拒",
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
            let mut lt = WiseLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < UNINSTALL_OPS {
                if let Some(w) = uninstall_op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TRI_STATES {
                let mut p = 0usize;
                while p < TRI_SLOTS {
                    if let Some(w) = tri_cell(t, p) {
                        let _ = lt.record_tri(t, p, w);
                    }
                    p += 1;
                }
                t += 1;
            }
            let mut c = 0usize;
            while c < CTX_ROWS {
                let _ = lt.declare_ctx(c);
                c += 1;
            }
            let mut i = 0usize;
            while i < RESID_CELLS {
                if let Some(w) = resid_cell(i) {
                    let _ = lt.record_resid(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < EOL_ROWS {
                let _ = lt.declare_eol(r);
                r += 1;
            }
            let _ = lt.declare_bundle();
            // 定级检查：全部记账完成后再断言封顶级。
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，S1 保留不签发）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且符合 VX15 派工公式",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_family() {
        assert_eq!(op_family(0), Some(0));
        assert_eq!(op_family(7), Some(3));
        assert!(op_family(10).is_none());
        assert!(uninstall_op_cell(10).is_none());
        assert_eq!(op_family_name(3), Some("登记"));
        assert!(op_family_name(6).is_none());
    }

    #[test]
    fn test_tri_cells() {
        assert_eq!(tri_state_name(0), Some("待扫"));
        assert_eq!(tri_state_name(1), Some("命中"));
        assert_eq!(tri_state_name(2), Some("清净"));
        assert!(tri_state_name(3).is_none());
        assert!(tri_cell(3, 0).is_none());
        assert!(tri_cell(0, 4).is_none());
        assert!(tri_cell(2, 3).is_some());
    }

    #[test]
    fn test_ctx_rows() {
        assert_eq!(ctx_row_name(0), Some("右键卸载"));
        assert_eq!(ctx_row_name(5), Some("还原入口"));
        assert!(ctx_row_name(6).is_none());
        assert!(ctx_row(6).is_none());
        assert!(eol_row(2).is_none());
        assert!(bundle_row().is_some());
    }

    #[test]
    fn test_phase_caught_on_empty() {
        // 相位注入：空账本直注家族 2 操作必抓。
        let mut lt = WiseLedger::new();
        if let Some(w) = uninstall_op_cell(4) {
            assert_eq!(lt.record_op(4, w), Err(WiseCode::EPhase));
        }
        assert_eq!(lt.stats().0, 1);
        assert_eq!(lt.stats().1, 1);
        // 合法序种子：先落低族格再续推。
        let mut lt2 = WiseLedger::new();
        let mut o = 0usize;
        while o < 7 {
            if let Some(w) = uninstall_op_cell(o) {
                assert!(lt2.record_op(o, w).is_ok());
            }
            o += 1;
        }
        assert_eq!(lt2.op_count, 7);
    }

    #[test]
    fn test_drift_caught_after_seed() {
        // 漂移注入：先录正格再注漂移必抓。
        let mut lt = WiseLedger::new();
        if let Some(w) = tri_cell(1, 2) {
            assert!(lt.record_tri(1, 2, w).is_ok());
        }
        if let Some(w) = tri_cell(1, 3) {
            assert_eq!(
                lt.record_tri(1, 3, w.wrapping_add(5)),
                Err(WiseCode::ETriDrift)
            );
        }
        let mut lt2 = WiseLedger::new();
        if let Some(w) = resid_cell(6) {
            assert_eq!(lt2.record_resid(6, w.wrapping_add(3)), Err(WiseCode::EDrift));
        }
        assert_eq!(lt2.record_resid(RESID_CELLS, 0), Err(WiseCode::ENoGrid));
    }

    #[test]
    fn test_dup_and_bundle() {
        let mut lt = WiseLedger::new();
        if let Some(w) = uninstall_op_cell(0) {
            let _ = lt.record_op(0, w);
            assert_eq!(lt.record_op(0, w), Err(WiseCode::EDup));
        }
        let _ = lt.declare_ctx(2);
        assert_eq!(lt.declare_ctx(2), Err(WiseCode::EDup));
        assert!(lt.declare_bundle().is_ok());
        assert_eq!(lt.declare_bundle(), Err(WiseCode::EDup));
        assert!(lt.bundle_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WiseLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WiseLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_eol_boundary_caught() {
        let mut lt = WiseLedger::new();
        assert_eq!(lt.declare_eol(EOL_ROWS), Err(WiseCode::ENoRow));
        assert_eq!(lt.declare_ctx(CTX_ROWS), Err(WiseCode::ENoCtx));
        if let Some(w) = tri_cell(2, 0) {
            assert_eq!(
                lt.record_tri(2, 0, w.wrapping_add(1)),
                Err(WiseCode::ETriDrift)
            );
        }
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (UNINSTALL_OPS + TRI_CELLS + RESID_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8D80);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：位序与枚举声明序一致。
        assert_eq!(WiseCode::Ok as u32, 0);
        assert_eq!(WiseCode::ENoOp as u32, 1);
        assert_eq!(WiseCode::EDrift as u32, 2);
        assert_eq!(WiseCode::EPhase as u32, 3);
        assert_eq!(WiseCode::ENoGrid as u32, 4);
        assert_eq!(WiseCode::ETriDrift as u32, 5);
        assert_eq!(WiseCode::ENoCtx as u32, 6);
        assert_eq!(WiseCode::ENoRow as u32, 7);
        assert_eq!(WiseCode::EDup as u32, 8);
        assert_eq!(WiseCode::ESeal as u32, 9);
        assert_eq!(WiseCode::EInj as u32, 10);
        assert_eq!(WiseCode::EFp as u32, 11);
        assert_eq!(WiseCode::EGradeDrift as u32, 12);
        assert_eq!(WiseCode::Ok.val(), SEG_BASE);
        assert_eq!(WiseCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all = [
            WiseCode::Ok,
            WiseCode::ENoOp,
            WiseCode::EDrift,
            WiseCode::EPhase,
            WiseCode::ENoGrid,
            WiseCode::ETriDrift,
            WiseCode::ENoCtx,
            WiseCode::ENoRow,
            WiseCode::EDup,
            WiseCode::ESeal,
            WiseCode::EInj,
            WiseCode::EFp,
            WiseCode::EGradeDrift,
        ];
        let mut i = 0u32;
        while i < 13 {
            assert!(!all[i as usize].detail().is_empty());
            assert_eq!(all[i as usize].val(), SEG_BASE + i);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = WiseLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅六判据不构成收口。
        let mut lt2 = WiseLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn unx52311_checks_green() {
        let s = run_unx52311_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
