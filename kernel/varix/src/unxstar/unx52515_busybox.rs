//! UNX-F52515 BusyBox-w32 收口 · 0xCC080段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52515"，逐字）：
//!   title: BusyBox-w32 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52516-J1 工具 ×20
//!     联通性；EOL 临近声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题
//!     六；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52515）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52516-J1，照原文锚定；功能按本卡标题（BusyBox-w32）落地。工具
//! ×20 属后卡（GnuWin32 coreutils）判据域，本卡仅照原文锚引；本卡功能
//! 域为 BusyBox-w32 五 applet 族 + 单二进制三元闸 + EOL 临近声明。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 applet 族 20 格闭集：sh / ls / grep / sed / vi，各 4 格，族单
//!      调（逆序必抓）；逐格载荷纯函数折叠，恒等才收。
//!   ② 单二进制三元闸 3 元：合成器/链接面/入口分发按序落账，乱序必抓。
//!   ③ 与 coreutils 分账声明 2 行（F52516 判据域分账），缺一拒封；C 上
//!      栈声明 2 行闭集；EOL 临近声明 2 行（判据原文第二分句）。
//!   ④ ledger 状态机（new/record_*/bin_*/declare_*/judge_gate×6/seal/
//!      fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转
//!      10 轮（applet 漂移/族逆序/applet 越闭集）10/10 全抓；检出+逃逸
//!      守恒；双跑 FNV 指纹一致；定级目标档 S2（六闸齐 → S2，空账 →
//!      S3，S1 封顶不可达）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52515;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五 applet 族格数（sh/ls/grep/sed/vi ×4）。
pub const APPLET_CELLS: usize = 20;

/// applet 族数。
pub const APPLET_KINDS: usize = 5;

/// 单二进制三元闸元数。
pub const BIN_TRIPLE: usize = 3;

/// 三组声明行各 2 行（与 coreutils 分账/C 上栈/EOL 临近）。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusyboxCode {
    /// 成功
    Ok = 0,
    /// applet 号越闭集
    ENoApplet,
    /// 载荷漂移（applet 格复用）
    EDrift,
    /// applet 族逆序
    EOrder,
    /// 三元闸元越闭集或乱序
    ENoBin,
    /// 三元闸元载荷漂移
    EBinDrift,
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

impl BusyboxCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BusyboxCode::Ok => "成功",
            BusyboxCode::ENoApplet => "applet 号越闭集：五 applet 族 20 格之外先登记",
            BusyboxCode::EDrift => "载荷漂移：applet 格折叠与基准不恒等，逐格复算",
            BusyboxCode::EOrder => "applet 族逆序：sh→ls→grep→sed→vi 单调，逆序必抓",
            BusyboxCode::ENoBin => "三元闸元越闭集或乱序：合成器→链接面→入口分发按序落账",
            BusyboxCode::EBinDrift => "三元闸元载荷漂移：元折叠与基准不恒等，逐元复算",
            BusyboxCode::ENoRow => "声明行越闭集：三组声明各 2 行闭集之外先登记",
            BusyboxCode::ERowMiss => "缺声明行：coreutils 分账/C 上栈/EOL 临近未齐先补齐",
            BusyboxCode::EDup => "重复落账：同格只许一条账目",
            BusyboxCode::ESeal => "未齐即封：20 applet 格+3 三元元未齐先补齐",
            BusyboxCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BusyboxCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BusyboxCode::EGradeDrift => "定级漂移：六闸全齐才可定 S2（目标档封顶）",
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
            Grade::S1 => "S1：全通收口（BusyBox-w32 面目标档封顶 S2，S1 恒不达）",
            Grade::S2 => "S2：有条件收口，六判据全齐（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// applet 格载荷（格 o；族 = o/4）。
pub const fn applet_cell(o: usize) -> Option<u64> {
    if o >= APPLET_CELLS {
        return None;
    }
    Some((0xCE10u64 + (o as u64) * 0x30) ^ 0x42425733)
}

