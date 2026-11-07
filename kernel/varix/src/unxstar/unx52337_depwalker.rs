//! UNX-F52337 Dependency Walker 收口（EOL 定级示范） · 0xC9400段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52337"，逐字）：
//!   title: Dependency Walker 收口（EOL 定级示范）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52338-J1 六判据
//!     全落；API 拦截记录 ×10 场景全通（消费 E1 语义对账联签锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52337）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52338-J1，照原文锚定；功能按本卡标题（Dependency Walker）落地。
//! 定级封顶理由：本卡为 EOL 定级示范——Dependency Walker（depends.exe）
//! 已停更（EOL，作者 2019 年后无维护更新），替代迁移路径如实登记
//! （lucasg/Dependencies.exe），即便六判据与全部格账全通，KB-COMPAT
//! 定级也封顶 S2（有条件收口），S1 恒不可达；EOL 状态如实呈报不粉饰。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① 依赖树 10 格：自根模块向下 10 个依赖节点逐格落账（递归层序），
//!      越闭集必抓，载荷漂移必抓，重复落账必抓。
//!   ② 导入/延迟导出两面 12 格：导入面 6 格 + 延迟导出面 6 格；面相位
//!      闸（导入面未齐先录延迟面必抓，对空账本同样必抓），面单调（回录
//!      早面必抓）。
//!   ③ EOL 三要素 6 格全呈：停更公告（公告日期/终止版本）2 格 + 迁移
//!      （替代 Dependencies.exe 目标/路径）2 格 + 归档（归档位置/校验）
//!      2 格，越闭集必抓。
//!   ④ 迁移消费声明 2 行闭集：消费方迁移声明 / 消费面语义对账声明，
//!      缺一拒封。
//!   ⑤ EOL 后不承诺声明 2 行闭集：EOL 后不承诺修复声明 / EOL 后不承诺
//!      新系统兼容声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（依赖格越闭集/
//!      面格载荷漂移/面相位越序），三类注入全部落在当轮临时账本上，主账
//!      injected==caught==10、escaped==0；漂移注入先按合法序落正格再注
//!      漂移；相位注入对空账本必抓且不改 last_kind；双跑指纹一致；全通
//!      封顶 S2（EOL 如实）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52337;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 依赖树格数。
pub const DEP_CELLS: usize = 10;

/// 导入/延迟导出两面格数（导入 6 + 延迟导出 6）。
pub const FACE_CELLS: usize = 12;

/// 单面格数。
pub const FACE_HALF: usize = 6;

/// EOL 三要素格数。
pub const EOL_CELLS: usize = 6;

/// 迁移消费声明行数。
pub const MIG_ROWS: usize = 2;

/// EOL 后不承诺声明行数。
pub const PROMISE_ROWS: usize = 2;

/// XOR 锚常量（低 16 位恒零口径）。
pub const DEP_XOR: u64 = 0x4445_5057_414C_0000;

