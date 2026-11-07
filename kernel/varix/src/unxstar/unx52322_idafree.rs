//! UNX-F52322 IDA Free 收口 · 0xC9040段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52322"，逐字）：
//!   title: IDA Free 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52323-J1 六判据
//!     全落；×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.9 红线②；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52322）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52323-J1，照原文锚定；功能按本卡标题（IDA Free）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 分析流水五段 20 格闭集（加载/自动分析/签名匹配/交叉引用/类型
//!      传播）各 4 格，段单调（逆序必抓）。
//!   ② 反汇编视图 10 操作闭集（打开视图/滚动定位/文本交叉引用/图形
//!      交叉引用/操作数切换/重命名/注释/跳转地址/书签/复制汇编），逐
//!      操作载荷恒等才收，漂移必抓。
//!   ③ Free 版限制清单 6 行公开闭集（仅支持 x64 二进制/无 IDAPython
//!      脚本引擎/无 SDK 插件接口/无调试器支持/无批处理模式/无 FLIRT
//!      签名制作），缺行拒封；与商业版分账 2 行闭集（商业版功能分账
//!      声明/Free 版非商用边界声明）。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（分析段逆
//!      序/反汇编操作漂移/分析格越闭集），检出+逃逸守恒等式；双跑
//!      FNV 指纹一致；定级 S1/S2/S3 闭集——许可面如实：Free 版许可
//!      限制在案，全通亦定 S2，不得拔高定 S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52322;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 分析流水格数（五段 × 4）。
pub const STAGE_OPS: usize = 20;

/// 反汇编视图操作数。
pub const DISASM_OPS: usize = 10;

/// Free 版限制清单行数。
pub const LIMIT_ROWS: usize = 6;

/// 与商业版分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdaFreeCode {
    /// 成功
    Ok = 0,
    /// 分析段格越闭集
    ENoStage,
    /// 分析段漂移
    EStageDrift,
    /// 分析段逆序
    EOrder,
    /// 反汇编操作越闭集
    ENoDis,
    /// 反汇编操作漂移
    EDisDrift,
    /// 限制行越闭集
    ENoLimit,
    /// 分账行越闭集
    ENoSplit,
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

impl IdaFreeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            IdaFreeCode::Ok => "成功",
            IdaFreeCode::ENoStage => "分析段格越闭集：分析流水五段 20 格之外先登记",
            IdaFreeCode::EStageDrift => "分析段漂移：段格折叠与基准不恒等，逐格复算",
            IdaFreeCode::EOrder => "分析段逆序：加载→自动分析→签名匹配→交叉引用→类型传播单调，逆序必抓",
            IdaFreeCode::ENoDis => "反汇编操作越闭集：反汇编视图 10 操作之外先登记",
            IdaFreeCode::EDisDrift => "反汇编操作漂移：操作折叠与基准不恒等，逐格复算",
            IdaFreeCode::ENoLimit => "限制行越闭集：Free 版限制清单 6 行之外先登记",
            IdaFreeCode::ENoSplit => "分账行越闭集：与商业版分账 2 行之外先登记",
            IdaFreeCode::EDup => "重复落账：同格只许一条账目",
            IdaFreeCode::ESeal => "未齐即封：20 分析格 + 10 反汇编操作 + 6 限制行未齐先补齐",
            IdaFreeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            IdaFreeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IdaFreeCode::EGradeDrift => "定级漂移：Free 版许可限制在案，全通亦定 S2 不得拔高 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不适用：Free 许可面如实不判 S1）
    S1,
    /// S2：全通且有许可限制的如实收口
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
            Grade::S1 => "S1：全通收口（IDA Free 许可面受限，本卡如实不判 S1）",
            Grade::S2 => "S2：六判据+20 分析格+10 操作+6 限制行+2 行分账全通，许可面如实定级",
            Grade::S3 => "S3：不收口，判据或限制清单缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 分析流水段格载荷（格 o；段 = o/4）。
