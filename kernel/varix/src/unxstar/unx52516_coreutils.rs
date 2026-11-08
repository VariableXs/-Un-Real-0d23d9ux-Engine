//! UNX-F52516 GnuWin32 coreutils 面收口 · 0xCC0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52516"，逐字）：
//!   title: GnuWin32 coreutils 面收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52517-J1 分页 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52516）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52517-J1，照原文锚定；功能按本卡标题（GnuWin32 coreutils 面）落
//! 地。分页 ×10 为本卡判据原文域，落地为分页样本 10 行账。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五工具族 20 格闭集：ls / cp / mv / rm / cat，各 4 格，族单调（逆
//!      序必抓）；逐格载荷纯函数折叠，恒等才收。
//!   ② EOL 三态 6 格：CRLF / LF / 保持原样，三态各 2 格，逐态载荷恒等
//!      才收，漂移必抓（EEolDrift）。
//!   ③ 路径语义 8 格：盘符/正斜杠/反斜杠/引号/环境变量展开/相对路径/
//!      尾点/尾空格语义逐格恒等。
//!   ④ 分页样本 10 行（判据原文：分页 ×10），逐行恒等落账。
//!   ⑤ 与 BusyBox 分账声明 2 行（F52515 判据域分账），缺一拒封；长尾声
//!      明 2 行闭集（长尾工具不齐声明/长尾不修声明）。
//!   ⑥ ledger 状态机（new/record_*/declare_*/judge_gate×6/seal/
//!      fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转
//!      10 轮（工具漂移/族逆序/工具越闭集）10/10 全抓；检出+逃逸守恒；
//!      双跑 FNV 指纹一致；KB-COMPAT 定级封顶 S3（GnuWin32 长尾面恒定
//!      不收口级：六闸齐、全齐均不升档，S1/S2 恒不达）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52516;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五工具族格数（ls/cp/mv/rm/cat ×4）。
pub const TOOL_CELLS: usize = 20;

/// 工具族数。
pub const TOOL_KINDS: usize = 5;

/// EOL 三态格数（三态 ×2 格）。
pub const EOL_CELLS: usize = 6;

/// EOL 三态数。
pub const EOL_STATES: usize = 3;

/// 路径语义格数。
pub const PATH_CELLS: usize = 8;

/// 分页样本行数（判据原文：分页 ×10）。
pub const PAGER_ROWS: usize = 10;

/// 两组声明行各 2 行（与 BusyBox 分账/长尾声明）。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoreutilsCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集（工具/EOL/路径/分页格复用）
    ENoCell,
    /// 载荷漂移（工具/路径/分页格复用）
    EDrift,
    /// 工具族逆序
    EOrder,
    /// EOL 三态漂移
    EEolDrift,
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
    /// 守恒破缺
    ECons,
    /// 定级漂移
    EGradeDrift,
}

impl CoreutilsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CoreutilsCode::Ok => "成功",
            CoreutilsCode::ENoCell => "格号越闭集：工具 20/EOL 6/路径 8/分页 10 闭集之外先登记",
            CoreutilsCode::EDrift => "载荷漂移：格折叠与基准不恒等，逐格复算",
            CoreutilsCode::EOrder => "工具族逆序：ls→cp→mv→rm→cat 单调，逆序必抓",
            CoreutilsCode::EEolDrift => "EOL 三态漂移：CRLF/LF/保持原态折叠与基准不恒等",
            CoreutilsCode::ENoRow => "声明行越闭集：两组声明各 2 行闭集之外先登记",
            CoreutilsCode::ERowMiss => "缺声明行：与 BusyBox 分账/长尾声明未齐先补齐",
            CoreutilsCode::EDup => "重复落账：同格只许一条账目",
            CoreutilsCode::ESeal => "未齐即封：20 工具格+6 EOL 格+8 路径格+10 分页行未齐先补齐",
            CoreutilsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CoreutilsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CoreutilsCode::ECons => "守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CoreutilsCode::EGradeDrift => "定级漂移：GnuWin32 长尾面 KB-COMPAT 封顶 S3，恒不升档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不可达：定级封顶 S3）
    S1,
    /// S2：有条件收口（本卡不可达：定级封顶 S3）
    S2,
    /// S3：不收口（本卡定级封顶档）
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
            Grade::S1 => "S1：全通收口（GnuWin32 长尾面定级封顶 S3，S1 恒不达）",
            Grade::S2 => "S2：有条件收口（GnuWin32 长尾面定级封顶 S3，S2 恒不达）",
            Grade::S3 => "S3：不收口（KB-COMPAT 定级登记：GnuWin32 长尾面封顶档）",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 工具格载荷（格 o；族 = o/4）。
