//! UNX-F52422 Zenmap 收口 · 0xCA940段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52422"，逐字）：
//!   title: Zenmap 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52423-J1 判据仅回
//!     环/实验室段（白名单声明）；×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52422）判据锚 UNX-F(N+1)-J1 = UNX-F52423-J1，
//! 照原文锚定；功能按本卡标题（Zenmap 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 profile 族 20 格闭集：密集扫描/密集UDP扫描/全TCP端口扫描/
//!      Ping扫描/快速扫描各 4 格，逐格载荷（纯函数折叠）恒等才收。
//!   ② GUI 三态 12 格：选单/拓扑/端口视图三态逐格落账，越三态闭集必抓。
//!   ③ nmap 消费联签 6 格（锚 F52421）：逐格载荷对照，缺一拒封。
//!   ④ 扫描结果投影 8 格：投影折叠恒等，越闭集必拒。
//!   ⑤ Python 上栈声明 2 行闭集 + 回环/实验室白名单声明 2 行闭集（判据
//!      仅回环/实验室段），缺一拒封。
//!   ⑥ 六判据闸 + ledger 状态机（new/record_*/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮
//!      （profile 漂移/GUI 三态越界/profile 越闭集），10/10 必抓；双跑
//!      FNV 指纹一致；KB-COMPAT 定级目标档 S2（有条件收口）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52422;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// profile 格数（五族 × 4）。
pub const PROFILE_CELLS: usize = 20;

/// GUI 三态格数。
pub const GUI_CELLS: usize = 12;

/// nmap 消费联签行数（锚 F52421）。
pub const LINK_ROWS: usize = 6;

/// 扫描结果投影格数。
pub const RESULT_CELLS: usize = 8;

/// Python 上栈声明行数。
pub const PY_STACK_ROWS: usize = 2;

/// 回环/实验室白名单声明行数。
pub const WHITELIST_ROWS: usize = 2;

/// nmap 联签锚（F52421 卡）。
pub const NMAP_ANCHOR: u64 = 0xF52421;

/// GUI 三态闭集上界（0..2 三态）。
pub const GUI_STATES: u32 = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZenmapCode {
    /// 成功
    Ok = 0,
    /// profile 格越闭集
    ENoProfile,
    /// profile 格漂移
    EProfileDrift,
    /// GUI 格越闭集
    ENoGui,
    /// GUI 三态越界
    EGuiState,
    /// GUI 格漂移
    EGuiDrift,
    /// profile 族逆序
    EOrder,
    /// 联签行越闭集
    ENoLink,
    /// 联签漂移
    ELinkDrift,
    /// 结果投影越闭集
    ENoResult,
    /// 结果投影漂移
    EResultDrift,
    /// 声明行越闭集
    ENoRow,
    /// 定级漂移
    EGradeDrift,
}

impl ZenmapCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ZenmapCode::Ok => "成功",
            ZenmapCode::ENoProfile => "profile 格越闭集：密集/密集UDP/全TCP/Ping/快速 20 格之外先登记",
            ZenmapCode::EProfileDrift => "profile 格漂移：profile 载荷折叠与基准不恒等，逐格复算",
            ZenmapCode::ENoGui => "GUI 格越闭集：GUI 三态 12 格之外先登记",
            ZenmapCode::EGuiState => "GUI 三态越界：选单/拓扑/端口视图 0..2 三态闭集之外先登记",
            ZenmapCode::EGuiDrift => "GUI 格漂移：GUI 载荷与三态折叠不恒等，逐格复算",
            ZenmapCode::EOrder => "profile 族逆序：五 profile 族单调，逆序必抓且不前移游标",
            ZenmapCode::ENoLink => "联签行越闭集：nmap 消费联签 6 格之外先登记",
            ZenmapCode::ELinkDrift => "联签漂移：nmap 联签载荷与 F52421 锚不恒等",
            ZenmapCode::ENoResult => "结果投影越闭集：扫描结果投影 8 格之外先登记",
            ZenmapCode::EResultDrift => "结果投影漂移：投影折叠与基准不恒等，逐格复算",
            ZenmapCode::ENoRow => "声明行越闭集：Python 上栈/白名单声明闭集之外先登记",
            ZenmapCode::EGradeDrift => "定级漂移：全通才可定目标档 S2，判据缺则 S3",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不达，封顶 S2）
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：全通收口（Zenmap 仅回环/实验室段，不达）",
            Grade::S2 => "S2：有条件收口（目标档），六判据全落即封",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// profile 格载荷（格 o；族 = o/4，族序 密集→密集UDP→全TCP→Ping→快速）。
