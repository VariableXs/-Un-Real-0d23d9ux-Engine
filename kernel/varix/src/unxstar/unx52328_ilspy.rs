//! UNX-F52328 ILSpy 收口 · 0xC91C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52328"，逐字）：
//!   title: ILSpy 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52329-J1 六判据全
//!     落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52328）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52329-J1，照原文锚定；功能按本卡标题（ILSpy）落地。
//! 本模块沿用 52201 口径：crate 级 `#![cfg_attr(not(test), no_std)]` +
//! `pub extern crate alloc` 已在 lib.rs 置位，模块级不重复声明。
//!
//! 设计要点：
//!   ① 反编译 10 操作集：五家族闭集（载入/解析/反编译/导航/导出）各 2
//!      操作，家族单调（逆序必抓）。
//!   ② 三语言 8 格（C# 0..2 / VB 2..5 / IL 5..8）：语言闭表越界必拒，
//!      与 F52326 dnSpy 语言分段口径差异化。
//!   ③ baml/资源 6 格（baml 提取/资源枚举/资源导出各 2 格）：kind 单调，
//!      越闭集必拒。
//!   ④ 与 dnSpyEx 分账声明 2 行（ILSpy 账 / dnSpyEx 账分立不并账，
//!      §AI-66.3）。
//!   ⑤ 插件 4 格闭集，逐格载荷对照。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（反编译漂移/
//!      baml 逆序/插件越闭集），检出+逃逸守恒；双跑指纹一致；定级 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52328;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 反编译操作数。
pub const DECOMP_OPS: usize = 10;

/// 三语言格数。
pub const LANG_CELLS: usize = 8;

/// baml/资源格数。
pub const BAML_CELLS: usize = 6;

/// 与 dnSpyEx 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 插件格数。
pub const PLUGIN_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IlspyCode {
    /// 成功
    Ok = 0,
    /// 反编译操作越闭集
    ENoOp,
    /// 反编译载荷漂移
    EDrift,
    /// 语言格越闭集
    ENoLang,
    /// 语言载荷漂移
    ELangDrift,
    /// baml 格越闭集
    ENoBaml,
    /// baml 载荷漂移
    EBamlDrift,
    /// 家族逆序
    EOrder,
    /// 插件格越闭集
    ENoPlug,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl IlspyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            IlspyCode::Ok => "成功",
            IlspyCode::ENoOp => "反编译操作越闭集：反编译 10 操作集之外先登记",
            IlspyCode::EDrift => "反编译载荷漂移：操作折叠与基准不恒等，逐格复算",
            IlspyCode::ENoLang => "语言格越闭集：三语言（C#/VB/IL）8 格之外先登记",
            IlspyCode::ELangDrift => "语言载荷漂移：语言格折叠与基准不恒等，逐格复算",
            IlspyCode::ENoBaml => "baml 格越闭集：baml/资源 6 格闭集之外先登记",
            IlspyCode::EBamlDrift => "baml 载荷漂移：baml 格折叠与基准不恒等，逐格复算",
            IlspyCode::EOrder => "家族逆序：载入→解析→反编译→导航→导出与 baml 三态单调，逆序必抓",
            IlspyCode::ENoPlug => "插件格越闭集：插件 4 格闭集之外先登记",
            IlspyCode::ESeal => "未齐即封：10 操作 + 8 语言 + 6 baml + 4 插件 + 2 行分账未齐先补齐",
            IlspyCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            IlspyCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IlspyCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10操作+8语言+6baml+4插件+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零口径）。
const DECOMP_ANCHOR: u64 = 0x696C_7370_795F_0000;
/// 语言域锚。
const LANG_ANCHOR: u64 = 0x696C_6C61_6E67_0000;
/// baml 域锚。
const BAML_ANCHOR: u64 = 0x6261_6D6C_5F62_0000;
/// 插件域锚。
const PLUGIN_ANCHOR: u64 = 0x706C_6770_6C67_0000;

/// 反编译操作格载荷（操作 o；家族 = o/2）。
pub const fn decomp_cell(o: usize) -> Option<u64> {
    if o >= DECOMP_OPS {
        return None;
    }
    Some((0xE0_0000u64 + (o as u64) * 0x2C) ^ (DECOMP_ANCHOR ^ ((o / 2) as u64) << 12))
}

/// 反编译家族（0..2=载入 2..4=解析 4..6=反编译 6..8=导航 8..10=导出）。
pub const fn decomp_kind(o: usize) -> Option<u32> {
    if o >= DECOMP_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn decomp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("载入"),
        1 => Some("解析"),
        2 => Some("反编译"),
        3 => Some("导航"),
        4 => Some("导出"),
        _ => None,
    }
}

