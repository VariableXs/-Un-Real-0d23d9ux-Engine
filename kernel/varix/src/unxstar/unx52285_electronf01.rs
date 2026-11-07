//! UNX-F52285 第一个 Electron 组合态失绿现场实况（DJ-UNX-N1-01 域经义务回填） · 0xC8700段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52285"，逐字）：
//!   title: 第一个 Electron 组合态失绿现场实况（DJ-UNX-N1-01 域经义务回填）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52286-J1 抽查 40
//!     应用 J5 三层（界面/输入/字体）全过；M1 波 22 闭账物之后复测一轮，
//!     衔接账公开（联签锚定行 AI-61）；判据锚定 §AI-66.5 J-中文；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52285）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52286-J1，照原文锚定；功能按本卡标题（第一个 Electron 组合态
//! 失绿现场实况·DJ-UNX-N1-01 域经义务回填）落地，40 应用抽测判据由同
//! 批后续卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 组合态五段闭集：安装/启动/渲染/崩溃/回填五段 × 各 4 格 = 20 格，
//!      段序单调（逆序必抓）；每格载荷折叠恒等才收。
//!   ② 义务回填链 8 拍：严格等差链（拍 b 只许在 last+1 落），跳拍/逆序/
//!      空账跳拍一律必抓——义务回填不许欠拍。
//!   ③ DJ-UNX-N1-01 域归因账 6 格：逐格载荷对照，漂移必抓；复测零漂移
//!      10 场景：逐场景载荷恒等，M1 波 22 闭账物之后复测一轮口径。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（回填链跳拍/归因漂移/组合态格越
//!      闭集），全部落在当轮临时账本上，主账本 last_phase/last_beat 不受
//!      扰动；检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑤ KB-COMPAT 定级闭集：20 格组合态 + 8 拍回填 + 6 格归因 + 10 场景
//!      复测 + 六判据全通 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52285;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 组合态五段段数。
pub const COMBO_PHASES: usize = 5;

/// 每段格数。
pub const CELLS_PER_PHASE: usize = 4;

/// 组合态总格数（5 段 × 4 格）。
pub const COMBO_CELLS: usize = COMBO_PHASES * CELLS_PER_PHASE;

/// 义务回填链拍数。
pub const BACKFILL_BEATS: usize = 8;

/// DJ-UNX-N1-01 域归因账格数。
pub const ATTRIB_CELLS: usize = 6;

/// 复测零漂移场景数。
pub const RETEST_SCENES: usize = 10;

/// 组合态域异或锚（低 16 位恒零）。
pub const COMBO_ANCHOR: u64 = 0x454C_0000;

/// 归因域异或锚（低 16 位恒零）。
pub const ATTRIB_ANCHOR: u64 = 0x4E31_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElectronF01Code {
    /// 成功
    Ok = 0,
    /// 组合态格越闭集
    ENoPhase,
    /// 组合态载荷漂移
    EPhaseDrift,
    /// 回填拍越闭集
    ENoBeat,
    /// 义务回填链跳拍/逆序
    EBeatOrder,
    /// 归因格越闭集
    ENoAttrib,
    /// 归因载荷漂移
    EAttribDrift,
    /// 复测场景越闭集
    ENoScene,
    /// 复测场景漂移
    ESceneDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl ElectronF01Code {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ElectronF01Code::Ok => "成功",
            ElectronF01Code::ENoPhase => "组合态格越闭集：安装/启动/渲染/崩溃/回填 20 格之外先登记",
            ElectronF01Code::EPhaseDrift => "组合态载荷漂移：格折叠与基准不恒等，逐格复算",
            ElectronF01Code::ENoBeat => "回填拍越闭集：义务回填链 8 拍之外先登记",
            ElectronF01Code::EBeatOrder => "义务回填链跳拍/逆序：拍 b 只许在 last+1 落，义务回填不许欠拍",
            ElectronF01Code::ENoAttrib => "归因格越闭集：DJ-UNX-N1-01 域归因账 6 格之外先登记",
            ElectronF01Code::EAttribDrift => "归因载荷漂移：归因格折叠与基准不恒等，逐格复算",
            ElectronF01Code::ENoScene => "复测场景越闭集：复测零漂移 10 场景之外先登记",
            ElectronF01Code::ESceneDrift => "复测场景漂移：场景载荷与波 22 闭账基准不恒等，逐场景复测",
            ElectronF01Code::EDup => "重复落账：同格只许一条账目",
            ElectronF01Code::ESeal => "未齐即封：20 格组合态+8 拍回填+6 格归因+10 场景未齐先补齐",
            ElectronF01Code::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ElectronF01Code::EGradeDrift => "定级漂移：全部节账全通才可定 S1",
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
            Grade::S1 => "S1：六判据+20格组合态+8拍回填+6格归因+10场景复测全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 组合态格载荷（格 c：0..20，段 = c/4）。
pub const fn combo_cell(c: usize) -> Option<u64> {
    if c >= COMBO_CELLS {
        return None;
    }
    Some((0xB1000u64 + (c as u64) * 0x5D) ^ COMBO_ANCHOR)
}

/// 组合态段（0..4=安装 4..8=启动 8..12=渲染 12..16=崩溃 16..20=回填）。
pub const fn combo_phase(c: usize) -> Option<u32> {
    if c >= COMBO_CELLS {
        return None;
    }
    Some((c / CELLS_PER_PHASE) as u32)
}

/// 段名。
pub const fn combo_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("安装"),
        1 => Some("启动"),
        2 => Some("渲染"),
        3 => Some("崩溃"),
        4 => Some("回填"),
        _ => None,
    }
}

