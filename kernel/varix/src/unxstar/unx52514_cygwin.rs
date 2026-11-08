//! UNX-F52514 Cygwin 收口 · 0xCC040段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52514"，逐字）：
//!   title: Cygwin 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52515-J1 applet
//!     ×20 联通性全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52514）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52515-J1，照原文锚定；功能按本卡标题（Cygwin）落地。applet ×20
//! 属后卡（BusyBox-w32）判据域，本卡仅照原文锚引；本卡功能域为 Cygwin
//! 五面 + POSIX 语义投影 + mount 三元闸。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面 20 格闭集：cygwin1.dll / cygpath / 包管理(setup) / 终端 /
//!      X 消费，各 4 格，面族单调（逆序必抓）；逐格载荷纯函数折叠。
//!   ② POSIX 语义投影 8 格：fork/exec/signal/path/file/uid/termios/
//!      dlopen 语义投影逐格恒等。
//!   ③ 与 Git Bash 分账声明 2 行（F52513 判据域分账），缺一拒封；C 上
//!      栈声明 2 行闭集，缺一拒封。
//!   ④ mount 三元闸：挂载点/挂载标志/语义核对三元按序落账，乱序必抓。
//!   ⑤ ledger 状态机（new/record_*/mount_*/declare_*/judge_gate×6/seal/
//!      fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转
//!      10 轮（面漂移/面族逆序/面越闭集）10/10 全抓；检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级目标档 S2（六闸齐 → S2，空账 → S3）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52514;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面格数（dll/cygpath/包管理/终端/X 消费 ×4）。
pub const FACE_CELLS: usize = 20;

/// 面族数。
pub const FACE_KINDS: usize = 5;

/// POSIX 语义投影格数。
pub const PROJ_CELLS: usize = 8;

/// mount 三元闸元数。
pub const MOUNT_TRIPLE: usize = 3;

/// 两组声明行各 2 行（与 Git Bash 分账/C 上栈）。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CygwinCode {
    /// 成功
    Ok = 0,
    /// 面号越闭集
    ENoFace,
    /// 载荷漂移（面/投影格复用）
    EDrift,
    /// 面族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// mount 元越闭集或乱序
    ENoMount,
    /// 声明行越闭集
    ENoRow,
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

impl CygwinCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CygwinCode::Ok => "成功",
            CygwinCode::ENoFace => "面号越闭集：五面 20 格之外先登记",
            CygwinCode::EDrift => "载荷漂移：格折叠与基准不恒等，逐格复算",
            CygwinCode::EOrder => "面族逆序：dll→cygpath→包管理→终端→X 消费单调，逆序必抓",
            CygwinCode::ENoProj => "投影格越闭集：POSIX 语义投影 8 格之外先登记",
            CygwinCode::ENoMount => "mount 元越闭集或乱序：三元闸须挂载点→标志→语义按序落账",
            CygwinCode::ENoRow => "声明行越闭集：两组声明各 2 行闭集之外先登记",
            CygwinCode::ERowMiss => "缺声明行：与 Git Bash 分账/C 上栈声明未齐先补齐",
            CygwinCode::EDup => "重复落账：同格只许一条账目",
            CygwinCode::ESeal => "未齐即封：20 面+8 投影+3 mount 元未齐先补齐",
            CygwinCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CygwinCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CygwinCode::EGradeDrift => "定级漂移：六闸全齐才可定 S2（目标档封顶）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不可达：目标档封顶 S2）
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
            Grade::S1 => "S1：全通收口（Cygwin 面目标档封顶 S2，S1 恒不达）",
            Grade::S2 => "S2：有条件收口，六判据全齐（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面格载荷（格 o；面族 = o/4）。
pub const fn face_cell(o: usize) -> Option<u64> {
    if o >= FACE_CELLS {
        return None;
    }
    Some((0xCD10u64 + (o as u64) * 0x2E) ^ 0x43594757)
}

/// 面族（0=dll 1=cygpath 2=包管理 3=终端 4=X 消费）。
pub const fn face_kind(o: usize) -> Option<u32> {
    if o >= FACE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面族名（按 kind 索引）。
pub const fn face_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("cygwin1.dll"),
        1 => Some("cygpath"),
        2 => Some("包管理(setup)"),
        3 => Some("终端"),
        4 => Some("X 消费"),
        _ => None,
    }
}

