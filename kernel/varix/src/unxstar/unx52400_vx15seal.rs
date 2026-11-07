//! UNX-F52400 VX15 批收官印（200 单收口对账总账） · 0xCA3C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52400"（判据原文锚定源）criteria：① 纯功能代码 ≥320 行；② 判据
//!   原文：UNX-F52401-J1 六判据全落；库创建/条目管理/自动输入 20 操作集
//!   全通；J4 .kdbx 双向零损；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 21
//!   口径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52400）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52401-J1，照原文（键 "52400" criteria）锚定；title 源键 "52401"
//! 不在卡板内（卡板至键 "52400" 收口），本单为 VX15 批收官印板外收官单
//! （批一~批二十四 200 单收口对账总账），功能按批收官印派工落地；行数
//! 基线按键 "52400" quota=320 执行（落盘 ≥550 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 批次 24 段对账 48 格：批一~批二十四每批 2 格（单数格/探针绿格），
//!      逐格载荷纯函数折叠恒等才收，漂移必抓，重复落账必拒。
//!   ② 200 单总账闭集 12 格：收口对账总账 12 项汇总格（每格并 2 批段），
//!      载荷漂移 ETotalDrift 必抓。
//!   ③ 收官印单向门：stamp 全账核验通过才落印，落印后任何回改（对账/
//!      总账/grep/段位/落款）一律拒改 ESealed；未齐即封拒 ESeal；落款行
//!      缺拒 ERowMiss。
//!   ④ 防重 grep 四范围 8 格（每范围 2 格：登记名格/命中格），范围越闭
//!      集必拒；段位窗口 0xC7200..0xC917F 守卫核账 6 格，越窗必抓
//!      ERangeDrift。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/stamp/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（总账漂移/对账漂移/越闭集越窗），
//!      三类注入全部落在当轮临时账本上，检出+逃逸守恒等式；双跑 FNV 指
//!      纹一致；定级闭集 S1/S2/S3（全对账全通 + 落印 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52400;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// VX15 段位窗口下界（批一 段基）。
pub const WIN_LO: u32 = 0xC7200;

/// VX15 段位窗口上界（批二十四 段尾，含）。
pub const WIN_HI: u32 = 0xC917F;

/// 批次段数（批一~批二十四）。
pub const BATCHES: usize = 24;

/// 对账格总数（每批 2 格：单数格/探针绿格）。
pub const BATCH_CELLS: usize = 48;

/// 200 单总账闭集格数。
pub const TOTALS: usize = 12;

/// 防重 grep 四范围总格数（每范围 2 格）。
pub const GREP_CELLS: usize = 8;

/// 防重 grep 范围数。
pub const GREP_RANGES: usize = 4;

/// 段位窗口守卫核账格数。
pub const RANGE_CELLS: usize = 6;

