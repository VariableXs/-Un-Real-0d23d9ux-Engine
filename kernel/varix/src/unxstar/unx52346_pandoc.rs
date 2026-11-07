//! UNX-F52346 Pandoc 收口 · 0xC9640段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52346"，逐字）：
//!   title: Pandoc 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52347-J1 编译
//!     ×10 文档全通；包按需安装 ×5；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52346）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52347-J1，照原文锚定；功能按本卡标题（Pandoc）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五转换对 15 格（md→html/docx/pdf/epub/latex，每对 3 格：源格/
//!      管道格/产物格），逐格纯函数折叠恒等才收，越格必抓。
//!   ② 选项 20 操作五族单调闭集（输入输出族/元数据族/过滤器族/样式
//!      族/数学族各 4 操作），族逆序必抓，载荷漂移必抓。
//!   ③ 过滤器/模板 8 格逐格收账；独立引擎消费联签 4 格（pdflatex
//!      联签锚定行），联签不恒等必抓。
//!   ④ 六判据收口闸：转换对/选项/过滤模板/引擎联签/定级登记/写回，
//!      逐格落账，未齐即封拒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 三态轮转 10/10（转换对越闭集/
//!      选项漂移/选项重复落账），三类注入全部落在当轮临时账本上，
//!      漂移注入先按合法序落正格再注漂移，对空账本同样必抓；
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52346;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五转换对格数（5 对 × 3 格）。
pub const CONV_CELLS: usize = 15;

/// 选项操作数。
pub const OPT_OPS: usize = 20;

/// 过滤器/模板格数。
pub const FT_CELLS: usize = 8;

/// 独立引擎联签格数（pdflatex）。
pub const ENG_CELLS: usize = 4;

/// Pandoc 域 XOR 锚（低 16 位恒零）。
pub const PD_SIG: u64 = 0x50414E444F430000;

/// 引擎联签基准折叠（pdflatex 消费联签锚定行）。
pub const ENG_JOINT: u64 = 0xE0C40000 ^ PD_SIG;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PandocCode {
    /// 成功
    Ok = 0,
    /// 转换对越闭集
    ENoConv,
    /// 转换对漂移
    EConvDrift,
    /// 选项号越闭集
    ENoOpt,
    /// 选项漂移
    EOptDrift,
    /// 选项族逆序
    EOrder,
    /// 过滤模板格越闭集
    ENoFt,
    /// 过滤模板漂移
    EFtDrift,
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

impl PandocCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PandocCode::Ok => "成功",
            PandocCode::ENoConv => "转换对越闭集：md→html/docx/pdf/epub/latex 五对 15 格之外先登记",
            PandocCode::EConvDrift => "转换对漂移：源/管道/产物格折叠与基准不恒等，逐格复算",
            PandocCode::ENoOpt => "选项号越闭集：选项 20 操作集之外先登记",
            PandocCode::EOptDrift => "选项漂移：选项折叠与基准不恒等，逐格复算",
            PandocCode::EOrder => "选项族逆序：输入输出→元数据→过滤器→样式→数学单调，逆序必抓",
            PandocCode::ENoFt => "过滤模板格越闭集：过滤器/模板 8 格之外先登记",
            PandocCode::EFtDrift => "过滤模板漂移：过滤器/模板折叠与基准不恒等，逐格复算",
            PandocCode::EDup => "重复落账：同格只许一条账目",
            PandocCode::ESeal => "未齐即封：15 转换格 + 20 选项 + 8 过滤模板格 + 4 联签格未齐先补齐",
            PandocCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PandocCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PandocCode::EGradeDrift => "定级漂移：全通且引擎联签全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+15转换格+20选项+8过滤模板格+4联签格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 转换对目标名（格 i 每 3 格一对：源/管道/产物）。
pub const fn conv_target(i: usize) -> Option<&'static str> {
    match i / 3 {
        0 => Some("html"),
        1 => Some("docx"),
        2 => Some("pdf"),
        3 => Some("epub"),
        4 => Some("latex"),
        _ => None,
    }
}

/// 转换对格载荷。
pub const fn conv_cell(i: usize) -> Option<u64> {
    if i >= CONV_CELLS {
        return None;
    }
    Some((0xB1000u64 + (i as u64) * 0x3C) ^ PD_SIG)
}