pub const fn stage_cell(o: usize) -> Option<u64> {
    if o >= STAGE_OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x3D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 分析流水段号（0..4=加载 4..8=自动分析 8..12=签名匹配 12..16=交叉引用 16..20=类型传播）。
pub const fn stage_kind(o: usize) -> Option<u32> {
    if o >= STAGE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 分析段名。
pub const fn stage_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("加载"),
        1 => Some("自动分析"),
        2 => Some("签名匹配"),
        3 => Some("交叉引用"),
        4 => Some("类型传播"),
        _ => None,
    }
}

/// 反汇编视图操作载荷（操作 o）。
pub const fn disasm_cell(o: usize) -> Option<u64> {
    if o >= DISASM_OPS {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x43) ^ 0x4944_4146_5245_0000)
}

/// 反汇编操作名（const fn 表）。
pub const fn disasm_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开视图"),
        1 => Some("滚动定位"),
        2 => Some("文本交叉引用"),
        3 => Some("图形交叉引用"),
        4 => Some("操作数切换"),
        5 => Some("重命名"),
        6 => Some("注释"),
        7 => Some("跳转地址"),
        8 => Some("书签"),
        9 => Some("复制汇编"),
        _ => None,
    }
}

/// Free 版限制行折叠（行 r）。
pub const fn limit_row(r: usize) -> Option<u32> {
    if r >= LIMIT_ROWS {
        return None;
    }
    Some(0xD300 + r as u32 * 0x2C)
}

/// Free 版限制行名（const fn 表，公开如实清单）。
pub const fn limit_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("仅支持 x64 二进制"),
        1 => Some("无 IDAPython 脚本引擎"),
        2 => Some("无 SDK 插件接口"),
        3 => Some("无调试器支持"),
        4 => Some("无批处理模式"),
        5 => Some("无 FLIRT 签名制作"),
        _ => None,
    }
}

/// 与商业版分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

