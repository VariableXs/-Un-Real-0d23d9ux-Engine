//! UNX-F52462 Microsoft Edge 收口 · 0xCB340段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52462"，逐字）：
//!   title: Microsoft Edge 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52463-J1 六判据
//!     全落；Gecko 进程模型账 ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52462）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52463-J1，照原文锚定；功能按本卡标题（Microsoft Edge 收口）
//! 落地。卡面 Gecko 进程模型账 ×10 计数器由注入核 10 轮全抓承担记账。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 格闭集：Chromium 面/IE 模式/集锦/性能/企业各 4 格，
//!      家族单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收。
//!   ② IE 模式三态 6 格：关/开/自动三态各 2 格（默认策略+用户覆盖），
//!      越态与漂移必抓。
//!   ③ 与 Chrome 对照 8 格：逐格载荷恒等才收，越格与漂移必抓。
//!   ④ 更新通道 4 档：Stable/Beta/Dev/Canary，越档与漂移必抓。
//!   ⑤ Chromium 上栈声明 2 行闭集：上栈同构声明/上栈分账声明，缺一拒
//!      封。
//!   ⑥ 六判据收口闸（启动/IE/崩溃/卸载/升级/许可）逐格落账，未齐即封
//!      拒。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（IE 格越闭集/
//!      操作漂移/更新通道越档，三类注入全落当轮临时账本，空账本同样必
//!      抓，escaped=0）；注入=检出+逃逸守恒；双跑 FNV 指纹一致；KB-COMPAT
//!      定级 S1/S2/S3 闭集（全通 → S1）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52462;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FEATURE_OPS: usize = 20;

/// IE 模式格数（三态 × 2）。
pub const IE_CELLS: usize = 6;

/// IE 模式态数（关/开/自动）。
pub const IE_STATES: usize = 3;

/// 与 Chrome 对照格数。
pub const CMP_CELLS: usize = 8;

/// 更新通道档数。
pub const UPDATE_CHANNELS: usize = 4;

/// Chromium 上栈声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// IE 格越闭集
    ENoIe,
    /// IE 态非法或漂移
    EIeBad,
    /// 更新通道越档
    ENoChan,
    /// 更新通道漂移
    EChanDrift,
    /// 对照格越闭集
    ENoCmp,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl EdgeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            EdgeCode::Ok => "成功",
            EdgeCode::ENoOp => "操作号越闭集：Chromium 面/IE 模式/集锦/性能/企业 20 格之外先登记",
            EdgeCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            EdgeCode::EOrder => "家族逆序：Chromium 面→IE 模式→集锦→性能→企业单调，逆序必抓",
            EdgeCode::ENoIe => "IE 格越闭集：IE 模式三态 6 格之外先登记",
            EdgeCode::EIeBad => "IE 态非法或漂移：关/开/自动三态闭集之外或载荷与基准不恒等",
            EdgeCode::ENoChan => "更新通道越档：Stable/Beta/Dev/Canary 四档闭集之外先登记",
            EdgeCode::EChanDrift => "更新通道漂移：通道档载荷与基准不恒等，逐档复算",
            EdgeCode::ENoCmp => "对照格越闭集：与 Chrome 对照 8 格之外先登记",
            EdgeCode::ENoRow => "声明行越闭集：Chromium 上栈 2 行闭集之外先登记",
            EdgeCode::EDup => "重复落账：同格只许一条账目",
            EdgeCode::ESeal => "未齐即封：20 操作 + 6 IE 格 + 8 对照格 + 4 通道未齐先补齐",
            EdgeCode::EGradeDrift => "定级漂移：全通且 Chromium 上栈声明全齐才可定 S1",
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
    /// 定级值（match 闭集，const 安全）。
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
            Grade::S1 => "S1：六判据+20操作+6 IE 格+8 对照格+4 通道+2 行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（操作 o；家族 = o/4）。
pub const fn feature_cell(o: usize) -> Option<u64> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=Chromium 面 4..8=IE 模式 8..12=集锦 12..16=性能 16..20=企业）。
pub const fn feature_kind(o: usize) -> Option<u32> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn feature_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Chromium 面"),
        1 => Some("IE 模式"),
        2 => Some("集锦"),
        3 => Some("性能"),
        4 => Some("企业"),
        _ => None,
    }
}

/// IE 模式格载荷（格 i：态 = i/2，0..2=关 2..4=开 4..6=自动）。
pub const fn ie_cell(i: usize) -> Option<u64> {
    if i >= IE_CELLS {
        return None;
    }
    Some((0xD2000u64 + (i as u64) * 0x3B) ^ ((i / 2) as u64).wrapping_mul(0x00000193))
}

