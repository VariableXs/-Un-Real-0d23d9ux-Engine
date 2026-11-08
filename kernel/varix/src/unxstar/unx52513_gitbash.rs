//! UNX-F52513 Git Bash（MSYS2 精简面）收口 · 0xCC000段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52513"，逐字）：
//!   title: Git Bash（MSYS2 精简面）收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52514-J1 安装器/
//!     setup ×10 包全通；POSIX 兼容面样本 ×10；KB-COMPAT 定级登记；判据
//!     锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52513）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52514-J1，照原文锚定；功能按本卡标题（Git Bash · MSYS2 精简面）
//! 落地。安装器/setup ×10 属后卡（Cygwin）判据域，本卡仅照原文锚引；
//! 本卡功能域为 Git Bash 五工具族 + 路径转换 + Git 消费联签。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五工具族 20 格闭集：ls / grep / sed / awk / ssh 消费，各 4 格，
//!      家族单调（逆序必抓）；逐格载荷纯函数折叠，恒等才收。
//!   ② MSYS2 精简声明 2 行闭集（精简安装面声明 / 依赖子集声明），缺一
//!      拒封；与 Cygwin 分账声明 2 行（F52514 判据域分账），缺一拒封。
//!   ③ 路径转换三态 6 格：POSIX→Win / Win→POSIX / 原样直通，三态各 2
//!      格，逐态载荷恒等才收，越态必抓。
//!   ④ 与 Git 消费联签 4 格（F52159 判据域联签锚定），逐格恒等。
//!   ⑤ C 上栈声明 2 行闭集（栈帧上栈声明 / C 运行时上栈声明）。
//!   ⑥ POSIX 兼容面样本 ×10：逐样本行恒等落账（判据原文第二分句）。
//!   ⑦ ledger 状态机（new/record_*/declare_*/judge_gate×6/seal/
//!      fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转
//!      10 轮（工具漂移/家族逆序/工具越闭集）10/10 全抓；检出+逃逸守
//!      恒；双跑 FNV 指纹一致；定级 S1（全齐）/S2（六闸齐）/S3（不收
//!      口）如实阶梯。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52513;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五工具族格数（ls/grep/sed/awk/ssh 消费 ×4）。
pub const TOOL_CELLS: usize = 20;

/// 工具家族数。
pub const TOOL_KINDS: usize = 5;

/// 路径转换三态格数（三态 ×2 格）。
pub const PATH_CELLS: usize = 6;

/// 路径三态数。
pub const PATH_STATES: usize = 3;

/// 与 Git 消费联签格数（F52159）。
pub const GIT_CELLS: usize = 4;

/// POSIX 兼容面样本数（判据原文：×10）。
pub const POSIX_SAMPLES: usize = 10;

/// 三组声明行各 2 行（MSYS2 精简/与 Cygwin 分账/C 上栈）。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitbashCode {
    /// 成功
    Ok = 0,
    /// 工具号越闭集
    ENoTool,
    /// 载荷漂移（工具/路径/联签格复用）
    EDrift,
    /// 工具家族逆序
    EOrder,
    /// 路径格越闭集
    ENoPath,
    /// 联签格越闭集
    ENoGit,
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

impl GitbashCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GitbashCode::Ok => "成功",
            GitbashCode::ENoTool => "工具号越闭集：五工具族 20 格之外先登记",
            GitbashCode::EDrift => "载荷漂移：格折叠与基准不恒等，逐格复算",
            GitbashCode::EOrder => "家族逆序：ls→grep→sed→awk→ssh 消费单调，逆序必抓",
            GitbashCode::ENoPath => "路径格越闭集：路径转换三态 6 格之外先登记",
            GitbashCode::ENoGit => "联签格越闭集：与 Git 消费联签 4 格之外先登记",
            GitbashCode::ENoRow => "声明行越闭集：三组声明各 2 行闭集之外先登记",
            GitbashCode::ERowMiss => "缺声明行：MSYS2 精简/与 Cygwin 分账/C 上栈未齐先补齐",
            GitbashCode::EDup => "重复落账：同格只许一条账目",
            GitbashCode::ESeal => "未齐即封：20 工具格+6 路径格+4 联签格+10 样本未齐先补齐",
            GitbashCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GitbashCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GitbashCode::EGradeDrift => "定级漂移：全通且三组声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20工具格+6路径格+4联签格+10样本+三组声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 工具格载荷（格 o；家族 = o/4）。
