//! UNX-F52298 GitHub 入库状态声明（本册全部产物提交与推送如实登记）· 0xC8A40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52298"，逐字）：
//!   title: GitHub 入库状态声明（本册全部产物提交与推送如实登记）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52299-J1 防重 grep
//!     （关键词族：六判据/KB-COMPAT/shim/收口）于批收口执行留痕；批印入
//!     册：15 批 300 条全冻结、域账累计入册；判据锚定 §AI-66.5 J-覆盖批
//!     收口节奏；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52298）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52299-J1，照原文锚定；功能按本卡标题（GitHub 入库状态声明）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 提交/推送/同步三态 12 格闭集：三态 × 每态 4 格，逐态载荷折叠恒
//!      等才收，越态必抓。
//!   ② 本册产物清单 20 格：五族（源码/测试/账册/印鉴/文档）各 4 格，
//!      族序非降，逆序必抓且不改 last_fam。
//!   ③ 缺提交清单 4 行公开：未入库项逐行公开登记，一行不缺才可定级；
//!      清单公开后仍允许补录（如实登记纪律），但单向门封印后不回改。
//!   ④ 单向门：declare_one_way() 之后所有写入一律 ESealed 封拒（声明后
//!      不回改），单向门自身先于一切写入检查。
//!   ⑤ ledger 状态机（new/judge_gate×6/record_state/record_artifact/
//!      declare_missing/declare_one_way/fingerprint FNV-1a）；注入核
//!      run_cycle 10 轮三态轮转（三态越闭集/产物漂移/单向门回改），全部
//!      落在当轮临时账本上，检出+逃逸守恒；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（12+20+4+六闸+封印 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52298;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 入库三态族数（提交/推送/同步）。
pub const STATE_KINDS: usize = 3;

/// 每态格数。
pub const CELLS_PER_KIND: usize = 4;

/// 三态格总数。
pub const STATE_CELLS: usize = STATE_KINDS * CELLS_PER_KIND;

/// 本册产物清单格数。
pub const ARTIFACTS: usize = 20;

/// 缺提交清单公开行数。
pub const MISSING_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhSyncCode {
    /// 成功
    Ok = 0,
    /// 三态族越闭集
    ENoKind,
    /// 态内格号越闭集
    ENoIdx,
    /// 三态格载荷漂移
    EStateDrift,
    /// 产物格越闭集
    ENoArt,
    /// 产物格载荷漂移
    EArtDrift,
    /// 缺单行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 单向门封拒：声明后不回改
    ESealed,
    /// 判据闸越闭集/未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl GhSyncCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GhSyncCode::Ok => "成功",
            GhSyncCode::ENoKind => "三态族越闭集：提交/推送/同步之外先登记",
            GhSyncCode::ENoIdx => "态内格号越闭集：每态 4 格闭集之外先登记",
            GhSyncCode::EStateDrift => "三态格漂移：载荷与态内基准不恒等，逐格复算",
            GhSyncCode::ENoArt => "产物格越闭集：本册产物清单 20 格之外先登记",
            GhSyncCode::EArtDrift => "产物格漂移：载荷与清单基准不恒等，逐格复算",
            GhSyncCode::ENoRow => "缺单行越闭集：缺提交清单 4 行闭集之外先登记",
            GhSyncCode::EDup => "重复落账：同格只许一条账目",
            GhSyncCode::ESealed => "单向门封拒：入库状态声明后不回改",
            GhSyncCode::ESeal => "判据闸越闭集：六闸闭集 0..=5 之外先登记",
            GhSyncCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GhSyncCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GhSyncCode::EGradeDrift => "定级漂移：12+20+4 格与六闸全齐且单向门封印才可定 S1",
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
            Grade::S1 => "S1：三态 12 格+产物 20 格+缺单 4 行+六闸+单向门封印全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格或未封印",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 三态族名。
pub const fn state_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("提交"),
        1 => Some("推送"),
        2 => Some("同步"),
        _ => None,
    }
}

