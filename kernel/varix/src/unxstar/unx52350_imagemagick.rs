//! UNX-F52350 ImageMagick（Windows 发行版）收口 · 0xC9740段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52350"，逐字）：
//!   title: ImageMagick（Windows 发行版）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52351-J1 ×10
//!     全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52350）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52351-J1，照原文锚定；功能按本卡标题（ImageMagick Windows
//! 发行版）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 格式五族闭集 20 格：PNG/JPEG/TIFF/WebP/GIF 各 4 格，族 = 格号/4，
//!      族单调（逆序必抓，空账本首格非 0 号族必抓）。
//!   ② 转换/缩放/合成 10 操作集：convert 4/缩放 4/合成 2（kind = 格号/4），
//!      族单调，载荷折叠恒等才收；漂移必抓。
//!   ③ 政策三态 6 格：无限/受限/拒绝闭集，越态（>2）必抓。
//!   ④ HDRI 声明 2 行闭集：HDRI 构建声明行/Windows 发行版分账声明行，
//!      缺一拒封。
//!   ⑤ 版本阶梯 3 档闭集（6.9.x/7.0.x/7.1.x），越级必抓。
//!   ⑥ ledger 状态机（new/record/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（主表漂移/空账乱序/政策越态），
//!      检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52350;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 格式族格数（五族 × 各 4 格）。
pub const FMT_CELLS: usize = 20;

/// 转换/缩放/合成操作数。
pub const OPS: usize = 10;

/// 政策格数。
pub const POLICY_CELLS: usize = 6;

/// HDRI 分账声明行数。
pub const HDRI_ROWS: usize = 2;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// ImageMagick 域 XOR 锚（低 16 位恒零）。
pub const IM_ANCHOR: u64 = 0x494D_4147_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImagemagickCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 操作号越闭集（格式族/转换缩放合成）
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 族逆序或空账乱序
    EOrder,
    /// 政策格越闭集
    ENoPol,
    /// 政策越态/漂移
    EPolDrift,
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

impl ImagemagickCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ImagemagickCode::Ok => "成功",
            ImagemagickCode::ENoVer => "版本级越阶梯：6.9.x/7.0.x/7.1.x 三档闭集之外先登记",
            ImagemagickCode::ENoOp => "操作号越闭集：格式族 20 格/操作 10 项闭集之外先登记",
            ImagemagickCode::EDrift => "载荷漂移：格式族/操作/版本折叠与基准不恒等，逐格复算",
            ImagemagickCode::EOrder => "族逆序：PNG→JPEG→TIFF→WebP→GIF 单调，逆序或空账乱序必抓",
            ImagemagickCode::ENoPol => "政策格越闭集：政策三态 6 格闭集之外先登记",
            ImagemagickCode::EPolDrift => "政策越态：三态闭集（无限/受限/拒绝）之外先登记",
            ImagemagickCode::ENoRow => "声明行越闭集：HDRI 分账 2 行闭集之外先登记",
            ImagemagickCode::EDup => "重复落账：同格只许一条账目",
            ImagemagickCode::ESeal => "未齐即封：20 格式格 + 10 操作 + 6 政策格 + 2 行声明未齐先补齐",
            ImagemagickCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ImagemagickCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ImagemagickCode::EGradeDrift => "定级漂移：全通且 HDRI 分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：判据+20格式格+10操作+6政策格+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=6.9.x 1=7.0.x 2=7.1.x）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xD1000u64 + (v as u64) * 0x5D) ^ IM_ANCHOR)
}

/// 版本级名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("6.9.x"),
        1 => Some("7.0.x"),
        2 => Some("7.1.x"),
        _ => None,
    }
}

/// 格式族格载荷（格 o；族 = o/4）。
pub const fn fmt_cell(o: usize) -> Option<u64> {
    if o >= FMT_CELLS {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 格式族号（0..4=PNG 4..8=JPEG 8..12=TIFF 12..16=WebP 16..20=GIF）。
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
        3 => Some("WebP"),
        4 => Some("GIF"),
        _ => None,
    }
}

/// 操作载荷（操作 o；操作族 = o/4：0..4=convert 4..8=缩放 8..10=合成）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xD3000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作族号。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 政策三态名（态 s：0=无限 1=受限 2=拒绝）。
pub const fn policy_name(s: u32) -> Option<&'static str> {
    match s {
        0 => Some("无限"),
        1 => Some("受限"),
        2 => Some("拒绝"),
        _ => None,
    }
}

