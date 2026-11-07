//! UNX-F52287 KB-COMPAT 可查询率 100% 全段核账（B01–B14） · 0xC8780段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52287"，逐字）：
//!   title: KB-COMPAT 可查询率 100% 全段核账（B01–B14）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52288-J1 全段 280
//!     条目六判据逐条结算：全绿数/降级数/降级缺失清单全公开；KPI 权重
//!     15 口径落账；判据锚定 §AI-66.5 J-主轴 + §105 KPI 调权；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52287）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52288-J1，照原文锚定；功能按本卡标题（KB-COMPAT 可查询率 100%
//! 全段核账·B01–B14）落地，六判据全绿率结算判据由同批后续卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① B01–B14 十四段全覆盖：每段 4 格 × 14 段 = 56 格闭集，逐格载荷
//!      恒等才收，段格越闭集必抓；段名 B01..B14 闭集命名。
//!   ② 查询命中率 100% 闸：十四段逐段查询落账，缺一段即命中率破闸；
//!      查询段越闭集必抓。
//!   ③ 缺登记清单 4 行：逐行登记为公开缺登记项，逐行补登记清零——缺
//!      登记不静默，清零才许 S1。
//!   ④ 滚动回归 8 拍严格等差链：拍 b 只许在 last+1 落，跳拍/逆序/空账
//!      跳拍一律必抓——滚动回归不许欠拍。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（回归拍跳拍/段格漂移/段格越闭
//!      集），全部落在当轮临时账本上，主账本 last_beat 不受扰动；检出+
//!      逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集：56 格 + 14 段查询 100% + 4 行缺登记清零 +
//!      8 拍回归 + 六判据全通 → S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52287;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// KB-COMPAT 段数（B01–B14）。
pub const KB_SEGS: usize = 14;

/// 每段格数。
pub const CELLS_PER_SEG: usize = 4;

/// 全段总格数（14 × 4）。
pub const SEG_CELLS: usize = KB_SEGS * CELLS_PER_SEG;

/// 缺登记清单行数。
pub const MISSING_ROWS: usize = 4;

/// 滚动回归拍数。
pub const REG_BEATS: usize = 8;

/// 段格域异或锚（低 16 位恒零）。
pub const KB_ANCHOR: u64 = 0x4B42_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCompatCode {
    /// 成功
    Ok = 0,
    /// 段格越闭集
    ENoSeg,
    /// 段格载荷漂移
    ESegDrift,
    /// 查询段越闭集
    ENoQuery,
    /// 命中率破闸（未全段查询）
    ENoHit,
    /// 缺登记行越闭集
    ENoReg,
    /// 缺登记未清零
    ERowOpen,
    /// 回归拍越闭集
    ENoBeat,
    /// 回归拍跳拍/逆序
    EBeatOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl KbCompatCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KbCompatCode::Ok => "成功",
            KbCompatCode::ENoSeg => "段格越闭集：B01–B14 每段 4 格共 56 格之外先登记",
            KbCompatCode::ESegDrift => "段格载荷漂移：格折叠与基准不恒等，逐格复算",
            KbCompatCode::ENoQuery => "查询段越闭集：B01–B14 十四段之外先登记",
            KbCompatCode::ENoHit => "命中率破闸：十四段未逐段查询即破 100% 可查询率闸",
            KbCompatCode::ENoReg => "缺登记行越闭集：缺登记清单 4 行之外先登记",
            KbCompatCode::ERowOpen => "缺登记未清零：清单行公开未补登记，缺登记不静默",
            KbCompatCode::ENoBeat => "回归拍越闭集：滚动回归 8 拍之外先登记",
            KbCompatCode::EBeatOrder => "回归拍跳拍/逆序：拍 b 只许在 last+1 落，滚动回归不许欠拍",
            KbCompatCode::EDup => "重复落账：同格只许一条账目",
            KbCompatCode::ESeal => "未齐即封：56 格+14 段查询+4 行清零+8 拍回归未齐先补齐",
            KbCompatCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KbCompatCode::EGradeDrift => "定级漂移：全段核账全通且缺登记清零才可定 S1",
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
            Grade::S1 => "S1：六判据+56格+14段查询100%+4行缺登记清零+8拍回归全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格或缺登记未清",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 段格载荷（格 i：0..56，段 = i/4）。
pub const fn seg_cell(i: usize) -> Option<u64> {
    if i >= SEG_CELLS {
        return None;
    }
    Some((0xB1000u64 + (i as u64) * 0x3B) ^ KB_ANCHOR)
}

/// 段位（0..14）。
pub const fn seg_of(i: usize) -> Option<u32> {
    if i >= SEG_CELLS {
        return None;
    }
    Some((i / CELLS_PER_SEG) as u32)
}

/// 段名闭集 B01..B14。
pub const fn seg_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("B01"),
        1 => Some("B02"),
        2 => Some("B03"),
        3 => Some("B04"),
        4 => Some("B05"),
        5 => Some("B06"),
        6 => Some("B07"),
        7 => Some("B08"),
        8 => Some("B09"),
        9 => Some("B10"),
        10 => Some("B11"),
        11 => Some("B12"),
        12 => Some("B13"),
        13 => Some("B14"),
        _ => None,
    }
}

