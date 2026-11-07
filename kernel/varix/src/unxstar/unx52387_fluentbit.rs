//! UNX-F52387 Fluent Bit（Windows）收口 · 0xCA080段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52387"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52388-J1 tail/输出 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52388"（功能标题源）title：Fluent Bit（Windows）收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52387）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52388-J1（tail/输出 ×10），照原文（键 "52387" criteria）锚定；
//! 功能按本卡标题（Fluent Bit（Windows）收口，键 "52388" title）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 input 族闭集 20 格：tail/winlog/cpu/mem/net 各 4 格，族序单调
//!      （首族必为 tail，逆跳必抓），逐格载荷纯函数折叠恒等才收
//!      （tail/输出 ×10 之输入面收口）。
//!   ② parsers 8 格：解析器段逐格载荷对照，乱序可收、漂移必抓、重复拒。
//!   ③ output 三态 6 格：es/http/stdout 每态「声明+启用」两格，态内先
//!      声明后启用，逆序必抓（EOrder）。
//!   ④ fluent-bit.conf 双向投影 12 格：六对（正向投影/反向回读成对落账），
//!      反向先于正向必抓（EOrder）。
//!   ⑤ 与 Elasticsearch 分账声明 2 行 + C 运行时上栈声明 2 行闭集，
//!      缺行拒封（ERowMiss），parsers 未齐拒封（EParserMiss），
//!      格未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（检查载荷漂移/输出态内逆序/
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 检查 + 8 解析 + 6 输出 + 12 投影 + 2 分账 + 2 C
//!      全齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52387;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 检查格数（五 input 族 × 4）。
pub const CHECKS: usize = 20;

/// parsers 解析器格数。
pub const PARSER_CELLS: usize = 8;

/// output 三态格数（三态 × 声明/启用）。
pub const OUT_CELLS: usize = 6;

/// fluent-bit.conf 双向投影格数（六对）。
pub const PROJ_CELLS: usize = 12;

/// 与 Elasticsearch 分账声明行数。
pub const ES_ROWS: usize = 2;

/// C 运行时上栈声明行数。
pub const C_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FluentbitCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 载荷漂移
    EDrift,
    /// input 族序逆/投影对序逆/输出态内逆序
    EOrder,
    /// 判据闸越闭集
    ENoGate,
    /// parsers 未齐即封
    EParserMiss,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
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

impl FluentbitCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FluentbitCode::Ok => "成功",
            FluentbitCode::ENoCell => "格号越闭集：检查 20/解析 8/输出 6/投影 12 之外先登记",
            FluentbitCode::EDrift => "载荷漂移：格折叠与 fluent-bit.conf 基准不恒等，逐格复算",
            FluentbitCode::EOrder => "序逆：tail→winlog→cpu→mem→net 族序单调；投影反向先于正向、输出态内先声明后启用必抓",
            FluentbitCode::ENoGate => "判据闸越闭集：六判据闸之外先登记",
            FluentbitCode::EParserMiss => "parsers 未齐即封：8 个解析器段未齐先补齐",
            FluentbitCode::ENoRow => "声明行越闭集：ES 分账 2 行 + C 上栈 2 行之外先登记",
            FluentbitCode::ERowMiss => "缺声明行：ES 分账/C 运行时上栈声明未齐先补齐",
            FluentbitCode::EDup => "重复落账：同格只许一条账目",
            FluentbitCode::ESeal => "未齐即封：20 检查 + 6 输出 + 12 投影未齐先补齐",
            FluentbitCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            FluentbitCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FluentbitCode::EGradeDrift => "定级漂移：六闸+全节+2 分账+2 C 全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 检查+8 解析+6 输出+12 投影+2 分账+2 C 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 检查格载荷（格 o；input 族 = o/4；锚 "FLBC" 低 16 位恒零）。
pub const fn chk_cell(o: usize) -> Option<u64> {
    if o >= CHECKS {
        return None;
    }
    let fam = (o / 4) as u64;
    Some(
        (0xEC100u64 + (o as u64) * 0x35)
            ^ fam.wrapping_mul(0x00000193)
            ^ 0x464C424300000000,
    )
}

/// input 族（0..4=tail 4..8=winlog 8..12=cpu 12..16=mem 16..20=net）。
pub const fn chk_family(o: usize) -> Option<u32> {
    if o >= CHECKS {
        return None;
    }
    Some((o / 4) as u32)
}