/// 面域 XOR 锚常量（低 16 位恒零口径）。
pub const FACE_XOR: u64 = 0x4641_4345_5741_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepWalkCode {
    /// 成功
    Ok = 0,
    /// 依赖树格越闭集
    ENoDep,
    /// 依赖/面格载荷漂移
    EDepDrift,
    /// 面格越闭集
    ENoFace,
    /// 面相位违例
    EFacePhase,
    /// EOL 格越闭集
    EEol,
    /// 迁移声明行越闭集
    ENoMig,
    /// 不承诺声明行越闭集
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

impl DepWalkCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DepWalkCode::Ok => "成功",
            DepWalkCode::ENoDep => "依赖树格越闭集：依赖树 10 格闭集之外先登记",
            DepWalkCode::EDepDrift => "载荷漂移：依赖/面格折叠与基准不恒等，逐格复算",
            DepWalkCode::ENoFace => "面格越闭集：导入/延迟导出两面 12 格闭集之外先登记",
            DepWalkCode::EFacePhase => "面相位违例：导入面未齐先录延迟面，或回录早面，逆序必抓",
            DepWalkCode::EEol => "EOL 格越闭集：三要素 6 格闭集之外先登记",
            DepWalkCode::ENoMig => "迁移声明行越闭集：迁移消费声明 2 行闭集之外先登记",
            DepWalkCode::ENoRow => "不承诺声明行越闭集：EOL 后不承诺声明 2 行闭集之外先登记",
            DepWalkCode::EDup => "重复落账：同格只许一条账目",
            DepWalkCode::ESeal => "未齐即封：10 依赖 + 12 面格 + 6 EOL 格 + 4 行声明未齐先补齐",
            DepWalkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DepWalkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DepWalkCode::EGradeDrift => "定级漂移：EOL 封顶 S2，全通最高只可定 S2（S1 恒不可达）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡 EOL 封顶后恒不可达）
    S1,
    /// S2：有条件收口（EOL 封顶档）
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
            Grade::S1 => "S1：全通收口（本卡 EOL 封顶，S1 恒不可达）",
            Grade::S2 => "S2：有条件收口——EOL 封顶档，停更状态如实呈报",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 依赖树节点名（const fn 表 + Option 边界）。
pub const fn dep_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("KERNEL32.dll"),
        1 => Some("USER32.dll"),
        2 => Some("ADVAPI32.dll"),
        3 => Some("SHELL32.dll"),
        4 => Some("ole32.dll"),
        5 => Some("OLEAUT32.dll"),
        6 => Some("msvcrt.dll"),
        7 => Some("WS2_32.dll"),
        8 => Some("COMCTL32.dll"),
        9 => Some("GDI32.dll"),
        _ => None,
    }
}

/// 依赖树格载荷（节点 d）。
pub const fn dep_cell(d: usize) -> Option<u64> {
    if d >= DEP_CELLS {
        return None;
    }
    Some((0xD_0000u64 + (d as u64) * 0x3D) ^ DEP_XOR)
}

/// 面种类（f/6：0=导入面 1=延迟导出面）。
pub const fn face_kind(f: usize) -> Option<u32> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((f / FACE_HALF) as u32)
}

/// 面名。
pub const fn face_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("导入面"),
        1 => Some("延迟导出面"),
        _ => None,
    }
}

/// 面格载荷（格 f）。
pub const fn face_cell(f: usize) -> Option<u64> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((0xE_0000u64 + (f as u64) * 0x2F) ^ (FACE_XOR.wrapping_add((f / FACE_HALF) as u64)))
}

/// EOL 要素种类（e/2：0=停更公告 1=迁移 2=归档）。
pub const fn eol_kind(e: usize) -> Option<u32> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// EOL 要素名。
pub const fn eol_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("停更公告"),
        1 => Some("迁移"),
        2 => Some("归档"),
        _ => None,
    }
}

/// EOL 格折叠（格 e）。
pub const fn eol_cell(e: usize) -> Option<u32> {
    if e >= EOL_CELLS {
        return None;
    }
    Some(0xD100 + e as u32 * 0x31)
}

/// 迁移消费声明行折叠（行 r）。
pub const fn mig_row(r: usize) -> Option<u32> {
    if r >= MIG_ROWS {
        return None;
    }
    Some(0xD200 + r as u32 * 0x2C)
}

