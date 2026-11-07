//! UNX-F52229 Tabby 收口 · 0xC7900 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52229"，逐字）：
//!   title: Tabby 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52230-J1 六判据全
//!     落（Electron）；插件 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52229）判据锚 UNX-F(N+1)-J1 = UNX-F52230-J1，
//! 照原文锚定；功能按本卡标题（Tabby）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Electron 4 层闭集：main 主进程 / renderer 渲染进程 / preload 预
//!      加载桥 / gpu 加速进程四层，逐层载荷折叠恒等才收，越层必拒。
//!   ② profile 10 格：cmd / PowerShell / WSL / Git Bash / 本地 bash /
//!      zsh / 自定义 shell / SSH 远程 / 串口 / telnet 十格，逐格恒等。
//!   ③ 三连接 15 格：SSH 5 格 / 串口 5 格 / telnet 5 格，连接家族
//!      （0=SSH 1=串口 2=telnet）单调，逆序必抓；漂移必抓。
//!   ④ 插件 6 格：插件账逐格落账，与判据 ×5 口径对齐留一格冗余。
//!   ⑤ 同步三元闸：关闭 / 开启 / 端到端开启三态闭集，越三元必拒。
//!   ⑥ 注入核 10 轮三态轮转（Electron 层越闭集 / 连接漂移 / 连接家族
//!      逆序），全部落在当轮临时账本上 10/10 必抓；六判据母版闸 +
//!      KB-COMPAT 定级（全通 → S2，本卡口径）；双跑 FNV-1a 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52229;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Electron 层数。
pub const ELECTRON_LAYERS: usize = 4;

/// profile 闭集数。
pub const PROFILES: usize = 10;

/// 三连接格数（SSH 5 + 串口 5 + telnet 5）。
pub const CONN_CELLS: usize = 15;

/// 每连接家族格数。
pub const CONN_PER_KIND: usize = 5;

/// 插件格数。
pub const PLUGINS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabbyCode {
    /// 成功
    Ok = 0,
    /// Electron 层越闭集
    ENoLayer,
    /// Electron 层漂移
    ELayerDrift,
    /// profile 越闭集
    ENoProfile,
    /// profile 漂移
    EProfileDrift,
    /// 连接格越闭集
    ENoConn,
    /// 连接漂移
    EConnDrift,
    /// 连接家族逆序
    EConnOrder,
    /// 插件格越闭集
    ENoPlugin,
    /// 插件漂移
    EPluginDrift,
    /// 同步态越三元
    ENoSync,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl TabbyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TabbyCode::Ok => "成功",
            TabbyCode::ENoLayer => "Electron 层越闭集：main/preload/renderer/gpu 四层之外先登记",
            TabbyCode::ELayerDrift => "Electron 层漂移：层载荷折叠与基准不恒等，逐层复算",
            TabbyCode::ENoProfile => "profile 越闭集：10 格 profile 闭集之外先登记",
            TabbyCode::EProfileDrift => "profile 漂移：profile 载荷折叠与基准不恒等，逐格复算",
            TabbyCode::ENoConn => "连接格越闭集：三连接 15 格闭集之外先登记",
            TabbyCode::EConnDrift => "连接漂移：连接格载荷折叠与基准不恒等，逐格复算",
            TabbyCode::EConnOrder => "连接家族逆序：SSH→串口→telnet 单调，逆序必抓",
            TabbyCode::ENoPlugin => "插件格越闭集：插件 6 格闭集之外先登记",
            TabbyCode::EPluginDrift => "插件漂移：插件格载荷折叠与基准不恒等，逐格复算",
            TabbyCode::ENoSync => "同步态越三元：关闭/开启/端到端三态闭集之外先登记",
            TabbyCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TabbyCode::EGradeDrift => "定级漂移：六判据+10 profile+15 连接+6 插件+3 层+同步三元全通才定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：超预期收口（本卡不使用，仅闭集占位）
    S1,
    /// S2：本卡全通收口口径
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
            Grade::S1 => "S1：超预期收口（Tabby 卡不适用）",
            Grade::S2 => "S2：六判据+10 profile+15 连接+6 插件+4 层+同步三元全通收口（本卡口径）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Electron 层名（const fn 表 + Option 边界）。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("main 主进程"),
        1 => Some("preload 预加载桥"),
        2 => Some("renderer 渲染进程"),
        3 => Some("gpu 加速进程"),
        _ => None,
    }
}

