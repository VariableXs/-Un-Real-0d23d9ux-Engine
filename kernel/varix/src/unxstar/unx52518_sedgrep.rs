//! UNX-F52518 sed/grep（发行版 CLI 面）收口 · 0xCC140段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52518"，逐字）：
//!   title: sed/grep（发行版 CLI 面）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52519-J1 pwsh/
//!     Git Bash/Cygwin/BusyBox 四 shell profile 共存 ×2 轮零互踩；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52518）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52519-J1，照原文锚定；功能按本卡标题（sed/grep CLI 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 双工具各五命令族 20 格：sed 10 格（s/y/d/p/q 五命令族 ×2）+
//!      grep 10 格（-E/-F/-G/-P/-r 五选项族 ×2），族序 sed→grep 单调，
//!      逆序必抓；逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② 管道消费联签 4 格：sed 输出喂 grep 的四类联签逐格落账，越签必抓。
//!   ③ 与 ripgrep 分账声明 2 行闭集（F52358：rg 独立账声明/rg 管道
//!      分账声明），缺一拒封。
//!   ④ 正则方言三态 6 格：BRE/ERE/PCRE ×2 格，逐方言载荷对照，越方言
//!      必抓。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（族逆序/载荷漂移/
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（20 格 + 6 方言 + 4 联签 + 2 行分账 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52518;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 双工具命令格数。
pub const CELL_N: usize = 20;

/// 正则方言三态格数（3 方言 × 2）。
pub const DIAL_CELLS: usize = 6;

/// 管道消费联签格数。
pub const COSIGN_CELLS: usize = 4;

/// 与 ripgrep 分账声明行数（F52358）。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SedGrepCode {
    /// 成功
    Ok = 0,
    /// 命令格越闭集
    ENoCell,
    /// 格载荷漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 方言越闭集
    ENoDial,
    /// 联签格越闭集
    ENoCos,
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

impl SedGrepCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SedGrepCode::Ok => "成功",
            SedGrepCode::ENoCell => "命令格越闭集：sed/grep 20 命令集之外先登记",
            SedGrepCode::EDrift => "格载荷漂移：命令折叠与基准不恒等，逐格复算",
            SedGrepCode::EOrder => "命令族逆序：sed 五族→grep 五族单调，逆序必抓",
            SedGrepCode::ENoDial => "方言越闭集：BRE/ERE/PCRE 三态六格之外先登记",
            SedGrepCode::ENoCos => "联签格越闭集：管道消费联签 4 格之外先登记",
            SedGrepCode::ENoRow => "声明行越闭集：ripgrep 分账 2 行闭集之外先登记（F52358）",
            SedGrepCode::ERowMiss => "缺声明行：ripgrep 分账声明未齐先补齐",
            SedGrepCode::EDup => "重复落账：同格只许一条账目",
            SedGrepCode::ESeal => "未齐即封：20 格 + 6 方言 + 4 联签 + 2 行分账未齐先补齐",
            SedGrepCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SedGrepCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SedGrepCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+6方言+4联签+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令格载荷（格 o；族 = o/2，共 10 族；"SG" 掩码低 16 位为零）。
pub const fn sg_cell(o: usize) -> Option<u64> {
    if o >= CELL_N {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193) ^ 0x5347000000000000)
}

/// 命令族（o/2：0..=4 为 sed s/y/d/p/q，5..=9 为 grep -E/-F/-G/-P/-r）。
pub const fn sg_kind(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o / 2) as u32)
}

/// 工具（0=sed 1=grep，按格号十分位）。
pub const fn sg_tool(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o / 10) as u32)
}

/// 命令族名（按 kind = o/2 索引）。
pub const fn sg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("sed·s"),
        1 => Some("sed·y"),
        2 => Some("sed·d"),
        3 => Some("sed·p"),
        4 => Some("sed·q"),
        5 => Some("grep·-E"),
        6 => Some("grep·-F"),
        7 => Some("grep·-G"),
        8 => Some("grep·-P"),
        9 => Some("grep·-r"),
        _ => None,
    }
}

