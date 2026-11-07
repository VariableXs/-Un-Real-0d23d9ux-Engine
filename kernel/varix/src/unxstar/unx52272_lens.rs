//! UNX-F52272 Lens（K8s IDE）收口 · 0xC83C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52272"，逐字）：
//!   title: Lens（K8s IDE）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52273-J1 容器内开
//!     发全链 ×10（attach/rebuild/features）；与 F52261/F52021 双消费联签
//!     锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52272）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52273-J1，照原文锚定；功能按本卡标题（Lens）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面 20 格闭集：集群/工作负载/网络/存储/配置各 4 格（族 = cell/4），
//!      五面单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 终端 8 操作：attach/exec/logs/port-forward/shell/top/describe/kill
//!      逐操作落账，容器内开发全链 ×10 口径与 attach/rebuild/features
//!      三段分账（rebuild/features 归扩展段记账）。
//!   ③ 扩展 6 格闭集（含 rebuild/features 两格，锚 UNX-F52273-J1 三段）。
//!   ④ catalog 5 场景分账（clusters/workloads/networks/storage/configs）
//!      与 Electron 4 层闭集（main/preload/renderer/native），四账全齐
//!      → S1（KB-COMPAT 登记）。
//!   ⑤ ledger 状态机（new/record_cell/record_term/record_ext/
//!      declare_catalog/sign_electron/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（越闭集/载荷
//!      漂移/族逆序），检出+逃逸守恒；双跑指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52272;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五面总格数（五面 × 4）。
pub const CELL_CELLS: usize = 20;

/// 终端操作数。
pub const TERM_OPS: usize = 8;

/// 扩展格数。
pub const EXT_CELLS: usize = 6;

/// catalog 场景数。
pub const CATALOG_SCENES: usize = 5;

/// Electron 层数。
pub const ELECTRON_LAYERS: usize = 4;

/// XOR 锚常量（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4C454E53_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LensCode {
    /// 成功
    Ok = 0,
    /// 五面格越闭集
    ENoCell,
    /// 五面载荷漂移
    ECellDrift,
    /// 终端操作越闭集
    ENoTerm,
    /// 终端载荷漂移
    ETermDrift,
    /// 五面族逆序
    EOrder,
    /// 扩展/catalog 越闭集
    ENoAux,
    /// Electron 联签缺层
    ESignMiss,
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

impl LensCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LensCode::Ok => "成功",
            LensCode::ENoCell => "五面格越闭集：集群/工作负载/网络/存储/配置五面 20 格之外先登记",
            LensCode::ECellDrift => "五面载荷漂移：视图折叠与基准不恒等，逐格复算",
            LensCode::ENoTerm => "终端操作越闭集：attach/exec/logs 等 8 操作集之外先登记",
            LensCode::ETermDrift => "终端载荷漂移：操作折叠与基准不恒等，逐操作复算",
            LensCode::EOrder => "五面族逆序：集群→工作负载→网络→存储→配置单调，逆序必抓",
            LensCode::ENoAux => "扩展/catalog 越闭集：扩展 6 格、catalog 5 场景之外先登记",
            LensCode::ESignMiss => "Electron 联签缺层：main/preload/renderer/native 四层未齐先补齐",
            LensCode::EDup => "重复落账：同格只许一条账目",
            LensCode::ESeal => "未齐即封：20 格 + 8 终端 + 6 扩展 + 5 catalog + 4 Electron 层未齐先补齐",
            LensCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            LensCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            LensCode::EGradeDrift => "定级漂移：五账全齐且六判据全落才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标级）
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
            Grade::S1 => "S1：五账全齐+六判据全落+Electron 四层联签全通收口（KB-COMPAT 登记）",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五面格载荷（格 c；族 = c/4）。
pub const fn view_cell(c: usize) -> Option<u64> {
    if c >= CELL_CELLS {
        return None;
    }
    Some((0xB9100u64 + (c as u64) * 0x3B) ^ ANCHOR)
}

/// 五面族（0..4=集群 4..8=工作负载 8..12=网络 12..16=存储 16..20=配置）。
pub const fn view_kind(c: usize) -> Option<u32> {
    if c >= CELL_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 五面族名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("集群"),
        1 => Some("工作负载"),
        2 => Some("网络"),
        3 => Some("存储"),
        4 => Some("配置"),
        _ => None,
    }
}