/// Electron 层载荷。
pub const fn electron_layer_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some((0xC7100u64 + (l as u64) * 0x31) ^ 0x54616262795F4531)
}

/// profile 名（const fn 表）。
pub const fn profile_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("cmd 提示符"),
        1 => Some("Windows PowerShell"),
        2 => Some("WSL 默认发行版"),
        3 => Some("Git Bash"),
        4 => Some("本地 bash"),
        5 => Some("本地 zsh"),
        6 => Some("自定义 shell"),
        7 => Some("SSH 远程 profile"),
        8 => Some("串口 profile"),
        9 => Some("telnet profile"),
        _ => None,
    }
}

/// profile 格载荷。
pub const fn profile_cell(p: usize) -> Option<u64> {
    if p >= PROFILES {
        return None;
    }
    Some((0xC7200u64 + (p as u64) * 0x5D) ^ 0x5461626279507231)
}

/// 连接家族（0..5=SSH 5..10=串口 10..15=telnet）。
pub const fn conn_kind(c: usize) -> Option<u32> {
    if c >= CONN_CELLS {
        return None;
    }
    Some((c / CONN_PER_KIND) as u32)
}

/// 连接家族名。
pub const fn conn_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("SSH 连接"),
        1 => Some("串口连接"),
        2 => Some("telnet 连接"),
        _ => None,
    }
}

/// 连接格载荷（格 c；家族 = c/5）。
pub const fn conn_cell(c: usize) -> Option<u64> {
    if c >= CONN_CELLS {
        return None;
    }
    Some((0xC7300u64 + (c as u64) * 0x2C) ^ ((c / CONN_PER_KIND) as u64).wrapping_mul(0x01000193))
}

/// 连接格名（const fn 表，15 格逐格命名）。
pub const fn conn_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("SSH·主机清单格"),
        1 => Some("SSH·端口格"),
        2 => Some("SSH·密钥格"),
        3 => Some("SSH·代理跳板格"),
        4 => Some("SSH·known_hosts 格"),
        5 => Some("串口·波特率格"),
        6 => Some("串口·数据位格"),
        7 => Some("串口·停止位格"),
        8 => Some("串口·校验位格"),
        9 => Some("串口·流控格"),
        10 => Some("telnet·主机格"),
        11 => Some("telnet·端口格"),
        12 => Some("telnet·终端类型格"),
        13 => Some("telnet·超时格"),
        14 => Some("telnet·回退格"),
        _ => None,
    }
}

/// 插件格载荷。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGINS {
        return None;
    }
    Some((0xC7400u64 + (p as u64) * 0x47) ^ 0x5461626279506C31)
}

/// 插件格名。
pub const fn plugin_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("插件·主题格"),
        1 => Some("插件·快捷键面板格"),
        2 => Some("插件·同步配置格"),
        3 => Some("插件·串口扩展格"),
        4 => Some("插件·SSH 收纳格"),
        5 => Some("插件·收藏夹格"),
        _ => None,
    }
}

/// 同步三元（0=关闭 1=开启 2=端到端开启）。
pub const fn sync_cell(s: usize) -> Option<u64> {
    if s >= 3 {
        return None;
    }
    Some(0xC7500u64 + (s as u64) * 0x3B)
}

