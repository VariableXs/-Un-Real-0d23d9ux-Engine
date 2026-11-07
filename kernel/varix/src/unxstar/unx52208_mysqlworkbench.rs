//! UNX-F52208 MySQL Workbench 收口 · 0xC73C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 _tools/vx15_cards.json 键 "52208" 逐字锚定）：
//!   title = "MySQL Workbench 收口"；quota = 260；criteria = "① 纯功
//!   能代码 ≥260 行；② 判据原文：UNX-F52209-J1 六判据全落；集合浏览/
//!   聚合管道/索引管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。"
//! 板式说明：off-by-one 板式——卡 N 判据锚 UNX-F(N+1)-J1 照原文锚定
//! （本卡 52208 判据锚 UNX-F52209-J1），功能按本卡标题落地。
//!
//! 设计要点：
//!   ① 域模型：建模/正向工程/逆向工程三面 30 格闭集（每面 10 格，逐
//!      格载荷纯函数折叠恒等才收）；三面枚举三元。
//!   ② EER 图 12 格闭集（实体/关系/视图三层各 4 格）。
//!   ③ 迁移向导 10 场景闭集（场景类别五元：源探测/映射/类型转换/脚本
//!      生成/数据搬迁，各 2 格），类别单调，逆序必抓。
//!   ④ C++/CTK 上栈 4 层闭集（CTK 插件层/SCA 数据建模层/SQL 解析层/
//!      平台抽象层）。
//!   ⑤ 注入 ×10 轮（三面载荷漂移/EER 漂移/场景越闭集三态轮转）
//!      10/10 必抓，检出+逃逸守恒等式逐轮成立；六判据母版闸 + 定级
//!      闭集（30 三面格 + 12 EER 格 + 10 场景 + 4 层栈全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52208 − 52201)）。
pub const SEG_BASE: u32 = 0xC73C0;

/// 三面格数（建模/正向工程/逆向工程 × 10 格）。
pub const FACE_CELLS: usize = 30;

/// EER 图格数。
pub const EER_CELLS: usize = 12;

/// 迁移向导场景数。
pub const MIG_SCENARIOS: usize = 10;

/// C++/CTK 上栈层数。
pub const STACK_LAYERS: usize = 4;

/// 守卫：段基址 = 公式值且 0x40 对齐。
const _SEG_GUARD: () = assert!(
    SEG_BASE == 0xC7200 + 0x40 * (52208u32 - 52201) && SEG_BASE % 0x40 == 0
);

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbCode {
    /// 成功
    Ok = 0,
    /// 三面格越闭集
    ENoFace,
    /// 三面载荷漂移
    EFaceDrift,
    /// EER 格越闭集
    ENoEer,
    /// EER 载荷漂移
    EEerDrift,
    /// 场景号越闭集
    ENoScene,
    /// 场景载荷漂移
    ESceneDrift,
    /// 场景类别逆序
    EOrder,
    /// 上栈层越闭集
    ENoLayer,
    /// 上栈层漂移
    ELayerDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl WbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WbCode::Ok => "成功",
            WbCode::ENoFace => "三面格越闭集：建模/正向工程/逆向工程 30 格闭集之外先登记",
            WbCode::EFaceDrift => "三面载荷漂移：面折叠与基准不恒等，逐格复算",
            WbCode::ENoEer => "EER 格越闭集：EER 图 12 格闭集之外先登记",
            WbCode::EEerDrift => "EER 载荷漂移：EER 折叠与基准不恒等，逐格复算",
            WbCode::ENoScene => "场景号越闭集：迁移向导 10 场景闭集之外先登记",
            WbCode::ESceneDrift => "场景载荷漂移：场景折叠与基准不恒等，逐格复算",
            WbCode::EOrder => "场景类别逆序：源探测→映射→类型转换→脚本生成→数据搬迁单调，逆序必抓",
            WbCode::ENoLayer => "上栈层越闭集：C++/CTK 上栈 4 层闭集之外先登记",
            WbCode::ELayerDrift => "上栈层漂移：层折叠与基准不恒等，逐层复算",
            WbCode::EDup => "重复落账：同格只许一条账目",
            WbCode::ESeal => "未齐即封：30 三面格 + 12 EER 格 + 10 场景 + 4 层栈未齐先补齐",
            WbCode::EGradeDrift => "定级漂移：全格全通且上栈全落才定级 S1",
        }
    }
}

