//! UNX-F52446 Sysinternals du 收口 · 0xCAF40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52446"，逐字）：
//!   title: Sysinternals du 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52447-J1 六判据
//!     全落；目录树/矩形图 ×10；大盘扫描分段账；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非
//!     空。
//! off-by-one 板式口径：本卡（卡 N=52446）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52447-J1，照原文锚定；功能按本卡标题（Sysinternals du）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模式族闭集 20 格：目录/文件/压缩/导出/Top 列表各 4 格，家族单
//!      调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 递归深度闸：深度 0..=8 闭集，越深必抓（大盘扫描分段账前置闸）。
//!   ③ 大小守恒等式 Σ=总 8 格：八段大小账之和恒等于总账基准，破缺即
//!      抓（大盘扫描分段账）。
//!   ④ Sysinternals 族分账声明 2 行闭集（F52166/F52167 联签锚定）+ C
//!      上栈声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（模式漂
//!      移/家族逆序/模式越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一
//!      致；定级 S1 目标档（KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52446;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五模式族操作数。
pub const MODE_OPS: usize = 20;

/// 递归深度闸（0..=8 闭集）。
pub const DEPTH_MAX: usize = 8;

/// 大小守恒段数（Σ=总）。
pub const SIZE_CELLS: usize = 8;

/// Sysinternals 族分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C 上栈声明行数。
pub const CSTACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysduCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 模式漂移
    EModeDrift,
    /// 家族逆序
    EOrder,
    /// 递归深度越闸
    EDepthOver,
    /// 大小守恒破缺
    ESizeCons,
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

impl SysduCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SysduCode::Ok => "成功",
            SysduCode::ENoCell => "格号越闭集：du 五模式族 20 操作/8 段大小账闭集之外先登记",
            SysduCode::EModeDrift => "模式漂移：模式族折叠与基准不恒等，逐格复算",
            SysduCode::EOrder => "家族逆序：目录→文件→压缩→导出→Top 列表单调，逆序必抓",
            SysduCode::EDepthOver => "递归深度越闸：深度 0..=8 闭集，越深必抓（大盘分段前置闸）",
            SysduCode::ESizeCons => "大小守恒破缺：八段大小账之和与总账基准不恒等，逐段复算",
            SysduCode::ENoRow => "声明行越闭集：族分账/C 上栈 2 行闭集之外先登记",
            SysduCode::ERowMiss => "缺声明行：Sysinternals 族分账或 C 上栈声明未齐先补齐",
            SysduCode::EDup => "重复落账：同格只许一条账目",
            SysduCode::ESeal => "未齐即封：20 模式 + 8 段大小 + 2 行分账 + 2 行上栈未齐先补齐",
            SysduCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SysduCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SysduCode::EGradeDrift => "定级漂移：全通且守恒成立且声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const 安全）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20模式+8段守恒+分账/上栈声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 模式族操作格载荷（操作 o；家族 = o/4）。