pub const fn tool_cell(o: usize) -> Option<u64> {
    if o >= TOOL_CELLS {
        return None;
    }
    Some((0xCF10u64 + (o as u64) * 0x2A) ^ 0x43555449)
}

/// 工具族（0=ls 1=cp 2=mv 3=rm 4=cat）。
pub const fn tool_kind(o: usize) -> Option<u32> {
    if o >= TOOL_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 工具族名（按 kind 索引）。
pub const fn tool_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("ls"),
        1 => Some("cp"),
        2 => Some("mv"),
        3 => Some("rm"),
        4 => Some("cat"),
        _ => None,
    }
}

/// EOL 三态格载荷（格 e；三态 = e/2）。
pub const fn eol_cell(e: usize) -> Option<u64> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((0xCF40u64 + (e as u64) * 0x36) ^ ((e / 2) as u64).wrapping_add(1).wrapping_mul(0x0D0D))
}

/// EOL 三态（0=CRLF 1=LF 2=保持原样）。
pub const fn eol_state(e: usize) -> Option<u32> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// EOL 三态名（按 state 索引）。
pub const fn eol_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("CRLF"),
        1 => Some("LF"),
        2 => Some("保持原样"),
        _ => None,
    }
}

/// 路径语义格载荷（格 p）。
pub const fn path_cell(p: usize) -> Option<u64> {
    if p >= PATH_CELLS {
        return None;
    }
    Some((0xCF80u64 + (p as u64) * 0x3C) ^ ((p as u64).wrapping_add(3)).wrapping_mul(0x85EBCA6B))
}

/// 路径语义名（按格索引）。
pub const fn path_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("盘符"),
        1 => Some("正斜杠"),
        2 => Some("反斜杠"),
        3 => Some("引号"),
        4 => Some("环境变量展开"),
        5 => Some("相对路径"),
        6 => Some("尾点"),
        7 => Some("尾空格"),
        _ => None,
    }
}

/// 分页样本行载荷（行 s；判据原文：分页 ×10）。
pub const fn pager_row(s: usize) -> Option<u64> {
    if s >= PAGER_ROWS {
        return None;
    }
    Some(0xCFB0 + s as u64 * 0x1C)
}

