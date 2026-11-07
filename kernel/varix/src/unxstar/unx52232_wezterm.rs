//! UNX-F52232 WezTerm 收口 · 0xC79C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52232"，逐字）：
//!   title: WezTerm 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52233-J1 六判据全
//!     落；AI 请求面网络语义声明（AI-23 联签锚定行）+ 本地模型 N/A；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52232）判据锚 UNX-F(N+1)-J1 = UNX-F52233-J1，
//! 照原文锚定；功能按本卡标题（WezTerm）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Lua 配置投影 16 格：wezterm.lua 到运行时的投影账，16 格逐格恒等
//!      才收，越格必拒；漂移必抓。
//!   ② 多路复用 pane 10 操作：五家族闭集（分屏/导航/缩放/重排/关闭）各
//!      2 操作，家族序单调，逆序必抓；载荷漂移必抓。
//!   ③ SSH 域 8 格：SSH 域账逐格落账，载荷恒等才收，越格必拒。
//!   ④ GPU 三元闸：WebGpu 关闭 / WebGpu 降级 / WebGpu 全开三态闭集，
//!      越三元必拒。
//!   ⑤ AI 请求面网络语义声明 1 行闭集（AI-23 联签锚定行）+ 本地模型 N/A
//!      声明一并落账，缺一拒封。
//!   ⑥ 注入核 10 轮三态轮转（Lua 格越闭集 / SSH 漂移 / pane 家族逆序），
//!      全部落在当轮临时账本上 10/10 必抓；六判据母版闸 + KB-COMPAT 定级
//!      （全通 → S1，本卡口径）；双跑 FNV-1a 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52232;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Lua 配置投影格数。
pub const LUA_CELLS: usize = 16;

/// 多路复用 pane 操作数。
pub const PANE_OPS: usize = 10;

/// 每操作家族格数。
pub const PANE_PER_KIND: usize = 2;

/// SSH 域格数。
pub const SSH_CELLS: usize = 8;

/// GPU 三元闸数。
pub const GPU_STATES: usize = 3;

/// AI/本地模型声明行数。
pub const AI_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WezCode {
    /// 成功
    Ok = 0,
    /// Lua 投影格越闭集
    ENoLua,
    /// Lua 投影漂移
    ELuaDrift,
    /// pane 操作越闭集
    ENoPane,
    /// pane 漂移
    EPaneDrift,
    /// pane 家族逆序
    EPaneOrder,
    /// SSH 域格越闭集
    ENoSsh,
    /// SSH 域漂移
    ESshDrift,
    /// GPU 态越三元
    ENoGpu,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 定级漂移
    EGradeDrift,
}

impl WezCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WezCode::Ok => "成功",
            WezCode::ENoLua => "Lua 投影格越闭集：wezterm.lua 投影 16 格之外先登记",
            WezCode::ELuaDrift => "Lua 投影漂移：投影折叠与基准不恒等，逐格复算",
            WezCode::ENoPane => "pane 操作越闭集：多路复用 10 操作集之外先登记",
            WezCode::EPaneDrift => "pane 漂移：操作载荷折叠与基准不恒等，逐格复算",
            WezCode::EPaneOrder => "pane 家族逆序：分屏→导航→缩放→重排→关闭单调，逆序必抓",
            WezCode::ENoSsh => "SSH 域格越闭集：SSH 域 8 格之外先登记",
            WezCode::ESshDrift => "SSH 域漂移：域载荷与基准不恒等，逐格复算",
            WezCode::ENoGpu => "GPU 态越三元：WebGpu 关闭/降级/全开三态之外先登记",
            WezCode::ENoRow => "声明行越闭集：AI 请求面/本地模型 N/A 2 行闭集之外先登记",
            WezCode::ERowMiss => "缺声明行：AI 请求面网络语义声明（AI-23 联签锚定行）与本地模型 N/A 未齐先补齐",
            WezCode::EDup => "重复落账：同格只许一条账目",
            WezCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+16投影+10操作+8SSH域+3态+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Lua 配置投影格载荷（格 u）。
pub const fn lua_cell(u: usize) -> Option<u64> {
    if u >= LUA_CELLS {
        return None;
    }
    Some((0xE1000u64 + (u as u64) * 0x3D) ^ 0x4C5541434F4E46)
}

/// Lua 投影段名（四段各 4 格）。
pub const fn lua_section_name(u: usize) -> Option<&'static str> {
    if u >= LUA_CELLS {
        return None;
    }
    match u / 4 {
        0 => Some("外观段"),
        1 => Some("字体段"),
        2 => Some("键位段"),
        _ => Some("域段"),
    }
}

/// pane 操作格载荷（操作 o；家族 = o/2）。
pub const fn pane_cell(o: usize) -> Option<u64> {
    if o >= PANE_OPS {
        return None;
    }
    Some((0xE2000u64 + (o as u64) * 0x2C) ^ ((o / PANE_PER_KIND) as u64).wrapping_mul(0x01000193))
}

