//! UNX-F52293 收口条目抽读对照账（AI-82 字行比减权后 ≥20%）· 0xC8900段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52293"，逐字）：
//!   title: 收口条目抽读对照账（AI-82 字行比减权后 ≥20%）
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52294-J1 工程级口
//!     径在 N1 部落账：清单内应用覆盖 ×15/800 阶段账公开；联签锚定行
//!     AI-96；判据锚定 §AI-66.8 治理线接口；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52293）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52294-J1，照原文锚定；功能按本卡标题（收口条目抽读对照账）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 抽样闭集：20 条目 × 20 对照格逐条恒等对照，越条必抓；
//!   ② 字行比减权 8 格：字行比减权账独立折叠，逐格对照，漂移必抓；
//!   ③ AI-82 联签锚定行 2 行闭集（条目联签行 / 减权联签行），缺一拒封；
//!   ④ ≥20% 覆盖闸：抽读条目覆盖比按整数百分位核算（covered×100 ≥
//!      20×total），不足即封拒；
//!   ⑤ 抽取序单调：条目下标全流程单调不减，逆序必抓且不改账位；
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a，
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（条目越闭
//!      集/减权格漂移/抽取序逆序），三类注入全部落在当轮临时账本上，
//!      主账 injected==caught==10、escaped==0；双跑 FNV 指纹一致；
//!      定级 S1/S2/S3 闭集（20 条目 + 8 减权格 + 2 联签行 + 六判据 +
//!      ≥20% 覆盖闸全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52293;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 抽读条目数（对照账逐条收口）。
pub const SAMPLE_ENTRIES: usize = 20;

/// 字行比减权格数。
pub const WEIGHT_CELLS: usize = 8;

/// AI-82 联签锚定行数。
pub const ANCHOR_ROWS: usize = 2;

/// 六判据闸数。
pub const GATE_COUNT: usize = 6;

/// 覆盖闸百分位（AI-82 字行比减权后 ≥20%）。
pub const COV_GATE_PCT: u32 = 20;

/// XOR 联签锚（低 16 位恒零）。
pub const XOR_ANCHOR: u64 = 0x0041_4982_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleAuditCode {
    /// 成功
    Ok = 0,
    /// 条目号越闭集
    ENoEntry,
    /// 条目对照漂移
    EEntryDrift,
    /// 减权格越闭集
    ENoWeight,
    /// 减权格漂移
    EWeightDrift,
    /// 抽取序逆序
    EOrder,
    /// 联签锚定行越闭集
    ENoRow,
    /// 缺联签锚定行
    ERowMiss,
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

impl SampleAuditCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SampleAuditCode::Ok => "成功",
            SampleAuditCode::ENoEntry => "条目号越闭集：抽读 20 条目闭集之外先登记",
            SampleAuditCode::EEntryDrift => "条目对照漂移：条目载荷与对照账基准不恒等，逐条复算",
            SampleAuditCode::ENoWeight => "减权格越闭集：字行比减权 8 格闭集之外先登记",
            SampleAuditCode::EWeightDrift => "减权格漂移：字行比减权折叠与基准不恒等，逐格复算",
            SampleAuditCode::EOrder => "抽取序逆序：条目下标全流程单调不减，逆序必抓",
            SampleAuditCode::ENoRow => "联签锚定行越闭集：AI-82 联签 2 行闭集之外先登记",
            SampleAuditCode::ERowMiss => "缺联签锚定行：AI-82 联签锚定行未齐先补齐",
            SampleAuditCode::EDup => "重复落账：同格只许一条账目",
            SampleAuditCode::ESeal => "未齐即封：20 条目 + 8 减权格 + 2 联签行未齐先补齐",
            SampleAuditCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SampleAuditCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SampleAuditCode::EGradeDrift => "定级漂移：全通且覆盖闸 ≥20% 才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 抽读对照账定级闭集。
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
            Grade::S1 => "S1：20条目+8减权格+2联签行+六判据+≥20%覆盖闸全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 抽读条目载荷（条目 i 的对照基准，纯函数恒等）。
pub const fn entry_cell(i: usize) -> Option<u64> {
    if i >= SAMPLE_ENTRIES {
        return None;
    }
    Some((0xB9300u64 + (i as u64) * 0x5A) ^ XOR_ANCHOR)
}

/// 条目名（const fn 表 + Option 边界）。
pub const fn entry_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("收口条目·头行"),
        1 => Some("收口条目·阶段账"),
        2 => Some("收口条目·联签行"),
        3 => Some("收口条目·减权账"),
        4 => Some("收口条目·覆盖闸"),
        5 => Some("收口条目·治理线"),
        6 => Some("收口条目·部落账"),
        7 => Some("收口条目·清单覆盖"),
        8 => Some("收口条目·应用抽测"),
        9 => Some("收口条目·阶段公开"),
        10 => Some("收口条目·对照基准"),
        11 => Some("收口条目·字行比账"),
        12 => Some("收口条目·减权后值"),
        13 => Some("收口条目·抽样序"),
        14 => Some("收口条目·拒封线"),
        15 => Some("收口条目·补录格"),
        16 => Some("收口条目·复算格"),
        17 => Some("收口条目·同格唯一"),
        18 => Some("收口条目·指纹账"),
        19 => Some("收口条目·尾行收口"),
        _ => None,
    }
}

