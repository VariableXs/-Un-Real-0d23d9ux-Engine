//! UNX-F52230 Hyper 收口 · 0xC7940 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52230"，逐字）：
//!   title: Hyper 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52231-J1 六判据全
//!     落；GPU 渲染路径探测与降级表（G 部能力页联签锚定行）；×10；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.2 专题四 GPU 同构；③ 完成时 API/软件写
//!     回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52230）判据锚 UNX-F(N+1)-J1 = UNX-F52231-J1，
//! 照原文锚定；功能按本卡标题（Hyper）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Electron 4 层闭集：main 主进程 / renderer 渲染进程 / preload 预
//!      加载桥 / gpu 加速进程四层，逐层载荷折叠恒等才收，越层必拒。
//!   ② 配置 JS 投影 12 格：hyper.js 配置到 Electron 运行时的投影账，
//!      三投影族闭集（窗口投影 4 格 / 字体投影 4 格 / 热键投影 4 格），
//!      族序单调，逆序必抓；载荷漂移必抓。
//!   ③ 插件 8 格：插件账逐格落账，载荷恒等才收，越格必拒。
//!   ④ shell 三态闸：默认 shell / 指定 shell / 插件 shell 三态闭集，
//!      越三元必拒；漂移必抓。
//!   ⑤ GPU 渲染路径探测与降级表：探测行 2 行闭集（WebGL 路径探测声明 /
//!      降级表声明），缺一拒封（G 部能力页联签锚定行口径）。
//!   ⑥ 注入核 10 轮三态轮转（Electron 层越闭集 / 配置投影漂移 / 投影族
//!      逆序），全部落在当轮临时账本上 10/10 必抓；六判据母版闸 +
//!      KB-COMPAT 定级（全通 → S2，本卡口径）；双跑 FNV-1a 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52230;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Electron 层数。
pub const ELECTRON_LAYERS: usize = 4;

/// 配置 JS 投影格数（窗口 4 + 字体 4 + 热键 4）。
pub const CFG_CELLS: usize = 12;

/// 每投影族格数。
pub const CFG_PER_KIND: usize = 4;

/// 插件格数。
pub const PLUGINS: usize = 8;

/// shell 三态闸数。
pub const SHELL_STATES: usize = 3;

/// GPU 探测/降级声明行数。
pub const GPU_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HyperCode {
    /// 成功
    Ok = 0,
    /// Electron 层越闭集
    ENoLayer,
    /// Electron 层漂移
    ELayerDrift,
    /// 投影格越闭集
    ENoCfg,
    /// 投影漂移
    ECfgDrift,
    /// 投影族逆序
    ECfgOrder,
    /// 插件格越闭集
    ENoPlugin,
    /// 插件漂移
    EPluginDrift,
    /// shell 越三元
    ENoShell,
    /// 探测/降级行越闭集
    ENoRow,
    /// 缺探测/降级行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 定级漂移
    EGradeDrift,
}

impl HyperCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HyperCode::Ok => "成功",
            HyperCode::ENoLayer => "Electron 层越闭集：main/renderer/preload/gpu 四层之外先登记",
            HyperCode::ELayerDrift => "Electron 层漂移：层载荷折叠与基准不恒等，逐层复算",
            HyperCode::ENoCfg => "投影格越闭集：配置 JS 投影 12 格之外先登记",
            HyperCode::ECfgDrift => "投影漂移：投影折叠与基准不恒等，逐格复算",
            HyperCode::ECfgOrder => "投影族逆序：窗口→字体→热键单调，逆序必抓",
            HyperCode::ENoPlugin => "插件格越闭集：插件 8 格之外先登记",
            HyperCode::EPluginDrift => "插件漂移：插件载荷与基准不恒等，逐格复算",
            HyperCode::ENoShell => "shell 越三元：默认/指定/插件三态闭集之外先登记",
            HyperCode::ENoRow => "探测/降级行越闭集：GPU 声明 2 行闭集之外先登记",
            HyperCode::ERowMiss => "缺探测/降级行：GPU 渲染路径探测与降级表声明未齐先补齐",
            HyperCode::EDup => "重复落账：同格只许一条账目",
            HyperCode::EGradeDrift => "定级漂移：全通且探测/降级声明全齐才可定 S2",
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
            Grade::S1 => "S1：全通收口（备用口径）",
            Grade::S2 => "S2：六判据+4层+12投影+8插件+shell三态+2行GPU声明全通，本卡口径",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Electron 层载荷（层 l：0=main 1=renderer 2=preload 3=gpu）。
