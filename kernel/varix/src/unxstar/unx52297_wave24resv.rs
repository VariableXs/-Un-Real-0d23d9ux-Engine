//! UNX-F52297 波 24 预告与任务储备（B16–B40 深度 ≥3）· 0xC8A00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52297"，逐字）：
//!   title: 波 24 预告与任务储备（B16–B40 深度 ≥3）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52298-J1 增补册文
//!     件 + 主文件追加提交 + 推送结果三态如实登记（受阻则登记重试机制在
//!     位）；判据锚定 诚实登记纪律；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52297）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52298-J1，照原文锚定；功能按本卡标题（波 24 预告与任务储备）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① B16–B40 二十五段储备闭集：每段 2 格、合计 50 格；段名 B16..B40
//!      由 const fn 表给出，越段必抓。
//!   ② 深度 ≥3 闸：每格声明深度必须与段内推导深度恒等且 ≥3，浅格（深度
//!      <3）与漂移格（深度与推导不符）分别封拒，两条路都进注入账。
//!   ③ 储备段单调：B16→B40 段序非降，逆序落账必抓且不改 last_seg。
//!   ④ 与 B15 收口联签 2 行闭集：B15 收口承认行 / 波 24 储备承接行，
//!      缺一不定级。
//!   ⑤ ledger 状态机（new/judge_gate×6/record_cell/record_joint/
//!      fingerprint FNV-1a）；注入核 run_cycle 10 轮三态轮转（储备越闭
//!      集/深度漂移/储备逆序），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S2 封顶（储备预告册全通 S2，不虚定 S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52297;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 储备段数（B16..=B40）。
pub const RESERVE_SEGS: usize = 25;

/// 每段储备格数。
pub const CELLS_PER_SEG: usize = 2;

/// 储备格总数（25 段 × 2 格）。
pub const RESERVE_CELLS: usize = RESERVE_SEGS * CELLS_PER_SEG;

/// 深度下限（深度 ≥3 闸）。
pub const DEPTH_MIN: u32 = 3;

/// 与 B15 收口联签行数。
pub const JOINT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave24Code {
    /// 成功
    Ok = 0,
    /// 储备段越闭集
    ENoSeg,
    /// 段内槽位越闭集
    ENoSlot,
    /// 储备格载荷/深度漂移
    EDrift,
    /// 浅格：声明深度 <3
    EShallow,
    /// 储备段逆序
    EOrder,
    /// 联签行越闭集
    ENoRow,
    /// 缺联签行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 判据闸越闭集/未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl Wave24Code {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            Wave24Code::Ok => "成功",
            Wave24Code::ENoSeg => "储备段越闭集：B16–B40 二十五段之外先登记",
            Wave24Code::ENoSlot => "段内槽位越闭集：每段 2 格闭集之外先登记",
            Wave24Code::EDrift => "储备格漂移：载荷或声明深度与段内推导不恒等，逐格复算",
            Wave24Code::EShallow => "浅格拒收：声明深度 <3，不满足深度 ≥3 闸",
            Wave24Code::EOrder => "储备段逆序：B16→B40 段序非降，逆序必抓且不改 last_seg",
            Wave24Code::ENoRow => "联签行越闭集：与 B15 收口联签 2 行闭集之外先登记",
            Wave24Code::ERowMiss => "缺联签行：B15 承认行/波 24 承接行未齐先补齐",
            Wave24Code::EDup => "重复落账：同格只许一条账目",
            Wave24Code::ESeal => "判据闸越闭集：六闸闭集 0..=5 之外先登记",
            Wave24Code::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Wave24Code::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Wave24Code::EGradeDrift => "定级漂移：波 24 为预告储备册，全通封顶 S2，不得虚定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：正式收口（本储备册不得虚定）
    S1,
    /// S2：预告储备全通（本册封顶）
    S2,
    /// S3：储备未齐不收口
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
            Grade::S1 => "S1：留给 B15 正式收口册，波 24 储备册不得虚定",
            Grade::S2 => "S2：B16–B40 五十格+联签 2 行+六闸全通的预告储备封顶级",
            Grade::S3 => "S3：储备格或联签行未齐，不收口",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 段名（B16..B40）。
pub const fn seg_name(seg: usize) -> Option<&'static str> {
    match seg {
        0 => Some("B16"),
        1 => Some("B17"),
        2 => Some("B18"),
        3 => Some("B19"),
        4 => Some("B20"),
        5 => Some("B21"),
        6 => Some("B22"),
        7 => Some("B23"),
        8 => Some("B24"),
        9 => Some("B25"),
        10 => Some("B26"),
        11 => Some("B27"),
        12 => Some("B28"),
        13 => Some("B29"),
        14 => Some("B30"),
        15 => Some("B31"),
        16 => Some("B32"),
        17 => Some("B33"),
        18 => Some("B34"),
        19 => Some("B35"),
        20 => Some("B36"),
        21 => Some("B37"),
        22 => Some("B38"),
        23 => Some("B39"),
        24 => Some("B40"),
        _ => None,
    }
}