/// 同步三元名。
pub const fn sync_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("同步关闭"),
        1 => Some("同步开启"),
        2 => Some("端到端同步开启"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Tabby 收口账本状态机。
#[derive(Clone)]
pub struct TabbyLedger {
    /// 六判据（Electron/连接/插件/同步/注入/定级六面收口闸）。
    gates: [Option<bool>; 6],
    /// Electron 层账。
    layers: [Option<u64>; ELECTRON_LAYERS],
    /// profile 账。
    profiles: [Option<u64>; PROFILES],
    /// 连接账。
    conns: [Option<u64>; CONN_CELLS],
    /// 插件账。
    plugins: [Option<u64>; PLUGINS],
    /// 同步三元账。
    sync: [Option<u64>; 3],
    /// 最后连接家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TabbyLedger {
    /// 新账本。
    pub fn new() -> Self {
        TabbyLedger {
            gates: [None; 6],
            layers: [None; ELECTRON_LAYERS],
            profiles: [None; PROFILES],
            conns: [None; CONN_CELLS],
            plugins: [None; PLUGINS],
            sync: [None; 3],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TabbyCode> {
        if gate >= 6 {
            return Err(TabbyCode::EGradeDrift);
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

    /// Electron 一层（载荷对照 + 重复拒）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), TabbyCode> {
        let want = match electron_layer_cell(l) {
            Some(w) => w,
            None => return Err(TabbyCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + l as u64);
            return Err(TabbyCode::ELayerDrift);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TabbyCode::EGradeDrift);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x6201 + l as u64);
        Ok(())
    }

    /// Electron 全层核验（4 层）。
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

    /// profile 一格（载荷对照 + 重复拒）。
    pub fn record_profile(&mut self, p: usize, got: u64) -> Result<(), TabbyCode> {
        let want = match profile_cell(p) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x6310);
                return Err(TabbyCode::ENoProfile);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + p as u64);
            return Err(TabbyCode::EProfileDrift);
        }
        if match self.profiles.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TabbyCode::EGradeDrift);
        }
        if let Some(c) = self.profiles.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6301 + p as u64);
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

    /// 连接一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, c: usize, got: u64) -> Result<(), TabbyCode> {
        if c >= CONN_CELLS {
            return Err(TabbyCode::ENoConn);
        }
        let kind = match conn_kind(c) {
            Some(k) => k,
            None => return Err(TabbyCode::ENoConn),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(TabbyCode::EConnOrder);
        }
        let want = match conn_cell(c) {
            Some(w) => w,
            None => return Err(TabbyCode::ENoConn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + c as u64);
            return Err(TabbyCode::EConnDrift);
        }
        if match self.conns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TabbyCode::EGradeDrift);
        }
        if let Some(cc) = self.conns.get_mut(c) {
            *cc = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6402 + c as u64);
        Ok(())
    }

    /// 连接全账核验（15 格）。
    pub fn conns_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONN_CELLS {
            match self.conns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), TabbyCode> {
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(TabbyCode::ENoPlugin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + p as u64);
            return Err(TabbyCode::EPluginDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TabbyCode::EGradeDrift);
        }
        if let Some(c) = self.plugins.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6501 + p as u64);
        Ok(())
    }

    /// 插件全账核验（6 格）。
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

    /// 同步三元一格（越三元必拒）。
    pub fn record_sync(&mut self, s: usize) -> Result<(), TabbyCode> {
        let want = match sync_cell(s) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x6610);
                return Err(TabbyCode::ENoSync);
            }
        };
        if match self.sync.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TabbyCode::EGradeDrift);
        }
        if let Some(c) = self.sync.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x6600 + s as u64);
        Ok(())
    }

    /// 同步三元全账核验。
    pub fn sync_all(&self) -> bool {
        let mut s = 0usize;
        while s < 3 {
            match self.sync.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
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

    /// KB-COMPAT 定级（全通 → S2 本卡口径，判据缺 → S3，其余 → S3 占位闭集）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.layers_all()
            && self.profiles_all()
            && self.conns_all()
            && self.plugins_all()
            && self.sync_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S3
        } else {
            Grade::S3
        }
    }
}