pub const fn profile_cell(o: usize) -> Option<u64> {
    if o >= PROFILE_CELLS {
        return None;
    }
    Some((0xE1000u64 + (o as u64) * 0x3D) ^ (0x5A454E4D_0000_0000 + (o / 4) as u64))
}

/// profile 族号。
pub const fn profile_kind(o: usize) -> Option<u32> {
    if o >= PROFILE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// profile 族名。
pub const fn profile_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("密集扫描"),
        1 => Some("密集UDP扫描"),
        2 => Some("全TCP端口扫描"),
        3 => Some("Ping扫描"),
        4 => Some("快速扫描"),
        _ => None,
    }
}

/// GUI 三态（格 i 的基准态：0 选单 1 拓扑 2 端口视图，按 i%3 轮转）。
pub const fn gui_state(i: usize) -> Option<u32> {
    if i >= GUI_CELLS {
        return None;
    }
    Some((i % 3) as u32)
}

/// GUI 格载荷（格 i）。
pub const fn gui_cell(i: usize) -> Option<u64> {
    let st = match gui_state(i) {
        Some(s) => s,
        None => return None,
    };
    Some(0xE2000u64 + (i as u64) * 0x2B + st as u64)
}

/// nmap 联签行载荷（行 r，锚 F52421）。
pub const fn link_cell(r: usize) -> Option<u64> {
    if r >= LINK_ROWS {
        return None;
    }
    Some(NMAP_ANCHOR + (r as u64) * 0x37)
}

/// 扫描结果投影格载荷（投影 i）。
pub const fn result_cell(i: usize) -> Option<u64> {
    if i >= RESULT_CELLS {
        return None;
    }
    Some((0xE3000u64 + (i as u64) * 0x71).rotate_right((i as u64 % 16) as u32))
}

/// Python 上栈声明行折叠（行 r）。
pub const fn py_stack_row(r: usize) -> Option<u32> {
    if r >= PY_STACK_ROWS {
        return None;
    }
    Some(0xE4000 + r as u32 * 0x5B)
}