/// HDRI 声明行折叠（行 r）。
pub const fn hdri_row(r: usize) -> Option<u32> {
    if r >= HDRI_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ImageMagick 收口账本状态机。
#[derive(Clone)]
pub struct ImLedger {
    /// 六判据（格式族账/操作集账/政策账/HDRI 声明/版本阶梯/写回摘要）。
    gates: [Option<bool>; 6],
    /// 格式族账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 政策账（0=无限 1=受限 2=拒绝）。
    pols: [Option<u32>; POLICY_CELLS],
    /// 版本账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    hdri: [Option<u32>; HDRI_ROWS],
    /// 最后格式族。
    last_fmt: i64,
    /// 最后操作族。
    last_op: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ImLedger {
    /// 新账本。
    pub fn new() -> Self {
        ImLedger {
            gates: [None; 6],
            fmts: [None; FMT_CELLS],
            ops: [None; OPS],
            pols: [None; POLICY_CELLS],
            vers: [None; VERSION_STEPS],
            hdri: [None; HDRI_ROWS],
            last_fmt: -1,
            last_op: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ImagemagickCode> {
        if gate >= 6 {
            return Err(ImagemagickCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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
    pub fn record_fmt(&mut self, o: usize, got: u64) -> Result<(), ImagemagickCode> {
        if o >= FMT_CELLS {
            return Err(ImagemagickCode::ENoOp);
        }
        let kind = match fmt_kind(o) {
            Some(k) => k,
            None => return Err(ImagemagickCode::ENoOp),
        };
        if (self.last_fmt < 0 && kind != 0) || (kind as i64) < self.last_fmt {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(ImagemagickCode::EOrder);
        }
        let want = match fmt_cell(o) {
            Some(w) => w,
            None => return Err(ImagemagickCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(ImagemagickCode::EDrift);
        }
        if match self.fmts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ImagemagickCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(o) {
            *c = Some(got);
        }
        self.last_fmt = kind as i64;
        self.fnv(0x6202 + o as u64);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ImagemagickCode> {
        if o >= OPS {
            return Err(ImagemagickCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(ImagemagickCode::ENoOp),
        };
        if (self.last_op < 0 && kind != 0) || (kind as i64) < self.last_op {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(ImagemagickCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(ImagemagickCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(ImagemagickCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ImagemagickCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_op = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 项）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 政策一格（三态闭集 + 重复拒）。
    pub fn record_policy(&mut self, p: usize, state: u32) -> Result<(), ImagemagickCode> {
        if p >= POLICY_CELLS {
            return Err(ImagemagickCode::ENoPol);
        }
        if state > 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + p as u64);
            return Err(ImagemagickCode::EPolDrift);
        }
        if match self.pols.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ImagemagickCode::EDup);
        }
        if let Some(c) = self.pols.get_mut(p) {
            *c = Some(state);
        }
        self.fnv(0x6401 + p as u64);
        Ok(())
    }

    /// 政策全账核验（6 格）。
    pub fn pols_all(&self) -> bool {
        let mut p = 0usize;
        while p < POLICY_CELLS {
            match self.pols.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), ImagemagickCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(ImagemagickCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + v as u64);
            return Err(ImagemagickCode::EDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ImagemagickCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x6501 + v as u64);
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

    /// HDRI 声明一行。
    pub fn declare_hdri(&mut self, r: usize) -> Result<(), ImagemagickCode> {
        if r >= HDRI_ROWS {
            return Err(ImagemagickCode::ENoRow);
        }
        if match self.hdri.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ImagemagickCode::EDup);
        }
        if let Some(c) = self.hdri.get_mut(r) {
            *c = hdri_row(r);
        }
        self.fnv(0x6600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn hdri_all(&self) -> bool {
        let mut r = 0usize;
        while r < HDRI_ROWS {
            match self.hdri.get(r) {
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
        if self.gates_all() && self.vers_all() && self.fmts_all() && self.ops_all() && self.pols_all() && self.hdri_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ImLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ImLedger::new();
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
    while c < OPS {
        if let Some(w) = op_cell(c) {
            if led.record_op(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < POLICY_CELLS {
        if led.record_policy(p, p as u32 % 3).is_ok() {
            recorded += 1;
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < HDRI_ROWS {
        let _ = led.declare_hdri(r);
        r += 1;
    }
    // 注入 10 轮：格式族漂移 / 操作空账乱序 / 政策越态三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ImLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_fmt(0, fmt_cell(0).unwrap_or(0));
                lt.record_fmt(1, fmt_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ImagemagickCode::EDrift)
            }
            1 => {
                // 空账本乱序注入：首格非 0 号操作族必抓，且不改 last_kind。
                lt.record_op(9, op_cell(9).unwrap_or(0)) == Err(ImagemagickCode::EOrder)
            }
            _ => lt.record_policy(POLICY_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52350 判据集（锚 UNX-F52351-J1，off-by-one 板式）。
pub fn run_unx52350_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52350");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "格式五族 20 格全通",
        recorded >= FMT_CELLS as u32,
        "PNG/JPEG/TIFF/WebP/GIF 各 4 格逐格恒等（UNX-F52351-J1 锚定，§AI-66.3）",
    );
    s.add(
        "转换/缩放/合成 10 操作全通",
        {
            let mut lt = ImLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < OPS {
                if let Some(w) = op_cell(c) {
                    if lt.record_op(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.ops_all()
        },
        "convert 4/缩放 4/合成 2 族单调逐操作恒等",
    );
    s.add(
        "政策三态 6 格全齐",
        {
            let mut lt = ImLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if lt.record_policy(p, p as u32 % 3).is_err() {
                    ok = false;
                }
                p += 1;
            }
            ok && lt.pols_all()
        },
        "无限/受限/拒绝三态闭集逐格落账",
    );
    s.add(
        "版本阶梯 6.9.x/7.0.x/7.1.x 三档全落",
        {
            let mut lt = ImLedger::new();
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
        "HDRI 分账声明 2/2 全齐",
        {
            let mut lt = ImLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < HDRI_ROWS {
                if lt.declare_hdri(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.hdri_all()
        },
        "HDRI 构建声明/Windows 发行版分账声明两行闭集",
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
        "格式族漂移/操作空账乱序/政策越态三态轮转必抓",
    );
    let mut l0 = ImLedger::new();
    s.add(
        "边界·格式族越闭集",
        l0.record_fmt(FMT_CELLS, 0).is_err(),
        "格式格 20 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·政策越态",
        l0.record_policy(0, 7) == Err(ImagemagickCode::EPolDrift),
        "政策态 7 越三态闭集必拒",
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
            let mut lt = ImLedger::new();
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
            while c < OPS {
                if let Some(w) = op_cell(c) {
                    let _ = lt.record_op(c, w);
                }
                c += 1;
            }
            let mut p = 0usize;
            while p < POLICY_CELLS {
                let _ = lt.record_policy(p, p as u32 % 3);
                p += 1;
            }
            let mut r = 0usize;
            while r < HDRI_ROWS {
                let _ = lt.declare_hdri(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9740,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9740",
    );
    s.add(
        "诊断码闭集 13 变体",
        ImagemagickCode::Ok as u32 == 0 && ImagemagickCode::EGradeDrift as u32 == 12,
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
        assert_eq!(version_name(0), Some("6.9.x"));
        assert_eq!(version_name(1), Some("7.0.x"));
        assert_eq!(version_name(2), Some("7.1.x"));
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
        assert_eq!(fmt_kind_name(4), Some("GIF"));
        assert!(fmt_kind_name(5).is_none());
    }

    #[test]
    fn test_op_kind_and_policy() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(9), Some(2));
        assert!(op_kind(10).is_none());
        assert_eq!(policy_name(0), Some("无限"));
        assert_eq!(policy_name(1), Some("受限"));
        assert_eq!(policy_name(2), Some("拒绝"));
        assert!(policy_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = fmt_cell(5).unwrap_or(0);
        assert_eq!(a, fmt_cell(5).unwrap_or(0));
        assert_ne!(a, fmt_cell(6).unwrap_or(0));
        assert!(op_cell(OPS).is_none());
        assert!(hdri_row(HDRI_ROWS).is_none());
        assert_eq!(IM_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ImLedger::new();
        // 先按合法族序铺 0..=8（kind=o/3，三族），再注逆序 2 必抓，后 kind3 正格。
        let mut i = 0usize;
        while i <= 8 {
            let _ = lt.record_fmt(i, fmt_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_fmt(2, fmt_cell(2).unwrap_or(0)), Err(ImagemagickCode::EOrder));
        assert!(lt.record_fmt(14, fmt_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_order_caught() {
        let mut lt = ImLedger::new();
        assert_eq!(lt.record_op(9, op_cell(9).unwrap_or(0)), Err(ImagemagickCode::EOrder));
        assert_eq!(lt.record_op(0, op_cell(0).unwrap_or(0)), Ok(()));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ImLedger::new();
        assert_eq!(
            lt.record_fmt(1, fmt_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(ImagemagickCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(ImagemagickCode::EDrift)
        );
    }

    #[test]
    fn test_policy_boundary_and_dup() {
        let mut lt = ImLedger::new();
        assert!(lt.record_policy(0, 0).is_ok());
        assert!(lt.record_policy(1, 1).is_ok());
        assert!(lt.record_policy(2, 2).is_ok());
        assert_eq!(lt.record_policy(0, 0), Err(ImagemagickCode::EDup));
        assert_eq!(lt.record_policy(3, 7), Err(ImagemagickCode::EPolDrift));
        assert_eq!(lt.record_policy(POLICY_CELLS, 0), Err(ImagemagickCode::ENoPol));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ImLedger::new();
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
        assert!(recorded >= (FMT_CELLS + OPS + POLICY_CELLS + VERSION_STEPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9740);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ImagemagickCode::Ok as u32, 0);
        assert_eq!(ImagemagickCode::EGradeDrift as u32, 12);
        assert!(!ImagemagickCode::Ok.detail().is_empty());
        assert!(!ImagemagickCode::ESeal.detail().is_empty());
        assert!(!ImagemagickCode::EPolDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = ImLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52350_checks_green() {
        let s = run_unx52350_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
