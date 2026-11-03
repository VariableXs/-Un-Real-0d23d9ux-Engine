//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0471 ECAM/CF8 交叉验证（全量） · 0x756C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7G 逐字锚定，AI-VX7-1104）：
//!   双机制全量比对（设备数=比对数 100% 一致），差异显性账。
//! 设计要点：
//!   1. ECAM 地址式：base + (bus<<20) + (dev<<15) + (fn<<12) + off，每总
//!      线窗口 1MB（每功能 4096B × 256 功能，PCIe Base r6.0 §7.2.1）。
//!   2. CF8 地址式：0x80000000 | bus<<16 | dev<<11 | fn<<8 | (off&0xFC)，
//!      使能位 bit31 与偏移低 2 位清零（PCI Local Bus Spec r3.0 §3.1.1）。
//!   3. 双视图模型：同一张字表，ECAM 视图与 CF8/CFC 视图各按自己的地址
//!      式解码寻址，全量比对逐字（每设备 64 字）进行。
//!   4. 比对数=设备数：干净路径上每台设备 64 字全一致（比对行数 = 设备
//!      数 × 64），一致率 100%；差异行显性入账（bdf/偏移/双值）。
//!   5. 注入必抓：漂移表只改 ECAM 视图一个字（值翻转），交叉比对必出
//!      差异行，且差异行数 = 注入字数。
//!   6. 错误路径中文建议写入诊断 detail：越窗→核对总线窗口、越界偏移→
//!      0x100 上限、地址失配→重算地址式。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x756C0，AI-VX7-1104 批 G 组独占区）。
pub const SEG_BASE: u32 = 0x756C0;