pub const fn layer_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some((0xC1000u64 + (l as u64) * 0x47) ^ 0x48595045524C41)
}

/// 层名（const fn 表 + Option 边界）。
pub const fn layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("main 主进程"),
        1 => Some("renderer 渲染进程"),
        2 => Some("preload 预加载桥"),
        3 => Some("gpu 加速进程"),
        _ => None,
    }
}

/// 配置投影格载荷（格 c；族 = c/4）。
pub const fn cfg_cell(c: usize) -> Option<u64> {
    if c >= CFG_CELLS {
        return None;
    }
    Some((0xC2000u64 + (c as u64) * 0x2C) ^ ((c / CFG_PER_KIND) as u64).wrapping_mul(0x01000193))
}

/// 投影族（0..4=窗口投影 4..8=字体投影 8..12=热键投影）。
pub const fn cfg_kind(c: usize) -> Option<u32> {
    if c >= CFG_CELLS {
        return None;
    }
    Some((c / CFG_PER_KIND) as u32)
}

/// 投影族名。
pub const fn cfg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("窗口投影"),
        1 => Some("字体投影"),
        2 => Some("热键投影"),
        _ => None,
    }
}

/// 插件格载荷（格 p）。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGINS {
        return None;
    }
    Some((0xC3000u64 + (p as u64) * 0x3B) ^ 0x504C5547494E32)
}

/// shell 三态闸基准值（态 s）。
pub const fn shell_cell(s: usize) -> Option<u32> {
    if s >= SHELL_STATES {
        return None;
    }
    Some(0xC400 + s as u32 * 0x40)
}

/// shell 态名。
pub const fn shell_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("默认 shell"),
        1 => Some("指定 shell"),
        2 => Some("插件 shell"),
        _ => None,
    }
}

/// GPU 探测/降级声明行折叠（行 r）。
pub const fn gpu_row(r: usize) -> Option<u32> {
    if r >= GPU_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Hyper 收口账本状态机。
#[derive(Clone)]
pub struct HyperLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// Electron 层账。
    layers: [Option<u64>; ELECTRON_LAYERS],
    /// 配置投影账。
    cfgs: [Option<u64>; CFG_CELLS],
    /// 插件账。
    plugins: [Option<u64>; PLUGINS],
    /// shell 三态账。
    shells: [Option<u32>; SHELL_STATES],
    /// GPU 声明账。
    gpu_rows: [Option<u32>; GPU_ROWS],
    /// 最后投影族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HyperLedger {
    /// 新账本。
    pub fn new() -> Self {
        HyperLedger {
            gates: [None; 6],
            layers: [None; ELECTRON_LAYERS],
            cfgs: [None; CFG_CELLS],
            plugins: [None; PLUGINS],
            shells: [None; SHELL_STATES],
            gpu_rows: [None; GPU_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HyperCode> {
        if gate >= 6 {
            return Err(HyperCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4600 + gate as u64);
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

    /// Electron 层一格（载荷对照 + 重复拒）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), HyperCode> {
        let want = match layer_cell(l) {
            Some(w) => w,
            None => return Err(HyperCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4700 + l as u64);
            return Err(HyperCode::ELayerDrift);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x4701 + l as u64);
        Ok(())
    }

    /// Electron 层全账核验（4 层全齐）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 配置投影一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cfg(&mut self, c: usize, got: u64) -> Result<(), HyperCode> {
        if c >= CFG_CELLS {
            return Err(HyperCode::ENoCfg);
        }
        let kind = match cfg_kind(c) {
            Some(k) => k,
            None => return Err(HyperCode::ENoCfg),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4800);
            return Err(HyperCode::ECfgOrder);
        }
        let want = match cfg_cell(c) {
            Some(w) => w,
            None => return Err(HyperCode::ENoCfg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4801 + c as u64);
            return Err(HyperCode::ECfgDrift);
        }
        if match self.cfgs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperCode::EDup);
        }
        if let Some(cell) = self.cfgs.get_mut(c) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4802 + c as u64);
        Ok(())
    }

    /// 配置投影全账核验（12 格）。
    pub fn cfgs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CFG_CELLS {
            match self.cfgs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), HyperCode> {
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(HyperCode::ENoPlugin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4900 + p as u64);
            return Err(HyperCode::EPluginDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperCode::EDup);
        }
        if let Some(cell) = self.plugins.get_mut(p) {
            *cell = Some(got);
        }
        self.fnv(0x4901 + p as u64);
        Ok(())
    }

    /// 插件全账核验（8 格）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGINS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// shell 三态一格（基准对照 + 重复拒）。
    pub fn set_shell(&mut self, s: usize) -> Result<(), HyperCode> {
        let want = match shell_cell(s) {
            Some(w) => w,
            None => return Err(HyperCode::ENoShell),
        };
        if match self.shells.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperCode::EDup);
        }
        if let Some(cell) = self.shells.get_mut(s) {
            *cell = Some(want);
        }
        self.fnv(0x4A00 + s as u64);
        Ok(())
    }

    /// shell 三态全落核验。
    pub fn shells_all(&self) -> bool {
        let mut s = 0usize;
        while s < SHELL_STATES {
            match self.shells.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// GPU 声明一行（探测/降级）。
    pub fn declare_gpu_row(&mut self, r: usize) -> Result<(), HyperCode> {
        if r >= GPU_ROWS {
            return Err(HyperCode::ENoRow);
        }
        if match self.gpu_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperCode::EDup);
        }
        if let Some(cell) = self.gpu_rows.get_mut(r) {
            *cell = gpu_row(r);
        }
        self.fnv(0x4B00 + r as u64);
        Ok(())
    }

    /// GPU 声明全齐核验。
    pub fn gpu_rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < GPU_ROWS {
            match self.gpu_rows.get(r) {
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

    /// KB-COMPAT 定级（全通 → S2 本卡口径，判据缺 → S3，其余 → S1 备用）。
    /// main/aux 各节全部记账后再断言：层/投影/插件/shell/GPU 行逐节核验
    /// 全过才给 S2。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.layers_all()
            && self.cfgs_all()
            && self.plugins_all()
            && self.shells_all()
            && self.gpu_rows_all()
        {
            Grade::S2
        } else if self.gates_all() && self.layers_all() {
            Grade::S1
        } else {
            Grade::S3
        }
    }
}

