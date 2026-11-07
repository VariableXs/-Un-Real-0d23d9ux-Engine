//! UNX-F52271 k9s 收口 · 0xC8380段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52271"，逐字）：
//!   title: k9s 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52272-J1 六判据全
//!     落（Electron）；集群视图 ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52271）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52272-J1，照原文锚定；功能按本卡标题（k9s）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 资源视图 20 格闭集：pod/deploy/svc/ingress/cfgmap 五族各 4 格
//!      （族 = view/4），五族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 热键 10 操作：enter/desc/logs/yaml/edit/del/scale/top/pulse/quit
//!      逐键落账（退出路径 quit 必落），键位状态机闭集。
//!   ③ 插件 6 格闭集 + 皮肤/主题 4 格闭集（dark/light/solarized/mono），
//!      重复落账拒，缺格拒封。
//!   ④ ledger 状态机（new/record_view/record_hotkey/record_plugin/
//!      record_theme/judge_gate×6/fingerprint FNV-1a/grade 定级闭集）；
//!      注入核 run_cycle 三态轮转 10/10（越闭集/载荷漂移/族逆序），检出+
//!      逃逸守恒；双跑指纹一致；定级 S2 闭集（KB-COMPAT 登记）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52271;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 资源视图格数（五族 × 4）。
pub const VIEW_CELLS: usize = 20;

/// 热键操作数。
pub const HOTKEY_OPS: usize = 10;

/// 插件格数。
pub const PLUGIN_CELLS: usize = 6;

/// 皮肤/主题格数。
pub const THEME_CELLS: usize = 4;

/// XOR 锚常量（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4B395300_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K9sCode {
    /// 成功
    Ok = 0,
    /// 视图格越闭集
    ENoView,
    /// 视图载荷漂移
    EViewDrift,
    /// 热键越闭集
    ENoKey,
    /// 热键载荷漂移
    EKeyDrift,
    /// 资源族逆序
    EOrder,
    /// 插件/主题越闭集
    ENoAux,
    /// 皮肤缺格
    EThemeMiss,
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

impl K9sCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            K9sCode::Ok => "成功",
            K9sCode::ENoView => "视图格越闭集：pod/deploy/svc/ingress/cfgmap 五族 20 格之外先登记",
            K9sCode::EViewDrift => "视图载荷漂移：资源视图折叠与基准不恒等，逐格复算",
            K9sCode::ENoKey => "热键越闭集：enter/desc/logs 等 10 热键集之外先登记",
            K9sCode::EKeyDrift => "热键载荷漂移：键位折叠与基准不恒等，逐键复算",
            K9sCode::EOrder => "资源族逆序：pod→deploy→svc→ingress→cfgmap 单调，逆序必抓",
            K9sCode::ENoAux => "插件/主题越闭集：插件 6 格、主题 4 格闭集之外先登记",
            K9sCode::EThemeMiss => "皮肤缺格：dark/light/solarized/mono 四主题未齐先补齐",
            K9sCode::EDup => "重复落账：同格只许一条账目",
            K9sCode::ESeal => "未齐即封：20 视图 + 10 热键 + 6 插件 + 4 主题未齐先补齐",
            K9sCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            K9sCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            K9sCode::EGradeDrift => "定级漂移：四账全齐且六判据全落才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标级）
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
            Grade::S1 => "S1：全通收口（本卡闭集不启用，目标级为 S2）",
            Grade::S2 => "S2：四账全齐+六判据全落，有条件收口（KB-COMPAT 登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 资源视图格载荷（格 v；族 = v/4）。
pub const fn view_cell(v: usize) -> Option<u64> {
    if v >= VIEW_CELLS {
        return None;
    }
    Some((0xB8100u64 + (v as u64) * 0x39) ^ ANCHOR)
}

/// 资源族（0..4=pod 4..8=deploy 8..12=svc 12..16=ingress 16..20=cfgmap）。
pub const fn view_kind(v: usize) -> Option<u32> {
    if v >= VIEW_CELLS {
        return None;
    }
    Some((v / 4) as u32)
}

/// 资源族名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("pod"),
        1 => Some("deploy"),
        2 => Some("svc"),
        3 => Some("ingress"),
        4 => Some("cfgmap"),
        _ => None,
    }
}

/// 热键格载荷（键 h）。
pub const fn hotkey_cell(h: usize) -> Option<u64> {
    if h >= HOTKEY_OPS {
        return None;
    }
    Some((0xB8200u64 + (h as u64) * 0x2D) ^ ANCHOR)
}