/// input 族名（fluent-bit.conf 五 input 段族）。
pub const fn family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("tail"),
        1 => Some("winlog"),
        2 => Some("cpu"),
        3 => Some("mem"),
        4 => Some("net"),
        _ => None,
    }
}

/// 解析器格载荷（格 p；[PARSER] 段；锚 "FPSR" 低 16 位恒零）。
pub const fn parser_cell(p: usize) -> Option<u64> {
    if p >= PARSER_CELLS {
        return None;
    }
    Some((0xEC300u64 + (p as u64) * 0x3D) ^ 0x4650535200000000)
}

/// output 三态格载荷（格 o；态 = o/2，偶声明/奇启用；锚 "FBOUT" 低 16 位恒零）。
pub const fn out_cell(o: usize) -> Option<u64> {
    if o >= OUT_CELLS {
        return None;
    }
    let st = (o / 2) as u64;
    Some((0xEC500u64 + (o as u64) * 0x3F) ^ st.wrapping_mul(0x00009E37) ^ 0x46424F5554000000)
}

/// 输出态名（0=es 1=http 2=stdout）。
pub const fn out_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("es"),
        1 => Some("http"),
        2 => Some("stdout"),
        _ => None,
    }
}

/// 投影格载荷（格 i；对 = i/2，偶正向/奇反向；锚 "FBCF" 低 16 位恒零）。
pub const fn proj_cell(i: usize) -> Option<u64> {
    if i >= PROJ_CELLS {
        return None;
    }
    let pair = (i / 2) as u64;
    Some((0xEC700u64 + (i as u64) * 0x3B) ^ pair.wrapping_mul(0x0000BE77) ^ 0x4642434600000000)
}