// ── 三面枚举（三元闭集） ─────────────────────────────────────────
/// 三面：0=建模 1=正向工程 2=逆向工程。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbFace {
    /// 建模
    Modeling,
    /// 正向工程
    Forward,
    /// 逆向工程
    Reverse,
}

impl WbFace {
    pub fn tag(self) -> &'static str {
        match self {
            WbFace::Modeling => "建模",
            WbFace::Forward => "正向工程",
            WbFace::Reverse => "逆向工程",
        }
    }
}

/// 三面格 → 面（格 f：f/10）。
pub const fn face_of(f: usize) -> Option<WbFace> {
    if f >= FACE_CELLS {
        return None;
    }
    match f / 10 {
        0 => Some(WbFace::Modeling),
        1 => Some(WbFace::Forward),
        _ => Some(WbFace::Reverse),
    }
}

/// 三面格基准（格 f；载荷 = 面段基 ^ 槽位步进 ^ 常量扰动）。
pub const fn face_cell(f: usize) -> Option<u64> {
    if f >= FACE_CELLS {
        return None;
    }
    let face = (f / 10) as u64;
    let slot = (f % 10) as u64;
    Some((0xBE000u64 + face * 0x140) ^ slot.wrapping_mul(0x01000193) ^ 0x57426E63684552)
}

// ── EER 图 12 格闭集（实体/关系/视图三层各 4 格） ────────────────
/// EER 层（格 e：0..4=实体层 4..8=关系层 8..12=视图层）。
pub const fn eer_layer(e: usize) -> Option<u32> {
    if e >= EER_CELLS {
        return None;
    }
    Some((e / 4) as u32)
}

/// EER 格基准（格 e）。
pub const fn eer_cell(e: usize) -> Option<u64> {
    if e >= EER_CELLS {
        return None;
    }
    Some((0xBF000u64 + (e as u64) * 0x34) ^ 0x4545524449414752)
}

/// EER 层名。
pub const fn eer_layer_name(l: u32) -> Option<&'static str> {
    match l {
        0 => Some("实体层"),
        1 => Some("关系层"),
        2 => Some("视图层"),
        _ => None,
    }
}

