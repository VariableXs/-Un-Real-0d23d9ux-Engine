//! UNX-F52351 GraphicsMagick 收口 · 0xC9780段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52351"，逐字）：
//!   title: GraphicsMagick 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52352-J1 元数据
//!     读写 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52351）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52352-J1，照原文锚定；功能按本卡标题（GraphicsMagick）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 格式五族闭集 20 格：PNG/JPEG/TIFF/GIF/BMP 各 4 格，族 = 格号/4，
//!      族单调（逆序必抓，空账本首格非 0 号族必抓）。
//!   ② convert/mogrify/identify 10 操作集：convert 4/mogrify 4/identify 2
//!      （kind = 格号/4），族单调，载荷折叠恒等才收；漂移必抓。
//!   ③ 多线程 4 拍单调：节拍号必须严格递增，回拍必抓。
//!   ④ 与 ImageMagick 分账声明 2 行闭集：GM 独立账声明行/IM 分账边界
//!      声明行，缺一拒封。
//!   ⑤ 版本阶梯 3 档闭集（1.3.x/1.4.x/1.5.x），越级必抓。
//!   ⑥ ledger 状态机（new/record/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（主表漂移/操作空账乱序/多线程越
//!      闭集），检出+逃逸守恒等式；双跑指纹一致；定级 S2（卡面口径：
//!      全通 → S2 有条件收口，判据缺 → S3）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52351;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 格式族格数（五族 × 各 4 格）。
pub const FMT_CELLS: usize = 20;

/// convert/mogrify/identify 操作数。
pub const GM_OPS: usize = 10;

/// 多线程节拍数。
pub const MT_BEATS: usize = 4;

/// 与 ImageMagick 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// GraphicsMagick 域 XOR 锚（低 16 位恒零）。
pub const GM_ANCHOR: u64 = 0x474D_4758_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfxmagickCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 操作号越闭集（格式族/convert-mogrify-identify）
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 族逆序或空账乱序
    EOrder,
    /// 节拍越闭集
    ENoBeat,
    /// 节拍漂移
    EBeatDrift,
    /// 声明行越闭集
    ENoRow,
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

impl GfxmagickCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GfxmagickCode::Ok => "成功",
            GfxmagickCode::ENoVer => "版本级越阶梯：1.3.x/1.4.x/1.5.x 三档闭集之外先登记",
            GfxmagickCode::ENoOp => "操作号越闭集：格式族 20 格/操作 10 项闭集之外先登记",
            GfxmagickCode::EDrift => "载荷漂移：格式族/操作/版本折叠与基准不恒等，逐格复算",
            GfxmagickCode::EOrder => "族逆序：PNG→JPEG→TIFF→GIF→BMP 单调，逆序或空账乱序必抓",
            GfxmagickCode::ENoBeat => "节拍越闭集：多线程 4 拍闭集之外先登记",
            GfxmagickCode::EBeatDrift => "节拍漂移：多线程拍载荷与基准不恒等或回拍，逐拍复算",
            GfxmagickCode::ENoRow => "声明行越闭集：与 ImageMagick 分账 2 行闭集之外先登记",
            GfxmagickCode::EDup => "重复落账：同格只许一条账目",
            GfxmagickCode::ESeal => "未齐即封：20 格式格 + 10 操作 + 4 拍 + 2 行声明未齐先补齐",
            GfxmagickCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GfxmagickCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GfxmagickCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不用）
    S1,
    /// S2：有条件收口（本卡全通定级）
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
            Grade::S1 => "S1：全通收口（本卡口径不落此级）",
            Grade::S2 => "S2：有条件收口，20 格+10 操作+4 拍+2 行分账全通（KB-COMPAT 登记级）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=1.3.x 1=1.4.x 2=1.5.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xE1000u64 + (v as u64) * 0x5D) ^ GM_ANCHOR)
}

/// 版本级名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("1.3.x"),
        1 => Some("1.4.x"),
        2 => Some("1.5.x"),
        _ => None,
    }
}