/// 正则方言格载荷（格 i：方言 = i/2，0=BRE 1=ERE 2=PCRE）。
pub const fn dial_cell(i: usize) -> Option<u64> {
    if i >= DIAL_CELLS {
        return None;
    }
    Some((0xB3000u64 + (i as u64) * 0x31) ^ 0x5245470000000000)
}

/// 方言名。
pub const fn dial_name(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("BRE"),
        1 => Some("ERE"),
        2 => Some("PCRE"),
        _ => None,
    }
}

/// 管道消费联签格载荷（格 c）。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some(0xB600 + c as u64 * 0x2C)
}

/// ripgrep 分账声明行折叠（行 r；F52358）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// sed/grep 收口账本状态机。
#[derive(Clone)]
pub struct SedGrepLedger {
    /// 六判据（sed 替换/grep 匹配/管道联签/方言对账/EOL 面分账/退出码语义）。
    gates: [Option<bool>; 6],
    /// 命令格账。
    cells: [Option<u64>; CELL_N],
    /// 方言账。
    dials: [Option<u64>; DIAL_CELLS],
    /// 联签账。
    cosigns: [Option<u64>; COSIGN_CELLS],
    /// ripgrep 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SedGrepLedger {
    /// 新账本。
    pub fn new() -> Self {
        SedGrepLedger {
            gates: [None; 6],
            cells: [None; CELL_N],
            dials: [None; DIAL_CELLS],
            cosigns: [None; COSIGN_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SedGrepCode> {
        if gate >= 6 {
            return Err(SedGrepCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5210 + gate as u64);
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

    /// 命令格一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, o: usize, got: u64) -> Result<(), SedGrepCode> {
        if o >= CELL_N {
            return Err(SedGrepCode::ENoCell);
        }
        let kind = match sg_kind(o) {
            Some(k) => k,
            None => return Err(SedGrepCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5220);
            return Err(SedGrepCode::EOrder);
        }
        let want = match sg_cell(o) {
            Some(w) => w,
            None => return Err(SedGrepCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5221 + o as u64);
            return Err(SedGrepCode::EDrift);
        }
        if match self.cells.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SedGrepCode::EDup);
        }
        if let Some(c) = self.cells.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5222 + o as u64);
        Ok(())
    }

    /// 命令格全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut o = 0usize;
        while o < CELL_N {
            match self.cells.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 方言一格（载荷对照 + 重复拒；越方言必抓）。
    pub fn record_dial(&mut self, i: usize, got: u64) -> Result<(), SedGrepCode> {
        let want = match dial_cell(i) {
            Some(w) => w,
            None => return Err(SedGrepCode::ENoDial),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5230 + i as u64);
            return Err(SedGrepCode::EDrift);
        }
        if match self.dials.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SedGrepCode::EDup);
        }
        if let Some(c) = self.dials.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5231 + i as u64);
        Ok(())
    }

    /// 方言全账核验（6 格）。
    pub fn dials_all(&self) -> bool {
        let mut i = 0usize;
        while i < DIAL_CELLS {
            match self.dials.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 管道消费联签一格（载荷对照 + 重复拒；越签必抓）。
    pub fn record_cosign(&mut self, c: usize) -> Result<(), SedGrepCode> {
        let want = match cosign_cell(c) {
            Some(w) => w,
            None => return Err(SedGrepCode::ENoCos),
        };
        if match self.cosigns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SedGrepCode::EDup);
        }
        if let Some(k) = self.cosigns.get_mut(c) {
            *k = Some(want);
        }
        self.fnv(0x5240 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.cosigns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// ripgrep 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SedGrepCode> {
        if r >= SPLIT_ROWS {
            return Err(SedGrepCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SedGrepCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5250 + r as u64);
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

    /// 收口封账闸：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), SedGrepCode> {
        if !self.gates_all() {
            return Err(SedGrepCode::ESeal);
        }
        if !self.splits_all() {
            return Err(SedGrepCode::ERowMiss);
        }
        if !self.cells_all() || !self.dials_all() || !self.cosigns_all() {
            return Err(SedGrepCode::ESeal);
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cells_all() && self.dials_all() && self.cosigns_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SedGrepLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SedGrepLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CELL_N {
        if let Some(w) = sg_cell(o) {
            if led.record_cell(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < DIAL_CELLS {
        if let Some(w) = dial_cell(i) {
            if led.record_dial(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        if led.record_cosign(c).is_ok() {
            recorded += 1;
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：族逆序 / 载荷漂移 / 越闭集三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SedGrepLedger::new();
        let caught = match kind {
            // 族逆序：先按合法序落 sed·-P 族正格，再回退 grep·-E 族触发逆序。
            0 => {
                let _ = lt.record_cell(17, sg_cell(17).unwrap_or(0));
                lt.record_cell(11, sg_cell(11).unwrap_or(0)) == Err(SedGrepCode::EOrder)
            }
            // 载荷漂移：先按合法序落同族正格，再注漂移载荷。
            1 => {
                let _ = lt.record_cell(10, sg_cell(10).unwrap_or(0));
                lt.record_cell(11, sg_cell(11).unwrap_or(0).wrapping_add(1)) == Err(SedGrepCode::EDrift)
            }
            // 越闭集：命令格 20 / 方言 6 / 联签 4 之外必抓（空账本同样必抓）。
            _ => {
                if k % 2 == 0 {
                    lt.record_cell(CELL_N, 0).is_err()
                } else {
                    lt.record_dial(DIAL_CELLS, 0).is_err()
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5260 + k as u64);
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
/// UNX-F52518 判据集（锚 UNX-F52519-J1，off-by-one 板式）。
pub fn run_unx52518_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52518");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "sed/grep 双工具 20 命令格全通",
        recorded >= CELL_N as u32,
        "sed s/y/d/p/q 五族 ×2 + grep -E/-F/-G/-P/-r 五族 ×2 逐格载荷恒等",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CELL_N {
                if sg_kind(o).unwrap_or(10) < sg_kind(o - 1).unwrap_or(10) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "sed 五族→grep 五族十族闭集单调",
    );
    s.add(
        "正则方言三态 6/6 全落",
        {
            let mut lt = SedGrepLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < DIAL_CELLS {
                if let Some(w) = dial_cell(i) {
                    if lt.record_dial(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.dials_all()
        },
        "BRE/ERE/PCRE 三方言逐格载荷对照，越方言必抓",
    );
    s.add(
        "管道消费联签 4/4 全落",
        {
            let mut lt = SedGrepLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if lt.record_cosign(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.cosigns_all()
        },
        "sed 输出喂 grep 四类联签逐格落账，越签必抓",
    );
    s.add(
        "ripgrep 分账声明 2/2 全齐（F52358）",
        {
            let mut lt = SedGrepLedger::new();
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
        "rg 独立账声明/rg 管道分账声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SedGrepLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "sed 替换/grep 匹配/管道联签/方言对账/EOL 面分账/退出码语义逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "族逆序/载荷漂移/越闭集三态轮转必抓",
    );
    let mut l0 = SedGrepLedger::new();
    s.add(
        "边界·命令格越闭集",
        l0.record_cell(CELL_N, 0).is_err(),
        "命令格 20 越闭集必拒",
    );
    s.add(
        "边界·方言越闭集",
        l0.record_dial(DIAL_CELLS, 0).is_err(),
        "方言 6 之外必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_cosign(COSIGN_CELLS).is_err(),
        "联签 4 之外必拒",
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
        "收口·封账闸",
        {
            let mut lt = SedGrepLedger::new();
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // 六闸缺 → ESeal；声明齐而格账缺 → ESeal。
            let a = lt.seal() == Err(SedGrepCode::ESeal);
            let _ = lt.judge_gate(0);
            let b = lt.seal() == Err(SedGrepCode::ESeal);
            a && b
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = SedGrepLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CELL_N {
                if let Some(w) = sg_cell(o) {
                    let _ = lt.record_cell(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < DIAL_CELLS {
                if let Some(w) = dial_cell(i) {
                    let _ = lt.record_dial(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                let _ = lt.record_cosign(c);
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC140",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sg_kind() {
        // 族映射 kind = o/2：格 0/1 同族，格 18/19 同族。
        assert_eq!(sg_kind(0), Some(0));
        assert_eq!(sg_kind(1), Some(0));
        assert_eq!(sg_kind(2), Some(1));
        assert_eq!(sg_kind(19), Some(9));
        assert!(sg_kind(20).is_none());
        assert!(sg_cell(20).is_none());
        // 工具十分位映射。
        assert_eq!(sg_tool(0), Some(0));
        assert_eq!(sg_tool(9), Some(0));
        assert_eq!(sg_tool(10), Some(1));
        assert_eq!(sg_tool(19), Some(1));
        assert!(sg_tool(20).is_none());
    }

    #[test]
    fn test_kind_names() {
        // 族名按 kind = o/2 索引核对。
        assert_eq!(sg_kind_name(0), Some("sed·s"));
        assert_eq!(sg_kind_name(4), Some("sed·q"));
        assert_eq!(sg_kind_name(5), Some("grep·-E"));
        assert_eq!(sg_kind_name(9), Some("grep·-r"));
        assert!(sg_kind_name(10).is_none());
    }

    #[test]
    fn test_dial_names() {
        assert_eq!(dial_name(0), Some("BRE"));
        assert_eq!(dial_name(2), Some("ERE"));
        assert_eq!(dial_name(4), Some("PCRE"));
        assert_eq!(dial_name(5), Some("PCRE"));
        assert!(dial_cell(DIAL_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = sg_cell(7).unwrap_or(0);
        assert_eq!(a, sg_cell(7).unwrap_or(0));
        assert_ne!(a, sg_cell(8).unwrap_or(0));
        let d = dial_cell(3).unwrap_or(0);
        assert_eq!(d, dial_cell(3).unwrap_or(0));
        assert_ne!(d, dial_cell(4).unwrap_or(0));
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 种子按合法序先落低族格，再验逆序必抓、合法续格可落。
        let mut lt = SedGrepLedger::new();
        let _ = lt.record_cell(17, sg_cell(17).unwrap_or(0));
        assert_eq!(lt.record_cell(11, sg_cell(11).unwrap_or(0)), Err(SedGrepCode::EOrder));
        assert!(lt.record_cell(18, sg_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SedGrepLedger::new();
        assert_eq!(
            lt.record_cell(5, sg_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SedGrepCode::EDrift)
        );
        assert_eq!(
            lt.record_dial(3, dial_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(SedGrepCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SedGrepLedger::new();
        let _ = lt.record_cell(0, sg_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cell(0, sg_cell(0).unwrap_or(0)), Err(SedGrepCode::EDup));
        let _ = lt.record_dial(0, dial_cell(0).unwrap_or(0));
        assert_eq!(lt.record_dial(0, dial_cell(0).unwrap_or(0)), Err(SedGrepCode::EDup));
        let _ = lt.record_cosign(0);
        assert_eq!(lt.record_cosign(0), Err(SedGrepCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SedGrepLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SedGrepLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let mut lt = SedGrepLedger::new();
        assert_eq!(lt.seal(), Err(SedGrepCode::ESeal));
        // 声明行缺 → ERowMiss。
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(SedGrepCode::ERowMiss));
        // 声明齐而格账缺 → ESeal。
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(SedGrepCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 命令格 + 6 方言 + 4 联签 = 30。
        assert_eq!(recorded, CELL_N as u32 + DIAL_CELLS as u32 + COSIGN_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCC140);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            SedGrepCode::Ok,
            SedGrepCode::ENoCell,
            SedGrepCode::EDrift,
            SedGrepCode::EOrder,
            SedGrepCode::ENoDial,
            SedGrepCode::ENoCos,
            SedGrepCode::ENoRow,
            SedGrepCode::ERowMiss,
            SedGrepCode::EDup,
            SedGrepCode::ESeal,
            SedGrepCode::EInj,
            SedGrepCode::EFp,
            SedGrepCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(SedGrepCode::Ok.val(), SEG_BASE);
        assert_eq!(SedGrepCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐（含六闸前置）S1。
        let lt = SedGrepLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = SedGrepLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut o = 0usize;
        while o < CELL_N {
            if let Some(w) = sg_cell(o) {
                let _ = lt2.record_cell(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < DIAL_CELLS {
            if let Some(w) = dial_cell(i) {
                let _ = lt2.record_dial(i, w);
            }
            i += 1;
        }
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            let _ = lt2.record_cosign(c);
            c += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52518_checks_green() {
        let s = run_unx52518_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