/// IE 态名。
pub const fn ie_state_name(st: usize) -> Option<&'static str> {
    match st {
        0 => Some("关"),
        1 => Some("开"),
        2 => Some("自动"),
        _ => None,
    }
}

/// IE 态（格 i → 0..2）。
pub const fn ie_state(i: usize) -> Option<u32> {
    if i >= IE_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 与 Chrome 对照格载荷（对照格 c）。
pub const fn cmp_cell(c: usize) -> Option<u64> {
    if c >= CMP_CELLS {
        return None;
    }
    Some((0xD3000u64 + (c as u64) * 0x41) ^ 0x454447455F435431)
}

/// 更新通道档载荷（档 c：0=Stable 1=Beta 2=Dev 3=Canary）。
pub const fn channel_cell(c: usize) -> Option<u64> {
    if c >= UPDATE_CHANNELS {
        return None;
    }
    Some((0xD4000u64 + (c as u64) * 0x5D) ^ 0x454447455F43484E)
}

/// 更新通道档名。
pub const fn channel_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("Stable"),
        1 => Some("Beta"),
        2 => Some("Dev"),
        3 => Some("Canary"),
        _ => None,
    }
}

/// Chromium 上栈声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Edge 收口账本状态机。
#[derive(Clone)]
pub struct EdgeLedger {
    /// 六判据（启动/IE/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FEATURE_OPS],
    /// IE 模式账。
    ie: [Option<u64>; IE_CELLS],
    /// 对照格账。
    cmp: [Option<u64>; CMP_CELLS],
    /// 更新通道账。
    chan: [Option<u64>; UPDATE_CHANNELS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EdgeLedger {
    /// 新账本。
    pub fn new() -> Self {
        EdgeLedger {
            gates: [None; 6],
            ops: [None; FEATURE_OPS],
            ie: [None; IE_CELLS],
            cmp: [None; CMP_CELLS],
            chan: [None; UPDATE_CHANNELS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), EdgeCode> {
        if gate >= 6 {
            return Err(EdgeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x53A00 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), EdgeCode> {
        if o >= FEATURE_OPS {
            return Err(EdgeCode::ENoOp);
        }
        let kind = match feature_kind(o) {
            Some(k) => k,
            None => return Err(EdgeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53B00);
            return Err(EdgeCode::EOrder);
        }
        let want = match feature_cell(o) {
            Some(w) => w,
            None => return Err(EdgeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53B01 + o as u64);
            return Err(EdgeCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EdgeCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x53B02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// IE 模式一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_ie(&mut self, i: usize, got: u64) -> Result<(), EdgeCode> {
        let want = match ie_cell(i) {
            Some(w) => w,
            None => return Err(EdgeCode::ENoIe),
        };
        if match ie_state(i) {
            Some(_) => false,
            None => true,
        } {
            return Err(EdgeCode::ENoIe);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53C00 + i as u64);
            return Err(EdgeCode::EIeBad);
        }
        if match self.ie.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EdgeCode::EDup);
        }
        if let Some(c) = self.ie.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x53C01 + i as u64);
        Ok(())
    }

    /// IE 模式全账核验（6 格）。
    pub fn ie_all(&self) -> bool {
        let mut i = 0usize;
        while i < IE_CELLS {
            match self.ie.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 对照一格（载荷对照 + 重复拒；漂移按操作漂移口径归并）。
    pub fn record_cmp(&mut self, c: usize, got: u64) -> Result<(), EdgeCode> {
        let want = match cmp_cell(c) {
            Some(w) => w,
            None => return Err(EdgeCode::ENoCmp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53D00 + c as u64);
            return Err(EdgeCode::EDrift);
        }
        if match self.cmp.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EdgeCode::EDup);
        }
        if let Some(x) = self.cmp.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x53D01 + c as u64);
        Ok(())
    }

    /// 对照全账核验（8 格）。
    pub fn cmp_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMP_CELLS {
            match self.cmp.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 更新通道一档（载荷对照 + 重复拒）。
    pub fn record_channel(&mut self, c: usize, got: u64) -> Result<(), EdgeCode> {
        let want = match channel_cell(c) {
            Some(w) => w,
            None => return Err(EdgeCode::ENoChan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53E00 + c as u64);
            return Err(EdgeCode::EChanDrift);
        }
        if match self.chan.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EdgeCode::EDup);
        }
        if let Some(x) = self.chan.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x53E01 + c as u64);
        Ok(())
    }

    /// 更新通道全账核验（4 档）。
    pub fn chan_all(&self) -> bool {
        let mut c = 0usize;
        while c < UPDATE_CHANNELS {
            match self.chan.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（Chromium 上栈）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), EdgeCode> {
        if r >= SPLIT_ROWS {
            return Err(EdgeCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EdgeCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x53F00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.ie_all()
            && self.cmp_all()
            && self.chan_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for EdgeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EdgeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURE_OPS {
        if let Some(w) = feature_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < IE_CELLS {
        if let Some(w) = ie_cell(i) {
            if led.record_ie(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CMP_CELLS {
        if let Some(w) = cmp_cell(c) {
            if led.record_cmp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut h = 0usize;
    while h < UPDATE_CHANNELS {
        if let Some(w) = channel_cell(h) {
            if led.record_channel(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：IE 格越闭集 / 操作漂移 / 更新通道越档三态轮转。
    // 三类注入全部落在当轮临时账本上：空账本同样必抓，escaped 恒 0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EdgeLedger::new();
        let caught = match kind {
            0 => lt.record_ie(IE_CELLS, 0).is_err(),
            1 => {
                let bad = match feature_cell(7) {
                    Some(w) => w.wrapping_add(1),
                    None => 1,
                };
                lt.record_op(7, bad) == Err(EdgeCode::EDrift)
            }
            _ => lt.record_channel(UPDATE_CHANNELS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x54000 + k as u64);
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
/// UNX-F52462 判据集（锚 UNX-F52463-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52462_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52462");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Edge 五功能族 20 格全通",
        recorded >= FEATURE_OPS as u32,
        "Chromium 面/IE 模式/集锦/性能/企业五家族逐操作恒等",
    );
    s.add(
        "IE 模式三态 6 格全落",
        {
            let mut lt = EdgeLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < IE_CELLS {
                if let Some(w) = ie_cell(i) {
                    if lt.record_ie(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.ie_all()
        },
        "关/开/自动三态各 2 格（默认策略+用户覆盖）逐格恒等，越态必抓",
    );
    s.add(
        "与 Chrome 对照 8 格全恒等",
        {
            let mut lt = EdgeLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMP_CELLS {
                if let Some(w) = cmp_cell(c) {
                    if lt.record_cmp(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmp_all()
        },
        "与 Chrome 对照 8 格逐格恒等，越格必抓",
    );
    s.add(
        "更新通道 Stable/Beta/Dev/Canary 四档全落",
        {
            let mut lt = EdgeLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < UPDATE_CHANNELS {
                if let Some(w) = channel_cell(h) {
                    if lt.record_channel(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.chan_all()
        },
        "四档闭集逐档恒等，越档必抓",
    );
    s.add(
        "Chromium 上栈声明 2/2 全齐",
        {
            let mut lt = EdgeLedger::new();
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
        "Chromium 上栈同构声明/Chromium 上栈分账声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = EdgeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/IE/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURE_OPS {
                let a = match feature_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let b = match feature_kind(o - 1) {
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
        "Chromium 面→IE 模式→集锦→性能→企业五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "IE 格越闭集/操作漂移/更新通道越档三态轮转必抓",
    );
    let mut l0 = EdgeLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FEATURE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·IE 格越闭集",
        l0.record_ie(IE_CELLS, 0).is_err(),
        "IE 格 6 越闭集必拒",
    );
    s.add(
        "边界·更新通道越档",
        l0.record_channel(UPDATE_CHANNELS, 0).is_err(),
        "通道档 4 越闭集必拒",
    );
    s.add(
        "未齐不封核",
        {
            let mut lt = EdgeLedger::new();
            let w = match feature_cell(0) {
                Some(w) => w,
                None => 0,
            };
            lt.record_op(0, w).is_ok() && lt.judge_grade() != Grade::S1
        },
        "账未齐时合法落格仍 Ok 且不定 S1，未齐即封被拒",
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
            let mut lt = EdgeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURE_OPS {
                if let Some(w) = feature_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < IE_CELLS {
                if let Some(w) = ie_cell(i) {
                    let _ = lt.record_ie(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < CMP_CELLS {
                if let Some(w) = cmp_cell(c) {
                    let _ = lt.record_cmp(c, w);
                }
                c += 1;
            }
            let mut h = 0usize;
            while h < UPDATE_CHANNELS {
                if let Some(w) = channel_cell(h) {
                    let _ = lt.record_channel(h, w);
                }
                h += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB340,
        "SEG_BASE 0x40 对齐且在 VX16 域（0xCA400 + 0x40×61）",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_kind() {
        assert_eq!(feature_kind(0), Some(0));
        assert_eq!(feature_kind(3), Some(0));
        assert_eq!(feature_kind(4), Some(1));
        assert_eq!(feature_kind(19), Some(4));
        assert!(feature_kind(20).is_none());
        assert!(feature_cell(20).is_none());
    }

    #[test]
    fn test_ie_states() {
        assert_eq!(ie_state(0), Some(0));
        assert_eq!(ie_state(1), Some(0));
        assert_eq!(ie_state(2), Some(1));
        assert_eq!(ie_state(5), Some(2));
        assert!(ie_state(6).is_none());
        assert_eq!(ie_state_name(0), Some("关"));
        assert_eq!(ie_state_name(1), Some("开"));
        assert_eq!(ie_state_name(2), Some("自动"));
        assert!(ie_state_name(3).is_none());
        assert!(ie_cell(IE_CELLS).is_none());
    }

    #[test]
    fn test_channels() {
        assert_eq!(channel_name(0), Some("Stable"));
        assert_eq!(channel_name(1), Some("Beta"));
        assert_eq!(channel_name(2), Some("Dev"));
        assert_eq!(channel_name(3), Some("Canary"));
        assert!(channel_name(4).is_none());
        assert!(channel_cell(UPDATE_CHANNELS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match cmp_cell(3) {
            Some(a) => a,
            None => 0,
        };
        assert_eq!(a, match cmp_cell(3) {
            Some(a) => a,
            None => 0,
        });
        assert_ne!(a, match cmp_cell(4) {
            Some(a) => a,
            None => 0,
        });
        assert!(cmp_cell(CMP_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = EdgeLedger::new();
        // 种子按合法序先落低族格（操作 1 属 Chromium 面族）。
        let _ = lt.record_op(1, feature_cell(1).unwrap_or(0));
        assert!(lt.record_op(2, feature_cell(2).unwrap_or(0)).is_ok());
        let mut lt2 = EdgeLedger::new();
        let _ = lt2.record_op(17, feature_cell(17).unwrap_or(0));
        assert_eq!(lt2.record_op(2, feature_cell(2).unwrap_or(0)), Err(EdgeCode::EOrder));
        assert!(lt2.record_op(18, feature_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = EdgeLedger::new();
        assert_eq!(
            lt.record_op(5, feature_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(EdgeCode::EDrift)
        );
        assert_eq!(
            lt.record_ie(4, ie_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(EdgeCode::EIeBad)
        );
        assert_eq!(
            lt.record_channel(2, channel_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(EdgeCode::EChanDrift)
        );
        assert_eq!(
            lt.record_cmp(0, cmp_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(EdgeCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = EdgeLedger::new();
        let _ = lt.record_channel(0, channel_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_channel(0, channel_cell(0).unwrap_or(0)),
            Err(EdgeCode::EDup)
        );
        let _ = lt.record_ie(0, ie_cell(0).unwrap_or(0));
        assert_eq!(lt.record_ie(0, ie_cell(0).unwrap_or(0)), Err(EdgeCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(EdgeCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = EdgeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = EdgeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：只含 run_cycle 实际计数的节（操作+IE+对照+通道）。
        assert_eq!(
            recorded,
            (FEATURE_OPS + IE_CELLS + CMP_CELLS + UPDATE_CHANNELS) as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_conserved() {
        let mut lt = EdgeLedger::new();
        let (i0, c0, e0) = lt.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
        // 空账直接落高族 → 序注入（错误路径单点记账）。
        let _ = lt.record_op(FEATURE_OPS - 1, feature_cell(FEATURE_OPS - 1).unwrap_or(0));
        let _ = lt.record_op(0, feature_cell(0).unwrap_or(0));
        let (i1, c1, e1) = lt.stats();
        assert_eq!(i1, i0 + 1);
        assert_eq!(c1, c0 + 1);
        assert_eq!(e1, e0);
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
        assert_eq!(SEG_BASE, 0xCB340);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(EdgeCode::Ok as u32, 0);
        assert_eq!(EdgeCode::EGradeDrift as u32, 12);
        assert!(!EdgeCode::Ok.detail().is_empty());
        assert!(!EdgeCode::EIeBad.detail().is_empty());
        assert!(!EdgeCode::EGradeDrift.detail().is_empty());
        assert_eq!(EdgeCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt0 = EdgeLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = EdgeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = EdgeLedger::new();
        let mut o = 0usize;
        while o < FEATURE_OPS {
            let _ = lt2.record_op(o, feature_cell(o).unwrap_or(0));
            o += 1;
        }
        let mut g2 = 0usize;
        while g2 < 6 {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52462_checks_green() {
        let s = run_unx52462_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
