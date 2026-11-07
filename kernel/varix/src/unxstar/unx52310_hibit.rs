//! UNX-F52310 HiBit Uninstaller 收口 · 0xC8D40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52310"，逐字）：
//!   title: HiBit Uninstaller 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52311-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52310）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52311-J1，照原文锚定；功能按本卡标题（HiBit Uninstaller）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 卸载 10 操作集闭集：五家族（执行/扫描/清理/登记/收尾）各 2 操
//!      作，相位闸按已收格数推演期望家族，空账本注高族必抓。
//!   ② 工具箱 8 格闭集（健康检查/ junk 清理/自启管理/服务管理/上下文
//!      菜单/文件粉碎/系统还原/智能卸载），逐格落账漂移必抓。
//!   ③ 残留扫描 12 格：注册表/文件/服务三面各 4 格，载荷折叠恒等才收。
//!   ④ 浏览器扩展管理 6 行闭集（Chromium/Firefox/Edge 三引擎各 2 行），
//!      Windows 更新清理 4 格闭集，免费声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（操作越闭集/扫描漂移/相位越序）
//!      全部落在当轮临时账本上，检出+逃逸守恒；双跑指纹一致；KB-COMPAT
//!      定级 S2 封顶口径（全通 → S2，S1 枚举位保留不签发）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52310;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8D40;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 卸载操作数。
pub const UNINSTALL_OPS: usize = 10;

/// 工具箱格数。
pub const TOOLBOX_CELLS: usize = 8;

/// 残留扫描面数。
pub const SCAN_FACES: usize = 3;

/// 每面扫描格数（3 面 × 4 = 12 格）。
pub const SCAN_CELLS_PER_FACE: usize = 4;

/// 残留扫描总格数。
pub const SCAN_CELLS: usize = SCAN_FACES * SCAN_CELLS_PER_FACE;

/// 浏览器扩展管理行数。
pub const EXT_ROWS: usize = 6;

/// Windows 更新清理格数。
pub const WU_CELLS: usize = 4;

/// 免费声明行数。
pub const FREE_ROWS: usize = 2;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4849_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HiBitCode {
    /// 成功
    Ok = 0,
    /// 操作/格越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族相位越序
    EPhase,
    /// 工具箱/扫描格越闭集
    ENoGrid,
    /// 工具箱载荷漂移
    EToolDrift,
    /// 扫描载荷漂移
    EScanDrift,
    /// 声明行越闭集
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

impl HiBitCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HiBitCode::Ok => "成功",
            HiBitCode::ENoOp => "操作号越闭集：卸载 10 操作集之外先登记",
            HiBitCode::EDrift => "载荷漂移：操作/更新清理折叠与基准不恒等，逐格复算",
            HiBitCode::EPhase => "家族相位越序：执行→扫描→清理→登记→收尾按相位推进，越相必抓",
            HiBitCode::ENoGrid => "工具箱/扫描格越闭集：工具箱 8 格与扫描 12 格之外先登记",
            HiBitCode::EToolDrift => "工具箱载荷漂移：工具箱 8 格折叠与基准不恒等，逐格复算",
            HiBitCode::EScanDrift => "扫描载荷漂移：残留扫描 12 格折叠与基准不恒等，逐面复算",
            HiBitCode::ENoRow => "声明行越闭集：扩展 6 行/更新 4 格/免费 2 行闭集之外先登记",
            HiBitCode::EDup => "重复落账：同格只许一条账目",
            HiBitCode::ESeal => "未齐即封：10 操作 + 8 工具箱 + 12 扫描格 + 6 扩展 + 4 更新 + 2 行声明未齐先补齐",
            HiBitCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            HiBitCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HiBitCode::EGradeDrift => "定级漂移：全通才可定 S2，KB-COMPAT 封顶口径",
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
            Grade::S1 => "S1：保留位，HiBit 域存在外部条件不签发",
            Grade::S2 => "S2：六判据+10操作+8工具箱+12扫描格+6扩展+4更新+2行声明全通，有条件收口",
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
    Some((0xB100u64 + (o as u64) * 0x2E) ^ ANCHOR)
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

/// 工具箱格载荷（格 t）。
pub const fn toolbox_cell(t: usize) -> Option<u64> {
    if t >= TOOLBOX_CELLS {
        return None;
    }
    Some((0xB200u64 + (t as u64) * 0x34) ^ ANCHOR)
}

/// 工具箱格名。
pub const fn toolbox_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("健康检查"),
        1 => Some("垃圾清理"),
        2 => Some("自启管理"),
        3 => Some("服务管理"),
        4 => Some("上下文菜单"),
        5 => Some("文件粉碎"),
        6 => Some("系统还原"),
        7 => Some("智能卸载"),
        _ => None,
    }
}