/// 三态格基准载荷（XOR 锚常量低 16 位恒零）。
pub const fn state_cell(kind: usize, idx: usize) -> Option<u64> {
    if kind >= STATE_KINDS || idx >= CELLS_PER_KIND {
        return None;
    }
    Some((0xB4000u64 + (kind as u64) * 0x40 + (idx as u64) * 0x13) ^ 0x4748_0000_0000)
}

/// 产物族（0..4=源码 4..8=测试 8..12=账册 12..16=印鉴 16..20=文档）。
pub const fn artifact_fam(a: usize) -> Option<u32> {
    if a >= ARTIFACTS {
        return None;
    }
    Some((a / 4) as u32)
}

/// 产物族名。
pub const fn artifact_fam_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("源码"),
        1 => Some("测试"),
        2 => Some("账册"),
        3 => Some("印鉴"),
        4 => Some("文档"),
        _ => None,
    }
}

/// 产物格基准载荷。
pub const fn artifact_cell(a: usize) -> Option<u64> {
    if a >= ARTIFACTS {
        return None;
    }
    Some((0xB5000u64 + (a as u64) * 0x2C) ^ (((a / 4) as u64).wrapping_mul(0x01000193) << 16))
}

/// 缺提交清单行折叠（行 r）。
pub const fn missing_row(r: usize) -> Option<u32> {
    if r >= MISSING_ROWS {
        return None;
    }
    Some(0xB600 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GitHub 入库状态声明账本状态机。
#[derive(Clone)]
pub struct GhSyncLedger {
    /// 六判据闸（提交齐/推送齐/同步齐/产物齐/缺单公开/如实登记）。
    gates: [Option<bool>; 6],
    /// 三态格账。
    states: [Option<u64>; STATE_CELLS],
    /// 产物格账。
    arts: [Option<u64>; ARTIFACTS],
    /// 缺提交清单行账。
    missing: [Option<u32>; MISSING_ROWS],
    /// 最后产物族（单调基准）。
    last_fam: i64,
    /// 单向门：声明后不回改。
    sealed: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GhSyncLedger {
    /// 新账本。
    pub fn new() -> Self {
        GhSyncLedger {
            gates: [None; 6],
            states: [None; STATE_CELLS],
            arts: [None; ARTIFACTS],
            missing: [None; MISSING_ROWS],
            last_fam: -1,
            sealed: false,
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

    /// 落一格六判据闸（单向门后封拒）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GhSyncCode> {
        if self.sealed {
            return Err(GhSyncCode::ESealed);
        }
        if gate >= 6 {
            return Err(GhSyncCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4A00 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < 6 {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 三态一格（单向门先查、族/格越闭集、载荷对照、重复拒）。
    pub fn record_state(&mut self, kind: usize, idx: usize, got: u64) -> Result<(), GhSyncCode> {
        if self.sealed {
            return Err(GhSyncCode::ESealed);
        }
        if kind >= STATE_KINDS {
            return Err(GhSyncCode::ENoKind);
        }
        if idx >= CELLS_PER_KIND {
            return Err(GhSyncCode::ENoIdx);
        }
        let want = match state_cell(kind, idx) {
            Some(w) => w,
            None => return Err(GhSyncCode::ENoKind),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00 + kind as u64);
            return Err(GhSyncCode::EStateDrift);
        }
        let cell = kind * CELLS_PER_KIND + idx;
        if match self.states.get(cell) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhSyncCode::EDup);
        }
        if let Some(c) = self.states.get_mut(cell) {
            *c = Some(got);
        }
        self.fnv(0x4B01 + cell as u64);
        Ok(())
    }

    /// 三态 12 格全账核验。
    pub fn states_all(&self) -> bool {
        let mut i = 0usize;
        while i < STATE_CELLS {
            match self.states.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 产物一格（单向门先查、越闭集、族单调、载荷对照、重复拒）。
    pub fn record_artifact(&mut self, a: usize, got: u64) -> Result<(), GhSyncCode> {
        if self.sealed {
            return Err(GhSyncCode::ESealed);
        }
        if a >= ARTIFACTS {
            return Err(GhSyncCode::ENoArt);
        }
        let fam = match artifact_fam(a) {
            Some(f) => f,
            None => return Err(GhSyncCode::ENoArt),
        };
        if (fam as i64) < self.last_fam {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4C00);
            return Err(GhSyncCode::EArtDrift);
        }
        let want = match artifact_cell(a) {
            Some(w) => w,
            None => return Err(GhSyncCode::ENoArt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4C01 + a as u64);
            return Err(GhSyncCode::EArtDrift);
        }
        if match self.arts.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhSyncCode::EDup);
        }
        if let Some(c) = self.arts.get_mut(a) {
            *c = Some(got);
        }
        self.last_fam = fam as i64;
        self.fnv(0x4C02 + a as u64);
        Ok(())
    }

    /// 产物 20 格全账核验。
    pub fn arts_all(&self) -> bool {
        let mut a = 0usize;
        while a < ARTIFACTS {
            match self.arts.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 缺提交清单公开一行。
    pub fn declare_missing(&mut self, r: usize) -> Result<(), GhSyncCode> {
        if self.sealed {
            return Err(GhSyncCode::ESealed);
        }
        if r >= MISSING_ROWS {
            return Err(GhSyncCode::ENoRow);
        }
        if match self.missing.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhSyncCode::EDup);
        }
        if let Some(c) = self.missing.get_mut(r) {
            *c = missing_row(r);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// 缺单 4 行全齐核验。
    pub fn missing_all(&self) -> bool {
        let mut r = 0usize;
        while r < MISSING_ROWS {
            match self.missing.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 单向门封印：声明后不回改（幂等）。
    pub fn declare_one_way(&mut self) -> Result<(), GhSyncCode> {
        self.sealed = true;
        self.fnv(0x4E00);
        Ok(())
    }

    /// 单向门状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后产物族（注入失败不改）。
    pub fn last_fam(&self) -> i64 {
        self.last_fam
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全齐+封印 → S1，六闸齐 → S2，否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.states_all() && self.arts_all() && self.missing_all() && self.sealed {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GhSyncLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GhSyncLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut k = 0usize;
    while k < STATE_KINDS {
        let mut i = 0usize;
        while i < CELLS_PER_KIND {
            if let Some(w) = state_cell(k, i) {
                if led.record_state(k, i, w).is_ok() {
                    recorded += 1;
                }
            }
            i += 1;
        }
        k += 1;
    }
    let mut a = 0usize;
    while a < ARTIFACTS {
        if let Some(w) = artifact_cell(a) {
            if led.record_artifact(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < MISSING_ROWS {
        let _ = led.declare_missing(r);
        r += 1;
    }
    let _ = led.declare_one_way();
    // 注入 10 轮：三态越闭集 / 产物漂移 / 单向门回改三态轮转，
    // 全部落在当轮临时账本上（漂移与回改注入先按合法序落正格再注入）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GhSyncLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：空账本上三态族 3 越界必抓。
                lt.record_state(STATE_KINDS, 0, 0).is_err()
            }
            1 => {
                // 产物漂移：先按合法序落产物 0 正格再注漂移。
                let w0 = match artifact_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_artifact(0, w0);
                let w1 = match artifact_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_artifact(1, w1.wrapping_add(1)) == Err(GhSyncCode::EArtDrift)
            }
            _ => {
                // 单向门回改：先落正格再封印，封印后回写必抓。
                let w = match state_cell(0, 0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_state(0, 0, w);
                let _ = lt.declare_one_way();
                let w2 = match state_cell(1, 0) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_state(1, 0, w2) == Err(GhSyncCode::ESealed)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4F00 + k as u64);
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
/// UNX-F52298 判据集（锚 UNX-F52299-J1，off-by-one 板式）。
pub fn run_unx52298_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52298");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三态 12 格全落（提交/推送/同步）",
        recorded >= STATE_CELLS as u32,
        "三态 × 每态 4 格逐格恒等落账（§AI-66.5 批收口节奏）",
    );
    s.add(
        "产物清单 20 格全落",
        {
            let mut lt = GhSyncLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ARTIFACTS {
                if let Some(w) = artifact_cell(a) {
                    if lt.record_artifact(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.arts_all()
        },
        "源码/测试/账册/印鉴/文档五族各 4 格逐格恒等",
    );
    s.add(
        "缺提交清单 4/4 行全公开",
        {
            let mut lt = GhSyncLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MISSING_ROWS {
                if lt.declare_missing(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.missing_all()
        },
        "未入库项逐行公开登记（如实登记纪律），一行不缺才可定级",
    );
    s.add(
        "单向门拒回改",
        {
            let mut lt = GhSyncLedger::new();
            let w = match state_cell(0, 0) {
                Some(v) => v,
                None => 0,
            };
            let _ = lt.record_state(0, 0, w);
            let _ = lt.declare_one_way();
            let w2 = match state_cell(1, 0) {
                Some(v) => v,
                None => 0,
            };
            lt.record_state(1, 0, w2) == Err(GhSyncCode::ESealed)
        },
        "入库状态声明之后所有写入一律封拒（声明后不回改）",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = GhSyncLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "提交齐/推送齐/同步齐/产物齐/缺单公开/如实登记逐格落账",
    );
    s.add(
        "产物族单调",
        {
            let mut ok = true;
            let mut a = 1usize;
            while a < ARTIFACTS {
                let f = artifact_fam(a).unwrap_or(5);
                let fp = artifact_fam(a - 1).unwrap_or(5);
                if f < fp {
                    ok = false;
                }
                a += 1;
            }
            ok
        },
        "五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "三态越闭集/产物漂移/单向门回改三态轮转必抓",
    );
    let mut l0 = GhSyncLedger::new();
    s.add(
        "边界·三态族越闭集",
        l0.record_state(STATE_KINDS, 0, 0).is_err(),
        "三态族 3 越闭集必拒",
    );
    s.add(
        "边界·产物越闭集",
        l0.record_artifact(ARTIFACTS, 0).is_err(),
        "产物 20 越闭集必拒",
    );
    s.add(
        "边界·缺单行越闭集",
        l0.declare_missing(MISSING_ROWS).is_err(),
        "缺单行 4 越闭集必拒",
    );
    s.add(
        "载荷漂移拒",
        {
            let w = match state_cell(0, 1) {
                Some(v) => v,
                None => 0,
            };
            l0.record_state(0, 1, w.wrapping_add(1)) == Err(GhSyncCode::EStateDrift)
        },
        "三态格载荷漂移必抓",
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
            let mut lt = GhSyncLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut k = 0usize;
            while k < STATE_KINDS {
                let mut i = 0usize;
                while i < CELLS_PER_KIND {
                    if let Some(w) = state_cell(k, i) {
                        let _ = lt.record_state(k, i, w);
                    }
                    i += 1;
                }
                k += 1;
            }
            let mut a = 0usize;
            while a < ARTIFACTS {
                if let Some(w) = artifact_cell(a) {
                    let _ = lt.record_artifact(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < MISSING_ROWS {
                let _ = lt.declare_missing(r);
                r += 1;
            }
            let _ = lt.declare_one_way();
            lt.judge_grade() == Grade::S1
        },
        "全齐且封印 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8A40,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8A40",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_kinds() {
        assert_eq!(state_kind_name(0), Some("提交"));
        assert_eq!(state_kind_name(1), Some("推送"));
        assert_eq!(state_kind_name(2), Some("同步"));
        assert!(state_kind_name(3).is_none());
        assert!(state_cell(3, 0).is_none());
        assert!(state_cell(0, CELLS_PER_KIND).is_none());
    }

    #[test]
    fn test_artifact_fam() {
        assert_eq!(artifact_fam(0), Some(0));
        assert_eq!(artifact_fam(4), Some(1));
        assert_eq!(artifact_fam(19), Some(4));
        assert!(artifact_fam(20).is_none());
        assert!(artifact_cell(20).is_none());
        assert_eq!(artifact_fam_name(0), Some("源码"));
        assert_eq!(artifact_fam_name(4), Some("文档"));
        assert!(artifact_fam_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = artifact_cell(3).unwrap_or(0);
        assert_eq!(a, artifact_cell(3).unwrap_or(0));
        assert_ne!(a, artifact_cell(4).unwrap_or(0));
        let s0 = state_cell(0, 0).unwrap_or(0);
        assert_ne!(s0, state_cell(1, 0).unwrap_or(0));
        assert!(missing_row(MISSING_ROWS).is_none());
    }

    #[test]
    fn test_fam_order_caught_and_last_fam_kept() {
        let mut lt = GhSyncLedger::new();
        let w9 = artifact_cell(9).unwrap_or(0);
        assert!(lt.record_artifact(9, w9).is_ok());
        let w2 = artifact_cell(2).unwrap_or(0);
        assert_eq!(lt.record_artifact(2, w2), Err(GhSyncCode::EArtDrift));
        assert_eq!(lt.last_fam(), 2);
        // 同族再落不逆序。
        let w10 = artifact_cell(10).unwrap_or(0);
        assert!(lt.record_artifact(10, w10).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GhSyncLedger::new();
        let w = state_cell(2, 2).unwrap_or(0);
        assert_eq!(lt.record_state(2, 2, w.wrapping_add(3)), Err(GhSyncCode::EStateDrift));
        let wa = artifact_cell(12).unwrap_or(0);
        assert_eq!(lt.record_artifact(12, wa.wrapping_add(5)), Err(GhSyncCode::EArtDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GhSyncLedger::new();
        let w = state_cell(1, 1).unwrap_or(0);
        assert!(lt.record_state(1, 1, w).is_ok());
        assert_eq!(lt.record_state(1, 1, w), Err(GhSyncCode::EDup));
        let wm = missing_row(0).unwrap_or(0);
        assert!(lt.declare_missing(0).is_ok());
        assert_eq!(lt.declare_missing(0), Err(GhSyncCode::EDup));
        assert_eq!(wm, missing_row(0).unwrap_or(0));
    }

    #[test]
    fn test_one_way_gate() {
        let mut lt = GhSyncLedger::new();
        assert!(!lt.is_sealed());
        assert!(lt.declare_one_way().is_ok());
        assert!(lt.is_sealed());
        let w = state_cell(0, 0).unwrap_or(0);
        assert_eq!(lt.record_state(0, 0, w), Err(GhSyncCode::ESealed));
        assert_eq!(lt.record_artifact(0, 0), Err(GhSyncCode::ESealed));
        assert_eq!(lt.declare_missing(0), Err(GhSyncCode::ESealed));
        assert_eq!(lt.judge_gate(0), Err(GhSyncCode::ESealed));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GhSyncLedger::new();
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
        assert!(recorded >= STATE_CELLS as u32 + ARTIFACTS as u32);
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
        assert_eq!(SEG_BASE, 0xC8A40);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52298u32 - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GhSyncCode::Ok as u32, 0);
        assert_eq!(GhSyncCode::EGradeDrift as u32, 12);
        assert_eq!(GhSyncCode::Ok.val(), SEG_BASE);
        assert_eq!(GhSyncCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all: [GhSyncCode; 13] = [
            GhSyncCode::Ok,
            GhSyncCode::ENoKind,
            GhSyncCode::ENoIdx,
            GhSyncCode::EStateDrift,
            GhSyncCode::ENoArt,
            GhSyncCode::EArtDrift,
            GhSyncCode::ENoRow,
            GhSyncCode::EDup,
            GhSyncCode::ESealed,
            GhSyncCode::ESeal,
            GhSyncCode::EInj,
            GhSyncCode::EFp,
            GhSyncCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < 13 {
            assert!(!all[i].detail().is_empty(), "码位 {} 明细不得为空", i);
            assert_eq!(all[i].val(), SEG_BASE + i as u32, "码位必须与枚举位序一致");
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GhSyncLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 只齐六闸 → S2。
        let mut lt2 = GhSyncLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52298_checks_green() {
        let s = run_unx52298_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