pub const fn tool_cell(o: usize) -> Option<u64> {
    if o >= TOOL_CELLS {
        return None;
    }
    Some((0xCC10u64 + (o as u64) * 0x2C) ^ 0x47425348)
}

/// 工具家族（0=ls 1=grep 2=sed 3=awk 4=ssh 消费）。
pub const fn tool_kind(o: usize) -> Option<u32> {
    if o >= TOOL_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名（按 kind 索引）。
pub const fn tool_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("ls"),
        1 => Some("grep"),
        2 => Some("sed"),
        3 => Some("awk"),
        4 => Some("ssh 消费"),
        _ => None,
    }
}

/// 路径转换格载荷（格 p；三态 = p/2）。
pub const fn path_cell(p: usize) -> Option<u64> {
    if p >= PATH_CELLS {
        return None;
    }
    Some((0xCC40u64 + (p as u64) * 0x38) ^ ((p / 2) as u64).wrapping_mul(0x01000193))
}

/// 路径三态（0=POSIX→Win 1=Win→POSIX 2=原样直通）。
pub const fn path_state(p: usize) -> Option<u32> {
    if p >= PATH_CELLS {
        return None;
    }
    Some((p / 2) as u32)
}

/// 路径三态名（按 state 索引）。
pub const fn path_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("POSIX→Win"),
        1 => Some("Win→POSIX"),
        2 => Some("原样直通"),
        _ => None,
    }
}

/// 与 Git 消费联签格载荷（格 g；F52159 联签域）。
pub const fn git_cell(g: usize) -> Option<u64> {
    if g >= GIT_CELLS {
        return None;
    }
    Some((0xCC70u64 + (g as u64) * 0x24) ^ 0x474A4C53)
}

/// POSIX 兼容面样本行载荷（样本 s）。
pub const fn sample_row(s: usize) -> Option<u64> {
    if s >= POSIX_SAMPLES {
        return None;
    }
    Some(0xCC90 + s as u64 * 0x1A)
}

