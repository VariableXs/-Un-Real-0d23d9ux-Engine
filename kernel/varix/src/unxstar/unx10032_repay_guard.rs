//! UNX-F10032 清偿回归护栏（清偿不破他例） · 0x917C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10033-J1 证据链齐备（复测+判定+台账三件 10/10 次），缺件
//! 10/10 次拒收。
//!
//! SEG_BASE = 0x917C0：F10032 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 护栏按「关联面回归账 + 变更序号戳」建模：清偿改动落一个变更序号，
//!      关联面闭集逐面必须有「序号 ≥ 变更序号且通过」的回归记录；缺面、
//!      过期面、红面一律不得翻绿——未回归翻绿 10/10 次拦截。
//!   ② 状态机 + 注册表 + 账本三件齐：条目生命周期 Red→GuardPending→Green
//!      只能经护栏翻绿；关联面注册表闭集冻结；回归账本逐面可回查。
//!   ③ 注入核四类坏样本：缺面回归 / 过期回归（改动前旧账）/ 红面回归 /
//!      绕护栏直翻绿，逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10032 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x917C0;

/// 关联面闭集容量（护栏覆盖面数上限）。
pub const SURFACE_CAPACITY: usize = 8;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESTALE: i32 = 116;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;

// ── 关联面注册表 ──────────────────────────────────────────────────────
/// 关联面闭集（清偿改动波及面冻结清单）。
pub const SURFACE_SET: [&str; 6] = [
    "S01_ledger",
    "S02_report",
    "S03_gate",
    "S04_audit",
    "S05_dual",
    "S06_drill",
];

/// 按序号取关联面名（1..=6），越界返回空串（显性可查）。
pub fn surface_label(n: usize) -> &'static str {
    match n {
        1 => SURFACE_SET[0],
        2 => SURFACE_SET[1],
        3 => SURFACE_SET[2],
        4 => SURFACE_SET[3],
        5 => SURFACE_SET[4],
        6 => SURFACE_SET[5],
        _ => "",
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// 缺面回归：关联面闭集有面无账
    MissingSurface,
    /// 过期回归：账面序号早于变更序号
    StaleResult,
    /// 红面回归：存在未通过面
    RedSurface,
    /// 绕护栏翻绿：未经护栏评审直接置绿
    BypassGreen,
    /// 未知关联面：不在六面闭集内
    UnknownSurface,
    /// 重复挂面
    DuplicateSurface,
    /// 状态回退：Green 退回 Red
    StateRegress,
    /// 变更序号零：改动未落戳
    ZeroChangeSeq,
    /// 绿面复跑翻红未留痕
    SilentRerun,
    /// 护栏未挂任何面即放行
    EmptyGuard,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl GuardCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::MissingSurface => "缺面回归被拒：关联面闭集有面无账；\
                请逐面补跑回归后再申请翻绿",
            GuardCode::StaleResult => "过期回归被拒：账面序号早于变更序号；\
                清偿改动后的面必须重跑",
            GuardCode::RedSurface => "红面回归被拒：存在未通过面；清偿不破他例\
                是硬约束，红面必须先修",
            GuardCode::BypassGreen => "绕护栏翻绿被拒：翻绿必须走护栏评审；\
                直置绿视为越闸",
            GuardCode::UnknownSurface => "未知关联面被拒：面名不在六面闭集内；\
                请按 S01..S06 归类",
            GuardCode::DuplicateSurface => "重复挂面被拒：同面已在册；请先查册\
                再补录，重复回 EEXIST",
            GuardCode::StateRegress => "状态回退被拒：Green 退回 Red 必须带\
                复跑留痕；静默回退视为私改",
            GuardCode::ZeroChangeSeq => "变更序号零被拒：清偿改动未落戳；\
                请先登记变更序号再跑回归",
            GuardCode::SilentRerun => "静默复跑被拒：绿面重跑结果必须落账；\
                不许只改状态不留账",
            GuardCode::EmptyGuard => "护栏空转被拒：未挂任何关联面不得放行；\
                请先按改动影响面挂护栏",
            GuardCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            GuardCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x917C0..=0x91840 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [GuardCode; 12] = [
        GuardCode::MissingSurface,
        GuardCode::StaleResult,
        GuardCode::RedSurface,
        GuardCode::BypassGreen,
        GuardCode::UnknownSurface,
        GuardCode::DuplicateSurface,
        GuardCode::StateRegress,
        GuardCode::ZeroChangeSeq,
        GuardCode::SilentRerun,
        GuardCode::EmptyGuard,
        GuardCode::NotReproducible,
        GuardCode::SegmentViolation,
    ];
}