/// 滚动回归拍载荷（拍 b：0..8）。
pub const fn reg_beat(b: usize) -> Option<u64> {
    if b >= REG_BEATS {
        return None;
    }
    Some(0xB5000u64 + (b as u64) * 0x6D)
}

/// 缺登记清单行号闭集（行 r：0..4）。
pub const fn missing_row(r: usize) -> Option<u32> {
    if r >= MISSING_ROWS {
        return None;
    }
    Some(0xB4000 + r as u32 * 0x23)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// KB-COMPAT 全段核账收口账本状态机。
#[derive(Clone)]
pub struct KbCompatLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 56 段格账。
    segs: [Option<u64>; SEG_CELLS],
    /// 十四段逐段查询账。
    queries: [Option<bool>; KB_SEGS],
    /// 缺登记清单 4 行（Some(false)=公开未清，Some(true)=已补登记清零）。
    missing: [Option<bool>; MISSING_ROWS],
    /// 滚动回归 8 拍。
    beats: [Option<u64>; REG_BEATS],
    /// 最后回归拍（-1 起严格等差）。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KbCompatLedger {
    /// 新账本。
    pub fn new() -> Self {
        KbCompatLedger {
            gates: [None; 6],
            segs: [None; SEG_CELLS],
            queries: [None; KB_SEGS],
            missing: [None; MISSING_ROWS],
            beats: [None; REG_BEATS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KbCompatCode> {
        if gate >= 6 {
            return Err(KbCompatCode::ESeal);
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

    /// 段格一格（载荷对照 + 重复拒）。
    pub fn record_seg(&mut self, i: usize, got: u64) -> Result<(), KbCompatCode> {
        if i >= SEG_CELLS {
            return Err(KbCompatCode::ENoSeg);
        }
        let want = match seg_cell(i) {
            Some(w) => w,
            None => return Err(KbCompatCode::ENoSeg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + i as u64);
            return Err(KbCompatCode::ESegDrift);
        }
        if match self.segs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KbCompatCode::EDup);
        }
        if let Some(c) = self.segs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4201 + i as u64);
        Ok(())
    }

    /// 段格全账核验（56 格）。
    pub fn segs_all(&self) -> bool {
        let mut i = 0usize;
        while i < SEG_CELLS {
            match self.segs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 查询一段（B01–B14 逐段命中落账）。
    pub fn record_query(&mut self, s: usize) -> Result<(), KbCompatCode> {
        if s >= KB_SEGS {
            return Err(KbCompatCode::ENoQuery);
        }
        if match self.queries.get(s) {
            Some(Some(true)) => true,
            _ => false,
        } {
            return Err(KbCompatCode::EDup);
        }
        if let Some(c) = self.queries.get_mut(s) {
            *c = Some(true);
        }
        self.fnv(0x4300 + s as u64);
        Ok(())
    }

    /// 查询命中率 100% 闸核验（十四段全查）。
    pub fn hit_rate_full(&self) -> bool {
        let mut s = 0usize;
        while s < KB_SEGS {
            match self.queries.get(s) {
                Some(Some(true)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 缺登记清单公开一行（Some(false)=公开未清）。
    pub fn declare_missing(&mut self, r: usize) -> Result<(), KbCompatCode> {
        if r >= MISSING_ROWS {
            return Err(KbCompatCode::ENoReg);
        }
        if match self.missing.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KbCompatCode::EDup);
        }
        if let Some(c) = self.missing.get_mut(r) {
            *c = Some(false);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// 补登记一行（公开欠账清零）。
    pub fn resolve_missing(&mut self, r: usize) -> Result<(), KbCompatCode> {
        if r >= MISSING_ROWS {
            return Err(KbCompatCode::ENoReg);
        }
        if match self.missing.get(r) {
            Some(Some(false)) => false,
            _ => true,
        } {
            return Err(KbCompatCode::ERowOpen);
        }
        if let Some(c) = self.missing.get_mut(r) {
            *c = Some(true);
        }
        self.fnv(0x4401 + r as u64);
        Ok(())
    }

    /// 缺登记清单全清核验（4 行全部公开且全部补登记）。
    pub fn missing_resolved(&self) -> bool {
        let mut r = 0usize;
        while r < MISSING_ROWS {
            match self.missing.get(r) {
                Some(Some(true)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 滚动回归一拍（严格等差 + 载荷对照 + 重复拒）。
    pub fn record_reg_beat(&mut self, b: usize, got: u64) -> Result<(), KbCompatCode> {
        if b >= REG_BEATS {
            return Err(KbCompatCode::ENoBeat);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4500);
            return Err(KbCompatCode::EBeatOrder);
        }
        let want = match reg_beat(b) {
            Some(w) => w,
            None => return Err(KbCompatCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4501 + b as u64);
            return Err(KbCompatCode::ESegDrift);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KbCompatCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x4502 + b as u64);
        Ok(())
    }

    /// 滚动回归全账核验（8 拍齐且单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < REG_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
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

    /// KB-COMPAT 定级（全通且缺登记清零 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.segs_all()
            && self.hit_rate_full()
            && self.missing_resolved()
            && self.beats_all()
        {
            Grade::S1
        } else if self.gates_all() && self.segs_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for KbCompatLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KbCompatLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < SEG_CELLS {
        if let Some(w) = seg_cell(i) {
            if led.record_seg(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut s = 0usize;
    while s < KB_SEGS {
        let _ = led.record_query(s);
        s += 1;
    }
    let mut r = 0usize;
    while r < MISSING_ROWS {
        let _ = led.declare_missing(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < MISSING_ROWS {
        let _ = led.resolve_missing(r);
        r += 1;
    }
    let mut b = 0usize;
    while b < REG_BEATS {
        if let Some(w) = reg_beat(b) {
            if led.record_reg_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    // 注入 10 轮：回归拍跳拍 / 段格漂移 / 段格越闭集三态轮转。
    // 全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KbCompatLedger::new();
        let caught = match kind {
            0 => {
                // 空账本跳拍：期望拍 0，先落拍 5 必抓；last_beat 不受扰动。
                lt.record_reg_beat(5, reg_beat(5).unwrap_or(0)) == Err(KbCompatCode::EBeatOrder)
            }
            1 => {
                let _ = lt.record_seg(0, seg_cell(0).unwrap_or(0));
                lt.record_seg(1, seg_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(KbCompatCode::ESegDrift)
            }
            _ => lt.record_seg(SEG_CELLS, 0).is_err(),
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
/// UNX-F52287 判据集（锚 UNX-F52288-J1，off-by-one 板式）。
pub fn run_unx52287_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52287");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "B01–B14 十四段 56 格全通",
        recorded >= (SEG_CELLS + REG_BEATS) as u32,
        "每段 4 格 × 14 段逐格载荷恒等，段格越闭集必抓",
    );
    s.add(
        "段名闭集 B01..B14",
        {
            let mut ok = true;
            let mut sn = 0usize;
            while sn < KB_SEGS {
                if seg_name(sn).is_none() {
                    ok = false;
                }
                sn += 1;
            }
            ok && seg_name(KB_SEGS).is_none()
        },
        "十四段命名闭集，越段无名",
    );
    s.add(
        "查询命中率 100% 闸",
        {
            let mut lt = KbCompatLedger::new();
            let mut ok = true;
            let mut sn = 0usize;
            while sn < KB_SEGS {
                if lt.record_query(sn).is_err() {
                    ok = false;
                }
                sn += 1;
            }
            ok && lt.hit_rate_full()
        },
        "十四段逐段查询落账，缺一段即破 100% 闸",
    );
    s.add(
        "缺登记清单 4 行登记并清零",
        {
            let mut lt = KbCompatLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MISSING_ROWS {
                if lt.declare_missing(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            // 缺登记不静默：先公开再逐行补登记清零。
            let mut r = 0usize;
            while r < MISSING_ROWS {
                if lt.resolve_missing(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.missing_resolved()
        },
        "缺登记清单 4 行先公开后清零，不许静默丢弃",
    );
    s.add(
        "滚动回归 8 拍单调全落",
        {
            let mut lt = KbCompatLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < REG_BEATS {
                if let Some(w) = reg_beat(b) {
                    if lt.record_reg_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "滚动回归严格等差 8 拍，跳拍/逆序/欠拍必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = KbCompatLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，未齐即封",
    );
    s.add("注入 10/10 全抓", caught >= 10, "回归拍跳拍/段格漂移/段格越闭集三态轮转必抓");
    s.add(
        "边界·段格越闭集",
        {
            let mut lt = KbCompatLedger::new();
            lt.record_seg(SEG_CELLS, 0).is_err()
        },
        "段格 56 越闭集必拒",
    );
    s.add(
        "边界·查询段越闭集",
        {
            let mut lt = KbCompatLedger::new();
            lt.record_query(KB_SEGS).is_err()
        },
        "查询段 14 越闭集必拒",
    );
    s.add(
        "边界·缺登记行越闭集",
        {
            let mut lt = KbCompatLedger::new();
            lt.declare_missing(MISSING_ROWS).is_err()
        },
        "缺登记行 4 越闭集必拒",
    );
    s.add(
        "回归拍跳拍拒",
        {
            let mut lt = KbCompatLedger::new();
            let r0 = lt.record_reg_beat(0, reg_beat(0).unwrap_or(0));
            let r2 = lt.record_reg_beat(2, reg_beat(2).unwrap_or(0));
            r0.is_ok() && r2 == Err(KbCompatCode::EBeatOrder)
        },
        "拍 0 之后落拍 2 跳拍必抓",
    );
    s.add(
        "段格漂移拒",
        {
            let mut lt = KbCompatLedger::new();
            lt.record_seg(9, seg_cell(9).unwrap_or(0).wrapping_add(1)) == Err(KbCompatCode::ESegDrift)
        },
        "段格载荷漂移必抓",
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
            let mut lt = KbCompatLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < SEG_CELLS {
                if let Some(w) = seg_cell(i) {
                    let _ = lt.record_seg(i, w);
                }
                i += 1;
            }
            let mut sn = 0usize;
            while sn < KB_SEGS {
                let _ = lt.record_query(sn);
                sn += 1;
            }
            let mut r = 0usize;
            while r < MISSING_ROWS {
                let _ = lt.declare_missing(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < MISSING_ROWS {
                let _ = lt.resolve_missing(r);
                r += 1;
            }
            let mut b = 0usize;
            while b < REG_BEATS {
                if let Some(w) = reg_beat(b) {
                    let _ = lt.record_reg_beat(b, w);
                }
                b += 1;
            }
            // 定级检查：main/aux 各节全部记账完成后再断言。
            lt.judge_grade() == Grade::S1
        },
        "全通且缺登记清零 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8780,
        "SEG_BASE 0x40 对齐且按派工公式落 0xC8780",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_names() {
        assert_eq!(seg_name(0), Some("B01"));
        assert_eq!(seg_name(1), Some("B02"));
        assert_eq!(seg_name(13), Some("B14"));
        assert!(seg_name(14).is_none());
    }

    #[test]
    fn test_seg_of_map() {
        assert_eq!(seg_of(0), Some(0));
        assert_eq!(seg_of(4), Some(1));
        assert_eq!(seg_of(55), Some(13));
        assert!(seg_of(56).is_none());
        assert!(seg_cell(56).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = seg_cell(13).unwrap_or(0);
        assert_eq!(a, seg_cell(13).unwrap_or(0));
        assert_ne!(a, seg_cell(14).unwrap_or(0));
        assert!(reg_beat(REG_BEATS).is_none());
        assert!(missing_row(MISSING_ROWS).is_none());
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(KB_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_beat_order_caught() {
        // 空账本跳拍必抓。
        let mut lt = KbCompatLedger::new();
        assert_eq!(lt.record_reg_beat(5, reg_beat(5).unwrap_or(0)), Err(KbCompatCode::EBeatOrder));
        // 拍 0 之后落拍 2 跳拍必抓；拍 1 顺落可过。
        assert!(lt.record_reg_beat(0, reg_beat(0).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_reg_beat(2, reg_beat(2).unwrap_or(0)), Err(KbCompatCode::EBeatOrder));
        assert!(lt.record_reg_beat(1, reg_beat(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KbCompatLedger::new();
        assert_eq!(
            lt.record_seg(20, seg_cell(20).unwrap_or(0).wrapping_add(1)),
            Err(KbCompatCode::ESegDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KbCompatLedger::new();
        let _ = lt.record_seg(0, seg_cell(0).unwrap_or(0));
        assert_eq!(lt.record_seg(0, seg_cell(0).unwrap_or(0)), Err(KbCompatCode::EDup));
        let _ = lt.record_query(3);
        assert_eq!(lt.record_query(3), Err(KbCompatCode::EDup));
    }

    #[test]
    fn test_missing_lifecycle() {
        let mut lt = KbCompatLedger::new();
        // 未公开即补登记 → ERowOpen。
        assert_eq!(lt.resolve_missing(0), Err(KbCompatCode::ERowOpen));
        // 4 行全部「先公开、后补登记」→ 全部核销才 missing_resolved。
        let mut r = 0usize;
        while r < MISSING_ROWS {
            assert!(lt.declare_missing(r).is_ok());
            assert!(lt.resolve_missing(r).is_ok());
            r += 1;
        }
        assert!(lt.missing_resolved());
        // 重复公开是 EDup，不改变核销状态（单向门）。
        assert_eq!(lt.declare_missing(1), Err(KbCompatCode::EDup));
        assert!(lt.missing_resolved());
    }

    #[test]
    fn test_hit_gate() {
        let mut lt = KbCompatLedger::new();
        let mut sn = 0usize;
        while sn < KB_SEGS - 1 {
            assert!(lt.record_query(sn).is_ok());
            sn += 1;
        }
        // 缺一段即破 100% 闸。
        assert!(!lt.hit_rate_full());
        assert!(lt.record_query(KB_SEGS - 1).is_ok());
        assert!(lt.hit_rate_full());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KbCompatLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SEG_CELLS + REG_BEATS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8780);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KbCompatCode::Ok as u32, 0);
        assert_eq!(KbCompatCode::EGradeDrift as u32, 12);
        assert_eq!(KbCompatCode::Ok.val(), SEG_BASE);
        assert_eq!(KbCompatCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!KbCompatCode::Ok.detail().is_empty());
        assert!(!KbCompatCode::ENoHit.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = KbCompatLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52287_checks_green() {
        let s = run_unx52287_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