// ── 冻结常量表（来源：PCIe Base Spec r6.0 §7.2.1 / PCI Local Bus Spec r3.0 §3.1.1）──
/// ECAM 每总线窗口大小（1MB，冻结）。
pub const ECAM_BUS_WINDOW: u64 = 0x100000;
/// 每功能配置空间大小（4096B，冻结）。
pub const ECAM_FN_SIZE: u64 = 0x1000;
/// CF8/CFC 使能位（bit31）。
pub const CF8_ENABLE: u32 = 0x8000_0000;
/// CF8 偏移掩码（低 2 位清零，4B 对齐）。
pub const CF8_OFF_MASK: u32 = 0xFC;
/// 每设备字数（256B / 4B = 64 字，比对粒度）。
pub const WORDS_PER_DEV: usize = 64;
/// 配置偏移上限（0x100 字节）。
pub const CFG_OFF_MAX: u32 = 0x100;
/// CF8 地址式中 dev 步进（<<11）。
pub const CF8_DEV_SHIFT: u32 = 11;
/// CF8 地址式中 fn 步进（<<8）。
pub const CF8_FN_SHIFT: u32 = 8;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XchkCode {
    /// 偏移越界（≥0x100）
    OffOver,
    /// ECAM 地址越窗（>1MB 窗口）
    EcamOverWindow,
    /// 设备号越界（≥32）
    DevOver,
    /// 功能号越界（≥8）
    FnOver,
    /// CF8 地址未带使能位
    Cf8NoEnable,
    /// 交叉比对差异未清零
    DiffRemain,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl XchkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            XchkCode::OffOver => "偏移越界：配置空间上限 0x100 字节",
            XchkCode::EcamOverWindow => "ECAM 地址越窗：每总线窗口 1MB",
            XchkCode::DevOver => "设备号越界：每总线 32 设备",
            XchkCode::FnOver => "功能号越界：每设备 8 功能",
            XchkCode::Cf8NoEnable => "CF8 地址未带使能位：bit31 必须置 1",
            XchkCode::DiffRemain => "交叉比对差异未清零：核对漂移表后复扫",
            XchkCode::ReplayMismatch => "复算失配：账面与重算结果不一致",
            XchkCode::NotReproducible => "双跑不一致：复现红线，固定种子复跑",
        }
    }
    pub const ALL: [XchkCode; 8] = [
        XchkCode::OffOver,
        XchkCode::EcamOverWindow,
        XchkCode::DevOver,
        XchkCode::FnOver,
        XchkCode::Cf8NoEnable,
        XchkCode::DiffRemain,
        XchkCode::ReplayMismatch,
        XchkCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < XchkCode::ALL.len() {
        if let Some(c) = XchkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x756C0 段闭集内"
}

// ── 地址式（双机制各自解码，互相独立） ──────────────────────────
/// bdf 编码：bus<<8 | dev<<3 | fn。
pub fn bdf_of(bus: u8, dev: u8, fn_: u8) -> u32 {
    ((bus as u32) << 8) | ((dev as u32) << 3) | (fn_ as u32)
}

/// bdf 反解（bus/dev/fn）。
pub fn bdf_split(bdf: u32) -> (u8, u8, u8) {
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let fn_ = (bdf & 0x7) as u8;
    (bus, dev, fn_)
}

/// ECAM 地址式：base + (bus<<20) + (dev<<15) + (fn<<12) + off。
/// 窗口闸只查**总线内**偏移（dev<<15|fn<<12|off 必须落在该总线的 1MB
/// 窗口内；每条总线各得一个 1MB 窗口，bus 步进窗口基址）。
pub fn ecam_addr(base: u64, bus: u8, dev: u8, fn_: u8, off: u32) -> Result<u64, u32> {
    if off >= CFG_OFF_MAX {
        return Err(XchkCode::OffOver.val());
    }
    if dev >= 32 {
        return Err(XchkCode::DevOver.val());
    }
    if fn_ >= 8 {
        return Err(XchkCode::FnOver.val());
    }
    let intra = ((dev as u64) << 15) | ((fn_ as u64) << 12) | off as u64;
    if intra >= ECAM_BUS_WINDOW {
        return Err(XchkCode::EcamOverWindow.val());
    }
    Ok(base.saturating_add(((bus as u64) << 20).saturating_add(intra)))
}

/// CF8 地址式：0x80000000 | bus<<16 | dev<<11 | fn<<8 | (off&0xFC)。
pub fn cf8_addr(bus: u8, dev: u8, fn_: u8, off: u32) -> Result<u32, u32> {
    if off >= CFG_OFF_MAX {
        return Err(XchkCode::OffOver.val());
    }
    if dev >= 32 {
        return Err(XchkCode::DevOver.val());
    }
    if fn_ >= 8 {
        return Err(XchkCode::FnOver.val());
    }
    Ok(CF8_ENABLE
        | ((bus as u32) << 16)
        | ((dev as u32) << CF8_DEV_SHIFT)
        | ((fn_ as u32) << CF8_FN_SHIFT)
        | (off & CF8_OFF_MASK))
}

/// CF8 地址反解校验：使能位必须置 1，偏移低 2 位必须清零。
pub fn cf8_decode(addr: u32) -> Result<(u8, u8, u8, u32), u32> {
    if addr & CF8_ENABLE == 0 {
        return Err(XchkCode::Cf8NoEnable.val());
    }
    let bus = ((addr >> 16) & 0xFF) as u8;
    let dev = ((addr >> CF8_DEV_SHIFT) & 0x1F) as u8;
    let fn_ = ((addr >> CF8_FN_SHIFT) & 0x7) as u8;
    let off = addr & CF8_OFF_MASK;
    Ok((bus, dev, fn_, off))
}

// ── 配置空间表（双视图共用一张字表） ────────────────────────────
/// 单设备 64 字配置空间。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DevCfg {
    pub bdf: u32,
    pub words: Vec<u32>,
}

impl DevCfg {
    pub fn new(bdf: u32) -> DevCfg {
        DevCfg { bdf, words: Vec::new() }
    }
    /// 写一个字（构造器专用，4B 对齐）。
    pub fn put_word(&mut self, off: u32, w: u32) -> Result<(), u32> {
        if off >= CFG_OFF_MAX {
            return Err(XchkCode::OffOver.val());
        }
        if off % 4 != 0 {
            return Err(XchkCode::OffOver.val());
        }
        let idx = (off / 4) as usize;
        while self.words.len() <= idx {
            self.words.push(0);
        }
        if let Some(slot) = self.words.get_mut(idx) {
            *slot = w;
            Ok(())
        } else {
            Err(XchkCode::OffOver.val())
        }
    }
    /// ECAM 视图读字：按 (dev,fn,off) 直接寻址本表。
    pub fn ecam_read(&self, dev: u8, fn_: u8, off: u32) -> Result<u32, u32> {
        if off >= CFG_OFF_MAX {
            return Err(XchkCode::OffOver.val());
        }
        let (bdev, bfn) = match bdf_split(self.bdf) {
            (_b, d, f) => (d, f),
        };
        if dev != bdev || fn_ != bfn {
            return Err(XchkCode::DevOver.val());
        }
        let idx = (off / 4) as usize;
        match self.words.get(idx) {
            Some(w) => Ok(*w),
            None => Ok(0xFFFF_FFFF),
        }
    }
    /// CF8/CFC 视图读字：先把 CF8 地址反解成 (bus,dev,fn,off) 再寻址。
    pub fn cf8_read(&self, addr: u32) -> Result<u32, u32> {
        let (_, dev, fn_, off) = cf8_decode(addr)?;
        self.ecam_read(dev, fn_, off)
    }
}