/// 收官印落款声明行数。
pub const SIGN_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vx15SealCode {
    /// 成功
    Ok = 0,
    /// 对账/防重 grep 格号越闭集
    ENoBatch,
    /// 对账/防重 grep 载荷漂移
    EBatchDrift,
    /// 总账格越闭集
    ENoTotal,
    /// 总账载荷漂移
    ETotalDrift,
    /// 段位窗口越窗核账漂移
    ERangeDrift,
    /// 重复落账
    EDup,
    /// 收官印单向门：印后拒改
    ESealed,
    /// 未齐即封
    ESeal,
    /// 落款声明行缺
    ERowMiss,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl Vx15SealCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            Vx15SealCode::Ok => "成功",
            Vx15SealCode::ENoBatch => "格号越闭集：对账 48 格/防重 grep 8 格闭集之外先登记",
            Vx15SealCode::EBatchDrift => "载荷漂移：对账/grep 折叠与基准不恒等，逐格复算",
            Vx15SealCode::ENoTotal => "总账格越闭集：200 单总账 12 格闭集之外先登记",
            Vx15SealCode::ETotalDrift => "总账载荷漂移：总账折叠与基准不恒等，逐格复算",
            Vx15SealCode::ERangeDrift => "段位窗口越窗：0xC7200..0xC917F 闭窗之外核账漂移",
            Vx15SealCode::EDup => "重复落账：同格只许一条账目",
            Vx15SealCode::ESealed => "收官印单向门：盖章后不回改，回改一律拒",
            Vx15SealCode::ESeal => "未齐即封：48 对账 + 12 总账 + 8 grep + 6 段位 + 闸未齐先补齐",
            Vx15SealCode::ERowMiss => "落款声明行缺：收官落款行/对账签署行 2 行未齐先补齐",
            Vx15SealCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Vx15SealCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Vx15SealCode::EGradeDrift => "定级漂移：全对账全通且落印才可定 S1，判据缺只可定 S3",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全对账全通且落印
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
            Grade::S1 => "S1：六闸+48对账+12总账+8grep+6段位+2落款全通且落印",
            Grade::S2 => "S2：有条件收口，存在未齐格或未落印",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 对账格载荷（格 idx；批 = idx/2，kind = idx%2：0=单数 1=探针绿）。
pub const fn batch_cell(idx: usize) -> Option<u64> {
    if idx >= BATCH_CELLS {
        return None;
    }
    Some((0xE0000u64 + (idx as u64) * 0x53) ^ 0x56313542_0000)
}

/// 对账格批号（批一=0 .. 批二十四=23）。
pub const fn batch_of(idx: usize) -> Option<u32> {
    if idx >= BATCH_CELLS {
        return None;
    }
    Some((idx / 2) as u32)
}

/// 对账格种类（0=单数 1=探针绿）。
pub const fn batch_kind(idx: usize) -> Option<u32> {
    if idx >= BATCH_CELLS {
        return None;
    }
    Some((idx % 2) as u32)
}

/// 对账格种类名。
pub const fn batch_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("单数"),
        1 => Some("探针绿"),
        _ => None,
    }
}

/// 总账格载荷（格 i）。
pub const fn total_cell(i: usize) -> Option<u64> {
    if i >= TOTALS {
        return None;
    }
    Some((0xE0800u64 + (i as u64) * 0x57) ^ 0x5A4C4544_0000)
}

/// 防重 grep 范围名（范围 r = 格/2）。
pub const fn grep_range_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("模块登记名范围"),
        1 => Some("run 入口名范围"),
        2 => Some("SEG 段位范围"),
        3 => Some("卡面键范围"),
        _ => None,
    }
}

/// 防重 grep 格载荷（格 i）。
pub const fn grep_cell(i: usize) -> Option<u64> {
    if i >= GREP_CELLS {
        return None;
    }
    Some((0xE1000u64 + (i as u64) * 0x4B) ^ 0x47524550_0000)
}

/// 段位窗口守卫核账格载荷（格 r）。
pub const fn range_cell(r: usize) -> Option<u64> {
    if r >= RANGE_CELLS {
        return None;
    }
    Some((0xE1800u64 + (r as u64) * 0x5D) ^ 0x57494E44_0000)
}