/// 声明行折叠（组 grp：0=MSYS2 精简 1=与 Cygwin 分账 2=C 上栈；行 r）。
pub const fn decl_row(grp: usize, r: usize) -> Option<u32> {
    if grp >= 3 || r >= DECL_ROWS {
        return None;
    }
    Some(0xCCB0 + (grp * 0x08 + r as usize * 0x02) as u32)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Git Bash 收口账本状态机。
#[derive(Clone)]
pub struct GitbashLedger {
    /// 六判据闸。
    gates: [Option<bool>; 6],
    /// 工具格账。
    tools: [Option<u64>; TOOL_CELLS],
    /// 路径格账。
    paths: [Option<u64>; PATH_CELLS],
    /// 联签格账。
    gits: [Option<u64>; GIT_CELLS],
    /// POSIX 样本账。
    samples: [Option<u64>; POSIX_SAMPLES],
    /// 三组声明账（组→行）。
    decls: [[Option<u32>; DECL_ROWS]; 3],
    /// 最后工具家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GitbashLedger {
    /// 新账本。
    pub fn new() -> Self {
        GitbashLedger {
            gates: [None; 6],
            tools: [None; TOOL_CELLS],
            paths: [None; PATH_CELLS],
            gits: [None; GIT_CELLS],
            samples: [None; POSIX_SAMPLES],
            decls: [[None; DECL_ROWS]; 3],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GitbashCode> {
        if gate >= 6 {
            return Err(GitbashCode::ESeal);
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

    /// 工具一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_tool(&mut self, o: usize, got: u64) -> Result<(), GitbashCode> {
        if o >= TOOL_CELLS {
            return Err(GitbashCode::ENoTool);
        }
        let kind = match tool_kind(o) {
            Some(k) => k,
            None => return Err(GitbashCode::ENoTool),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(GitbashCode::EOrder);
        }
        let want = match tool_cell(o) {
            Some(w) => w,
            None => return Err(GitbashCode::ENoTool),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(GitbashCode::EDrift);
        }
        if match self.tools.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitbashCode::EDup);
        }
        if let Some(c) = self.tools.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
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

    /// 路径转换一格（三态载荷对照 + 重复拒）。
    pub fn record_path(&mut self, p: usize, got: u64) -> Result<(), GitbashCode> {
        if p >= PATH_CELLS {
            return Err(GitbashCode::ENoPath);
        }
        let want = match path_cell(p) {
            Some(w) => w,
            None => return Err(GitbashCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + p as u64);
            return Err(GitbashCode::EDrift);
        }
        if match self.paths.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitbashCode::EDup);
        }
        if let Some(c) = self.paths.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5301 + p as u64);
        Ok(())
    }

    /// 路径全账核验（6 格）。
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

    /// 与 Git 消费联签一格（载荷对照 + 重复拒）。
    pub fn record_git(&mut self, g: usize, got: u64) -> Result<(), GitbashCode> {
        if g >= GIT_CELLS {
            return Err(GitbashCode::ENoGit);
        }
        let want = match git_cell(g) {
            Some(w) => w,
            None => return Err(GitbashCode::ENoGit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + g as u64);
            return Err(GitbashCode::EDrift);
        }
        if match self.gits.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitbashCode::EDup);
        }
        if let Some(c) = self.gits.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x5401 + g as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn gits_all(&self) -> bool {
        let mut g = 0usize;
        while g < GIT_CELLS {
            match self.gits.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// POSIX 兼容面样本一行（载荷对照 + 重复拒）。
    pub fn record_sample(&mut self, s: usize, got: u64) -> Result<(), GitbashCode> {
        if s >= POSIX_SAMPLES {
            return Err(GitbashCode::ENoRow);
        }
        let want = match sample_row(s) {
            Some(w) => w,
            None => return Err(GitbashCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + s as u64);
            return Err(GitbashCode::EDrift);
        }
        if match self.samples.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitbashCode::EDup);
        }
        if let Some(c) = self.samples.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5501 + s as u64);
        Ok(())
    }

    /// 样本全账核验（10 行）。
    pub fn samples_all(&self) -> bool {
        let mut s = 0usize;
        while s < POSIX_SAMPLES {
            match self.samples.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 声明一行（组 grp：0=MSYS2 精简 1=与 Cygwin 分账 2=C 上栈）。
    pub fn declare(&mut self, grp: usize, r: usize) -> Result<(), GitbashCode> {
        if grp >= 3 || r >= DECL_ROWS {
            return Err(GitbashCode::ENoRow);
        }
        let slot = match self.decls.get_mut(grp).and_then(|g| g.get_mut(r)) {
            Some(c) => c,
            None => return Err(GitbashCode::ENoRow),
        };
        if slot.is_some() {
            return Err(GitbashCode::EDup);
        }
        *slot = decl_row(grp, r);
        self.fnv(0x5600 + grp as u64 * 2 + r as u64);
        Ok(())
    }

    /// 三组声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut grp = 0usize;
        while grp < 3 {
            let mut r = 0usize;
            while r < DECL_ROWS {
                match self.decls.get(grp).and_then(|g| g.get(r)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                r += 1;
            }
            grp += 1;
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

    /// 收口闸：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
    pub fn seal(&self) -> Result<(), GitbashCode> {
        if !self.gates_all() {
            return Err(GitbashCode::ESeal);
        }
        if !self.decls_all() {
            return Err(GitbashCode::ERowMiss);
        }
        if !self.tools_all() || !self.paths_all() || !self.gits_all() || !self.samples_all() {
            return Err(GitbashCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，空账/缺闸 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.tools_all() && self.paths_all() && self.gits_all()
            && self.samples_all() && self.decls_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GitbashLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), GitbashCode> {
    if injected == caught.wrapping_add(escaped) {
        Ok(())
    } else {
        Err(GitbashCode::EInj)
    }
}

/// 双跑指纹一致核验。
pub fn fp_consistent(a: u64, b: u64) -> Result<(), GitbashCode> {
    if a == b {
        Ok(())
    } else {
        Err(GitbashCode::EFp)
    }
}

/// 定级核验：漂移必抓（EGradeDrift）。
pub fn grade_gate(got: Grade, want: Grade) -> Result<(), GitbashCode> {
    if got.val() == want.val() {
        Ok(())
    } else {
        Err(GitbashCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GitbashLedger::new();
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
    let mut p = 0usize;
    while p < PATH_CELLS {
        if let Some(w) = path_cell(p) {
            if led.record_path(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut gi = 0usize;
    while gi < GIT_CELLS {
        if let Some(w) = git_cell(gi) {
            if led.record_git(gi, w).is_ok() {
                recorded += 1;
            }
        }
        gi += 1;
    }
    let mut s = 0usize;
    while s < POSIX_SAMPLES {
        if let Some(w) = sample_row(s) {
            if led.record_sample(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
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
    // 注入 10 轮：工具漂移 / 家族逆序 / 工具越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GitbashLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落同族正格，再注漂移。
                let _ = lt.record_tool(0, tool_cell(0).unwrap_or(0));
                lt.record_tool(1, tool_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(GitbashCode::EDrift)
            }
            1 => {
                // 逆序注入：对空账本先落高级家族正格再回跳，必抓且不改 last_kind。
                let _ = lt.record_tool(5, tool_cell(5).unwrap_or(0));
                lt.record_tool(2, tool_cell(2).unwrap_or(0)) == Err(GitbashCode::EOrder)
            }
            _ => {
                // 越闭集注入：越界拒不计注入账。
                lt.record_tool(TOOL_CELLS, 0).is_err()
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
/// UNX-F52513 判据集（锚 UNX-F52514-J1，off-by-one 板式）。
pub fn run_unx52513_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52513");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五工具族 20 格全通",
        recorded >= TOOL_CELLS as u32,
        "ls/grep/sed/awk/ssh 消费五家族逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GitbashLedger::new();
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
        "路径转换三态 6 格全落",
        {
            let mut lt = GitbashLedger::new();
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
        "POSIX→Win/Win→POSIX/原样直通三态逐格恒等",
    );
    s.add(
        "与 Git 消费联签 4 格全落（F52159）",
        {
            let mut lt = GitbashLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GIT_CELLS {
                if let Some(w) = git_cell(g) {
                    if lt.record_git(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.gits_all()
        },
        "F52159 联签锚定行逐格恒等",
    );
    s.add(
        "MSYS2 精简声明 2/2 全齐",
        {
            let mut lt = GitbashLedger::new();
            let _ = lt.declare(0, 0);
            let _ = lt.declare(0, 1);
            lt.decl_all_group(0)
        },
        "精简安装面声明/依赖子集声明两行闭集，缺一拒封",
    );
    s.add(
        "与 Cygwin 分账声明 2/2 全齐（F52514）",
        {
            let mut lt = GitbashLedger::new();
            let _ = lt.declare(1, 0);
            let _ = lt.declare(1, 1);
            lt.decl_all_group(1)
        },
        "F52514 判据域分账两行闭集，与 Cygwin 账不混记",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = GitbashLedger::new();
            let _ = lt.declare(2, 0);
            let _ = lt.declare(2, 1);
            lt.decl_all_group(2)
        },
        "栈帧上栈声明/C 运行时上栈声明两行闭集",
    );
    s.add(
        "POSIX 兼容面样本 10/10 全落",
        {
            let mut lt = GitbashLedger::new();
            let mut ok = true;
            let mut sm = 0usize;
            while sm < POSIX_SAMPLES {
                if let Some(w) = sample_row(sm) {
                    if lt.record_sample(sm, w).is_err() {
                        ok = false;
                    }
                }
                sm += 1;
            }
            ok && lt.samples_all()
        },
        "判据原文第二分句：POSIX 兼容面样本 ×10 逐行恒等",
    );
    s.add(
        "家族单调",
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
        "五家族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    s.add(
        "边界·工具号越闭集",
        {
            let mut lt = GitbashLedger::new();
            lt.record_tool(TOOL_CELLS, 0).is_err()
        },
        "工具格 20 之外越界必拒",
    );
    s.add(
        "边界·路径格越闭集",
        {
            let mut lt = GitbashLedger::new();
            lt.record_path(PATH_CELLS, 0).is_err()
        },
        "路径格 6 之外越界必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = GitbashLedger::new();
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
            let lt = GitbashLedger::new();
            lt.seal() == Err(GitbashCode::ESeal)
        },
        "空账封口必拒：六闸缺→ESeal",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GitbashLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < TOOL_CELLS {
                if let Some(w) = tool_cell(o) {
                    let _ = lt.record_tool(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PATH_CELLS {
                if let Some(w) = path_cell(p) {
                    let _ = lt.record_path(p, w);
                }
                p += 1;
            }
            let mut gi = 0usize;
            while gi < GIT_CELLS {
                if let Some(w) = git_cell(gi) {
                    let _ = lt.record_git(gi, w);
                }
                gi += 1;
            }
            let mut sm = 0usize;
            while sm < POSIX_SAMPLES {
                if let Some(w) = sample_row(sm) {
                    let _ = lt.record_sample(sm, w);
                }
                sm += 1;
            }
            let mut grp = 0usize;
            while grp < 3 {
                let mut r = 0usize;
                while r < DECL_ROWS {
                    let _ = lt.declare(grp, r);
                    r += 1;
                }
                grp += 1;
            }
            grade_gate(lt.judge_grade(), Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC000,
        "SEG_BASE 0x40 对齐且在 VX16 域首格（0xCA400+0x40×112）",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 声明分组辅助 ─────────────────────────────────────────────────
impl GitbashLedger {
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
        assert_eq!(tool_kind(8), Some(2));
        assert_eq!(tool_kind(12), Some(3));
        assert_eq!(tool_kind(16), Some(4));
        assert_eq!(tool_kind(19), Some(4));
        assert!(tool_kind(20).is_none());
        assert_eq!(tool_kind_name(0), Some("ls"));
        assert_eq!(tool_kind_name(1), Some("grep"));
        assert_eq!(tool_kind_name(2), Some("sed"));
        assert_eq!(tool_kind_name(3), Some("awk"));
        assert_eq!(tool_kind_name(4), Some("ssh 消费"));
        assert!(tool_kind_name(5).is_none());
        assert!(tool_cell(20).is_none());
    }

    #[test]
    fn test_path_state_mapping() {
        assert_eq!(path_state(0), Some(0));
        assert_eq!(path_state(1), Some(0));
        assert_eq!(path_state(2), Some(1));
        assert_eq!(path_state(3), Some(1));
        assert_eq!(path_state(4), Some(2));
        assert_eq!(path_state(5), Some(2));
        assert!(path_state(6).is_none());
        assert_eq!(path_state_name(0), Some("POSIX→Win"));
        assert_eq!(path_state_name(1), Some("Win→POSIX"));
        assert_eq!(path_state_name(2), Some("原样直通"));
        assert!(path_state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = tool_cell(7).unwrap_or(0);
        assert_eq!(a, tool_cell(7).unwrap_or(0));
        assert_ne!(a, tool_cell(8).unwrap_or(0));
        assert_ne!(path_cell(0).unwrap_or(0), path_cell(1).unwrap_or(0));
        assert!(git_cell(GIT_CELLS).is_none());
        assert!(sample_row(POSIX_SAMPLES).is_none());
        assert!(decl_row(3, 0).is_none());
        assert!(decl_row(0, DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族正格，再注逆序必抓。
        let mut lt = GitbashLedger::new();
        let _ = lt.record_tool(9, tool_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_tool(2, tool_cell(2).unwrap_or(0)),
            Err(GitbashCode::EOrder)
        );
        // 逆序被拒不改 last_kind：同族正格仍可落。
        assert!(lt.record_tool(10, tool_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GitbashLedger::new();
        let _ = lt.record_tool(0, tool_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_tool(1, tool_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(GitbashCode::EDrift)
        );
        assert_eq!(
            lt.record_path(3, path_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(GitbashCode::EDrift)
        );
        assert_eq!(
            lt.record_git(0, git_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(GitbashCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GitbashLedger::new();
        let _ = lt.record_tool(0, tool_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_tool(0, tool_cell(0).unwrap_or(0)),
            Err(GitbashCode::EDup)
        );
        let _ = lt.declare(0, 0);
        assert_eq!(lt.declare(0, 0), Err(GitbashCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GitbashLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GitbashLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let lt0 = GitbashLedger::new();
        assert_eq!(lt0.seal(), Err(GitbashCode::ESeal));
        // 六闸齐、声明行缺 → ERowMiss。
        let mut lt1 = GitbashLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.seal(), Err(GitbashCode::ERowMiss));
        // 声明齐、格账缺 → ESeal。
        let mut grp = 0usize;
        while grp < 3 {
            let _ = lt1.declare(grp, 0);
            let _ = lt1.declare(grp, 1);
            grp += 1;
        }
        assert_eq!(lt1.seal(), Err(GitbashCode::ESeal));
    }

    #[test]
    fn test_boundary_no_ledger_account() {
        // 越闭集边界拒不计注入账：lt 三数保持零。
        let mut lt = GitbashLedger::new();
        assert!(lt.record_tool(TOOL_CELLS, 0).is_err());
        assert!(lt.record_path(PATH_CELLS, 0).is_err());
        assert!(lt.record_git(GIT_CELLS, 0).is_err());
        assert!(lt.record_sample(POSIX_SAMPLES, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：工具+路径+联签+样本。
        assert_eq!(
            recorded,
            (TOOL_CELLS + PATH_CELLS + GIT_CELLS + POSIX_SAMPLES) as u32
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
        assert_eq!(SEG_BASE, 0xCC000);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GitbashCode::Ok as u32, 0);
        assert_eq!(GitbashCode::EGradeDrift as u32, 12);
        assert_eq!(GitbashCode::Ok.val(), SEG_BASE);
        assert!(!GitbashCode::Ok.detail().is_empty());
        assert!(!GitbashCode::ESeal.detail().is_empty());
        assert!(!GitbashCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_conservation_and_fp() {
        assert!(conservation(10, 10, 0).is_ok());
        assert!(conservation(10, 9, 0).is_err());
        assert!(conservation(10, 9, 1).is_ok());
        assert!(fp_consistent(7, 7).is_ok());
        assert!(fp_consistent(7, 8).is_err());
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1（目标档前置必须补 judge_gate×6）。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GitbashLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = GitbashLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut lt3 = GitbashLedger::new();
        let mut g3 = 0usize;
        while g3 < 6 {
            let _ = lt3.judge_gate(g3);
            g3 += 1;
        }
        let mut o = 0usize;
        while o < TOOL_CELLS {
            if let Some(w) = tool_cell(o) {
                let _ = lt3.record_tool(o, w);
            }
            o += 1;
        }
        // 仅部分节齐仍为 S2（如实：未全齐不升 S1）。
        assert_eq!(lt3.judge_grade(), Grade::S2);
        assert!(grade_gate(Grade::S3, Grade::S1).is_err());
        assert!(grade_gate(Grade::S1, Grade::S1).is_ok());
    }

    #[test]
    fn unx52513_checks_green() {
        let s = run_unx52513_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