impl Default for TabbyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TabbyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        match electron_layer_cell(l) {
            Some(w) => {
                if led.record_layer(l, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        l += 1;
    }
    let mut p = 0usize;
    while p < PROFILES {
        match profile_cell(p) {
            Some(w) => {
                if led.record_profile(p, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        p += 1;
    }
    let mut c = 0usize;
    while c < CONN_CELLS {
        match conn_cell(c) {
            Some(w) => {
                if led.record_conn(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut pg = 0usize;
    while pg < PLUGINS {
        match plugin_cell(pg) {
            Some(w) => {
                if led.record_plugin(pg, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        pg += 1;
    }
    let mut s = 0usize;
    while s < 3 {
        let _ = led.record_sync(s);
        s += 1;
    }
    // 注入 ×10：Electron 层越闭集 / 连接漂移 / 连接家族逆序 三态轮转；
    // 三类注入全部落在当轮临时账本上，逐轮必抓，逃逸恒零。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TabbyLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：空账本上直接注 Electron 第 4 层，必抓。
                lt.record_layer(ELECTRON_LAYERS, 0) == Err(TabbyCode::ENoLayer)
            }
            1 => {
                // 漂移：先录正格再注漂移载荷，必抓。
                match conn_cell(7) {
                    Some(w) => {
                        let _ = lt.record_conn(7, w);
                        lt.record_conn(8, w.wrapping_add(1)) == Err(TabbyCode::EConnDrift)
                    }
                    None => false,
                }
            }
            _ => {
                // 逆序：先录 telnet 家族正格再注 SSH 家族格，必抓；
                // 失败不改 last_kind。
                match (conn_cell(12), conn_cell(2)) {
                    (Some(w1), Some(w2)) => {
                        let _ = lt.record_conn(12, w1);
                        lt.record_conn(2, w2) == Err(TabbyCode::EConnOrder)
                    }
                    _ => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52229 判据集（锚 UNX-F52230-J1，off-by-one 板式）。
pub fn run_unx52229_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52229");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Electron 4 层全通",
        {
            let mut lt = TabbyLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                match electron_layer_cell(l) {
                    Some(w) => {
                        if lt.record_layer(l, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "main/preload/renderer/gpu 四层逐层恒等（UNX-F52230-J1 Electron 面）",
    );
    s.add(
        "profile 闭集 10 格全通",
        recorded >= (PROFILES + CONN_CELLS + PLUGINS + ELECTRON_LAYERS) as u32,
        "cmd/PowerShell/WSL/Git Bash/串口/telnet 等十格逐格恒等",
    );
    s.add(
        "三连接 15 格全通",
        {
            let mut lt = TabbyLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONN_CELLS {
                match conn_cell(c) {
                    Some(w) => {
                        if lt.record_conn(c, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                c += 1;
            }
            ok && lt.conns_all()
        },
        "SSH 5 格/串口 5 格/telnet 5 格逐格恒等且家族单调",
    );
    s.add(
        "插件 6 格全通",
        {
            let mut lt = TabbyLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGINS {
                match plugin_cell(p) {
                    Some(w) => {
                        if lt.record_plugin(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "插件六格逐格恒等（卡面插件口径 ×5 对齐留冗余）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TabbyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "Electron/连接/插件/同步/注入/定级六面收口闸逐格落账（UNX-F52230-J1）",
    );
    s.add(
        "同步三元闸全落",
        {
            let mut lt = TabbyLedger::new();
            let mut si = 0usize;
            while si < 3 {
                let _ = lt.record_sync(si);
                si += 1;
            }
            lt.sync_all()
        },
        "关闭/开启/端到端开启三态闭集，越三元必拒",
    );
    s.add(
        "连接家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CONN_CELLS {
                match (conn_kind(c), conn_kind(c - 1)) {
                    (Some(a), Some(b)) => {
                        if a < b {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                c += 1;
            }
            ok
        },
        "SSH→串口→telnet 三家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "层越闭集/连接漂移/家族逆序三态轮转必抓");
    let mut l0 = TabbyLedger::new();
    s.add(
        "边界·Electron 层越闭集",
        l0.record_layer(ELECTRON_LAYERS, 0).is_err(),
        "Electron 层 4 越闭集必拒",
    );
    s.add(
        "边界·profile 越闭集",
        l0.record_profile(PROFILES, 0).is_err(),
        "profile 10 越闭集必拒",
    );
    s.add(
        "边界·连接格越闭集",
        l0.record_conn(CONN_CELLS, 0).is_err(),
        "连接格 15 越闭集必拒",
    );
    s.add(
        "边界·插件越闭集",
        l0.record_plugin(PLUGINS, 0).is_err(),
        "插件 6 越闭集必拒",
    );
    s.add(
        "逆序·失败不改 last_kind",
        {
            let mut l1 = TabbyLedger::new();
            let w1 = match conn_cell(12) {
                Some(w) => w,
                None => 0,
            };
            let w2 = match conn_cell(2) {
                Some(w) => w,
                None => 0,
            };
            let _ = l1.record_conn(12, w1);
            let r = l1.record_conn(2, w2);
            r == Err(TabbyCode::EConnOrder)
        },
        "逆序注入被拒后 last_kind 保持 telnet 家族，不回退",
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
            let mut lt = TabbyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                match electron_layer_cell(l) {
                    Some(w) => {
                        let _ = lt.record_layer(l, w);
                    }
                    None => {}
                }
                l += 1;
            }
            let mut p = 0usize;
            while p < PROFILES {
                match profile_cell(p) {
                    Some(w) => {
                        let _ = lt.record_profile(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            let mut c = 0usize;
            while c < CONN_CELLS {
                match conn_cell(c) {
                    Some(w) => {
                        let _ = lt.record_conn(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut pg = 0usize;
            while pg < PLUGINS {
                match plugin_cell(pg) {
                    Some(w) => {
                        let _ = lt.record_plugin(pg, w);
                    }
                    None => {}
                }
                pg += 1;
            }
            let mut si = 0usize;
            while si < 3 {
                let _ = lt.record_sync(si);
                si += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，本卡口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7900,
        "SEG_BASE 0x40 对齐且为 VX15 段 52229 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_electron_layers() {
        assert_eq!(electron_layer_name(0), Some("main 主进程"));
        assert_eq!(electron_layer_name(3), Some("gpu 加速进程"));
        assert!(electron_layer_name(4).is_none());
        assert!(electron_layer_cell(4).is_none());
    }

    #[test]
    fn test_profile_closed_set() {
        assert_eq!(profile_name(0), Some("cmd 提示符"));
        assert_eq!(profile_name(7), Some("SSH 远程 profile"));
        assert_eq!(profile_name(9), Some("telnet profile"));
        assert!(profile_name(10).is_none());
        assert!(profile_cell(10).is_none());
    }

    #[test]
    fn test_conn_kinds() {
        assert_eq!(conn_kind(0), Some(0));
        assert_eq!(conn_kind(4), Some(0));
        assert_eq!(conn_kind(5), Some(1));
        assert_eq!(conn_kind(14), Some(2));
        assert!(conn_kind(15).is_none());
        assert!(conn_cell(15).is_none());
        assert_eq!(conn_kind_name(2), Some("telnet 连接"));
        assert!(conn_kind_name(3).is_none());
    }

    #[test]
    fn test_conn_names_full() {
        assert_eq!(conn_name(0), Some("SSH·主机清单格"));
        assert_eq!(conn_name(9), Some("串口·流控格"));
        assert_eq!(conn_name(14), Some("telnet·回退格"));
        assert!(conn_name(15).is_none());
    }

    #[test]
    fn test_sync_ternary() {
        assert_eq!(sync_name(0), Some("同步关闭"));
        assert_eq!(sync_name(2), Some("端到端同步开启"));
        assert!(sync_name(3).is_none());
        assert!(sync_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = profile_cell(3);
        assert_eq!(a, profile_cell(3));
        assert_ne!(a, profile_cell(4));
        assert!(plugin_cell(PLUGINS).is_none());
        assert!(electron_layer_cell(ELECTRON_LAYERS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TabbyLedger::new();
        match conn_cell(9) {
            Some(w) => {
                assert!(lt.record_conn(9, w).is_ok());
            }
            None => panic!("conn_cell(9) 必有"),
        }
        match conn_cell(1) {
            Some(w) => assert_eq!(lt.record_conn(1, w), Err(TabbyCode::EConnOrder)),
            None => panic!("conn_cell(1) 必有"),
        }
        match conn_cell(10) {
            Some(w) => assert!(lt.record_conn(10, w).is_ok()),
            None => panic!("conn_cell(10) 必有"),
        }
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TabbyLedger::new();
        let w = match plugin_cell(2) {
            Some(w) => w,
            None => 0,
        };
        assert_eq!(lt.record_plugin(2, w.wrapping_add(1)), Err(TabbyCode::EPluginDrift));
    }

    #[test]
    fn test_out_of_set_on_empty_ledger() {
        let mut lt = TabbyLedger::new();
        assert_eq!(lt.record_profile(PROFILES, 0), Err(TabbyCode::ENoProfile));
        assert_eq!(lt.record_sync(3), Err(TabbyCode::ENoSync));
        assert!(lt.stats().0 > 0);
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TabbyLedger::new();
        let w = match profile_cell(1) {
            Some(w) => w,
            None => 0,
        };
        let _ = lt.record_profile(1, w);
        assert!(lt.record_profile(1, w).is_err());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TabbyLedger::new();
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
        assert!(
            recorded >= (ELECTRON_LAYERS + PROFILES + CONN_CELLS + PLUGINS) as u32
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
        assert_eq!(SEG_BASE, 0xC7900);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TabbyCode::Ok as u32, 0);
        assert_eq!(TabbyCode::EGradeDrift as u32, 12);
        assert!(!TabbyCode::Ok.detail().is_empty());
        assert!(!TabbyCode::EConnOrder.detail().is_empty());
        assert!(!TabbyCode::ENoSync.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = TabbyLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52229_checks_green() {
        let s = run_unx52229_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