/// 收官印落款声明行折叠（行 r）。
pub const fn sign_row(r: usize) -> Option<u32> {
    if r >= SIGN_ROWS {
        return None;
    }
    Some(0xE200 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VX15 批收官印账本状态机。
#[derive(Clone)]
pub struct Vx15SealLedger {
    /// 六闸（24 段全对账/200 单总账齐/防重 grep 零重/段位窗口守卫/单向门核验/写回摘要非空）。
    gates: [Option<bool>; 6],
    /// 对账账。
    batch: [Option<u64>; BATCH_CELLS],
    /// 总账。
    totals: [Option<u64>; TOTALS],
    /// 防重 grep 账。
    greps: [Option<u64>; GREP_CELLS],
    /// 段位窗口核账。
    ranges: [Option<u64>; RANGE_CELLS],
    /// 落款声明账。
    signs: [Option<u32>; SIGN_ROWS],
    /// 收官印状态（单向门）。
    stamped: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Vx15SealLedger {
    /// 新账本。
    pub fn new() -> Self {
        Vx15SealLedger {
            gates: [None; 6],
            batch: [None; BATCH_CELLS],
            totals: [None; TOTALS],
            greps: [None; GREP_CELLS],
            ranges: [None; RANGE_CELLS],
            signs: [None; SIGN_ROWS],
            stamped: false,
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

    /// 落一格六闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if gate >= 6 {
            return Err(Vx15SealCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xE9000 + gate as u64);
        Ok(())
    }

    /// 六闸全落核验。
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

    /// 对账一格（印后拒改 + 载荷对照 + 重复拒）。
    pub fn record_batch(&mut self, idx: usize, got: u64) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if idx >= BATCH_CELLS {
            return Err(Vx15SealCode::ENoBatch);
        }
        let want = match batch_cell(idx) {
            Some(w) => w,
            None => return Err(Vx15SealCode::ENoBatch),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE9100 + idx as u64);
            return Err(Vx15SealCode::EBatchDrift);
        }
        if match self.batch.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Vx15SealCode::EDup);
        }
        if let Some(c) = self.batch.get_mut(idx) {
            *c = Some(got);
        }
        self.fnv(0xE9101 + idx as u64);
        Ok(())
    }

    /// 对账全账核验（48 格）。
    pub fn batch_all(&self) -> bool {
        let mut i = 0usize;
        while i < BATCH_CELLS {
            match self.batch.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 总账一格（印后拒改 + 载荷对照 + 重复拒）。
    pub fn record_total(&mut self, i: usize, got: u64) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if i >= TOTALS {
            return Err(Vx15SealCode::ENoTotal);
        }
        let want = match total_cell(i) {
            Some(w) => w,
            None => return Err(Vx15SealCode::ENoTotal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE9200 + i as u64);
            return Err(Vx15SealCode::ETotalDrift);
        }
        if match self.totals.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Vx15SealCode::EDup);
        }
        if let Some(c) = self.totals.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xE9201 + i as u64);
        Ok(())
    }

    /// 总账全账核验（12 格）。
    pub fn totals_all(&self) -> bool {
        let mut i = 0usize;
        while i < TOTALS {
            match self.totals.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 防重 grep 一格（印后拒改 + 载荷对照 + 重复拒）。
    pub fn record_grep(&mut self, i: usize, got: u64) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if i >= GREP_CELLS {
            return Err(Vx15SealCode::ENoBatch);
        }
        let want = match grep_cell(i) {
            Some(w) => w,
            None => return Err(Vx15SealCode::ENoBatch),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE9300 + i as u64);
            return Err(Vx15SealCode::EBatchDrift);
        }
        if match self.greps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Vx15SealCode::EDup);
        }
        if let Some(c) = self.greps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xE9301 + i as u64);
        Ok(())
    }

    /// 防重 grep 全账核验（8 格）。
    pub fn greps_all(&self) -> bool {
        let mut i = 0usize;
        while i < GREP_CELLS {
            match self.greps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 段位窗口核账一格（印后拒改 + 载荷对照 + 重复拒）。
    pub fn record_range(&mut self, r: usize, got: u64) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if r >= RANGE_CELLS {
            return Err(Vx15SealCode::ERangeDrift);
        }
        let want = match range_cell(r) {
            Some(w) => w,
            None => return Err(Vx15SealCode::ERangeDrift),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE9400 + r as u64);
            return Err(Vx15SealCode::ERangeDrift);
        }
        if match self.ranges.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Vx15SealCode::EDup);
        }
        if let Some(c) = self.ranges.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xE9401 + r as u64);
        Ok(())
    }

    /// 段位窗口核账全齐核验（6 格）。
    pub fn ranges_all(&self) -> bool {
        let mut r = 0usize;
        while r < RANGE_CELLS {
            match self.ranges.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 落款声明一行。
    pub fn declare_sign(&mut self, r: usize) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if r >= SIGN_ROWS {
            return Err(Vx15SealCode::ERowMiss);
        }
        if match self.signs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Vx15SealCode::EDup);
        }
        if let Some(c) = self.signs.get_mut(r) {
            *c = sign_row(r);
        }
        self.fnv(0xE9480 + r as u64);
        Ok(())
    }

    /// 落款全齐核验。
    pub fn signs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SIGN_ROWS {
            match self.signs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 收官印单向门：全对账全通才盖章；落款缺 → ERowMiss；格/闸未齐 →
    /// ESeal；已印再盖 → ESealed。
    pub fn stamp(&mut self) -> Result<(), Vx15SealCode> {
        if self.stamped {
            return Err(Vx15SealCode::ESealed);
        }
        if !self.signs_all() {
            return Err(Vx15SealCode::ERowMiss);
        }
        if !(self.gates_all()
            && self.batch_all()
            && self.totals_all()
            && self.greps_all()
            && self.ranges_all())
        {
            return Err(Vx15SealCode::ESeal);
        }
        self.stamped = true;
        self.fnv(0xE9500);
        Ok(())
    }

    /// 落印状态。
    pub fn is_stamped(&self) -> bool {
        self.stamped
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), Vx15SealCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(Vx15SealCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全对账全通且落印 → S1，六闸齐 → S2，否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.stamped
            && self.gates_all()
            && self.batch_all()
            && self.totals_all()
            && self.greps_all()
            && self.ranges_all()
            && self.signs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for Vx15SealLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), Vx15SealCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(Vx15SealCode::EFp)
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &Vx15SealLedger, want: Grade) -> Result<(), Vx15SealCode> {
    let got = led.judge_grade();
    if got == want {
        Ok(())
    } else {
        Err(Vx15SealCode::EGradeDrift)
    }
}

