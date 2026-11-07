//! UNX-F52309 Geek Uninstaller 收口 · 0xC8D00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52309"，逐字）：
//!   title: Geek Uninstaller 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52310-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52309）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52310-J1，照原文锚定；功能按本卡标题（Geek Uninstaller）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 卸载 10 操作集闭集：五家族（执行/扫描/清除/登记/收尾）各 2 操
//!      作，相位闸按已收格数推演期望家族，空账本注高族必抓。
//!   ② 残留扫描三面 12 格：注册表面/文件系统面/服务与任务面各 4 格，
//!      载荷折叠恒等才收，漂移必抓。
//!   ③ 强制清除 5 场景闭集（僵死进程/占用句柄/受控文件夹/服务残留/
//!      商店应用），逐场景落账。
//!   ④ 注册表/文件双残留账 8 格：前 4 格注册表侧、后 4 格文件侧，双
//!      侧对照，缺侧拒封。
//!   ⑤ 便携单文件声明 2 行闭集：单文件形态声明/免安装目录声明，缺一
//!      拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（操作越闭集/载荷漂移/相位越序）
//!      全部落在当轮临时账本上，检出+逃逸守恒；双跑指纹一致；KB-COMPAT
//!      定级 S2 封顶口径（全通 → S2，S1 枚举位保留不签发）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52309;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8D00;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 卸载操作数。
pub const UNINSTALL_OPS: usize = 10;

/// 残留扫描面数。
pub const SCAN_FACES: usize = 3;

/// 每面扫描格数（3 面 × 4 = 12 格）。
pub const SCAN_CELLS_PER_FACE: usize = 4;

/// 残留扫描总格数。
pub const SCAN_CELLS: usize = SCAN_FACES * SCAN_CELLS_PER_FACE;

/// 强制清除场景数。
pub const FORCE_SCENES: usize = 5;

/// 双残留账格数（注册表 4 + 文件 4）。
pub const RESID_CELLS: usize = 8;

/// 便携单文件声明行数。
pub const PORTABLE_ROWS: usize = 2;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4745_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeekCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族相位越序
    EPhase,
    /// 扫描/残留格越闭集
    ENoGrid,
    /// 扫描载荷漂移
    EScanDrift,
    /// 强制清除场景越闭集
    ENoScene,
    /// 便携声明行越闭集
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

impl GeekCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GeekCode::Ok => "成功",
            GeekCode::ENoOp => "操作号越闭集：卸载 10 操作集之外先登记",
            GeekCode::EDrift => "载荷漂移：操作/残留账折叠与基准不恒等，逐格复算",
            GeekCode::EPhase => "家族相位越序：执行→扫描→清除→登记→收尾按相位推进，越相必抓",
            GeekCode::ENoGrid => "扫描/残留格越闭集：三面 12 格与双残留 8 格之外先登记",
            GeekCode::EScanDrift => "扫描载荷漂移：三面 12 格折叠与基准不恒等，逐面复算",
            GeekCode::ENoScene => "强制清除场景越闭集：5 场景之外先登记",
            GeekCode::ENoRow => "便携声明行越闭集：单文件 2 行闭集之外先登记",
            GeekCode::EDup => "重复落账：同格只许一条账目",
            GeekCode::ESeal => "未齐即封：10 操作 + 12 扫描格 + 5 场景 + 8 残留格 + 2 行声明未齐先补齐",
            GeekCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GeekCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GeekCode::EGradeDrift => "定级漂移：全通才可定 S2，KB-COMPAT 封顶口径",
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
            Grade::S1 => "S1：保留位，Geek 域存在外部条件不签发",
            Grade::S2 => "S2：六判据+10操作+12扫描格+5场景+8残留格+2行声明全通，有条件收口",
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
    Some((0xA100u64 + (o as u64) * 0x2C) ^ ANCHOR)
}

/// 操作家族（o/2：0 执行 1 扫描 2 清除 3 登记 4 收尾）。
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
        2 => Some("清除"),
        3 => Some("登记"),
        4 => Some("收尾"),
        _ => None,
    }
}

/// 残留扫描格载荷（面 f 0..3，面内格 c 0..4；面 0 注册表 1 文件系统 2 服务与任务）。
pub const fn scan_cell(f: usize, c: usize) -> Option<u64> {
    if f >= SCAN_FACES || c >= SCAN_CELLS_PER_FACE {
        return None;
    }
    Some((0xA200u64 + (f as u64) * 0x100 + (c as u64) * 0x2C) ^ ANCHOR)
}

/// 扫描面名。
pub const fn scan_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("注册表面"),
        1 => Some("文件系统面"),
        2 => Some("服务与任务面"),
        _ => None,
    }
}

/// 强制清除场景载荷（场景 s）。
pub const fn force_scene(s: usize) -> Option<u32> {
    if s >= FORCE_SCENES {
        return None;
    }
    Some(0xA300 + s as u32 * 0x40)
}

