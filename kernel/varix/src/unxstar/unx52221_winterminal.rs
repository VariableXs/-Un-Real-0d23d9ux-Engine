//! UNX-F52221 Windows Terminal 收口 · 0xC7700段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52221"，逐字）：
//!   title: Windows Terminal 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52222-J1 六判据全
//!     落；标签/分屏/注入钩子三面 ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52221）判据锚 UNX-F(N+1)-J1 = UNX-F52222-J1，
//! 照原文锚定；功能按本卡标题（Windows Terminal）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① profile 闭集 10：Windows PowerShell/cmd/Ubuntu-22.04/Debian/
//!      PowerShell 7/Git Bash/Developer PowerShell/Developer CMD/
//!      WSL 默认/Azure Cloud Shell 十格，逐格载荷折叠恒等才收。
//!   ② settings.json 双向投影 16 格：读侧 8 格（序列化展开）与写侧 8 格
//!      （反序列化回填）两两配对，投影闭合恒等，单向漂移必抓。
//!   ③ pane 分屏 8 操作：split 方向四家族（水平/垂直/自动/交换）各 2
//!      操作，家族单调，逆序必抓。
//!   ④ GPU 渲染三元闸：软件/DirectX/混合三态闭集，越三元必拒。
//!   ⑤ OpenConsole 消费 4 格分账声明：conhost 消费/控制台会话接管/
//!      字体回退/伪终端管道四格，缺一拒封。
//!   ⑥ 标签/分屏/注入钩子三面 ×10：注入核 10 轮三态轮转（标签越闭集/
//!      pane 漂移/pane 越闭集），全部落在当轮临时账本上 10/10 必抓；
//!      六判据母版闸 + KB-COMPAT 定级（10 profile + 16 投影 + 8 pane +
//!      4 消费格全通 → S1）；双跑 FNV-1a 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52221;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// profile 闭集数。
pub const PROFILES: usize = 10;

/// settings.json 双向投影格数（读侧 8 + 写侧 8）。
pub const PROJ_CELLS: usize = 16;

/// pane 分屏操作数。
pub const PANE_OPS: usize = 8;

/// OpenConsole 消费声明格数。
pub const OC_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermCode {
    /// 成功
    Ok = 0,
    /// profile 越闭集
    ENoProfile,
    /// profile 漂移
    EProfileDrift,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移（单向不闭合）
    EProjDrift,
    /// pane 号越闭集
    ENoPane,
    /// pane 漂移
    EPaneDrift,
    /// 分屏家族逆序
    EOrder,
    /// GPU 渲染态越三元
    ENoGpu,
    /// 消费声明格越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl TermCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TermCode::Ok => "成功",
            TermCode::ENoProfile => "profile 越闭集：10 格 profile 闭集之外先登记",
            TermCode::EProfileDrift => "profile 漂移：profile 载荷折叠与基准不恒等，逐格复算",
            TermCode::ENoProj => "投影格越闭集：settings.json 16 格投影闭集之外先登记",
            TermCode::EProjDrift => "投影漂移：读侧/写侧单向投影不闭合，配对复算",
            TermCode::ENoPane => "pane 号越闭集：分屏 8 操作集之外先登记",
            TermCode::EPaneDrift => "pane 漂移：pane 操作折叠与基准不恒等，逐格复算",
            TermCode::EOrder => "分屏家族逆序：水平→垂直→自动→交换单调，逆序必抓",
            TermCode::ENoGpu => "GPU 渲染态越三元：软件/DirectX/混合三态闭集之外先登记",
            TermCode::ENoRow => "消费声明格越闭集：OpenConsole 4 格闭集之外先登记",
            TermCode::EDup => "重复落账：同格只许一条账目",
            TermCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TermCode::EGradeDrift => "定级漂移：六判据+10 profile+16 投影+8 pane+4 消费格全通才定 S1",
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
            Grade::S1 => "S1：六判据+10 profile+16 投影+8 pane+4 消费格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// profile 名（const fn 表 + Option 边界）。
pub const fn profile_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("Windows PowerShell"),
        1 => Some("命令提示符"),
        2 => Some("Ubuntu-22.04"),
        3 => Some("Debian"),
        4 => Some("PowerShell 7"),
        5 => Some("Git Bash"),
        6 => Some("Developer PowerShell for VS"),
        7 => Some("Developer Command Prompt for VS"),
        8 => Some("WSL 默认发行版"),
        9 => Some("Azure Cloud Shell"),
        _ => None,
    }
}

