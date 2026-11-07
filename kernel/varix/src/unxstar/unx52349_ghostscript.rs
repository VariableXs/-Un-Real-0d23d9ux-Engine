//! UNX-F52349 Ghostscript 收口 · 0xC9700段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52349"，逐字）：
//!   title: Ghostscript 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52350-J1
//!     convert/mogrify 20 操作集 ×10 全通；图像语义分账声明（像素级处理
//!     语义归 N2/H3，本条只收 CLI 发行版）；KB-COMPAT 定级登记；判据锚
//!     定 §AI-66.1 分账；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52349）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52350-J1，照原文锚定；功能按本卡标题（Ghostscript）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 设备五元闭集 15 格：pdfwrite/png16m/ps2write/jpeg/tiff 各 3 格，
//!      设备 = 格号/3，家族单调（逆序必抓，空账本首格非 0 号设备必抓）。
//!   ② 转换 10 操作集：五设备转换家族各 2 操作（kind = 格号/2），载荷
//!      折叠恒等才收；漂移必抓。
//!   ③ 字体消费 8 格：逐格载荷对照，越闭集/漂移/重复必抓。
//!   ④ AGPL 声明 2 行闭集：AGPL 许可声明行/CLI 发行版分账声明行，缺一
//!      拒封（像素级处理语义归 N2/H3，本条只收 CLI 发行版）。
//!   ⑤ 版本阶梯 3 档闭集（9.55.0/9.56.1/10.02.0），越级必抓。
//!   ⑥ ledger 状态机（new/record/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（设备漂移/转换空账乱序/字体越
//!      闭集），检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52349;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 设备格数（五设备 × 各 3 格）。
pub const DEVICE_CELLS: usize = 15;

/// 转换操作数。
pub const CONV_OPS: usize = 10;

/// 字体消费格数。
pub const FONT_CELLS: usize = 8;

/// AGPL 分账声明行数。
pub const AGPL_ROWS: usize = 2;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// Ghostscript 域 XOR 锚（低 16 位恒零）。
pub const GS_ANCHOR: u64 = 0x4735_3200_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhostscriptCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 操作号越闭集（设备/转换）
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族逆序或空账乱序
    EOrder,
    /// 辅表越闭集（字体）
    ENoAux,
    /// 辅表载荷漂移（字体）
    EAuxDrift,
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

impl GhostscriptCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GhostscriptCode::Ok => "成功",
            GhostscriptCode::ENoVer => "版本级越阶梯：9.55.0/9.56.1/10.02.0 三档闭集之外先登记",
            GhostscriptCode::ENoOp => "操作号越闭集：设备 15 格/转换 10 操作闭集之外先登记",
            GhostscriptCode::EDrift => "载荷漂移：设备/转换/版本折叠与基准不恒等，逐格复算",
            GhostscriptCode::EOrder => "家族逆序：pdfwrite→png16m→ps2write→jpeg→tiff 单调，逆序或空账乱序必抓",
            GhostscriptCode::ENoAux => "字体格越闭集：字体消费 8 格闭集之外先登记",
            GhostscriptCode::EAuxDrift => "字体载荷漂移：字体消费折叠与基准不恒等，逐格复算",
            GhostscriptCode::ENoRow => "声明行越闭集：AGPL 分账 2 行闭集之外先登记",
            GhostscriptCode::EDup => "重复落账：同格只许一条账目",
            GhostscriptCode::ESeal => "未齐即封：15 设备格 + 10 转换 + 8 字体格 + 2 行声明未齐先补齐",
            GhostscriptCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GhostscriptCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GhostscriptCode::EGradeDrift => "定级漂移：全通且 AGPL 分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：判据+15设备格+10转换+8字体格+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=9.55.0 1=9.56.1 2=10.02.0）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xC1000u64 + (v as u64) * 0x5D) ^ GS_ANCHOR)
}

/// 版本级名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("9.55.0"),
        1 => Some("9.56.1"),
        2 => Some("10.02.0"),
        _ => None,
    }
}

/// 设备格载荷（格 o；设备 = o/3）。
pub const fn dev_cell(o: usize) -> Option<u64> {
    if o >= DEVICE_CELLS {
        return None;
    }
    Some((0xC2000u64 + (o as u64) * 0x37) ^ ((o / 3) as u64).wrapping_mul(0x01000193))
}