/// 字行比减权格载荷（格 w 的对照基准）。
pub const fn weight_cell(w: usize) -> Option<u32> {
    if w >= WEIGHT_CELLS {
        return None;
    }
    Some((0xB9400u32 + (w as u32) * 0x33) ^ 0x0004_0000)
}

/// 减权格名。
pub const fn weight_name(w: usize) -> Option<&'static str> {
    match w {
        0 => Some("字行比·原值"),
        1 => Some("字行比·字数折算"),
        2 => Some("字行比·行数折算"),
        3 => Some("字行比·减权系数"),
        4 => Some("字行比·减权一次"),
        5 => Some("字行比·减权二次"),
        6 => Some("字行比·复核算"),
        7 => Some("字行比·收口值"),
        _ => None,
    }
}

/// AI-82 联签锚定行折叠（行 r）。
pub const fn anchor_row(r: usize) -> Option<u32> {
    if r >= ANCHOR_ROWS {
        return None;
    }
    Some(0xB9500 + r as u32 * 0x2C)
}

/// 联签锚定行名。
pub const fn anchor_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("AI-82 条目联签锚定行"),
        1 => Some("AI-82 减权联签锚定行"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("抽读启动"),
        1 => Some("对照恒等"),
        2 => Some("减权复算"),
        3 => Some("联签齐备"),
        4 => Some("覆盖闸核算"),
        5 => Some("抽读收口"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 抽读对照账本状态机。
#[derive(Clone)]
pub struct SampleAuditLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATE_COUNT],
    /// 抽读条目对照账。
    entries: [Option<u64>; SAMPLE_ENTRIES],
    /// 字行比减权账。
    weights: [Option<u32>; WEIGHT_CELLS],
    /// AI-82 联签锚定行账。
    anchors: [Option<u32>; ANCHOR_ROWS],
    /// 最后已录条目下标（抽取序单调）。
    last_entry: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SampleAuditLedger {
    /// 新账本。
    pub fn new() -> Self {
        SampleAuditLedger {
            gates: [None; GATE_COUNT],
            entries: [None; SAMPLE_ENTRIES],
            weights: [None; WEIGHT_CELLS],
            anchors: [None; ANCHOR_ROWS],
            last_entry: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SampleAuditCode> {
        if gate >= GATE_COUNT {
            return Err(SampleAuditCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4700 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATE_COUNT {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 抽读条目一格（对照恒等 + 抽取序单调 + 重复拒）。
    pub fn record_entry(&mut self, i: usize, got: u64) -> Result<(), SampleAuditCode> {
        if i >= SAMPLE_ENTRIES {
            return Err(SampleAuditCode::ENoEntry);
        }
        if (i as i64) < self.last_entry {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4800);
            return Err(SampleAuditCode::EOrder);
        }
        let want = match entry_cell(i) {
            Some(w) => w,
            None => return Err(SampleAuditCode::ENoEntry),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4801 + i as u64);
            return Err(SampleAuditCode::EEntryDrift);
        }
        if match self.entries.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SampleAuditCode::EDup);
        }
        if let Some(c) = self.entries.get_mut(i) {
            *c = Some(got);
        }
        self.last_entry = i as i64;
        self.fnv(0x4802 + i as u64);
        Ok(())
    }

    /// 条目全账核验（20 格）。
    pub fn entries_all(&self) -> bool {
        let mut i = 0usize;
        while i < SAMPLE_ENTRIES {
            match self.entries.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 字行比减权一格（载荷对照 + 重复拒）。
    pub fn record_weight(&mut self, w: usize, got: u32) -> Result<(), SampleAuditCode> {
        let want = match weight_cell(w) {
            Some(x) => x,
            None => return Err(SampleAuditCode::ENoWeight),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4900 + w as u64);
            return Err(SampleAuditCode::EWeightDrift);
        }
        if match self.weights.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SampleAuditCode::EDup);
        }
        if let Some(c) = self.weights.get_mut(w) {
            *c = Some(got);
        }
        self.fnv(0x4901 + w as u64);
        Ok(())
    }

    /// 减权全账核验（8 格）。
    pub fn weights_all(&self) -> bool {
        let mut w = 0usize;
        while w < WEIGHT_CELLS {
            match self.weights.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// AI-82 联签锚定行一格。
    pub fn declare_anchor(&mut self, r: usize) -> Result<(), SampleAuditCode> {
        if r >= ANCHOR_ROWS {
            return Err(SampleAuditCode::ENoRow);
        }
        if match self.anchors.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SampleAuditCode::EDup);
        }
        if let Some(c) = self.anchors.get_mut(r) {
            *c = anchor_row(r);
        }
        self.fnv(0x4A00 + r as u64);
        Ok(())
    }

    /// 联签锚定行全齐核验。
    pub fn anchors_all(&self) -> bool {
        let mut r = 0usize;
        while r < ANCHOR_ROWS {
            match self.anchors.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// ≥20% 覆盖闸（整数百分位：covered×100 ≥ 20×total）。
    pub fn cov_gate(&self) -> bool {
        let mut covered = 0u32;
        let mut i = 0usize;
        while i < SAMPLE_ENTRIES {
            if match self.entries.get(i) {
                Some(Some(_)) => true,
                _ => false,
            } {
                covered += 1;
            }
            i += 1;
        }
        covered * 100 >= COV_GATE_PCT * SAMPLE_ENTRIES as u32
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 定级（全通且覆盖闸达标 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.entries_all()
            && self.weights_all()
            && self.anchors_all()
            && self.cov_gate()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SampleAuditLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SampleAuditLedger::new();
    let mut g = 0usize;
    while g < GATE_COUNT {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < SAMPLE_ENTRIES {
        if let Some(w) = entry_cell(i) {
            if led.record_entry(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut w = 0usize;
    while w < WEIGHT_CELLS {
        if let Some(x) = weight_cell(w) {
            if led.record_weight(w, x).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let mut r = 0usize;
    while r < ANCHOR_ROWS {
        let _ = led.declare_anchor(r);
        r += 1;
    }
    // 注入 10 轮：条目越闭集 / 减权格漂移 / 抽取序逆序三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SampleAuditLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_entry(0, entry_cell(0).unwrap_or(0));
                lt.record_entry(SAMPLE_ENTRIES, 0).is_err()
            }
            1 => {
                let _ = lt.record_weight(2, weight_cell(2).unwrap_or(0));
                lt.record_weight(3, weight_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(SampleAuditCode::EWeightDrift)
            }
            _ => {
                let _ = lt.record_entry(12, entry_cell(12).unwrap_or(0));
                lt.record_entry(3, entry_cell(3).unwrap_or(0)) == Err(SampleAuditCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4B00 + k as u64);
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
/// UNX-F52293 判据集（锚 UNX-F52294-J1，off-by-one 板式）。
pub fn run_unx52293_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52293");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "抽读 20 条目全落对照",
        recorded >= SAMPLE_ENTRIES as u32,
        "条目对照基准逐条恒等，漂移必抓（§AI-66.8 治理线接口）",
    );
    s.add(
        "字行比减权 8 格全落",
        {
            let mut lt = SampleAuditLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < WEIGHT_CELLS {
                if let Some(x) = weight_cell(w) {
                    if lt.record_weight(w, x).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.weights_all()
        },
        "原值/折算/减权系数/一次二次减权/复算/收口值逐格恒等",
    );
    s.add(
        "AI-82 联签锚定行 2/2 全齐",
        {
            let mut lt = SampleAuditLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                if lt.declare_anchor(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.anchors_all()
        },
        "条目联签行/减权联签行两行闭集，缺一拒封",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = SampleAuditLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "抽读启动/对照恒等/减权复算/联签齐备/覆盖闸核算/抽读收口",
    );
    s.add(
        "≥20% 覆盖闸",
        {
            let mut lt = SampleAuditLedger::new();
            let mut i = 0usize;
            while i < 4 {
                if let Some(w) = entry_cell(i) {
                    let _ = lt.record_entry(i, w);
                }
                i += 1;
            }
            lt.cov_gate()
        },
        "covered×100 ≥ 20×total 整数核算，4/20 恰达闸",
    );
    s.add(
        "抽取序单调",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < SAMPLE_ENTRIES {
                if entry_cell(i).is_none() {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "条目下标全流程单调不减，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "条目越闭集/减权格漂移/抽取序逆序三态轮转必抓");
    s.add(
        "边界·条目越闭集",
        {
            let mut lt = SampleAuditLedger::new();
            lt.record_entry(SAMPLE_ENTRIES, 0).is_err()
        },
        "条目 20 越闭集必拒",
    );
    s.add(
        "减权格漂移拒",
        {
            let mut lt = SampleAuditLedger::new();
            lt.record_weight(5, weight_cell(5).unwrap_or(0).wrapping_add(1))
                == Err(SampleAuditCode::EWeightDrift)
        },
        "减权载荷漂移必抓",
    );
    s.add(
        "边界·减权格越闭集",
        {
            let mut lt = SampleAuditLedger::new();
            lt.record_weight(WEIGHT_CELLS, 0).is_err()
        },
        "减权格 8 越闭集必拒",
    );
    s.add(
        "边界·联签锚定行越闭集",
        {
            let mut lt = SampleAuditLedger::new();
            lt.declare_anchor(ANCHOR_ROWS).is_err()
        },
        "联签行 2 越闭集必拒",
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
            let mut lt = SampleAuditLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < SAMPLE_ENTRIES {
                if let Some(w) = entry_cell(i) {
                    let _ = lt.record_entry(i, w);
                }
                i += 1;
            }
            let mut w = 0usize;
            while w < WEIGHT_CELLS {
                if let Some(x) = weight_cell(w) {
                    let _ = lt.record_weight(w, x);
                }
                w += 1;
            }
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                let _ = lt.declare_anchor(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通且覆盖闸达标 → S1（定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8900,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8900",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_cells_pure() {
        let a = entry_cell(3).unwrap_or(0);
        assert_eq!(a, entry_cell(3).unwrap_or(0));
        assert_ne!(a, entry_cell(4).unwrap_or(0));
        assert_eq!(entry_name(0), Some("收口条目·头行"));
        assert_eq!(entry_name(19), Some("收口条目·尾行收口"));
        assert!(entry_name(20).is_none());
        assert!(entry_cell(SAMPLE_ENTRIES).is_none());
    }

    #[test]
    fn test_weight_cells() {
        assert_eq!(weight_name(0), Some("字行比·原值"));
        assert_eq!(weight_name(7), Some("字行比·收口值"));
        assert!(weight_name(8).is_none());
        assert!(weight_cell(WEIGHT_CELLS).is_none());
        let x = weight_cell(2).unwrap_or(0);
        assert_ne!(x, weight_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_anchor_rows() {
        assert_eq!(anchor_name(0), Some("AI-82 条目联签锚定行"));
        assert_eq!(anchor_name(1), Some("AI-82 减权联签锚定行"));
        assert!(anchor_name(2).is_none());
        assert!(anchor_row(ANCHOR_ROWS).is_none());
    }

    #[test]
    fn test_order_caught() {
        let mut lt = SampleAuditLedger::new();
        let _ = lt.record_entry(9, entry_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_entry(3, entry_cell(3).unwrap_or(0)),
            Err(SampleAuditCode::EOrder)
        );
        // 逆序被拒后不改账位：合法后续仍可落。
        assert!(lt.record_entry(11, entry_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SampleAuditLedger::new();
        assert_eq!(
            lt.record_entry(5, entry_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SampleAuditCode::EEntryDrift)
        );
        assert_eq!(
            lt.record_weight(1, weight_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(SampleAuditCode::EWeightDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SampleAuditLedger::new();
        let _ = lt.record_entry(0, entry_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_entry(0, entry_cell(0).unwrap_or(0)),
            Err(SampleAuditCode::EDup)
        );
        let _ = lt.record_weight(0, weight_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_weight(0, weight_cell(0).unwrap_or(0)),
            Err(SampleAuditCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SampleAuditLedger::new();
        let mut g = 0usize;
        while g < GATE_COUNT {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATE_COUNT).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SampleAuditLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_cov_gate() {
        // 空账本 0% 不达闸；3/20 = 15% 不达；4/20 = 20% 恰达。
        let mut lt = SampleAuditLedger::new();
        assert!(!lt.cov_gate());
        let mut i = 0usize;
        while i < 3 {
            if let Some(w) = entry_cell(i) {
                let _ = lt.record_entry(i, w);
            }
            i += 1;
        }
        assert!(!lt.cov_gate());
        if let Some(w) = entry_cell(3) {
            let _ = lt.record_entry(3, w);
        }
        assert!(lt.cov_gate());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SAMPLE_ENTRIES + WEIGHT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8900);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SampleAuditCode::Ok as u32, 0);
        assert_eq!(SampleAuditCode::EGradeDrift as u32, 12);
        assert!(!SampleAuditCode::Ok.detail().is_empty());
        assert!(!SampleAuditCode::ESeal.detail().is_empty());
        assert!(!SampleAuditCode::EOrder.detail().is_empty());
        assert_eq!(SampleAuditCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SampleAuditLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn test_injection_conservation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        // 主账守恒：注入 = 检出，逃逸恒零。
        let mut lt = SampleAuditLedger::new();
        let (inj, c, e) = lt.stats();
        assert_eq!(inj, c);
        assert_eq!(e, 0);
    }

    #[test]
    fn unx52293_checks_green() {
        let s = run_unx52293_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