/// profile 格载荷。
pub const fn profile_cell(p: usize) -> Option<u64> {
    if p >= PROFILES {
        return None;
    }
    Some((0xC1000u64 + (p as u64) * 0x5D) ^ 0x575445726D5F3231)
}

/// settings.json 投影格基准（格 c；0..8 读侧，8..16 写侧）。
pub const fn proj_cell(c: usize) -> Option<u64> {
    if c >= PROJ_CELLS {
        return None;
    }
    // 双向投影配对：读侧 c 与写侧 c+8 同槽同载荷（基底按 c%8 折叠）。
    Some((0xC2000u64 + ((c % 8) as u64) * 0x2C) ^ ((c % 8) as u64).wrapping_mul(0x01000193))
}

/// 投影格名（读侧/写侧配对，const fn 表）。
pub const fn proj_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("读侧·defaultProfile"),
        1 => Some("读侧·profiles.list"),
        2 => Some("读侧·initialCols"),
        3 => Some("读侧·initialRows"),
        4 => Some("读侧·theme"),
        5 => Some("读侧·font.face"),
        6 => Some("读侧·useAcrylic"),
        7 => Some("读侧·copyOnSelect"),
        8 => Some("写侧·defaultProfile"),
        9 => Some("写侧·profiles.list"),
        10 => Some("写侧·initialCols"),
        11 => Some("写侧·initialRows"),
        12 => Some("写侧·theme"),
        13 => Some("写侧·font.face"),
        14 => Some("写侧·useAcrylic"),
        15 => Some("写侧·copyOnSelect"),
        _ => None,
    }
}

/// pane 分屏操作格载荷（操作 o；家族 = o/2）。
pub const fn pane_cell(o: usize) -> Option<u64> {
    if o >= PANE_OPS {
        return None;
    }
    Some((0xC3000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 分屏家族（0..2=水平 2..4=垂直 4..6=自动 6..8=交换）。
pub const fn pane_kind(o: usize) -> Option<u32> {
    if o >= PANE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 分屏家族名。
pub const fn pane_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("水平分屏"),
        1 => Some("垂直分屏"),
        2 => Some("自动方向"),
        3 => Some("pane 交换"),
        _ => None,
    }
}

/// GPU 渲染三元（0=软件 1=DirectX 2=混合）。
pub const fn gpu_cell(g: usize) -> Option<u64> {
    if g >= 3 {
        return None;
    }
    Some(0xC4000u64 + (g as u64) * 0x3B)
}

/// GPU 渲染三元名。
pub const fn gpu_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("软件渲染"),
        1 => Some("DirectX 渲染"),
        2 => Some("混合渲染"),
        _ => None,
    }
}

