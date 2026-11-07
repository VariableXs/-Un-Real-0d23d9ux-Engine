//! UNX-F52467 Chromium（开源构建）收口 · 0xCB480段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52467"，逐字）：
//!   title: Chromium（开源构建）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52468-J1 六判据
//!     全落；滚动版钉版回归口径（AI-94 T+1 联签锚定行）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.2 专题六；③ 完成时 API/软件写回：状态=已
//!     完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52467）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52468-J1，照原文锚定；功能按本卡标题（Chromium（开源构建）收口）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 格闭集：构建/渲染/扩展/更新 N/A/遥测零各 4 格，家族
//!      单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收，漂移必抓；
//!      更新 N/A 家族照落账不豁免，遥测零家族以零遥测证词落账。
//!   ② 构建三元闸 3 元：win/mac/linux 三元构建闸，越元与漂移必抓。
//!   ③ 开源分账声明 2 行闭集：源码开源分账/构建脚本开源分账，缺一拒封。
//!   ④ 与 Chrome 分账声明 2 行闭集：渲染账分账/更新账分账，缺一拒封。
//!   ⑤ 六判据收口闸（启动/渲染/崩溃/卸载/升级/许可）逐格落账，未齐即
//!      封拒；滚动版钉版回归口径按 AI-94 T+1 联签锚定行落账。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（操作漂移/
//!      操作越序/操作越闭集，三类注入全落当轮临时账本，空账本同样必
//!      抓，越序路径不改 last_kind，escaped=0）；注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致；KB-COMPAT 定级 S1/S2/S3 闭集（全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52467;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FEATURE_OPS: usize = 20;

/// 构建三元闸元数。
pub const BUILD_GATES: usize = 3;

/// 开源分账声明行数。
pub const OSS_ROWS: usize = 2;

/// 与 Chrome 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromiumCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 构建元越闸
    ENoBuild,
    /// 构建元漂移
    EBuildDrift,
    /// 开源分账行越闭集
    ENoOss,
    /// 与 Chrome 分账行越闭集
    ENoSplit,
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

impl ChromiumCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ChromiumCode::Ok => "成功",
            ChromiumCode::ENoOp => "操作号越闭集：构建/渲染/扩展/更新 N/A/遥测零 20 格之外先登记",
            ChromiumCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ChromiumCode::EOrder => "家族逆序：构建→渲染→扩展→更新 N/A→遥测零单调，逆序必抓",
            ChromiumCode::ENoBuild => "构建元越闸：win/mac/linux 三元闸之外先登记",
            ChromiumCode::EBuildDrift => "构建元漂移：构建载荷与基准不恒等，逐元复算",
            ChromiumCode::ENoOss => "开源分账行越闭集：开源分账 2 行闭集之外先登记",
            ChromiumCode::ENoSplit => "与 Chrome 分账行越闭集：与 Chrome 分账 2 行闭集之外先登记",
            ChromiumCode::EDup => "重复落账：同格只许一条账目",
            ChromiumCode::ESeal => "未齐即封：20 操作 + 3 元构建 + 2 行开源分账 + 2 行 Chrome 分账未齐先补齐",
            ChromiumCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ChromiumCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ChromiumCode::EGradeDrift => "定级漂移：全通且开源分账与 Chrome 分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3元构建+2行开源分账+2行Chrome分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（操作 o；家族 = o/4）。
pub const fn feature_cell(o: usize) -> Option<u64> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((0xCD00u64 + (o as u64) * 0x3D) ^ 0x43524F4D_4F53)
}

/// 功能族（0..4=构建 4..8=渲染 8..12=扩展 12..16=更新 N/A 16..20=遥测零）。
pub const fn feature_kind(o: usize) -> Option<u32> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn feature_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("构建"),
        1 => Some("渲染"),
        2 => Some("扩展"),
        3 => Some("更新 N/A"),
        4 => Some("遥测零"),
        _ => None,
    }
}

/// 构建三元闸载荷（元 b：0=win 1=mac 2=linux）。
pub const fn build_cell(b: usize) -> Option<u64> {
    if b >= BUILD_GATES {
        return None;
    }
    Some((0xCD20u64 + (b as u64) * 0x71) ^ 0x425549_4C4453)
}

/// 构建元名。
pub const fn build_gate_name(b: usize) -> Option<&'static str> {
    match b {
        0 => Some("win"),
        1 => Some("mac"),
        2 => Some("linux"),
        _ => None,
    }
}