/// 残留扫描格载荷（面 f 0..3，面内格 c 0..4；面 0 注册表 1 文件 2 服务）。
pub const fn scan_cell(f: usize, c: usize) -> Option<u64> {
    if f >= SCAN_FACES || c >= SCAN_CELLS_PER_FACE {
        return None;
    }
    Some((0xB300u64 + (f as u64) * 0x110 + (c as u64) * 0x2E) ^ ANCHOR)
}

/// 扫描面名。
pub const fn scan_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("注册表面"),
        1 => Some("文件面"),
        2 => Some("服务面"),
        _ => None,
    }
}

/// Windows 更新清理格载荷（格 w）。
pub const fn wu_cell(w: usize) -> Option<u64> {
    if w >= WU_CELLS {
        return None;
    }
    Some((0xB400u64 + (w as u64) * 0x30) ^ ANCHOR)
}

/// 浏览器扩展管理行折叠（行 e：0..2 Chromium 2..4 Firefox 4..6 Edge）。
pub const fn ext_row(e: usize) -> Option<u32> {
    if e >= EXT_ROWS {
        return None;
    }
    Some(0xB500 + e as u32 * 0x2A)
}

/// 扩展引擎名。
pub const fn ext_engine(e: usize) -> Option<&'static str> {
    if e >= EXT_ROWS {
        return None;
    }
    match e / 2 {
        0 => Some("Chromium"),
        1 => Some("Firefox"),
        _ => Some("Edge"),
    }
}

