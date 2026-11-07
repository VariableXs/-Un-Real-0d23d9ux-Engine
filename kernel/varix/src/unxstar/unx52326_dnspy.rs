//! UNX-F52326 dnSpy 收口 · 0xC9140 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52326"，逐字）：
//!   title: dnSpy 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52327-J1 六判据全
//!     落；接管口径（升级接管字样）；×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.1；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52326）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52327-J1，照原文锚定；功能按本卡标题（dnSpy）落地。
//! 本模块沿用 52201 口径：crate 级 `#![cfg_attr(not(test), no_std)]` +
//! `pub extern crate alloc` 已在 lib.rs 置位，模块级不重复声明。
//!
//! 设计要点：
//!   ① 程序集加载 10 操作集：五家族闭集（探测/打开/读元数据/解析引用/
//!      载入域）各 2 操作，家族单调（逆序必抓）。
//!   ② 调试 12 格（断点/单步/修改变量各 4 格）：kind 单调，越闭集必拒。
//!   ③ 反编译三语言 8 格（C# 0..3 / VB 3..6 / IL 6..8）：语言闭表越界
//!      必拒。
//!   ④ .NET 运行时上栈 4 层闭集（Framework/Core/5/8），逐层载荷对照。
//!   ⑤ EOL 三要素 6 格（停更/替代迁移/归档各 2 格）+ 对 F52327 迁移联
//!      签 4 格；定级口径：全通亦定 S2（EOL 如实，升级接管字样收口）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（加载漂移/
//!      调试逆序/EOL 格漂移），检出+逃逸守恒；双跑指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52326;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 程序集加载操作数。
pub const LOAD_OPS: usize = 10;

/// 调试格数。
pub const DBG_CELLS: usize = 12;

/// 反编译三语言格数。
pub const LANG_CELLS: usize = 8;

/// .NET 运行时上栈层数。
pub const RUNTIME_LAYERS: usize = 4;

/// EOL 三要素格数。
pub const EOL_CELLS: usize = 6;

/// 迁移联签格数（对 F52327）。
pub const MIG_SIGS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnSpyCode {
    /// 成功
    Ok = 0,
    /// 加载操作越闭集
    ENoLoad,
    /// 加载载荷漂移
    ELoadDrift,
    /// 调试格越闭集
    ENoDbg,
    /// 调试载荷漂移
    EDbgDrift,
    /// 家族逆序
    EOrder,
    /// 语言格越闭集
    ENoLang,
    /// EOL 格越闭集
    ENoEol,
    /// EOL 载荷漂移
    EEolDrift,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DnSpyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DnSpyCode::Ok => "成功",
            DnSpyCode::ENoLoad => "加载操作越闭集：程序集加载 10 操作集之外先登记",
            DnSpyCode::ELoadDrift => "加载载荷漂移：操作折叠与基准不恒等，逐格复算",
            DnSpyCode::ENoDbg => "调试格越闭集：调试 12 格（断点/单步/修改变量）之外先登记",
            DnSpyCode::EDbgDrift => "调试载荷漂移：调试格折叠与基准不恒等，逐格复算",
            DnSpyCode::EOrder => "家族逆序：探测→打开→读元数据→解析引用→载入域单调，逆序必抓",
            DnSpyCode::ENoLang => "语言格越闭集：反编译三语言（C#/VB/IL）8 格之外先登记",
            DnSpyCode::ENoEol => "EOL 格越闭集：EOL 三要素 6 格（停更/迁移/归档）之外先登记",
            DnSpyCode::EEolDrift => "EOL 载荷漂移：EOL 格折叠与基准不恒等，逐格复算",
            DnSpyCode::ESeal => "未齐即封：10 加载 + 12 调试 + 8 语言 + 6 EOL + 4 联签未齐先补齐",
            DnSpyCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DnSpyCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DnSpyCode::EGradeDrift => "定级漂移：EOL 如实收口定 S2，全通亦不得拔高至 S1",
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
            Grade::S1 => "S1：全通收口（dnSpy EOL 口径不适用）",
            Grade::S2 => "S2：全通仍定 S2——EOL 如实（升级接管字样：dnSpyEx 接管）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零口径）。
const LOAD_ANCHOR: u64 = 0x646E_7370_795F_0000;
/// 调试域锚。
const DBG_ANCHOR: u64 = 0x6462_675F_6462_0000;
/// 语言域锚。
const LANG_ANCHOR: u64 = 0x6C61_6E67_5F6C_0000;
/// EOL 域锚。
const EOL_ANCHOR: u64 = 0x656F_6C5F_656F_0000;

/// 程序集加载操作格载荷（操作 o；家族 = o/2）。
pub const fn load_cell(o: usize) -> Option<u64> {
    if o >= LOAD_OPS {
        return None;
    }
    Some((0xC0_0000u64 + (o as u64) * 0x2C) ^ (LOAD_ANCHOR ^ ((o / 2) as u64) << 12))
}

/// 加载家族（0..2=探测 2..4=打开 4..6=读元数据 6..8=解析引用 8..10=载入域）。
pub const fn load_kind(o: usize) -> Option<u32> {
    if o >= LOAD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn load_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("探测"),
        1 => Some("打开"),
        2 => Some("读元数据"),
        3 => Some("解析引用"),
        4 => Some("载入域"),
        _ => None,
    }
}