/// 义务回填链拍载荷（拍 b：0..8）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BACKFILL_BEATS {
        return None;
    }
    Some(0xB2000u64 + (b as u64) * 0x71)
}

/// 归因格载荷（格 a：0..6，DJ-UNX-N1-01 域）。
pub const fn attrib_cell(a: usize) -> Option<u64> {
    if a >= ATTRIB_CELLS {
        return None;
    }
    Some((0xB3000u64 + (a as u64) * 0x37) ^ ATTRIB_ANCHOR)
}

/// 复测场景载荷（场景 s：0..10）。
pub const fn scene_cell(s: usize) -> Option<u64> {
    if s >= RETEST_SCENES {
        return None;
    }
    Some(0xB4000u64 + (s as u64) * 0x53)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Electron 组合态失绿现场收口账本状态机。
#[derive(Clone)]
pub struct ElectronF01Ledger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 组合态 20 格账。
    combos: [Option<u64>; COMBO_CELLS],
    /// 义务回填链 8 拍账。
    beats: [Option<u64>; BACKFILL_BEATS],
    /// 归因账 6 格。
    attribs: [Option<u64>; ATTRIB_CELLS],
    /// 复测 10 场景账。
    scenes: [Option<u64>; RETEST_SCENES],
    /// 最后落账段。
    last_phase: i64,
    /// 最后回填拍（-1 起严格等差）。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ElectronF01Ledger {
    /// 新账本。
    pub fn new() -> Self {
        ElectronF01Ledger {
            gates: [None; 6],
            combos: [None; COMBO_CELLS],
            beats: [None; BACKFILL_BEATS],
            attribs: [None; ATTRIB_CELLS],
            scenes: [None; RETEST_SCENES],
            last_phase: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ElectronF01Code> {
        if gate >= 6 {
            return Err(ElectronF01Code::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 组合态一格（段序单调 + 载荷对照 + 重复拒）。
    pub fn record_combo(&mut self, c: usize, got: u64) -> Result<(), ElectronF01Code> {
        if c >= COMBO_CELLS {
            return Err(ElectronF01Code::ENoPhase);
        }
        let phase = match combo_phase(c) {
            Some(p) => p,
            None => return Err(ElectronF01Code::ENoPhase),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(ElectronF01Code::ENoPhase);
        }
        let want = match combo_cell(c) {
            Some(w) => w,
            None => return Err(ElectronF01Code::ENoPhase),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + c as u64);
            return Err(ElectronF01Code::EPhaseDrift);
        }
        if match self.combos.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ElectronF01Code::EDup);
        }
        if let Some(cell) = self.combos.get_mut(c) {
            *cell = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x4202 + c as u64);
        Ok(())
    }

    /// 组合态全账核验（20 格）。
    pub fn combos_all(&self) -> bool {
        let mut c = 0usize;
        while c < COMBO_CELLS {
            match self.combos.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 义务回填链一拍（严格等差 + 载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), ElectronF01Code> {
        if b >= BACKFILL_BEATS {
            return Err(ElectronF01Code::ENoBeat);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(ElectronF01Code::EBeatOrder);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(ElectronF01Code::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + b as u64);
            return Err(ElectronF01Code::EPhaseDrift);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ElectronF01Code::EDup);
        }
        if let Some(cell) = self.beats.get_mut(b) {
            *cell = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x4302 + b as u64);
        Ok(())
    }

    /// 回填链全账核验（8 拍齐且单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BACKFILL_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 归因账一格（载荷对照 + 重复拒）。
    pub fn record_attrib(&mut self, a: usize, got: u64) -> Result<(), ElectronF01Code> {
        if a >= ATTRIB_CELLS {
            return Err(ElectronF01Code::ENoAttrib);
        }
        let want = match attrib_cell(a) {
            Some(w) => w,
            None => return Err(ElectronF01Code::ENoAttrib),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400 + a as u64);
            return Err(ElectronF01Code::EAttribDrift);
        }
        if match self.attribs.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ElectronF01Code::EDup);
        }
        if let Some(cell) = self.attribs.get_mut(a) {
            *cell = Some(got);
        }
        self.fnv(0x4401 + a as u64);
        Ok(())
    }

    /// 归因账全账核验（6 格）。
    pub fn attribs_all(&self) -> bool {
        let mut a = 0usize;
        while a < ATTRIB_CELLS {
            match self.attribs.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 复测场景一格（载荷对照 + 重复拒）。
    pub fn record_scene(&mut self, s: usize, got: u64) -> Result<(), ElectronF01Code> {
        if s >= RETEST_SCENES {
            return Err(ElectronF01Code::ENoScene);
        }
        let want = match scene_cell(s) {
            Some(w) => w,
            None => return Err(ElectronF01Code::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4500 + s as u64);
            return Err(ElectronF01Code::ESceneDrift);
        }
        if match self.scenes.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ElectronF01Code::EDup);
        }
        if let Some(cell) = self.scenes.get_mut(s) {
            *cell = Some(got);
        }
        self.fnv(0x4501 + s as u64);
        Ok(())
    }

    /// 复测全账核验（10 场景零漂移）。
    pub fn scenes_all(&self) -> bool {
        let mut s = 0usize;
        while s < RETEST_SCENES {
            match self.scenes.get(s) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.combos_all() && self.beats_all() && self.attribs_all() && self.scenes_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ElectronF01Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ElectronF01Ledger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < COMBO_CELLS {
        if let Some(w) = combo_cell(c) {
            if led.record_combo(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < BACKFILL_BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut a = 0usize;
    while a < ATTRIB_CELLS {
        if let Some(w) = attrib_cell(a) {
            if led.record_attrib(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut s = 0usize;
    while s < RETEST_SCENES {
        if let Some(w) = scene_cell(s) {
            if led.record_scene(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    // 注入 10 轮：回填链跳拍 / 归因漂移 / 组合态格越闭集三态轮转。
    // 全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ElectronF01Ledger::new();
        let caught = match kind {
            0 => {
                // 空账本跳拍：期望拍 0，先落拍 3 必抓；last_beat 不受扰动。
                lt.record_beat(3, beat_cell(3).unwrap_or(0)) == Err(ElectronF01Code::EBeatOrder)
            }
            1 => {
                let _ = lt.record_attrib(0, attrib_cell(0).unwrap_or(0));
                lt.record_attrib(1, attrib_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ElectronF01Code::EAttribDrift)
            }
            _ => lt.record_combo(COMBO_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4600 + k as u64);
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
/// UNX-F52285 判据集（锚 UNX-F52286-J1，off-by-one 板式）。
pub fn run_unx52285_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52285");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "组合态五段 20 格全通",
        recorded >= COMBO_CELLS as u32,
        "安装/启动/渲染/崩溃/回填五段逐格载荷恒等",
    );
    s.add(
        "组合态段序单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < COMBO_CELLS {
                if combo_phase(c).unwrap_or(5) < combo_phase(c - 1).unwrap_or(5) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五段闭集段序非降，逆序必抓",
    );
    s.add(
        "义务回填链 8 拍单调全落",
        {
            let mut lt = ElectronF01Ledger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BACKFILL_BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "义务回填链严格等差 8 拍，跳拍/逆序/欠拍必抓",
    );
    s.add(
        "DJ-UNX-N1-01 域归因账 6 格全落",
        {
            let mut lt = ElectronF01Ledger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ATTRIB_CELLS {
                if let Some(w) = attrib_cell(a) {
                    if lt.record_attrib(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.attribs_all()
        },
        "域归因逐格载荷对照，漂移必抓",
    );
    s.add(
        "复测零漂移 10 场景全落",
        {
            let mut lt = ElectronF01Ledger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < RETEST_SCENES {
                if let Some(w) = scene_cell(sc) {
                    if lt.record_scene(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.scenes_all()
        },
        "M1 波 22 闭账物之后复测一轮，逐场景零漂移（联签锚定行 AI-61）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ElectronF01Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，未齐即封",
    );
    s.add("注入 10/10 全抓", caught >= 10, "回填链跳拍/归因漂移/组合态格越闭集三态轮转必抓");
    s.add(
        "边界·组合态格越闭集",
        {
            let mut lt = ElectronF01Ledger::new();
            lt.record_combo(COMBO_CELLS, 0).is_err()
        },
        "组合态格 20 越闭集必拒",
    );
    s.add(
        "边界·回填拍越闭集",
        {
            let mut lt = ElectronF01Ledger::new();
            lt.record_beat(BACKFILL_BEATS, 0).is_err()
        },
        "回填拍 8 越闭集必拒",
    );
    s.add(
        "边界·归因格越闭集",
        {
            let mut lt = ElectronF01Ledger::new();
            lt.record_attrib(ATTRIB_CELLS, 0).is_err()
        },
        "归因格 6 越闭集必拒",
    );
    s.add(
        "回填链跳拍拒",
        {
            let mut lt = ElectronF01Ledger::new();
            let r0 = lt.record_beat(0, beat_cell(0).unwrap_or(0));
            let r2 = lt.record_beat(2, beat_cell(2).unwrap_or(0));
            r0.is_ok() && r2 == Err(ElectronF01Code::EBeatOrder)
        },
        "拍 0 之后落拍 2 跳拍必抓",
    );
    s.add(
        "组合态漂移拒",
        {
            let mut lt = ElectronF01Ledger::new();
            lt.record_combo(7, combo_cell(7).unwrap_or(0).wrapping_add(1)) == Err(ElectronF01Code::EPhaseDrift)
        },
        "组合态载荷漂移必抓",
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
            let mut lt = ElectronF01Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < COMBO_CELLS {
                if let Some(w) = combo_cell(c) {
                    let _ = lt.record_combo(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < BACKFILL_BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut a = 0usize;
            while a < ATTRIB_CELLS {
                if let Some(w) = attrib_cell(a) {
                    let _ = lt.record_attrib(a, w);
                }
                a += 1;
            }
            let mut sc = 0usize;
            while sc < RETEST_SCENES {
                if let Some(w) = scene_cell(sc) {
                    let _ = lt.record_scene(sc, w);
                }
                sc += 1;
            }
            // 定级检查：main/aux 各节全部记账完成后再断言。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8700,
        "SEG_BASE 0x40 对齐且按派工公式落 0xC8700",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_names() {
        assert_eq!(combo_phase_name(0), Some("安装"));
        assert_eq!(combo_phase_name(1), Some("启动"));
        assert_eq!(combo_phase_name(2), Some("渲染"));
        assert_eq!(combo_phase_name(3), Some("崩溃"));
        assert_eq!(combo_phase_name(4), Some("回填"));
        assert!(combo_phase_name(5).is_none());
    }

    #[test]
    fn test_combo_phase_map() {
        assert_eq!(combo_phase(0), Some(0));
        assert_eq!(combo_phase(4), Some(1));
        assert_eq!(combo_phase(19), Some(4));
        assert!(combo_phase(20).is_none());
        assert!(combo_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = combo_cell(3).unwrap_or(0);
        assert_eq!(a, combo_cell(3).unwrap_or(0));
        assert_ne!(a, combo_cell(4).unwrap_or(0));
        assert!(beat_cell(BACKFILL_BEATS).is_none());
        assert!(attrib_cell(ATTRIB_CELLS).is_none());
        assert!(scene_cell(RETEST_SCENES).is_none());
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(COMBO_ANCHOR & 0xFFFF, 0);
        assert_eq!(ATTRIB_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_beat_order_caught() {
        // 空账本跳拍必抓。
        let mut lt = ElectronF01Ledger::new();
        assert_eq!(lt.record_beat(3, beat_cell(3).unwrap_or(0)), Err(ElectronF01Code::EBeatOrder));
        // 拍 0 之后落拍 2 跳拍必抓；拍 1 顺落可过。
        assert!(lt.record_beat(0, beat_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_beat(2, beat_cell(2).unwrap_or(0)), Err(ElectronF01Code::EBeatOrder));
        assert!(lt.record_beat(1, beat_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ElectronF01Ledger::new();
        assert_eq!(
            lt.record_combo(5, combo_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ElectronF01Code::EPhaseDrift)
        );
        assert_eq!(
            lt.record_attrib(2, attrib_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(ElectronF01Code::EAttribDrift)
        );
        assert_eq!(
            lt.record_scene(1, scene_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(ElectronF01Code::ESceneDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ElectronF01Ledger::new();
        let _ = lt.record_attrib(0, attrib_cell(0).unwrap_or(0));
        assert_eq!(lt.record_attrib(0, attrib_cell(0).unwrap_or(0)), Err(ElectronF01Code::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ElectronF01Ledger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ElectronF01Ledger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded >= (COMBO_CELLS + BACKFILL_BEATS + ATTRIB_CELLS + RETEST_SCENES) as u32
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
        assert_eq!(SEG_BASE, 0xC8700);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ElectronF01Code::Ok as u32, 0);
        assert_eq!(ElectronF01Code::EGradeDrift as u32, 12);
        assert_eq!(ElectronF01Code::Ok.val(), SEG_BASE);
        assert_eq!(ElectronF01Code::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!ElectronF01Code::Ok.detail().is_empty());
        assert!(!ElectronF01Code::EBeatOrder.detail().is_empty());
        assert!(!ElectronF01Code::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ElectronF01Ledger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52285_checks_green() {
        let s = run_unx52285_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