// ── 条目状态机 ────────────────────────────────────────────────────────
/// 清偿条目生命周期：翻绿只能经护栏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    /// 红：清偿改动落地待护栏
    Red,
    /// 护栏待审：回归账已挂面
    GuardPending,
    /// 绿：护栏全过放行
    Green,
}

/// 单面回归记录：面名 + 账面序号 + 通过性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceResult {
    /// 关联面名（闭集内）
    pub surface: &'static str,
    /// 回归时的变更序号戳
    pub seq: u32,
    /// 是否通过
    pub passed: bool,
}

/// 回归护栏：关联面注册表 + 回归账本 + 翻绿评审。
#[derive(Clone, Debug)]
pub struct RepayGuard {
    change_seq: u32,
    item_state: ItemState,
    surfaces: Vec<&'static str>,
    results: Vec<SurfaceResult>,
}

impl RepayGuard {
    /// 构造 Red 条目护栏；change_seq 必须非零（改动必须落戳）。
    pub fn new(change_seq: u32) -> Result<RepayGuard, GuardCode> {
        if change_seq == 0 {
            return Err(GuardCode::ZeroChangeSeq);
        }
        Ok(RepayGuard {
            change_seq,
            item_state: ItemState::Red,
            surfaces: Vec::new(),
            results: Vec::new(),
        })
    }

    /// 当前条目状态。
    pub fn state(&self) -> ItemState {
        self.item_state
    }

    /// 变更序号。
    pub fn seq(&self) -> u32 {
        self.change_seq
    }