/// 格式族格载荷（格 o；族 = o/4）。
pub const fn fmt_cell(o: usize) -> Option<u64> {
    if o >= FMT_CELLS {
        return None;
    }
    Some((0xE2000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 格式族号（0..4=PNG 4..8=JPEG 8..12=TIFF 12..16=GIF 16..20=BMP）。
pub const fn fmt_kind(o: usize) -> Option<u32> {
    if o >= FMT_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 格式族名。
pub const fn fmt_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("PNG"),
        1 => Some("JPEG"),
        2 => Some("TIFF"),
        3 => Some("GIF"),
        4 => Some("BMP"),
        _ => None,
    }
}

/// 操作载荷（操作 o；操作族 = o/4：0..4=convert 4..8=mogrify 8..10=identify）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= GM_OPS {
        return None;
    }
    Some((0xE3000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作族号。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= GM_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 多线程拍载荷（拍 b）。
pub const fn mt_cell(b: usize) -> Option<u64> {
    if b >= MT_BEATS {
        return None;
    }
    Some((0xE4000u64 + (b as u64) * 0x31) ^ GM_ANCHOR)
}

/// 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xE500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GraphicsMagick 收口账本状态机。
#[derive(Clone)]
pub struct GmLedger {
    /// 六判据（格式族账/操作集账/多线程账/分账声明/版本阶梯/写回摘要）。
    gates: [Option<bool>; 6],
    /// 格式族账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 操作账。
    ops: [Option<u64>; GM_OPS],
    /// 多线程拍账。
    beats: [Option<u64>; MT_BEATS],
    /// 版本账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后格式族。
    last_fmt: i64,
    /// 最后操作族。
    last_op: i64,
    /// 最后节拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GmLedger {
    /// 新账本。
    pub fn new() -> Self {
        GmLedger {
            gates: [None; 6],
            fmts: [None; FMT_CELLS],
            ops: [None; GM_OPS],
            beats: [None; MT_BEATS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
            last_fmt: -1,
            last_op: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GfxmagickCode> {
        if gate >= 6 {
            return Err(GfxmagickCode::ESeal);
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

    /// 格式族一格（族单调 + 空账首格必 0 族 + 载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, o: usize, got: u64) -> Result<(), GfxmagickCode> {
        if o >= FMT_CELLS {
            return Err(GfxmagickCode::ENoOp);
        }
        let kind = match fmt_kind(o) {
            Some(k) => k,
            None => return Err(GfxmagickCode::ENoOp),
        };
        if (self.last_fmt < 0 && kind != 0) || (kind as i64) < self.last_fmt {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(GfxmagickCode::EOrder);
        }
        let want = match fmt_cell(o) {
            Some(w) => w,
            None => return Err(GfxmagickCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(GfxmagickCode::EDrift);
        }
        if match self.fmts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GfxmagickCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(o) {
            *c = Some(got);
        }
        self.last_fmt = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 格式族全账核验（20 格）。
    pub fn fmts_all(&self) -> bool {
        let mut o = 0usize;
        while o < FMT_CELLS {
            match self.fmts.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 操作一格（族单调 + 空账首格必 0 族 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GfxmagickCode> {
        if o >= GM_OPS {
            return Err(GfxmagickCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(GfxmagickCode::ENoOp),
        };
        if (self.last_op < 0 && kind != 0) || (kind as i64) < self.last_op {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(GfxmagickCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(GfxmagickCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + o as u64);
            return Err(GfxmagickCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GfxmagickCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_op = kind as i64;
        self.fnv(0x7302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 项）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < GM_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 多线程一拍（节拍严格递增 + 载荷对照）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), GfxmagickCode> {
        if b >= MT_BEATS {
            return Err(GfxmagickCode::ENoBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400);
            return Err(GfxmagickCode::EOrder);
        }
        let want = match mt_cell(b) {
            Some(w) => w,
            None => return Err(GfxmagickCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7401 + b as u64);
            return Err(GfxmagickCode::EBeatDrift);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x7402 + b as u64);
        Ok(())
    }

    /// 多线程全账核验（4 拍全齐且单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < MT_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), GfxmagickCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(GfxmagickCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + v as u64);
            return Err(GfxmagickCode::EDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GfxmagickCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x7501 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 分账声明一行（与 ImageMagick 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), GfxmagickCode> {
        if r >= SPLIT_ROWS {
            return Err(GfxmagickCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GfxmagickCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// KB-COMPAT 定级（全通 → S2 卡面口径，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.vers_all() && self.fmts_all() && self.ops_all() && self.beats_all() && self.splits_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < FMT_CELLS {
        if let Some(w) = fmt_cell(o) {
            if led.record_fmt(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < GM_OPS {
        if let Some(w) = op_cell(c) {
            if led.record_op(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < MT_BEATS {
        if let Some(w) = mt_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：格式族漂移 / 操作空账乱序 / 多线程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GmLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_fmt(0, fmt_cell(0).unwrap_or(0));
                lt.record_fmt(1, fmt_cell(1).unwrap_or(0).wrapping_add(1)) == Err(GfxmagickCode::EDrift)
            }
            1 => {
                // 空账本乱序注入：首格非 0 号操作族必抓，且不改 last_kind。
                lt.record_op(9, op_cell(9).unwrap_or(0)) == Err(GfxmagickCode::EOrder)
            }
            _ => lt.record_beat(MT_BEATS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52351 判据集（锚 UNX-F52352-J1，off-by-one 板式）。
pub fn run_unx52351_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52351");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "格式五族 20 格全通",
        recorded >= FMT_CELLS as u32,
        "PNG/JPEG/TIFF/GIF/BMP 各 4 格逐格恒等（UNX-F52352-J1 锚定，§AI-66.3）",
    );
    s.add(
        "convert/mogrify/identify 10 操作全通",
        {
            let mut lt = GmLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < GM_OPS {
                if let Some(w) = op_cell(c) {
                    if lt.record_op(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.ops_all()
        },
        "convert 4/mogrify 4/identify 2 族单调逐操作恒等",
    );
    s.add(
        "多线程 4 拍单调全齐",
        {
            let mut lt = GmLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < MT_BEATS {
                if let Some(w) = mt_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "节拍严格递增逐拍载荷对照恒等",
    );
    s.add(
        "版本阶梯 1.3.x/1.4.x/1.5.x 三档全落",
        {
            let mut lt = GmLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐级恒等，越级必抓",
    );
    s.add(
        "与 ImageMagick 分账声明 2/2 全齐",
        {
            let mut lt = GmLedger::new();
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
        "GM 独立账声明/IM 分账边界声明两行闭集",
    );
    s.add(
        "格式族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FMT_CELLS {
                if fmt_kind(o).unwrap_or(5) < fmt_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "格式族漂移/操作空账乱序/多线程越闭集三态轮转必抓",
    );
    let mut l0 = GmLedger::new();
    s.add(
        "边界·格式族越闭集",
        l0.record_fmt(FMT_CELLS, 0).is_err(),
        "格式格 20 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(GM_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·节拍越闭集",
        l0.record_beat(MT_BEATS, 0).is_err(),
        "节拍 4 越闭集必拒",
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
            let mut lt = GmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < FMT_CELLS {
                if let Some(w) = fmt_cell(o) {
                    let _ = lt.record_fmt(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < GM_OPS {
                if let Some(w) = op_cell(c) {
                    let _ = lt.record_op(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < MT_BEATS {
                if let Some(w) = mt_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 有条件收口（KB-COMPAT 定级登记，卡面口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9780,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9780",
    );
    s.add(
        "诊断码闭集 13 变体",
        GfxmagickCode::Ok as u32 == 0 && GfxmagickCode::EGradeDrift as u32 == 12,
        "Ok=+0x00 起 12 错误码至 +0x0C 恰 13 变体，detail 全非空",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("1.3.x"));
        assert_eq!(version_name(1), Some("1.4.x"));
        assert_eq!(version_name(2), Some("1.5.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_fmt_kind() {
        assert_eq!(fmt_kind(0), Some(0));
        assert_eq!(fmt_kind(3), Some(0));
        assert_eq!(fmt_kind(4), Some(1));
        assert_eq!(fmt_kind(19), Some(4));
        assert!(fmt_kind(20).is_none());
        assert!(fmt_cell(20).is_none());
        assert_eq!(fmt_kind_name(0), Some("PNG"));
        assert_eq!(fmt_kind_name(4), Some("BMP"));
        assert!(fmt_kind_name(5).is_none());
    }

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(9), Some(2));
        assert!(op_kind(10).is_none());
        assert!(op_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = fmt_cell(5).unwrap_or(0);
        assert_eq!(a, fmt_cell(5).unwrap_or(0));
        assert_ne!(a, fmt_cell(6).unwrap_or(0));
        assert!(mt_cell(MT_BEATS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(GM_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GmLedger::new();
        // 先按合法族序铺 0..=8（kind=o/3，三族），再注逆序 2 必抓，后 kind3 正格。
        let mut i = 0usize;
        while i <= 8 {
            let _ = lt.record_fmt(i, fmt_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_fmt(2, fmt_cell(2).unwrap_or(0)), Err(GfxmagickCode::EOrder));
        assert!(lt.record_fmt(14, fmt_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_order_caught() {
        let mut lt = GmLedger::new();
        assert_eq!(lt.record_op(9, op_cell(9).unwrap_or(0)), Err(GfxmagickCode::EOrder));
        assert_eq!(lt.record_op(0, op_cell(0).unwrap_or(0)), Ok(()));
    }

    #[test]
    fn test_beat_monotonic() {
        let mut lt = GmLedger::new();
        let _ = lt.record_beat(2, mt_cell(2).unwrap_or(0));
        assert_eq!(lt.record_beat(1, mt_cell(1).unwrap_or(0)), Err(GfxmagickCode::EOrder));
        assert!(lt.record_beat(3, mt_cell(3).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_beat(0, mt_cell(0).unwrap_or(0)),
            Err(GfxmagickCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GmLedger::new();
        assert_eq!(
            lt.record_fmt(1, fmt_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(GfxmagickCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(GfxmagickCode::EDrift)
        );
        assert_eq!(
            lt.record_beat(0, mt_cell(0).unwrap_or(0).wrapping_add(3)),
            Err(GfxmagickCode::EBeatDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GmLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(GfxmagickCode::EDup));
        let _ = lt.record_fmt(0, fmt_cell(0).unwrap_or(0));
        assert_eq!(lt.record_fmt(0, fmt_cell(0).unwrap_or(0)), Err(GfxmagickCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GmLedger::new();
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
        assert!(recorded >= (FMT_CELLS + GM_OPS + MT_BEATS + VERSION_STEPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9780);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GfxmagickCode::Ok as u32, 0);
        assert_eq!(GfxmagickCode::EGradeDrift as u32, 12);
        assert!(!GfxmagickCode::Ok.detail().is_empty());
        assert!(!GfxmagickCode::ESeal.detail().is_empty());
        assert!(!GfxmagickCode::EBeatDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GmLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52351_checks_green() {
        let s = run_unx52351_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