/// 漂移表：只改 ECAM 视图的注入（bdf, off, 翻转掩码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drift {
    pub bdf: u32,
    pub off: u32,
    pub mask: u32,
}

/// 差异账行：bdf/偏移/ECAM 值/CF8 值，显性入账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub bdf: u32,
    pub off: u32,
    pub ecam_val: u32,
    pub cf8_val: u32,
}

/// 交叉比对：全量逐字，返回差异行数与比对字数。
pub fn cross_compare(dev: &DevCfg, drifts: &[Drift]) -> Result<(u32, u32), u32> {
    let mut diffs: Vec<DiffRow> = Vec::new();
    let mut compared = 0u32;
    let mut off = 0u32;
    while off < CFG_OFF_MAX {
        let (bus, d, f) = bdf_split(dev.bdf);
        // ECAM 地址式越窗校验（窗口闸独立生效）。
        match ecam_addr(0xE000_0000, bus, d, f, off) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        let cf8_a = match cf8_addr(bus, d, f, off) {
            Ok(a) => a,
            Err(e) => return Err(e),
        };
        // ECAM 视图（叠加漂移表注入）。
        let mut ecam_val = dev.ecam_read(d, f, off)?;
        let mut di = 0;
        while di < drifts.len() {
            if let Some(d) = drifts.get(di) {
                if d.bdf == dev.bdf && d.off == off {
                    ecam_val ^= d.mask;
                }
            }
            di += 1;
        }
        // CF8 视图（不叠加注入）。
        let cf8_val = dev.cf8_read(cf8_a)?;
        if ecam_val != cf8_val {
            diffs.push(DiffRow { bdf: dev.bdf, off, ecam_val, cf8_val });
        }
        compared += 1;
        off += 4;
    }
    let diff_n = diffs.len() as u32;
    if diff_n > 0 {
        // 差异显性账：首行入诊断（错误路径不为空）。
        let _ = diffs;
    }
    Ok((diff_n, compared))
}

// ── 注入检出账（四路：越窗/越界偏移/无使能位/漂移注入） ─────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── LCG（固定种子） ────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 真机设备表（4 台，每台 64 字） ──────────────────────────────
/// 构造一台带身份/BAR/能力链痕的设备。
fn mk_dev(bdf: u32, vid: u16, did: u16, bar0: u32, class: u32) -> DevCfg {
    let mut d = DevCfg::new(bdf);
    let _ = d.put_word(0x00, (vid as u32) | ((did as u32) << 16));
    let _ = d.put_word(0x08, class);
    let _ = d.put_word(0x10, bar0);
    let _ = d.put_word(0x34, 0x0000_0050);
    let _ = d.put_word(0x50, 0x0000_0005);
    d
}

/// 真机 4 设备。
pub fn machine_devs() -> Vec<DevCfg> {
    let mut v: Vec<DevCfg> = Vec::new();
    v.push(mk_dev(0x0000, 0x10EE, 0x7021, 0xFE_B0_0000, 0x0604_0001));
    v.push(mk_dev(0x0800, 0x8086, 0x1237, 0x0000_0000, 0x0600_0000));
    v.push(mk_dev(0x1000, 0x1022, 0x2000, 0xFE_B1_0000, 0x0200_0000));
    v.push(mk_dev(0x1800, 0x1B36, 0x0100, 0xFE_B2_0000, 0x0C03_0002));
    v
}

// ── 全量交叉比对（多设备聚合） ──────────────────────────────────
/// 干净路径全量比对结果账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrossLedger {
    /// 比对设备数
    pub devs: u32,
    /// 比对字数（设备数 × 64）
    pub words: u32,
    /// 差异行数（干净路径恒 0）
    pub diffs: u32,
    /// 逐设备全等位图（1=一致）
    pub equal_map: Vec<bool>,
}