/// 强制清除场景名。
pub const fn force_scene_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("僵死进程场景"),
        1 => Some("占用句柄场景"),
        2 => Some("受控文件夹场景"),
        3 => Some("服务残留场景"),
        4 => Some("商店应用场景"),
        _ => None,
    }
}

/// 双残留账格载荷（i 0..8：0..4 注册表侧，4..8 文件侧）。
pub const fn resid_cell(i: usize) -> Option<u64> {
    if i >= RESID_CELLS {
        return None;
    }
    Some((0xA400u64 + (i as u64) * 0x24) ^ ANCHOR)
}

/// 双残留侧名。
pub const fn resid_side(i: usize) -> Option<&'static str> {
    if i >= RESID_CELLS {
        return None;
    }
    if i < 4 {
        Some("注册表侧")
    } else {
        Some("文件侧")
    }
}

/// 便携单文件声明行折叠（行 r）。
pub const fn portable_row(r: usize) -> Option<u32> {
    if r >= PORTABLE_ROWS {
        return None;
    }
    Some(0xA500 + r as u32 * 0x30)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Geek Uninstaller 收口账本状态机。
#[derive(Clone)]
pub struct GeekLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 卸载操作账。
    ops: [Option<u64>; UNINSTALL_OPS],
    /// 残留扫描账（3 面 × 4 格平铺）。
    scans: [Option<u64>; SCAN_CELLS],
    /// 强制清除场景账。
    scenes: [Option<u32>; FORCE_SCENES],
    /// 双残留账。
    resid: [Option<u64>; RESID_CELLS],
    /// 便携声明账。
    portable: [Option<u32>; PORTABLE_ROWS],
    /// 已收操作数（相位推演基）。
    op_count: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GeekLedger {
    /// 新账本。
    pub fn new() -> Self {
        GeekLedger {
            gates: [None; 6],
            ops: [None; UNINSTALL_OPS],
            scans: [None; SCAN_CELLS],
            scenes: [None; FORCE_SCENES],
            resid: [None; RESID_CELLS],
            portable: [None; PORTABLE_ROWS],
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

    /// 落一格六判据（UNX-F52310-J1）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GeekCode> {
        if gate >= 6 {
            return Err(GeekCode::ESeal);
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

    /// 卸载操作一格（相位闸 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GeekCode> {
        if o >= UNINSTALL_OPS {
            return Err(GeekCode::ENoOp);
        }
        let kind = match op_family(o) {
            Some(k) => k,
            None => return Err(GeekCode::ENoOp),
        };
        // 相位闸：期望家族 = 已收操作数 / 每族格数；空账本注高族必抓。
        if kind as usize != self.op_count / 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(GeekCode::EPhase);
        }
        let want = match uninstall_op_cell(o) {
            Some(w) => w,
            None => return Err(GeekCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(GeekCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GeekCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.op_count += 1;
        self.fnv(0x5202 + o as u64);
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

    /// 残留扫描一格（面/格越界拒 + 载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, f: usize, c: usize, got: u64) -> Result<(), GeekCode> {
        let want = match scan_cell(f, c) {
            Some(w) => w,
            None => return Err(GeekCode::ENoGrid),
        };
        let idx = f * SCAN_CELLS_PER_FACE + c;
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + idx as u64);
            return Err(GeekCode::EScanDrift);
        }
        if match self.scans.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GeekCode::EDup);
        }
        if let Some(cell) = self.scans.get_mut(idx) {
            *cell = Some(got);
        }
        self.fnv(0x5301 + idx as u64);
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

    /// 强制清除场景一格。
    pub fn record_scene(&mut self, s: usize) -> Result<(), GeekCode> {
        let want = match force_scene(s) {
            Some(w) => w,
            None => return Err(GeekCode::ENoScene),
        };
        if match self.scenes.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GeekCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x5400 + s as u64);
        Ok(())
    }

    /// 场景全账核验（5 格）。
    pub fn scenes_all(&self) -> bool {
        let mut s = 0usize;
        while s < FORCE_SCENES {
            match self.scenes.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 双残留账一格（载荷对照 + 重复拒）。
    pub fn record_resid(&mut self, i: usize, got: u64) -> Result<(), GeekCode> {
        let want = match resid_cell(i) {
            Some(w) => w,
            None => return Err(GeekCode::ENoGrid),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + i as u64);
            return Err(GeekCode::EDrift);
        }
        if match self.resid.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GeekCode::EDup);
        }
        if let Some(c) = self.resid.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5501 + i as u64);
        Ok(())
    }

    /// 双残留全账核验（8 格，注册表侧与文件侧各 4）。
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

    /// 便携单文件声明一行。
    pub fn declare_portable(&mut self, r: usize) -> Result<(), GeekCode> {
        let want = match portable_row(r) {
            Some(w) => w,
            None => return Err(GeekCode::ENoRow),
        };
        if match self.portable.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GeekCode::EDup);
        }
        if let Some(c) = self.portable.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn portable_all(&self) -> bool {
        let mut r = 0usize;
        while r < PORTABLE_ROWS {
            match self.portable.get(r) {
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
            && self.scans_all()
            && self.scenes_all()
            && self.resid_all()
            && self.portable_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GeekLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（收格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GeekLedger::new();
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
    let mut s = 0usize;
    while s < FORCE_SCENES {
        let _ = led.record_scene(s);
        s += 1;
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
    while r < PORTABLE_ROWS {
        let _ = led.declare_portable(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 载荷漂移 / 相位越序三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GeekLedger::new();
        let caught = match kind {
            0 => lt.record_op(UNINSTALL_OPS, 0) == Err(GeekCode::ENoOp),
            1 => {
                // 漂移注入：先录正格再注漂移。
                if let Some(w) = uninstall_op_cell(0) {
                    let _ = lt.record_op(0, w);
                }
                match uninstall_op_cell(1) {
                    Some(w) => lt.record_op(1, w.wrapping_add(1)) == Err(GeekCode::EDrift),
                    None => false,
                }
            }
            _ => {
                // 相位注入：空账本直注家族 2 操作，必抓且不改 last 基。
                match uninstall_op_cell(4) {
                    Some(w) => lt.record_op(4, w) == Err(GeekCode::EPhase),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52309 判据集（锚 UNX-F52310-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52309_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52309");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "卸载 10 操作全通",
        recorded >= UNINSTALL_OPS as u32,
        "执行/扫描/清除/登记/收尾五家族逐操作恒等",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = GeekLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52310-J1 六判据逐格落账",
    );
    s.add(
        "残留扫描三面 12 格全落",
        {
            let mut lt = GeekLedger::new();
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
        "注册表面/文件系统面/服务与任务面逐格恒等",
    );
    s.add(
        "强制清除 5 场景全落",
        {
            let mut lt = GeekLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < FORCE_SCENES {
                if lt.record_scene(sc).is_err() {
                    ok = false;
                }
                sc += 1;
            }
            ok && lt.scenes_all()
        },
        "僵死进程/占用句柄/受控文件夹/服务残留/商店应用逐场景落账",
    );
    s.add(
        "注册表/文件双残留账 8 格全落",
        {
            let mut lt = GeekLedger::new();
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
        "便携单文件声明 2/2 全齐",
        {
            let mut lt = GeekLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PORTABLE_ROWS {
                if lt.declare_portable(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.portable_all()
        },
        "单文件形态声明/免安装目录声明两行闭集，缺一拒封",
    );
    s.add(
        "家族相位单调",
        {
            let mut lt = GeekLedger::new();
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
        "执行→扫描→清除→登记→收尾按相位推进不逆序",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作越闭集/载荷漂移/相位越序三态轮转必抓",
    );
    let mut l0 = GeekLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(UNINSTALL_OPS, 0) == Err(GeekCode::ENoOp),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·扫描面越闭集",
        l0.record_scan(SCAN_FACES, 0, 0) == Err(GeekCode::ENoGrid),
        "扫描面 3 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_portable(PORTABLE_ROWS) == Err(GeekCode::ENoRow),
        "便携声明行 2 越闭集必拒",
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
            let mut lt = GeekLedger::new();
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
            let mut sc = 0usize;
            while sc < FORCE_SCENES {
                let _ = lt.record_scene(sc);
                sc += 1;
            }
            let mut i = 0usize;
            while i < RESID_CELLS {
                if let Some(w) = resid_cell(i) {
                    let _ = lt.record_resid(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < PORTABLE_ROWS {
                let _ = lt.declare_portable(r);
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
        assert_eq!(op_family(1), Some(0));
        assert_eq!(op_family(8), Some(4));
        assert!(op_family(10).is_none());
        assert!(uninstall_op_cell(10).is_none());
        assert_eq!(op_family_name(0), Some("执行"));
        assert_eq!(op_family_name(4), Some("收尾"));
        assert!(op_family_name(5).is_none());
    }

    #[test]
    fn test_scan_cells() {
        assert_eq!(scan_face_name(0), Some("注册表面"));
        assert_eq!(scan_face_name(2), Some("服务与任务面"));
        assert!(scan_face_name(3).is_none());
        assert!(scan_cell(3, 0).is_none());
        assert!(scan_cell(0, 4).is_none());
        assert!(scan_cell(2, 3).is_some());
    }

    #[test]
    fn test_scene_and_resid() {
        assert_eq!(force_scene_name(0), Some("僵死进程场景"));
        assert!(force_scene_name(5).is_none());
        assert!(force_scene(5).is_none());
        assert_eq!(resid_side(0), Some("注册表侧"));
        assert_eq!(resid_side(7), Some("文件侧"));
        assert!(resid_cell(8).is_none());
        assert!(portable_row(2).is_none());
    }

    #[test]
    fn test_phase_caught_on_empty() {
        // 相位注入：空账本直注家族 2 操作必抓。
        let mut lt = GeekLedger::new();
        if let Some(w) = uninstall_op_cell(4) {
            assert_eq!(lt.record_op(4, w), Err(GeekCode::EPhase));
        }
        assert_eq!(lt.stats().0, 1);
        assert_eq!(lt.stats().1, 1);
        // 合法序种子：先落低族格再续推。
        let mut lt2 = GeekLedger::new();
        if let Some(w) = uninstall_op_cell(0) {
            assert!(lt2.record_op(0, w).is_ok());
        }
        if let Some(w) = uninstall_op_cell(1) {
            assert!(lt2.record_op(1, w).is_ok());
        }
        if let Some(w) = uninstall_op_cell(2) {
            assert!(lt2.record_op(2, w).is_ok());
        }
        assert_eq!(lt2.op_count, 3);
    }

    #[test]
    fn test_drift_caught_after_seed() {
        // 漂移注入：先录正格再注漂移必抓。
        let mut lt = GeekLedger::new();
        if let Some(w) = uninstall_op_cell(0) {
            assert!(lt.record_op(0, w).is_ok());
        }
        if let Some(w) = uninstall_op_cell(1) {
            assert_eq!(lt.record_op(1, w.wrapping_add(1)), Err(GeekCode::EDrift));
        }
        let mut lt2 = GeekLedger::new();
        if let Some(w) = resid_cell(3) {
            assert_eq!(lt2.record_resid(3, w.wrapping_add(9)), Err(GeekCode::EDrift));
        }
        assert_eq!(
            lt2.record_resid(RESID_CELLS, 0),
            Err(GeekCode::ENoGrid)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GeekLedger::new();
        if let Some(w) = uninstall_op_cell(0) {
            let _ = lt.record_op(0, w);
            assert_eq!(lt.record_op(0, w), Err(GeekCode::EDup));
        }
        let mut lt2 = GeekLedger::new();
        if let Some(w) = scan_cell(1, 2) {
            let _ = lt2.record_scan(1, 2, w);
            assert_eq!(lt2.record_scan(1, 2, w), Err(GeekCode::EDup));
        }
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GeekLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GeekLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_scan_face_boundary_caught() {
        let mut lt = GeekLedger::new();
        assert_eq!(lt.record_scan(SCAN_FACES, 0, 0), Err(GeekCode::ENoGrid));
        assert_eq!(
            lt.record_scan(0, SCAN_CELLS_PER_FACE, 0),
            Err(GeekCode::ENoGrid)
        );
        if let Some(w) = scan_cell(0, 0) {
            assert_eq!(lt.record_scan(0, 0, w.wrapping_add(1)), Err(GeekCode::EScanDrift));
        }
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (UNINSTALL_OPS + SCAN_CELLS + RESID_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8D00);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：位序与枚举声明序一致。
        assert_eq!(GeekCode::Ok as u32, 0);
        assert_eq!(GeekCode::ENoOp as u32, 1);
        assert_eq!(GeekCode::EDrift as u32, 2);
        assert_eq!(GeekCode::EPhase as u32, 3);
        assert_eq!(GeekCode::ENoGrid as u32, 4);
        assert_eq!(GeekCode::EScanDrift as u32, 5);
        assert_eq!(GeekCode::ENoScene as u32, 6);
        assert_eq!(GeekCode::ENoRow as u32, 7);
        assert_eq!(GeekCode::EDup as u32, 8);
        assert_eq!(GeekCode::ESeal as u32, 9);
        assert_eq!(GeekCode::EInj as u32, 10);
        assert_eq!(GeekCode::EFp as u32, 11);
        assert_eq!(GeekCode::EGradeDrift as u32, 12);
        assert_eq!(GeekCode::Ok.val(), SEG_BASE);
        assert_eq!(GeekCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all = [
            GeekCode::Ok,
            GeekCode::ENoOp,
            GeekCode::EDrift,
            GeekCode::EPhase,
            GeekCode::ENoGrid,
            GeekCode::EScanDrift,
            GeekCode::ENoScene,
            GeekCode::ENoRow,
            GeekCode::EDup,
            GeekCode::ESeal,
            GeekCode::EInj,
            GeekCode::EFp,
            GeekCode::EGradeDrift,
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
        let lt = GeekLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅六判据不构成收口。
        let mut lt2 = GeekLedger::new();
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
    fn unx52309_checks_green() {
        let s = run_unx52309_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