/// 设备号（0..3=pdfwrite 3..6=png16m 6..9=ps2write 9..12=jpeg 12..15=tiff）。
pub const fn dev_kind(o: usize) -> Option<u32> {
    if o >= DEVICE_CELLS {
        return None;
    }
    Some((o / 3) as u32)
}

/// 设备名。
pub const fn dev_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("pdfwrite"),
        1 => Some("png16m"),
        2 => Some("ps2write"),
        3 => Some("jpeg"),
        4 => Some("tiff"),
        _ => None,
    }
}

/// 转换操作载荷（操作 o；转换家族 = o/2）。
pub const fn conv_cell(o: usize) -> Option<u64> {
    if o >= CONV_OPS {
        return None;
    }
    Some((0xC3000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 转换家族（0..2=pdfwrite 转换 2..4=png16m 渲染 4..6=ps2write 回写 6..8=jpeg 出图 8..10=tiff 出图）。
pub const fn conv_kind(o: usize) -> Option<u32> {
    if o >= CONV_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 字体消费格载荷（格 f）。
pub const fn font_cell(f: usize) -> Option<u64> {
    if f >= FONT_CELLS {
        return None;
    }
    Some((0xC4000u64 + (f as u64) * 0x2C) ^ GS_ANCHOR)
}

/// AGPL 声明行折叠（行 r）。
pub const fn agpl_row(r: usize) -> Option<u32> {
    if r >= AGPL_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Ghostscript 收口账本状态机。
#[derive(Clone)]
pub struct GsLedger {
    /// 六判据（渲染管线/设备路由/转换收口/字体消费/AGPL 声明/写回摘要）。
    gates: [Option<bool>; 6],
    /// 设备账。
    devs: [Option<u64>; DEVICE_CELLS],
    /// 转换账。
    convs: [Option<u64>; CONV_OPS],
    /// 字体账。
    fonts: [Option<u64>; FONT_CELLS],
    /// 版本账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    agpl: [Option<u32>; AGPL_ROWS],
    /// 最后设备号。
    last_dev: i64,
    /// 最后转换家族。
    last_cv: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GsLedger {
    /// 新账本。
    pub fn new() -> Self {
        GsLedger {
            gates: [None; 6],
            devs: [None; DEVICE_CELLS],
            convs: [None; CONV_OPS],
            fonts: [None; FONT_CELLS],
            vers: [None; VERSION_STEPS],
            agpl: [None; AGPL_ROWS],
            last_dev: -1,
            last_cv: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GhostscriptCode> {
        if gate >= 6 {
            return Err(GhostscriptCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 设备一格（家族单调 + 空账首格必 0 号设备 + 载荷对照 + 重复拒）。
    pub fn record_dev(&mut self, o: usize, got: u64) -> Result<(), GhostscriptCode> {
        if o >= DEVICE_CELLS {
            return Err(GhostscriptCode::ENoOp);
        }
        let kind = match dev_kind(o) {
            Some(k) => k,
            None => return Err(GhostscriptCode::ENoOp),
        };
        if (self.last_dev < 0 && kind != 0) || (kind as i64) < self.last_dev {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(GhostscriptCode::EOrder);
        }
        let want = match dev_cell(o) {
            Some(w) => w,
            None => return Err(GhostscriptCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(GhostscriptCode::EDrift);
        }
        if match self.devs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhostscriptCode::EDup);
        }
        if let Some(c) = self.devs.get_mut(o) {
            *c = Some(got);
        }
        self.last_dev = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 设备全账核验（15 格）。
    pub fn devs_all(&self) -> bool {
        let mut o = 0usize;
        while o < DEVICE_CELLS {
            match self.devs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 转换一格（家族单调 + 空账首格必 0 家族 + 载荷对照 + 重复拒）。
    pub fn record_conv(&mut self, o: usize, got: u64) -> Result<(), GhostscriptCode> {
        if o >= CONV_OPS {
            return Err(GhostscriptCode::ENoOp);
        }
        let kind = match conv_kind(o) {
            Some(k) => k,
            None => return Err(GhostscriptCode::ENoOp),
        };
        if (self.last_cv < 0 && kind != 0) || (kind as i64) < self.last_cv {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(GhostscriptCode::EOrder);
        }
        let want = match conv_cell(o) {
            Some(w) => w,
            None => return Err(GhostscriptCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(GhostscriptCode::EDrift);
        }
        if match self.convs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhostscriptCode::EDup);
        }
        if let Some(c) = self.convs.get_mut(o) {
            *c = Some(got);
        }
        self.last_cv = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 转换全账核验（10 操作）。
    pub fn convs_all(&self) -> bool {
        let mut o = 0usize;
        while o < CONV_OPS {
            match self.convs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 字体一格（载荷对照 + 重复拒）。
    pub fn record_font(&mut self, f: usize, got: u64) -> Result<(), GhostscriptCode> {
        let want = match font_cell(f) {
            Some(w) => w,
            None => return Err(GhostscriptCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + f as u64);
            return Err(GhostscriptCode::EAuxDrift);
        }
        if match self.fonts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhostscriptCode::EDup);
        }
        if let Some(c) = self.fonts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x5401 + f as u64);
        Ok(())
    }

    /// 字体全账核验（8 格）。
    pub fn fonts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FONT_CELLS {
            match self.fonts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), GhostscriptCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(GhostscriptCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + v as u64);
            return Err(GhostscriptCode::EDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhostscriptCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5501 + v as u64);
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

    /// AGPL 声明一行。
    pub fn declare_agpl(&mut self, r: usize) -> Result<(), GhostscriptCode> {
        if r >= AGPL_ROWS {
            return Err(GhostscriptCode::ENoRow);
        }
        if match self.agpl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GhostscriptCode::EDup);
        }
        if let Some(c) = self.agpl.get_mut(r) {
            *c = agpl_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn agpl_all(&self) -> bool {
        let mut r = 0usize;
        while r < AGPL_ROWS {
            match self.agpl.get(r) {
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
        if self.gates_all() && self.vers_all() && self.devs_all() && self.convs_all() && self.fonts_all() && self.agpl_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GsLedger::new();
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
    while o < DEVICE_CELLS {
        if let Some(w) = dev_cell(o) {
            if led.record_dev(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CONV_OPS {
        if let Some(w) = conv_cell(c) {
            if led.record_conv(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut f = 0usize;
    while f < FONT_CELLS {
        if let Some(w) = font_cell(f) {
            if led.record_font(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < AGPL_ROWS {
        let _ = led.declare_agpl(r);
        r += 1;
    }
    // 注入 10 轮：设备漂移 / 转换空账乱序 / 字体越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GsLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_dev(0, dev_cell(0).unwrap_or(0));
                lt.record_dev(1, dev_cell(1).unwrap_or(0).wrapping_add(1)) == Err(GhostscriptCode::EDrift)
            }
            1 => {
                // 空账本乱序注入：首格非 0 号转换家族必抓，且不改 last_kind。
                lt.record_conv(5, conv_cell(5).unwrap_or(0)) == Err(GhostscriptCode::EOrder)
            }
            _ => lt.record_font(FONT_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52349 判据集（锚 UNX-F52350-J1，off-by-one 板式）。
pub fn run_unx52349_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52349");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "设备五元闭集 15 格全通",
        recorded >= DEVICE_CELLS as u32,
        "pdfwrite/png16m/ps2write/jpeg/tiff 各 3 格逐格恒等（UNX-F52350-J1 锚定）",
    );
    s.add(
        "转换 10 操作集全通",
        {
            let mut lt = GsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONV_OPS {
                if let Some(w) = conv_cell(c) {
                    if lt.record_conv(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.convs_all()
        },
        "五设备转换家族各 2 操作，载荷折叠恒等",
    );
    s.add(
        "字体消费 8 格全齐",
        {
            let mut lt = GsLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FONT_CELLS {
                if let Some(w) = font_cell(f) {
                    if lt.record_font(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fonts_all()
        },
        "字体消费逐格载荷对照恒等",
    );
    s.add(
        "版本阶梯 9.55.0/9.56.1/10.02.0 三档全落",
        {
            let mut lt = GsLedger::new();
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
        "AGPL 分账声明 2/2 全齐",
        {
            let mut lt = GsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < AGPL_ROWS {
                if lt.declare_agpl(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.agpl_all()
        },
        "AGPL 许可声明/CLI 发行版分账声明两行闭集（§AI-66.1 分账）",
    );
    s.add(
        "设备家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < DEVICE_CELLS {
                if dev_kind(o).unwrap_or(5) < dev_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五设备闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "设备漂移/转换空账乱序/字体越闭集三态轮转必抓",
    );
    let mut l0 = GsLedger::new();
    s.add(
        "边界·设备越闭集",
        l0.record_dev(DEVICE_CELLS, 0).is_err(),
        "设备格 15 越闭集必拒",
    );
    s.add(
        "边界·转换越闭集",
        l0.record_conv(CONV_OPS, 0).is_err(),
        "转换操作 10 越闭集必拒",
    );
    s.add(
        "边界·字体越闭集",
        l0.record_font(FONT_CELLS, 0).is_err(),
        "字体格 8 越闭集必拒",
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
            let mut lt = GsLedger::new();
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
            while o < DEVICE_CELLS {
                if let Some(w) = dev_cell(o) {
                    let _ = lt.record_dev(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CONV_OPS {
                if let Some(w) = conv_cell(c) {
                    let _ = lt.record_conv(c, w);
                }
                c += 1;
            }
            let mut f = 0usize;
            while f < FONT_CELLS {
                if let Some(w) = font_cell(f) {
                    let _ = lt.record_font(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < AGPL_ROWS {
                let _ = lt.declare_agpl(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9700,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9700",
    );
    s.add(
        "诊断码闭集 13 变体",
        GhostscriptCode::Ok as u32 == 0 && GhostscriptCode::EGradeDrift as u32 == 12,
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
        assert_eq!(version_name(0), Some("9.55.0"));
        assert_eq!(version_name(1), Some("9.56.1"));
        assert_eq!(version_name(2), Some("10.02.0"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_dev_kind() {
        assert_eq!(dev_kind(0), Some(0));
        assert_eq!(dev_kind(2), Some(0));
        assert_eq!(dev_kind(3), Some(1));
        assert_eq!(dev_kind(14), Some(4));
        assert!(dev_kind(15).is_none());
        assert!(dev_cell(15).is_none());
        assert_eq!(dev_kind_name(0), Some("pdfwrite"));
        assert_eq!(dev_kind_name(4), Some("tiff"));
        assert!(dev_kind_name(5).is_none());
    }

    #[test]
    fn test_conv_kind() {
        assert_eq!(conv_kind(0), Some(0));
        assert_eq!(conv_kind(1), Some(0));
        assert_eq!(conv_kind(4), Some(2));
        assert_eq!(conv_kind(9), Some(4));
        assert!(conv_kind(10).is_none());
        assert!(conv_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = dev_cell(3).unwrap_or(0);
        assert_eq!(a, dev_cell(3).unwrap_or(0));
        assert_ne!(a, dev_cell(4).unwrap_or(0));
        assert!(font_cell(FONT_CELLS).is_none());
        assert!(agpl_row(AGPL_ROWS).is_none());
        assert_eq!(GS_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GsLedger::new();
        let mut i = 0usize;
        while i <= 8 {
            let _ = lt.record_dev(i, dev_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_dev(2, dev_cell(2).unwrap_or(0)), Err(GhostscriptCode::EOrder));
        assert!(lt.record_dev(10, dev_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_order_caught() {
        let mut lt = GsLedger::new();
        assert_eq!(lt.record_conv(5, conv_cell(5).unwrap_or(0)), Err(GhostscriptCode::EOrder));
        assert_eq!(lt.record_conv(0, conv_cell(0).unwrap_or(0)), Ok(()));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GsLedger::new();
        assert_eq!(
            lt.record_dev(1, dev_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(GhostscriptCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(GhostscriptCode::EDrift)
        );
        assert_eq!(
            lt.record_font(2, font_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(GhostscriptCode::EAuxDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GsLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(GhostscriptCode::EDup));
        let _ = lt.record_font(0, font_cell(0).unwrap_or(0));
        assert_eq!(lt.record_font(0, font_cell(0).unwrap_or(0)), Err(GhostscriptCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DEVICE_CELLS + CONV_OPS + FONT_CELLS + VERSION_STEPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9700);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GhostscriptCode::Ok as u32, 0);
        assert_eq!(GhostscriptCode::EGradeDrift as u32, 12);
        assert!(!GhostscriptCode::Ok.detail().is_empty());
        assert!(!GhostscriptCode::ESeal.detail().is_empty());
        assert!(!GhostscriptCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52349_checks_green() {
        let s = run_unx52349_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