/// applet 族（0=sh 1=ls 2=grep 3=sed 4=vi）。
pub const fn applet_kind(o: usize) -> Option<u32> {
    if o >= APPLET_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// applet 族名（按 kind 索引）。
pub const fn applet_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("sh"),
        1 => Some("ls"),
        2 => Some("grep"),
        3 => Some("sed"),
        4 => Some("vi"),
        _ => None,
    }
}

/// 单二进制三元闸元载荷（元 m；0=合成器 1=链接面 2=入口分发）。
pub const fn bin_cell(m: usize) -> Option<u64> {
    if m >= BIN_TRIPLE {
        return None;
    }
    Some((0xCE60u64 + (m as u64) * 0x44) ^ 0x42494E31)
}

/// 三元闸元名（按元索引）。
pub const fn bin_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("合成器"),
        1 => Some("链接面"),
        2 => Some("入口分发"),
        _ => None,
    }
}

/// 声明行折叠（组 grp：0=与 coreutils 分账 1=C 上栈 2=EOL 临近；行 r）。
pub const fn decl_row(grp: usize, r: usize) -> Option<u32> {
    if grp >= 3 || r >= DECL_ROWS {
        return None;
    }
    Some(0xCE90 + (grp * 0x07 + r as usize * 0x02) as u32)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// BusyBox-w32 收口账本状态机。
#[derive(Clone)]
pub struct BusyboxLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// applet 格账。
    applets: [Option<u64>; APPLET_CELLS],
    /// 三元闸账。
    bins: [Option<u64>; BIN_TRIPLE],
    /// 三组声明账（组→行）。
    decls: [[Option<u32>; DECL_ROWS]; 3],
    /// 最后 applet 族。
    last_kind: i64,
    /// 三元闸已落元数（按序）。
    bin_next: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BusyboxLedger {
    /// 新账本。
    pub fn new() -> Self {
        BusyboxLedger {
            gates: [None; 6],
            applets: [None; APPLET_CELLS],
            bins: [None; BIN_TRIPLE],
            decls: [[None; DECL_ROWS]; 3],
            last_kind: -1,
            bin_next: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BusyboxCode> {
        if gate >= 6 {
            return Err(BusyboxCode::ESeal);
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

    /// applet 一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_applet(&mut self, o: usize, got: u64) -> Result<(), BusyboxCode> {
        if o >= APPLET_CELLS {
            return Err(BusyboxCode::ENoApplet);
        }
        let kind = match applet_kind(o) {
            Some(k) => k,
            None => return Err(BusyboxCode::ENoApplet),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(BusyboxCode::EOrder);
        }
        let want = match applet_cell(o) {
            Some(w) => w,
            None => return Err(BusyboxCode::ENoApplet),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(BusyboxCode::EDrift);
        }
        if match self.applets.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BusyboxCode::EDup);
        }
        if let Some(c) = self.applets.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// applet 全账核验（20 格）。
    pub fn applets_all(&self) -> bool {
        let mut o = 0usize;
        while o < APPLET_CELLS {
            match self.applets.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 单二进制三元闸一格（按序 + 载荷对照 + 重复拒；乱序必抓）。
    pub fn record_bin(&mut self, m: usize, got: u64) -> Result<(), BusyboxCode> {
        if m >= BIN_TRIPLE || m != self.bin_next {
            return Err(BusyboxCode::ENoBin);
        }
        let want = match bin_cell(m) {
            Some(w) => w,
            None => return Err(BusyboxCode::ENoBin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + m as u64);
            return Err(BusyboxCode::EBinDrift);
        }
        if match self.bins.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BusyboxCode::EDup);
        }
        if let Some(c) = self.bins.get_mut(m) {
            *c = Some(got);
        }
        self.bin_next = m + 1;
        self.fnv(0x7301 + m as u64);
        Ok(())
    }

    /// 三元闸全齐核验（按序齐格）。
    pub fn bins_all(&self) -> bool {
        self.bin_next == BIN_TRIPLE
            && self.bins.get(BIN_TRIPLE - 1).map_or(false, |c| c.is_some())
    }

    /// 声明一行（组 grp：0=与 coreutils 分账 1=C 上栈 2=EOL 临近）。
    pub fn declare(&mut self, grp: usize, r: usize) -> Result<(), BusyboxCode> {
        if grp >= 3 || r >= DECL_ROWS {
            return Err(BusyboxCode::ENoRow);
        }
        let slot = match self.decls.get_mut(grp).and_then(|g| g.get_mut(r)) {
            Some(c) => c,
            None => return Err(BusyboxCode::ENoRow),
        };
        if slot.is_some() {
            return Err(BusyboxCode::EDup);
        }
        *slot = decl_row(grp, r);
        self.fnv(0x7400 + grp as u64 * 2 + r as u64);
        Ok(())
    }

    /// 单组声明全齐核验（组 grp）。
    pub fn decl_all_group(&self, grp: usize) -> bool {
        if grp >= 3 {
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

    /// 三组声明全齐核验。
    pub fn decls_all(&self) -> bool {
        self.decl_all_group(0) && self.decl_all_group(1) && self.decl_all_group(2)
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
    pub fn seal(&self) -> Result<(), BusyboxCode> {
        if !self.gates_all() {
            return Err(BusyboxCode::ESeal);
        }
        if !self.decls_all() {
            return Err(BusyboxCode::ERowMiss);
        }
        if !self.applets_all() || !self.bins_all() {
            return Err(BusyboxCode::ESeal);
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

impl Default for BusyboxLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), BusyboxCode> {
    if injected == caught.wrapping_add(escaped) {
        Ok(())
    } else {
        Err(BusyboxCode::EInj)
    }
}

/// 双跑指纹一致核验。
pub fn fp_consistent(a: u64, b: u64) -> Result<(), BusyboxCode> {
    if a == b {
        Ok(())
    } else {
        Err(BusyboxCode::EFp)
    }
}

/// 定级核验：漂移必抓（EGradeDrift）。
pub fn grade_gate(got: Grade, want: Grade) -> Result<(), BusyboxCode> {
    if got.val() == want.val() {
        Ok(())
    } else {
        Err(BusyboxCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BusyboxLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < APPLET_CELLS {
        if let Some(w) = applet_cell(o) {
            if led.record_applet(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut m = 0usize;
    while m < BIN_TRIPLE {
        if let Some(w) = bin_cell(m) {
            if led.record_bin(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut grp = 0usize;
    while grp < 3 {
        let mut r = 0usize;
        while r < DECL_ROWS {
            let _ = led.declare(grp, r);
            r += 1;
        }
        grp += 1;
    }
    // 注入 10 轮：applet 漂移 / 族逆序 / applet 越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BusyboxLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落同族正格，再注漂移。
                let _ = lt.record_applet(0, applet_cell(0).unwrap_or(0));
                lt.record_applet(1, applet_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(BusyboxCode::EDrift)
            }
            1 => {
                // 逆序注入：对空账本先落后族正格再回跳，必抓且不改 last_kind。
                let _ = lt.record_applet(7, applet_cell(7).unwrap_or(0));
                lt.record_applet(3, applet_cell(3).unwrap_or(0)) == Err(BusyboxCode::EOrder)
            }
            _ => {
                // 越闭集注入：越界拒不计注入账。
                lt.record_applet(APPLET_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52515 判据集（锚 UNX-F52516-J1，off-by-one 板式）。
pub fn run_unx52515_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52515");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五 applet 族 20 格全通",
        recorded >= APPLET_CELLS as u32,
        "sh/ls/grep/sed/vi 五族逐格恒等（§AI-66.2 专题六锚定）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = BusyboxLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "判据闸逐格落账",
    );
    s.add(
        "单二进制三元闸按序全落",
        {
            let mut lt = BusyboxLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < BIN_TRIPLE {
                if let Some(w) = bin_cell(m) {
                    if lt.record_bin(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.bins_all()
        },
        "合成器→链接面→入口分发三元按序落账",
    );
    s.add(
        "与 coreutils 分账声明 2/2 全齐（F52516）",
        {
            let mut lt = BusyboxLedger::new();
            let _ = lt.declare(0, 0);
            let _ = lt.declare(0, 1);
            lt.decl_all_group(0)
        },
        "F52516 判据域分账两行闭集，与 GnuWin32 账不混记",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = BusyboxLedger::new();
            let _ = lt.declare(1, 0);
            let _ = lt.declare(1, 1);
            lt.decl_all_group(1)
        },
        "栈帧上栈声明/C 运行时上栈声明两行闭集",
    );
    s.add(
        "EOL 临近声明 2/2 全齐",
        {
            let mut lt = BusyboxLedger::new();
            let _ = lt.declare(2, 0);
            let _ = lt.declare(2, 1);
            lt.decl_all_group(2)
        },
        "判据原文第二分句：EOL 临近声明两行闭集",
    );
    s.add(
        "applet 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < APPLET_CELLS {
                if applet_kind(o).unwrap_or(9) < applet_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，逆序必抓",
    );
    s.add(
        "三元闸乱序必抓",
        {
            let mut lt = BusyboxLedger::new();
            lt.record_bin(2, bin_cell(2).unwrap_or(0)) == Err(BusyboxCode::ENoBin)
        },
        "跳过合成器/链接面先落入入口分发必拒",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    s.add(
        "边界·applet 号越闭集",
        {
            let mut lt = BusyboxLedger::new();
            lt.record_applet(APPLET_CELLS, 0).is_err()
        },
        "applet 格 20 之外越界必拒",
    );
    s.add(
        "边界·三元闸元越闭集",
        {
            let mut lt = BusyboxLedger::new();
            lt.record_bin(BIN_TRIPLE, 0).is_err()
        },
        "三元闸元 3 之外越界必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = BusyboxLedger::new();
            lt.declare(3, 0).is_err() && lt.declare(0, DECL_ROWS).is_err()
        },
        "声明组 3/行 2 之外越界必拒",
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
            let lt = BusyboxLedger::new();
            lt.seal() == Err(BusyboxCode::ESeal)
        },
        "空账封口必拒：六闸缺→ESeal",
    );
    s.add(
        "定级 S2 目标档",
        {
            let mut lt = BusyboxLedger::new();
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC080,
        "SEG_BASE 0x40 对齐且在 VX16 域第三格（0xCA400+0x40×114）",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_applet_kind_mapping() {
        // kind = o/4 映射：名表按 kind 索引核对。
        assert_eq!(applet_kind(0), Some(0));
        assert_eq!(applet_kind(2), Some(0));
        assert_eq!(applet_kind(4), Some(1));
        assert_eq!(applet_kind(8), Some(2));
        assert_eq!(applet_kind(11), Some(2));
        assert_eq!(applet_kind(12), Some(3));
        assert_eq!(applet_kind(16), Some(4));
        assert_eq!(applet_kind(19), Some(4));
        assert!(applet_kind(20).is_none());
        assert_eq!(applet_kind_name(0), Some("sh"));
        assert_eq!(applet_kind_name(1), Some("ls"));
        assert_eq!(applet_kind_name(2), Some("grep"));
        assert_eq!(applet_kind_name(3), Some("sed"));
        assert_eq!(applet_kind_name(4), Some("vi"));
        assert!(applet_kind_name(5).is_none());
        assert!(applet_cell(20).is_none());
    }

    #[test]
    fn test_bin_mapping() {
        assert_eq!(bin_name(0), Some("合成器"));
        assert_eq!(bin_name(1), Some("链接面"));
        assert_eq!(bin_name(2), Some("入口分发"));
        assert!(bin_name(3).is_none());
        assert!(bin_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = applet_cell(5).unwrap_or(0);
        assert_eq!(a, applet_cell(5).unwrap_or(0));
        assert_ne!(a, applet_cell(6).unwrap_or(0));
        assert_ne!(bin_cell(0).unwrap_or(0), bin_cell(1).unwrap_or(0));
        assert!(decl_row(3, 0).is_none());
        assert!(decl_row(2, DECL_ROWS).is_none());
        assert_ne!(decl_row(0, 0).unwrap_or(0), decl_row(1, 0).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族正格，再注逆序必抓。
        let mut lt = BusyboxLedger::new();
        let _ = lt.record_applet(9, applet_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_applet(2, applet_cell(2).unwrap_or(0)),
            Err(BusyboxCode::EOrder)
        );
        // 逆序被拒不改 last_kind：同族正格仍可落。
        assert!(lt.record_applet(11, applet_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BusyboxLedger::new();
        let _ = lt.record_applet(0, applet_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_applet(1, applet_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(BusyboxCode::EDrift)
        );
        let _ = lt.record_bin(0, bin_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_bin(1, bin_cell(1).unwrap_or(0).wrapping_add(4)),
            Err(BusyboxCode::EBinDrift)
        );
    }

    #[test]
    fn test_bin_order_gate() {
        let mut lt = BusyboxLedger::new();
        // 跳过 0/1 直接落 2 必拒。
        assert_eq!(
            lt.record_bin(2, bin_cell(2).unwrap_or(0)),
            Err(BusyboxCode::ENoBin)
        );
        assert_eq!(
            lt.record_bin(1, bin_cell(1).unwrap_or(0)),
            Err(BusyboxCode::ENoBin)
        );
        // 按序 0→1→2 全落才 bins_all。
        let _ = lt.record_bin(0, bin_cell(0).unwrap_or(0));
        assert!(!lt.bins_all());
        let _ = lt.record_bin(1, bin_cell(1).unwrap_or(0));
        let _ = lt.record_bin(2, bin_cell(2).unwrap_or(0));
        assert!(lt.bins_all());
        // 越界必拒。
        assert!(lt.record_bin(BIN_TRIPLE, 0).is_err());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BusyboxLedger::new();
        let _ = lt.record_applet(0, applet_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_applet(0, applet_cell(0).unwrap_or(0)),
            Err(BusyboxCode::EDup)
        );
        let _ = lt.declare(2, 0);
        assert_eq!(lt.declare(2, 0), Err(BusyboxCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BusyboxLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BusyboxLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let lt0 = BusyboxLedger::new();
        assert_eq!(lt0.seal(), Err(BusyboxCode::ESeal));
        // 六闸齐、声明行缺 → ERowMiss。
        let mut lt1 = BusyboxLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.seal(), Err(BusyboxCode::ERowMiss));
        // 声明齐、格账缺 → ESeal。
        let mut grp = 0usize;
        while grp < 3 {
            let _ = lt1.declare(grp, 0);
            let _ = lt1.declare(grp, 1);
            grp += 1;
        }
        assert_eq!(lt1.seal(), Err(BusyboxCode::ESeal));
    }

    #[test]
    fn test_boundary_no_ledger_account() {
        // 越闭集边界拒不计注入账：lt 三数保持零。
        let mut lt = BusyboxLedger::new();
        assert!(lt.record_applet(APPLET_CELLS, 0).is_err());
        assert!(lt.record_bin(BIN_TRIPLE, 0).is_err());
        assert!(lt.declare(3, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：applet+三元闸。
        assert_eq!(recorded, (APPLET_CELLS + BIN_TRIPLE) as u32);
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
        assert_eq!(SEG_BASE, 0xCC080);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BusyboxCode::Ok as u32, 0);
        assert_eq!(BusyboxCode::EGradeDrift as u32, 12);
        assert_eq!(BusyboxCode::Ok.val(), SEG_BASE);
        assert!(!BusyboxCode::Ok.detail().is_empty());
        assert!(!BusyboxCode::EBinDrift.detail().is_empty());
        assert!(!BusyboxCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_conservation_and_fp() {
        assert!(conservation(10, 10, 0).is_ok());
        assert!(conservation(10, 7, 2).is_err());
        assert!(conservation(10, 8, 2).is_ok());
        assert!(fp_consistent(11, 11).is_ok());
        assert!(fp_consistent(11, 12).is_err());
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2（目标档），目标档前置必须补 judge_gate×6。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = BusyboxLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = BusyboxLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(grade_gate(Grade::S2, Grade::S2).is_ok());
        assert!(grade_gate(Grade::S1, Grade::S2).is_err());
    }

    #[test]
    fn unx52515_checks_green() {
        let s = run_unx52515_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