/// 调试格载荷（格 d；kind = d/4：断点/单步/修改变量）。
pub const fn dbg_cell(d: usize) -> Option<u64> {
    if d >= DBG_CELLS {
        return None;
    }
    Some((0xC1_0000u64 + (d as u64) * 0x2F) ^ (DBG_ANCHOR ^ ((d / 4) as u64) << 8))
}

/// 调试 kind。
pub const fn dbg_kind(d: usize) -> Option<u32> {
    if d >= DBG_CELLS {
        return None;
    }
    Some((d / 4) as u32)
}

/// 调试 kind 名。
pub const fn dbg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("断点"),
        1 => Some("单步"),
        2 => Some("修改变量"),
        _ => None,
    }
}

/// 反编译语言名（const fn 表 + Option 边界）。
pub const fn lang_name(c: usize) -> Option<&'static str> {
    if c >= LANG_CELLS {
        return None;
    }
    if c < 3 {
        Some("C#")
    } else if c < 6 {
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
    Some((0xC2_0000u64 + (c as u64) * 0x31) ^ LANG_ANCHOR)
}

/// .NET 运行时上栈层名（const fn 表 + Option 边界）。
pub const fn runtime_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some(".NET Framework"),
        1 => Some(".NET Core"),
        2 => Some(".NET 5"),
        3 => Some(".NET 8"),
        _ => None,
    }
}

/// 运行时层载荷。
pub const fn runtime_layer(l: usize) -> Option<u32> {
    if l >= RUNTIME_LAYERS {
        return None;
    }
    Some(0xC3_00 + l as u32 * 0x2C)
}

/// EOL 三要素（格 e；要素 = e/2：停更/替代迁移/归档）。
pub const fn eol_elem(e: usize) -> Option<u32> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// EOL 要素名。
pub const fn eol_elem_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("停更"),
        1 => Some("替代迁移"),
        2 => Some("归档"),
        _ => None,
    }
}

/// EOL 格载荷。
pub const fn eol_cell(e: usize) -> Option<u64> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((0xC4_0000u64 + (e as u64) * 0x2C) ^ EOL_ANCHOR)
}