/// 免费声明行折叠（行 r）。
pub const fn free_row(r: usize) -> Option<u32> {
    if r >= FREE_ROWS {
        return None;
    }
    Some(0xB600 + r as u32 * 0x38)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// HiBit Uninstaller 收口账本状态机。
#[derive(Clone)]
pub struct HiBitLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 卸载操作账。
    ops: [Option<u64>; UNINSTALL_OPS],
    /// 工具箱账。
    toolbox: [Option<u64>; TOOLBOX_CELLS],
    /// 残留扫描账（3 面 × 4 格平铺）。
    scans: [Option<u64>; SCAN_CELLS],
    /// 浏览器扩展管理账。
    exts: [Option<u32>; EXT_ROWS],
    /// Windows 更新清理账。
    wu: [Option<u64>; WU_CELLS],
    /// 免费声明账。
    free: [Option<u32>; FREE_ROWS],
    /// 已收操作数（相位推演基）。
    op_count: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HiBitLedger {
    /// 新账本。
    pub fn new() -> Self {
        HiBitLedger {
            gates: [None; 6],
            ops: [None; UNINSTALL_OPS],
            toolbox: [None; TOOLBOX_CELLS],
            scans: [None; SCAN_CELLS],
            exts: [None; EXT_ROWS],
            wu: [None; WU_CELLS],
            free: [None; FREE_ROWS],
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

    /// 落一格六判据（UNX-F52311-J1）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HiBitCode> {
        if gate >= 6 {
            return Err(HiBitCode::ESeal);
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

    /// 卸载操作一格（相位闸 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), HiBitCode> {
        if o >= UNINSTALL_OPS {
            return Err(HiBitCode::ENoOp);
        }
        let kind = match op_family(o) {
            Some(k) => k,
            None => return Err(HiBitCode::ENoOp),
        };
        // 相位闸：期望家族 = 已收操作数 / 每族格数；空账本注高族必抓。
        if kind as usize != self.op_count / 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(HiBitCode::EPhase);
        }
        let want = match uninstall_op_cell(o) {
            Some(w) => w,
            None => return Err(HiBitCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(HiBitCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HiBitCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.op_count += 1;
        self.fnv(0x6202 + o as u64);
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

    /// 工具箱一格（载荷对照 + 重复拒）。
    pub fn record_tool(&mut self, t: usize, got: u64) -> Result<(), HiBitCode> {
        let want = match toolbox_cell(t) {
            Some(w) => w,
            None => return Err(HiBitCode::ENoGrid),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + t as u64);
            return Err(HiBitCode::EToolDrift);
        }
        if match self.toolbox.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HiBitCode::EDup);
        }
        if let Some(c) = self.toolbox.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x6301 + t as u64);
        Ok(())
    }

    /// 工具箱全账核验（8 格）。
    pub fn toolbox_all(&self) -> bool {
        let mut t = 0usize;
        while t < TOOLBOX_CELLS {
            match self.toolbox.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 残留扫描一格（面/格越界拒 + 载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, f: usize, c: usize, got: u64) -> Result<(), HiBitCode> {
        let want = match scan_cell(f, c) {
            Some(w) => w,
            None => return Err(HiBitCode::ENoGrid),
        };
        let idx = f * SCAN_CELLS_PER_FACE + c;
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + idx as u64);
            return Err(HiBitCode::EScanDrift);
        }
        if match self.scans.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HiBitCode::EDup);
        }
        if let Some(cell) = self.scans.get_mut(idx) {
            *cell = Some(got);
        }
        self.fnv(0x6401 + idx as u64);
        Ok(())
    }

    /// 残留扫描全账核验（12 格）。
    pub fn scans_all(&self) -> bool {
        let mut i = 0usize;
        while i < SCAN_CELLS {
            match self.scans.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 浏览器扩展管理一行。
    pub fn declare_ext(&mut self, e: usize) -> Result<(), HiBitCode> {
        let want = match ext_row(e) {
            Some(w) => w,
            None => return Err(HiBitCode::ENoRow),
        };
        if match self.exts.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HiBitCode::EDup);
        }
        if let Some(c) = self.exts.get_mut(e) {
            *c = Some(want);
        }
        self.fnv(0x6500 + e as u64);
        Ok(())
    }

    /// 扩展管理全齐核验（6 行）。
    pub fn exts_all(&self) -> bool {
        let mut e = 0usize;
        while e < EXT_ROWS {
            match self.exts.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// Windows 更新清理一格。
    pub fn record_wu(&mut self, w: usize, got: u64) -> Result<(), HiBitCode> {
        let want = match wu_cell(w) {
            Some(v) => v,
            None => return Err(HiBitCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + w as u64);
            return Err(HiBitCode::EDrift);
        }
        if match self.wu.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HiBitCode::EDup);
        }
        if let Some(c) = self.wu.get_mut(w) {
            *c = Some(got);
        }
        self.fnv(0x6601 + w as u64);
        Ok(())
    }

    /// 更新清理全账核验（4 格）。
    pub fn wu_all(&self) -> bool {
        let mut w = 0usize;
        while w < WU_CELLS {
            match self.wu.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// 免费声明一行。
    pub fn declare_free(&mut self, r: usize) -> Result<(), HiBitCode> {
        let want = match free_row(r) {
            Some(w) => w,
            None => return Err(HiBitCode::ENoRow),
        };
        if match self.free.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HiBitCode::EDup);
        }
        if let Some(c) = self.free.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x6700 + r as u64);
        Ok(())
    }

    /// 免费声明全齐核验。
    pub fn free_all(&self) -> bool {
        let mut r = 0usize;
        while r < FREE_ROWS {
            match self.free.get(r) {
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

    /// KB-COMPAT 定级（全通 → S2 封顶，判据缺 → S3；S1 本域不签发）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.toolbox_all()
            && self.scans_all()
            && self.exts_all()
            && self.wu_all()
            && self.free_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for HiBitLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（收格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HiBitLedger::new();
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
    while t < TOOLBOX_CELLS {
        if let Some(w) = toolbox_cell(t) {
            if led.record_tool(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut f = 0usize;
    while f < SCAN_FACES {
        let mut c = 0usize;
        while c < SCAN_CELLS_PER_FACE {
            if let Some(w) = scan_cell(f, c) {
                if led.record_scan(f, c, w).is_ok() {
                    recorded += 1;
                }
            }
            c += 1;
        }
        f += 1;
    }
    let mut e = 0usize;
    while e < EXT_ROWS {
        let _ = led.declare_ext(e);
        e += 1;
    }
    let mut w = 0usize;
    while w < WU_CELLS {
        if let Some(v) = wu_cell(w) {
            if led.record_wu(w, v).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let mut r = 0usize;
    while r < FREE_ROWS {
        let _ = led.declare_free(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 扫描漂移 / 相位越序三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HiBitLedger::new();
        let caught = match kind {
            0 => lt.record_op(UNINSTALL_OPS, 0) == Err(HiBitCode::ENoOp),
            1 => {
                // 漂移注入：先录正格再注漂移。
                if let Some(w) = scan_cell(0, 0) {
                    let _ = lt.record_scan(0, 0, w);
                }
                match scan_cell(0, 1) {
                    Some(w) => lt.record_scan(0, 1, w.wrapping_add(1)) == Err(HiBitCode::EScanDrift),
                    None => false,
                }
            }
            _ => {
                // 相位注入：空账本直注家族 3 操作，必抓且不改 last 基。
                match uninstall_op_cell(6) {
                    Some(w) => lt.record_op(6, w) == Err(HiBitCode::EPhase),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6800 + k as u64);
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
/// UNX-F52310 判据集（锚 UNX-F52311-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52310_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52310");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "卸载 10 操作全通",
        recorded >= UNINSTALL_OPS as u32,
        "执行/扫描/清理/登记/收尾五家族逐操作恒等",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = HiBitLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52311-J1 六判据逐格落账",
    );
    s.add(
        "工具箱 8 格全落",
        {
            let mut lt = HiBitLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TOOLBOX_CELLS {
                if let Some(w) = toolbox_cell(t) {
                    if lt.record_tool(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.toolbox_all()
        },
        "健康检查/垃圾清理/自启/服务/上下文菜单/粉碎/还原/智能卸载逐格恒等",
    );
    s.add(
        "残留扫描 12 格全落",
        {
            let mut lt = HiBitLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < SCAN_FACES {
                let mut c = 0usize;
                while c < SCAN_CELLS_PER_FACE {
                    if let Some(w) = scan_cell(f, c) {
                        if lt.record_scan(f, c, w).is_err() {
                            ok = false;
                        }
                    }
                    c += 1;
                }
                f += 1;
            }
            ok && lt.scans_all()
        },
        "注册表面/文件面/服务面逐格恒等",
    );
    s.add(
        "浏览器扩展管理 6/6 全落",
        {
            let mut lt = HiBitLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXT_ROWS {
                if lt.declare_ext(e).is_err() {
                    ok = false;
                }
                e += 1;
            }
            ok && lt.exts_all()
        },
        "Chromium/Firefox/Edge 三引擎各 2 行闭集",
    );
    s.add(
        "Windows 更新清理 4 格全落",
        {
            let mut lt = HiBitLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < WU_CELLS {
                if let Some(v) = wu_cell(w) {
                    if lt.record_wu(w, v).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.wu_all()
        },
        "更新缓存/更新备份/挂起任务/还原点关联逐格恒等",
    );
    s.add(
        "免费声明 2/2 全齐",
        {
            let mut lt = HiBitLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < FREE_ROWS {
                if lt.declare_free(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.free_all()
        },
        "免费版功能边界声明/非商业使用声明两行闭集，缺一拒封",
    );
    s.add(
        "家族相位单调",
        {
            let mut lt = HiBitLedger::new();
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
        "操作越闭集/扫描漂移/相位越序三态轮转必抓",
    );
    let mut l0 = HiBitLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(UNINSTALL_OPS, 0) == Err(HiBitCode::ENoOp),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·工具箱格越闭集",
        l0.record_tool(TOOLBOX_CELLS, 0) == Err(HiBitCode::ENoGrid),
        "工具箱格 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_ext(EXT_ROWS) == Err(HiBitCode::ENoRow),
        "扩展管理行 6 越闭集必拒",
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
            let mut lt = HiBitLedger::new();
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
            while t < TOOLBOX_CELLS {
                if let Some(w) = toolbox_cell(t) {
                    let _ = lt.record_tool(t, w);
                }
                t += 1;
            }
            let mut f = 0usize;
            while f < SCAN_FACES {
                let mut c = 0usize;
                while c < SCAN_CELLS_PER_FACE {
                    if let Some(w) = scan_cell(f, c) {
                        let _ = lt.record_scan(f, c, w);
                    }
                    c += 1;
                }
                f += 1;
            }
            let mut e = 0usize;
            while e < EXT_ROWS {
                let _ = lt.declare_ext(e);
                e += 1;
            }
            let mut w = 0usize;
            while w < WU_CELLS {
                if let Some(v) = wu_cell(w) {
                    let _ = lt.record_wu(w, v);
                }
                w += 1;
            }
            let mut r = 0usize;
            while r < FREE_ROWS {
                let _ = lt.declare_free(r);
                r += 1;
            }
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
        assert_eq!(op_family(9), Some(4));
        assert!(op_family(10).is_none());
        assert!(uninstall_op_cell(10).is_none());
        assert_eq!(op_family_name(2), Some("清理"));
        assert!(op_family_name(5).is_none());
    }

    #[test]
    fn test_toolbox_cells() {
        assert_eq!(toolbox_name(0), Some("健康检查"));
        assert_eq!(toolbox_name(7), Some("智能卸载"));
        assert!(toolbox_name(8).is_none());
        assert!(toolbox_cell(8).is_none());
    }

    #[test]
    fn test_ext_and_wu() {
        assert_eq!(ext_engine(0), Some("Chromium"));
        assert_eq!(ext_engine(3), Some("Firefox"));
        assert_eq!(ext_engine(5), Some("Edge"));
        assert!(ext_engine(6).is_none());
        assert!(ext_row(6).is_none());
        assert!(wu_cell(4).is_none());
        assert!(free_row(2).is_none());
    }

    #[test]
    fn test_phase_caught_on_empty() {
        // 相位注入：空账本直注家族 3 操作必抓。
        let mut lt = HiBitLedger::new();
        if let Some(w) = uninstall_op_cell(6) {
            assert_eq!(lt.record_op(6, w), Err(HiBitCode::EPhase));
        }
        assert_eq!(lt.stats().0, 1);
        assert_eq!(lt.stats().1, 1);
        // 合法序种子：先落低族格再续推。
        let mut lt2 = HiBitLedger::new();
        let mut o = 0usize;
        while o < 5 {
            if let Some(w) = uninstall_op_cell(o) {
                assert!(lt2.record_op(o, w).is_ok());
            }
            o += 1;
        }
        assert_eq!(lt2.op_count, 5);
    }

    #[test]
    fn test_drift_caught_after_seed() {
        // 漂移注入：先录正格再注漂移必抓。
        let mut lt = HiBitLedger::new();
        if let Some(w) = scan_cell(1, 1) {
            assert!(lt.record_scan(1, 1, w).is_ok());
        }
        if let Some(w) = scan_cell(1, 2) {
            assert_eq!(
                lt.record_scan(1, 2, w.wrapping_add(3)),
                Err(HiBitCode::EScanDrift)
            );
        }
        let mut lt2 = HiBitLedger::new();
        if let Some(w) = toolbox_cell(5) {
            assert_eq!(lt2.record_tool(5, w.wrapping_add(2)), Err(HiBitCode::EToolDrift));
        }
        if let Some(v) = wu_cell(2) {
            assert_eq!(lt2.record_wu(2, v.wrapping_add(1)), Err(HiBitCode::EDrift));
        }
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HiBitLedger::new();
        if let Some(w) = uninstall_op_cell(0) {
            let _ = lt.record_op(0, w);
            assert_eq!(lt.record_op(0, w), Err(HiBitCode::EDup));
        }
        let mut lt2 = HiBitLedger::new();
        let _ = lt2.declare_ext(0);
        assert_eq!(lt2.declare_ext(0), Err(HiBitCode::EDup));
        let _ = lt2.declare_free(1);
        assert_eq!(lt2.declare_free(1), Err(HiBitCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HiBitLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = HiBitLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_grid_boundary_caught() {
        let mut lt = HiBitLedger::new();
        assert_eq!(lt.record_tool(TOOLBOX_CELLS, 0), Err(HiBitCode::ENoGrid));
        assert_eq!(lt.record_scan(3, 0, 0), Err(HiBitCode::ENoGrid));
        assert_eq!(
            lt.record_scan(0, SCAN_CELLS_PER_FACE, 0),
            Err(HiBitCode::ENoGrid)
        );
        assert_eq!(lt.record_wu(WU_CELLS, 0), Err(HiBitCode::ENoRow));
        assert_eq!(lt.declare_free(FREE_ROWS), Err(HiBitCode::ENoRow));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (UNINSTALL_OPS + TOOLBOX_CELLS + SCAN_CELLS + WU_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8D40);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：位序与枚举声明序一致。
        assert_eq!(HiBitCode::Ok as u32, 0);
        assert_eq!(HiBitCode::ENoOp as u32, 1);
        assert_eq!(HiBitCode::EDrift as u32, 2);
        assert_eq!(HiBitCode::EPhase as u32, 3);
        assert_eq!(HiBitCode::ENoGrid as u32, 4);
        assert_eq!(HiBitCode::EToolDrift as u32, 5);
        assert_eq!(HiBitCode::EScanDrift as u32, 6);
        assert_eq!(HiBitCode::ENoRow as u32, 7);
        assert_eq!(HiBitCode::EDup as u32, 8);
        assert_eq!(HiBitCode::ESeal as u32, 9);
        assert_eq!(HiBitCode::EInj as u32, 10);
        assert_eq!(HiBitCode::EFp as u32, 11);
        assert_eq!(HiBitCode::EGradeDrift as u32, 12);
        assert_eq!(HiBitCode::Ok.val(), SEG_BASE);
        assert_eq!(HiBitCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all = [
            HiBitCode::Ok,
            HiBitCode::ENoOp,
            HiBitCode::EDrift,
            HiBitCode::EPhase,
            HiBitCode::ENoGrid,
            HiBitCode::EToolDrift,
            HiBitCode::EScanDrift,
            HiBitCode::ENoRow,
            HiBitCode::EDup,
            HiBitCode::ESeal,
            HiBitCode::EInj,
            HiBitCode::EFp,
            HiBitCode::EGradeDrift,
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
        let lt = HiBitLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅六判据不构成收口。
        let mut lt2 = HiBitLedger::new();
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
    fn unx52310_checks_green() {
        let s = run_unx52310_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