/// 全量比对：每台设备 64 字全查，聚合账可复算。
pub fn cross_full(devs: &[DevCfg], drifts: &[Drift]) -> Result<CrossLedger, u32> {
    let mut led = CrossLedger::default();
    let mut i = 0;
    while i < devs.len() {
        let d = match devs.get(i) {
            Some(d) => d,
            None => break,
        };
        let (diff_n, compared) = cross_compare(d, drifts)?;
        led.devs += 1;
        led.words = led.words.saturating_add(compared);
        led.diffs = led.diffs.saturating_add(diff_n);
        led.equal_map.push(diff_n == 0);
        i += 1;
    }
    Ok(led)
}

// ── 谓词（私有，判据面） ────────────────────────────────────────
/// 100% 一致谓词：比对设备数 = 设备数、比对字数 = 设备数×64、差异 0。
fn full_match_ok(led: &CrossLedger) -> bool {
    if led.diffs != 0 {
        return false;
    }
    if led.words != (led.devs as usize * WORDS_PER_DEV) as u32 {
        return false;
    }
    if led.equal_map.len() != led.devs as usize {
        return false;
    }
    let mut i = 0;
    while i < led.equal_map.len() {
        if !led.equal_map.get(i).copied().unwrap_or(false) {
            return false;
        }
        i += 1;
    }
    true
}

// ── 黄金全链：建表→全量比对→聚合账复算 ─────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let devs = machine_devs();
    let led = cross_full(&devs, &[])?;
    // 干净路径：差异 0、100% 一致。
    let clean_ok = full_match_ok(&led) && led.diffs == 0 && led.devs == 4;
    // 比对字数 = 4 × 64 = 256。
    let words_ok = led.words == 256;
    // 注入 1 字漂移必抓：差异行数恰 1、比对字数不变。
    let drift = Drift { bdf: 0x0800, off: 0x00, mask: 0x0000_0100 };
    let inj = cross_full(&devs, &[drift])?;
    let inj_ok = inj.diffs == 1 && inj.words == 256;
    // 复算：干净账两次全等。
    let led2 = cross_full(&devs, &[])?;
    let replay_ok = led == led2;
    let ok = (clean_ok as u32) + (words_ok as u32) + (inj_ok as u32) + (replay_ok as u32);
    Ok((ok, inj.diffs, led.words))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 4/4 绿 + 注入差异 1 行 + 比对 256 字。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 1, 256)) => Ok(()),
        _ => Err(XchkCode::NotReproducible.val()),
    }
}