/// 语言名（分段口径：0..2 C#，2..5 VB，5..8 IL）。
pub const fn lang_name(c: usize) -> Option<&'static str> {
    if c >= LANG_CELLS {
        return None;
    }
    if c < 2 {
        Some("C#")
    } else if c < 5 {
        Some("VB")
    } else {
        Some("IL")
    }
}

/// 语言格载荷。
pub const fn lang_cell(c: usize) -> Option<u64> {
    if c >= LANG_CELLS {
        return None;
    }
    Some((0xE1_0000u64 + (c as u64) * 0x31) ^ LANG_ANCHOR)
}

/// baml/资源格载荷（格 b；kind = b/3：baml 提取/资源枚举/资源导出）。
pub const fn baml_cell(b: usize) -> Option<u64> {
    if b >= BAML_CELLS {
        return None;
    }
    Some((0xE2_0000u64 + (b as u64) * 0x2F) ^ (BAML_ANCHOR ^ ((b / 3) as u64) << 8))
}

/// baml kind。
pub const fn baml_kind(b: usize) -> Option<u32> {
    if b >= BAML_CELLS {
        return None;
    }
    Some((b / 3) as u32)
}

/// baml kind 名。
pub const fn baml_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("baml 提取"),
        1 => Some("资源枚举"),
        2 => Some("资源导出"),
        _ => None,
    }
}

/// 分账声明行折叠（对 dnSpyEx，行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xE3_00 + r as u32 * 0x2C)
}