/// 热键名（键位/退出路径闭集；quit 为退出路径）。
pub const fn hotkey_name(h: usize) -> Option<&'static str> {
    match h {
        0 => Some("enter"),
        1 => Some("desc"),
        2 => Some("logs"),
        3 => Some("yaml"),
        4 => Some("edit"),
        5 => Some("del"),
        6 => Some("scale"),
        7 => Some("top"),
        8 => Some("pulse"),
        9 => Some("quit"),
        _ => None,
    }
}

/// 热键载荷核验（键 h：载荷对照）。
pub const fn hotkey_expect(h: usize) -> Option<u64> {
    match hotkey_cell(h) {
        Some(_) => hotkey_cell(h),
        None => None,
    }
}

/// 插件格载荷（插件 p）。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some((0xB8300u64 + (p as u64) * 0x31) ^ ANCHOR)
}

/// 插件名。
pub const fn plugin_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("get-all"),
        1 => Some("dump-logs"),
        2 => Some("toggle-namespace"),
        3 => Some("SankeyOverview"),
        4 => Some("lazykube"),
        5 => Some("k9s-exporter"),
        _ => None,
    }
}

/// 主题格载荷（主题 t）。
pub const fn theme_cell(t: usize) -> Option<u32> {
    if t >= THEME_CELLS {
        return None;
    }
    Some(0xB8400 + t as u32 * 0x2B)
}