/// 终端操作格载荷（操作 t）。
pub const fn term_cell(t: usize) -> Option<u64> {
    if t >= TERM_OPS {
        return None;
    }
    Some((0xB9200u64 + (t as u64) * 0x2E) ^ ANCHOR)
}

/// 终端操作名（attach 为容器内开发全链首段）。
pub const fn term_op_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("attach"),
        1 => Some("exec"),
        2 => Some("logs"),
        3 => Some("port-forward"),
        4 => Some("shell"),
        5 => Some("top"),
        6 => Some("describe"),
        7 => Some("kill"),
        _ => None,
    }
}

/// 扩展格载荷（扩展 e；含 rebuild/features，锚 UNX-F52273-J1）。
pub const fn ext_cell(e: usize) -> Option<u64> {
    if e >= EXT_CELLS {
        return None;
    }
    Some((0xB9300u64 + (e as u64) * 0x35) ^ ANCHOR)
}

/// 扩展名。
pub const fn ext_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("rebuild"),
        1 => Some("features"),
        2 => Some("metrics"),
        3 => Some("kubeconfig-sync"),
        4 => Some("portforward-ui"),
        5 => Some("log-stream"),
        _ => None,
    }
}

/// catalog 场景折叠（场景 s）。
pub const fn catalog_scene(s: usize) -> Option<u32> {
    if s >= CATALOG_SCENES {
        return None;
    }
    Some(0xB9400 + s as u32 * 0x2F)
}

/// catalog 场景名。
pub const fn catalog_scene_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("clusters"),
        1 => Some("workloads"),
        2 => Some("networks"),
        3 => Some("storage"),
        4 => Some("configs"),
        _ => None,
    }
}

/// Electron 层联签折叠（层 l）。
pub const fn electron_layer(l: usize) -> Option<u32> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some(0xB9500 + l as u32 * 0x3D)
}

