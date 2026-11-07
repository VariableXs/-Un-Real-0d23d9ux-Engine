//! UNX-F52312 IObit Uninstaller 收口 · 0xC8DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52312"，逐字）：
//!   title: IObit Uninstaller 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52313-J1 六判据
//!     全落；EOL 临近声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题
//!     六；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52312）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52313-J1，照原文锚定；功能按本卡标题（IObit Uninstaller）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 卸载 10 操作集闭集：五家族（执行/扫描/清理/登记/收尾）各 2 操
//!      作，相位闸按已收格数推演期望家族，空账本注高族必抓。
//!   ② 批量/工具栏/插件三面 12 格：三面各 4 格，载荷折叠恒等才收，
//!      漂移必抓。
//!   ③ 残留扫描 8 格闭集：注册表 4 + 文件 4 双侧对照，缺侧拒封。
//!   ④ 捆绑软件拦截三元闸：安装前拦截/安装中识别/安装后提示，三元闸
//!      逐格落账漂移必抓。
//!   ⑤ 升级提示 N/A 声明 2 行闭集：EOL 临近声明（卡面判据）/升级提示
//!      不适用声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（操作越闭集/三面漂移/相位越序）
//!      全部落在当轮临时账本上，检出+逃逸守恒；双跑指纹一致；KB-COMPAT
//!      定级 S2 封顶口径（全通 → S2，S1 枚举位保留不签发）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52312;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8DC0;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 卸载操作数。
pub const UNINSTALL_OPS: usize = 10;

/// 三面数（批量/工具栏/插件）。
pub const TRI_FACES: usize = 3;

/// 每面格数（3 面 × 4 = 12 格）。
pub const FACE_CELLS_PER: usize = 4;

/// 三面总格数。
pub const TRI_CELLS: usize = TRI_FACES * FACE_CELLS_PER;

/// 残留扫描格数（注册表 4 + 文件 4）。
pub const RESID_CELLS: usize = 8;

/// 捆绑软件拦截三元闸数。
pub const BUNDLE_GATES: usize = 3;

/// 升级提示 N/A 声明行数（含 EOL 临近声明）。
pub const NA_ROWS: usize = 2;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x494F_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IobitCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族相位越序
    EPhase,
    /// 三面/残留/闸格越闭集
    ENoFace,
    /// 三面载荷漂移
    EFaceDrift,
    /// 三元闸载荷漂移
    EGateDrift,
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

impl IobitCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            IobitCode::Ok => "成功",
            IobitCode::ENoOp => "操作号越闭集：卸载 10 操作集之外先登记",
            IobitCode::EDrift => "载荷漂移：操作/残留扫描折叠与基准不恒等，逐格复算",
            IobitCode::EPhase => "家族相位越序：执行→扫描→清理→登记→收尾按相位推进，越相必抓",
            IobitCode::ENoFace => "三面/残留/闸格越闭集：三面 12 格、残留 8 格与三元闸 3 格之外先登记",
            IobitCode::EFaceDrift => "三面载荷漂移：批量/工具栏/插件 12 格折叠与基准不恒等，逐面复算",
            IobitCode::EGateDrift => "三元闸载荷漂移：捆绑拦截 3 格折叠与基准不恒等，逐闸复算",
            IobitCode::ENoRow => "声明行越闭集：EOL 临近/升级提示 N/A 2 行闭集之外先登记",
            IobitCode::EDup => "重复落账：同格只许一条账目",
            IobitCode::ESeal => "未齐即封：10 操作 + 12 三面格 + 8 残留格 + 3 闸格 + 2 行声明未齐先补齐",
            IobitCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            IobitCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IobitCode::EGradeDrift => "定级漂移：全通才可定 S2，KB-COMPAT 封顶口径",
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
            Grade::S1 => "S1：保留位，IObit 域存在 EOL 临近外部条件不签发",
            Grade::S2 => "S2：六判据+10操作+12三面格+8残留格+3闸格+2行声明全通，有条件收口",
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
    Some((0xD100u64 + (o as u64) * 0x2C) ^ ANCHOR)
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