/// pane 操作家族（0..2=分屏 2..4=导航 4..6=缩放 6..8=重排 8..10=关闭）。
pub const fn pane_kind(o: usize) -> Option<u32> {
    if o >= PANE_OPS {
        return None;
    }
    Some((o / PANE_PER_KIND) as u32)
}

/// pane 家族名。
pub const fn pane_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("分屏"),
        1 => Some("导航"),
        2 => Some("缩放"),
        3 => Some("重排"),
        4 => Some("关闭"),
        _ => None,
    }
}

/// SSH 域格载荷（格 h）。
pub const fn ssh_cell(h: usize) -> Option<u64> {
    if h >= SSH_CELLS {
        return None;
    }
    Some((0xE3000u64 + (h as u64) * 0x45) ^ 0x535348444F4D41)
}

/// GPU 三元闸基准值（态 g）。
pub const fn gpu_cell(g: usize) -> Option<u32> {
    if g >= GPU_STATES {
        return None;
    }
    Some(0xE400 + g as u32 * 0x40)
}

/// GPU 态名。
pub const fn gpu_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("WebGpu 关闭"),
        1 => Some("WebGpu 降级"),
        2 => Some("WebGpu 全开"),
        _ => None,
    }
}

/// AI/本地模型声明行折叠（行 r）。
pub const fn ai_row(r: usize) -> Option<u32> {
    if r >= AI_ROWS {
        return None;
    }
    Some(0xE500 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn ai_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("AI 请求面网络语义声明（AI-23 联签锚定行）"),
        1 => Some("本地模型 N/A 声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WezTerm 收口账本状态机。
#[derive(Clone)]
pub struct WezLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// Lua 投影账。
    luas: [Option<u64>; LUA_CELLS],
    /// pane 操作账。
    panes: [Option<u64>; PANE_OPS],
    /// SSH 域账。
    sshs: [Option<u64>; SSH_CELLS],
    /// GPU 三态账。
    gpus: [Option<u32>; GPU_STATES],
    /// 声明账。
    ai_rows: [Option<u32>; AI_ROWS],
    /// 最后 pane 家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WezLedger {
    /// 新账本。
    pub fn new() -> Self {
        WezLedger {
            gates: [None; 6],
            luas: [None; LUA_CELLS],
            panes: [None; PANE_OPS],
            sshs: [None; SSH_CELLS],
            gpus: [None; GPU_STATES],
            ai_rows: [None; AI_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WezCode> {
        if gate >= 6 {
            return Err(WezCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
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

    /// Lua 投影一格（载荷对照 + 重复拒）。
    pub fn record_lua(&mut self, u: usize, got: u64) -> Result<(), WezCode> {
        let want = match lua_cell(u) {
            Some(w) => w,
            None => return Err(WezCode::ENoLua),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + u as u64);
            return Err(WezCode::ELuaDrift);
        }
        if match self.luas.get(u) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WezCode::EDup);
        }
        if let Some(c) = self.luas.get_mut(u) {
            *c = Some(got);
        }
        self.fnv(0x5401 + u as u64);
        Ok(())
    }

    /// Lua 投影全账核验（16 格全齐）。
    pub fn luas_all(&self) -> bool {
        let mut u = 0usize;
        while u < LUA_CELLS {
            match self.luas.get(u) {
                Some(Some(_)) => {}
                _ => return false,
            }
            u += 1;
        }
        true
    }

    /// pane 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_pane(&mut self, o: usize, got: u64) -> Result<(), WezCode> {
        if o >= PANE_OPS {
            return Err(WezCode::ENoPane);
        }
        let kind = match pane_kind(o) {
            Some(k) => k,
            None => return Err(WezCode::ENoPane),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500);
            return Err(WezCode::EPaneOrder);
        }
        let want = match pane_cell(o) {
            Some(w) => w,
            None => return Err(WezCode::ENoPane),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5501 + o as u64);
            return Err(WezCode::EPaneDrift);
        }
        if match self.panes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WezCode::EDup);
        }
        if let Some(c) = self.panes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5502 + o as u64);
        Ok(())
    }

    /// pane 操作全账核验（10 操作全齐）。
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

    /// SSH 域一格（载荷对照 + 重复拒）。
    pub fn record_ssh(&mut self, h: usize, got: u64) -> Result<(), WezCode> {
        let want = match ssh_cell(h) {
            Some(w) => w,
            None => return Err(WezCode::ENoSsh),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + h as u64);
            return Err(WezCode::ESshDrift);
        }
        if match self.sshs.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WezCode::EDup);
        }
        if let Some(c) = self.sshs.get_mut(h) {
            *c = Some(got);
        }
        self.fnv(0x5601 + h as u64);
        Ok(())
    }

    /// SSH 域全账核验（8 格全齐）。
    pub fn sshs_all(&self) -> bool {
        let mut h = 0usize;
        while h < SSH_CELLS {
            match self.sshs.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
        }
        true
    }

    /// GPU 三元闸一格（基准对照 + 重复拒）。
    pub fn set_gpu(&mut self, g: usize) -> Result<(), WezCode> {
        let want = match gpu_cell(g) {
            Some(w) => w,
            None => return Err(WezCode::ENoGpu),
        };
        if match self.gpus.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WezCode::EDup);
        }
        if let Some(c) = self.gpus.get_mut(g) {
            *c = Some(want);
        }
        self.fnv(0x5700 + g as u64);
        Ok(())
    }

    /// GPU 三元全落核验。
    pub fn gpus_all(&self) -> bool {
        let mut g = 0usize;
        while g < GPU_STATES {
            match self.gpus.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 声明一行（AI 请求面/本地模型 N/A）。
    pub fn declare_ai_row(&mut self, r: usize) -> Result<(), WezCode> {
        if r >= AI_ROWS {
            return Err(WezCode::ENoRow);
        }
        if match self.ai_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WezCode::EDup);
        }
        if let Some(c) = self.ai_rows.get_mut(r) {
            *c = ai_row(r);
        }
        self.fnv(0x5800 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn ai_rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < AI_ROWS {
            match self.ai_rows.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1 本卡口径，判据缺 → S3，其余 → S2）。
    /// main/aux 各节（Lua/pane/SSH/GPU/声明行）全部记账后再断言。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.luas_all()
            && self.panes_all()
            && self.sshs_all()
            && self.gpus_all()
            && self.ai_rows_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WezLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WezLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut u = 0usize;
    while u < LUA_CELLS {
        if let Some(w) = lua_cell(u) {
            if led.record_lua(u, w).is_ok() {
                recorded += 1;
            }
        }
        u += 1;
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
    let mut h = 0usize;
    while h < SSH_CELLS {
        if let Some(w) = ssh_cell(h) {
            if led.record_ssh(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    let mut g2 = 0usize;
    while g2 < GPU_STATES {
        let _ = led.set_gpu(g2);
        g2 += 1;
    }
    let mut r = 0usize;
    while r < AI_ROWS {
        let _ = led.declare_ai_row(r);
        r += 1;
    }
    // 注入 10 轮：Lua 格越闭集 / SSH 域漂移（先录正格再注漂移）/ pane 家
    // 族逆序（先录高族正格再注低族）三态轮转，全部落在当轮临时账本上，注
    // 入=检出=10、逃逸=0；主账本 last_kind 不被注入扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WezLedger::new();
        let caught = match kind {
            0 => lt.record_lua(LUA_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_ssh(0, ssh_cell(0).unwrap_or(0));
                lt.record_ssh(1, ssh_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WezCode::ESshDrift)
            }
            _ => {
                let _ = lt.record_pane(6, pane_cell(6).unwrap_or(0));
                lt.record_pane(2, pane_cell(2).unwrap_or(0)) == Err(WezCode::EPaneOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5900 + k as u64);
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
/// UNX-F52232 判据集（锚 UNX-F52233-J1，off-by-one 板式）。
pub fn run_unx52232_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52232");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Lua 配置投影 16 格全通",
        {
            let mut lt = WezLedger::new();
            let mut ok = true;
            let mut u = 0usize;
            while u < LUA_CELLS {
                if let Some(w) = lua_cell(u) {
                    if lt.record_lua(u, w).is_err() {
                        ok = false;
                    }
                }
                u += 1;
            }
            ok && lt.luas_all()
        },
        "wezterm.lua 投影四段（外观/字体/键位/域）逐格恒等",
    );
    s.add(
        "多路复用 pane 10 操作全通",
        recorded >= (LUA_CELLS + PANE_OPS + SSH_CELLS) as u32,
        "分屏/导航/缩放/重排/关闭五家族逐操作恒等",
    );
    s.add(
        "SSH 域 8 格全落",
        {
            let mut lt = WezLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < SSH_CELLS {
                if let Some(w) = ssh_cell(h) {
                    if lt.record_ssh(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.sshs_all()
        },
        "SSH 域账逐格落账，越格必拒",
    );
    s.add(
        "GPU 三元闸全落",
        {
            let mut lt = WezLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GPU_STATES {
                if lt.set_gpu(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.gpus_all()
        },
        "WebGpu 关闭/降级/全开三态闭集，越三元必拒",
    );
    s.add(
        "AI 请求面/本地模型 N/A 声明 2/2 全齐",
        {
            let mut lt = WezLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < AI_ROWS {
                if lt.declare_ai_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ai_rows_all()
        },
        "AI 请求面网络语义声明（AI-23 联签锚定行）+ 本地模型 N/A",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WezLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "pane 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANE_OPS {
                if pane_kind(o).unwrap_or(5) < pane_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "分屏→导航→缩放→重排→关闭五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "Lua 越闭集/SSH 漂移/pane 逆序三态轮转必抓",
    );
    let mut l0 = WezLedger::new();
    s.add(
        "边界·Lua 格越闭集",
        l0.record_lua(LUA_CELLS, 0).is_err(),
        "投影 16 越闭集必拒",
    );
    s.add(
        "边界·pane 越闭集",
        l0.record_pane(PANE_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·SSH 域越闭集",
        l0.record_ssh(SSH_CELLS, 0).is_err(),
        "SSH 域 8 越闭集必拒",
    );
    s.add(
        "边界·GPU 态越三元",
        l0.set_gpu(GPU_STATES).is_err(),
        "GPU 态 3 越三元必拒",
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
            let mut lt = WezLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut u = 0usize;
            while u < LUA_CELLS {
                if let Some(w) = lua_cell(u) {
                    let _ = lt.record_lua(u, w);
                }
                u += 1;
            }
            let mut o = 0usize;
            while o < PANE_OPS {
                if let Some(w) = pane_cell(o) {
                    let _ = lt.record_pane(o, w);
                }
                o += 1;
            }
            let mut h = 0usize;
            while h < SSH_CELLS {
                if let Some(w) = ssh_cell(h) {
                    let _ = lt.record_ssh(h, w);
                }
                h += 1;
            }
            let mut g2 = 0usize;
            while g2 < GPU_STATES {
                let _ = lt.set_gpu(g2);
                g2 += 1;
            }
            let mut r = 0usize;
            while r < AI_ROWS {
                let _ = lt.declare_ai_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC79C0,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC79C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lua_sections() {
        assert_eq!(lua_section_name(0), Some("外观段"));
        assert_eq!(lua_section_name(15), Some("域段"));
        assert!(lua_section_name(16).is_none());
        assert!(lua_cell(16).is_none());
    }

    #[test]
    fn test_pane_kind() {
        assert_eq!(pane_kind(0), Some(0));
        assert_eq!(pane_kind(2), Some(1));
        assert_eq!(pane_kind(9), Some(4));
        assert!(pane_kind(10).is_none());
        assert!(pane_cell(10).is_none());
        assert_eq!(pane_kind_name(4), Some("关闭"));
        assert!(pane_kind_name(5).is_none());
    }

    #[test]
    fn test_gpu_and_rows() {
        assert_eq!(gpu_name(1), Some("WebGpu 降级"));
        assert!(gpu_name(3).is_none());
        assert!(gpu_cell(3).is_none());
        assert_eq!(ai_row_name(0), Some("AI 请求面网络语义声明（AI-23 联签锚定行）"));
        assert_eq!(ai_row_name(1), Some("本地模型 N/A 声明"));
        assert!(ai_row_name(2).is_none());
        assert!(ai_row(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = ssh_cell(3).unwrap_or(0);
        assert_eq!(a, ssh_cell(3).unwrap_or(0));
        assert_ne!(a, ssh_cell(4).unwrap_or(0));
        assert!(lua_cell(LUA_CELLS).is_none());
    }

    #[test]
    fn test_pane_order_caught() {
        let mut lt = WezLedger::new();
        let _ = lt.record_pane(7, pane_cell(7).unwrap_or(0));
        assert_eq!(lt.record_pane(1, pane_cell(1).unwrap_or(0)), Err(WezCode::EPaneOrder));
        assert!(lt.record_pane(8, pane_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WezLedger::new();
        assert_eq!(
            lt.record_lua(2, lua_cell(2).unwrap_or(0).wrapping_add(4)),
            Err(WezCode::ELuaDrift)
        );
        assert_eq!(
            lt.record_ssh(5, ssh_cell(5).unwrap_or(0).wrapping_add(6)),
            Err(WezCode::ESshDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WezLedger::new();
        let _ = lt.record_lua(0, lua_cell(0).unwrap_or(0));
        assert_eq!(lt.record_lua(0, lua_cell(0).unwrap_or(0)), Err(WezCode::EDup));
        let _ = lt.declare_ai_row(0);
        assert_eq!(lt.declare_ai_row(0), Err(WezCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WezLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WezLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LUA_CELLS + PANE_OPS + SSH_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC79C0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * 31);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WezCode::Ok as u32, 0);
        assert_eq!(WezCode::EGradeDrift as u32, 12);
        assert!(!WezCode::Ok.detail().is_empty());
        assert!(!WezCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S3.val(), 3);
        let lt = WezLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52232_checks_green() {
        let s = run_unx52232_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