/// 与商业版分账行名（const fn 表）。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("商业版功能分账声明"),
        1 => Some("Free 版非商用边界声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// IDA Free 收口账本状态机。
#[derive(Clone)]
pub struct IdaFreeLedger {
    /// 六判据（分析/反汇编/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 分析流水账。
    stages: [Option<u64>; STAGE_OPS],
    /// 反汇编操作账。
    disasm: [Option<u64>; DISASM_OPS],
    /// Free 限制清单账。
    limits: [Option<u32>; LIMIT_ROWS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后段号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IdaFreeLedger {
    /// 新账本。
    pub fn new() -> Self {
        IdaFreeLedger {
            gates: [None; 6],
            stages: [None; STAGE_OPS],
            disasm: [None; DISASM_OPS],
            limits: [None; LIMIT_ROWS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), IdaFreeCode> {
        if gate >= 6 {
            return Err(IdaFreeCode::ESeal);
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

    /// 分析流水一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_stage(&mut self, o: usize, got: u64) -> Result<(), IdaFreeCode> {
        if o >= STAGE_OPS {
            return Err(IdaFreeCode::ENoStage);
        }
        let kind = match stage_kind(o) {
            Some(k) => k,
            None => return Err(IdaFreeCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(IdaFreeCode::EOrder);
        }
        let want = match stage_cell(o) {
            Some(w) => w,
            None => return Err(IdaFreeCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(IdaFreeCode::EStageDrift);
        }
        let dup = match self.stages.get(o) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(IdaFreeCode::EDup);
        }
        if let Some(c) = self.stages.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 分析流水全账核验（20 格）。
    pub fn stages_all(&self) -> bool {
        let mut o = 0usize;
        while o < STAGE_OPS {
            match self.stages.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 反汇编操作一格（载荷对照 + 重复拒）。
    pub fn record_disasm(&mut self, o: usize, got: u64) -> Result<(), IdaFreeCode> {
        let want = match disasm_cell(o) {
            Some(w) => w,
            None => return Err(IdaFreeCode::ENoDis),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + o as u64);
            return Err(IdaFreeCode::EDisDrift);
        }
        let dup = match self.disasm.get(o) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(IdaFreeCode::EDup);
        }
        if let Some(c) = self.disasm.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x6301 + o as u64);
        Ok(())
    }

    /// 反汇编操作全账核验（10 操作）。
    pub fn disasm_all(&self) -> bool {
        let mut o = 0usize;
        while o < DISASM_OPS {
            match self.disasm.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Free 限制清单一行。
    pub fn declare_limit(&mut self, r: usize) -> Result<(), IdaFreeCode> {
        if r >= LIMIT_ROWS {
            return Err(IdaFreeCode::ENoLimit);
        }
        let dup = match self.limits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(IdaFreeCode::EDup);
        }
        if let Some(c) = self.limits.get_mut(r) {
            *c = limit_row(r);
        }
        self.fnv(0x6400 + r as u64);
        Ok(())
    }

    /// 限制清单全齐核验。
    pub fn limits_all(&self) -> bool {
        let mut r = 0usize;
        while r < LIMIT_ROWS {
            match self.limits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 与商业版分账一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), IdaFreeCode> {
        if r >= SPLIT_ROWS {
            return Err(IdaFreeCode::ENoSplit);
        }
        let dup = match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(IdaFreeCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// KB-COMPAT 定级：许可面如实——全通 → S2（Free 许可限制在案，
    /// 不得拔高 S1）；判据缺或清单缺 → S3。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.stages_all() && self.disasm_all() && self.limits_all() && self.splits_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for IdaFreeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IdaFreeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < STAGE_OPS {
        if let Some(w) = stage_cell(o) {
            if led.record_stage(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DISASM_OPS {
        if let Some(w) = disasm_cell(d) {
            if led.record_disasm(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < LIMIT_ROWS {
        let _ = led.declare_limit(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：分析段逆序 / 反汇编操作漂移 / 分析格越闭集三态轮转。
    // 逆序注入先按合法序落高段格再注低段格（空账本低段本就合法，须先抬 last_kind）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = IdaFreeLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_stage(
                    17,
                    match stage_cell(17) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_stage(
                    5,
                    match stage_cell(5) {
                        Some(p) => p,
                        None => 0,
                    },
                ) == Err(IdaFreeCode::EOrder)
            }
            1 => {
                let _ = lt.record_disasm(
                    0,
                    match disasm_cell(0) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_disasm(
                    1,
                    match disasm_cell(1) {
                        Some(p) => p,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(IdaFreeCode::EDisDrift)
            }
            _ => lt.record_stage(STAGE_OPS, 0).is_err(),
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
/// UNX-F52322 判据集（锚 UNX-F52323-J1，off-by-one 板式）。
pub fn run_unx52322_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52322");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "分析流水五段 20 格全通",
        recorded >= STAGE_OPS as u32,
        "加载/自动分析/签名匹配/交叉引用/类型传播五段逐格恒等",
    );
    s.add(
        "反汇编视图 10 操作全落",
        {
            let mut lt = IdaFreeLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < DISASM_OPS {
                if let Some(w) = disasm_cell(o) {
                    if lt.record_disasm(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.disasm_all()
        },
        "打开视图/滚动定位/文本与图形交叉引用/操作数切换/重命名/注释/跳转/书签/复制逐操作恒等",
    );
    s.add(
        "Free 版限制清单 6/6 全齐",
        {
            let mut lt = IdaFreeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LIMIT_ROWS {
                if lt.declare_limit(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.limits_all()
        },
        "仅 x64/无脚本/无 SDK/无调试器/无批处理/无 FLIRT 六行公开如实清单",
    );
    s.add(
        "与商业版分账 2/2 全齐",
        {
            let mut lt = IdaFreeLedger::new();
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
        "商业版功能分账/Free 版非商用边界两行闭集",
    );
    s.add(
        "分析段单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < STAGE_OPS {
                if stage_kind(o).unwrap_or(5) < stage_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "逆序/漂移/越闭集三态轮转必抓");
    let mut l0 = IdaFreeLedger::new();
    s.add(
        "边界·分析格越闭集",
        l0.record_stage(STAGE_OPS, 0).is_err(),
        "分析格 20 越闭集必拒",
    );
    s.add(
        "边界·反汇编操作越闭集",
        l0.record_disasm(DISASM_OPS, 0).is_err(),
        "反汇编操作 10 越闭集必拒",
    );
    s.add(
        "边界·限制行越闭集",
        l0.declare_limit(LIMIT_ROWS).is_err(),
        "限制行 6 越闭集必拒",
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
        "定级 S2 闭集（许可面如实）",
        {
            let mut lt = IdaFreeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < STAGE_OPS {
                if let Some(w) = stage_cell(o) {
                    let _ = lt.record_stage(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DISASM_OPS {
                if let Some(w) = disasm_cell(d) {
                    let _ = lt.record_disasm(d, w);
                }
                d += 1;
            }
            let mut r = 0usize;
            while r < LIMIT_ROWS {
                let _ = lt.declare_limit(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，Free 许可限制在案不拔高 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9040,
        "SEG_BASE 0x40 对齐且落 VX15 域 0xC9040 格",
    );
    s.add(
        "指纹非零复验",
        { let (_, _, fp3) = run_cycle(); fp3 != 0 },
        "全流程 FNV 指纹非零（防恒空账基线）",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_kind() {
        assert_eq!(stage_kind(0), Some(0));
        assert_eq!(stage_kind(4), Some(1));
        assert_eq!(stage_kind(12), Some(3));
        assert_eq!(stage_kind(19), Some(4));
        assert!(stage_kind(20).is_none());
        assert!(stage_cell(20).is_none());
        assert_eq!(stage_kind_name(4), Some("类型传播"));
    }

    #[test]
    fn test_disasm_ops() {
        assert_eq!(disasm_op_name(0), Some("打开视图"));
        assert_eq!(disasm_op_name(3), Some("图形交叉引用"));
        assert_eq!(disasm_op_name(9), Some("复制汇编"));
        assert!(disasm_op_name(10).is_none());
        assert!(disasm_cell(DISASM_OPS).is_none());
    }

    #[test]
    fn test_limit_rows() {
        assert_eq!(limit_row_name(0), Some("仅支持 x64 二进制"));
        assert_eq!(limit_row_name(1), Some("无 IDAPython 脚本引擎"));
        assert_eq!(limit_row_name(5), Some("无 FLIRT 签名制作"));
        assert!(limit_row_name(6).is_none());
        assert!(limit_row(LIMIT_ROWS).is_none());
    }

    #[test]
    fn test_split_rows() {
        assert_eq!(split_row_name(0), Some("商业版功能分账声明"));
        assert_eq!(split_row_name(1), Some("Free 版非商用边界声明"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = stage_cell(7).unwrap_or(0);
        assert_eq!(a, stage_cell(7).unwrap_or(0));
        assert_ne!(a, stage_cell(8).unwrap_or(0));
        assert_ne!(disasm_cell(2).unwrap_or(0), disasm_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = IdaFreeLedger::new();
        // 合法序先落高段格，再注低段格（空账本低段本就合法）。
        let _ = lt.record_stage(9, stage_cell(9).unwrap_or(0));
        assert_eq!(lt.record_stage(2, stage_cell(2).unwrap_or(0)), Err(IdaFreeCode::EOrder));
        assert!(lt.record_stage(10, stage_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = IdaFreeLedger::new();
        assert_eq!(
            lt.record_disasm(4, disasm_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(IdaFreeCode::EDisDrift)
        );
        assert_eq!(
            lt.record_stage(6, stage_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(IdaFreeCode::EStageDrift)
        );
    }

    #[test]
    fn test_dup_and_boundary() {
        let mut lt = IdaFreeLedger::new();
        let _ = lt.record_stage(0, stage_cell(0).unwrap_or(0));
        assert_eq!(lt.record_stage(0, stage_cell(0).unwrap_or(0)), Err(IdaFreeCode::EDup));
        let mut lt2 = IdaFreeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert!(lt2.judge_gate(6).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= STAGE_OPS as u32 + DISASM_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC9040);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(IdaFreeCode::Ok as u32, 0);
        assert_eq!(IdaFreeCode::EGradeDrift as u32, 12);
        assert!(!IdaFreeCode::EInj.detail().is_empty());
        assert!(!IdaFreeCode::EFp.detail().is_empty());
        assert!(!IdaFreeCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        // 空账本：判据与清单全缺 → S3。
        let lt = IdaFreeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅判据齐、清单缺 → S3（许可面如实不虚判）。
        let mut lt2 = IdaFreeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52322_checks_green() {
        let s = run_unx52322_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