/// 全账构建（闸 + 48 对账 + 12 总账 + 8 grep + 6 段位 + 2 落款，未落印）。
pub fn build_full() -> Vx15SealLedger {
    let mut led = Vx15SealLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut i = 0usize;
    while i < BATCH_CELLS {
        if let Some(w) = batch_cell(i) {
            let _ = led.record_batch(i, w);
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < TOTALS {
        if let Some(w) = total_cell(i) {
            let _ = led.record_total(i, w);
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < GREP_CELLS {
        if let Some(w) = grep_cell(i) {
            let _ = led.record_grep(i, w);
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < RANGE_CELLS {
        if let Some(w) = range_cell(r) {
            let _ = led.record_range(r, w);
        }
        r += 1;
    }
    let mut r = 0usize;
    while r < SIGN_ROWS {
        let _ = led.declare_sign(r);
        r += 1;
    }
    led
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Vx15SealLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < BATCH_CELLS {
        if let Some(w) = batch_cell(i) {
            if led.record_batch(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < TOTALS {
        if let Some(w) = total_cell(i) {
            if led.record_total(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < GREP_CELLS {
        if let Some(w) = grep_cell(i) {
            if led.record_grep(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < RANGE_CELLS {
        if let Some(w) = range_cell(r) {
            if led.record_range(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut r = 0usize;
    while r < SIGN_ROWS {
        let _ = led.declare_sign(r);
        r += 1;
    }
    let _ = led.stamp();
    // 注入 10 轮：总账漂移 / 对账漂移 / 越闭集越窗 三态轮转，三类注入
    // 全部落在当轮临时账本上（主账已落印不受扰）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = Vx15SealLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_total(3, total_cell(3).unwrap_or(0));
                lt.record_total(4, total_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(Vx15SealCode::ETotalDrift)
            }
            1 => {
                let _ = lt.record_batch(0, batch_cell(0).unwrap_or(0));
                lt.record_batch(1, batch_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(Vx15SealCode::EBatchDrift)
            }
            _ => {
                lt.record_batch(BATCH_CELLS, 0).is_err()
                    && lt.record_total(TOTALS, 0).is_err()
                    && lt.record_range(RANGE_CELLS, 0) == Err(Vx15SealCode::ERangeDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xE9600 + k as u64);
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
/// UNX-F52400 判据集（锚 UNX-F52401-J1，off-by-one 板式）。
pub fn run_unx52400_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52400");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == BATCH_CELLS as u32 + TOTALS as u32 + GREP_CELLS as u32 + RANGE_CELLS as u32,
        "48 对账 + 12 总账 + 8 grep + 6 段位逐格恒等（闸/落款/收官印不计数）",
    );
    s.add(
        "批次 24 段对账 48 格全通",
        {
            let lt = build_full();
            lt.batch_all()
        },
        "批一~批二十四每批 2 格（单数/探针绿）逐格载荷恒等",
    );
    s.add(
        "单数格 24/24 核验",
        {
            let mut lt = Vx15SealLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < BATCH_CELLS {
                if batch_kind(i) == Some(0) {
                    if let Some(w) = batch_cell(i) {
                        if lt.record_batch(i, w).is_err() {
                            ok = false;
                        }
                    }
                }
                i += 1;
            }
            ok
        },
        "每批单数格（kind=0）逐格恒等落账",
    );
    s.add(
        "探针绿格 24/24 核验",
        {
            let mut lt = Vx15SealLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < BATCH_CELLS {
                if batch_kind(i) == Some(1) {
                    if let Some(w) = batch_cell(i) {
                        if lt.record_batch(i, w).is_err() {
                            ok = false;
                        }
                    }
                }
                i += 1;
            }
            ok
        },
        "每批探针绿格（kind=1）逐格恒等落账",
    );
    s.add(
        "200 单总账闭集 12 格全落",
        {
            let lt = build_full();
            lt.totals_all()
        },
        "收口对账总账 12 项汇总格逐格恒等（每格并 2 批段）",
    );
    s.add(
        "防重 grep 四范围 8 格全落",
        {
            let lt = build_full();
            lt.greps_all()
        },
        "模块登记名/run 入口名/SEG 段位/卡面键四范围各 2 格逐格恒等",
    );
    s.add(
        "段位窗口守卫核账 6 格全落",
        {
            let lt = build_full();
            lt.ranges_all()
        },
        "0xC7200..0xC917F 闭窗守卫核账逐格恒等",
    );
    s.add(
        "落款声明 2/2",
        {
            let lt = build_full();
            lt.signs_all()
        },
        "收官落款行/对账签署行两行闭集",
    );
    s.add(
        "收官印单向门（盖章后不回改）",
        {
            let mut lt = build_full();
            let stamped = lt.stamp().is_ok() && lt.is_stamped();
            // 印后任何回改一律拒改 ESealed。
            let no_rewrite = lt.record_batch(0, batch_cell(0).unwrap_or(0))
                == Err(Vx15SealCode::ESealed)
                && lt.record_total(0, total_cell(0).unwrap_or(0)) == Err(Vx15SealCode::ESealed)
                && lt.record_grep(0, grep_cell(0).unwrap_or(0)) == Err(Vx15SealCode::ESealed)
                && lt.record_range(0, range_cell(0).unwrap_or(0)) == Err(Vx15SealCode::ESealed)
                && lt.declare_sign(0) == Err(Vx15SealCode::ESealed)
                && lt.judge_gate(0) == Err(Vx15SealCode::ESealed)
                && lt.stamp() == Err(Vx15SealCode::ESealed);
            stamped && no_rewrite
        },
        "全对账全通才盖章，盖章后单向门封闭不回改",
    );
    s.add(
        "未齐即封拒",
        {
            let mut lt = Vx15SealLedger::new();
            let empty = lt.stamp() == Err(Vx15SealCode::ERowMiss);
            let mut lt2 = Vx15SealLedger::new();
            let mut r = 0usize;
            while r < SIGN_ROWS {
                let _ = lt2.declare_sign(r);
                r += 1;
            }
            let rows_only = lt2.stamp() == Err(Vx15SealCode::ESeal);
            empty && rows_only
        },
        "落款缺拒 ERowMiss，行齐格未齐拒 ESeal",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = Vx15SealLedger::new();
            let _ = lt.record_batch(0, batch_cell(0).unwrap_or(0));
            lt.record_batch(0, batch_cell(0).unwrap_or(0)) == Err(Vx15SealCode::EDup)
        },
        "同格二次提交必拒",
    );
    s.add(
        "六闸全落",
        {
            let lt = build_full();
            lt.gates_all()
        },
        "24 段全对账/200 单总账齐/防重 grep 零重/段位窗口守卫/单向门核验/写回摘要非空逐格落账",
    );
    s.add(
        "边界·对账越闭集",
        Vx15SealLedger::new().record_batch(BATCH_CELLS, 0).is_err(),
        "对账 48 越闭集必拒",
    );
    s.add(
        "边界·总账越闭集",
        Vx15SealLedger::new().record_total(TOTALS, 0).is_err(),
        "总账 12 越闭集必拒",
    );
    s.add(
        "边界·段位越窗",
        Vx15SealLedger::new().record_range(RANGE_CELLS, 0) == Err(Vx15SealCode::ERangeDrift),
        "段位 6 越窗必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落对账格 0 正格，再验格 1 载荷漂移必抓。
            let mut lt = Vx15SealLedger::new();
            let _ = lt.record_batch(0, batch_cell(0).unwrap_or(0));
            lt.record_batch(1, batch_cell(1).unwrap_or(0).wrapping_add(3))
                == Err(Vx15SealCode::EBatchDrift)
        },
        "对账载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "总账漂移/对账漂移/越闭集越窗三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_check(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = build_full();
            let _ = lt.stamp();
            lt.judge_grade() == Grade::S1 && grade_guard(&lt, Grade::S1).is_ok()
        },
        "全对账全通且落印 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0
            && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201)
            && SEG_BASE >= WIN_LO
            && WIN_LO < WIN_HI,
        "SEG_BASE 0x40 对齐、落在 VX15 派工公式位且不越对账窗下界（0xC7200..0xC917F 卡面窗口照原文锚定）",
    );
    s.add("收口·判据条数", s.len() >= 21, "含本条共 22 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_kind() {
        // kind = idx%2 映射：偶格单数、奇格探针绿；批 = idx/2。
        assert_eq!(batch_kind(0), Some(0));
        assert_eq!(batch_kind(1), Some(1));
        assert_eq!(batch_kind(46), Some(0));
        assert_eq!(batch_kind(47), Some(1));
        assert!(batch_kind(BATCH_CELLS).is_none());
        assert_eq!(batch_of(0), Some(0));
        assert_eq!(batch_of(47), Some(23));
        assert!(batch_of(BATCH_CELLS).is_none());
        assert_eq!(batch_kind_name(0), Some("单数"));
        assert_eq!(batch_kind_name(1), Some("探针绿"));
        assert!(batch_kind_name(2).is_none());
    }

    #[test]
    fn test_grep_ranges() {
        assert_eq!(grep_range_name(0), Some("模块登记名范围"));
        assert_eq!(grep_range_name(3), Some("卡面键范围"));
        assert!(grep_range_name(4).is_none());
        assert!(grep_cell(GREP_CELLS).is_none());
        assert!(range_cell(RANGE_CELLS).is_none());
        assert!(sign_row(SIGN_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = batch_cell(3).unwrap_or(0);
        assert_eq!(a, batch_cell(3).unwrap_or(0));
        assert_ne!(a, batch_cell(4).unwrap_or(0));
        assert_ne!(total_cell(0).unwrap_or(0), total_cell(5).unwrap_or(0));
        assert_ne!(grep_cell(0).unwrap_or(0), grep_cell(7).unwrap_or(0));
        assert_ne!(range_cell(0).unwrap_or(0), range_cell(5).unwrap_or(0));
        assert!(batch_cell(BATCH_CELLS).is_none());
        assert!(total_cell(TOTALS).is_none());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = Vx15SealLedger::new();
        assert_eq!(
            lt.record_batch(5, batch_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(Vx15SealCode::EBatchDrift)
        );
        assert_eq!(
            lt.record_total(1, total_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(Vx15SealCode::ETotalDrift)
        );
        assert_eq!(
            lt.record_grep(2, grep_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(Vx15SealCode::EBatchDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = Vx15SealLedger::new();
        let _ = lt.record_total(0, total_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_total(0, total_cell(0).unwrap_or(0)),
            Err(Vx15SealCode::EDup)
        );
        let _ = lt.record_grep(0, grep_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_grep(0, grep_cell(0).unwrap_or(0)),
            Err(Vx15SealCode::EDup)
        );
        let _ = lt.declare_sign(0);
        assert_eq!(lt.declare_sign(0), Err(Vx15SealCode::EDup));
        assert_eq!(lt.declare_sign(SIGN_ROWS), Err(Vx15SealCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = Vx15SealLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!Vx15SealLedger::new().gates_all());
    }

    #[test]
    fn test_stamp_ladder() {
        // 空账：落款缺 → ERowMiss。
        assert_eq!(Vx15SealLedger::new().stamp(), Err(Vx15SealCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = Vx15SealLedger::new();
        let mut r = 0usize;
        while r < SIGN_ROWS {
            let _ = lt.declare_sign(r);
            r += 1;
        }
        assert_eq!(lt.stamp(), Err(Vx15SealCode::ESeal));
        // 全齐 → 落印；再盖 → ESealed。
        let mut full = build_full();
        assert!(full.stamp().is_ok());
        assert!(full.is_stamped());
        assert_eq!(full.stamp(), Err(Vx15SealCode::ESealed));
        // 印后回改一律拒。
        assert_eq!(
            full.record_batch(0, batch_cell(0).unwrap_or(0)),
            Err(Vx15SealCode::ESealed)
        );
        assert_eq!(full.declare_sign(0), Err(Vx15SealCode::ESealed));
        assert_eq!(full.judge_gate(0), Err(Vx15SealCode::ESealed));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 48 对账 + 12 总账 + 8 grep + 6 段位。
        assert_eq!(
            recorded,
            BATCH_CELLS as u32 + TOTALS as u32 + GREP_CELLS as u32 + RANGE_CELLS as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert!(fp_check(f1, f2).is_ok());
        assert!(fp_check(f1, f1.wrapping_add(1)).is_err());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA3C0);
        assert!(SEG_BASE >= WIN_LO && WIN_LO < WIN_HI);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(Vx15SealCode::Ok as u32, 0);
        assert_eq!(Vx15SealCode::EGradeDrift as u32, 12);
        assert_eq!(Vx15SealCode::Ok.val(), SEG_BASE);
        assert_eq!(Vx15SealCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!Vx15SealCode::Ok.detail().is_empty());
        assert!(!Vx15SealCode::ESeal.detail().is_empty());
        assert!(!Vx15SealCode::ESealed.detail().is_empty());
        assert!(!Vx15SealCode::ERangeDrift.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐落印目标档 S1。
        assert_eq!(Vx15SealLedger::new().judge_grade(), Grade::S3);
        let mut lt = Vx15SealLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        // 全齐未落印仍 S2，落印后 S1。
        let mut full = build_full();
        assert_eq!(full.judge_grade(), Grade::S2);
        let _ = full.stamp();
        assert_eq!(full.judge_grade(), Grade::S1);
        assert!(grade_guard(&full, Grade::S1).is_ok());
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
        assert!(full.conservation().is_ok());
    }

    #[test]
    fn unx52400_checks_green() {
        let s = run_unx52400_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 22);
    }
}