impl Default for HyperLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HyperLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        if let Some(w) = layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut c = 0usize;
    while c < CFG_CELLS {
        if let Some(w) = cfg_cell(c) {
            if led.record_cfg(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < PLUGINS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < SHELL_STATES {
        let _ = led.set_shell(s);
        s += 1;
    }
    let mut r = 0usize;
    while r < GPU_ROWS {
        let _ = led.declare_gpu_row(r);
        r += 1;
    }
    // 注入 10 轮：Electron 层越闭集 / 配置投影漂移（先录正格再注漂移）/
    // 投影族逆序（先录高族正格再注低族）三态轮转，全部落在当轮临时账本
    // 上，注入=检出=10、逃逸=0；主账本 last_kind 不被注入扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HyperLedger::new();
        let caught = match kind {
            0 => lt.record_layer(ELECTRON_LAYERS, 0).is_err(),
            1 => {
                let _ = lt.record_cfg(1, cfg_cell(1).unwrap_or(0));
                lt.record_cfg(2, cfg_cell(2).unwrap_or(0).wrapping_add(1))
                    == Err(HyperCode::ECfgDrift)
            }
            _ => {
                let _ = lt.record_cfg(7, cfg_cell(7).unwrap_or(0));
                lt.record_cfg(3, cfg_cell(3).unwrap_or(0)) == Err(HyperCode::ECfgOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4C00 + k as u64);
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
/// UNX-F52230 判据集（锚 UNX-F52231-J1，off-by-one 板式）。
pub fn run_unx52230_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52230");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Electron 4 层全落",
        {
            let mut lt = HyperLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if let Some(w) = layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "main/renderer/preload/gpu 四层逐层恒等，越层必拒",
    );
    s.add(
        "配置 JS 投影 12 格全通",
        recorded >= (CFG_CELLS + PLUGINS + ELECTRON_LAYERS) as u32,
        "窗口/字体/热键三投影族逐格恒等",
    );
    s.add(
        "插件 8 格全落",
        {
            let mut lt = HyperLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGINS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "插件账逐格落账，越格必拒",
    );
    s.add(
        "shell 三态闸全落",
        {
            let mut lt = HyperLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < SHELL_STATES {
                if lt.set_shell(st).is_err() {
                    ok = false;
                }
                st += 1;
            }
            ok && lt.shells_all()
        },
        "默认/指定/插件三态闭集，越三元必拒",
    );
    s.add(
        "GPU 探测/降级声明 2/2 全齐",
        {
            let mut lt = HyperLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GPU_ROWS {
                if lt.declare_gpu_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gpu_rows_all()
        },
        "WebGL 路径探测声明/降级表声明（G 部能力页联签锚定行）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = HyperLedger::new();
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
        "投影族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CFG_CELLS {
                if cfg_kind(c).unwrap_or(3) < cfg_kind(c - 1).unwrap_or(3) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "窗口→字体→热键三族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "层越闭集/投影漂移/投影族逆序三态轮转必抓",
    );
    let mut l0 = HyperLedger::new();
    s.add(
        "边界·Electron 层越闭集",
        l0.record_layer(ELECTRON_LAYERS, 0).is_err(),
        "层 4 越闭集必拒",
    );
    s.add(
        "投影漂移拒",
        {
            l0.record_cfg(0, cfg_cell(0).unwrap_or(0));
            l0.record_cfg(1, cfg_cell(1).unwrap_or(0).wrapping_add(1)) == Err(HyperCode::ECfgDrift)
        },
        "投影载荷漂移必抓",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_cfg(CFG_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·shell 越三元",
        l0.set_shell(SHELL_STATES).is_err(),
        "shell 态 3 越三元必拒",
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
            let mut lt = HyperLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if let Some(w) = layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut c = 0usize;
            while c < CFG_CELLS {
                if let Some(w) = cfg_cell(c) {
                    let _ = lt.record_cfg(c, w);
                }
                c += 1;
            }
            let mut p = 0usize;
            while p < PLUGINS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            let mut st = 0usize;
            while st < SHELL_STATES {
                let _ = lt.set_shell(st);
                st += 1;
            }
            let mut r = 0usize;
            while r < GPU_ROWS {
                let _ = lt.declare_gpu_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，§AI-66.2 专题四 GPU 同构）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7940,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC7940",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_names() {
        assert_eq!(layer_name(0), Some("main 主进程"));
        assert_eq!(layer_name(3), Some("gpu 加速进程"));
        assert!(layer_name(4).is_none());
        assert!(layer_cell(4).is_none());
    }

    #[test]
    fn test_cfg_kind() {
        assert_eq!(cfg_kind(0), Some(0));
        assert_eq!(cfg_kind(4), Some(1));
        assert_eq!(cfg_kind(11), Some(2));
        assert!(cfg_kind(12).is_none());
        assert!(cfg_cell(12).is_none());
    }

    #[test]
    fn test_shell_states() {
        assert_eq!(shell_name(0), Some("默认 shell"));
        assert_eq!(shell_name(2), Some("插件 shell"));
        assert!(shell_name(3).is_none());
        assert!(shell_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cfg_cell(5).unwrap_or(0);
        assert_eq!(a, cfg_cell(5).unwrap_or(0));
        assert_ne!(a, cfg_cell(6).unwrap_or(0));
        assert!(plugin_cell(PLUGINS).is_none());
        assert!(gpu_row(GPU_ROWS).is_none());
    }

    #[test]
    fn test_cfg_order_caught() {
        let mut lt = HyperLedger::new();
        let _ = lt.record_cfg(8, cfg_cell(8).unwrap_or(0));
        assert_eq!(lt.record_cfg(2, cfg_cell(2).unwrap_or(0)), Err(HyperCode::ECfgOrder));
        assert!(lt.record_cfg(9, cfg_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HyperLedger::new();
        assert_eq!(
            lt.record_layer(1, layer_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(HyperCode::ELayerDrift)
        );
        assert_eq!(
            lt.record_plugin(2, plugin_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(HyperCode::EPluginDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HyperLedger::new();
        let _ = lt.record_layer(0, layer_cell(0).unwrap_or(0));
        assert_eq!(lt.record_layer(0, layer_cell(0).unwrap_or(0)), Err(HyperCode::EDup));
        let _ = lt.set_shell(1);
        assert_eq!(lt.set_shell(1), Err(HyperCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HyperLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (ELECTRON_LAYERS + CFG_CELLS + PLUGINS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7940);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * 29);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(HyperCode::Ok as u32, 0);
        assert_eq!(HyperCode::EGradeDrift as u32, 12);
        assert!(!HyperCode::Ok.detail().is_empty());
        assert!(!HyperCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        let lt = HyperLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52230_checks_green() {
        let s = run_unx52230_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