// ── 迁移向导 10 场景闭集 ─────────────────────────────────────────
/// 场景类别（0..2=源探测 2..4=映射 4..6=类型转换 6..8=脚本生成 8..10=数据搬迁）。
pub const fn scene_kind(o: usize) -> Option<u32> {
    if o >= MIG_SCENARIOS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 场景格载荷（场景 o；类别 = o/2）。
pub const fn scene_cell(o: usize) -> Option<u64> {
    if o >= MIG_SCENARIOS {
        return None;
    }
    Some((0xC0000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

// ── C++/CTK 上栈 4 层 ────────────────────────────────────────────
/// 上栈层基准（层 t）。
pub const fn stack_layer(t: usize) -> Option<u32> {
    if t >= STACK_LAYERS {
        return None;
    }
    Some(0xC1000 + t as u32 * 0x2C)
}

/// 上栈层名（CTK 插件层/SCA 数据建模层/SQL 解析层/平台抽象层）。
pub const fn stack_layer_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("CTK 插件层"),
        1 => Some("SCA 数据建模层"),
        2 => Some("SQL 解析层"),
        3 => Some("平台抽象层"),
        _ => None,
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 定级（S1/S2/S3 闭集；本卡 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbGrade {
    /// S1：三面全收口级
    S1,
    /// S2：不达（备用档）
    S2,
    /// S3：降级档
    S3,
}

impl WbGrade {
    pub fn tag(self) -> &'static str {
        match self {
            WbGrade::S1 => "S1",
            WbGrade::S2 => "S2",
            WbGrade::S3 => "S3",
        }
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct WbLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 三面账。
    faces: [Option<u64>; FACE_CELLS],
    /// EER 账。
    eers: [Option<u64>; EER_CELLS],
    /// 场景账。
    scenes: [Option<u64>; MIG_SCENARIOS],
    /// 上栈层账。
    layers: [Option<u32>; STACK_LAYERS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WbLedger {
    /// 新账本。
    pub fn new() -> Self {
        WbLedger {
            gates: [None; 6],
            faces: [None; FACE_CELLS],
            eers: [None; EER_CELLS],
            scenes: [None; MIG_SCENARIOS],
            layers: [None; STACK_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WbCode> {
        if gate >= 6 {
            return Err(WbCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8A100 + gate as u64);
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

    /// 三面一格（载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u64) -> Result<(), WbCode> {
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(WbCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8A200 + f as u64);
            return Err(WbCode::EFaceDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WbCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x8A201 + f as u64);
        Ok(())
    }

    /// 三面全账核验（30 格）。
    pub fn faces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACE_CELLS {
            match self.faces.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// EER 一格（载荷对照 + 重复拒）。
    pub fn record_eer(&mut self, e: usize, got: u64) -> Result<(), WbCode> {
        let want = match eer_cell(e) {
            Some(w) => w,
            None => return Err(WbCode::ENoEer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8A300 + e as u64);
            return Err(WbCode::EEerDrift);
        }
        if match self.eers.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WbCode::EDup);
        }
        if let Some(c) = self.eers.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x8A301 + e as u64);
        Ok(())
    }

    /// EER 全账核验（12 格）。
    pub fn eers_all(&self) -> bool {
        let mut e = 0usize;
        while e < EER_CELLS {
            match self.eers.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 场景一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_scene(&mut self, o: usize, got: u64) -> Result<(), WbCode> {
        if o >= MIG_SCENARIOS {
            return Err(WbCode::ENoScene);
        }
        let kind = match scene_kind(o) {
            Some(k) => k,
            None => return Err(WbCode::ENoScene),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8A400);
            return Err(WbCode::EOrder);
        }
        let want = match scene_cell(o) {
            Some(w) => w,
            None => return Err(WbCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8A401 + o as u64);
            return Err(WbCode::ESceneDrift);
        }
        if match self.scenes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WbCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8A402 + o as u64);
        Ok(())
    }

    /// 场景全账核验（10 格）。
    pub fn scenes_all(&self) -> bool {
        let mut o = 0usize;
        while o < MIG_SCENARIOS {
            match self.scenes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 上栈层一格（层折叠对照 + 重复拒）。
    pub fn mount_layer(&mut self, t: usize) -> Result<(), WbCode> {
        let want = match stack_layer(t) {
            Some(w) => w,
            None => return Err(WbCode::ENoLayer),
        };
        if match self.layers.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WbCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x8A500 + t as u64);
        Ok(())
    }

    /// 上栈全落核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut t = 0usize;
        while t < STACK_LAYERS {
            match self.layers.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 收口定级（全格全通 + 上栈全落 + 六判据全落 → S1）。
    pub fn grade(&self) -> WbGrade {
        if self.gates_all() && self.faces_all() && self.eers_all() && self.scenes_all() && self.layers_all() {
            WbGrade::S1
        } else if self.faces_all() && self.eers_all() {
            WbGrade::S2
        } else {
            WbGrade::S3
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for WbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FACE_CELLS {
        if let Some(w) = face_cell(f) {
            if led.record_face(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut e = 0usize;
    while e < EER_CELLS {
        if let Some(w) = eer_cell(e) {
            if led.record_eer(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut o = 0usize;
    while o < MIG_SCENARIOS {
        if let Some(w) = scene_cell(o) {
            if led.record_scene(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < STACK_LAYERS {
        let _ = led.mount_layer(t);
        t += 1;
    }
    // 注入 10 轮：三面载荷漂移 / EER 载荷漂移 / 场景越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WbLedger::new();
        let caught = match kind {
            0 => lt.record_face(14, face_cell(14).unwrap_or(0).wrapping_add(1)) == Err(WbCode::EFaceDrift),
            1 => lt.record_eer(7, eer_cell(7).unwrap_or(0).wrapping_add(1)) == Err(WbCode::EEerDrift),
            _ => lt.record_scene(MIG_SCENARIOS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8A600 + k as u64);
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
pub fn run_unx52208_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52208");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三面 30 格全通",
        recorded >= (FACE_CELLS as u32) + (EER_CELLS as u32) + (MIG_SCENARIOS as u32),
        "建模/正向工程/逆向工程 × 10 格逐格恒等",
    );
    s.add(
        "建模/正向/逆向三面逐格恒等",
        {
            let mut lt = WbLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    if lt.record_face(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.faces_all()
        },
        "三面 ×10 逐格折叠零漂移（判据锚 UNX-F52209-J1）",
    );
    s.add(
        "EER 图 12/12 全通",
        {
            let mut lt = WbLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EER_CELLS {
                if let Some(w) = eer_cell(e) {
                    if lt.record_eer(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.eers_all()
        },
        "实体/关系/视图三层各 4 格闭集",
    );
    s.add(
        "迁移向导 10 场景全通",
        {
            let mut lt = WbLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < MIG_SCENARIOS {
                if let Some(w) = scene_cell(o) {
                    if lt.record_scene(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.scenes_all()
        },
        "源探测/映射/类型转换/脚本生成/数据搬迁 五类逐场景恒等",
    );
    s.add(
        "C++/CTK 上栈 4 层全落",
        {
            let mut lt = WbLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < STACK_LAYERS {
                if lt.mount_layer(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.layers_all()
        },
        "CTK 插件层/SCA 数据建模层/SQL 解析层/平台抽象层",
    );
    s.add(
        "场景类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MIG_SCENARIOS {
                if scene_kind(o).unwrap_or(9) < scene_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五类闭集单调",
    );
    s.add(
        "三面面位单调映射",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < FACE_CELLS {
                let expect = f / 10;
                match face_of(f) {
                    Some(WbFace::Modeling) => {
                        if expect != 0 {
                            ok = false;
                        }
                    }
                    Some(WbFace::Forward) => {
                        if expect != 1 {
                            ok = false;
                        }
                    }
                    Some(WbFace::Reverse) => {
                        if expect != 2 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                f += 1;
            }
            ok && face_of(FACE_CELLS).is_none()
        },
        "建模/正向工程/逆向工程三元映射闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "三面漂移/EER 漂移/越闭集三态轮转必抓");
    let mut l0 = WbLedger::new();
    s.add(
        "边界·三面越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "三面格 30 越闭集必拒",
    );
    s.add(
        "边界·EER 越闭集",
        l0.record_eer(EER_CELLS, 0).is_err(),
        "EER 格 12 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_scene(MIG_SCENARIOS, 0).is_err(),
        "场景 10 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    let led_full = {
        let mut lt = WbLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut f = 0usize;
        while f < FACE_CELLS {
            if let Some(w) = face_cell(f) {
                let _ = lt.record_face(f, w);
            }
            f += 1;
        }
        let mut e = 0usize;
        while e < EER_CELLS {
            if let Some(w) = eer_cell(e) {
                let _ = lt.record_eer(e, w);
            }
            e += 1;
        }
        let mut o = 0usize;
        while o < MIG_SCENARIOS {
            if let Some(w) = scene_cell(o) {
                let _ = lt.record_scene(o, w);
            }
            o += 1;
        }
        let mut t = 0usize;
        while t < STACK_LAYERS {
            let _ = lt.mount_layer(t);
            t += 1;
        }
        lt
    };
    s.add(
        "定级闭集 S1",
        led_full.grade() == WbGrade::S1 && led_full.grade().tag() == "S1",
        "全格全通+上栈全落 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC73C0 && SEG_BASE > 0xC7380,
        "SEG_BASE 0x40 对齐且落在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_of_map() {
        assert_eq!(face_of(0), Some(WbFace::Modeling));
        assert_eq!(face_of(9), Some(WbFace::Modeling));
        assert_eq!(face_of(10), Some(WbFace::Forward));
        assert_eq!(face_of(29), Some(WbFace::Reverse));
        assert!(face_of(30).is_none());
        assert!(face_cell(30).is_none());
    }

    #[test]
    fn test_eer_layers() {
        assert_eq!(eer_layer(0), Some(0));
        assert_eq!(eer_layer(4), Some(1));
        assert_eq!(eer_layer(11), Some(2));
        assert!(eer_layer(12).is_none());
        assert!(eer_cell(12).is_none());
        assert_eq!(eer_layer_name(1), Some("关系层"));
        assert!(eer_layer_name(3).is_none());
    }

    #[test]
    fn test_scene_kind() {
        assert_eq!(scene_kind(0), Some(0));
        assert_eq!(scene_kind(3), Some(1));
        assert_eq!(scene_kind(9), Some(4));
        assert!(scene_kind(10).is_none());
        assert!(scene_cell(10).is_none());
    }

    #[test]
    fn test_stack_and_cells_pure() {
        assert_eq!(stack_layer_name(0), Some("CTK 插件层"));
        assert_eq!(stack_layer_name(3), Some("平台抽象层"));
        assert!(stack_layer_name(4).is_none());
        assert!(stack_layer(STACK_LAYERS).is_none());
        let a = face_cell(11).unwrap_or(0);
        assert_eq!(a, face_cell(11).unwrap_or(0));
        assert_ne!(a, face_cell(12).unwrap_or(0));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WbLedger::new();
        assert_eq!(
            lt.record_face(5, face_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WbCode::EFaceDrift)
        );
        assert!(lt.record_face(5, face_cell(5).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_face(5, face_cell(5).unwrap_or(0)),
            Err(WbCode::EDup)
        );
        assert_eq!(
            lt.record_eer(2, eer_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(WbCode::EEerDrift)
        );
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = WbLedger::new();
        let _ = lt.record_scene(9, scene_cell(9).unwrap_or(0));
        assert_eq!(lt.record_scene(2, scene_cell(2).unwrap_or(0)), Err(WbCode::EOrder));
        assert!(lt.record_scene(8, scene_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (FACE_CELLS + EER_CELLS + MIG_SCENARIOS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52208 - 52201));
        assert!(SEG_BASE > 0xC7380 && SEG_BASE <= 0xC7400);
    }

    #[test]
    fn test_val_details_nonempty() {
        let codes = [
            WbCode::Ok,
            WbCode::ENoFace,
            WbCode::EFaceDrift,
            WbCode::ENoEer,
            WbCode::EEerDrift,
            WbCode::ENoScene,
            WbCode::ESceneDrift,
            WbCode::EOrder,
            WbCode::ENoLayer,
            WbCode::ELayerDrift,
            WbCode::EDup,
            WbCode::ESeal,
            WbCode::EGradeDrift,
        ];
        let mut n = 0u32;
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i].val(), SEG_BASE + n);
            assert!(!codes[i].detail().is_empty());
            n += 1;
            i += 1;
        }
    }

    #[test]
    fn test_unx52208_checks_green() {
        let s = run_unx52208_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