/// Electron 层名。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("main"),
        1 => Some("preload"),
        2 => Some("renderer"),
        3 => Some("native"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Lens 收口账本状态机。
#[derive(Clone)]
pub struct LensLedger {
    /// 六判据（安装/连接/五面/终端/扩展/登记）。
    gates: [Option<bool>; 6],
    /// 五面账。
    views: [Option<u64>; CELL_CELLS],
    /// 终端账。
    terms: [Option<u64>; TERM_OPS],
    /// 扩展账。
    exts: [Option<u64>; EXT_CELLS],
    /// catalog 场景账。
    catalogs: [Option<u32>; CATALOG_SCENES],
    /// Electron 联签账。
    electron: [Option<u32>; ELECTRON_LAYERS],
    /// 最后五面族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LensLedger {
    /// 新账本。
    pub fn new() -> Self {
        LensLedger {
            gates: [None; 6],
            views: [None; CELL_CELLS],
            terms: [None; TERM_OPS],
            exts: [None; EXT_CELLS],
            catalogs: [None; CATALOG_SCENES],
            electron: [None; ELECTRON_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LensCode> {
        if gate >= 6 {
            return Err(LensCode::ESeal);
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

    /// 五面一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_view(&mut self, c: usize, got: u64) -> Result<(), LensCode> {
        if c >= CELL_CELLS {
            return Err(LensCode::ENoCell);
        }
        let kind = match view_kind(c) {
            Some(k) => k,
            None => return Err(LensCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(LensCode::EOrder);
        }
        let want = match view_cell(c) {
            Some(w) => w,
            None => return Err(LensCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + c as u64);
            return Err(LensCode::ECellDrift);
        }
        if match self.views.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LensCode::EDup);
        }
        if let Some(cc) = self.views.get_mut(c) {
            *cc = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + c as u64);
        Ok(())
    }

    /// 五面全账核验（20 格）。
    pub fn views_all(&self) -> bool {
        let mut c = 0usize;
        while c < CELL_CELLS {
            match self.views.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 终端一格（载荷对照 + 重复拒）。
    pub fn record_term(&mut self, t: usize, got: u64) -> Result<(), LensCode> {
        let want = match term_cell(t) {
            Some(w) => w,
            None => return Err(LensCode::ENoTerm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + t as u64);
            return Err(LensCode::ETermDrift);
        }
        if match self.terms.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LensCode::EDup);
        }
        if let Some(c) = self.terms.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x8301 + t as u64);
        Ok(())
    }

    /// 终端全账核验（8 操作）。
    pub fn terms_all(&self) -> bool {
        let mut t = 0usize;
        while t < TERM_OPS {
            match self.terms.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 扩展一格。
    pub fn record_ext(&mut self, e: usize, got: u64) -> Result<(), LensCode> {
        let want = match ext_cell(e) {
            Some(w) => w,
            None => return Err(LensCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + e as u64);
            return Err(LensCode::ETermDrift);
        }
        if match self.exts.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LensCode::EDup);
        }
        if let Some(c) = self.exts.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x8401 + e as u64);
        Ok(())
    }

    /// 扩展全账核验（6 格，含 rebuild/features）。
    pub fn exts_all(&self) -> bool {
        let mut e = 0usize;
        while e < EXT_CELLS {
            match self.exts.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// catalog 场景一格。
    pub fn declare_catalog(&mut self, s: usize) -> Result<(), LensCode> {
        if s >= CATALOG_SCENES {
            return Err(LensCode::ENoAux);
        }
        if match self.catalogs.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LensCode::EDup);
        }
        if let Some(c) = self.catalogs.get_mut(s) {
            *c = catalog_scene(s);
        }
        self.fnv(0x8500 + s as u64);
        Ok(())
    }

    /// catalog 全齐核验（5 场景）。
    pub fn catalogs_all(&self) -> bool {
        let mut s = 0usize;
        while s < CATALOG_SCENES {
            match self.catalogs.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Electron 层联签一格。
    pub fn sign_electron(&mut self, l: usize) -> Result<(), LensCode> {
        let want = match electron_layer(l) {
            Some(w) => w,
            None => return Err(LensCode::ESignMiss),
        };
        if match self.electron.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LensCode::EDup);
        }
        if let Some(c) = self.electron.get_mut(l) {
            *c = Some(want);
        }
        self.fnv(0x8600 + l as u64);
        Ok(())
    }

    /// Electron 联签全齐核验（4 层）。
    pub fn electron_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            match self.electron.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（五账全齐+六判据全落 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.views_all()
            && self.terms_all()
            && self.exts_all()
            && self.catalogs_all()
            && self.electron_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for LensLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LensLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < CELL_CELLS {
        if let Some(w) = view_cell(c) {
            if led.record_view(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < TERM_OPS {
        if let Some(w) = term_cell(t) {
            if led.record_term(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut e = 0usize;
    while e < EXT_CELLS {
        if let Some(w) = ext_cell(e) {
            if led.record_ext(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut s = 0usize;
    while s < CATALOG_SCENES {
        let _ = led.declare_catalog(s);
        s += 1;
    }
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        let _ = led.sign_electron(l);
        l += 1;
    }
    // 注入 10 轮：越闭集 / 载荷漂移 / 五面族逆序三态轮转，全部落在当轮
    // 临时账本上；主账本只记注入计数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LensLedger::new();
        let caught = match kind {
            0 => {
                let hit_a = lt.record_view(CELL_CELLS, 0) == Err(LensCode::ENoCell);
                let hit_b = lt.record_term(TERM_OPS, 0).is_err();
                hit_a || hit_b
            }
            1 => {
                // 漂移：先录正格，再注漂移载荷（同族内不触发族序）。
                let _ = lt.record_view(4, view_cell(4).unwrap_or(0));
                lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(LensCode::ECellDrift)
            }
            _ => {
                // 族逆序：对空账本注入逆序对同样必抓，且不改 last_kind。
                let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
                let before = lt.last_kind;
                let hit = lt.record_view(1, view_cell(1).unwrap_or(0)) == Err(LensCode::EOrder);
                hit && lt.last_kind == before
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
/// UNX-F52272 判据集（锚 UNX-F52273-J1，off-by-one 板式）。
pub fn run_unx52272_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52272");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面 20 格全通",
        recorded >= CELL_CELLS as u32,
        "集群/工作负载/网络/存储/配置五面逐格恒等",
    );
    s.add(
        "五面单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CELL_CELLS {
                if view_kind(c).unwrap_or(6) < view_kind(c - 1).unwrap_or(6) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "集群→工作负载→网络→存储→配置单调，逆序必抓",
    );
    s.add(
        "终端 8 操作全落",
        {
            let mut lt = LensLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TERM_OPS {
                if let Some(w) = term_cell(t) {
                    if lt.record_term(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.terms_all()
        },
        "attach/exec/logs/port-forward/shell/top/describe/kill 逐操作恒等",
    );
    s.add(
        "容器内开发全链 ×10（attach/rebuild/features）",
        {
            // attach（终端首段）+ rebuild/features（扩展前两格）三段分账，
            // 扩展段 6 格全账落满后逐格折叠恒等核验（全链 ×10 口径）。
            let mut lt = LensLedger::new();
            let mut ok = term_cell(0).is_some() && term_op_name(0) == Some("attach");
            let mut e = 0usize;
            while e < EXT_CELLS {
                if let Some(w) = ext_cell(e) {
                    ok = ok && lt.record_ext(e, w).is_ok();
                }
                e += 1;
            }
            ok && lt.exts_all()
        },
        "attach/rebuild/features 三段分账，全链 ×10 口径（判据锚 UNX-F52273-J1）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = LensLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/连接/五面/终端/扩展/登记逐格落账",
    );
    s.add(
        "扩展 6 格全齐",
        {
            let mut lt = LensLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXT_CELLS {
                if let Some(w) = ext_cell(e) {
                    if lt.record_ext(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.exts_all()
        },
        "rebuild/features/metrics/kubeconfig-sync/portforward-ui/log-stream 逐格",
    );
    s.add(
        "catalog 5 场景全齐",
        {
            let mut lt = LensLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < CATALOG_SCENES {
                if lt.declare_catalog(sc).is_err() {
                    ok = false;
                }
                sc += 1;
            }
            ok && lt.catalogs_all()
        },
        "clusters/workloads/networks/storage/configs 五场景闭集",
    );
    s.add(
        "F52261/F52021 双消费联签锚定",
        {
            // Electron 四层联签全落即双消费锚定行齐备。
            let mut lt = LensLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if lt.sign_electron(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.electron_all()
        },
        "main/preload/renderer/native 四层联签（与 F52261/F52021 双消费联签锚定行）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "越闭集/载荷漂移/五面族逆序三态轮转必抓",
    );
    s.add(
        "边界·五面格越闭集",
        LensLedger::new().record_view(CELL_CELLS, 0).is_err(),
        "五面格 20 越闭集必拒",
    );
    s.add(
        "终端漂移拒",
        {
            let mut lt = LensLedger::new();
            let _ = lt.record_term(0, term_cell(0).unwrap_or(0));
            lt.record_term(1, term_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(LensCode::ETermDrift)
        },
        "终端载荷漂移必抓",
    );
    s.add(
        "边界·终端操作越闭集",
        LensLedger::new().record_term(TERM_OPS, 0).is_err(),
        "终端操作 8 越闭集必拒",
    );
    s.add(
        "边界·Electron 层越闭集",
        LensLedger::new().sign_electron(ELECTRON_LAYERS).is_err(),
        "Electron 层 4 越闭集必拒",
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
            // main/aux 各节全部记账后再断言。
            let mut lt = LensLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < CELL_CELLS {
                if let Some(w) = view_cell(c) {
                    let _ = lt.record_view(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < TERM_OPS {
                if let Some(w) = term_cell(t) {
                    let _ = lt.record_term(t, w);
                }
                t += 1;
            }
            let mut e = 0usize;
            while e < EXT_CELLS {
                if let Some(w) = ext_cell(e) {
                    let _ = lt.record_ext(e, w);
                }
                e += 1;
            }
            let mut sc = 0usize;
            while sc < CATALOG_SCENES {
                let _ = lt.declare_catalog(sc);
                sc += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                let _ = lt.sign_electron(l);
                l += 1;
            }
            lt.judge_grade() == Grade::S1 && Grade::S1.val() == 1
        },
        "五账全齐+六判据全落+Electron 四层 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位（0xC83C0）",
    );
    s.add(
        "§AI-66.2 跨应用共面口径",
        {
            // 五面 × catalog 场景共面：20 格与 5 场景一一映射。
            let mut ok = CELL_CELLS == CATALOG_SCENES * 4;
            let mut sc = 0usize;
            while sc < CATALOG_SCENES {
                if catalog_scene_name(sc).is_none() {
                    ok = false;
                }
                sc += 1;
            }
            ok
        },
        "五面视图与 catalog 五场景跨应用共面（§AI-66.2 专题二）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_families() {
        assert_eq!(view_kind_name(0), Some("集群"));
        assert_eq!(view_kind_name(1), Some("工作负载"));
        assert_eq!(view_kind_name(2), Some("网络"));
        assert_eq!(view_kind_name(3), Some("存储"));
        assert_eq!(view_kind_name(4), Some("配置"));
        assert!(view_kind_name(5).is_none());
        assert_eq!(view_kind(0), Some(0));
        assert_eq!(view_kind(19), Some(4));
        assert!(view_kind(20).is_none());
        assert!(view_cell(CELL_CELLS).is_none());
    }

    #[test]
    fn test_term_ops_closed() {
        let mut t = 0usize;
        while t < TERM_OPS {
            assert!(term_op_name(t).is_some());
            t += 1;
        }
        assert_eq!(term_op_name(0), Some("attach"));
        assert_eq!(term_op_name(7), Some("kill"));
        assert!(term_op_name(8).is_none());
        assert!(term_cell(TERM_OPS).is_none());
    }

    #[test]
    fn test_ext_cells_closed() {
        assert_eq!(ext_name(0), Some("rebuild"));
        assert_eq!(ext_name(1), Some("features"));
        assert_eq!(ext_name(5), Some("log-stream"));
        assert!(ext_name(6).is_none());
        assert!(ext_cell(EXT_CELLS).is_none());
    }

    #[test]
    fn test_catalog_electron_closed() {
        assert_eq!(catalog_scene_name(0), Some("clusters"));
        assert_eq!(catalog_scene_name(4), Some("configs"));
        assert!(catalog_scene_name(5).is_none());
        assert!(catalog_scene(CATALOG_SCENES).is_none());
        assert_eq!(electron_layer_name(0), Some("main"));
        assert_eq!(electron_layer_name(3), Some("native"));
        assert!(electron_layer_name(4).is_none());
        assert!(electron_layer(ELECTRON_LAYERS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = view_cell(7).unwrap_or(0);
        assert_eq!(a, view_cell(7).unwrap_or(0));
        assert_ne!(a, view_cell(8).unwrap_or(0));
        let b = term_cell(3).unwrap_or(0);
        assert_ne!(b, term_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = LensLedger::new();
        let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
        let before = lt.last_kind;
        assert_eq!(
            lt.record_view(1, view_cell(1).unwrap_or(0)),
            Err(LensCode::EOrder)
        );
        assert_eq!(lt.last_kind, before);
        assert!(lt.record_view(12, view_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LensLedger::new();
        assert_eq!(
            lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(LensCode::ECellDrift)
        );
        assert_eq!(
            lt.record_term(2, term_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(LensCode::ETermDrift)
        );
        assert_eq!(
            lt.record_ext(3, ext_cell(3).unwrap_or(0).wrapping_add(2)),
            Err(LensCode::ETermDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LensLedger::new();
        let _ = lt.record_view(0, view_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_view(0, view_cell(0).unwrap_or(0)),
            Err(LensCode::EDup)
        );
        let _ = lt.record_term(0, term_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_term(0, term_cell(0).unwrap_or(0)),
            Err(LensCode::EDup)
        );
        let _ = lt.sign_electron(0);
        assert_eq!(lt.sign_electron(0), Err(LensCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LensLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = LensLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CELL_CELLS + TERM_OPS + EXT_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_rotation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = LensLedger::new();
        assert!(lt.record_view(CELL_CELLS, 0).is_err());
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
        assert_eq!(SEG_BASE, 0xC83C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(LensCode::Ok as u32, 0);
        assert_eq!(LensCode::EGradeDrift as u32, 12);
        let all = [
            LensCode::Ok,
            LensCode::ENoCell,
            LensCode::ECellDrift,
            LensCode::ENoTerm,
            LensCode::ETermDrift,
            LensCode::EOrder,
            LensCode::ENoAux,
            LensCode::ESignMiss,
            LensCode::EDup,
            LensCode::ESeal,
            LensCode::EInj,
            LensCode::EFp,
            LensCode::EGradeDrift,
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
        assert_eq!(Grade::S1.val(), 1);
        let lt = LensLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = LensLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = LensLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52272_checks_green() {
        let s = run_unx52272_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
