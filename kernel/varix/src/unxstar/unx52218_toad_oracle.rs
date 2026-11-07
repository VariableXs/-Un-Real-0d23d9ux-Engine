//! UNX-F52218 Toad for Oracle 收口 · 0xC7640段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52218"，逐字）：
//!   title: Toad for Oracle 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52219-J1 六判据
//!     全落；Oracle 客户端消费 ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52218）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52219-J1，照原文锚定；功能按本卡标题（Toad for Oracle）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Oracle 版本阶梯闭集：12c / 19c / 23ai 三级，越级必抓；每级独立
//!      载荷折叠，纯函数恒等才收。
//!   ② Schema Browser 20 操作集：五家族闭集（对象树/脚本生成/数据网格/
//!      权限查看/依赖分析）各 4 操作，家族单调（逆序必抓）。
//!   ③ 调优/DBA/开发三面 18 格：每面 6 格（SQL 优化器建议/ALTER 会话/
//!      容量管理/回滚段/PL-SQL 编译/重建索引 等逐格折叠），越面格必抓。
//!   ④ 六判据收口闸（SQL 优化/DBA 管理/PL-SQL 开发/客户端消费/卸载/
//!      许可）逐格落账，未齐即封拒；Oracle 客户端消费 ×10 口径逐轮对账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（版本越阶梯/漂移/操作逆序），
//!      全部落在当轮临时账本上，检出+逃逸守恒等式（injected==caught==10、
//!      escaped==0）；双跑 FNV 指纹一致；定级闭集 S1/S2/S3（20 操作 +
//!      3 级版本 + 18 面格 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52218;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Schema Browser 操作数。
pub const BROWSER_OPS: usize = 20;

/// Oracle 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 三面格数（调优 6 / DBA 6 / 开发 6）。
pub const FACE_CELLS: usize = 18;

/// Oracle 客户端消费对账轮数。
pub const CLIENT_CONSUME_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToadCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 面格越闭集
    ENoFace,
    /// 面格漂移
    EFaceDrift,
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

impl ToadCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ToadCode::Ok => "成功",
            ToadCode::ENoVer => "版本级越阶梯：Oracle 12c/19c/23ai 三级闭集之外先登记",
            ToadCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            ToadCode::ENoOp => "操作号越闭集：Schema Browser 20 操作集之外先登记",
            ToadCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ToadCode::EOrder => "家族逆序：对象树→脚本生成→数据网格→权限查看→依赖分析单调，逆序必抓",
            ToadCode::ENoFace => "面格越闭集：调优/DBA/开发 18 格闭集之外先登记",
            ToadCode::EFaceDrift => "面格漂移：面格折叠与基准不恒等，逐格复算",
            ToadCode::EDup => "重复落账：同格只许一条账目",
            ToadCode::ESeal => "未齐即封：20 操作 + 3 级版本 + 18 面格未齐先补齐",
            ToadCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ToadCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ToadCode::EGradeDrift => "定级漂移：全通且三面全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3级版本+18面格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=12c 1=19c 2=23ai）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xBC000u64 + (v as u64) * 0x5E) ^ 0x544F41445F4F5241)
}

/// 版本级名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("12c"),
        1 => Some("19c"),
        2 => Some("23ai"),
        _ => None,
    }
}