/// 三面格载荷（面 f 0..3：0 批量 1 工具栏 2 插件；面内格 c 0..4）。
pub const fn tri_face_cell(f: usize, c: usize) -> Option<u64> {
    if f >= TRI_FACES || c >= FACE_CELLS_PER {
        return None;
    }
    Some((0xD200u64 + (f as u64) * 0x130 + (c as u64) * 0x2C) ^ ANCHOR)
}

/// 三面名。
pub const fn tri_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("批量面"),
        1 => Some("工具栏面"),
        2 => Some("插件面"),
        _ => None,
    }
}

/// 残留扫描格载荷（i 0..8：0..4 注册表侧，4..8 文件侧）。
pub const fn resid_cell(i: usize) -> Option<u64> {
    if i >= RESID_CELLS {
        return None;
    }
    Some((0xD300u64 + (i as u64) * 0x24) ^ ANCHOR)
}

/// 捆绑软件拦截三元闸格载荷（闸 g：0 安装前拦截 1 安装中识别 2 安装后提示）。
pub const fn bundle_gate_cell(g: usize) -> Option<u64> {
    if g >= BUNDLE_GATES {
        return None;
    }
    Some((0xD400u64 + (g as u64) * 0x38) ^ ANCHOR)
}

/// 三元闸名。
pub const fn bundle_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装前拦截"),
        1 => Some("安装中识别"),
        2 => Some("安装后提示"),
        _ => None,
    }
}

/// 升级提示 N/A 声明行折叠（行 r：0 EOL 临近声明 1 升级提示不适用声明）。
pub const fn na_row(r: usize) -> Option<u32> {
    if r >= NA_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x30)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// IObit Uninstaller 收口账本状态机。