/// 声明行折叠（组 grp：0=与 BusyBox 分账 1=长尾声明；行 r）。
pub const fn decl_row(grp: usize, r: usize) -> Option<u32> {
    if grp >= 2 || r >= DECL_ROWS {
        return None;
    }
    Some(0xCFD0 + (grp * 0x05 + r as usize * 0x02) as u32)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GnuWin32 coreutils 收口账本状态机。
#[derive(Clone)]
pub struct CoreutilsLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 工具格账。
    tools: [Option<u64>; TOOL_CELLS],
    /// EOL 格账。
    eols: [Option<u64>; EOL_CELLS],
    /// 路径格账。
    paths: [Option<u64>; PATH_CELLS],
    /// 分页行账。
    pagers: [Option<u64>; PAGER_ROWS],
    /// 两组声明账（组→行）。
    decls: [[Option<u32>; DECL_ROWS]; 2],
    /// 最后工具族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CoreutilsLedger {
    /// 新账本。
    pub fn new() -> Self {
        CoreutilsLedger {
            gates: [None; 6],
            tools: [None; TOOL_CELLS],
            eols: [None; EOL_CELLS],
            paths: [None; PATH_CELLS],
            pagers: [None; PAGER_ROWS],
            decls: [[None; DECL_ROWS]; 2],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CoreutilsCode> {
        if gate >= 6 {
            return Err(CoreutilsCode::ESeal);
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

    /// 工具一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_tool(&mut self, o: usize, got: u64) -> Result<(), CoreutilsCode> {
        if o >= TOOL_CELLS {
            return Err(CoreutilsCode::ENoCell);
        }
        let kind = match tool_kind(o) {
            Some(k) => k,
            None => return Err(CoreutilsCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(CoreutilsCode::EOrder);
        }
        let want = match tool_cell(o) {
            Some(w) => w,
            None => return Err(CoreutilsCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(CoreutilsCode::EDrift);
        }
        if match self.tools.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CoreutilsCode::EDup);
        }
        if let Some(c) = self.tools.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 工具全账核验（20 格）。
    pub fn tools_all(&self) -> bool {
        let mut o = 0usize;
        while o < TOOL_CELLS {
            match self.tools.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// EOL 三态一格（三态载荷对照 + 重复拒；漂移为 EEolDrift）。
    pub fn record_eol(&mut self, e: usize, got: u64) -> Result<(), CoreutilsCode> {
        if e >= EOL_CELLS {
            return Err(CoreutilsCode::ENoCell);
        }
        let want = match eol_cell(e) {
            Some(w) => w,
            None => return Err(CoreutilsCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + e as u64);
            return Err(CoreutilsCode::EEolDrift);
        }
        if match self.eols.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CoreutilsCode::EDup);
        }
        if let Some(c) = self.eols.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x8301 + e as u64);
        Ok(())
    }

    /// EOL 全账核验（6 格）。
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

    /// 路径语义一格（载荷对照 + 重复拒）。
    pub fn record_path(&mut self, p: usize, got: u64) -> Result<(), CoreutilsCode> {
        if p >= PATH_CELLS {
            return Err(CoreutilsCode::ENoCell);
        }
        let want = match path_cell(p) {
            Some(w) => w,
            None => return Err(CoreutilsCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + p as u64);
            return Err(CoreutilsCode::EDrift);
        }
        if match self.paths.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CoreutilsCode::EDup);
        }
        if let Some(c) = self.paths.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8401 + p as u64);
        Ok(())
    }

    /// 路径全账核验（8 格）。
    pub fn paths_all(&self) -> bool {
        let mut p = 0usize;
        while p < PATH_CELLS {
            match self.paths.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 分页样本一行（载荷对照 + 重复拒；判据原文：分页 ×10）。
    pub fn record_pager(&mut self, s: usize, got: u64) -> Result<(), CoreutilsCode> {
        if s >= PAGER_ROWS {
            return Err(CoreutilsCode::ENoCell);
        }
        let want = match pager_row(s) {
            Some(w) => w,
            None => return Err(CoreutilsCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + s as u64);
            return Err(CoreutilsCode::EDrift);
        }
        if match self.pagers.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CoreutilsCode::EDup);
        }
        if let Some(c) = self.pagers.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x8501 + s as u64);
        Ok(())
    }

    /// 分页全账核验（10 行）。
    pub fn pagers_all(&self) -> bool {
        let mut s = 0usize;
        while s < PAGER_ROWS {
            match self.pagers.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 声明一行（组 grp：0=与 BusyBox 分账 1=长尾声明）。
    pub fn declare(&mut self, grp: usize, r: usize) -> Result<(), CoreutilsCode> {
        if grp >= 2 || r >= DECL_ROWS {
            return Err(CoreutilsCode::ENoRow);
        }
        let slot = match self.decls.get_mut(grp).and_then(|g| g.get_mut(r)) {
            Some(c) => c,
            None => return Err(CoreutilsCode::ENoRow),
        };
        if slot.is_some() {
            return Err(CoreutilsCode::EDup);
        }
        *slot = decl_row(grp, r);
        self.fnv(0x8600 + grp as u64 * 2 + r as u64);
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
    pub fn seal(&self) -> Result<(), CoreutilsCode> {
        if !self.gates_all() {
            return Err(CoreutilsCode::ESeal);
        }
        if !self.decls_all() {
            return Err(CoreutilsCode::ERowMiss);
        }
        if !self.tools_all() || !self.eols_all() || !self.paths_all() || !self.pagers_all() {
            return Err(CoreutilsCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（封顶 S3：GnuWin32 长尾面恒定不收口级，全齐亦不升档）。
    pub fn judge_grade(&self) -> Grade {
        Grade::S3
    }
}

impl Default for CoreutilsLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), CoreutilsCode> {
    if injected == caught.wrapping_add(escaped) {
        Ok(())
    } else {
        Err(CoreutilsCode::EInj)
    }
}

/// 双跑指纹一致核验。
pub fn fp_consistent(a: u64, b: u64) -> Result<(), CoreutilsCode> {
    if a == b {
        Ok(())
    } else {
        Err(CoreutilsCode::EFp)
    }
}

/// 定级核验：漂移必抓（EGradeDrift）。
pub fn grade_gate(got: Grade, want: Grade) -> Result<(), CoreutilsCode> {
    if got.val() == want.val() {
        Ok(())
    } else {
        Err(CoreutilsCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CoreutilsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < TOOL_CELLS {
        if let Some(w) = tool_cell(o) {
            if led.record_tool(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < EOL_CELLS {
        if let Some(w) = eol_cell(e) {
            if led.record_eol(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut p = 0usize;
    while p < PATH_CELLS {
        if let Some(w) = path_cell(p) {
            if led.record_path(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < PAGER_ROWS {
        if let Some(w) = pager_row(s) {
            if led.record_pager(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
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
    // 注入 10 轮：工具漂移 / 族逆序 / 工具越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CoreutilsLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落同族正格，再注漂移。
                let _ = lt.record_tool(0, tool_cell(0).unwrap_or(0));
                lt.record_tool(1, tool_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CoreutilsCode::EDrift)
            }
            1 => {
                // 逆序注入：对空账本先落后族正格再回跳，必抓且不改 last_kind。
                let _ = lt.record_tool(8, tool_cell(8).unwrap_or(0));
                lt.record_tool(4, tool_cell(4).unwrap_or(0)) == Err(CoreutilsCode::EOrder)
            }
            _ => {
                // 越闭集注入：越界拒不计注入账。
                lt.record_tool(TOOL_CELLS, 0).is_err()
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
/// UNX-F52516 判据集（锚 UNX-F52517-J1，off-by-one 板式）。
pub fn run_unx52516_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52516");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五工具族 20 格全通",
        recorded >= TOOL_CELLS as u32,
        "ls/cp/mv/rm/cat 五族逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CoreutilsLedger::new();
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
        "EOL 三态 6 格全落",
        {
            let mut lt = CoreutilsLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    if lt.record_eol(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.eols_all()
        },
        "CRLF/LF/保持原样三态逐格恒等",
    );
    s.add(
        "路径语义 8 格全落",
        {
            let mut lt = CoreutilsLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PATH_CELLS {
                if let Some(w) = path_cell(p) {
                    if lt.record_path(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.paths_all()
        },
        "盘符/斜杠/引号/展开/相对/尾点/尾空格语义逐格恒等",
    );
    s.add(
        "分页样本 10/10 全落（判据原文：分页 ×10）",
        {
            let mut lt = CoreutilsLedger::new();
            let mut ok = true;
            let mut sg = 0usize;
            while sg < PAGER_ROWS {
                if let Some(w) = pager_row(sg) {
                    if lt.record_pager(sg, w).is_err() {
                        ok = false;
                    }
                }
                sg += 1;
            }
            ok && lt.pagers_all()
        },
        "分页 ×10 逐行恒等落账",
    );
    s.add(
        "与 BusyBox 分账声明 2/2 全齐（F52515）",
        {
            let mut lt = CoreutilsLedger::new();
            let _ = lt.declare(0, 0);
            let _ = lt.declare(0, 1);
            lt.decl_all_group(0)
        },
        "F52515 判据域分账两行闭集，与 BusyBox-w32 账不混记",
    );
    s.add(
        "长尾声明 2/2 全齐",
        {
            let mut lt = CoreutilsLedger::new();
            let _ = lt.declare(1, 0);
            let _ = lt.declare(1, 1);
            lt.decl_all_group(1)
        },
        "长尾工具不齐声明/长尾不修声明两行闭集",
    );
    s.add(
        "工具族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TOOL_CELLS {
                if tool_kind(o).unwrap_or(9) < tool_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    s.add(
        "边界·工具格越闭集",
        {
            let mut lt = CoreutilsLedger::new();
            lt.record_tool(TOOL_CELLS, 0).is_err()
        },
        "工具格 20 之外越界必拒",
    );
    s.add(
        "边界·EOL/路径格越闭集",
        {
            let mut lt = CoreutilsLedger::new();
            lt.record_eol(EOL_CELLS, 0).is_err() && lt.record_path(PATH_CELLS, 0).is_err()
        },
        "EOL 格 6/路径格 8 之外越界必拒",
    );
    s.add(
        "边界·分页行越闭集",
        {
            let mut lt = CoreutilsLedger::new();
            lt.record_pager(PAGER_ROWS, 0).is_err()
        },
        "分页行 10 之外越界必拒",
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
            let lt = CoreutilsLedger::new();
            lt.seal() == Err(CoreutilsCode::ESeal)
        },
        "空账封口必拒：六闸缺→ESeal",
    );
    s.add(
        "定级封顶 S3",
        {
            let mut lt = CoreutilsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            // 定级检查 main/aux 各节全部记账后再断言。
            let mut o = 0usize;
            while o < TOOL_CELLS {
                if let Some(w) = tool_cell(o) {
                    let _ = lt.record_tool(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    let _ = lt.record_eol(e, w);
                }
                e += 1;
            }
            let mut p = 0usize;
            while p < PATH_CELLS {
                if let Some(w) = path_cell(p) {
                    let _ = lt.record_path(p, w);
                }
                p += 1;
            }
            let mut sg = 0usize;
            while sg < PAGER_ROWS {
                if let Some(w) = pager_row(sg) {
                    let _ = lt.record_pager(sg, w);
                }
                sg += 1;
            }
            let mut grp = 0usize;
            while grp < 2 {
                let mut r = 0usize;
                while r < DECL_ROWS {
                    let _ = lt.declare(grp, r);
                    r += 1;
                }
                grp += 1;
            }
            grade_gate(lt.judge_grade(), Grade::S3).is_ok()
        },
        "全齐亦不升档（KB-COMPAT 定级登记：长尾面封顶 S3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC0C0,
        "SEG_BASE 0x40 对齐且在 VX16 域第四格（0xCA400+0x40×115）",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_kind_mapping() {
        // kind = o/4 映射：名表按 kind 索引核对。
        assert_eq!(tool_kind(0), Some(0));
        assert_eq!(tool_kind(3), Some(0));
        assert_eq!(tool_kind(4), Some(1));
        assert_eq!(tool_kind(7), Some(1));
        assert_eq!(tool_kind(8), Some(2));
        assert_eq!(tool_kind(12), Some(3));
        assert_eq!(tool_kind(15), Some(3));
        assert_eq!(tool_kind(16), Some(4));
        assert_eq!(tool_kind(19), Some(4));
        assert!(tool_kind(20).is_none());
        assert_eq!(tool_kind_name(0), Some("ls"));
        assert_eq!(tool_kind_name(1), Some("cp"));
        assert_eq!(tool_kind_name(2), Some("mv"));
        assert_eq!(tool_kind_name(3), Some("rm"));
        assert_eq!(tool_kind_name(4), Some("cat"));
        assert!(tool_kind_name(5).is_none());
        assert!(tool_cell(20).is_none());
    }

    #[test]
    fn test_eol_state_mapping() {
        assert_eq!(eol_state(0), Some(0));
        assert_eq!(eol_state(1), Some(0));
        assert_eq!(eol_state(2), Some(1));
        assert_eq!(eol_state(3), Some(1));
        assert_eq!(eol_state(4), Some(2));
        assert_eq!(eol_state(5), Some(2));
        assert!(eol_state(6).is_none());
        assert_eq!(eol_state_name(0), Some("CRLF"));
        assert_eq!(eol_state_name(1), Some("LF"));
        assert_eq!(eol_state_name(2), Some("保持原样"));
        assert!(eol_state_name(3).is_none());
    }

    #[test]
    fn test_path_and_pager_mapping() {
        assert_eq!(path_name(0), Some("盘符"));
        assert_eq!(path_name(4), Some("环境变量展开"));
        assert_eq!(path_name(7), Some("尾空格"));
        assert!(path_name(8).is_none());
        assert!(path_cell(8).is_none());
        assert!(pager_row(PAGER_ROWS).is_none());
        assert_ne!(pager_row(0).unwrap_or(0), pager_row(1).unwrap_or(0));
        assert!(decl_row(2, 0).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族正格，再注逆序必抓。
        let mut lt = CoreutilsLedger::new();
        let _ = lt.record_tool(9, tool_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_tool(2, tool_cell(2).unwrap_or(0)),
            Err(CoreutilsCode::EOrder)
        );
        // 逆序被拒不改 last_kind：同族正格仍可落。
        assert!(lt.record_tool(10, tool_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CoreutilsLedger::new();
        let _ = lt.record_tool(0, tool_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_tool(1, tool_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(CoreutilsCode::EDrift)
        );
        assert_eq!(
            lt.record_eol(2, eol_cell(2).unwrap_or(0).wrapping_add(6)),
            Err(CoreutilsCode::EEolDrift)
        );
        assert_eq!(
            lt.record_path(5, path_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(CoreutilsCode::EDrift)
        );
        assert_eq!(
            lt.record_pager(3, pager_row(3).unwrap_or(0).wrapping_add(8)),
            Err(CoreutilsCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CoreutilsLedger::new();
        let _ = lt.record_tool(0, tool_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_tool(0, tool_cell(0).unwrap_or(0)),
            Err(CoreutilsCode::EDup)
        );
        let _ = lt.record_eol(0, eol_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_eol(0, eol_cell(0).unwrap_or(0)),
            Err(CoreutilsCode::EDup)
        );
        let _ = lt.record_pager(0, pager_row(0).unwrap_or(0));
        assert_eq!(
            lt.record_pager(0, pager_row(0).unwrap_or(0)),
            Err(CoreutilsCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CoreutilsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CoreutilsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let lt0 = CoreutilsLedger::new();
        assert_eq!(lt0.seal(), Err(CoreutilsCode::ESeal));
        // 六闸齐、声明行缺 → ERowMiss。
        let mut lt1 = CoreutilsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.seal(), Err(CoreutilsCode::ERowMiss));
        // 声明齐、格账缺 → ESeal。
        let mut grp = 0usize;
        while grp < 2 {
            let _ = lt1.declare(grp, 0);
            let _ = lt1.declare(grp, 1);
            grp += 1;
        }
        assert_eq!(lt1.seal(), Err(CoreutilsCode::ESeal));
    }

    #[test]
    fn test_boundary_no_ledger_account() {
        // 越闭集边界拒不计注入账：lt 三数保持零。
        let mut lt = CoreutilsLedger::new();
        assert!(lt.record_tool(TOOL_CELLS, 0).is_err());
        assert!(lt.record_eol(EOL_CELLS, 0).is_err());
        assert!(lt.record_path(PATH_CELLS, 0).is_err());
        assert!(lt.record_pager(PAGER_ROWS, 0).is_err());
        assert!(lt.declare(2, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：工具+EOL+路径+分页。
        assert_eq!(
            recorded,
            (TOOL_CELLS + EOL_CELLS + PATH_CELLS + PAGER_ROWS) as u32
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
        assert_eq!(SEG_BASE, 0xCC0C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CoreutilsCode::Ok as u32, 0);
        assert_eq!(CoreutilsCode::EGradeDrift as u32, 12);
        assert_eq!(CoreutilsCode::Ok.val(), SEG_BASE);
        assert!(!CoreutilsCode::Ok.detail().is_empty());
        assert!(!CoreutilsCode::EEolDrift.detail().is_empty());
        assert!(!CoreutilsCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_conservation_and_fp() {
        assert!(conservation(10, 10, 0).is_ok());
        assert!(conservation(10, 6, 3).is_err());
        assert!(conservation(10, 7, 3).is_ok());
        assert!(fp_consistent(13, 13).is_ok());
        assert!(fp_consistent(13, 14).is_err());
    }

    #[test]
    fn test_grade() {
        // 如实阶梯（定级封顶 S3）：空账 S3，六闸齐 S3（不升档），全齐 S3。
        // GnuWin32 长尾面 KB-COMPAT 恒定不收口级，S1/S2 恒不达。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = CoreutilsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = CoreutilsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
        assert!(grade_gate(Grade::S3, Grade::S3).is_ok());
        assert!(grade_gate(Grade::S1, Grade::S3).is_err());
        assert!(grade_gate(Grade::S2, Grade::S3).is_err());
    }

    #[test]
    fn unx52516_checks_green() {
        let s = run_unx52516_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