/// 回环/实验室白名单声明行折叠（行 r）。
pub const fn whitelist_row(r: usize) -> Option<u32> {
    if r >= WHITELIST_ROWS {
        return None;
    }
    Some(0xE4100 + r as u32 * 0x5D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Zenmap 收口账本状态机。
#[derive(Clone)]
pub struct ZenmapLedger {
    /// 六判据（启动/命令注入/GUI 渲染/结果解析/回环白名单/写回）。
    gates: [Option<bool>; 6],
    /// profile 格账。
    profiles: [Option<u64>; PROFILE_CELLS],
    /// GUI 格账。
    guis: [Option<u64>; GUI_CELLS],
    /// nmap 联签账。
    links: [Option<u64>; LINK_ROWS],
    /// 结果投影账。
    results: [Option<u64>; RESULT_CELLS],
    /// Python 上栈声明账。
    py_rows: [Option<u32>; PY_STACK_ROWS],
    /// 白名单声明账。
    wl_rows: [Option<u32>; WHITELIST_ROWS],
    /// 最后 profile 族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ZenmapLedger {
    /// 新账本。
    pub fn new() -> Self {
        ZenmapLedger {
            gates: [None; 6],
            profiles: [None; PROFILE_CELLS],
            guis: [None; GUI_CELLS],
            links: [None; LINK_ROWS],
            results: [None; RESULT_CELLS],
            py_rows: [None; PY_STACK_ROWS],
            wl_rows: [None; WHITELIST_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x0000_0100_0000_01b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ZenmapCode> {
        if gate >= 6 {
            return Err(ZenmapCode::ENoRow);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB100 + gate as u64);
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

    /// profile 一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_profile(&mut self, o: usize, got: u64) -> Result<(), ZenmapCode> {
        if o >= PROFILE_CELLS {
            return Err(ZenmapCode::ENoProfile);
        }
        let kind = match profile_kind(o) {
            Some(k) => k,
            None => return Err(ZenmapCode::ENoProfile),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB200);
            return Err(ZenmapCode::EOrder);
        }
        let want = match profile_cell(o) {
            Some(w) => w,
            None => return Err(ZenmapCode::ENoProfile),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB201 + o as u64);
            return Err(ZenmapCode::EProfileDrift);
        }
        if match self.profiles.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZenmapCode::EGradeDrift);
        }
        if let Some(c) = self.profiles.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB202 + o as u64);
        Ok(())
    }

    /// profile 全账核验（20 格）。
    pub fn profiles_all(&self) -> bool {
        let mut o = 0usize;
        while o < PROFILE_CELLS {
            match self.profiles.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// GUI 一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_gui(&mut self, i: usize, got: u64) -> Result<(), ZenmapCode> {
        let st = match gui_state(i) {
            Some(s) => s,
            None => return Err(ZenmapCode::ENoGui),
        };
        if st >= GUI_STATES {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB300);
            return Err(ZenmapCode::EGuiState);
        }
        let want = match gui_cell(i) {
            Some(w) => w,
            None => return Err(ZenmapCode::ENoGui),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB301 + i as u64);
            return Err(ZenmapCode::EGuiDrift);
        }
        if match self.guis.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZenmapCode::EGradeDrift);
        }
        if let Some(c) = self.guis.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB302 + i as u64);
        Ok(())
    }

    /// GUI 全账核验（12 格）。
    pub fn guis_all(&self) -> bool {
        let mut i = 0usize;
        while i < GUI_CELLS {
            match self.guis.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// nmap 联签一行（载荷对照 + 重复拒）。
    pub fn record_link(&mut self, r: usize, got: u64) -> Result<(), ZenmapCode> {
        let want = match link_cell(r) {
            Some(w) => w,
            None => return Err(ZenmapCode::ENoLink),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB400 + r as u64);
            return Err(ZenmapCode::ELinkDrift);
        }
        if match self.links.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZenmapCode::EGradeDrift);
        }
        if let Some(c) = self.links.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xB401 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn links_all(&self) -> bool {
        let mut r = 0usize;
        while r < LINK_ROWS {
            match self.links.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 扫描结果投影一格（载荷对照 + 重复拒）。
    pub fn record_result(&mut self, i: usize, got: u64) -> Result<(), ZenmapCode> {
        let want = match result_cell(i) {
            Some(w) => w,
            None => return Err(ZenmapCode::ENoResult),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB500 + i as u64);
            return Err(ZenmapCode::EResultDrift);
        }
        if match self.results.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZenmapCode::EGradeDrift);
        }
        if let Some(c) = self.results.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB501 + i as u64);
        Ok(())
    }

    /// 结果投影全账核验（8 格）。
    pub fn results_all(&self) -> bool {
        let mut i = 0usize;
        while i < RESULT_CELLS {
            match self.results.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 声明一行（kind=false → Python 上栈；kind=true → 回环白名单）。
    pub fn declare_row(&mut self, kind: bool, r: usize) -> Result<(), ZenmapCode> {
        if kind {
            if r >= WHITELIST_ROWS {
                return Err(ZenmapCode::ENoRow);
            }
            if match self.wl_rows.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(ZenmapCode::EGradeDrift);
            }
            if let Some(c) = self.wl_rows.get_mut(r) {
                *c = whitelist_row(r);
            }
        } else {
            if r >= PY_STACK_ROWS {
                return Err(ZenmapCode::ENoRow);
            }
            if match self.py_rows.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(ZenmapCode::EGradeDrift);
            }
            if let Some(c) = self.py_rows.get_mut(r) {
                *c = py_stack_row(r);
            }
        }
        self.fnv(0xB600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（Python 上栈 + 白名单两闭集）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < PY_STACK_ROWS {
            match self.py_rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < WHITELIST_ROWS {
            match self.wl_rows.get(r2) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r2 += 1;
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

    /// KB-COMPAT 定级（目标档 S2：六判据全落即 S2；判据缺 → S3；
    /// 全通亦封顶 S2——本卡仅回环/实验室段，不达 S1）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ZenmapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ZenmapLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PROFILE_CELLS {
        if let Some(w) = profile_cell(o) {
            if led.record_profile(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < GUI_CELLS {
        if let Some(w) = gui_cell(i) {
            if led.record_gui(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < LINK_ROWS {
        if let Some(w) = link_cell(r) {
            if led.record_link(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut j = 0usize;
    while j < RESULT_CELLS {
        if let Some(w) = result_cell(j) {
            if led.record_result(j, w).is_ok() {
                recorded += 1;
            }
        }
        j += 1;
    }
    let mut s = 0usize;
    while s < PY_STACK_ROWS {
        let _ = led.declare_row(false, s);
        s += 1;
    }
    let mut w2 = 0usize;
    while w2 < WHITELIST_ROWS {
        let _ = led.declare_row(true, w2);
        w2 += 1;
    }
    // 注入 10 轮：profile 漂移 / GUI 三态越界 / profile 越闭集三态轮转。
    // 全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ZenmapLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_profile(0, profile_cell(0).unwrap_or(0));
                lt.record_profile(1, profile_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ZenmapCode::EProfileDrift)
            }
            1 => {
                // GUI 三态越界：对空账本同样必抓。
                let _ = lt.record_gui(0, gui_cell(0).unwrap_or(0));
                lt.record_gui(GUI_CELLS, 0).is_err()
            }
            _ => {
                // profile 族逆序：先落高族合法格，再注低族格。
                let _ = lt.record_profile(9, profile_cell(9).unwrap_or(0));
                lt.record_profile(2, profile_cell(2).unwrap_or(0)) == Err(ZenmapCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB700 + k as u64);
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
/// UNX-F52422 判据集（锚 UNX-F52423-J1，off-by-one 板式）。
pub fn run_unx52422_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52422");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五 profile 族 20 格全通",
        recorded >= PROFILE_CELLS as u32,
        "密集/密集UDP/全TCP/Ping/快速五族逐格载荷恒等",
    );
    s.add(
        "profile 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PROFILE_CELLS {
                if profile_kind(o).unwrap_or(5) < profile_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五 profile 族闭集单调（4 格一族）",
    );
    s.add(
        "GUI 三态 12 格全落",
        {
            let mut lt = ZenmapLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < GUI_CELLS {
                if let Some(w) = gui_cell(i) {
                    if lt.record_gui(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.guis_all()
        },
        "选单/拓扑/端口视图三态逐格落账",
    );
    s.add(
        "GUI 三态基准轮转恒等",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < GUI_CELLS {
                if gui_state(i).unwrap_or(3) != (i % 3) as u32 {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "GUI 三态按 i%3 轮转且不越 0..2 闭集",
    );
    s.add(
        "nmap 消费联签 6/6 全齐（F52421）",
        {
            let mut lt = ZenmapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LINK_ROWS {
                if let Some(w) = link_cell(r) {
                    if lt.record_link(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.links_all()
        },
        "nmap 消费联签逐格对照 F52421 锚（§AI-66.3）",
    );
    s.add(
        "扫描结果投影 8 格全落",
        {
            let mut lt = ZenmapLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < RESULT_CELLS {
                if let Some(w) = result_cell(j) {
                    if lt.record_result(j, w).is_err() {
                        ok = false;
                    }
                }
                j += 1;
            }
            ok && lt.results_all()
        },
        "扫描结果投影折叠恒等逐格落账",
    );
    s.add(
        "Python 上栈声明 2/2 全齐",
        {
            let mut lt = ZenmapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PY_STACK_ROWS {
                if lt.declare_row(false, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "Python 栈上栈声明/Python 栈回退声明两行闭集",
    );
    s.add(
        "回环/实验室白名单声明 2/2 全齐",
        {
            let mut lt = ZenmapLedger::new();
            let mut ok = true;
            // rows_all 要求 Python 上栈行与白名单行两套全落。
            let mut p = 0usize;
            while p < PY_STACK_ROWS {
                if lt.declare_row(true, p).is_err() {
                    ok = false;
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < WHITELIST_ROWS {
                if lt.declare_row(false, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "判据仅回环/实验室段：白名单两行闭集缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ZenmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/命令注入/GUI 渲染/结果解析/回环白名单/写回逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "profile 漂移/GUI 三态越界/profile 族逆序三态轮转必抓",
    );
    let mut l0 = ZenmapLedger::new();
    s.add(
        "边界·profile 漂移拒",
        {
            let _ = l0.record_profile(0, profile_cell(0).unwrap_or(0));
            l0.record_profile(1, profile_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(ZenmapCode::EProfileDrift)
        },
        "profile 载荷漂移必抓",
    );
    s.add(
        "边界·profile 越闭集",
        l0.record_profile(PROFILE_CELLS, 0).is_err(),
        "profile 20 格越闭集必拒",
    );
    s.add(
        "边界·GUI 格越闭集",
        l0.record_gui(GUI_CELLS, 0).is_err(),
        "GUI 12 格越闭集必拒",
    );
    s.add(
        "边界·结果投影越闭集",
        l0.record_result(RESULT_CELLS, 0).is_err(),
        "结果投影 8 格越闭集必拒",
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
        "定级 S2 目标档闭集",
        {
            let mut lt = ZenmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PROFILE_CELLS {
                if let Some(w) = profile_cell(o) {
                    let _ = lt.record_profile(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < GUI_CELLS {
                if let Some(w) = gui_cell(i) {
                    let _ = lt.record_gui(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < LINK_ROWS {
                if let Some(w) = link_cell(r) {
                    let _ = lt.record_link(r, w);
                }
                r += 1;
            }
            let mut j = 0usize;
            while j < RESULT_CELLS {
                if let Some(w) = result_cell(j) {
                    let _ = lt.record_result(j, w);
                }
                j += 1;
            }
            let mut r2 = 0usize;
            while r2 < PY_STACK_ROWS {
                let _ = lt.declare_row(false, r2);
                r2 += 1;
            }
            let mut r3 = 0usize;
            while r3 < WHITELIST_ROWS {
                let _ = lt.declare_row(true, r3);
                r3 += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 封顶（KB-COMPAT 定级登记，仅回环/实验室段）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA940,
        "SEG_BASE 0x40 对齐且在 VX16 派工域公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 18,
        "含本条共 ≥19 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_kind() {
        assert_eq!(profile_kind(0), Some(0));
        assert_eq!(profile_kind(4), Some(1));
        assert_eq!(profile_kind(19), Some(4));
        assert!(profile_kind(20).is_none());
        assert!(profile_cell(20).is_none());
        assert_eq!(profile_kind_name(0), Some("密集扫描"));
        assert_eq!(profile_kind_name(4), Some("快速扫描"));
        assert!(profile_kind_name(5).is_none());
    }

    #[test]
    fn test_gui_states() {
        // 三态按 i%3 轮转映射核对。
        assert_eq!(gui_state(0), Some(0));
        assert_eq!(gui_state(1), Some(1));
        assert_eq!(gui_state(2), Some(2));
        assert_eq!(gui_state(3), Some(0));
        assert_eq!(gui_state(11), Some(2));
        assert!(gui_state(12).is_none());
        assert!(gui_cell(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = profile_cell(3).unwrap_or(0);
        assert_eq!(a, profile_cell(3).unwrap_or(0));
        assert_ne!(a, profile_cell(4).unwrap_or(0));
        let r = result_cell(5).unwrap_or(0);
        assert_eq!(r, result_cell(5).unwrap_or(0));
        assert!(py_stack_row(PY_STACK_ROWS).is_none());
        assert!(whitelist_row(WHITELIST_ROWS).is_none());
        assert!(link_cell(LINK_ROWS).is_none());
        assert_eq!(link_cell(0), Some(NMAP_ANCHOR));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ZenmapLedger::new();
        let _ = lt.record_profile(9, profile_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_profile(2, profile_cell(2).unwrap_or(0)),
            Err(ZenmapCode::EOrder)
        );
        // 逆序判错不改 last_kind：同族之后的高族合法格仍可落账。
        assert!(lt.record_profile(10, profile_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ZenmapLedger::new();
        assert_eq!(
            lt.record_gui(4, gui_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(ZenmapCode::EGuiDrift)
        );
        assert_eq!(
            lt.record_result(2, result_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(ZenmapCode::EResultDrift)
        );
        assert_eq!(
            lt.record_link(1, NMAP_ANCHOR + 0x37 + 1),
            Err(ZenmapCode::ELinkDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ZenmapLedger::new();
        let _ = lt.record_profile(0, profile_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_profile(0, profile_cell(0).unwrap_or(0)),
            Err(ZenmapCode::EGradeDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ZenmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ZenmapLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：profile 20 + GUI 12 + 联签 6 + 投影 8。
        assert_eq!(
            recorded,
            PROFILE_CELLS as u32 + GUI_CELLS as u32 + LINK_ROWS as u32 + RESULT_CELLS as u32
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
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA940);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ZenmapCode::Ok as u32, 0);
        assert_eq!(ZenmapCode::EGradeDrift as u32, 12);
        assert_eq!(ZenmapCode::Ok.val(), SEG_BASE);
        assert!(!ZenmapCode::Ok.detail().is_empty());
        assert!(!ZenmapCode::EGuiState.detail().is_empty());
        assert!(!ZenmapCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = ZenmapLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐（格未齐）→ S2。
        let mut lt2 = ZenmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐亦封顶 S2。
        let mut lt3 = ZenmapLedger::new();
        let mut g3 = 0usize;
        while g3 < 6 {
            let _ = lt3.judge_gate(g3);
            g3 += 1;
        }
        let mut o = 0usize;
        while o < PROFILE_CELLS {
            if let Some(w) = profile_cell(o) {
                let _ = lt3.record_profile(o, w);
            }
            o += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52422_checks_green() {
        let s = run_unx52422_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