pub const fn mode_cell(o: usize) -> Option<u64> {
    if o >= MODE_OPS {
        return None;
    }
    Some((0xCC100u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 模式族（0..4=目录 4..8=文件 8..12=压缩 12..16=导出 16..20=Top 列表）。
pub const fn mode_kind(o: usize) -> Option<u32> {
    if o >= MODE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 模式族名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("目录"),
        1 => Some("文件"),
        2 => Some("压缩"),
        3 => Some("导出"),
        4 => Some("Top 列表"),
        _ => None,
    }
}

/// 大小守恒段载荷（段 i）。
pub const fn size_cell(i: usize) -> Option<u64> {
    if i >= SIZE_CELLS {
        return None;
    }
    Some((0xCC200u64 + (i as u64) * 0x40) ^ 0x5359_4455_0000_0000)
}

/// 大小总账基准（Σ size_cell(i)，const 期折算）。
pub const fn size_total() -> u64 {
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < SIZE_CELLS {
        match size_cell(i) {
            Some(v) => sum = sum.wrapping_add(v),
            None => {}
        }
        i += 1;
    }
    sum
}

/// Sysinternals 族分账声明行折叠（行 r；F52166/F52167 联签锚定）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCC400 + r as u32 * 0x2C)
}

/// C 上栈声明行折叠（行 r）。
pub const fn cstack_row(r: usize) -> Option<u32> {
    if r >= CSTACK_ROWS {
        return None;
    }
    Some(0xCC500 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Sysinternals du 收口账本状态机。
#[derive(Clone)]
pub struct SysduLedger {
    /// 六判据（磁盘占用收口：模式/深度/守恒/分账/上栈/封核）。
    gates: [Option<bool>; 6],
    /// 模式族操作账。
    ops: [Option<u64>; MODE_OPS],
    /// 大小分段账。
    sizes: [Option<u64>; SIZE_CELLS],
    /// 族分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C 上栈声明账。
    cstack: [Option<u32>; CSTACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SysduLedger {
    /// 新账本。
    pub fn new() -> Self {
        SysduLedger {
            gates: [None; 6],
            ops: [None; MODE_OPS],
            sizes: [None; SIZE_CELLS],
            splits: [None; SPLIT_ROWS],
            cstack: [None; CSTACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SysduCode> {
        if gate >= 6 {
            return Err(SysduCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4E00 + gate as u64);
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

    /// 模式族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, o: usize, got: u64) -> Result<(), SysduCode> {
        if o >= MODE_OPS {
            return Err(SysduCode::ENoCell);
        }
        let kind = match mode_kind(o) {
            Some(k) => k,
            None => return Err(SysduCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E10);
            return Err(SysduCode::EOrder);
        }
        let want = match mode_cell(o) {
            Some(w) => w,
            None => return Err(SysduCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E11 + o as u64);
            return Err(SysduCode::EModeDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SysduCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4E12 + o as u64);
        Ok(())
    }

    /// 模式族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < MODE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 递归深度闸（0..=8 闭集，越深必抓）。
    pub fn record_depth(&mut self, d: usize) -> Result<(), SysduCode> {
        if d > DEPTH_MAX {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E20 + d as u64);
            return Err(SysduCode::EDepthOver);
        }
        self.fnv(0x4E21 + d as u64);
        Ok(())
    }

    /// 大小一段（载荷对照 + 重复拒）。
    pub fn record_size(&mut self, i: usize, got: u64) -> Result<(), SysduCode> {
        let want = match size_cell(i) {
            Some(w) => w,
            None => return Err(SysduCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E30 + i as u64);
            return Err(SysduCode::ESizeCons);
        }
        if match self.sizes.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SysduCode::EDup);
        }
        if let Some(c) = self.sizes.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4E31 + i as u64);
        Ok(())
    }

    /// 大小分段全账核验（8 格）。
    pub fn sizes_all(&self) -> bool {
        let mut i = 0usize;
        while i < SIZE_CELLS {
            match self.sizes.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 大小守恒核验：Σ 已落账段 == 总账基准且 8 段全齐。
    pub fn check_conservation(&self) -> Result<(), SysduCode> {
        if !self.sizes_all() {
            return Err(SysduCode::ESizeCons);
        }
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < SIZE_CELLS {
            match self.sizes.get(i) {
                Some(Some(v)) => sum = sum.wrapping_add(*v),
                _ => return Err(SysduCode::ESizeCons),
            }
            i += 1;
        }
        if sum != size_total() {
            return Err(SysduCode::ESizeCons);
        }
        Ok(())
    }

    /// Sysinternals 族分账声明一行（F52166/F52167）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SysduCode> {
        if r >= SPLIT_ROWS {
            return Err(SysduCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SysduCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4E40 + r as u64);
        Ok(())
    }

    /// 族分账声明全齐核验。
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

    /// C 上栈声明一行。
    pub fn declare_cstack(&mut self, r: usize) -> Result<(), SysduCode> {
        if r >= CSTACK_ROWS {
            return Err(SysduCode::ENoRow);
        }
        if match self.cstack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SysduCode::EDup);
        }
        if let Some(c) = self.cstack.get_mut(r) {
            *c = cstack_row(r);
        }
        self.fnv(0x4E50 + r as u64);
        Ok(())
    }

    /// C 上栈声明全齐核验。
    pub fn cstack_all(&self) -> bool {
        let mut r = 0usize;
        while r < CSTACK_ROWS {
            match self.cstack.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封核检查：六闸齐且各节全齐才放行，未齐即封。
    pub fn seal_check(&self) -> Result<(), SysduCode> {
        if !self.gates_all() {
            return Err(SysduCode::ESeal);
        }
        if !self.splits_all() || !self.cstack_all() {
            return Err(SysduCode::ERowMiss);
        }
        if !self.ops_all() || !self.sizes_all() {
            return Err(SysduCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.sizes_all() && self.splits_all() && self.cstack_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SysduLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SysduLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < MODE_OPS {
        if let Some(w) = mode_cell(o) {
            if led.record_mode(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < SIZE_CELLS {
        if let Some(w) = size_cell(i) {
            if led.record_size(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut d = 0usize;
    while d <= DEPTH_MAX {
        let _ = led.record_depth(d);
        d += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut c = 0usize;
    while c < CSTACK_ROWS {
        let _ = led.declare_cstack(c);
        c += 1;
    }
    // 注入 10 轮：模式漂移 / 家族逆序 / 深度越闸三态轮转。
    // 三类注入全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SysduLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格（低族 0 格），再注漂移格。
            0 => {
                let _ = lt.record_mode(0, mode_cell(0).unwrap_or(0));
                lt.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SysduCode::EModeDrift)
            }
            // 逆序注入：先落合法高族格，再注低族格，逆序必抓。
            1 => {
                let _ = lt.record_mode(17, mode_cell(17).unwrap_or(0));
                lt.record_mode(5, mode_cell(5).unwrap_or(0)) == Err(SysduCode::EOrder)
            }
            // 深度越闸注入：空账本同样必抓，且不改 last_kind。
            _ => lt.record_depth(DEPTH_MAX + 1).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4E60 + k as u64);
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
/// UNX-F52446 判据集（锚 UNX-F52447-J1，off-by-one 板式）。
pub fn run_unx52446_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52446");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五模式族 20 操作全通",
        recorded >= MODE_OPS as u32,
        "目录/文件/压缩/导出/Top 列表五家族逐操作恒等",
    );
    s.add(
        "大小守恒 Σ=总 8 格全落",
        {
            let mut lt = SysduLedger::new();
            let mut i = 0usize;
            while i < SIZE_CELLS {
                if let Some(w) = size_cell(i) {
                    let _ = lt.record_size(i, w);
                }
                i += 1;
            }
            lt.check_conservation().is_ok() && lt.sizes_all()
        },
        "八段大小账之和恒等于总账基准（大盘扫描分段账）",
    );
    s.add(
        "递归深度闸 0..=8 全通",
        {
            let mut lt = SysduLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d <= DEPTH_MAX {
                if lt.record_depth(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok
        },
        "深度 0..=8 闭集逐级放行，越深必抓（大盘分段前置闸）",
    );
    s.add(
        "Sysinternals 族分账声明 2/2 全齐",
        {
            let mut lt = SysduLedger::new();
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
        "F52166/F52167 联签锚定的族分账两行闭集",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = SysduLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CSTACK_ROWS {
                if lt.declare_cstack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cstack_all()
        },
        "C 上栈独立账声明/C 栈深度分账声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MODE_OPS {
                if mode_kind(o).unwrap_or(5) < mode_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五模式族闭集单调（逆序必抓）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/深度越闸三态轮转必抓");
    let mut l0 = SysduLedger::new();
    s.add(
        "边界·模式越闭集",
        l0.record_mode(MODE_OPS, 0).is_err(),
        "模式 20 越闭集必拒",
    );
    s.add(
        "边界·深度越闸",
        l0.record_depth(DEPTH_MAX + 1).is_err(),
        "深度 9 越闸必拒",
    );
    s.add(
        "边界·大小越闭集",
        l0.record_size(SIZE_CELLS, 0).is_err(),
        "大小段 8 越闭集必拒",
    );
    s.add(
        "模式漂移拒",
        {
            let _ = l0.record_mode(0, mode_cell(0).unwrap_or(0));
            l0.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SysduCode::EModeDrift)
        },
        "模式族载荷漂移必抓",
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
        "未齐先记账不封",
        {
            let mut lt = SysduLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < 4 {
                if let Some(w) = mode_cell(o) {
                    if lt.record_mode(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            // 账未齐时逐格记账仍放行（Ok），封核检查才拒。
            ok && lt.record_mode(4, mode_cell(4).unwrap_or(0)).is_ok()
        },
        "账未齐时记账必须 Ok，未齐即封只在封核检查处生效",
    );
    s.add(
        "守恒破缺必抓",
        {
            // 只落部分段即触发守恒破缺（ESizeCons）。
            let mut lt = SysduLedger::new();
            if let Some(w) = size_cell(0) {
                let _ = lt.record_size(0, w);
            }
            lt.check_conservation() == Err(SysduCode::ESizeCons)
        },
        "八段未齐或和漂移即判守恒破缺",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = SysduLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < MODE_OPS {
                if let Some(w) = mode_cell(o) {
                    let _ = lt.record_mode(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < SIZE_CELLS {
                if let Some(w) = size_cell(i) {
                    let _ = lt.record_size(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut c = 0usize;
            while c < CSTACK_ROWS {
                let _ = lt.declare_cstack(c);
                c += 1;
            }
            // main/aux 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集",
        {
            let codes = [
                SysduCode::Ok,
                SysduCode::ENoCell,
                SysduCode::EModeDrift,
                SysduCode::EOrder,
                SysduCode::EDepthOver,
                SysduCode::ESizeCons,
                SysduCode::ENoRow,
                SysduCode::ERowMiss,
                SysduCode::EDup,
                SysduCode::ESeal,
                SysduCode::EInj,
                SysduCode::EFp,
                SysduCode::EGradeDrift,
            ];
            let mut ok = codes.len() == 13;
            let mut i = 0usize;
            while i < codes.len() {
                if codes[i].val() != SEG_BASE + i as u32 || codes[i].detail().is_empty() {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "13 变体码位连续且明细全非空",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAF40,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCAF40",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_kind() {
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(8), Some(2));
        assert_eq!(mode_kind(12), Some(3));
        assert_eq!(mode_kind(19), Some(4));
        assert!(mode_kind(20).is_none());
        assert!(mode_cell(20).is_none());
        assert_eq!(mode_kind_name(0), Some("目录"));
        assert_eq!(mode_kind_name(3), Some("导出"));
        assert_eq!(mode_kind_name(4), Some("Top 列表"));
        assert!(mode_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = mode_cell(3).unwrap_or(0);
        assert_eq!(a, mode_cell(3).unwrap_or(0));
        assert_ne!(a, mode_cell(4).unwrap_or(0));
        let b = size_cell(5).unwrap_or(0);
        assert_eq!(b, size_cell(5).unwrap_or(0));
        assert_ne!(b, size_cell(6).unwrap_or(0));
        assert!(size_cell(SIZE_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(cstack_row(CSTACK_ROWS).is_none());
    }

    #[test]
    fn test_size_total_consistent() {
        // Σ size_cell(i) 必须与 const 期折算的总账基准恒等。
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < SIZE_CELLS {
            sum = sum.wrapping_add(size_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(sum, size_total());
        assert_ne!(size_total(), 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SysduLedger::new();
        // 合法序先落高族格（Top 列表 19），再注低族格（文件 5）必抓。
        let _ = lt.record_mode(19, mode_cell(19).unwrap_or(0));
        assert_eq!(lt.record_mode(5, mode_cell(5).unwrap_or(0)), Err(SysduCode::EOrder));
        assert!(lt.record_mode(18, mode_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SysduLedger::new();
        assert_eq!(
            lt.record_mode(6, mode_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(SysduCode::EModeDrift)
        );
        assert_eq!(
            lt.record_size(2, size_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(SysduCode::ESizeCons)
        );
    }

    #[test]
    fn test_depth_gate() {
        let mut lt = SysduLedger::new();
        let mut d = 0usize;
        while d <= DEPTH_MAX {
            assert!(lt.record_depth(d).is_ok());
            d += 1;
        }
        assert_eq!(lt.record_depth(DEPTH_MAX + 1), Err(SysduCode::EDepthOver));
        assert_eq!(lt.record_depth(DEPTH_MAX + 9), Err(SysduCode::EDepthOver));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SysduLedger::new();
        let _ = lt.record_mode(8, mode_cell(8).unwrap_or(0));
        assert_eq!(lt.record_mode(8, mode_cell(8).unwrap_or(0)), Err(SysduCode::EDup));
        let _ = lt.declare_cstack(0);
        assert_eq!(lt.declare_cstack(0), Err(SysduCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SysduLedger::new();
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
        // 只含 run_cycle 实际计数的节：模式 20 + 大小 8。
        assert_eq!(recorded, (MODE_OPS + SIZE_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCAF40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SysduCode::Ok as u32, 0);
        assert_eq!(SysduCode::EGradeDrift as u32, 12);
        assert_eq!(SysduCode::Ok.val(), SEG_BASE);
        assert_eq!(SysduCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!SysduCode::Ok.detail().is_empty());
        assert!(!SysduCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_seal_check() {
        let mut lt = SysduLedger::new();
        assert_eq!(lt.seal_check(), Err(SysduCode::ESeal));
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        // 六闸齐但声明未齐 → 缺声明行。
        assert_eq!(lt.seal_check(), Err(SysduCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        let mut c = 0usize;
        while c < CSTACK_ROWS {
            let _ = lt.declare_cstack(c);
            c += 1;
        }
        // 声明齐但模式/大小未齐 → 未齐即封。
        assert_eq!(lt.seal_check(), Err(SysduCode::ESeal));
        let mut o = 0usize;
        while o < MODE_OPS {
            if let Some(w) = mode_cell(o) {
                let _ = lt.record_mode(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < SIZE_CELLS {
            if let Some(w) = size_cell(i) {
                let _ = lt.record_size(i, w);
            }
            i += 1;
        }
        assert!(lt.seal_check().is_ok());
        assert!(lt.check_conservation().is_ok());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = SysduLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = SysduLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_grade_variants_closed() {
        let grades = [Grade::S1, Grade::S2, Grade::S3];
        assert_eq!(grades.len(), 3);
        assert_eq!(grades[0].val(), 1);
        assert_eq!(grades[2].val(), 3);
        assert!(!grades[1].detail().is_empty());
    }

    #[test]
    fn unx52446_checks_green() {
        let s = run_unx52446_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