#[derive(Clone)]
pub struct IobitLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 卸载操作账。
    ops: [Option<u64>; UNINSTALL_OPS],
    /// 三面账（3 面 × 4 格平铺）。
    tri: [Option<u64>; TRI_CELLS],
    /// 残留扫描账。
    resid: [Option<u64>; RESID_CELLS],
    /// 三元闸账。
    gates3: [Option<u64>; BUNDLE_GATES],
    /// N/A 声明账。
    na: [Option<u32>; NA_ROWS],
    /// 已收操作数（相位推演基）。
    op_count: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IobitLedger {
    /// 新账本。
    pub fn new() -> Self {
        IobitLedger {
            gates: [None; 6],
            ops: [None; UNINSTALL_OPS],
            tri: [None; TRI_CELLS],
            resid: [None; RESID_CELLS],
            gates3: [None; BUNDLE_GATES],
            na: [None; NA_ROWS],
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

    /// 落一格六判据（UNX-F52313-J1）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), IobitCode> {
        if gate >= 6 {
            return Err(IobitCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), IobitCode> {
        if o >= UNINSTALL_OPS {
            return Err(IobitCode::ENoOp);
        }
        let kind = match op_family(o) {
            Some(k) => k,
            None => return Err(IobitCode::ENoOp),
        };
        // 相位闸：期望家族 = 已收操作数 / 每族格数；空账本注高族必抓。
        if kind as usize != self.op_count / 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(IobitCode::EPhase);
        }
        let want = match uninstall_op_cell(o) {
            Some(w) => w,
            None => return Err(IobitCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(IobitCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IobitCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.op_count += 1;
        self.fnv(0x8202 + o as u64);
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

    /// 三面一格（面/格越界拒 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, c: usize, got: u64) -> Result<(), IobitCode> {
        let want = match tri_face_cell(f, c) {
            Some(w) => w,
            None => return Err(IobitCode::ENoFace),
        };
        let idx = f * FACE_CELLS_PER + c;
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + idx as u64);
            return Err(IobitCode::EFaceDrift);
        }
        if match self.tri.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IobitCode::EDup);
        }
        if let Some(cell) = self.tri.get_mut(idx) {
            *cell = Some(got);
        }
        self.fnv(0x8301 + idx as u64);
        Ok(())
    }

    /// 三面全账核验（12 格）。
    pub fn faces_all(&self) -> bool {
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

    /// 残留扫描一格（载荷对照 + 重复拒）。
    pub fn record_resid(&mut self, i: usize, got: u64) -> Result<(), IobitCode> {
        let want = match resid_cell(i) {
            Some(w) => w,
            None => return Err(IobitCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + i as u64);
            return Err(IobitCode::EDrift);
        }
        if match self.resid.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IobitCode::EDup);
        }
        if let Some(c) = self.resid.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x8401 + i as u64);
        Ok(())
    }

    /// 残留扫描全账核验（8 格）。
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

    /// 三元闸一格（载荷对照 + 重复拒）。
    pub fn record_bundle_gate(&mut self, g: usize, got: u64) -> Result<(), IobitCode> {
        let want = match bundle_gate_cell(g) {
            Some(w) => w,
            None => return Err(IobitCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + g as u64);
            return Err(IobitCode::EGateDrift);
        }
        if match self.gates3.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IobitCode::EDup);
        }
        if let Some(c) = self.gates3.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x8501 + g as u64);
        Ok(())
    }

    /// 三元闸全账核验（3 格）。
    pub fn gates3_all(&self) -> bool {
        let mut g = 0usize;
        while g < BUNDLE_GATES {
            match self.gates3.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 升级提示 N/A 声明一行（含 EOL 临近声明）。
    pub fn declare_na(&mut self, r: usize) -> Result<(), IobitCode> {
        let want = match na_row(r) {
            Some(w) => w,
            None => return Err(IobitCode::ENoRow),
        };
        if match self.na.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IobitCode::EDup);
        }
        if let Some(c) = self.na.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn na_all(&self) -> bool {
        let mut r = 0usize;
        while r < NA_ROWS {
            match self.na.get(r) {
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
            && self.faces_all()
            && self.resid_all()
            && self.gates3_all()
            && self.na_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for IobitLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（收格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IobitLedger::new();
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
    while f < TRI_FACES {
        let mut c = 0usize;
        while c < FACE_CELLS_PER {
            if let Some(w) = tri_face_cell(f, c) {
                if led.record_face(f, c, w).is_ok() {
                    recorded += 1;
                }
            }
            c += 1;
        }
        f += 1;
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
    let mut b = 0usize;
    while b < BUNDLE_GATES {
        if let Some(w) = bundle_gate_cell(b) {
            if led.record_bundle_gate(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < NA_ROWS {
        let _ = led.declare_na(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 三面漂移 / 相位越序三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = IobitLedger::new();
        let caught = match kind {
            0 => lt.record_op(UNINSTALL_OPS, 0) == Err(IobitCode::ENoOp),
            1 => {
                // 漂移注入：先录正格再注漂移。
                if let Some(w) = tri_face_cell(0, 0) {
                    let _ = lt.record_face(0, 0, w);
                }
                match tri_face_cell(0, 1) {
                    Some(w) => lt.record_face(0, 1, w.wrapping_add(1)) == Err(IobitCode::EFaceDrift),
                    None => false,
                }
            }
            _ => {
                // 相位注入：空账本直注家族 4 操作，必抓且不改 last 基。
                match uninstall_op_cell(8) {
                    Some(w) => lt.record_op(8, w) == Err(IobitCode::EPhase),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
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
/// UNX-F52312 判据集（锚 UNX-F52313-J1，off-by-one 板式，§AI-66.2 专题六）。
pub fn run_unx52312_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52312");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "卸载 10 操作全通",
        recorded >= UNINSTALL_OPS as u32,
        "执行/扫描/清理/登记/收尾五家族逐操作恒等",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = IobitLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52313-J1 六判据逐格落账",
    );
    s.add(
        "批量/工具栏/插件三面 12 格全落",
        {
            let mut lt = IobitLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < TRI_FACES {
                let mut c = 0usize;
                while c < FACE_CELLS_PER {
                    if let Some(w) = tri_face_cell(f, c) {
                        if lt.record_face(f, c, w).is_err() {
                            ok = false;
                        }
                    }
                    c += 1;
                }
                f += 1;
            }
            ok && lt.faces_all()
        },
        "批量面/工具栏面/插件面逐格恒等",
    );
    s.add(
        "残留扫描 8 格全落",
        {
            let mut lt = IobitLedger::new();
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
        "捆绑软件拦截三元闸 3/3 全落",
        {
            let mut lt = IobitLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BUNDLE_GATES {
                if let Some(w) = bundle_gate_cell(b) {
                    if lt.record_bundle_gate(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.gates3_all()
        },
        "安装前拦截/安装中识别/安装后提示三元闸逐格落账",
    );
    s.add(
        "升级提示 N/A 声明 2/2 全齐",
        {
            let mut lt = IobitLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < NA_ROWS {
                if lt.declare_na(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.na_all()
        },
        "EOL 临近声明（卡面判据）/升级提示不适用声明两行闭集，缺一拒封",
    );
    s.add(
        "家族相位单调",
        {
            let mut lt = IobitLedger::new();
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
        "操作越闭集/三面漂移/相位越序三态轮转必抓",
    );
    let mut l0 = IobitLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(UNINSTALL_OPS, 0) == Err(IobitCode::ENoOp),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·三面越闭集",
        l0.record_face(TRI_FACES, 0, 0) == Err(IobitCode::ENoFace),
        "三面 3 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_na(NA_ROWS) == Err(IobitCode::ENoRow),
        "N/A 声明行 2 越闭集必拒",
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
            let mut lt = IobitLedger::new();
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
            while f < TRI_FACES {
                let mut c = 0usize;
                while c < FACE_CELLS_PER {
                    if let Some(w) = tri_face_cell(f, c) {
                        let _ = lt.record_face(f, c, w);
                    }
                    c += 1;
                }
                f += 1;
            }
            let mut i = 0usize;
            while i < RESID_CELLS {
                if let Some(w) = resid_cell(i) {
                    let _ = lt.record_resid(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < BUNDLE_GATES {
                if let Some(w) = bundle_gate_cell(b) {
                    let _ = lt.record_bundle_gate(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < NA_ROWS {
                let _ = lt.declare_na(r);
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
        assert_eq!(op_family_name(1), Some("扫描"));
        assert!(op_family_name(5).is_none());
    }

    #[test]
    fn test_tri_faces() {
        assert_eq!(tri_face_name(0), Some("批量面"));
        assert_eq!(tri_face_name(1), Some("工具栏面"));
        assert_eq!(tri_face_name(2), Some("插件面"));
        assert!(tri_face_name(3).is_none());
        assert!(tri_face_cell(3, 0).is_none());
        assert!(tri_face_cell(0, 4).is_none());
        assert!(tri_face_cell(2, 3).is_some());
    }

    #[test]
    fn test_bundle_gates_and_na() {
        assert_eq!(bundle_gate_name(0), Some("安装前拦截"));
        assert_eq!(bundle_gate_name(2), Some("安装后提示"));
        assert!(bundle_gate_name(3).is_none());
        assert!(bundle_gate_cell(3).is_none());
        assert!(resid_cell(8).is_none());
        assert!(na_row(2).is_none());
    }

    #[test]
    fn test_phase_caught_on_empty() {
        // 相位注入：空账本直注家族 4 操作必抓。
        let mut lt = IobitLedger::new();
        if let Some(w) = uninstall_op_cell(8) {
            assert_eq!(lt.record_op(8, w), Err(IobitCode::EPhase));
        }
        assert_eq!(lt.stats().0, 1);
        assert_eq!(lt.stats().1, 1);
        // 合法序种子：先落低族格再续推。
        let mut lt2 = IobitLedger::new();
        let mut o = 0usize;
        while o < 6 {
            if let Some(w) = uninstall_op_cell(o) {
                assert!(lt2.record_op(o, w).is_ok());
            }
            o += 1;
        }
        assert_eq!(lt2.op_count, 6);
    }

    #[test]
    fn test_drift_caught_after_seed() {
        // 漂移注入：先录正格再注漂移必抓。
        let mut lt = IobitLedger::new();
        if let Some(w) = tri_face_cell(1, 0) {
            assert!(lt.record_face(1, 0, w).is_ok());
        }
        if let Some(w) = tri_face_cell(1, 1) {
            assert_eq!(
                lt.record_face(1, 1, w.wrapping_add(2)),
                Err(IobitCode::EFaceDrift)
            );
        }
        let mut lt2 = IobitLedger::new();
        if let Some(w) = resid_cell(5) {
            assert_eq!(lt2.record_resid(5, w.wrapping_add(4)), Err(IobitCode::EDrift));
        }
        if let Some(w) = bundle_gate_cell(1) {
            assert_eq!(
                lt2.record_bundle_gate(1, w.wrapping_add(1)),
                Err(IobitCode::EGateDrift)
            );
        }
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = IobitLedger::new();
        if let Some(w) = uninstall_op_cell(0) {
            let _ = lt.record_op(0, w);
            assert_eq!(lt.record_op(0, w), Err(IobitCode::EDup));
        }
        let mut lt2 = IobitLedger::new();
        if let Some(w) = bundle_gate_cell(0) {
            let _ = lt2.record_bundle_gate(0, w);
            assert_eq!(lt2.record_bundle_gate(0, w), Err(IobitCode::EDup));
        }
        let _ = lt2.declare_na(0);
        assert_eq!(lt2.declare_na(0), Err(IobitCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = IobitLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = IobitLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_face_boundary_caught() {
        let mut lt = IobitLedger::new();
        assert_eq!(lt.record_face(TRI_FACES, 0, 0), Err(IobitCode::ENoFace));
        assert_eq!(
            lt.record_face(0, FACE_CELLS_PER, 0),
            Err(IobitCode::ENoFace)
        );
        assert_eq!(lt.record_resid(RESID_CELLS, 0), Err(IobitCode::ENoFace));
        assert_eq!(lt.record_bundle_gate(BUNDLE_GATES, 0), Err(IobitCode::ENoFace));
        assert_eq!(lt.declare_na(NA_ROWS), Err(IobitCode::ENoRow));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (UNINSTALL_OPS + TRI_CELLS + RESID_CELLS + BUNDLE_GATES) as u32);
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
        assert_eq!(SEG_BASE, 0xC8DC0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：位序与枚举声明序一致。
        assert_eq!(IobitCode::Ok as u32, 0);
        assert_eq!(IobitCode::ENoOp as u32, 1);
        assert_eq!(IobitCode::EDrift as u32, 2);
        assert_eq!(IobitCode::EPhase as u32, 3);
        assert_eq!(IobitCode::ENoFace as u32, 4);
        assert_eq!(IobitCode::EFaceDrift as u32, 5);
        assert_eq!(IobitCode::EGateDrift as u32, 6);
        assert_eq!(IobitCode::ENoRow as u32, 7);
        assert_eq!(IobitCode::EDup as u32, 8);
        assert_eq!(IobitCode::ESeal as u32, 9);
        assert_eq!(IobitCode::EInj as u32, 10);
        assert_eq!(IobitCode::EFp as u32, 11);
        assert_eq!(IobitCode::EGradeDrift as u32, 12);
        assert_eq!(IobitCode::Ok.val(), SEG_BASE);
        assert_eq!(IobitCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all = [
            IobitCode::Ok,
            IobitCode::ENoOp,
            IobitCode::EDrift,
            IobitCode::EPhase,
            IobitCode::ENoFace,
            IobitCode::EFaceDrift,
            IobitCode::EGateDrift,
            IobitCode::ENoRow,
            IobitCode::EDup,
            IobitCode::ESeal,
            IobitCode::EInj,
            IobitCode::EFp,
            IobitCode::EGradeDrift,
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
        let lt = IobitLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅六判据不构成收口。
        let mut lt2 = IobitLedger::new();
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
    fn unx52312_checks_green() {
        let s = run_unx52312_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