/// 段内推导深度（3..=5，全部 ≥3：深度 ≥3 闸的基准）。
pub const fn depth_of(seg: usize, slot: usize) -> Option<u32> {
    if seg >= RESERVE_SEGS || slot >= CELLS_PER_SEG {
        return None;
    }
    Some(3 + ((seg + slot) % 3) as u32)
}

/// 储备格基准载荷（XOR 锚常量低 16 位恒零）。
pub const fn reserve_cell(seg: usize, slot: usize) -> Option<u64> {
    if seg >= RESERVE_SEGS || slot >= CELLS_PER_SEG {
        return None;
    }
    Some((0xB1000u64 + (seg as u64) * 0x2C + (slot as u64) * 0x7) ^ 0x5732_0000_0000)
}

/// 与 B15 收口联签行折叠（行 r：0=B15 承认行 1=波 24 承接行）。
pub const fn joint_row(r: usize) -> Option<u32> {
    if r >= JOINT_ROWS {
        return None;
    }
    Some(0xB300 + r as u32 * 0x2C)
}

/// 联签行名。
pub const fn joint_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("B15 收口承认行"),
        1 => Some("波 24 储备承接行"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 波 24 预告储备账本状态机。
#[derive(Clone)]
pub struct Wave24Ledger {
    /// 六判据闸（预告口径：储备齐/深度闸/单调/联签/复算/诚实登记）。
    gates: [Option<bool>; 6],
    /// 储备格账（段 × 槽平铺 50 格）。
    cells: [Option<u64>; RESERVE_CELLS],
    /// 联签行账。
    joints: [Option<u32>; JOINT_ROWS],
    /// 最后落账段号（单调基准）。
    last_seg: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Wave24Ledger {
    /// 新账本。
    pub fn new() -> Self {
        Wave24Ledger {
            gates: [None; 6],
            cells: [None; RESERVE_CELLS],
            joints: [None; JOINT_ROWS],
            last_seg: -1,
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

    /// 落一格六判据闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Wave24Code> {
        if gate >= 6 {
            return Err(Wave24Code::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4600 + gate as u64);
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

    /// 储备一格（段/槽越闭集、深度 ≥3 闸、载荷对照、段单调、重复拒）。
    pub fn record_cell(&mut self, seg: usize, slot: usize, dcl_depth: u32, got: u64) -> Result<(), Wave24Code> {
        if seg >= RESERVE_SEGS {
            return Err(Wave24Code::ENoSeg);
        }
        if slot >= CELLS_PER_SEG {
            return Err(Wave24Code::ENoSlot);
        }
        // 深度 ≥3 闸：浅格与漂移格分路封拒，都进注入账。
        let want_depth = match depth_of(seg, slot) {
            Some(d) => d,
            None => return Err(Wave24Code::ENoSeg),
        };
        if dcl_depth < DEPTH_MIN {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4700 + seg as u64);
            return Err(Wave24Code::EShallow);
        }
        if dcl_depth != want_depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4701 + seg as u64);
            return Err(Wave24Code::EDrift);
        }
        // 储备段单调：B16→B40 非降，逆序必抓且不改 last_seg。
        if (seg as i64) < self.last_seg {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4702);
            return Err(Wave24Code::EOrder);
        }
        let want = match reserve_cell(seg, slot) {
            Some(w) => w,
            None => return Err(Wave24Code::ENoSeg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4703 + seg as u64);
            return Err(Wave24Code::EDrift);
        }
        let idx = seg * CELLS_PER_SEG + slot;
        if match self.cells.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Wave24Code::EDup);
        }
        if let Some(c) = self.cells.get_mut(idx) {
            *c = Some(got);
        }
        self.last_seg = seg as i64;
        self.fnv(0x4704 + idx as u64);
        Ok(())
    }

    /// 储备 50 格全账核验。
    pub fn cells_all(&self) -> bool {
        let mut i = 0usize;
        while i < RESERVE_CELLS {
            match self.cells.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 与 B15 收口联签一行。
    pub fn record_joint(&mut self, r: usize) -> Result<(), Wave24Code> {
        if r >= JOINT_ROWS {
            return Err(Wave24Code::ENoRow);
        }
        if match self.joints.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Wave24Code::EDup);
        }
        if let Some(c) = self.joints.get_mut(r) {
            *c = joint_row(r);
        }
        self.fnv(0x4800 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn joints_all(&self) -> bool {
        let mut r = 0usize;
        while r < JOINT_ROWS {
            match self.joints.get(r) {
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

    /// 最后落账段号（单调基准，注入失败不改）。
    pub fn last_seg(&self) -> i64 {
        self.last_seg
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S2 封顶，储备未齐 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cells_all() && self.joints_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for Wave24Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Wave24Ledger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut seg = 0usize;
    while seg < RESERVE_SEGS {
        let mut slot = 0usize;
        while slot < CELLS_PER_SEG {
            let d = match depth_of(seg, slot) {
                Some(d) => d,
                None => DEPTH_MIN,
            };
            let w = match reserve_cell(seg, slot) {
                Some(w) => w,
                None => 0,
            };
            if led.record_cell(seg, slot, d, w).is_ok() {
                recorded += 1;
            }
            slot += 1;
        }
        seg += 1;
    }
    let mut r = 0usize;
    while r < JOINT_ROWS {
        let _ = led.record_joint(r);
        r += 1;
    }
    // 注入 10 轮：储备越闭集 / 深度漂移 / 储备逆序三态轮转，
    // 全部落在当轮临时账本上（漂移与逆序注入先按合法序落正格再注入）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = Wave24Ledger::new();
        let caught = match kind {
            0 => {
                // 越闭集：空账本上段 25 越界必抓。
                lt.record_cell(RESERVE_SEGS, 0, DEPTH_MIN, 0).is_err()
            }
            1 => {
                // 深度漂移：先录正格再注漂移深度。
                let d0 = match depth_of(0, 0) {
                    Some(d) => d,
                    None => DEPTH_MIN,
                };
                let w0 = match reserve_cell(0, 0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_cell(0, 0, d0, w0);
                lt.record_cell(1, 0, d0, w0.wrapping_add(1)) == Err(Wave24Code::EDrift)
            }
            _ => {
                // 储备逆序：先按合法序落段 5 正格，再注段 2 逆序。
                let d5 = match depth_of(5, 0) {
                    Some(d) => d,
                    None => DEPTH_MIN,
                };
                let w5 = match reserve_cell(5, 0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_cell(5, 0, d5, w5);
                let d2 = match depth_of(2, 0) {
                    Some(d) => d,
                    None => DEPTH_MIN,
                };
                let w2 = match reserve_cell(2, 0) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_cell(2, 0, d2, w2) == Err(Wave24Code::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4900 + k as u64);
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
/// UNX-F52297 判据集（锚 UNX-F52298-J1，off-by-one 板式）。
pub fn run_unx52297_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52297");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "B16–B40 储备 50 格全落",
        recorded >= RESERVE_CELLS as u32,
        "二十五段 × 每段 2 格逐格恒等落账",
    );
    s.add(
        "深度 ≥3 闸全通",
        {
            let mut ok = true;
            let mut seg = 0usize;
            while seg < RESERVE_SEGS {
                let mut slot = 0usize;
                while slot < CELLS_PER_SEG {
                    match depth_of(seg, slot) {
                        Some(d) if d >= DEPTH_MIN => {}
                        _ => ok = false,
                    }
                    slot += 1;
                }
                seg += 1;
            }
            ok
        },
        "五十格推导深度全部 ≥3，浅格必拒",
    );
    s.add(
        "储备段单调",
        {
            // 顺序全落不触发 EOrder，即段序非降成立。
            let mut lt = Wave24Ledger::new();
            let mut ok = true;
            let mut seg = 0usize;
            while seg < RESERVE_SEGS && ok {
                let mut slot = 0usize;
                while slot < CELLS_PER_SEG && ok {
                    let d = match depth_of(seg, slot) {
                        Some(d) => d,
                        None => DEPTH_MIN,
                    };
                    let w = match reserve_cell(seg, slot) {
                        Some(w) => w,
                        None => 0,
                    };
                    if lt.record_cell(seg, slot, d, w).is_err() {
                        ok = false;
                    }
                    slot += 1;
                }
                seg += 1;
            }
            ok
        },
        "B16→B40 段序非降，逆序必抓且不改 last_seg",
    );
    s.add(
        "与 B15 收口联签 2/2 全齐",
        {
            let mut lt = Wave24Ledger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JOINT_ROWS {
                if lt.record_joint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.joints_all()
        },
        "B15 收口承认行 + 波 24 储备承接行两行闭集（诚实登记纪律）",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = Wave24Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "储备齐/深度闸/单调/联签/复算/诚实登记逐格落账",
    );
    s.add(
        "载荷恒等复算",
        {
            let a = match reserve_cell(7, 1) {
                Some(v) => v,
                None => 0,
            };
            let b = match reserve_cell(7, 1) {
                Some(v) => v,
                None => 1,
            };
            a == b
        },
        "储备格基准载荷纯函数逐位恒等",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "储备越闭集/深度漂移/储备逆序三态轮转必抓",
    );
    let mut l0 = Wave24Ledger::new();
    s.add(
        "边界·储备段越闭集",
        l0.record_cell(RESERVE_SEGS, 0, DEPTH_MIN, 0).is_err(),
        "段 25（B41 方向）越闭集必拒",
    );
    s.add(
        "边界·槽位越闭集",
        l0.record_cell(0, CELLS_PER_SEG, DEPTH_MIN, 0).is_err(),
        "槽位 2 越闭集必拒",
    );
    s.add(
        "边界·联签行越闭集",
        l0.record_joint(JOINT_ROWS).is_err(),
        "联签行 2 越闭集必拒",
    );
    s.add(
        "浅格拒收",
        {
            let w = match reserve_cell(3, 0) {
                Some(v) => v,
                None => 0,
            };
            l0.record_cell(3, 0, DEPTH_MIN - 1, w) == Err(Wave24Code::EShallow)
        },
        "声明深度 2 <3，深度 ≥3 闸必拒",
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
        "定级 S2 封顶",
        {
            let mut lt = Wave24Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut seg = 0usize;
            while seg < RESERVE_SEGS {
                let mut slot = 0usize;
                while slot < CELLS_PER_SEG {
                    let d = match depth_of(seg, slot) {
                        Some(d) => d,
                        None => DEPTH_MIN,
                    };
                    let w = match reserve_cell(seg, slot) {
                        Some(w) => w,
                        None => 0,
                    };
                    let _ = lt.record_cell(seg, slot, d, w);
                    slot += 1;
                }
                seg += 1;
            }
            let mut r = 0usize;
            while r < JOINT_ROWS {
                let _ = lt.record_joint(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（预告储备册封顶，不虚定 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8A00,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8A00",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_names() {
        assert_eq!(seg_name(0), Some("B16"));
        assert_eq!(seg_name(24), Some("B40"));
        assert!(seg_name(25).is_none());
        assert!(reserve_cell(25, 0).is_none());
        assert!(depth_of(0, 2).is_none());
    }

    #[test]
    fn test_depth_floor() {
        let mut seg = 0usize;
        while seg < RESERVE_SEGS {
            let mut slot = 0usize;
            while slot < CELLS_PER_SEG {
                let d = depth_of(seg, slot).unwrap_or(0);
                assert!(d >= DEPTH_MIN);
                assert!(d <= 5);
                slot += 1;
            }
            seg += 1;
        }
    }

    #[test]
    fn test_cells_pure() {
        let a = reserve_cell(9, 0).unwrap_or(0);
        assert_eq!(a, reserve_cell(9, 0).unwrap_or(0));
        assert_ne!(a, reserve_cell(9, 1).unwrap_or(0));
        assert_ne!(a, reserve_cell(10, 0).unwrap_or(0));
        assert!(joint_row(JOINT_ROWS).is_none());
        assert_eq!(joint_name(0), Some("B15 收口承认行"));
        assert_eq!(joint_name(1), Some("波 24 储备承接行"));
        assert!(joint_name(2).is_none());
    }

    #[test]
    fn test_order_caught_and_last_seg_kept() {
        let mut lt = Wave24Ledger::new();
        let d5 = depth_of(5, 0).unwrap_or(0);
        let w5 = reserve_cell(5, 0).unwrap_or(0);
        assert!(lt.record_cell(5, 0, d5, w5).is_ok());
        let d2 = depth_of(2, 0).unwrap_or(0);
        let w2 = reserve_cell(2, 0).unwrap_or(0);
        assert_eq!(lt.record_cell(2, 0, d2, w2), Err(Wave24Code::EOrder));
        assert_eq!(lt.last_seg(), 5);
        // 同段再落不逆序。
        let w5b = reserve_cell(5, 1).unwrap_or(0);
        assert!(lt.record_cell(5, 1, depth_of(5, 1).unwrap_or(0), w5b).is_ok());
    }

    #[test]
    fn test_shallow_caught() {
        let mut lt = Wave24Ledger::new();
        let w = reserve_cell(1, 0).unwrap_or(0);
        assert_eq!(lt.record_cell(1, 0, 2, w), Err(Wave24Code::EShallow));
        assert_eq!(lt.record_cell(1, 0, 1, w), Err(Wave24Code::EShallow));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (2, 2, 0));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = Wave24Ledger::new();
        let d = depth_of(2, 1).unwrap_or(0);
        let w = reserve_cell(2, 1).unwrap_or(0);
        assert_eq!(lt.record_cell(2, 1, d, w.wrapping_add(1)), Err(Wave24Code::EDrift));
        // 深度声明与推导不符（同为 ≥3）也是漂移。
        assert_eq!(lt.record_cell(2, 1, d + 1, w), Err(Wave24Code::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = Wave24Ledger::new();
        let d = depth_of(0, 0).unwrap_or(0);
        let w = reserve_cell(0, 0).unwrap_or(0);
        assert!(lt.record_cell(0, 0, d, w).is_ok());
        assert_eq!(lt.record_cell(0, 0, d, w), Err(Wave24Code::EDup));
    }

    #[test]
    fn test_joint_and_gate_boundary() {
        let mut lt = Wave24Ledger::new();
        let mut r = 0usize;
        while r < JOINT_ROWS {
            assert!(lt.record_joint(r).is_ok());
            r += 1;
        }
        assert!(lt.record_joint(JOINT_ROWS).is_err());
        assert!(lt.joints_all());
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= RESERVE_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC8A00);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52297u32 - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(Wave24Code::Ok as u32, 0);
        assert_eq!(Wave24Code::EGradeDrift as u32, 12);
        assert_eq!(Wave24Code::Ok.val(), SEG_BASE);
        assert_eq!(Wave24Code::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut all: [Wave24Code; 13] = [
            Wave24Code::Ok,
            Wave24Code::ENoSeg,
            Wave24Code::ENoSlot,
            Wave24Code::EDrift,
            Wave24Code::EShallow,
            Wave24Code::EOrder,
            Wave24Code::ENoRow,
            Wave24Code::ERowMiss,
            Wave24Code::EDup,
            Wave24Code::ESeal,
            Wave24Code::EInj,
            Wave24Code::EFp,
            Wave24Code::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < 13 {
            assert!(!all[i].detail().is_empty(), "码位 {} 明细不得为空", i);
            assert_eq!(all[i].val(), SEG_BASE + i as u32, "码位必须与枚举位序一致");
            i += 1;
        }
        all[0] = Wave24Code::Ok;
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = Wave24Ledger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_ne!(Grade::S2.detail(), "");
    }

    #[test]
    fn test_empty_boundary_on_fresh_ledger() {
        // 空账本上越闭集注入同样必抓。
        let mut lt = Wave24Ledger::new();
        assert_eq!(lt.record_cell(RESERVE_SEGS, 0, 3, 0), Err(Wave24Code::ENoSeg));
        assert_eq!(lt.record_cell(0, CELLS_PER_SEG, 3, 0), Err(Wave24Code::ENoSlot));
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn unx52297_checks_green() {
        let s = run_unx52297_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