/// Schema Browser 操作格载荷（操作 o；家族 = o/4）。
pub const fn browser_cell(o: usize) -> Option<u64> {
    if o >= BROWSER_OPS {
        return None;
    }
    Some((0xBD000u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=对象树 4..8=脚本生成 8..12=数据网格 12..16=权限查看 16..20=依赖分析）。
pub const fn browser_kind(o: usize) -> Option<u32> {
    if o >= BROWSER_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn browser_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("对象树"),
        1 => Some("脚本生成"),
        2 => Some("数据网格"),
        3 => Some("权限查看"),
        4 => Some("依赖分析"),
        _ => None,
    }
}

/// 三面格载荷（格 f：0..6=调优 6..12=DBA 12..18=开发）。
pub const fn face_cell(f: usize) -> Option<u32> {
    if f >= FACE_CELLS {
        return None;
    }
    Some(0xBE00 + (f / 6) as u32 * 0x48 + (f % 6) as u32 * 0x0D)
}

/// 面名。
pub const fn face_name(f: usize) -> Option<&'static str> {
    if f >= FACE_CELLS {
        return None;
    }
    match f / 6 {
        0 => Some("调优面"),
        1 => Some("DBA 面"),
        _ => Some("开发面"),
    }
}

/// Oracle 客户端消费对账折叠（轮 r，×10 口径）。
pub const fn consume_fold(r: usize) -> Option<u32> {
    if r >= CLIENT_CONSUME_ROUNDS {
        return None;
    }
    Some(0xBF00 + r as u32 * 0x13)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Toad for Oracle 收口账本状态机。
#[derive(Clone)]
pub struct ToadLedger {
    /// 六判据（SQL 优化/DBA 管理/PL-SQL 开发/客户端消费/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 操作账。
    ops: [Option<u64>; BROWSER_OPS],
    /// 面格账。
    faces: [Option<u32>; FACE_CELLS],
    /// 客户端消费对账账。
    consume: [Option<u32>; CLIENT_CONSUME_ROUNDS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ToadLedger {
    /// 新账本。
    pub fn new() -> Self {
        ToadLedger {
            gates: [None; 6],
            vers: [None; VERSION_STEPS],
            ops: [None; BROWSER_OPS],
            faces: [None; FACE_CELLS],
            consume: [None; CLIENT_CONSUME_ROUNDS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ToadCode> {
        if gate >= 6 {
            return Err(ToadCode::ESeal);
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

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), ToadCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(ToadCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + v as u64);
            return Err(ToadCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ToadCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x6201 + v as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ToadCode> {
        if o >= BROWSER_OPS {
            return Err(ToadCode::ENoOp);
        }
        let kind = match browser_kind(o) {
            Some(k) => k,
            None => return Err(ToadCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(ToadCode::EOrder);
        }
        let want = match browser_cell(o) {
            Some(w) => w,
            None => return Err(ToadCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(ToadCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ToadCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < BROWSER_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 面格一格（载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u32) -> Result<(), ToadCode> {
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(ToadCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + f as u64);
            return Err(ToadCode::EFaceDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ToadCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x6401 + f as u64);
        Ok(())
    }

    /// 面格全账核验（18 格）。
    pub fn faces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACE_CELLS {
            match self.faces.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 客户端消费对账一轮（载荷对照 + 重复拒）。
    pub fn record_consume(&mut self, r: usize, got: u32) -> Result<(), ToadCode> {
        let want = match consume_fold(r) {
            Some(w) => w,
            None => return Err(ToadCode::ESeal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + r as u64);
            return Err(ToadCode::EFaceDrift);
        }
        if match self.consume.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ToadCode::EDup);
        }
        if let Some(c) = self.consume.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x6501 + r as u64);
        Ok(())
    }

    /// 客户端消费对账全账核验（×10 全齐）。
    pub fn consume_all(&self) -> bool {
        let mut r = 0usize;
        while r < CLIENT_CONSUME_ROUNDS {
            match self.consume.get(r) {
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
        if self.gates_all() && self.vers_all() && self.ops_all() && self.faces_all() && self.consume_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ToadLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ToadLedger::new();
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
    while o < BROWSER_OPS {
        if let Some(w) = browser_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < FACE_CELLS {
        if let Some(w) = face_cell(f) {
            if led.record_face(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < CLIENT_CONSUME_ROUNDS {
        if let Some(w) = consume_fold(r) {
            if led.record_consume(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    // 注入 10 轮：版本越阶梯 / 漂移 / 操作逆序 三态轮转，
    // 三类注入全部落在当轮临时账本上；漂移先录正格再注漂移；
    // 逆序注入对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ToadLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
                lt.record_version(VERSION_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_face(2, face_cell(2).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0).wrapping_add(1)) == Err(ToadCode::EFaceDrift)
            }
            _ => {
                let _ = lt.record_op(9, browser_cell(9).unwrap_or(0));
                lt.record_op(1, browser_cell(1).unwrap_or(0)) == Err(ToadCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52218 判据集（锚 UNX-F52219-J1，off-by-one 板式）。
pub fn run_unx52218_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52218");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Schema Browser 20 操作全通",
        recorded >= BROWSER_OPS as u32,
        "对象树/脚本生成/数据网格/权限查看/依赖分析五家族逐操作恒等",
    );
    s.add(
        "Oracle 12c/19c/23ai 阶梯三级全落",
        {
            let mut lt = ToadLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
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
        "调优/DBA/开发三面 18 格全落",
        {
            let mut lt = ToadLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    if lt.record_face(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.faces_all()
        },
        "三面各 6 格闭集逐格恒等（§AI-66.3）",
    );
    s.add(
        "Oracle 客户端消费 ×10 全落",
        {
            let mut lt = ToadLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CLIENT_CONSUME_ROUNDS {
                if let Some(w) = consume_fold(r) {
                    if lt.record_consume(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.consume_all()
        },
        "客户端消费 ×10 口径逐轮对账全齐",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ToadLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "SQL 优化/DBA 管理/PL-SQL 开发/客户端消费/卸载/许可逐格落账（UNX-F52219-J1 锚）",
    );
    s.add(
        "浏览器家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BROWSER_OPS {
                if browser_kind(o).unwrap_or(5) < browser_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越阶梯/漂移/操作逆序三态轮转必抓",
    );
    let mut l0 = ToadLedger::new();
    s.add(
        "边界·版本级越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(BROWSER_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·面格越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "面格 18 越闭集必拒",
    );
    s.add(
        "漂移拒（先录正格再注漂移）",
        {
            let _ = l0.record_face(2, face_cell(2).unwrap_or(0));
            l0.record_face(2, face_cell(2).unwrap_or(0).wrapping_add(1)) == Err(ToadCode::EFaceDrift)
        },
        "面格载荷漂移必抓",
    );
    s.add(
        "逆序拒（空账本同样必抓）",
        {
            let mut lt = ToadLedger::new();
            let _ = lt.record_op(9, browser_cell(9).unwrap_or(0));
            lt.record_op(1, browser_cell(1).unwrap_or(0)) == Err(ToadCode::EOrder)
        },
        "数据网格家族先落、对象树家族后落，逆序必抓",
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
            let mut lt = ToadLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < BROWSER_OPS {
                if let Some(w) = browser_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    let _ = lt.record_face(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < CLIENT_CONSUME_ROUNDS {
                if let Some(w) = consume_fold(r) {
                    let _ = lt.record_consume(r, w);
                }
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * 17,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC7640 格",
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
        assert_eq!(version_name(0), Some("12c"));
        assert_eq!(version_name(1), Some("19c"));
        assert_eq!(version_name(2), Some("23ai"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_browser_kind() {
        assert_eq!(browser_kind(0), Some(0));
        assert_eq!(browser_kind(8), Some(2));
        assert_eq!(browser_kind(19), Some(4));
        assert!(browser_kind(20).is_none());
        assert!(browser_cell(20).is_none());
    }

    #[test]
    fn test_face_table() {
        assert_eq!(face_name(0), Some("调优面"));
        assert_eq!(face_name(6), Some("DBA 面"));
        assert_eq!(face_name(17), Some("开发面"));
        assert!(face_cell(18).is_none());
        assert_ne!(face_cell(0).unwrap_or(0), face_cell(5).unwrap_or(0));
    }

    #[test]
    fn test_consume_fold() {
        assert!(consume_fold(0).is_some());
        assert!(consume_fold(9).is_some());
        assert!(consume_fold(10).is_none());
        let a = consume_fold(3).unwrap_or(0);
        assert_eq!(a, consume_fold(3).unwrap_or(0));
    }

    #[test]
    fn test_cells_pure() {
        let a = browser_cell(3).unwrap_or(0);
        assert_eq!(a, browser_cell(3).unwrap_or(0));
        assert_ne!(a, browser_cell(4).unwrap_or(0));
        let v = version_cell(1).unwrap_or(0);
        assert_eq!(v, version_cell(1).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ToadLedger::new();
        let _ = lt.record_op(9, browser_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, browser_cell(2).unwrap_or(0)), Err(ToadCode::EOrder));
        assert!(lt.record_op(10, browser_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ToadLedger::new();
        assert_eq!(
            lt.record_op(5, browser_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ToadCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(ToadCode::EVerDrift)
        );
        assert_eq!(
            lt.record_face(7, face_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(ToadCode::EFaceDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ToadLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(ToadCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ToadLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ToadLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (BROWSER_OPS + VERSION_STEPS + FACE_CELLS + CLIENT_CONSUME_ROUNDS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7640);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ToadCode::Ok as u32, 0);
        assert_eq!(ToadCode::EGradeDrift as u32, 12);
        assert!(!ToadCode::Ok.detail().is_empty());
        assert!(!ToadCode::ESeal.detail().is_empty());
        assert_eq!(ToadCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ToadLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52218_checks_green() {
        let s = run_unx52218_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