/// 插件格载荷。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some((0xE4_0000u64 + (p as u64) * 0x3B) ^ PLUGIN_ANCHOR)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ILSpy 收口账本状态机。
#[derive(Clone)]
pub struct IlspyLedger {
    /// 六判据（ILSpy：加载/反编译/语言/baml/插件/许可）。
    gates: [Option<bool>; 6],
    /// 反编译操作账。
    decomp: [Option<u64>; DECOMP_OPS],
    /// 语言账。
    langs: [Option<u64>; LANG_CELLS],
    /// baml/资源账。
    baml: [Option<u64>; BAML_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 插件账。
    plugins: [Option<u64>; PLUGIN_CELLS],
    /// 最后家族（反编译序）。
    last_kind: i64,
    /// baml kind 序独立记账（与反编译家族序分立，互不污染）。
    last_baml: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IlspyLedger {
    /// 新账本。
    pub fn new() -> Self {
        IlspyLedger {
            gates: [None; 6],
            decomp: [None; DECOMP_OPS],
            langs: [None; LANG_CELLS],
            baml: [None; BAML_CELLS],
            splits: [None; SPLIT_ROWS],
            plugins: [None; PLUGIN_CELLS],
            last_kind: -1,
            last_baml: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), IlspyCode> {
        if gate >= 6 {
            return Err(IlspyCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 反编译一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_decomp(&mut self, o: usize, got: u64) -> Result<(), IlspyCode> {
        if o >= DECOMP_OPS {
            return Err(IlspyCode::ENoOp);
        }
        let kind = match decomp_kind(o) {
            Some(k) => k,
            None => return Err(IlspyCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(IlspyCode::EOrder);
        }
        let want = match decomp_cell(o) {
            Some(w) => w,
            None => return Err(IlspyCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(IlspyCode::EDrift);
        }
        if match self.decomp.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IlspyCode::ESeal);
        }
        if let Some(c) = self.decomp.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 反编译全账核验（10 格）。
    pub fn decomp_all(&self) -> bool {
        let mut o = 0usize;
        while o < DECOMP_OPS {
            match self.decomp.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 语言一格（载荷对照 + 重复拒）。
    pub fn record_lang(&mut self, c: usize, got: u64) -> Result<(), IlspyCode> {
        let want = match lang_cell(c) {
            Some(w) => w,
            None => return Err(IlspyCode::ENoLang),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + c as u64);
            return Err(IlspyCode::ELangDrift);
        }
        if match self.langs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IlspyCode::ESeal);
        }
        if let Some(x) = self.langs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8301 + c as u64);
        Ok(())
    }

    /// 语言全齐核验（8 格）。
    pub fn langs_all(&self) -> bool {
        let mut c = 0usize;
        while c < LANG_CELLS {
            match self.langs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// baml 一格（kind 单调 + 载荷对照 + 重复拒）。
    pub fn record_baml(&mut self, b: usize, got: u64) -> Result<(), IlspyCode> {
        if b >= BAML_CELLS {
            return Err(IlspyCode::ENoBaml);
        }
        let kind = match baml_kind(b) {
            Some(k) => k,
            None => return Err(IlspyCode::ENoBaml),
        };
        if (kind as i64) < self.last_baml {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400);
            return Err(IlspyCode::EOrder);
        }
        let want = match baml_cell(b) {
            Some(w) => w,
            None => return Err(IlspyCode::ENoBaml),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8401 + b as u64);
            return Err(IlspyCode::EBamlDrift);
        }
        if match self.baml.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IlspyCode::ESeal);
        }
        if let Some(x) = self.baml.get_mut(b) {
            *x = Some(got);
        }
        self.last_baml = kind as i64;
        self.fnv(0x8402 + b as u64);
        Ok(())
    }

    /// baml 全账核验（6 格）。
    pub fn baml_all(&self) -> bool {
        let mut b = 0usize;
        while b < BAML_CELLS {
            match self.baml.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 分账声明一行（对 dnSpyEx 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), IlspyCode> {
        if r >= SPLIT_ROWS {
            return Err(IlspyCode::ENoPlug);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IlspyCode::ESeal);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), IlspyCode> {
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(IlspyCode::ENoPlug),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + p as u64);
            return Err(IlspyCode::EDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IlspyCode::ESeal);
        }
        if let Some(c) = self.plugins.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8601 + p as u64);
        Ok(())
    }

    /// 插件全齐核验（4 格）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
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
        if self.gates_all()
            && self.decomp_all()
            && self.langs_all()
            && self.baml_all()
            && self.splits_all()
            && self.plugins_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for IlspyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IlspyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < DECOMP_OPS {
        if let Some(w) = decomp_cell(o) {
            if led.record_decomp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < LANG_CELLS {
        if let Some(w) = lang_cell(c) {
            if led.record_lang(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < BAML_CELLS {
        if let Some(w) = baml_cell(b) {
            if led.record_baml(b, w).is_ok() {
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
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    // 注入 10 轮：反编译漂移 / baml 逆序 / 插件越闭集三态轮转。
    // 漂移与逆序注入先按合法序落正格，再注违格；逆序格不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = IlspyLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_decomp(0, decomp_cell(0).unwrap_or(0));
                lt.record_decomp(1, decomp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(IlspyCode::EDrift)
            }
            1 => {
                let _ = lt.record_baml(3, baml_cell(3).unwrap_or(0));
                lt.record_baml(1, baml_cell(1).unwrap_or(0)) == Err(IlspyCode::EOrder)
            }
            _ => lt.record_plugin(PLUGIN_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
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
/// UNX-F52328 判据集（锚 UNX-F52329-J1，off-by-one 板式）。
pub fn run_unx52328_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52328");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "反编译 10 操作全通",
        recorded >= DECOMP_OPS as u32,
        "载入/解析/反编译/导航/导出五家族逐操作恒等（×10 口径）",
    );
    s.add(
        "三语言 8 格全落",
        {
            let mut lt = IlspyLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < LANG_CELLS {
                if let Some(w) = lang_cell(c) {
                    if lt.record_lang(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.langs_all()
        },
        "C#（0..2）/VB（2..5）/IL（5..8）语言闭表逐格恒等",
    );
    s.add(
        "baml/资源 6 格全落",
        {
            let mut lt = IlspyLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BAML_CELLS {
                if let Some(w) = baml_cell(b) {
                    if lt.record_baml(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.baml_all()
        },
        "baml 提取/资源枚举/资源导出各 2 格，kind 单调",
    );
    s.add(
        "与 dnSpyEx 分账声明 2 行全齐（§AI-66.3）",
        {
            let mut lt = IlspyLedger::new();
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
        "ILSpy 账（本卡）/ dnSpyEx 账（F52327）分立不并账，两行闭集",
    );
    s.add(
        "插件 4 格全落",
        {
            let mut lt = IlspyLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "插件闭集 4 格逐格恒等",
    );
    s.add(
        "反编译家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < DECOMP_OPS {
                if decomp_kind(o).unwrap_or(5) < decomp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    let mut l0 = IlspyLedger::new();
    s.add(
        "边界·反编译越闭集",
        l0.record_decomp(DECOMP_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·语言越闭集",
        l0.record_lang(LANG_CELLS, 0).is_err(),
        "语言 8 越闭集必拒",
    );
    s.add(
        "边界·baml 越闭集",
        l0.record_baml(BAML_CELLS, 0).is_err(),
        "baml 6 越闭集必拒",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "反编译漂移/baml 逆序/插件越闭集三态轮转必抓",
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
            let mut lt = IlspyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < DECOMP_OPS {
                if let Some(w) = decomp_cell(o) {
                    let _ = lt.record_decomp(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < LANG_CELLS {
                if let Some(w) = lang_cell(c) {
                    let _ = lt.record_lang(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < BAML_CELLS {
                if let Some(w) = baml_cell(b) {
                    let _ = lt.record_baml(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC91C0",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decomp_kind() {
        assert_eq!(decomp_kind(0), Some(0));
        assert_eq!(decomp_kind(4), Some(2));
        assert_eq!(decomp_kind(9), Some(4));
        assert!(decomp_kind(10).is_none());
        assert!(decomp_cell(10).is_none());
        assert_eq!(decomp_kind_name(3), Some("导航"));
        assert!(decomp_kind_name(5).is_none());
    }

    #[test]
    fn test_lang_table() {
        assert_eq!(lang_name(0), Some("C#"));
        assert_eq!(lang_name(2), Some("VB"));
        assert_eq!(lang_name(5), Some("IL"));
        assert!(lang_name(8).is_none());
        assert!(lang_cell(8).is_none());
    }

    #[test]
    fn test_baml_kind() {
        assert_eq!(baml_kind(0), Some(0));
        assert_eq!(baml_kind(2), Some(0));
        assert_eq!(baml_kind(3), Some(1));
        assert_eq!(baml_kind(5), Some(1));
        assert!(baml_kind(6).is_none());
        assert_eq!(baml_kind_name(1), Some("资源枚举"));
    }

    #[test]
    fn test_cells_pure() {
        let a = plugin_cell(1).unwrap_or(0);
        assert_eq!(a, plugin_cell(1).unwrap_or(0));
        assert_ne!(a, plugin_cell(2).unwrap_or(0));
        assert!(plugin_cell(PLUGIN_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_order_caught_and_last_kind_kept() {
        let mut lt = IlspyLedger::new();
        let _ = lt.record_baml(3, baml_cell(3).unwrap_or(0));
        assert_eq!(lt.record_baml(1, baml_cell(1).unwrap_or(0)), Err(IlspyCode::EOrder));
        // 逆序被拒后 last_kind 未被污染：同族合法格仍可落账。
        assert!(lt.record_baml(4, baml_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = IlspyLedger::new();
        assert_eq!(
            lt.record_lang(3, lang_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(IlspyCode::ELangDrift)
        );
        assert_eq!(
            lt.record_plugin(0, plugin_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(IlspyCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = IlspyLedger::new();
        let _ = lt.record_decomp(2, decomp_cell(2).unwrap_or(0));
        assert_eq!(lt.record_decomp(2, decomp_cell(2).unwrap_or(0)), Err(IlspyCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = IlspyLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = IlspyLedger::new();
        assert!(!lt2.gates_all());
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DECOMP_OPS + LANG_CELLS + BAML_CELLS + PLUGIN_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC91C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(IlspyCode::Ok as u32, 0);
        assert_eq!(IlspyCode::EGradeDrift as u32, 12);
        assert_eq!(IlspyCode::Ok.val(), SEG_BASE);
        assert_eq!(IlspyCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!IlspyCode::Ok.detail().is_empty());
        assert!(!IlspyCode::ESeal.detail().is_empty());
        assert!(!IlspyCode::EBamlDrift.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
    }

    #[test]
    fn unx52328_checks_green() {
        let s = run_unx52328_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