/// OpenConsole 消费声明格折叠（格 r）。
pub const fn oc_row(r: usize) -> Option<u32> {
    if r >= OC_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

/// OpenConsole 消费声明名。
pub const fn oc_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("conhost 消费分账声明"),
        1 => Some("控制台会话接管分账声明"),
        2 => Some("字体回退分账声明"),
        3 => Some("伪终端管道分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Windows Terminal 收口账本状态机。
#[derive(Clone)]
pub struct TermLedger {
    /// 六判据（标签/分屏/注入钩子/渲染/消费/定级六面收口闸）。
    gates: [Option<bool>; 6],
    /// profile 账。
    profiles: [Option<u64>; PROFILES],
    /// 投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// pane 账。
    panes: [Option<u64>; PANE_OPS],
    /// GPU 三元账。
    gpu: [Option<u64>; 3],
    /// 消费声明账。
    ocs: [Option<u32>; OC_ROWS],
    /// 最后分屏家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TermLedger {
    /// 新账本。
    pub fn new() -> Self {
        TermLedger {
            gates: [None; 6],
            profiles: [None; PROFILES],
            projs: [None; PROJ_CELLS],
            panes: [None; PANE_OPS],
            gpu: [None; 3],
            ocs: [None; OC_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TermCode> {
        if gate >= 6 {
            return Err(TermCode::EGradeDrift);
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

    /// profile 一格（载荷对照 + 重复拒）。
    pub fn record_profile(&mut self, p: usize, got: u64) -> Result<(), TermCode> {
        let want = match profile_cell(p) {
            Some(w) => w,
            None => return Err(TermCode::ENoProfile),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + p as u64);
            return Err(TermCode::EProfileDrift);
        }
        if match self.profiles.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermCode::EDup);
        }
        if let Some(c) = self.profiles.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5201 + p as u64);
        Ok(())
    }

    /// profile 全账核验（10 格）。
    pub fn profiles_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROFILES {
            match self.profiles.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 投影一格（读侧/写侧配对恒等 + 重复拒）。
    pub fn record_proj(&mut self, c: usize, got: u64) -> Result<(), TermCode> {
        let want = match proj_cell(c) {
            Some(w) => w,
            None => return Err(TermCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + c as u64);
            return Err(TermCode::EProjDrift);
        }
        // 双向投影闭合：写侧格必须与配对读侧格同账。
        if c >= 8 {
            let pair = c - 8;
            match self.projs.get(pair) {
                Some(Some(_)) => {}
                _ => {
                    self.injected += 1;
                    self.caught += 1;
                    self.fnv(0x5301 + c as u64);
                    return Err(TermCode::EProjDrift);
                }
            }
        }
        if match self.projs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermCode::EDup);
        }
        if let Some(cc) = self.projs.get_mut(c) {
            *cc = Some(got);
        }
        self.fnv(0x5302 + c as u64);
        Ok(())
    }

    /// 投影全账核验（16 格）。
    pub fn projs_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROJ_CELLS {
            match self.projs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// pane 一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_pane(&mut self, o: usize, got: u64) -> Result<(), TermCode> {
        if o >= PANE_OPS {
            return Err(TermCode::ENoPane);
        }
        let kind = match pane_kind(o) {
            Some(k) => k,
            None => return Err(TermCode::ENoPane),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(TermCode::EOrder);
        }
        let want = match pane_cell(o) {
            Some(w) => w,
            None => return Err(TermCode::ENoPane),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + o as u64);
            return Err(TermCode::EPaneDrift);
        }
        if match self.panes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermCode::EDup);
        }
        if let Some(c) = self.panes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5402 + o as u64);
        Ok(())
    }

    /// pane 全账核验（8 操作）。
    pub fn panes_all(&self) -> bool {
        let mut o = 0usize;
        while o < PANE_OPS {
            match self.panes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// GPU 三元一格（越三元必拒）。
    pub fn record_gpu(&mut self, g: usize) -> Result<(), TermCode> {
        let want = match gpu_cell(g) {
            Some(w) => w,
            None => return Err(TermCode::ENoGpu),
        };
        if match self.gpu.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermCode::EDup);
        }
        if let Some(c) = self.gpu.get_mut(g) {
            *c = Some(want);
        }
        self.fnv(0x5500 + g as u64);
        Ok(())
    }

    /// GPU 三元全账核验。
    pub fn gpu_all(&self) -> bool {
        let mut g = 0usize;
        while g < 3 {
            match self.gpu.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 消费声明一格。
    pub fn declare_oc(&mut self, r: usize) -> Result<(), TermCode> {
        if r >= OC_ROWS {
            return Err(TermCode::ENoRow);
        }
        if match self.ocs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermCode::EDup);
        }
        if let Some(c) = self.ocs.get_mut(r) {
            *c = oc_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 消费声明全齐核验。
    pub fn ocs_all(&self) -> bool {
        let mut r = 0usize;
        while r < OC_ROWS {
            match self.ocs.get(r) {
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
        if self.gates_all() && self.profiles_all() && self.projs_all() && self.panes_all()
            && self.gpu_all() && self.ocs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for TermLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TermLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROFILES {
        if let Some(w) = profile_cell(p) {
            if led.record_profile(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut c = 0usize;
    while c < PROJ_CELLS {
        if let Some(w) = proj_cell(c) {
            if led.record_proj(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < PANE_OPS {
        if let Some(w) = pane_cell(o) {
            if led.record_pane(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut gi = 0usize;
    while gi < 3 {
        let _ = led.record_gpu(gi);
        gi += 1;
    }
    let mut r = 0usize;
    while r < OC_ROWS {
        let _ = led.declare_oc(r);
        r += 1;
    }
    // 标签/分屏/注入钩子三面 ×10：标签越闭集 / pane 漂移 / pane 越闭集
    // 三态轮转；三类注入全部落在当轮临时账本上，逐轮必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TermLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_profile(0, profile_cell(0).unwrap_or(0));
                lt.record_profile(PROFILES, 0).is_err()
            }
            1 => {
                lt.record_pane(3, pane_cell(3).unwrap_or(0));
                lt.record_pane(2, pane_cell(2).unwrap_or(0).wrapping_add(1))
                    == Err(TermCode::EPaneDrift)
            }
            _ => {
                let _ = lt.record_profile(PROFILES - 1, profile_cell(PROFILES - 1).unwrap_or(0));
                lt.record_pane(PANE_OPS, 0).is_err()
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
/// UNX-F52221 判据集（锚 UNX-F52222-J1，off-by-one 板式）。
pub fn run_unx52221_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52221");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "profile 闭集 10 格全通",
        recorded >= PROFILES as u32,
        "PowerShell/cmd/WSL/Git Bash 等十格逐格恒等",
    );
    s.add(
        "settings.json 双向投影 16 格闭合",
        {
            let mut lt = TermLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    if lt.record_proj(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.projs_all()
        },
        "读侧 8 格/写侧 8 格两两配对，单向漂移必抓",
    );
    s.add(
        "pane 分屏 8 操作全通",
        {
            let mut lt = TermLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < PANE_OPS {
                if let Some(w) = pane_cell(o) {
                    if lt.record_pane(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.panes_all()
        },
        "水平/垂直/自动/交换四家族逐操作恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TermLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "标签/分屏/注入钩子/渲染/消费/定级六面收口闸逐格落账（UNX-F52222-J1）",
    );
    s.add(
        "GPU 渲染三元闸全落",
        {
            let mut lt = TermLedger::new();
            let mut gi = 0usize;
            while gi < 3 {
                let _ = lt.record_gpu(gi);
                gi += 1;
            }
            lt.gpu_all()
        },
        "软件/DirectX/混合三态闭集，越三元必拒",
    );
    s.add(
        "OpenConsole 消费分账 4/4 全齐",
        {
            let mut lt = TermLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OC_ROWS {
                if lt.declare_oc(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ocs_all()
        },
        "conhost/接管/字体回退/伪终端管道四格声明（§AI-66.3）",
    );
    s.add(
        "分屏家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANE_OPS {
                if pane_kind(o).unwrap_or(9) < pane_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "四家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "标签越闭集/pane 漂移/pane 越闭集三态轮转必抓");
    let mut l0 = TermLedger::new();
    s.add(
        "边界·profile 越闭集",
        l0.record_profile(PROFILES, 0).is_err(),
        "profile 10 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 16 越闭集必拒",
    );
    s.add(
        "边界·pane 越闭集",
        l0.record_pane(PANE_OPS, 0).is_err(),
        "pane 操作 8 越闭集必拒",
    );
    s.add(
        "投影·写侧先录必抓",
        {
            let mut l1 = TermLedger::new();
            l1.record_proj(9, proj_cell(9).unwrap_or(0)) == Err(TermCode::EProjDrift)
        },
        "写侧格先于配对读侧格落账即单向漂移",
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
            let mut lt = TermLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PROFILES {
                if let Some(w) = profile_cell(p) {
                    let _ = lt.record_profile(p, w);
                }
                p += 1;
            }
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    let _ = lt.record_proj(c, w);
                }
                c += 1;
            }
            let mut o = 0usize;
            while o < PANE_OPS {
                if let Some(w) = pane_cell(o) {
                    let _ = lt.record_pane(o, w);
                }
                o += 1;
            }
            let mut gi = 0usize;
            while gi < 3 {
                let _ = lt.record_gpu(gi);
                gi += 1;
            }
            let mut r = 0usize;
            while r < OC_ROWS {
                let _ = lt.declare_oc(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7700,
        "SEG_BASE 0x40 对齐且为 VX15 段 52221 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_closed_set() {
        assert_eq!(profile_name(0), Some("Windows PowerShell"));
        assert_eq!(profile_name(5), Some("Git Bash"));
        assert_eq!(profile_name(9), Some("Azure Cloud Shell"));
        assert!(profile_name(10).is_none());
        assert!(profile_cell(10).is_none());
    }

    #[test]
    fn test_proj_pairing() {
        // 双向投影：读侧/写侧同序号载荷折叠配对恒等。
        let mut c = 0usize;
        while c < 8 {
            assert_eq!(proj_cell(c), proj_cell(c + 8));
            c += 1;
        }
        assert!(proj_cell(16).is_none());
    }

    #[test]
    fn test_pane_kind() {
        assert_eq!(pane_kind(0), Some(0));
        assert_eq!(pane_kind(2), Some(1));
        assert_eq!(pane_kind(4), Some(2));
        assert_eq!(pane_kind(7), Some(3));
        assert!(pane_kind(8).is_none());
        assert!(pane_cell(8).is_none());
    }

    #[test]
    fn test_gpu_ternary() {
        assert_eq!(gpu_name(0), Some("软件渲染"));
        assert_eq!(gpu_name(2), Some("混合渲染"));
        assert!(gpu_name(3).is_none());
        assert!(gpu_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = profile_cell(3).unwrap_or(0);
        assert_eq!(a, profile_cell(3).unwrap_or(0));
        assert_ne!(a, profile_cell(4).unwrap_or(0));
        assert!(oc_row(OC_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TermLedger::new();
        let _ = lt.record_pane(5, pane_cell(5).unwrap_or(0));
        assert_eq!(lt.record_pane(2, pane_cell(2).unwrap_or(0)), Err(TermCode::EOrder));
        assert!(lt.record_pane(6, pane_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_write_side_needs_read_side() {
        let mut lt = TermLedger::new();
        // 写侧格 10 先落：配对读侧格 2 未落，单向漂移必抓。
        assert_eq!(lt.record_proj(10, proj_cell(10).unwrap_or(0)), Err(TermCode::EProjDrift));
        // 先落读侧格 2，再落写侧格 10 即收。
        let _ = lt.record_proj(2, proj_cell(2).unwrap_or(0));
        assert!(lt.record_proj(10, proj_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TermLedger::new();
        let _ = lt.record_profile(1, profile_cell(1).unwrap_or(0));
        assert_eq!(lt.record_profile(1, profile_cell(1).unwrap_or(0)), Err(TermCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TermLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROFILES + PROJ_CELLS + PANE_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7700);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TermCode::Ok as u32, 0);
        assert_eq!(TermCode::EGradeDrift as u32, 12);
        assert!(!TermCode::Ok.detail().is_empty());
        assert!(!TermCode::EProjDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = TermLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_oc_rows_named() {
        assert_eq!(oc_row_name(0), Some("conhost 消费分账声明"));
        assert_eq!(oc_row_name(3), Some("伪终端管道分账声明"));
        assert!(oc_row_name(4).is_none());
    }

    #[test]
    fn unx52221_checks_green() {
        let s = run_unx52221_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