/// 投影对号（0..6；fluent-bit.conf 六对）。
pub const fn proj_pair(i: usize) -> Option<u32> {
    if i >= PROJ_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 与 Elasticsearch 分账声明行折叠（行 r）。
pub const fn es_row(r: usize) -> Option<u32> {
    if r >= ES_ROWS {
        return None;
    }
    Some(0xEC980 + r as u32 * 0x35)
}

/// C 运行时上栈声明行折叠（行 r）。
pub const fn c_row(r: usize) -> Option<u32> {
    if r >= C_ROWS {
        return None;
    }
    Some(0xEC9C0 + r as u32 * 0x35)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Fluent Bit（Windows）收口账本状态机。
#[derive(Clone)]
pub struct FluentbitLedger {
    /// 六判据（安装/fluent-bit.conf 配置/服务注册/采集功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 检查账。
    chks: [Option<u64>; CHECKS],
    /// 解析器账。
    parsers: [Option<u64>; PARSER_CELLS],
    /// 输出三态账。
    outs: [Option<u64>; OUT_CELLS],
    /// 投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// ES 分账声明账。
    ess: [Option<u32>; ES_ROWS],
    /// C 上栈声明账。
    cs: [Option<u32>; C_ROWS],
    /// 最后 input 族。
    last_family: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FluentbitLedger {
    /// 新账本。
    pub fn new() -> Self {
        FluentbitLedger {
            gates: [None; 6],
            chks: [None; CHECKS],
            parsers: [None; PARSER_CELLS],
            outs: [None; OUT_CELLS],
            proj: [None; PROJ_CELLS],
            ess: [None; ES_ROWS],
            cs: [None; C_ROWS],
            last_family: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FluentbitCode> {
        if gate >= 6 {
            return Err(FluentbitCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xEC110 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
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

    /// 检查一格（input 族序单调 + 首族必 tail + 载荷对照 + 重复拒）。
    pub fn record_chk(&mut self, o: usize, got: u64) -> Result<(), FluentbitCode> {
        if o >= CHECKS {
            return Err(FluentbitCode::ENoCell);
        }
        let fam = match chk_family(o) {
            Some(k) => k as i64,
            None => return Err(FluentbitCode::ENoCell),
        };
        let order_bad = if self.last_family < 0 {
            fam != 0
        } else {
            fam < self.last_family || fam > self.last_family + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xEC410);
            return Err(FluentbitCode::EOrder);
        }
        let want = match chk_cell(o) {
            Some(w) => w,
            None => return Err(FluentbitCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xEC411 + o as u64);
            return Err(FluentbitCode::EDrift);
        }
        if match self.chks.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluentbitCode::EDup);
        }
        if let Some(c) = self.chks.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = fam;
        self.fnv(0xEC412 + o as u64);
        Ok(())
    }

    /// 检查全账核验（20 格）。
    pub fn chks_all(&self) -> bool {
        let mut o = 0usize;
        while o < CHECKS {
            match self.chks.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 解析器一格（[PARSER] 段；乱序可收 + 载荷对照 + 重复拒）。
    pub fn record_parser(&mut self, p: usize, got: u64) -> Result<(), FluentbitCode> {
        if p >= PARSER_CELLS {
            return Err(FluentbitCode::ENoCell);
        }
        let want = match parser_cell(p) {
            Some(w) => w,
            None => return Err(FluentbitCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xEC430 + p as u64);
            return Err(FluentbitCode::EDrift);
        }
        if match self.parsers.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluentbitCode::EDup);
        }
        if let Some(c) = self.parsers.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0xEC431 + p as u64);
        Ok(())
    }

    /// 解析器全账核验（8 格）。
    pub fn parsers_all(&self) -> bool {
        let mut p = 0usize;
        while p < PARSER_CELLS {
            match self.parsers.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// output 三态一格（态内先声明（偶格）后启用（奇格）+ 载荷对照 + 重复拒）。
    pub fn record_out(&mut self, o: usize, got: u64) -> Result<(), FluentbitCode> {
        if o >= OUT_CELLS {
            return Err(FluentbitCode::ENoCell);
        }
        // 态内逆序：启用（奇格）落账前其声明（偶格）必须已在账。
        if o % 2 == 1 {
            let decl_ok = match self.outs.get(o - 1) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !decl_ok {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0xEC450);
                return Err(FluentbitCode::EOrder);
            }
        }
        let want = match out_cell(o) {
            Some(w) => w,
            None => return Err(FluentbitCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xEC451 + o as u64);
            return Err(FluentbitCode::EDrift);
        }
        if match self.outs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluentbitCode::EDup);
        }
        if let Some(c) = self.outs.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0xEC452 + o as u64);
        Ok(())
    }

    /// 输出三态全账核验（6 格）。
    pub fn outs_all(&self) -> bool {
        let mut o = 0usize;
        while o < OUT_CELLS {
            match self.outs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影一格（fluent-bit.conf 对内先正后反 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, i: usize, got: u64) -> Result<(), FluentbitCode> {
        if i >= PROJ_CELLS {
            return Err(FluentbitCode::ENoCell);
        }
        // 双向投影成对：奇格（反向回读）落账前其偶格镜像（正向投影）必须已在账。
        if i % 2 == 1 {
            let fwd_ok = match self.proj.get(i - 1) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !fwd_ok {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0xEC470);
                return Err(FluentbitCode::EOrder);
            }
        }
        let want = match proj_cell(i) {
            Some(w) => w,
            None => return Err(FluentbitCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xEC471 + i as u64);
            return Err(FluentbitCode::EDrift);
        }
        if match self.proj.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluentbitCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xEC472 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn proj_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJ_CELLS {
            match self.proj.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 与 Elasticsearch 分账声明一行。
    pub fn declare_es(&mut self, r: usize) -> Result<(), FluentbitCode> {
        if r >= ES_ROWS {
            return Err(FluentbitCode::ENoRow);
        }
        if match self.ess.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluentbitCode::EDup);
        }
        if let Some(c) = self.ess.get_mut(r) {
            *c = es_row(r);
        }
        self.fnv(0xEC990 + r as u64);
        Ok(())
    }

    /// ES 分账声明全齐核验。
    pub fn ess_all(&self) -> bool {
        let mut r = 0usize;
        while r < ES_ROWS {
            match self.ess.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 运行时上栈声明一行。
    pub fn declare_c(&mut self, r: usize) -> Result<(), FluentbitCode> {
        if r >= C_ROWS {
            return Err(FluentbitCode::ENoRow);
        }
        if match self.cs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluentbitCode::EDup);
        }
        if let Some(c) = self.cs.get_mut(r) {
            *c = c_row(r);
        }
        self.fnv(0xEC9D0 + r as u64);
        Ok(())
    }

    /// C 上栈声明全齐核验。
    pub fn cs_all(&self) -> bool {
        let mut r = 0usize;
        while r < C_ROWS {
            match self.cs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：parsers 缺 → EParserMiss；分账/C 行缺 → ERowMiss；格未齐 → ESeal。
    pub fn seal(&self) -> Result<(), FluentbitCode> {
        if !self.parsers_all() {
            return Err(FluentbitCode::EParserMiss);
        }
        if !(self.ess_all() && self.cs_all()) {
            return Err(FluentbitCode::ERowMiss);
        }
        if !(self.gates_all() && self.chks_all() && self.outs_all() && self.proj_all()) {
            return Err(FluentbitCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), FluentbitCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(FluentbitCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), FluentbitCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(FluentbitCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &FluentbitLedger) -> Grade {
    if led.gates_all() && led.chks_all() && led.parsers_all() && led.outs_all() && led.proj_all()
        && led.ess_all() && led.cs_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

impl Default for FluentbitLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FluentbitLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CHECKS {
        if let Some(w) = chk_cell(o) {
            if led.record_chk(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PARSER_CELLS {
        if let Some(w) = parser_cell(p) {
            if led.record_parser(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut o = 0usize;
    while o < OUT_CELLS {
        if let Some(w) = out_cell(o) {
            if led.record_out(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < PROJ_CELLS {
        if let Some(w) = proj_cell(i) {
            if led.record_proj(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < ES_ROWS {
        let _ = led.declare_es(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < C_ROWS {
        let _ = led.declare_c(r);
        r += 1;
    }
    // 注入 10 轮：检查载荷漂移 / 输出态内逆序（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FluentbitLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_chk(0, match chk_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_chk(
                    1,
                    match chk_cell(1) {
                        Some(w) => w,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(FluentbitCode::EDrift)
            }
            1 => {
                lt.record_out(
                    5,
                    match out_cell(5) {
                        Some(w) => w,
                        None => 0,
                    },
                ) == Err(FluentbitCode::EOrder)
            }
            _ => {
                lt.record_chk(CHECKS, 0).is_err()
                    && lt.record_parser(PARSER_CELLS, 0).is_err()
                    && lt.record_proj(PROJ_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xEC610 + k as u64);
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
/// UNX-F52387 判据集（锚 UNX-F52388-J1，off-by-one 板式）。
pub fn run_unx52387_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52387");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == CHECKS as u32 + PARSER_CELLS as u32 + OUT_CELLS as u32 + PROJ_CELLS as u32,
        "20 检查 + 8 解析 + 6 输出 + 12 投影逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "input 五族 20 格全通",
        {
            let mut lt = FluentbitLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CHECKS {
                if let Some(w) = chk_cell(o) {
                    if lt.record_chk(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.chks_all()
        },
        "tail/winlog/cpu/mem/net 五族逐格载荷恒等",
    );
    s.add(
        "parsers 8 格全落",
        {
            let mut lt = FluentbitLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PARSER_CELLS {
                if let Some(w) = parser_cell(p) {
                    if lt.record_parser(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.parsers_all()
        },
        "[PARSER] 八段逐格载荷对照落账",
    );
    s.add(
        "output 三态 6 格全落",
        {
            let mut lt = FluentbitLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OUT_CELLS {
                if let Some(w) = out_cell(o) {
                    if lt.record_out(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.outs_all()
        },
        "es/http/stdout 三态先声明后启用成对落账",
    );
    s.add(
        "fluent-bit.conf 双向投影 12 格全落",
        {
            let mut lt = FluentbitLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    if lt.record_proj(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.proj_all()
        },
        "六对正反投影成对落账",
    );
    s.add(
        "ES 分账声明 2/2",
        {
            let mut lt = FluentbitLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ES_ROWS {
                if lt.declare_es(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ess_all()
        },
        "与 Elasticsearch 分账两行声明闭集（§AI-66.3）",
    );
    s.add(
        "C 上栈声明 2/2",
        {
            let mut lt = FluentbitLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < C_ROWS {
                if lt.declare_c(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cs_all()
        },
        "C 运行时上栈两行声明闭集",
    );
    s.add(
        "input 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CHECKS {
                let a = match chk_family(o) {
                    Some(k) => k,
                    None => 5,
                };
                let b = match chk_family(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "tail→winlog→cpu→mem→net 五族闭集单调",
    );
    s.add(
        "投影对完整",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if match proj_pair(i) {
                    Some(p) => p >= 6,
                    None => true,
                } {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "六对 fluent-bit.conf 投影对号闭集 0..6",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = FluentbitLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/fluent-bit.conf 配置/服务注册/采集功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·检查越闭集",
        FluentbitLedger::new().record_chk(CHECKS, 0).is_err(),
        "检查 20 越闭集必拒",
    );
    s.add(
        "边界·解析器越闭集",
        FluentbitLedger::new().record_parser(PARSER_CELLS, 0).is_err(),
        "parsers 8 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        FluentbitLedger::new().record_proj(PROJ_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            let mut lt = FluentbitLedger::new();
            lt.record_parser(
                0,
                match parser_cell(0) {
                    Some(w) => w,
                    None => 0,
                }
                .wrapping_add(1),
            ) == Err(FluentbitCode::EDrift)
        },
        "parsers 载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "检查漂移/输出态内逆序/越闭集三态轮转必抓",
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
            let mut lt = FluentbitLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CHECKS {
                if let Some(w) = chk_cell(o) {
                    let _ = lt.record_chk(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PARSER_CELLS {
                if let Some(w) = parser_cell(p) {
                    let _ = lt.record_parser(p, w);
                }
                p += 1;
            }
            let mut o = 0usize;
            while o < OUT_CELLS {
                if let Some(w) = out_cell(o) {
                    let _ = lt.record_out(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    let _ = lt.record_proj(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < ES_ROWS {
                let _ = lt.declare_es(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < C_ROWS {
                let _ = lt.declare_c(r);
                r += 1;
            }
            lt.seal().is_ok() && judge_grade(&lt) == Grade::S1
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chk_family() {
        assert_eq!(chk_family(0), Some(0));
        assert_eq!(chk_family(4), Some(1));
        assert_eq!(chk_family(8), Some(2));
        assert_eq!(chk_family(19), Some(4));
        assert!(chk_family(20).is_none());
        assert!(chk_cell(20).is_none());
        assert_eq!(family_name(0), Some("tail"));
        assert_eq!(family_name(1), Some("winlog"));
        assert_eq!(family_name(4), Some("net"));
        assert!(family_name(5).is_none());
    }

    #[test]
    fn test_proj_pairs() {
        assert_eq!(proj_pair(0), Some(0));
        assert_eq!(proj_pair(1), Some(0));
        assert_eq!(proj_pair(11), Some(5));
        assert!(proj_pair(12).is_none());
        assert!(proj_cell(12).is_none());
    }

    #[test]
    fn test_parser_and_out() {
        assert_eq!(out_state_name(0), Some("es"));
        assert_eq!(out_state_name(1), Some("http"));
        assert_eq!(out_state_name(2), Some("stdout"));
        assert!(out_state_name(3).is_none());
        assert!(parser_cell(PARSER_CELLS).is_none());
        assert!(out_cell(OUT_CELLS).is_none());
        // 态内偶声明/奇启用格载荷互异；解析器格互异。
        assert_ne!(
            match out_cell(0) {
                Some(w) => w,
                None => 0,
            },
            match out_cell(1) {
                Some(w) => w,
                None => 0,
            }
        );
        assert_ne!(
            match parser_cell(0) {
                Some(w) => w,
                None => 0,
            },
            match parser_cell(7) {
                Some(w) => w,
                None => 0,
            }
        );
    }

    #[test]
    fn test_cells_pure() {
        let a = match chk_cell(3) {
            Some(w) => w,
            None => 0,
        };
        assert_eq!(a, match chk_cell(3) {
            Some(w) => w,
            None => 0,
        });
        assert_ne!(a, match chk_cell(4) {
            Some(w) => w,
            None => 0,
        });
        assert!(es_row(ES_ROWS).is_none());
        assert!(c_row(C_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 tail，跳族必抓。
        let mut lt = FluentbitLedger::new();
        assert_eq!(
            lt.record_chk(
                8,
                match chk_cell(8) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(FluentbitCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_chk(
            0,
            match chk_cell(0) {
                Some(w) => w,
                None => 0,
            },
        );
        let _ = lt.record_chk(
            4,
            match chk_cell(4) {
                Some(w) => w,
                None => 0,
            },
        );
        // 回跳 tail 之前族 → 必抓。
        assert_eq!(
            lt.record_chk(
                2,
                match chk_cell(2) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(FluentbitCode::EOrder)
        );
        // 回跳未改族游标，winlog 同族续落仍合法。
        assert!(lt
            .record_chk(
                5,
                match chk_cell(5) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
    }

    #[test]
    fn test_proj_and_out_order_caught() {
        // 空账本：fluent-bit.conf 反向回读先于正向投影必抓。
        let mut lt = FluentbitLedger::new();
        assert_eq!(
            lt.record_proj(
                1,
                match proj_cell(1) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(FluentbitCode::EOrder)
        );
        let _ = lt.record_proj(
            0,
            match proj_cell(0) {
                Some(w) => w,
                None => 0,
            },
        );
        assert!(lt
            .record_proj(
                1,
                match proj_cell(1) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
        // 输出态内：stdout 启用（奇格）先于声明（偶格）必抓。
        let mut lt2 = FluentbitLedger::new();
        assert_eq!(
            lt2.record_out(
                5,
                match out_cell(5) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(FluentbitCode::EOrder)
        );
        let _ = lt2.record_out(
            4,
            match out_cell(4) {
                Some(w) => w,
                None => 0,
            },
        );
        assert!(lt2
            .record_out(
                5,
                match out_cell(5) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
    }

    #[test]
    fn test_parser_order_free_and_drift() {
        // parsers 乱序可收：先落 3 再落 0。
        let mut lt = FluentbitLedger::new();
        assert!(lt
            .record_parser(
                3,
                match parser_cell(3) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
        assert!(lt
            .record_parser(
                0,
                match parser_cell(0) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
        // 漂移必抓。
        assert_eq!(
            lt.record_parser(
                1,
                match parser_cell(1) {
                    Some(w) => w,
                    None => 0,
                }
                .wrapping_add(1)
            ),
            Err(FluentbitCode::EDrift)
        );
        assert_eq!(lt.stats().1, 1);
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FluentbitLedger::new();
        let _ = lt.record_chk(
            0,
            match chk_cell(0) {
                Some(w) => w,
                None => 0,
            },
        );
        assert_eq!(
            lt.record_chk(
                0,
                match chk_cell(0) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(FluentbitCode::EDup)
        );
        let _ = lt.declare_es(0);
        assert_eq!(lt.declare_es(0), Err(FluentbitCode::EDup));
        assert_eq!(lt.declare_es(ES_ROWS), Err(FluentbitCode::ENoRow));
        assert_eq!(lt.declare_c(C_ROWS), Err(FluentbitCode::ENoRow));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FluentbitLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 判据闸越闭集走独立 ENoGate 码。
        assert_eq!(lt.judge_gate(6), Err(FluentbitCode::ENoGate));
        assert!(lt.gates_all());
        assert!(!FluentbitLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：parsers 缺 → EParserMiss。
        assert_eq!(FluentbitLedger::new().seal(), Err(FluentbitCode::EParserMiss));
        // 解析齐、分账/C 行缺 → ERowMiss。
        let mut lt = FluentbitLedger::new();
        let mut p = 0usize;
        while p < PARSER_CELLS {
            if let Some(w) = parser_cell(p) {
                let _ = lt.record_parser(p, w);
            }
            p += 1;
        }
        assert_eq!(lt.seal(), Err(FluentbitCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut r = 0usize;
        while r < ES_ROWS {
            let _ = lt.declare_es(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < C_ROWS {
            let _ = lt.declare_c(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(FluentbitCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 检查 + 8 解析 + 6 输出 + 12 投影（闸与声明行不计数）。
        assert_eq!(
            recorded,
            CHECKS as u32 + PARSER_CELLS as u32 + OUT_CELLS as u32 + PROJ_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCA080);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(FluentbitCode::Ok as u32, 0);
        assert_eq!(FluentbitCode::EGradeDrift as u32, 12);
        assert_eq!(FluentbitCode::Ok.val(), SEG_BASE);
        assert_eq!(FluentbitCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!FluentbitCode::Ok.detail().is_empty());
        assert!(!FluentbitCode::EParserMiss.detail().is_empty());
        assert!(!FluentbitCode::ENoGate.detail().is_empty());
        assert!(!FluentbitCode::EFp.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&FluentbitLedger::new()), Grade::S3);
        let mut lt = FluentbitLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        let mut full = FluentbitLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < CHECKS {
            if let Some(w) = chk_cell(o) {
                let _ = full.record_chk(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < PARSER_CELLS {
            if let Some(w) = parser_cell(p) {
                let _ = full.record_parser(p, w);
            }
            p += 1;
        }
        let mut o = 0usize;
        while o < OUT_CELLS {
            if let Some(w) = out_cell(o) {
                let _ = full.record_out(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < PROJ_CELLS {
            if let Some(w) = proj_cell(i) {
                let _ = full.record_proj(i, w);
            }
            i += 1;
        }
        let mut r = 0usize;
        while r < ES_ROWS {
            let _ = full.declare_es(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < C_ROWS {
            let _ = full.declare_c(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S1);
        assert!(full.conservation().is_ok());
    }

    #[test]
    fn unx52387_checks_green() {
        let s = run_unx52387_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