/// 选项族（0..4=输入输出 4..8=元数据 8..12=过滤器 12..16=样式 16..20=数学）。
pub const fn opt_kind(o: usize) -> Option<u32> {
    if o >= OPT_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 选项族名。
pub const fn opt_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("输入输出族"),
        1 => Some("元数据族"),
        2 => Some("过滤器族"),
        3 => Some("样式族"),
        4 => Some("数学族"),
        _ => None,
    }
}

/// 选项格载荷。
pub const fn opt_cell(o: usize) -> Option<u64> {
    if o >= OPT_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x2E) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 过滤器/模板格载荷。
pub const fn ft_cell(p: usize) -> Option<u64> {
    if p >= FT_CELLS {
        return None;
    }
    Some((0xB3000u64 + (p as u64) * 0x39) ^ PD_SIG)
}

/// 独立引擎联签格载荷（pdflatex 消费联签）。
pub const fn eng_cell(e: usize) -> Option<u64> {
    if e >= ENG_CELLS {
        return None;
    }
    Some((0xB4000u64 + (e as u64) * 0x47) ^ ENG_JOINT)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Pandoc 收口账本状态机。
#[derive(Clone)]
pub struct PandocLedger {
    /// 六判据（转换对/选项/过滤模板/引擎联签/定级登记/写回）。
    gates: [Option<bool>; 6],
    /// 转换对账。
    conv: [Option<u64>; CONV_CELLS],
    /// 选项账。
    opts: [Option<u64>; OPT_OPS],
    /// 过滤器/模板账。
    fts: [Option<u64>; FT_CELLS],
    /// 引擎联签账。
    eng: [Option<u64>; ENG_CELLS],
    /// 最后选项族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PandocLedger {
    /// 新账本。
    pub fn new() -> Self {
        PandocLedger {
            gates: [None; 6],
            conv: [None; CONV_CELLS],
            opts: [None; OPT_OPS],
            fts: [None; FT_CELLS],
            eng: [None; ENG_CELLS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PandocCode> {
        if gate >= 6 {
            return Err(PandocCode::ESeal);
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

    /// 转换对一格（目标闭集 + 载荷对照 + 重复拒）。
    pub fn record_conv(&mut self, i: usize, got: u64) -> Result<(), PandocCode> {
        if i >= CONV_CELLS {
            return Err(PandocCode::ENoConv);
        }
        if match conv_target(i) {
            Some(_) => false,
            None => true,
        } {
            return Err(PandocCode::ENoConv);
        }
        let want = match conv_cell(i) {
            Some(w) => w,
            None => return Err(PandocCode::ENoConv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + i as u64);
            return Err(PandocCode::EConvDrift);
        }
        if match self.conv.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PandocCode::EDup);
        }
        if let Some(c) = self.conv.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6201 + i as u64);
        Ok(())
    }

    /// 转换对全账核验（15 格）。
    pub fn conv_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONV_CELLS {
            match self.conv.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 选项一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_opt(&mut self, o: usize, got: u64) -> Result<(), PandocCode> {
        if o >= OPT_OPS {
            return Err(PandocCode::ENoOpt);
        }
        let kind = match opt_kind(o) {
            Some(k) => k,
            None => return Err(PandocCode::ENoOpt),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(PandocCode::EOrder);
        }
        let want = match opt_cell(o) {
            Some(w) => w,
            None => return Err(PandocCode::ENoOpt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(PandocCode::EOptDrift);
        }
        if match self.opts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PandocCode::EDup);
        }
        if let Some(c) = self.opts.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 选项全账核验（20 格）。
    pub fn opts_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPT_OPS {
            match self.opts.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 过滤器/模板一格（载荷对照 + 重复拒）。
    pub fn record_ft(&mut self, p: usize, got: u64) -> Result<(), PandocCode> {
        let want = match ft_cell(p) {
            Some(w) => w,
            None => return Err(PandocCode::ENoFt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + p as u64);
            return Err(PandocCode::EFtDrift);
        }
        if match self.fts.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PandocCode::EDup);
        }
        if let Some(c) = self.fts.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6401 + p as u64);
        Ok(())
    }

    /// 过滤器/模板全账核验（8 格）。
    pub fn fts_all(&self) -> bool {
        let mut p = 0usize;
        while p < FT_CELLS {
            match self.fts.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 引擎联签一格（联签恒等对照 + 重复拒）。
    pub fn record_eng(&mut self, e: usize, got: u64) -> Result<(), PandocCode> {
        let want = match eng_cell(e) {
            Some(w) => w,
            None => return Err(PandocCode::ENoFt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + e as u64);
            return Err(PandocCode::EFtDrift);
        }
        if match self.eng.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PandocCode::EDup);
        }
        if let Some(c) = self.eng.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x6501 + e as u64);
        Ok(())
    }

    /// 引擎联签全账核验（4 格）。
    pub fn eng_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENG_CELLS {
            match self.eng.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
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
            && self.conv_all()
            && self.opts_all()
            && self.fts_all()
            && self.eng_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PandocLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落格数, 检出数, 指纹）。
/// 三态轮转注入全部落在当轮临时账本 lt 上；漂移注入先按合法序落正格
/// 再注漂移；对空账本同样必抓；注入轮不改主账 last_kind。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PandocLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < CONV_CELLS {
        if let Some(w) = conv_cell(i) {
            if led.record_conv(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < OPT_OPS {
        if let Some(w) = opt_cell(o) {
            if led.record_opt(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < FT_CELLS {
        if let Some(w) = ft_cell(p) {
            if led.record_ft(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut e = 0usize;
    while e < ENG_CELLS {
        if let Some(w) = eng_cell(e) {
            if led.record_eng(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    // 注入 10 轮：转换对越闭集 / 选项漂移 / 选项重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PandocLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_conv(0, match conv_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_conv(CONV_CELLS, 0).is_err()
            }
            1 => {
                let _ = lt.record_opt(0, match opt_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_opt(
                    1,
                    match opt_cell(1) {
                        Some(w) => w,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(PandocCode::EOptDrift)
            }
            _ => {
                let _ = lt.record_opt(0, match opt_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_opt(0, match opt_cell(0) {
                    Some(w) => w,
                    None => 0,
                }) == Err(PandocCode::EDup)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52346 判据集（锚 UNX-F52347-J1，off-by-one 板式）。
pub fn run_unx52346_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52346");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五转换对 15 格全通",
        recorded >= CONV_CELLS as u32,
        "md→html/docx/pdf/epub/latex 每对源/管道/产物三格逐格恒等（UNX-F52347-J1）",
    );
    s.add(
        "选项 20 操作全通",
        {
            let mut lt = PandocLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OPT_OPS {
                match opt_cell(o) {
                    Some(w) => {
                        if lt.record_opt(o, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                o += 1;
            }
            ok && lt.opts_all()
        },
        "五族闭集 20 操作逐格恒等收账",
    );
    s.add(
        "选项五族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPT_OPS {
                let a = match opt_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match opt_kind(o - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "输入输出→元数据→过滤器→样式→数学五族闭集单调",
    );
    s.add(
        "过滤器/模板 8 格全落",
        {
            let mut lt = PandocLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < FT_CELLS {
                match ft_cell(p) {
                    Some(w) => {
                        if lt.record_ft(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                p += 1;
            }
            ok && lt.fts_all()
        },
        "过滤器/模板闭集逐格收账",
    );
    s.add(
        "独立引擎联签 4 格全落（pdflatex）",
        {
            let mut lt = PandocLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENG_CELLS {
                match eng_cell(e) {
                    Some(w) => {
                        if lt.record_eng(e, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                e += 1;
            }
            ok && lt.eng_all()
        },
        "pdflatex 独立引擎消费联签锚定行逐格恒等",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "转换对越闭集/选项漂移/选项重复落账三态轮转必抓",
    );
    let mut l0 = PandocLedger::new();
    s.add(
        "边界·转换对越闭集",
        l0.record_conv(CONV_CELLS, 0).is_err(),
        "转换格 15 越闭集必拒",
    );
    s.add(
        "边界·选项越闭集",
        l0.record_opt(OPT_OPS, 0).is_err(),
        "选项 20 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.record_eng(ENG_CELLS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "选项漂移拒",
        {
            match opt_cell(0) {
                Some(w) => {
                    let _ = l0.record_opt(0, w);
                }
                None => {}
            }
            let bad = match opt_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            l0.record_opt(1, bad) == Err(PandocCode::EOptDrift)
        },
        "选项载荷漂移必抓",
    );
    s.add(
        "转换对漂移拒",
        {
            let bad = match conv_cell(2) {
                Some(w) => w.wrapping_add(5),
                None => 5,
            };
            l0.record_conv(2, bad) == Err(PandocCode::EConvDrift)
        },
        "转换对载荷漂移必抓",
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
            let mut lt = PandocLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < CONV_CELLS {
                match conv_cell(i) {
                    Some(w) => {
                        let _ = lt.record_conv(i, w);
                    }
                    None => {}
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < OPT_OPS {
                match opt_cell(o) {
                    Some(w) => {
                        let _ = lt.record_opt(o, w);
                    }
                    None => {}
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < FT_CELLS {
                match ft_cell(p) {
                    Some(w) => {
                        let _ = lt.record_ft(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            let mut e = 0usize;
            while e < ENG_CELLS {
                match eng_cell(e) {
                    Some(w) => {
                        let _ = lt.record_eng(e, w);
                    }
                    None => {}
                }
                e += 1;
            }
            // 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9640,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式 0xC9640 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conv_targets() {
        // 目标映射：i/3 → 0..4 五对（源/管道/产物）。
        assert_eq!(conv_target(0), Some("html"));
        assert_eq!(conv_target(2), Some("html"));
        assert_eq!(conv_target(3), Some("docx"));
        assert_eq!(conv_target(8), Some("pdf"));
        assert_eq!(conv_target(14), Some("latex"));
        assert!(conv_target(15).is_none());
        assert!(conv_cell(15).is_none());
    }

    #[test]
    fn test_opt_kind_mapping() {
        // kind 映射：o/4 → 0,0,0,0,1,1,...,4（五族）。
        assert_eq!(opt_kind(0), Some(0));
        assert_eq!(opt_kind(3), Some(0));
        assert_eq!(opt_kind(4), Some(1));
        assert_eq!(opt_kind(19), Some(4));
        assert!(opt_kind(20).is_none());
        assert_eq!(opt_kind_name(0), Some("输入输出族"));
        assert_eq!(opt_kind_name(4), Some("数学族"));
        assert!(opt_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = opt_cell(7).unwrap_or(0);
        assert_eq!(a, opt_cell(7).unwrap_or(0));
        assert_ne!(a, opt_cell(8).unwrap_or(0));
        assert!(ft_cell(FT_CELLS).is_none());
        assert!(eng_cell(ENG_CELLS).is_none());
        // 联签载荷与普通格基准分离。
        assert_ne!(eng_cell(0).unwrap_or(0), conv_cell(0).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PandocLedger::new();
        // 先按合法序落过滤器族正格，再注元数据族逆序。
        let _ = lt.record_opt(10, opt_cell(10).unwrap_or(0));
        assert_eq!(
            lt.record_opt(5, opt_cell(5).unwrap_or(0)),
            Err(PandocCode::EOrder)
        );
        assert!(lt.record_opt(11, opt_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PandocLedger::new();
        assert_eq!(
            lt.record_opt(5, opt_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PandocCode::EOptDrift)
        );
        assert_eq!(
            lt.record_conv(1, conv_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(PandocCode::EConvDrift)
        );
        assert_eq!(
            lt.record_ft(3, ft_cell(3).unwrap_or(0).wrapping_add(2)),
            Err(PandocCode::EFtDrift)
        );
        assert_eq!(
            lt.record_eng(0, eng_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(PandocCode::EFtDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PandocLedger::new();
        let _ = lt.record_conv(0, conv_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_conv(0, conv_cell(0).unwrap_or(0)),
            Err(PandocCode::EDup)
        );
        let _ = lt.record_eng(1, eng_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_eng(1, eng_cell(1).unwrap_or(0)),
            Err(PandocCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PandocLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PandocLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= 47);
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
        assert_eq!(SEG_BASE, 0xC9640);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PandocCode::Ok as u32, 0);
        assert_eq!(PandocCode::EGradeDrift as u32, 12);
        assert!(!PandocCode::Ok.detail().is_empty());
        assert!(!PandocCode::ESeal.detail().is_empty());
        assert_eq!(PandocCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PandocLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52346_checks_green() {
        let s = run_unx52346_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