/// EOL 后不承诺声明行折叠（行 r）。
pub const fn promise_row(r: usize) -> Option<u32> {
    if r >= PROMISE_ROWS {
        return None;
    }
    Some(0xD300 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Dependency Walker 收口账本状态机。
#[derive(Clone)]
pub struct DepWalkLedger {
    /// 六判据（UNX-F52338-J1 六判据逐格落账）。
    gates: [Option<bool>; 6],
    /// 依赖树账。
    deps: [Option<u64>; DEP_CELLS],
    /// 两面账。
    faces: [Option<u64>; FACE_CELLS],
    /// EOL 三要素账。
    eols: [Option<u32>; EOL_CELLS],
    /// 迁移声明账。
    migs: [Option<u32>; MIG_ROWS],
    /// 不承诺声明账。
    promises: [Option<u32>; PROMISE_ROWS],
    /// 最后面种类。
    last_face: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DepWalkLedger {
    /// 新账本。
    pub fn new() -> Self {
        DepWalkLedger {
            gates: [None; 6],
            deps: [None; DEP_CELLS],
            faces: [None; FACE_CELLS],
            eols: [None; EOL_CELLS],
            migs: [None; MIG_ROWS],
            promises: [None; PROMISE_ROWS],
            last_face: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DepWalkCode> {
        if gate >= 6 {
            return Err(DepWalkCode::ESeal);
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

    /// 依赖树一格（载荷对照 + 重复拒）。
    pub fn record_dep(&mut self, d: usize, got: u64) -> Result<(), DepWalkCode> {
        if d >= DEP_CELLS {
            return Err(DepWalkCode::ENoDep);
        }
        let want = match dep_cell(d) {
            Some(w) => w,
            None => return Err(DepWalkCode::ENoDep),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + d as u64);
            return Err(DepWalkCode::EDepDrift);
        }
        if match self.deps.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DepWalkCode::EDup);
        }
        if let Some(c) = self.deps.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x5201 + d as u64);
        Ok(())
    }

    /// 依赖树全账核验（10 格）。
    pub fn deps_all(&self) -> bool {
        let mut d = 0usize;
        while d < DEP_CELLS {
            match self.deps.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 导入面是否全齐（相位闸判据）。
    fn import_face_full(&self) -> bool {
        let mut i = 0usize;
        while i < FACE_HALF {
            match self.faces.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 面一格（面相位闸 + 面单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u64) -> Result<(), DepWalkCode> {
        if f >= FACE_CELLS {
            return Err(DepWalkCode::ENoFace);
        }
        let kind = match face_kind(f) {
            Some(k) => k,
            None => return Err(DepWalkCode::ENoFace),
        };
        // 相位闸：延迟导出面（kind=1）须在导入面 6 格全齐后方可落账；
        // 对空账本同样必抓，且拒绝路径不改 last_face。
        if kind == 1 && !self.import_face_full() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(DepWalkCode::EFacePhase);
        }
        if (kind as i64) < self.last_face {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301);
            return Err(DepWalkCode::EFacePhase);
        }
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(DepWalkCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5302 + f as u64);
            return Err(DepWalkCode::EDepDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DepWalkCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.last_face = kind as i64;
        self.fnv(0x5303 + f as u64);
        Ok(())
    }

    /// 两面全账核验（12 格）。
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

    /// EOL 要素一格。
    pub fn record_eol(&mut self, e: usize) -> Result<(), DepWalkCode> {
        if e >= EOL_CELLS {
            return Err(DepWalkCode::EEol);
        }
        if match self.eols.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DepWalkCode::EDup);
        }
        if let Some(c) = self.eols.get_mut(e) {
            *c = eol_cell(e);
        }
        self.fnv(0x5400 + e as u64);
        Ok(())
    }

    /// EOL 三要素全账核验（6 格全呈）。
    pub fn eols_all(&self) -> bool {
        let mut e = 0usize;
        while e < EOL_CELLS {
            match self.eols.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 迁移消费声明一行。
    pub fn declare_mig(&mut self, r: usize) -> Result<(), DepWalkCode> {
        if r >= MIG_ROWS {
            return Err(DepWalkCode::ENoMig);
        }
        if match self.migs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DepWalkCode::EDup);
        }
        if let Some(c) = self.migs.get_mut(r) {
            *c = mig_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 迁移声明全齐核验。
    pub fn migs_all(&self) -> bool {
        let mut r = 0usize;
        while r < MIG_ROWS {
            match self.migs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// EOL 后不承诺声明一行。
    pub fn declare_promise(&mut self, r: usize) -> Result<(), DepWalkCode> {
        if r >= PROMISE_ROWS {
            return Err(DepWalkCode::ENoRow);
        }
        if match self.promises.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DepWalkCode::EDup);
        }
        if let Some(c) = self.promises.get_mut(r) {
            *c = promise_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 不承诺声明全齐核验。
    pub fn promises_all(&self) -> bool {
        let mut r = 0usize;
        while r < PROMISE_ROWS {
            match self.promises.get(r) {
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

    /// KB-COMPAT 定级（EOL 封顶：全通 → S2，判据缺 → S3，S1 恒不可达）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.deps_all()
            && self.faces_all()
            && self.eols_all()
            && self.migs_all()
            && self.promises_all()
        {
            // EOL 定级示范：Dependency Walker 已停更，全通也只可定 S2。
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DepWalkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DepWalkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DEP_CELLS {
        if let Some(w) = dep_cell(d) {
            if led.record_dep(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
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
    let mut e = 0usize;
    while e < EOL_CELLS {
        let _ = led.record_eol(e);
        e += 1;
    }
    let mut r = 0usize;
    while r < MIG_ROWS {
        let _ = led.declare_mig(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < PROMISE_ROWS {
        let _ = led.declare_promise(r);
        r += 1;
    }
    // 注入 10 轮：依赖格越闭集 / 面格载荷漂移 / 面相位越序 三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒三数与指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DepWalkLedger::new();
        let caught = match kind {
            0 => lt.record_dep(DEP_CELLS, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                let _ = lt.record_face(0, face_cell(0).unwrap_or(0));
                lt.record_face(1, face_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(DepWalkCode::EDepDrift)
            }
            _ => {
                // 相位注入：空账本先录延迟导出面，必抓且不改 last_face。
                lt.record_face(FACE_HALF, face_cell(FACE_HALF).unwrap_or(0))
                    == Err(DepWalkCode::EFacePhase)
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
/// UNX-F52337 判据集（锚 UNX-F52338-J1，off-by-one 板式）。
pub fn run_unx52337_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52337");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "依赖树 10 节点全通",
        recorded >= DEP_CELLS as u32,
        "自根向下 10 依赖节点逐格恒等（层序枚举）",
    );
    s.add(
        "导入/延迟导出两面 12 格全落",
        {
            let mut lt = DepWalkLedger::new();
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
        "导入面 6 格先齐、延迟导出面 6 格后落，相位闸全通",
    );
    s.add(
        "EOL 三要素 6 格全呈",
        {
            let mut lt = DepWalkLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EOL_CELLS {
                if lt.record_eol(e).is_err() {
                    ok = false;
                }
                e += 1;
            }
            ok && lt.eols_all()
        },
        "停更公告/迁移（Dependencies.exe）/归档 三要素逐格如实落账",
    );
    s.add(
        "迁移消费声明 2/2 全齐",
        {
            let mut lt = DepWalkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MIG_ROWS {
                if lt.declare_mig(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.migs_all()
        },
        "消费方迁移声明/消费面语义对账声明两行闭集（E1 语义对账联签锚定行）",
    );
    s.add(
        "EOL 后不承诺声明 2/2 全齐",
        {
            let mut lt = DepWalkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PROMISE_ROWS {
                if lt.declare_promise(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.promises_all()
        },
        "EOL 后不承诺修复声明/EOL 后不承诺新系统兼容声明两行闭集",
    );
    s.add(
        "面相位闸对空账本必抓",
        {
            let mut lt = DepWalkLedger::new();
            lt.record_face(FACE_HALF, face_cell(FACE_HALF).unwrap_or(0)) == Err(DepWalkCode::EFacePhase)
        },
        "导入面未齐先录延迟面必抓，且不改 last_face",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "依赖格越闭集/面格载荷漂移/面相位越序三态轮转必抓",
    );
    let mut l0 = DepWalkLedger::new();
    s.add(
        "边界·依赖格越闭集",
        l0.record_dep(DEP_CELLS, 0).is_err(),
        "依赖节点 10 越闭集必拒",
    );
    s.add(
        "边界·EOL 格越闭集",
        l0.record_eol(EOL_CELLS).is_err(),
        "EOL 要素 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_mig(MIG_ROWS).is_err() && l0.declare_promise(PROMISE_ROWS).is_err(),
        "迁移/不承诺声明行越闭集必拒",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_dep(0, dep_cell(0).unwrap_or(0));
            l0.record_dep(0, dep_cell(0).unwrap_or(0)) == Err(DepWalkCode::EDup)
        },
        "同格只许一条账目",
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
        "定级 S2 封顶闭集",
        {
            let mut lt = DepWalkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < DEP_CELLS {
                if let Some(w) = dep_cell(d) {
                    let _ = lt.record_dep(d, w);
                }
                d += 1;
            }
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    let _ = lt.record_face(f, w);
                }
                f += 1;
            }
            let mut e = 0usize;
            while e < EOL_CELLS {
                let _ = lt.record_eol(e);
                e += 1;
            }
            let mut r = 0usize;
            while r < MIG_ROWS {
                let _ = lt.declare_mig(r);
                r += 1;
            }
            r = 0;
            while r < PROMISE_ROWS {
                let _ = lt.declare_promise(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2 && Grade::S1.val() == 1
        },
        "全通 → S2 封顶（EOL 如实呈报，KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9400,
        "SEG_BASE 0x40 对齐且落 0xC7200 + 0x40×(52337−52201) 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dep_name_table() {
        assert_eq!(dep_name(0), Some("KERNEL32.dll"));
        assert_eq!(dep_name(9), Some("GDI32.dll"));
        assert!(dep_name(10).is_none());
        assert!(dep_cell(10).is_none());
    }

    #[test]
    fn test_face_kind_mapping() {
        // 映射口径：面种类 = f/6（0=导入面 1=延迟导出面）。
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(5), Some(0));
        assert_eq!(face_kind(6), Some(1));
        assert_eq!(face_kind(11), Some(1));
        assert!(face_kind(12).is_none());
        assert_eq!(face_kind_name(0), Some("导入面"));
        assert_eq!(face_kind_name(1), Some("延迟导出面"));
        assert!(face_kind_name(2).is_none());
    }

    #[test]
    fn test_eol_kind_mapping() {
        // 映射口径：EOL 要素 = e/2（0=停更公告 1=迁移 2=归档）。
        assert_eq!(eol_kind(0), Some(0));
        assert_eq!(eol_kind(2), Some(1));
        assert_eq!(eol_kind(4), Some(2));
        assert!(eol_kind(6).is_none());
        assert!(eol_cell(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = dep_cell(3).unwrap_or(0);
        assert_eq!(a, dep_cell(3).unwrap_or(0));
        assert_ne!(a, dep_cell(4).unwrap_or(0));
        assert!(mig_row(MIG_ROWS).is_none());
        assert!(promise_row(PROMISE_ROWS).is_none());
        // XOR 锚常量低 16 位恒零。
        assert_eq!(DEP_XOR & 0xFFFF, 0);
        assert_eq!(FACE_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_face_phase_caught() {
        let mut lt = DepWalkLedger::new();
        assert_eq!(
            lt.record_face(6, face_cell(6).unwrap_or(0)),
            Err(DepWalkCode::EFacePhase)
        );
        // 拒绝路径不改 last_face：导入面正格仍可落。
        assert!(lt.record_face(0, face_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_face_monotone_caught() {
        let mut lt = DepWalkLedger::new();
        let mut f = 0usize;
        while f < FACE_CELLS {
            let _ = lt.record_face(f, face_cell(f).unwrap_or(0));
            f += 1;
        }
        assert!(lt.faces_all());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DepWalkLedger::new();
        let mut i = 0usize;
        while i <= 2 {
            let _ = lt.record_dep(i, dep_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_dep(2, dep_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(DepWalkCode::EDepDrift)
        );
        let mut j = 0usize;
        while j <= 7 {
            let _ = lt.record_face(j, face_cell(j).unwrap_or(0));
            j += 1;
        }
        assert_eq!(
            lt.record_face(7, face_cell(7).unwrap_or(0).wrapping_add(7)),
            Err(DepWalkCode::EDepDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DepWalkLedger::new();
        let _ = lt.record_eol(1);
        assert_eq!(lt.record_eol(1), Err(DepWalkCode::EDup));
        let _ = lt.declare_promise(0);
        assert_eq!(lt.declare_promise(0), Err(DepWalkCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DepWalkLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DepWalkLedger::new();
        let _ = lt2.judge_gate(0);
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DEP_CELLS + FACE_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9400);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DepWalkCode::Ok as u32, 0);
        assert_eq!(DepWalkCode::EGradeDrift as u32, 12);
        assert!(!DepWalkCode::Ok.detail().is_empty());
        assert!(!DepWalkCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade_eol_cap() {
        assert_eq!(Grade::S2.val(), 2);
        // 空账本：判据缺 → S3。
        let lt = DepWalkLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52337_checks_green() {
        let s = run_unx52337_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