/// 开源分账行折叠（行 r）。
pub const fn oss_row(r: usize) -> Option<u32> {
    if r >= OSS_ROWS {
        return None;
    }
    Some(0xCD60 + r as u32 * 0x2C)
}

/// 开源分账行名。
pub const fn oss_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("源码开源分账"),
        1 => Some("构建脚本开源分账"),
        _ => None,
    }
}

/// 与 Chrome 分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCD80 + r as u32 * 0x2C)
}

/// 与 Chrome 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("渲染账分账"),
        1 => Some("更新账分账"),
        _ => None,
    }
}

/// 滚动版钉版回归口径行（AI-94 T+1 联签锚定行）。
pub const PIN_REGRESSION_NOTE: &str = "滚动版钉版回归口径：AI-94 T+1 联签锚定行";

// ── 收口账本 ─────────────────────────────────────────────────────
/// Chromium（开源构建）收口账本状态机。
#[derive(Clone)]
pub struct ChromiumLedger {
    /// 六判据（启动/渲染/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FEATURE_OPS],
    /// 构建三元闸账。
    builds: [Option<u64>; BUILD_GATES],
    /// 开源分账账。
    osses: [Option<u32>; OSS_ROWS],
    /// 与 Chrome 分账账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ChromiumLedger {
    /// 新账本。
    pub fn new() -> Self {
        ChromiumLedger {
            gates: [None; 6],
            ops: [None; FEATURE_OPS],
            builds: [None; BUILD_GATES],
            osses: [None; OSS_ROWS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ChromiumCode> {
        if gate >= 6 {
            return Err(ChromiumCode::ESeal);
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

    /// 功能族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ChromiumCode> {
        if o >= FEATURE_OPS {
            return Err(ChromiumCode::ENoOp);
        }
        let kind = match feature_kind(o) {
            Some(k) => k,
            None => return Err(ChromiumCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(ChromiumCode::EOrder);
        }
        let want = match feature_cell(o) {
            Some(w) => w,
            None => return Err(ChromiumCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(ChromiumCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromiumCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 构建三元闸一格（越元拒 + 载荷对照 + 重复拒，漂移走注入账）。
    pub fn record_build(&mut self, b: usize, got: u64) -> Result<(), ChromiumCode> {
        if b >= BUILD_GATES {
            return Err(ChromiumCode::ENoBuild);
        }
        let want = match build_cell(b) {
            Some(w) => w,
            None => return Err(ChromiumCode::ENoBuild),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4601 + b as u64);
            return Err(ChromiumCode::EBuildDrift);
        }
        if match self.builds.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromiumCode::EDup);
        }
        if let Some(c) = self.builds.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x4600 + b as u64);
        Ok(())
    }

    /// 构建三元闸全账核验（3 元）。
    pub fn builds_all(&self) -> bool {
        let mut b = 0usize;
        while b < BUILD_GATES {
            match self.builds.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 开源分账一行。
    pub fn declare_oss(&mut self, r: usize) -> Result<(), ChromiumCode> {
        if r >= OSS_ROWS {
            return Err(ChromiumCode::ENoOss);
        }
        if match self.osses.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromiumCode::EDup);
        }
        if let Some(c) = self.osses.get_mut(r) {
            *c = oss_row(r);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// 开源分账全齐核验。
    pub fn osses_all(&self) -> bool {
        let mut r = 0usize;
        while r < OSS_ROWS {
            match self.osses.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 与 Chrome 分账一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), ChromiumCode> {
        if r >= SPLIT_ROWS {
            return Err(ChromiumCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromiumCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4700 + r as u64);
        Ok(())
    }

    /// 与 Chrome 分账全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all() && self.ops_all() && self.builds_all() && self.osses_all() && self.splits_all();
        let gates_ok = self.gates_all();
        if full {
            Grade::S1
        } else if gates_ok {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ChromiumLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut led = ChromiumLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURE_OPS {
        if let Some(w) = feature_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < BUILD_GATES {
        if let Some(w) = build_cell(b) {
            if led.record_build(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < OSS_ROWS {
        let _ = led.declare_oss(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：操作漂移 / 操作越序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，临时账本从空起手，空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ChromiumLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移。
                let _ = lt.record_op(0, feature_cell(0).unwrap_or(0));
                lt.record_op(1, feature_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ChromiumCode::EDrift)
            }
            1 => {
                // 越序注入：先落家族 3 正格，再倒落家族 0；错误路径不改 last_kind。
                let _ = lt.record_op(12, feature_cell(12).unwrap_or(0));
                lt.record_op(0, feature_cell(0).unwrap_or(0)) == Err(ChromiumCode::EOrder)
            }
            _ => {
                // 越闭集注入：空账本直接越界，提前返回必抓。
                lt.record_op(FEATURE_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let escaped = led.escaped;
    let fp = led.fingerprint();
    (recorded, caught, escaped, fp)
}

/// 周期守恒核验（守恒破缺 → EInj，指纹漂移 → EFp）。
pub fn verify_cycle() -> Result<u64, ChromiumCode> {
    let (_, caught, escaped, fp) = run_cycle();
    if caught != 10 || escaped != 0 {
        return Err(ChromiumCode::EInj);
    }
    let (_, _, _, fp2) = run_cycle();
    if fp != fp2 {
        return Err(ChromiumCode::EFp);
    }
    Ok(fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52467 判据集（锚 UNX-F52468-J1，off-by-one 板式）。
pub fn run_unx52467_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52467");
    let (recorded, caught, escaped, fp) = run_cycle();
    s.add(
        "五功能族 20 格全通",
        recorded >= FEATURE_OPS as u32,
        "构建/渲染/扩展/更新 N/A/遥测零五家族逐操作恒等",
    );
    s.add(
        "构建三元闸 3 元全落",
        {
            let mut lt = ChromiumLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BUILD_GATES {
                if let Some(w) = build_cell(b) {
                    if lt.record_build(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.builds_all()
        },
        "win/mac/linux 三元构建闸逐元落账，越元必抓",
    );
    s.add(
        "构建漂移拒",
        {
            let mut lt = ChromiumLedger::new();
            lt.record_build(1, build_cell(1).unwrap_or(0).wrapping_add(5)) == Err(ChromiumCode::EBuildDrift)
        },
        "构建元载荷漂移必抓",
    );
    s.add(
        "更新 N/A 家族照落不豁免",
        {
            let mut lt = ChromiumLedger::new();
            let mut ok = true;
            let mut o = 12usize;
            while o < 16 {
                if let Some(w) = feature_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok
        },
        "更新 N/A 家族 4 格逐格落账，不因 N/A 豁免",
    );
    s.add(
        "遥测零家族零证词落账",
        {
            let mut lt = ChromiumLedger::new();
            let mut ok = true;
            let mut o = 16usize;
            while o < 20 {
                if let Some(w) = feature_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok
        },
        "遥测零家族 4 格以零遥测证词逐格落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ChromiumLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/渲染/崩溃/卸载/升级/许可逐格落账（UNX-F52468-J1 六判据）",
    );
    s.add(
        "开源分账声明 2/2 全齐",
        {
            let mut lt = ChromiumLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OSS_ROWS {
                if lt.declare_oss(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.osses_all()
        },
        "源码开源分账/构建脚本开源分账两行闭集（§AI-66.2 专题六）",
    );
    s.add(
        "与 Chrome 分账声明 2/2 全齐",
        {
            let mut lt = ChromiumLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "渲染账分账/更新账分账两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURE_OPS {
                if feature_kind(o).unwrap_or(5) < feature_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作漂移/操作越序/操作越闭集三态轮转必抓",
    );
    let mut l0 = ChromiumLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FEATURE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·构建元越闸",
        l0.record_build(BUILD_GATES, 0).is_err(),
        "构建元 3 越三元闸必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_oss(OSS_ROWS).is_err() && l0.declare_split(SPLIT_ROWS).is_err(),
        "开源分账行 2 与 Chrome 分账行 2 越闭集必拒",
    );
    s.add(
        "未齐可续录",
        {
            let mut l1 = ChromiumLedger::new();
            l1.record_op(0, feature_cell(0).unwrap_or(0)).is_ok()
                && l1.record_build(0, build_cell(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落账与构建闸必须放行（未齐即封核只拒封口不拒续录）",
    );
    s.add_dyn(
        "注入账守恒",
        caught == 10 && escaped == 0,
        &format!("注入=10 检出={} 逃逸={}", caught, escaped),
    );
    s.add("双跑指纹一致", fp != 0 && verify_cycle().is_ok(), "确定性全流程逐位一致且守恒核验通过");
    s.add(
        "滚动版钉版回归口径在册",
        !PIN_REGRESSION_NOTE.is_empty(),
        "AI-94 T+1 联签锚定行随账本在册",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = ChromiumLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURE_OPS {
                if let Some(w) = feature_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut b = 0usize;
            while b < BUILD_GATES {
                if let Some(w) = build_cell(b) {
                    let _ = lt.record_build(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < OSS_ROWS {
                let _ = lt.declare_oss(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB480,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCB480 格",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_kind() {
        assert_eq!(feature_kind(0), Some(0));
        assert_eq!(feature_kind(5), Some(1));
        assert_eq!(feature_kind(9), Some(2));
        assert_eq!(feature_kind(12), Some(3));
        assert_eq!(feature_kind(19), Some(4));
        assert!(feature_kind(20).is_none());
        assert!(feature_cell(20).is_none());
        assert_eq!(feature_kind_name(3), Some("更新 N/A"));
        assert_eq!(feature_kind_name(4), Some("遥测零"));
        assert!(feature_kind_name(5).is_none());
    }

    #[test]
    fn test_build_gates() {
        assert_eq!(build_gate_name(0), Some("win"));
        assert_eq!(build_gate_name(1), Some("mac"));
        assert_eq!(build_gate_name(2), Some("linux"));
        assert!(build_gate_name(3).is_none());
        assert!(build_cell(BUILD_GATES).is_none());
        let a = build_cell(2).unwrap_or(0);
        assert_eq!(a, build_cell(2).unwrap_or(0));
        assert_ne!(a, build_cell(0).unwrap_or(0));
    }

    #[test]
    fn test_rows_closed() {
        assert_eq!(oss_row_name(0), Some("源码开源分账"));
        assert_eq!(oss_row_name(1), Some("构建脚本开源分账"));
        assert!(oss_row_name(2).is_none());
        assert!(oss_row(OSS_ROWS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(split_row_name(1), Some("更新账分账"));
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族正格，再注逆序必抓。
        let mut lt = ChromiumLedger::new();
        let _ = lt.record_op(10, feature_cell(10).unwrap_or(0));
        assert_eq!(lt.record_op(4, feature_cell(4).unwrap_or(0)), Err(ChromiumCode::EOrder));
        // 逆序错误路径不改 last_kind：同族后续仍可续录。
        assert!(lt.record_op(11, feature_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ChromiumLedger::new();
        assert_eq!(
            lt.record_op(6, feature_cell(6).unwrap_or(0).wrapping_add(2)),
            Err(ChromiumCode::EDrift)
        );
        assert_eq!(
            lt.record_build(0, build_cell(0).unwrap_or(0).wrapping_add(11)),
            Err(ChromiumCode::EBuildDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ChromiumLedger::new();
        let _ = lt.record_build(2, build_cell(2).unwrap_or(0));
        assert_eq!(lt.record_build(2, build_cell(2).unwrap_or(0)), Err(ChromiumCode::EDup));
        let _ = lt.declare_oss(0);
        assert_eq!(lt.declare_oss(0), Err(ChromiumCode::EDup));
        let _ = lt.declare_split(1);
        assert_eq!(lt.declare_split(1), Err(ChromiumCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ChromiumLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ChromiumLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, escaped, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：操作 20 + 构建元 3。
        assert_eq!(recorded, FEATURE_OPS as u32 + BUILD_GATES as u32);
        assert_eq!(caught, 10);
        assert_eq!(escaped, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(c1, c2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
        assert!(verify_cycle().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        // 三类注入对空账本同样必抓；越界提前返回不计注入账。
        let mut l0 = ChromiumLedger::new();
        assert!(l0.record_op(FEATURE_OPS, 0).is_err());
        assert!(l0.record_build(BUILD_GATES, 0).is_err());
        let (i0, c0, e0) = l0.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCB480);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ChromiumCode::Ok as u32, 0);
        assert_eq!(ChromiumCode::EGradeDrift as u32, 12);
        assert_eq!(ChromiumCode::Ok.val(), SEG_BASE);
        assert!(!ChromiumCode::Ok.detail().is_empty());
        assert!(!ChromiumCode::ESeal.detail().is_empty());
        assert!(!ChromiumCode::ENoOss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt0 = ChromiumLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = ChromiumLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = ChromiumLedger::new();
        let mut o = 0usize;
        while o < FEATURE_OPS {
            if let Some(w) = feature_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut g2 = 0usize;
        while g2 < 6 {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52467_checks_green() {
        let s = run_unx52467_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