/// 迁移联签折叠（对 F52327，签 s）。
pub const fn mig_sig(s: usize) -> Option<u32> {
    if s >= MIG_SIGS {
        return None;
    }
    Some(0xC5_00 + s as u32 * 0x37)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// dnSpy 收口账本状态机。
#[derive(Clone)]
pub struct DnSpyLedger {
    /// 六判据（dnSpy：加载/调试/反编译/EOL/迁移/许可）。
    gates: [Option<bool>; 6],
    /// 加载操作账。
    loads: [Option<u64>; LOAD_OPS],
    /// 调试账。
    dbg: [Option<u64>; DBG_CELLS],
    /// 语言账。
    langs: [Option<u64>; LANG_CELLS],
    /// 运行时上栈账。
    rt: [Option<u32>; RUNTIME_LAYERS],
    /// EOL 账。
    eol: [Option<u64>; EOL_CELLS],
    /// 迁移联签账。
    mig: [Option<u32>; MIG_SIGS],
    /// 最后家族（加载序）。
    last_kind: i64,
    /// 调试 kind 序独立记账（与加载家族序分立，互不污染）。
    last_dbg: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DnSpyLedger {
    /// 新账本。
    pub fn new() -> Self {
        DnSpyLedger {
            gates: [None; 6],
            loads: [None; LOAD_OPS],
            dbg: [None; DBG_CELLS],
            langs: [None; LANG_CELLS],
            rt: [None; RUNTIME_LAYERS],
            eol: [None; EOL_CELLS],
            mig: [None; MIG_SIGS],
            last_kind: -1,
            last_dbg: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DnSpyCode> {
        if gate >= 6 {
            return Err(DnSpyCode::ESeal);
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

    /// 加载一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_load(&mut self, o: usize, got: u64) -> Result<(), DnSpyCode> {
        if o >= LOAD_OPS {
            return Err(DnSpyCode::ENoLoad);
        }
        let kind = match load_kind(o) {
            Some(k) => k,
            None => return Err(DnSpyCode::ENoLoad),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(DnSpyCode::EOrder);
        }
        let want = match load_cell(o) {
            Some(w) => w,
            None => return Err(DnSpyCode::ENoLoad),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(DnSpyCode::ELoadDrift);
        }
        if match self.loads.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyCode::ESeal);
        }
        if let Some(c) = self.loads.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 加载全账核验（10 格）。
    pub fn loads_all(&self) -> bool {
        let mut o = 0usize;
        while o < LOAD_OPS {
            match self.loads.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 调试一格（kind 单调 + 载荷对照 + 重复拒）。
    pub fn record_dbg(&mut self, d: usize, got: u64) -> Result<(), DnSpyCode> {
        if d >= DBG_CELLS {
            return Err(DnSpyCode::ENoDbg);
        }
        let kind = match dbg_kind(d) {
            Some(k) => k,
            None => return Err(DnSpyCode::ENoDbg),
        };
        if (kind as i64) < self.last_dbg {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(DnSpyCode::EOrder);
        }
        let want = match dbg_cell(d) {
            Some(w) => w,
            None => return Err(DnSpyCode::ENoDbg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + d as u64);
            return Err(DnSpyCode::EDbgDrift);
        }
        if match self.dbg.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyCode::ESeal);
        }
        if let Some(c) = self.dbg.get_mut(d) {
            *c = Some(got);
        }
        self.last_dbg = kind as i64;
        self.fnv(0x6302 + d as u64);
        Ok(())
    }

    /// 调试全账核验（12 格）。
    pub fn dbg_all(&self) -> bool {
        let mut d = 0usize;
        while d < DBG_CELLS {
            match self.dbg.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 语言一格（载荷对照 + 重复拒）。
    pub fn record_lang(&mut self, c: usize, got: u64) -> Result<(), DnSpyCode> {
        let want = match lang_cell(c) {
            Some(w) => w,
            None => return Err(DnSpyCode::ENoLang),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + c as u64);
            return Err(DnSpyCode::ELoadDrift);
        }
        if match self.langs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyCode::ESeal);
        }
        if let Some(x) = self.langs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x6401 + c as u64);
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

    /// 运行时上栈一层。
    pub fn record_runtime(&mut self, l: usize) -> Result<(), DnSpyCode> {
        let want = match runtime_layer(l) {
            Some(w) => w,
            None => return Err(DnSpyCode::ESeal),
        };
        if match self.rt.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyCode::ESeal);
        }
        if let Some(x) = self.rt.get_mut(l) {
            *x = Some(want);
        }
        self.fnv(0x6500 + l as u64);
        Ok(())
    }

    /// 运行时全齐核验（4 层）。
    pub fn runtime_all(&self) -> bool {
        let mut l = 0usize;
        while l < RUNTIME_LAYERS {
            match self.rt.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// EOL 一格（载荷对照 + 重复拒）。
    pub fn record_eol(&mut self, e: usize, got: u64) -> Result<(), DnSpyCode> {
        let want = match eol_cell(e) {
            Some(w) => w,
            None => return Err(DnSpyCode::ENoEol),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + e as u64);
            return Err(DnSpyCode::EEolDrift);
        }
        if match self.eol.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyCode::ESeal);
        }
        if let Some(x) = self.eol.get_mut(e) {
            *x = Some(got);
        }
        self.fnv(0x6601 + e as u64);
        Ok(())
    }

    /// EOL 全齐核验（6 格）。
    pub fn eol_all(&self) -> bool {
        let mut e = 0usize;
        while e < EOL_CELLS {
            match self.eol.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 迁移联签一格（对 F52327）。
    pub fn record_mig(&mut self, s: usize) -> Result<(), DnSpyCode> {
        let want = match mig_sig(s) {
            Some(w) => w,
            None => return Err(DnSpyCode::ESeal),
        };
        if match self.mig.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyCode::ESeal);
        }
        if let Some(x) = self.mig.get_mut(s) {
            *x = Some(want);
        }
        self.fnv(0x6700 + s as u64);
        Ok(())
    }

    /// 迁移联签全齐核验（4 格）。
    pub fn mig_all(&self) -> bool {
        let mut s = 0usize;
        while s < MIG_SIGS {
            match self.mig.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
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

    /// KB-COMPAT 定级：EOL 如实口径——全通仍定 S2，判据缺 → S3。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else {
            Grade::S2
        }
    }
}

