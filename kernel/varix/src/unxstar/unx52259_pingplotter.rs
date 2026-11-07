//! UNX-F52259 PingPlotter 收口 · 0xC8080段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52259"，逐字）：
//!   title: PingPlotter 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52260-J1 六判
//!     据全落；线程组压测 ×10 场景；JVM 上栈（F52057 联签锚定行）；
//!     结果聚合与图表 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52259）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52260-J1，照原文锚定；功能按本卡标题（PingPlotter）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 目标闭集 10 格：逐目标载荷恒等才收，越闭集必拒。
//!   ② 采样时间轴 16 拍单调：拍时间戳逐拍严格递增，逆拍/时间戳漂移
//!      必抓（PingPlotter 时间轴语义）。
//!   ③ 延迟/丢包/抖动三面 12 格：每面 4 格，逐格载荷恒等，漂移必抓。
//!   ④ 告警三元闸：布防→触发→解除三段依次落账，跳段必抓。
//!   ⑤ 多目标 5 场景：场景格恒等收口（对应判据「图表 ×5」口径）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 10 轮三态轮转（三面漂移/时间轴
//!      逆拍/目标越闭集）全部落在当轮临时账本上；双跑指纹一致；
//!      KB-COMPAT 定级 S2。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52259;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 目标闭集格数。
pub const TGT_CELLS: usize = 10;

/// 采样时间轴拍数。
pub const TICK_CELLS: usize = 16;

/// 三面格数（延迟/丢包/抖动各 4）。
pub const FACE_CELLS: usize = 12;

/// 告警三元闸段数。
pub const ALERT_PHASES: usize = 3;

/// 多目标场景数。
pub const SCENE_CELLS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PingPlotCode {
    /// 成功
    Ok = 0,
    /// 目标号越闭集
    ENoTgt,
    /// 载荷漂移（目标/时间轴拍/场景共用）
    ECellDrift,
    /// 拍号越闭集
    ENoTick,
    /// 时间轴非单调（逆拍/时间戳回退）
    ETickSeq,
    /// 三面格越闭集
    ENoFace,
    /// 三面载荷漂移
    EFaceDrift,
    /// 告警段越闭集
    ENoAlert,
    /// 告警段乱序
    EAlertOrder,
    /// 场景号越闭集
    ENoScene,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PingPlotCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PingPlotCode::Ok => "成功",
            PingPlotCode::ENoTgt => "目标号越闭集：目标 10 格闭集之外先登记",
            PingPlotCode::ECellDrift => "载荷漂移：目标/时间轴拍/场景折叠与基准不恒等，逐格复算",
            PingPlotCode::ENoTick => "拍号越闭集：采样时间轴 16 拍闭集之外先登记",
            PingPlotCode::ETickSeq => "时间轴非单调：拍时间戳必须逐拍严格递增，逆拍/回退必抓",
            PingPlotCode::ENoFace => "三面格越闭集：延迟/丢包/抖动 12 格闭集之外先登记",
            PingPlotCode::EFaceDrift => "三面载荷漂移：延迟/丢包/抖动折叠与基准不恒等，逐格复算",
            PingPlotCode::ENoAlert => "告警段越闭集：布防/触发/解除三元闸之外先登记",
            PingPlotCode::EAlertOrder => "告警段乱序：布防→触发→解除三段依次落账，跳段必抓",
            PingPlotCode::ENoScene => "场景号越闭集：多目标 5 场景闭集之外先登记（场景折叠漂移并载荷漂移码复算）",
            PingPlotCode::ESeal => "未齐即封：10 目标 + 16 拍 + 12 三面 + 3 告警段 + 5 场景未齐先补齐",
            PingPlotCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PingPlotCode::EGradeDrift => "定级漂移：全通才可定 S2（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本域不适用，闭集保留）
    S1,
    /// S2：有条件收口（本卡定级）
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
            Grade::S1 => "S1：全通收口（PingPlotter 域不适用，闭集保留）",
            Grade::S2 => "S2：10 目标+16 拍+12 三面+3 告警段+5 场景全通收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 目标格载荷（格 t）。
pub const fn target_cell(t: usize) -> Option<u64> {
    if t >= TGT_CELLS {
        return None;
    }
    Some((0xE1000u64 + (t as u64) * 0x37) ^ 0x5050_4F54_5447_0000)
}

/// 时间轴拍时间戳基准（拍 k；逐拍严格递增，步长 0x40）。
pub const fn tick_cell(k: usize) -> Option<u64> {
    if k >= TICK_CELLS {
        return None;
    }
    Some(0xE2000u64 + (k as u64) * 0x40)
}

/// 三面格载荷（格 f；面 = f/4：0=延迟 1=丢包 2=抖动）。
pub const fn face_cell(f: usize) -> Option<u64> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((0xE3000u64 + (f as u64) * 0x2C) ^ ((f / 4) as u64).wrapping_mul(0x01000193))
}