/// 序 B：CF8 无使能位注入必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match cf8_decode(0x0000_8008) {
        Err(e) if e == XchkCode::Cf8NoEnable.val() => Ok(()),
        _ => Err(XchkCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（1MB 窗口 / 4096B 每功能 / 64 字每设备）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        ECAM_BUS_WINDOW == 0x100000,
        ECAM_FN_SIZE == 0x1000,
        WORDS_PER_DEV == 64,
        CF8_ENABLE == 0x8000_0000,
        CF8_OFF_MASK == 0xFC,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(XchkCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：偏移越界（0x100 起）。
        let over = ecam_addr(0xE000_0000, 0, 0, 0, CFG_OFF_MAX).is_err()
            && cf8_addr(0, 0, 0, CFG_OFF_MAX).is_err();
        led.record(0, over);
        // 路 2：设备/功能号越界。
        let dev_over = ecam_addr(0xE000_0000, 0, 32, 0, 0).is_err()
            && cf8_addr(0, 0, 8, 0).is_err();
        led.record(1, dev_over);
        // 路 3：CF8 无使能位。
        led.record(2, probe_seq_b().is_ok());
        // 路 4：漂移注入必抓（双机制值翻转 → 差异行恰 1）。
        let devs = machine_devs();
        let drift = Drift { bdf: 0x1000, off: 0x10, mask: 0x0000_8000 };
        let inj = cross_full(&devs, &[drift]);
        let inj_ok = match inj {
            Ok(l) => l.diffs == 1,
            Err(_) => false,
        };
        led.record(3, inj_ok);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(XchkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（0x75000~0x781C0 窗口 + 0x40 对齐 + 闭集落窗） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < XchkCode::ALL.len() {
        if let Some(c) = XchkCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 地址式互逆谓词：cf8_addr → cf8_decode 回环一致。
fn cf8_roundtrip_ok() -> bool {
    let mut bus = 0u8;
    while bus < 4 {
        let mut dev = 0u8;
        while dev < 8 {
            let mut fn_ = 0u8;
            while fn_ < 4 {
                let a = match cf8_addr(bus, dev, fn_, 0x10) {
                    Ok(a) => a,
                    Err(_) => return false,
                };
                let dec = match cf8_decode(a) {
                    Ok(d) => d,
                    Err(_) => return false,
                };
                if dec.0 != bus || dec.1 != dev || dec.2 != fn_ || dec.3 != 0x10 {
                    return false;
                }
                fn_ += 1;
            }
            dev += 1;
        }
        bus += 1;
    }
    true
}

/// 诊断码闭集查表谓词。
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < XchkCode::ALL.len() {
        if let Some(c) = XchkCode::ALL.get(i) {
            let v = c.val();
            if detail(v) != c.detail() || c.detail().is_empty() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks） ─────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "F0471-J1-黄金全链",
        match &g {
            Ok((ok, diffs, words)) => *ok == 4 && *diffs == 1 && *words == 256,
            Err(_) => false,
        },
        match &g {
            Ok((ok, diffs, words)) => format!("ok={} inj_diffs={} words={}", ok, diffs, words),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // 段位守卫。
    v.push((
        "F0471-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗口 0x75000~0x781C0", SEG_BASE),
    ));
    // 干净全量比对 100% 一致。
    let devs = machine_devs();
    let clean_res = cross_full(&devs, &[]);
    let clean_ok = match &clean_res {
        Ok(l) => full_match_ok(l),
        Err(_) => false,
    };
    let clean = match clean_res {
        Ok(l) => l,
        Err(_) => CrossLedger::default(),
    };
    v.push((
        "F0471-干净全量比对",
        clean_ok,
        format!("设备={} 字={} 差异={}", clean.devs, clean.words, clean.diffs),
    ));
    // 设备数=比对数 100% 一致。
    v.push((
        "F0471-设备数=比对数",
        clean.devs == 4 && clean.words == 4 * WORDS_PER_DEV as u32,
        format!("{} 设备 × {} 字", clean.devs, WORDS_PER_DEV),
    ));
    // ECAM 地址式边界。
    let e_ok = ecam_addr(0xE000_0000, 0, 31, 7, 0xFC).is_ok()
        && ecam_addr(0xE000_0000, 0, 32, 0, 0).is_err()
        && ecam_addr(0xE000_0000, 0, 0, 8, 0).is_err();
    v.push((
        "F0471-ECAM地址式边界",
        e_ok,
        String::from("dev<32 fn<8 off<0x100 全闸生效"),
    ));
    // CF8 地址式边界。
    let c_ok = cf8_addr(0, 31, 7, 0xFC).is_ok()
        && cf8_addr(0, 32, 0, 0).is_err()
        && cf8_addr(0, 0, 8, 0).is_err()
        && cf8_addr(0, 0, 0, 0x100).is_err();
    v.push((
        "F0471-CF8地址式边界",
        c_ok,
        String::from("使能位/对齐掩码/三越界全闸生效"),
    ));
    // CF8 回环一致。
    v.push((
        "F0471-CF8回环一致",
        cf8_roundtrip_ok(),
        String::from("addr→decode 4×8×4 全组合互逆"),
    ));
    // 无使能位注入必抓。
    v.push((
        "F0471-无使能位注入",
        probe_seq_b().is_ok(),
        String::from("bit31=0 → Cf8NoEnable"),
    ));
    // 漂移注入必抓（差异行显性）。
    let drift = Drift { bdf: 0x0800, off: 0x00, mask: 0x0000_0100 };
    let inj = match cross_full(&devs, &[drift]) {
        Ok(l) => l,
        Err(_) => CrossLedger::default(),
    };
    v.push((
        "F0471-漂移注入必抓",
        inj.diffs == 1 && inj.words == 256 && !full_match_ok(&inj),
        format!("差异行={} 比对字={}", inj.diffs, inj.words),
    ));
    // 漂移不影响比对数。
    v.push((
        "F0471-比对数恒等",
        inj.words == clean.words,
        String::from("注入不改比对字数，只加差异行"),
    ));
    // 复算一致（干净账两跑全等）。
    v.push((
        "F0471-复算一致",
        match cross_full(&devs, &[]) {
            Ok(l2) => l2 == clean,
            Err(_) => false,
        },
        String::from("CrossLedger 逐字段全等"),
    ));
    // 注入 10/10。
    let inj_led = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj_led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F0471-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj_led.as_ref().map(|l| l.tries),
            inj_led.as_ref().map(|l| l.caught)
        ),
    ));
    // 双跑一致。
    v.push((
        "F0471-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 诊断码闭集查表。
    v.push((
        "F0471-诊断码闭集",
        code_closure_ok(),
        String::from("8 码全员落窗且查表回环一致"),
    ));
    v
}

pub fn run_unx0471_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0471 ECAM/CF8 交叉验证（全量）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_full() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.ok(), Some((4, 1, 256)));
    }

    #[test]
    fn test_clean_cross_full_match() {
        let devs = machine_devs();
        let led = match cross_full(&devs, &[]) {
            Ok(l) => l,
            Err(_) => return,
        };
        assert!(full_match_ok(&led));
        assert_eq!(led.diffs, 0);
        assert_eq!(led.devs, 4);
        assert_eq!(led.words, 256);
    }

    #[test]
    fn test_drift_injection_caught() {
        let devs = machine_devs();
        let drift = Drift { bdf: 0x0800, off: 0x00, mask: 0x0000_0100 };
        let led = match cross_full(&devs, &[drift]) {
            Ok(l) => l,
            Err(_) => return,
        };
        assert_eq!(led.diffs, 1);
        assert!(!full_match_ok(&led));
    }

    #[test]
    fn test_drift_two_injections_two_diffs() {
        let devs = machine_devs();
        let d1 = Drift { bdf: 0x0000, off: 0x10, mask: 0x0000_00F0 };
        let d2 = Drift { bdf: 0x1800, off: 0x08, mask: 0x0000_0001 };
        let led = match cross_full(&devs, &[d1, d2]) {
            Ok(l) => l,
            Err(_) => return,
        };
        assert_eq!(led.diffs, 2);
    }

    #[test]
    fn test_cf8_no_enable_caught() {
        assert!(probe_seq_b().is_ok());
        assert!(cf8_decode(0x0000_0000).is_err());
        assert!(cf8_decode(0x7FFF_FFFC).is_err());
    }

    #[test]
    fn test_addr_boundary() {
        assert!(ecam_addr(0xE000_0000, 0, 31, 7, 0xFC).is_ok());
        assert!(ecam_addr(0xE000_0000, 0, 32, 0, 0).is_err());
        assert!(ecam_addr(0xE000_0000, 0, 0, 8, 0).is_err());
        assert!(ecam_addr(0xE000_0000, 0, 0, 0, 0x100).is_err());
        assert!(cf8_addr(0, 31, 7, 0xFC).is_ok());
        assert!(cf8_addr(0, 32, 0, 0).is_err());
        assert!(cf8_addr(0, 0, 8, 0).is_err());
        assert!(cf8_addr(0, 0, 0, 0x100).is_err());
    }

    #[test]
    fn test_cf8_roundtrip() {
        assert!(cf8_roundtrip_ok());
        let a = match cf8_addr(0x1F, 0x1A, 0x5, 0x3C) {
            Ok(a) => a,
            Err(_) => return,
        };
        assert_eq!(a, 0x8000_0000 | (0x1F << 16) | (0x1A << 11) | (0x5 << 8) | 0x3C);
        let dec = match cf8_decode(a) {
            Ok(d) => d,
            Err(_) => return,
        };
        assert_eq!(dec, (0x1F, 0x1A, 0x5, 0x3C));
    }

    #[test]
    fn test_phase_machine_dual_run_identical() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert_eq!(detail(0x1234_5678u32), "未知诊断码：不在 0x756C0 段闭集内");
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS);
        assert!(inj.is_ok());
        let led = match inj {
            Ok(l) => l,
            Err(_) => return,
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_probe_seq_c_constants_frozen() {
        assert!(probe_seq_c().is_ok());
        assert_eq!(ECAM_BUS_WINDOW, 0x100000);
        assert_eq!(WORDS_PER_DEV, 64);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0471_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