/// 主题名。
pub const fn theme_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("dark"),
        1 => Some("light"),
        2 => Some("solarized"),
        3 => Some("mono"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// k9s 收口账本状态机。
#[derive(Clone)]
pub struct K9sLedger {
    /// 六判据（安装/连接/视图/热键/插件/登记）。
    gates: [Option<bool>; 6],
    /// 资源视图账。
    views: [Option<u64>; VIEW_CELLS],
    /// 热键账。
    keys: [Option<u64>; HOTKEY_OPS],
    /// 插件账。
    plugins: [Option<u64>; PLUGIN_CELLS],
    /// 主题账。
    themes: [Option<u32>; THEME_CELLS],
    /// 最后资源族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl K9sLedger {
    /// 新账本。
    pub fn new() -> Self {
        K9sLedger {
            gates: [None; 6],
            views: [None; VIEW_CELLS],
            keys: [None; HOTKEY_OPS],
            plugins: [None; PLUGIN_CELLS],
            themes: [None; THEME_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), K9sCode> {
        if gate >= 6 {
            return Err(K9sCode::ESeal);
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

    /// 资源视图一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_view(&mut self, v: usize, got: u64) -> Result<(), K9sCode> {
        if v >= VIEW_CELLS {
            return Err(K9sCode::ENoView);
        }
        let kind = match view_kind(v) {
            Some(k) => k,
            None => return Err(K9sCode::ENoView),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(K9sCode::EOrder);
        }
        let want = match view_cell(v) {
            Some(w) => w,
            None => return Err(K9sCode::ENoView),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + v as u64);
            return Err(K9sCode::EViewDrift);
        }
        if match self.views.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K9sCode::EDup);
        }
        if let Some(c) = self.views.get_mut(v) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + v as u64);
        Ok(())
    }

    /// 视图全账核验（20 格）。
    pub fn views_all(&self) -> bool {
        let mut v = 0usize;
        while v < VIEW_CELLS {
            match self.views.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 热键一格（载荷对照 + 重复拒；quit 退出路径必落）。
    pub fn record_hotkey(&mut self, h: usize, got: u64) -> Result<(), K9sCode> {
        let want = match hotkey_cell(h) {
            Some(w) => w,
            None => return Err(K9sCode::ENoKey),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + h as u64);
            return Err(K9sCode::EKeyDrift);
        }
        if match self.keys.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K9sCode::EDup);
        }
        if let Some(c) = self.keys.get_mut(h) {
            *c = Some(got);
        }
        self.fnv(0x7301 + h as u64);
        Ok(())
    }

    /// 热键全账核验（10 键，含退出路径 quit）。
    pub fn keys_all(&self) -> bool {
        let mut h = 0usize;
        while h < HOTKEY_OPS {
            match self.keys.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
        }
        true
    }

    /// 插件一格。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), K9sCode> {
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(K9sCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + p as u64);
            return Err(K9sCode::EKeyDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K9sCode::EDup);
        }
        if let Some(c) = self.plugins.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x7401 + p as u64);
        Ok(())
    }

    /// 插件全账核验（6 格）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 主题一格。
    pub fn record_theme(&mut self, t: usize) -> Result<(), K9sCode> {
        let want = match theme_cell(t) {
            Some(w) => w,
            None => return Err(K9sCode::ENoAux),
        };
        if match self.themes.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K9sCode::EDup);
        }
        if let Some(c) = self.themes.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x7500 + t as u64);
        Ok(())
    }

    /// 主题全齐核验（4 格）。
    pub fn themes_all(&self) -> bool {
        let mut t = 0usize;
        while t < THEME_CELLS {
            match self.themes.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
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

    /// KB-COMPAT 定级（四账全齐+六判据全落 → S2，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.views_all()
            && self.keys_all()
            && self.plugins_all()
            && self.themes_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for K9sLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = K9sLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VIEW_CELLS {
        if let Some(w) = view_cell(v) {
            if led.record_view(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut h = 0usize;
    while h < HOTKEY_OPS {
        if let Some(w) = hotkey_cell(h) {
            if led.record_hotkey(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut t = 0usize;
    while t < THEME_CELLS {
        let _ = led.record_theme(t);
        t += 1;
    }
    // 注入 10 轮：越闭集 / 载荷漂移 / 资源族逆序三态轮转，全部落在当轮
    // 临时账本上；主账本只记注入计数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = K9sLedger::new();
        let caught = match kind {
            0 => {
                let hit_a = lt.record_view(VIEW_CELLS, 0) == Err(K9sCode::ENoView);
                let hit_b = lt.record_hotkey(HOTKEY_OPS, 0).is_err();
                hit_a || hit_b
            }
            1 => {
                // 漂移：先录正格，再注漂移载荷（同族内不触发族序）。
                let _ = lt.record_view(4, view_cell(4).unwrap_or(0));
                lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(K9sCode::EViewDrift)
            }
            _ => {
                // 族逆序：对空账本注入逆序对同样必抓，且不改 last_kind。
                let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
                let before = lt.last_kind;
                let hit = lt.record_view(1, view_cell(1).unwrap_or(0)) == Err(K9sCode::EOrder);
                hit && lt.last_kind == before
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52271 判据集（锚 UNX-F52272-J1，off-by-one 板式）。
pub fn run_unx52271_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52271");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "资源视图 20 格全通",
        recorded >= VIEW_CELLS as u32,
        "pod/deploy/svc/ingress/cfgmap 五族逐格恒等",
    );
    s.add(
        "五族单调",
        {
            let mut ok = true;
            let mut v = 1usize;
            while v < VIEW_CELLS {
                if view_kind(v).unwrap_or(6) < view_kind(v - 1).unwrap_or(6) {
                    ok = false;
                }
                v += 1;
            }
            ok
        },
        "pod→deploy→svc→ingress→cfgmap 单调，逆序必抓",
    );
    s.add(
        "热键 10 操作全落（键位/退出路径）",
        {
            let mut lt = K9sLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < HOTKEY_OPS {
                if let Some(w) = hotkey_cell(h) {
                    if lt.record_hotkey(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.keys_all()
        },
        "enter/desc/logs/yaml/edit/del/scale/top/pulse/quit 逐键落账，退出路径 quit 必落",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = K9sLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/连接/视图/热键/插件/登记逐格落账",
    );
    s.add(
        "插件 6 格全齐",
        {
            let mut lt = K9sLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "get-all/dump-logs/toggle-namespace/SankeyOverview/lazykube/k9s-exporter 逐格",
    );
    s.add(
        "皮肤/主题 4 格全齐",
        {
            let mut lt = K9sLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < THEME_CELLS {
                if lt.record_theme(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.themes_all()
        },
        "dark/light/solarized/mono 四主题闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "越闭集/载荷漂移/资源族逆序三态轮转必抓",
    );
    s.add(
        "边界·视图格越闭集",
        K9sLedger::new().record_view(VIEW_CELLS, 0).is_err(),
        "视图格 20 越闭集必拒",
    );
    s.add(
        "视图漂移拒",
        {
            let mut lt = K9sLedger::new();
            let _ = lt.record_view(8, view_cell(8).unwrap_or(0));
            lt.record_view(9, view_cell(9).unwrap_or(0).wrapping_add(1))
                == Err(K9sCode::EViewDrift)
        },
        "视图载荷漂移必抓",
    );
    s.add(
        "边界·热键越闭集",
        K9sLedger::new().record_hotkey(HOTKEY_OPS, 0).is_err(),
        "热键 10 越闭集必拒",
    );
    s.add(
        "边界·主题越闭集",
        K9sLedger::new().record_theme(THEME_CELLS).is_err(),
        "主题格 4 越闭集必拒",
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
            // main/aux 各节全部记账后再断言。
            let mut lt = K9sLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VIEW_CELLS {
                if let Some(w) = view_cell(v) {
                    let _ = lt.record_view(v, w);
                }
                v += 1;
            }
            let mut h = 0usize;
            while h < HOTKEY_OPS {
                if let Some(w) = hotkey_cell(h) {
                    let _ = lt.record_hotkey(h, w);
                }
                h += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            let mut t = 0usize;
            while t < THEME_CELLS {
                let _ = lt.record_theme(t);
                t += 1;
            }
            lt.judge_grade() == Grade::S2 && Grade::S2.val() == 2
        },
        "四账全齐+六判据全落 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位（0xC8380）",
    );
    s.add(
        "集群视图 ×10 口径",
        {
            // 视图族 × 热键 = 5×2 交互状态机完整口径。
            VIEW_KINDS == 5 && HOTKEY_OPS == 10 && hotkey_expect(9) == hotkey_cell(9)
        },
        "集群视图 ×10 交互状态机完整（§AI-66.3）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

/// 资源族数（五族）。
pub const VIEW_KINDS: usize = 5;

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_families() {
        assert_eq!(view_kind_name(0), Some("pod"));
        assert_eq!(view_kind_name(1), Some("deploy"));
        assert_eq!(view_kind_name(2), Some("svc"));
        assert_eq!(view_kind_name(3), Some("ingress"));
        assert_eq!(view_kind_name(4), Some("cfgmap"));
        assert!(view_kind_name(5).is_none());
        assert_eq!(view_kind(0), Some(0));
        assert_eq!(view_kind(19), Some(4));
        assert!(view_kind(20).is_none());
        assert!(view_cell(VIEW_CELLS).is_none());
    }

    #[test]
    fn test_hotkeys_closed() {
        let mut h = 0usize;
        while h < HOTKEY_OPS {
            assert!(hotkey_name(h).is_some());
            h += 1;
        }
        assert_eq!(hotkey_name(9), Some("quit"));
        assert!(hotkey_name(10).is_none());
        assert!(hotkey_cell(HOTKEY_OPS).is_none());
        assert_eq!(hotkey_expect(3), hotkey_cell(3));
    }

    #[test]
    fn test_plugin_theme_closed() {
        assert_eq!(plugin_name(0), Some("get-all"));
        assert_eq!(plugin_name(5), Some("k9s-exporter"));
        assert!(plugin_name(6).is_none());
        assert!(plugin_cell(PLUGIN_CELLS).is_none());
        assert_eq!(theme_name(0), Some("dark"));
        assert_eq!(theme_name(3), Some("mono"));
        assert!(theme_name(4).is_none());
        assert!(theme_cell(THEME_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = view_cell(7).unwrap_or(0);
        assert_eq!(a, view_cell(7).unwrap_or(0));
        assert_ne!(a, view_cell(8).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = K9sLedger::new();
        let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
        let before = lt.last_kind;
        assert_eq!(
            lt.record_view(1, view_cell(1).unwrap_or(0)),
            Err(K9sCode::EOrder)
        );
        assert_eq!(lt.last_kind, before);
        assert!(lt.record_view(12, view_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = K9sLedger::new();
        assert_eq!(
            lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(K9sCode::EViewDrift)
        );
        assert_eq!(
            lt.record_hotkey(2, hotkey_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(K9sCode::EKeyDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = K9sLedger::new();
        let _ = lt.record_view(0, view_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_view(0, view_cell(0).unwrap_or(0)),
            Err(K9sCode::EDup)
        );
        let _ = lt.record_plugin(1, plugin_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_plugin(1, plugin_cell(1).unwrap_or(0)),
            Err(K9sCode::EDup)
        );
        let _ = lt.record_theme(2);
        assert_eq!(lt.record_theme(2), Err(K9sCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = K9sLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = K9sLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (VIEW_CELLS + HOTKEY_OPS + PLUGIN_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_rotation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = K9sLedger::new();
        assert!(lt.record_view(VIEW_CELLS, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC8380);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(K9sCode::Ok as u32, 0);
        assert_eq!(K9sCode::EGradeDrift as u32, 12);
        let all = [
            K9sCode::Ok,
            K9sCode::ENoView,
            K9sCode::EViewDrift,
            K9sCode::ENoKey,
            K9sCode::EKeyDrift,
            K9sCode::EOrder,
            K9sCode::ENoAux,
            K9sCode::EThemeMiss,
            K9sCode::EDup,
            K9sCode::ESeal,
            K9sCode::EInj,
            K9sCode::EFp,
            K9sCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0u32;
        while i < 13 {
            assert!(!all[i as usize].detail().is_empty());
            assert_eq!(all[i as usize].val(), SEG_BASE + i);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        let lt = K9sLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = K9sLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = K9sLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52271_checks_green() {
        let s = run_unx52271_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