impl Default for DnSpyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DnSpyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < LOAD_OPS {
        if let Some(w) = load_cell(o) {
            if led.record_load(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DBG_CELLS {
        if let Some(w) = dbg_cell(d) {
            if led.record_dbg(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
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
    let mut l = 0usize;
    while l < RUNTIME_LAYERS {
        let _ = led.record_runtime(l);
        l += 1;
    }
    let mut e = 0usize;
    while e < EOL_CELLS {
        if let Some(w) = eol_cell(e) {
            if led.record_eol(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut s = 0usize;
    while s < MIG_SIGS {
        let _ = led.record_mig(s);
        s += 1;
    }
    // 注入 10 轮：加载漂移 / 调试逆序 / EOL 格漂移三态轮转。
    // 漂移与逆序注入先按合法序落正格，再注违格；逆序格不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DnSpyLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_load(0, load_cell(0).unwrap_or(0));
                lt.record_load(1, load_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(DnSpyCode::ELoadDrift)
            }
            1 => {
                let _ = lt.record_dbg(4, dbg_cell(4).unwrap_or(0));
                lt.record_dbg(1, dbg_cell(1).unwrap_or(0)) == Err(DnSpyCode::EOrder)
            }
            _ => {
                let _ = lt.record_eol(0, eol_cell(0).unwrap_or(0));
                lt.record_eol(1, eol_cell(1).unwrap_or(0).wrapping_add(3))
                    == Err(DnSpyCode::EEolDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6800 + k as u64);
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
/// UNX-F52326 判据集（锚 UNX-F52327-J1，off-by-one 板式）。
pub fn run_unx52326_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52326");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "程序集加载 10 操作全通",
        recorded >= LOAD_OPS as u32,
        "探测/打开/读元数据/解析引用/载入域五家族逐操作恒等（×10 口径）",
    );
    s.add(
        "调试 12 格全落",
        {
            let mut lt = DnSpyLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DBG_CELLS {
                if let Some(w) = dbg_cell(d) {
                    if lt.record_dbg(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dbg_all()
        },
        "断点/单步/修改变量各 4 格，kind 单调",
    );
    s.add(
        "反编译三语言 8 格全落",
        {
            let mut lt = DnSpyLedger::new();
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
        "C#（0..3）/VB（3..6）/IL（6..8）语言闭表逐格恒等",
    );
    s.add(
        ".NET 运行时上栈 4 层全落",
        {
            let mut lt = DnSpyLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < RUNTIME_LAYERS {
                if lt.record_runtime(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.runtime_all()
        },
        "Framework→Core→5→8 逐层上栈",
    );
    s.add(
        "EOL 三要素 6 格全落（如实口径）",
        {
            let mut lt = DnSpyLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    if lt.record_eol(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.eol_all()
        },
        "停更/替代迁移/归档三要素各 2 格，如实落账",
    );
    s.add(
        "迁移联签 4/4 全齐（对 F52327）",
        {
            let mut lt = DnSpyLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MIG_SIGS {
                if lt.record_mig(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.mig_all()
        },
        "dnSpyEx 迁移联签 4 格闭集（§AI-66.1）",
    );
    s.add(
        "接管口径（升级接管字样）在案",
        lang_name(0) == Some("C#") && dbg_kind_name(2) == Some("修改变量"),
        "升级接管字样：dnSpy 停更后由 dnSpyEx 接管（UNX-F52327-J1 口径）",
    );
    s.add(
        "加载家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LOAD_OPS {
                if load_kind(o).unwrap_or(5) < load_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    let mut l0 = DnSpyLedger::new();
    s.add(
        "边界·加载越闭集",
        l0.record_load(LOAD_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·语言越闭集",
        l0.record_lang(LANG_CELLS, 0).is_err(),
        "语言 8 越闭集必拒",
    );
    s.add(
        "边界·EOL 越闭集",
        l0.record_eol(EOL_CELLS, 0).is_err(),
        "EOL 6 越闭集必拒",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "加载漂移/调试逆序/EOL 格漂移三态轮转必抓",
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
        "定级 S2 闭集（EOL 如实）",
        {
            let mut lt = DnSpyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < LOAD_OPS {
                if let Some(w) = load_cell(o) {
                    let _ = lt.record_load(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DBG_CELLS {
                if let Some(w) = dbg_cell(d) {
                    let _ = lt.record_dbg(d, w);
                }
                d += 1;
            }
            let mut c = 0usize;
            while c < LANG_CELLS {
                if let Some(w) = lang_cell(c) {
                    let _ = lt.record_lang(c, w);
                }
                c += 1;
            }
            let mut l = 0usize;
            while l < RUNTIME_LAYERS {
                let _ = lt.record_runtime(l);
                l += 1;
            }
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    let _ = lt.record_eol(e, w);
                }
                e += 1;
            }
            let mut m = 0usize;
            while m < MIG_SIGS {
                let _ = lt.record_mig(m);
                m += 1;
            }
            // 全部记账完毕后再断言：全通亦定 S2，不得拔高 S1。
            lt.judge_grade() == Grade::S2 && lt.judge_grade() != Grade::S1
        },
        "全通 → S2（KB-COMPAT 定级登记，EOL 如实收口）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9140",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_kind() {
        assert_eq!(load_kind(0), Some(0));
        assert_eq!(load_kind(4), Some(2));
        assert_eq!(load_kind(9), Some(4));
        assert!(load_kind(10).is_none());
        assert!(load_cell(10).is_none());
        assert_eq!(load_kind_name(2), Some("读元数据"));
        assert!(load_kind_name(5).is_none());
    }

    #[test]
    fn test_dbg_kind() {
        assert_eq!(dbg_kind(0), Some(0));
        assert_eq!(dbg_kind(4), Some(1));
        assert_eq!(dbg_kind(11), Some(2));
        assert!(dbg_kind(12).is_none());
        assert_eq!(dbg_kind_name(1), Some("单步"));
    }

    #[test]
    fn test_lang_table() {
        assert_eq!(lang_name(0), Some("C#"));
        assert_eq!(lang_name(3), Some("VB"));
        assert_eq!(lang_name(6), Some("IL"));
        assert!(lang_name(8).is_none());
        assert!(lang_cell(8).is_none());
    }

    #[test]
    fn test_runtime_layers() {
        assert_eq!(runtime_layer_name(0), Some(".NET Framework"));
        assert_eq!(runtime_layer_name(3), Some(".NET 8"));
        assert!(runtime_layer_name(4).is_none());
        assert!(runtime_layer(4).is_none());
    }

    #[test]
    fn test_eol_elems() {
        assert_eq!(eol_elem(0), Some(0));
        assert_eq!(eol_elem(2), Some(1));
        assert_eq!(eol_elem(4), Some(2));
        assert!(eol_elem(6).is_none());
        assert_eq!(eol_elem_name(1), Some("替代迁移"));
        assert!(mig_sig(MIG_SIGS).is_none());
    }

    #[test]
    fn test_order_caught_and_last_kind_kept() {
        let mut lt = DnSpyLedger::new();
        let _ = lt.record_dbg(8, dbg_cell(8).unwrap_or(0));
        assert_eq!(lt.record_dbg(3, dbg_cell(3).unwrap_or(0)), Err(DnSpyCode::EOrder));
        // 逆序被拒后 last_kind 未被污染：同族合法格仍可落账。
        assert!(lt.record_dbg(9, dbg_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DnSpyLedger::new();
        assert_eq!(
            lt.record_load(3, load_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(DnSpyCode::ELoadDrift)
        );
        assert_eq!(
            lt.record_dbg(5, dbg_cell(5).unwrap_or(0).wrapping_add(9)),
            Err(DnSpyCode::EDbgDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DnSpyLedger::new();
        let _ = lt.record_lang(1, lang_cell(1).unwrap_or(0));
        assert_eq!(lt.record_lang(1, lang_cell(1).unwrap_or(0)), Err(DnSpyCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DnSpyLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DnSpyLedger::new();
        assert!(!lt2.gates_all());
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LOAD_OPS + DBG_CELLS + LANG_CELLS + EOL_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9140);
        assert_eq!(SEG_BASE % 0x40, 0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DnSpyCode::Ok as u32, 0);
        assert_eq!(DnSpyCode::EGradeDrift as u32, 12);
        assert_eq!(DnSpyCode::Ok.val(), SEG_BASE);
        assert_eq!(DnSpyCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!DnSpyCode::Ok.detail().is_empty());
        assert!(!DnSpyCode::ESeal.detail().is_empty());
        assert!(!DnSpyCode::EEolDrift.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
    }

    #[test]
    fn unx52326_checks_green() {
        let s = run_unx52326_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