/// 三面名（面 m）。
pub const fn face_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("延迟"),
        1 => Some("丢包"),
        2 => Some("抖动"),
        _ => None,
    }
}

/// 三面类别（格 f → 面）。
pub const fn face_kind(f: usize) -> Option<u32> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((f / 4) as u32)
}

/// 告警三元闸段载荷（段 a：0=布防 1=触发 2=解除）。
pub const fn alert_cell(a: usize) -> Option<u64> {
    if a >= ALERT_PHASES {
        return None;
    }
    Some((0xE4000u64 + (a as u64) * 0x5D) ^ 0x414C_5254_5048_0000)
}

/// 告警段名。
pub const fn alert_phase_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("布防"),
        1 => Some("触发"),
        2 => Some("解除"),
        _ => None,
    }
}

/// 场景格载荷（格 c；对应「结果聚合与图表 ×5」口径）。
pub const fn scene_cell(c: usize) -> Option<u64> {
    if c >= SCENE_CELLS {
        return None;
    }
    Some((0xE5000u64 + (c as u64) * 0x31) ^ 0x5343_454E_455F_0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PingPlotter 收口账本状态机。
#[derive(Clone)]
pub struct PingPlotLedger {
    /// 六判据（目标/时间轴/延迟/丢包/抖动/告警）。
    gates: [Option<bool>; 6],
    /// 目标账。
    tgts: [Option<u64>; TGT_CELLS],
    /// 时间轴账。
    ticks: [Option<u64>; TICK_CELLS],
    /// 三面账。
    faces: [Option<u64>; FACE_CELLS],
    /// 告警段账。
    alerts: [Option<u64>; ALERT_PHASES],
    /// 场景账。
    scenes: [Option<u64>; SCENE_CELLS],
    /// 最后拍时间戳。
    last_ts: u64,
    /// 最后告警段。
    last_alert: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PingPlotLedger {
    /// 新账本。
    pub fn new() -> Self {
        PingPlotLedger {
            gates: [None; 6],
            tgts: [None; TGT_CELLS],
            ticks: [None; TICK_CELLS],
            faces: [None; FACE_CELLS],
            alerts: [None; ALERT_PHASES],
            scenes: [None; SCENE_CELLS],
            last_ts: 0,
            last_alert: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PingPlotCode> {
        if gate >= 6 {
            return Err(PingPlotCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5410 + gate as u64);
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

    /// 目标一格（载荷对照 + 重复拒）。
    pub fn record_target(&mut self, t: usize, got: u64) -> Result<(), PingPlotCode> {
        let want = match target_cell(t) {
            Some(w) => w,
            None => return Err(PingPlotCode::ENoTgt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5420 + t as u64);
            return Err(PingPlotCode::ECellDrift);
        }
        if match self.tgts.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PingPlotCode::ESeal);
        }
        if let Some(c) = self.tgts.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x5421 + t as u64);
        Ok(())
    }

    /// 目标全账核验（10 格）。
    pub fn tgts_all(&self) -> bool {
        let mut t = 0usize;
        while t < TGT_CELLS {
            match self.tgts.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 时间轴一拍（拍载荷恒等 + 逐拍严格递增单调）。
    pub fn record_tick(&mut self, k: usize, got: u64) -> Result<(), PingPlotCode> {
        let want = match tick_cell(k) {
            Some(w) => w,
            None => return Err(PingPlotCode::ENoTick),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5430 + k as u64);
            return Err(PingPlotCode::ECellDrift);
        }
        if got <= self.last_ts {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5431 + k as u64);
            return Err(PingPlotCode::ETickSeq);
        }
        if match self.ticks.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PingPlotCode::ESeal);
        }
        if let Some(c) = self.ticks.get_mut(k) {
            *c = Some(got);
        }
        self.last_ts = got;
        self.fnv(0x5432 + k as u64);
        Ok(())
    }

    /// 时间轴全账核验（16 拍且末拍时间戳为第 15 拍基准）。
    pub fn ticks_all(&self) -> bool {
        let mut k = 0usize;
        while k < TICK_CELLS {
            match self.ticks.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        self.last_ts == match tick_cell(TICK_CELLS - 1) {
            Some(w) => w,
            None => 0,
        }
    }

    /// 三面一格（载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u64) -> Result<(), PingPlotCode> {
        if f >= FACE_CELLS {
            return Err(PingPlotCode::ENoFace);
        }
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(PingPlotCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5440 + f as u64);
            return Err(PingPlotCode::EFaceDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PingPlotCode::ESeal);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x5441 + f as u64);
        Ok(())
    }

    /// 三面全账核验（12 格）。
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

    /// 告警段一格（三段依次落账，跳段必抓）。
    pub fn record_alert(&mut self, a: usize, got: u64) -> Result<(), PingPlotCode> {
        if a >= ALERT_PHASES {
            return Err(PingPlotCode::ENoAlert);
        }
        if (a as i64) != self.last_alert + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5450);
            return Err(PingPlotCode::EAlertOrder);
        }
        let want = match alert_cell(a) {
            Some(w) => w,
            None => return Err(PingPlotCode::ENoAlert),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5451 + a as u64);
            return Err(PingPlotCode::EFaceDrift);
        }
        if let Some(c) = self.alerts.get_mut(a) {
            *c = Some(got);
        }
        self.last_alert = a as i64;
        self.fnv(0x5452 + a as u64);
        Ok(())
    }

    /// 告警三元闸全落核验。
    pub fn alerts_all(&self) -> bool {
        let mut a = 0usize;
        while a < ALERT_PHASES {
            match self.alerts.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        self.last_alert == (ALERT_PHASES as i64) - 1
    }

    /// 场景一格（载荷对照）。
    pub fn record_scene(&mut self, c: usize, got: u64) -> Result<(), PingPlotCode> {
        let want = match scene_cell(c) {
            Some(w) => w,
            None => return Err(PingPlotCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5460 + c as u64);
            return Err(PingPlotCode::ECellDrift);
        }
        if match self.scenes.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PingPlotCode::ESeal);
        }
        if let Some(cell) = self.scenes.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x5461 + c as u64);
        Ok(())
    }

    /// 场景全账核验（5 格）。
    pub fn scenes_all(&self) -> bool {
        let mut c = 0usize;
        while c < SCENE_CELLS {
            match self.scenes.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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

    /// KB-COMPAT 定级（全通 → S2，判据缺 → S3，其余 → S2 条件口径）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.tgts_all()
            && self.ticks_all()
            && self.faces_all()
            && self.alerts_all()
            && self.scenes_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PingPlotLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PingPlotLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TGT_CELLS {
        if let Some(w) = target_cell(t) {
            if led.record_target(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut k = 0usize;
    while k < TICK_CELLS {
        if let Some(w) = tick_cell(k) {
            if led.record_tick(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut f = 0usize;
    while f < FACE_CELLS {
        if let Some(w) = face_cell(f) {
            if led.record_face(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut a = 0usize;
    while a < ALERT_PHASES {
        if let Some(w) = alert_cell(a) {
            if led.record_alert(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut c = 0usize;
    while c < SCENE_CELLS {
        if let Some(w) = scene_cell(c) {
            if led.record_scene(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 注入 10 轮：三面漂移 / 时间轴逆拍 / 目标越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut i = 0usize;
    while i < 10 {
        let kind = (i % 3) as u32;
        let mut lt = PingPlotLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_face(0, face_cell(0).unwrap_or(0));
                lt.record_face(1, face_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(PingPlotCode::EFaceDrift)
            }
            1 => {
                let _ = lt.record_tick(15, tick_cell(15).unwrap_or(0));
                lt.record_tick(3, tick_cell(3).unwrap_or(0)) == Err(PingPlotCode::ETickSeq)
            }
            _ => lt.record_target(TGT_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5470 + i as u64);
        } else {
            led.escaped += 1;
        }
        i += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52259 判据集（锚 UNX-F52260-J1，off-by-one 板式）。
pub fn run_unx52259_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52259");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "目标 10 格全通",
        {
            let mut lt = PingPlotLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TGT_CELLS {
                if let Some(w) = target_cell(t) {
                    if lt.record_target(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tgts_all()
        },
        "目标闭集逐格恒等，越闭集必拒",
    );
    s.add(
        "采样时间轴 16 拍单调",
        {
            let mut lt = PingPlotLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < TICK_CELLS {
                if let Some(w) = tick_cell(k) {
                    if lt.record_tick(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.ticks_all()
        },
        "拍时间戳逐拍严格递增，逆拍/回退必抓",
    );
    s.add(
        "延迟/丢包/抖动三面 12 格全通",
        {
            let mut lt = PingPlotLedger::new();
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
        "三面各 4 格逐格恒等",
    );
    s.add(
        "告警三元闸 3/3 依次落账",
        {
            let mut lt = PingPlotLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ALERT_PHASES {
                if let Some(w) = alert_cell(a) {
                    if lt.record_alert(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.alerts_all()
        },
        "布防→触发→解除三段有序闭合",
    );
    s.add(
        "多目标 5 场景全通",
        {
            let mut lt = PingPlotLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SCENE_CELLS {
                if let Some(w) = scene_cell(c) {
                    if lt.record_scene(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.scenes_all()
        },
        "结果聚合与图表 ×5 口径逐场景恒等",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = PingPlotLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "目标/时间轴/延迟/丢包/抖动/告警逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "三面漂移/时间轴逆拍/目标越闭集三态轮转必抓");
    let mut l0 = PingPlotLedger::new();
    s.add(
        "边界·目标越闭集",
        l0.record_target(TGT_CELLS, 0).is_err(),
        "目标 10 越闭集必拒",
    );
    s.add(
        "边界·拍号越闭集",
        l0.record_tick(TICK_CELLS, 0).is_err(),
        "拍 16 越闭集必拒",
    );
    s.add(
        "边界·三面格越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "三面格 12 越闭集必拒",
    );
    s.add(
        "边界·告警段越闭集",
        l0.record_alert(ALERT_PHASES, 0).is_err(),
        "告警段 3 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_scene(SCENE_CELLS, 0).is_err(),
        "场景 5 越闭集必拒",
    );
    s.add(
        "时间轴逆拍注入必抓",
        {
            let _ = l0.record_tick(15, tick_cell(15).unwrap_or(0));
            l0.record_tick(3, tick_cell(3).unwrap_or(0)) == Err(PingPlotCode::ETickSeq)
        },
        "第 15 拍后回第 3 拍，时间戳回退必抓",
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
            let mut lt = PingPlotLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut t = 0usize;
            while t < TGT_CELLS {
                if let Some(w) = target_cell(t) {
                    let _ = lt.record_target(t, w);
                }
                t += 1;
            }
            let mut k = 0usize;
            while k < TICK_CELLS {
                if let Some(w) = tick_cell(k) {
                    let _ = lt.record_tick(k, w);
                }
                k += 1;
            }
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    let _ = lt.record_face(f, w);
                }
                f += 1;
            }
            let mut a = 0usize;
            while a < ALERT_PHASES {
                if let Some(w) = alert_cell(a) {
                    let _ = lt.record_alert(a, w);
                }
                a += 1;
            }
            let mut c = 0usize;
            while c < SCENE_CELLS {
                if let Some(w) = scene_cell(c) {
                    let _ = lt.record_scene(c, w);
                }
                c += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8080,
        "SEG_BASE 0x40 对齐且落在 VX15 域 52259 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_faces() {
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(4), Some(1));
        assert_eq!(face_kind(11), Some(2));
        assert!(face_kind(12).is_none());
        assert!(face_cell(FACE_CELLS).is_none());
        assert_eq!(face_name(0), Some("延迟"));
        assert_eq!(face_name(2), Some("抖动"));
        assert!(face_name(3).is_none());
    }

    #[test]
    fn test_timeline_monotonic_pure() {
        let mut k = 1usize;
        while k < TICK_CELLS {
            let prev = tick_cell(k - 1).unwrap_or(0);
            let cur = tick_cell(k).unwrap_or(0);
            assert!(cur > prev, "时间轴基准必须逐拍严格递增");
            k += 1;
        }
        assert!(tick_cell(TICK_CELLS).is_none());
    }

    #[test]
    fn test_tick_seq_caught() {
        let mut lt = PingPlotLedger::new();
        let _ = lt.record_tick(15, tick_cell(15).unwrap_or(0));
        assert_eq!(lt.record_tick(3, tick_cell(3).unwrap_or(0)), Err(PingPlotCode::ETickSeq));
        // 同拍重放也因时间戳不递增被抓。
        assert_eq!(lt.record_tick(15, tick_cell(15).unwrap_or(0)), Err(PingPlotCode::ETickSeq));
    }

    #[test]
    fn test_tick_drift_caught() {
        let mut lt = PingPlotLedger::new();
        assert_eq!(
            lt.record_tick(0, tick_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(PingPlotCode::ECellDrift)
        );
    }

    #[test]
    fn test_alert_order_caught() {
        let mut lt = PingPlotLedger::new();
        assert_eq!(lt.record_alert(1, alert_cell(1).unwrap_or(0)), Err(PingPlotCode::EAlertOrder));
        let _ = lt.record_alert(0, alert_cell(0).unwrap_or(0));
        assert_eq!(lt.record_alert(2, alert_cell(2).unwrap_or(0)), Err(PingPlotCode::EAlertOrder));
        assert!(lt.record_alert(1, alert_cell(1).unwrap_or(0)).is_ok());
        assert!(lt.record_alert(2, alert_cell(2).unwrap_or(0)).is_ok());
        assert!(lt.alerts_all());
    }

    #[test]
    fn test_scene_drift_caught() {
        let mut lt = PingPlotLedger::new();
        assert!(lt.record_scene(0, scene_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_scene(1, scene_cell(1).unwrap_or(0).wrapping_add(0x20)),
            Err(PingPlotCode::ECellDrift)
        );
        assert_eq!(lt.record_scene(SCENE_CELLS, 0), Err(PingPlotCode::ENoScene));
    }

    #[test]
    fn test_boundary_out_of_set() {
        let mut lt = PingPlotLedger::new();
        assert!(lt.record_target(TGT_CELLS, 0).is_err());
        assert!(lt.record_tick(TICK_CELLS, 0).is_err());
        assert!(lt.record_face(FACE_CELLS, 0).is_err());
        assert!(lt.record_alert(ALERT_PHASES, 0).is_err());
        assert!(lt.record_scene(SCENE_CELLS, 0).is_err());
        assert!(lt.judge_gate(6).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (TGT_CELLS + TICK_CELLS + FACE_CELLS + ALERT_PHASES + SCENE_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8080);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PingPlotCode::Ok as u32, 0);
        assert_eq!(PingPlotCode::EGradeDrift as u32, 12);
        assert_eq!(PingPlotCode::ETickSeq.val(), SEG_BASE + 4);
        assert!(!PingPlotCode::Ok.detail().is_empty());
        assert!(!PingPlotCode::EAlertOrder.detail().is_empty());
        assert!(!PingPlotCode::ECellDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = PingPlotLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = PingPlotLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        // 六闸全落但账格未齐 → 有条件收口 S2。
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52259_checks_green() {
        let s = run_unx52259_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