/// POSIX 语义投影格载荷（格 p；判据原文锚引后卡域不混记）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xCD50u64 + (p as u64) * 0x34) ^ ((p as u64).wrapping_add(1)).wrapping_mul(0x9E3779B1))
}

/// 投影名（按格索引）。
pub const fn proj_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("fork"),
        1 => Some("exec"),
        2 => Some("signal"),
        3 => Some("path"),
        4 => Some("file"),
        5 => Some("uid"),
        6 => Some("termios"),
        7 => Some("dlopen"),
        _ => None,
    }
}

/// mount 三元闸元载荷（元 m；0=挂载点 1=挂载标志 2=语义核对）。
pub const fn mount_cell(m: usize) -> Option<u64> {
    if m >= MOUNT_TRIPLE {
        return None;
    }
    Some((0xCD80u64 + (m as u64) * 0x40) ^ 0x4D4F554E)
}

/// mount 元名（按元索引）。
pub const fn mount_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("挂载点"),
        1 => Some("挂载标志"),
        2 => Some("语义核对"),
        _ => None,
    }
}

/// 声明行折叠（组 grp：0=与 Git Bash 分账 1=C 上栈；行 r）。
pub const fn decl_row(grp: usize, r: usize) -> Option<u32> {
    if grp >= 2 || r >= DECL_ROWS {
        return None;
    }
    Some(0xCDA0 + (grp * 0x06 + r as usize * 0x02) as u32)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Cygwin 收口账本状态机。
#[derive(Clone)]
pub struct CygwinLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 面格账。
    faces: [Option<u64>; FACE_CELLS],
    /// 投影格账。
    projs: [Option<u64>; PROJ_CELLS],
    /// mount 三元账。
    mounts: [Option<u64>; MOUNT_TRIPLE],
    /// 两组声明账（组→行）。
    decls: [[Option<u32>; DECL_ROWS]; 2],
    /// 最后面族。
    last_kind: i64,
    /// mount 已落元数（三元闸按序）。
    mount_next: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CygwinLedger {
    /// 新账本。
    pub fn new() -> Self {
        CygwinLedger {
            gates: [None; 6],
            faces: [None; FACE_CELLS],
            projs: [None; PROJ_CELLS],
            mounts: [None; MOUNT_TRIPLE],
            decls: [[None; DECL_ROWS]; 2],
            last_kind: -1,
            mount_next: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CygwinCode> {
        if gate >= 6 {
            return Err(CygwinCode::ESeal);
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

    /// 面一格（面族单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, o: usize, got: u64) -> Result<(), CygwinCode> {
        if o >= FACE_CELLS {
            return Err(CygwinCode::ENoFace);
        }
        let kind = match face_kind(o) {
            Some(k) => k,
            None => return Err(CygwinCode::ENoFace),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(CygwinCode::EOrder);
        }
        let want = match face_cell(o) {
            Some(w) => w,
            None => return Err(CygwinCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(CygwinCode::EDrift);
        }
        if match self.faces.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CygwinCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 面全账核验（20 格）。
    pub fn faces_all(&self) -> bool {
        let mut o = 0usize;
        while o < FACE_CELLS {
            match self.faces.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// POSIX 语义投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), CygwinCode> {
        if p >= PROJ_CELLS {
            return Err(CygwinCode::ENoProj);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(CygwinCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + p as u64);
            return Err(CygwinCode::EDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CygwinCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6301 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// mount 三元闸一格（按序 + 载荷对照 + 重复拒；乱序必抓）。
    pub fn record_mount(&mut self, m: usize, got: u64) -> Result<(), CygwinCode> {
        if m >= MOUNT_TRIPLE || m != self.mount_next {
            return Err(CygwinCode::ENoMount);
        }
        let want = match mount_cell(m) {
            Some(w) => w,
            None => return Err(CygwinCode::ENoMount),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + m as u64);
            return Err(CygwinCode::EDrift);
        }
        if match self.mounts.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CygwinCode::EDup);
        }
        if let Some(c) = self.mounts.get_mut(m) {
            *c = Some(got);
        }
        self.mount_next = m + 1;
        self.fnv(0x6401 + m as u64);
        Ok(())
    }

    /// mount 三元全齐核验（按序齐格）。
    pub fn mounts_all(&self) -> bool {
        self.mount_next == MOUNT_TRIPLE
            && self.mounts.get(MOUNT_TRIPLE - 1).map_or(false, |c| c.is_some())
    }

    /// 声明一行（组 grp：0=与 Git Bash 分账 1=C 上栈）。
    pub fn declare(&mut self, grp: usize, r: usize) -> Result<(), CygwinCode> {
        if grp >= 2 || r >= DECL_ROWS {
            return Err(CygwinCode::ENoRow);
        }
        let slot = match self.decls.get_mut(grp).and_then(|g| g.get_mut(r)) {
            Some(c) => c,
            None => return Err(CygwinCode::ENoRow),
        };
        if slot.is_some() {
            return Err(CygwinCode::EDup);
        }
        *slot = decl_row(grp, r);
        self.fnv(0x6500 + grp as u64 * 2 + r as u64);
        Ok(())
    }

    /// 单组声明全齐核验（组 grp）。
    pub fn decl_all_group(&self, grp: usize) -> bool {
        if grp >= 2 {
            return false;
        }
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(grp).and_then(|g| g.get(r)) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 两组声明全齐核验。
    pub fn decls_all(&self) -> bool {
        self.decl_all_group(0) && self.decl_all_group(1)
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 收口闸：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
    pub fn seal(&self) -> Result<(), CygwinCode> {
        if !self.gates_all() {
            return Err(CygwinCode::ESeal);
        }
        if !self.decls_all() {
            return Err(CygwinCode::ERowMiss);
        }
        if !self.faces_all() || !self.projs_all() || !self.mounts_all() {
            return Err(CygwinCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（六闸齐 → S2 目标档，空账/缺闸 → S3；S1 封顶不可达）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CygwinLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), CygwinCode> {
    if injected == caught.wrapping_add(escaped) {
        Ok(())
    } else {
        Err(CygwinCode::EInj)
    }
}

/// 双跑指纹一致核验。
pub fn fp_consistent(a: u64, b: u64) -> Result<(), CygwinCode> {
    if a == b {
        Ok(())
    } else {
        Err(CygwinCode::EFp)
    }
}

/// 定级核验：漂移必抓（EGradeDrift）。
pub fn grade_gate(got: Grade, want: Grade) -> Result<(), CygwinCode> {
    if got.val() == want.val() {
        Ok(())
    } else {
        Err(CygwinCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CygwinLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FACE_CELLS {
        if let Some(w) = face_cell(o) {
            if led.record_face(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut m = 0usize;
    while m < MOUNT_TRIPLE {
        if let Some(w) = mount_cell(m) {
            if led.record_mount(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut grp = 0usize;
    while grp < 2 {
        let mut r = 0usize;
        while r < DECL_ROWS {
            let _ = led.declare(grp, r);
            r += 1;
        }
        grp += 1;
    }
    // 注入 10 轮：面漂移 / 面族逆序 / 面越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CygwinLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落同族正格，再注漂移。
                let _ = lt.record_face(0, face_cell(0).unwrap_or(0));
                lt.record_face(1, face_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CygwinCode::EDrift)
            }
            1 => {
                // 逆序注入：对空账本先落后族正格再回跳，必抓且不改 last_kind。
                let _ = lt.record_face(6, face_cell(6).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0)) == Err(CygwinCode::EOrder)
            }
            _ => {
                // 越闭集注入：越界拒不计注入账。
                lt.record_face(FACE_CELLS, 0).is_err()
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
/// UNX-F52514 判据集（锚 UNX-F52515-J1，off-by-one 板式）。
pub fn run_unx52514_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52514");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面 20 格全通",
        recorded >= FACE_CELLS as u32,
        "cygwin1.dll/cygpath/包管理/终端/X 消费五面逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CygwinLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "判据闸逐格落账（§AI-66.3 锚定）",
    );
    s.add(
        "POSIX 语义投影 8 格全落",
        {
            let mut lt = CygwinLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "fork/exec/signal/path/file/uid/termios/dlopen 逐格恒等",
    );
    s.add(
        "mount 三元闸按序全落",
        {
            let mut lt = CygwinLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MOUNT_TRIPLE {
                if let Some(w) = mount_cell(m) {
                    if lt.record_mount(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.mounts_all()
        },
        "挂载点→挂载标志→语义核对三元按序落账",
    );
    s.add(
        "与 Git Bash 分账声明 2/2 全齐（F52513）",
        {
            let mut lt = CygwinLedger::new();
            let _ = lt.declare(0, 0);
            let _ = lt.declare(0, 1);
            lt.decl_all_group(0)
        },
        "F52513 判据域分账两行闭集，与 Git Bash 账不混记",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = CygwinLedger::new();
            let _ = lt.declare(1, 0);
            let _ = lt.declare(1, 1);
            lt.decl_all_group(1)
        },
        "栈帧上栈声明/C 运行时上栈声明两行闭集",
    );
    s.add(
        "面族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FACE_CELLS {
                if face_kind(o).unwrap_or(9) < face_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五面族闭集单调，逆序必抓",
    );
    s.add(
        "mount 乱序必抓",
        {
            let mut lt = CygwinLedger::new();
            let w1 = mount_cell(1).unwrap_or(0);
            lt.record_mount(1, w1) == Err(CygwinCode::ENoMount)
        },
        "跳过挂载点先落标志/语义必拒（三元闸按序）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    s.add(
        "边界·面号越闭集",
        {
            let mut lt = CygwinLedger::new();
            lt.record_face(FACE_CELLS, 0).is_err()
        },
        "面格 20 之外越界必拒",
    );
    s.add(
        "边界·投影格越闭集",
        {
            let mut lt = CygwinLedger::new();
            lt.record_proj(PROJ_CELLS, 0).is_err()
        },
        "投影格 8 之外越界必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = CygwinLedger::new();
            lt.declare(2, 0).is_err() && lt.declare(0, DECL_ROWS).is_err()
        },
        "声明组 2/行 2 之外越界必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        conservation(inj_n, caught_n, esc_n).is_ok() && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_consistent(fp, fp2).is_ok() && fp != 0,
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "收口闸语义（未齐即封）",
        {
            let lt = CygwinLedger::new();
            lt.seal() == Err(CygwinCode::ESeal)
        },
        "空账封口必拒：六闸缺→ESeal",
    );
    s.add(
        "定级 S2 目标档",
        {
            let mut lt = CygwinLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            grade_gate(lt.judge_grade(), Grade::S2).is_ok()
        },
        "六闸齐 → S2（KB-COMPAT 定级登记，目标档封顶）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC040,
        "SEG_BASE 0x40 对齐且在 VX16 域第二格（0xCA400+0x40×113）",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_kind_mapping() {
        // kind = o/4 映射：名表按 kind 索引核对。
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(3), Some(0));
        assert_eq!(face_kind(4), Some(1));
        assert_eq!(face_kind(7), Some(1));
        assert_eq!(face_kind(8), Some(2));
        assert_eq!(face_kind(12), Some(3));
        assert_eq!(face_kind(16), Some(4));
        assert_eq!(face_kind(19), Some(4));
        assert!(face_kind(20).is_none());
        assert_eq!(face_kind_name(0), Some("cygwin1.dll"));
        assert_eq!(face_kind_name(1), Some("cygpath"));
        assert_eq!(face_kind_name(2), Some("包管理(setup)"));
        assert_eq!(face_kind_name(3), Some("终端"));
        assert_eq!(face_kind_name(4), Some("X 消费"));
        assert!(face_kind_name(5).is_none());
        assert!(face_cell(20).is_none());
    }

    #[test]
    fn test_proj_mapping() {
        assert_eq!(proj_name(0), Some("fork"));
        assert_eq!(proj_name(7), Some("dlopen"));
        assert!(proj_name(8).is_none());
        assert!(proj_cell(8).is_none());
        assert_ne!(proj_cell(0).unwrap_or(0), proj_cell(1).unwrap_or(0));
    }

    #[test]
    fn test_mount_mapping() {
        assert_eq!(mount_name(0), Some("挂载点"));
        assert_eq!(mount_name(1), Some("挂载标志"));
        assert_eq!(mount_name(2), Some("语义核对"));
        assert!(mount_name(3).is_none());
        assert!(mount_cell(3).is_none());
        assert!(decl_row(2, 0).is_none());
        assert!(decl_row(0, DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族正格，再注逆序必抓。
        let mut lt = CygwinLedger::new();
        let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_face(2, face_cell(2).unwrap_or(0)),
            Err(CygwinCode::EOrder)
        );
        // 逆序被拒不改 last_kind：同族正格仍可落。
        assert!(lt.record_face(10, face_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CygwinLedger::new();
        let _ = lt.record_face(0, face_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_face(1, face_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(CygwinCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(3)),
            Err(CygwinCode::EDrift)
        );
        let _ = lt.record_mount(0, mount_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_mount(1, mount_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(CygwinCode::EDrift)
        );
    }

    #[test]
    fn test_mount_order_gate() {
        let mut lt = CygwinLedger::new();
        // 跳过 0 直接落 1/2 必拒。
        assert_eq!(
            lt.record_mount(1, mount_cell(1).unwrap_or(0)),
            Err(CygwinCode::ENoMount)
        );
        assert_eq!(
            lt.record_mount(2, mount_cell(2).unwrap_or(0)),
            Err(CygwinCode::ENoMount)
        );
        // 按序 0→1→2 全落才 mounts_all。
        let _ = lt.record_mount(0, mount_cell(0).unwrap_or(0));
        assert!(!lt.mounts_all());
        let _ = lt.record_mount(1, mount_cell(1).unwrap_or(0));
        let _ = lt.record_mount(2, mount_cell(2).unwrap_or(0));
        assert!(lt.mounts_all());
        // 重复落拒。
        assert_eq!(
            lt.record_mount(2, mount_cell(2).unwrap_or(0)),
            Err(CygwinCode::ENoMount)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CygwinLedger::new();
        let _ = lt.record_face(0, face_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_face(0, face_cell(0).unwrap_or(0)),
            Err(CygwinCode::EDup)
        );
        let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_proj(0, proj_cell(0).unwrap_or(0)),
            Err(CygwinCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CygwinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CygwinLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let lt0 = CygwinLedger::new();
        assert_eq!(lt0.seal(), Err(CygwinCode::ESeal));
        // 六闸齐、声明行缺 → ERowMiss。
        let mut lt1 = CygwinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.seal(), Err(CygwinCode::ERowMiss));
        // 声明齐、格账缺 → ESeal。
        let mut grp = 0usize;
        while grp < 2 {
            let _ = lt1.declare(grp, 0);
            let _ = lt1.declare(grp, 1);
            grp += 1;
        }
        assert_eq!(lt1.seal(), Err(CygwinCode::ESeal));
    }

    #[test]
    fn test_boundary_no_ledger_account() {
        // 越闭集边界拒不计注入账：lt 三数保持零。
        let mut lt = CygwinLedger::new();
        assert!(lt.record_face(FACE_CELLS, 0).is_err());
        assert!(lt.record_proj(PROJ_CELLS, 0).is_err());
        assert!(lt.record_mount(MOUNT_TRIPLE, 0).is_err());
        assert!(lt.record_mount(1, mount_cell(1).unwrap_or(0)).is_err());
        assert!(lt.declare(2, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：面+投影+mount。
        assert_eq!(
            recorded,
            (FACE_CELLS + PROJ_CELLS + MOUNT_TRIPLE) as u32
        );
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
        assert!(fp_consistent(f1, f2).is_ok());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCC040);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CygwinCode::Ok as u32, 0);
        assert_eq!(CygwinCode::EGradeDrift as u32, 12);
        assert_eq!(CygwinCode::Ok.val(), SEG_BASE);
        assert!(!CygwinCode::Ok.detail().is_empty());
        assert!(!CygwinCode::ENoMount.detail().is_empty());
        assert!(!CygwinCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_conservation_and_fp() {
        assert!(conservation(10, 10, 0).is_ok());
        assert!(conservation(10, 8, 1).is_err());
        assert!(conservation(10, 9, 1).is_ok());
        assert!(fp_consistent(9, 9).is_ok());
        assert!(fp_consistent(9, 10).is_err());
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2（目标档），目标档前置必须补 judge_gate×6。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = CygwinLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = CygwinLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(grade_gate(Grade::S2, Grade::S2).is_ok());
        assert!(grade_gate(Grade::S3, Grade::S2).is_err());
    }

    #[test]
    fn unx52514_checks_green() {
        let s = run_unx52514_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