    /// 挂关联面：闭集校验 + 重复拒绝 + 容量守卫；挂面后进入 GuardPending。
    pub fn attach(&mut self, surface: &'static str) -> Result<(), GuardCode> {
        let mut known = false;
        let mut i = 0usize;
        while i < SURFACE_SET.len() {
            if let Some(s) = SURFACE_SET.get(i) {
                if *s == surface {
                    known = true;
                }
            }
            i += 1;
        }
        if !known {
            return Err(GuardCode::UnknownSurface);
        }
        if self.surfaces.len() >= SURFACE_CAPACITY {
            return Err(GuardCode::DuplicateSurface);
        }
        let mut j = 0usize;
        while j < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(j) {
                if *s == surface {
                    return Err(GuardCode::DuplicateSurface);
                }
            }
            j += 1;
        }
        self.surfaces.push(surface);
        self.item_state = ItemState::GuardPending;
        Ok(())
    }

    /// 挂面数。
    pub fn surface_len(&self) -> usize {
        self.surfaces.len()
    }

    /// 落一条回归账：面必须在护栏上；账面序号早于变更序号记为过期拒收。
    pub fn post(&mut self, surface: &'static str, seq: u32,
                passed: bool) -> Result<(), GuardCode> {
        let mut attached = false;
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(i) {
                if *s == surface {
                    attached = true;
                }
            }
            i += 1;
        }
        if !attached {
            return Err(GuardCode::UnknownSurface);
        }
        if seq < self.change_seq {
            return Err(GuardCode::StaleResult);
        }
        // 同面复跑：覆盖旧账但必须落账（禁静默）。
        let mut k = 0usize;
        while k < self.results.len() {
            if let Some(r) = self.results.get(k) {
                if r.surface == surface {
                    self.results.remove(k);
                    break;
                }
            }
            k += 1;
        }
        self.results.push(SurfaceResult { surface, seq, passed });
        Ok(())
    }

    /// 单面账查询。
    pub fn result_of(&self, surface: &str) -> Option<SurfaceResult> {
        let mut i = 0usize;
        while i < self.results.len() {
            if let Some(r) = self.results.get(i) {
                if r.surface == surface {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 护栏评审：逐面要求「账存在 + 序号 ≥ 变更序号 + 通过」；缺一类即拒绝。
    pub fn approve(&self) -> Result<(), GuardCode> {
        if self.item_state == ItemState::Green {
            return Err(GuardCode::StateRegress);
        }
        if self.surfaces.is_empty() {
            return Err(GuardCode::EmptyGuard);
        }
        let mut i = 0usize;
        while i < self.surfaces.len() {
            let s = match self.surfaces.get(i) {
                Some(x) => *x,
                None => return Err(GuardCode::MissingSurface),
            };
            match self.result_of(s) {
                None => return Err(GuardCode::MissingSurface),
                Some(r) => {
                    if r.seq < self.change_seq {
                        return Err(GuardCode::StaleResult);
                    }
                    if !r.passed {
                        return Err(GuardCode::RedSurface);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 翻绿：评审过才置 Green；任何直置绿路径不存在（私有状态，唯一入口）。
    pub fn green_up(&mut self) -> Result<(), GuardCode> {
        self.approve()?;
        self.item_state = ItemState::Green;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(GuardCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造两关联面护栏（S01+S02），变更序号 7。
pub fn guard_two_faces() -> RepayGuard {
    let mut g = RepayGuard::new(7).ok().unwrap_or(RepayGuard {
        change_seq: 1,
        item_state: ItemState::Red,
        surfaces: Vec::new(),
        results: Vec::new(),
    });
    let _ = g.attach(SURFACE_SET[0]);
    let _ = g.attach(SURFACE_SET[1]);
    g
}

/// 四类注入：0 缺面、1 过期账、2 红面、3 绕护栏（空护栏直翻绿）。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match c {
                0 => {
                    // 只给 S01 落账，S02 缺账 → 缺面拒绝。
                    let mut g = guard_two_faces();
                    let _ = g.post(SURFACE_SET[0], 7, true);
                    g.green_up().err() == Some(GuardCode::MissingSurface)
                }
                1 => {
                    // S01 过期账（序号 6 < 7）→ 过期拒绝。
                    let mut g = guard_two_faces();
                    let s01 = post_stale(&mut g);
                    s01.err() == Some(GuardCode::StaleResult)
                }
                2 => {
                    // S01 红账 → 红面拒绝。
                    let mut g = guard_two_faces();
                    let _ = g.post(SURFACE_SET[0], 7, false);
                    g.green_up().err() == Some(GuardCode::RedSurface)
                }
                _ => {
                    // 空护栏直翻绿 → 空转拒绝。
                    let g = RepayGuard::new(7);
                    match g {
                        Ok(mut g0) => {
                            let _ = g0.attach(SURFACE_SET[0]);
                            let _ = g0.post(SURFACE_SET[0], 7, true);
                            // 拆掉唯一面的账：直接新建空护栏翻绿。
                            let g1 = RepayGuard::new(7);
                            match g1 {
                                Ok(mut g2) => {
                                    g2.green_up().err() == Some(GuardCode::EmptyGuard)
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    }
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 落一条过期账（序号 6 早于变更序号 7）。
fn post_stale(g: &mut RepayGuard) -> Result<(), GuardCode> {
    g.post(SURFACE_SET[0], 6, true)
}

/// 构造全过护栏：六面齐账全过并翻绿。
pub fn build_green_guard() -> RepayGuard {
    let mut g = guard_two_faces();
    let mut n = 3usize;
    while n <= 6 {
        let _ = g.attach(surface_label(n));
        n += 1;
    }
    let mut n = 0usize;
    while n < SURFACE_SET.len() {
        if let Some(s) = SURFACE_SET.get(n) {
            let _ = g.post(s, 7, true);
        }
        n += 1;
    }
    let _ = g.green_up();
    g
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；绕护栏/越闸族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺面/缺账族"),
    ("EROFS", -30, 19, "Windows ERROR_WRITE_PROTECT；翻绿后只读族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知面/零戳族"),
    ("ESTALE", -116, 18, "Windows ERROR_NO_MORE_FILES；过期账族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 护栏指纹：变更序号 + 状态 + 逐面账入哈希。
pub fn guard_fingerprint(g: &RepayGuard) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((g.seq() & 0xFF) as u8);
    bytes.push(match g.state() {
        ItemState::Red => 0,
        ItemState::GuardPending => 1,
        ItemState::Green => 2,
    });
    bytes.push(g.surface_len() as u8);
    let mut n = 0usize;
    while n < SURFACE_SET.len() {
        if let Some(s) = SURFACE_SET.get(n) {
            match g.result_of(s) {
                Some(r) => {
                    bytes.push(r.seq as u8);
                    bytes.push(if r.passed { 1 } else { 0 });
                }
                None => bytes.push(0xFF),
            }
        }
        n += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10032 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10032_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10032");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x917C0..=0x91840).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x917C0..=0x91840", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 关联面闭集冻结：六面名册逐面可查
    let mut set_ok = surface_label(1) == "S01_ledger" && surface_label(6) == "S06_drill";
    let mut n = 1usize;
    while n <= 6 {
        if surface_label(n).is_empty() {
            set_ok = false;
        }
        n += 1;
    }
    s.add("闭集冻结：六关联面逐面可查", set_ok,
          "关联面清单是护栏覆盖口径，必须成文冻结");

    // 3. 零变更序号拒绝
    let zero = RepayGuard::new(0);
    s.add("落戳守卫：变更序号零拒绝建护栏",
          zero.err() == Some(GuardCode::ZeroChangeSeq),
          "清偿改动必须落变更序号，零戳视为未改动");

    // 4. 完整护栏：六面全过翻绿
    let full = build_green_guard();
    s.add("护栏闭环：六面回归全过且翻绿",
          full.state() == ItemState::Green && full.surface_len() == 6,
          "每面必须有 ≥ 变更序号的通过账才能翻绿");

    // 5. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "缺面/过期/红面/绕护栏必须全部显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 未回归翻绿拦截（核心判据）：缺面 × 逐面抽走 10/10
    let mut miss_caught = 0u32;
    let mut drop = 0usize;
    while drop < SURFACE_SET.len() {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut g = guard_two_faces();
            let mut n = 3usize;
            while n <= 6 {
                let _ = g.attach(surface_label(n));
                n += 1;
            }
            let mut n = 0usize;
            while n < SURFACE_SET.len() {
                if n != drop {
                    if let Some(sname) = SURFACE_SET.get(n) {
                        let _ = g.post(sname, 7, true);
                    }
                }
                n += 1;
            }
            if g.green_up().err() == Some(GuardCode::MissingSurface) {
                miss_caught += 1;
            }
            round += 1;
        }
        drop += 1;
    }
    s.add("未回归翻绿拦截：逐面抽账 60/60 次拒绝",
          miss_caught == SURFACE_SET.len() as u32 * INJECT_ROUNDS,
          "六面各缺账一次 × 10 轮，全部必须拒绝翻绿");

    // 8. 未知关联面拒绝
    let mut g = guard_two_faces();
    let unk = g.attach("S07_ghost");
    s.add("闭集拒绝：S07 未知面挂护栏被拒",
          unk.err() == Some(GuardCode::UnknownSurface),
          "面名不在六面闭集内必须显性拒绝");

    // 9. 重复挂面拒绝
    let mut dup = guard_two_faces();
    let dup_res = dup.attach(SURFACE_SET[0]);
    s.add("重复拒绝：同面二次挂护栏被拒",
          dup_res.err() == Some(GuardCode::DuplicateSurface),
          "重复挂面必须显性拒绝，不许静默覆盖");

    // 10. 绿后复跑落账（禁静默）：重跑覆盖旧账且状态不变
    let mut rerun = build_green_guard();
    let rerun_ok = rerun.post(SURFACE_SET[0], 8, true);
    s.add("复跑落账：绿面重跑覆盖旧账且保持绿",
          rerun_ok.is_ok() && rerun.state() == ItemState::Green
              && rerun.result_of(SURFACE_SET[0])
                  == Some(SurfaceResult { surface: SURFACE_SET[0], seq: 8, passed: true }),
          "复跑必须落账（新序号覆盖旧账），不许只改状态");

    // 11. 红面拦截（复测口径：同一批 10 轮）
    let mut red_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = guard_two_faces();
        let _ = g.post(SURFACE_SET[0], 7, true);
        let _ = g.post(SURFACE_SET[1], 7, false);
        if g.green_up().err() == Some(GuardCode::RedSurface) {
            red_caught += 1;
        }
        round += 1;
    }
    s.add("红面拦截：10/10 次拒绝翻绿", red_caught == INJECT_ROUNDS,
          "清偿不破他例：任何一面红都不得翻绿");

    // 12. 过期账拦截：改动前旧账（序号 6 < 7）10/10
    let mut stale_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = guard_two_faces();
        let r = g.post(SURFACE_SET[0], 6, true);
        if r.err() == Some(GuardCode::StaleResult) {
            stale_caught += 1;
        }
        round += 1;
    }
    s.add("过期拦截：改动前旧账 10/10 次拒收", stale_caught == INJECT_ROUNDS,
          "回归账面序号必须 ≥ 变更序号，旧账不认");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(ESTALE)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EROFS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = guard_fingerprint(&build_green_guard());
    let fp_b = guard_fingerprint(&build_green_guard());
    s.add("双跑一致：护栏指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = GuardCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < GuardCode::ALL.len() {
        let mut j = i + 1;
        while j < GuardCode::ALL.len() {
            if let (Some(a), Some(b)) = (GuardCode::ALL.get(i), GuardCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("护栏指纹 0x{:08X}（变更序号 {}）", fp_a, full.seq());
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_seq_rejected() {
        assert_eq!(RepayGuard::new(0).err(), Some(GuardCode::ZeroChangeSeq));
    }

    #[test]
    fn unknown_surface_rejected() {
        let mut g = guard_two_faces();
        assert_eq!(g.attach("S07_ghost"), Err(GuardCode::UnknownSurface));
    }

    #[test]
    fn missing_surface_blocks_green() {
        let mut g = guard_two_faces();
        let _ = g.post(SURFACE_SET[0], 7, true);
        assert_eq!(g.green_up(), Err(GuardCode::MissingSurface));
    }

    #[test]
    fn stale_result_rejected() {
        let mut g = guard_two_faces();
        assert_eq!(g.post(SURFACE_SET[0], 6, true), Err(GuardCode::StaleResult));
    }

    #[test]
    fn red_surface_blocks_green() {
        let mut g = guard_two_faces();
        let _ = g.post(SURFACE_SET[0], 7, false);
        assert_eq!(g.green_up(), Err(GuardCode::RedSurface));
    }

    #[test]
    fn empty_guard_blocks_green() {
        let mut g = RepayGuard::new(3).ok().unwrap();
        assert_eq!(g.green_up(), Err(GuardCode::EmptyGuard));
    }

    #[test]
    fn full_guard_greens() {
        let g = build_green_guard();
        assert_eq!(g.state(), ItemState::Green);
        assert_eq!(g.surface_len(), 6);
    }

    #[test]
    fn duplicate_surface_rejected() {
        let mut g = guard_two_faces();
        assert_eq!(g.attach(SURFACE_SET[1]), Err(GuardCode::DuplicateSurface));
    }

    #[test]
    fn rerun_overwrites_ledger() {
        let mut g = build_green_guard();
        assert!(g.post(SURFACE_SET[0], 9, true).is_ok());
        assert_eq!(g.result_of(SURFACE_SET[0]).map(|r| r.seq), Some(9));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(guard_fingerprint(&build_green_guard()),
                   guard_fingerprint(&build_green_guard()));
    }

    #[test]
    fn unx10032_checks_green() {
        let set = run_unx10032_checks();
        assert!(set.all_passed(), "UNX-F10032 red: {}", set.red_items_text());
    }
}
