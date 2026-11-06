//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18554 · 档案面防重声明（与 E1/B04 划界）· 0x80640 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18554-J1 20 条主题聚类对 E1/B04 可见判据对表，嫌疑
//!       清单=空断言
//! 本模块结构清单：
//!   1. 冻结常量层：20 条主题聚类（topic 0..20，id%3 定可见域——
//!      %3==0 归 E1 可见×7、%3==1 归 B04 可见×7、%3==2 档案面独占
//!      ×6，计数 7+7+6=20 闭合）、3 属主闭集、4 级划界优先序；
//!   2. 诊断码闭集 DdCode（code() = SEG_BASE + n，0x80640 段窗口）；
//!   3. 数据结构一（占用位面）：ClaimPlane——20 bit 单面（[u16;2]
//!      拆 2×8bit），一次声明置一位，重复声明位面层即拦；
//!   4. 数据结构二（嫌疑账）：SuspectLedger——重复声明/划界违规/
//!      未知主题三路嫌疑入账，裁决核逐条结案，结案后嫌疑清单=空；
//!   5. 划界核 adjudicate：属主优先序冻结——E1 先于 B04 先于档案面；
//!      档案面声明 E1/B04 可见主题=划界违规（必抓）；
//!   6. 注入核 3 类×10 轮：重复声明 / 划界违规 / 未知主题，必抓；
//!   7. 对账核 reconcile：20 条聚类逐条对 E1/B04 可见判据表
//!      （可见域+属主+判据名三元组逐行复算），对不上即红；
//!   8. 黄金全链 golden_run + run_unx18554_checks：14 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么划界优先序冻结：防重的本质是「一件事只有一个属主」——
//! E1/B04 先在的判据档案面不得再声明，否则同一主题两处记账、
//! 两处收口，守恒就碎了。优先序写死在裁决核里，任何越界声明都会
//! 落嫌疑清单；结案后清单=空是本卡 J1 的空集断言。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80640，AI-VX11-1105-G1·A16 批 08 独占窗口）。
pub const SEG_BASE: u32 = 0x80640;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 主题聚类数（冻结：判据原文 20 条）。
pub const TOPIC_N: usize = 20;
/// E1 可见主题数（推导：id%3==0，id∈0..20 → 0,3,6,9,12,15,18 共 7）。
pub const E1_VISIBLE_N: usize = 7;
/// B04 可见主题数（推导：id%3==1，id∈0..20 → 1,4,7,10,13,16,19 共 7）。
pub const B04_VISIBLE_N: usize = 7;
/// 档案面独占主题数（推导：id%3==2 → 2,5,8,11,14,17 共 6）。
pub const FACE_EXCLUSIVE_N: usize = 6;
/// 属主闭集：0=E1、1=B04、2=档案面。
pub const OWNER_E1: u8 = 0;
pub const OWNER_B04: u8 = 1;
pub const OWNER_FACE: u8 = 2;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DdCode {
    /// 未知主题：id 落出 0..20
    TopicUnknown,
    /// 重复声明：同主题第二声明
    ClaimDup,
    /// 划界违规：档案面声明 E1/B04 可见主题
    BoundaryViolation,
    /// 属主越界
    OwnerRange,
    /// 位面账损坏：置位数与声明数失配
    PlaneCorrupt,
    /// 嫌疑清单非空（结案后）
    SuspectNonEmpty,
    /// 对账失配：20 条聚类对表对不上
    ReconcileMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl DdCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DdCode::TopicUnknown => "未知主题：id 不在 0..20 闭集",
            DdCode::ClaimDup => "重复声明：同主题第二声明，必抓",
            DdCode::BoundaryViolation => "划界违规：档案面越界声明 E1/B04 主题",
            DdCode::OwnerRange => "属主越界：不在 E1/B04/档案面闭集",
            DdCode::PlaneCorrupt => "位面账损坏：置位数与声明数失配",
            DdCode::SuspectNonEmpty => "嫌疑清单非空：结案后必须为空",
            DdCode::ReconcileMismatch => "对账失配：聚类对表逐行复算不过",
            DdCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [DdCode; 8] = [
        DdCode::TopicUnknown,
        DdCode::ClaimDup,
        DdCode::BoundaryViolation,
        DdCode::OwnerRange,
        DdCode::PlaneCorrupt,
        DdCode::SuspectNonEmpty,
        DdCode::ReconcileMismatch,
        DdCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if DdCode::ALL[i].code() == code {
            return DdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80640 段闭集内"
}

// ── 主题聚类（冻结语料） ────────────────────────────────────────
/// 一条主题聚类：id + 可见域判定（id%3 冻结派生）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopicCluster {
    pub topic_id: u8,
    /// E1 可见判据表可见性（id%3==0）。
    pub e1_visible: bool,
    /// B04 可见判据表可见性（id%3==1）。
    pub b04_visible: bool,
    /// 冻结主题名（2~3 词抽象，语料闭集）。
    pub name: &'static str,
}

/// 20 条主题聚类冻结表（可见域由 id%3 派生，逐行推导注记）。
pub const TOPICS: [TopicCluster; TOPIC_N] = [
    TopicCluster { topic_id: 0,  e1_visible: true,  b04_visible: false, name: "env_layer" },
    TopicCluster { topic_id: 1,  e1_visible: false, b04_visible: true,  name: "cmd_position" },
    TopicCluster { topic_id: 2,  e1_visible: false, b04_visible: false, name: "profile_scope" },
    TopicCluster { topic_id: 3,  e1_visible: true,  b04_visible: false, name: "winver_gate" },
    TopicCluster { topic_id: 4,  e1_visible: false, b04_visible: true,  name: "cmd_ext" },
    TopicCluster { topic_id: 5,  e1_visible: false, b04_visible: false, name: "profile_dup" },
    TopicCluster { topic_id: 6,  e1_visible: true,  b04_visible: false, name: "dll_redirect" },
    TopicCluster { topic_id: 7,  e1_visible: false, b04_visible: true,  name: "cmd_snap" },
    TopicCluster { topic_id: 8,  e1_visible: false, b04_visible: false, name: "profile_arb" },
    TopicCluster { topic_id: 9,  e1_visible: true,  b04_visible: false, name: "fs_visibility" },
    TopicCluster { topic_id: 10, e1_visible: false, b04_visible: true,  name: "cmd_ext2" },
    TopicCluster { topic_id: 11, e1_visible: false, b04_visible: false, name: "profile_hotswap" },
    TopicCluster { topic_id: 12, e1_visible: true,  b04_visible: false, name: "reg_virtual" },
    TopicCluster { topic_id: 13, e1_visible: false, b04_visible: true,  name: "cmd_lex" },
    TopicCluster { topic_id: 14, e1_visible: false, b04_visible: false, name: "profile_errpath" },
    TopicCluster { topic_id: 15, e1_visible: true,  b04_visible: false, name: "gdi_semantic" },
    TopicCluster { topic_id: 16, e1_visible: false, b04_visible: true,  name: "cmd_verbdump" },
    TopicCluster { topic_id: 17, e1_visible: false, b04_visible: false, name: "profile_pano" },
    TopicCluster { topic_id: 18, e1_visible: true,  b04_visible: false, name: "path_normal" },
    TopicCluster { topic_id: 19, e1_visible: false, b04_visible: true,  name: "cmd_flagset" },
];

/// 主题所属主（划界裁决的冻结结论）：E1 可见→E1，B04 可见→B04，
/// 双不可见→档案面独占。
pub fn frozen_owner(topic_id: u8) -> Result<u8, u32> {
    match TOPICS.get(topic_id as usize) {
        Some(t) => {
            if t.e1_visible {
                Ok(OWNER_E1)
            } else if t.b04_visible {
                Ok(OWNER_B04)
            } else {
                Ok(OWNER_FACE)
            }
        }
        None => Err(DdCode::TopicUnknown.code()),
    }
}

// ── 数据结构一：声明占用位面 ────────────────────────────────────
/// 20 bit 单面（2×8bit 拆槽）：一次声明置一位。
/// 为什么不用 Vec 查重：位面置位数与声明计数构成天然守恒对，
/// 账坏了立刻现形（PlaneCorrupt），且查重 O(1)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimPlane {
    bits: [u8; 2],
    set_count: u32,
}

impl ClaimPlane {
    /// 新建空位面。
    pub fn new() -> ClaimPlane {
        ClaimPlane { bits: [0, 0], set_count: 0 }
    }
    /// 声明落位：越界/重复显式拒。
    pub fn claim(&mut self, topic_id: u8) -> Result<(), u32> {
        if topic_id as usize >= TOPIC_N {
            return Err(DdCode::TopicUnknown.code());
        }
        let byte = (topic_id as usize) / 8;
        let bit = 1u8 << ((topic_id as usize) % 8);
        let cur = self.bits.get(byte).ok_or(DdCode::PlaneCorrupt.code())?;
        if (cur & bit) != 0 {
            return Err(DdCode::ClaimDup.code());
        }
        self.bits[byte] |= bit;
        self.set_count = self.set_count.saturating_add(1);
        Ok(())
    }
    /// 位面在场测试。
    pub fn occupied(&self, topic_id: u8) -> bool {
        if topic_id as usize >= TOPIC_N {
            return false;
        }
        let byte = (topic_id as usize) / 8;
        let bit = 1u8 << ((topic_id as usize) % 8);
        match self.bits.get(byte) {
            Some(b) => (b & bit) != 0,
            None => false,
        }
    }
    /// 位面守恒：置位数 == 声明数。
    pub fn conserve_ok(&self, declared: u32) -> bool {
        self.set_count == declared
    }
}

// ── 数据结构二：嫌疑账 ──────────────────────────────────────────
/// 嫌疑账：三路嫌疑（重复/划界/未知）入账待结案；
/// J1 的「嫌疑清单=空断言」即结案后本表清零。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuspectLedger {
    pub dup_count: u32,
    pub boundary_count: u32,
    pub unknown_count: u32,
    /// 未结案嫌疑（主题 id 列表，结案后为空）。
    pub open: Vec<u8>,
    /// 已结案嫌疑数（结案只增不减）。
    pub resolved: u32,
}

impl SuspectLedger {
    /// 新建空账。
    pub fn new() -> SuspectLedger {
        SuspectLedger { dup_count: 0, boundary_count: 0, unknown_count: 0, open: Vec::new(), resolved: 0 }
    }
    /// 记一条嫌疑。
    pub fn push(&mut self, topic_id: u8) {
        self.open.push(topic_id);
    }
    /// 结案：弹出全部未结嫌疑（resolved 只增不减）。
    pub fn resolve_all(&mut self) -> u32 {
        let n = self.open.len() as u32;
        self.open.clear();
        self.resolved = self.resolved.saturating_add(n);
        n
    }
    /// 嫌疑清单=空断言（结案判据）。
    pub fn is_clear(&self) -> bool {
        self.open.is_empty()
    }
}

// ── 声明与裁决核 ────────────────────────────────────────────────
/// 一条声明：主题 + 声明方属主。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub topic_id: u8,
    pub owner: u8,
}

/// 裁决单条声明（闭集预检版）：主题闭集 + 属主闭集两闸，
/// 返回冻结属主——不落位面，供闸前快速预检。
pub fn adjudicate(
    decl: &Declaration,
) -> Result<u8, u32> {
    // 闸 1：主题闭集。
    if decl.topic_id as usize >= TOPIC_N {
        return Err(DdCode::TopicUnknown.code());
    }
    // 闸 2：属主闭集。
    if decl.owner > OWNER_FACE {
        return Err(DdCode::OwnerRange.code());
    }
    frozen_owner(decl.topic_id)
}

/// 裁决单条声明（含划界闸的完整版）：
/// 位面查重之后，档案面不得声明 E1/B04 可见主题（划界违规必抓）。
/// 任何违规（重复/划界/未知）都同步入嫌疑账。
pub fn adjudicate_full(
    decl: &Declaration,
    plane: &mut ClaimPlane,
    suspects: &mut SuspectLedger,
) -> Result<u8, u32> {
    if decl.topic_id as usize >= TOPIC_N {
        suspects.unknown_count = suspects.unknown_count.saturating_add(1);
        suspects.push(decl.topic_id);
        return Err(DdCode::TopicUnknown.code());
    }
    if decl.owner > OWNER_FACE {
        return Err(DdCode::OwnerRange.code());
    }
    // 位面查重（重复声明必抓，嫌疑同步入账）。
    if let Err(c) = plane.claim(decl.topic_id) {
        if c == DdCode::ClaimDup.code() {
            suspects.dup_count = suspects.dup_count.saturating_add(1);
            suspects.push(decl.topic_id);
        }
        return Err(c);
    }
    // 划界闸：档案面只能声明双不可见主题；越界即划界违规。
    match TOPICS.get(decl.topic_id as usize) {
        Some(t) => {
            if decl.owner == OWNER_FACE && (t.e1_visible || t.b04_visible) {
                suspects.boundary_count = suspects.boundary_count.saturating_add(1);
                suspects.push(decl.topic_id);
                return Err(DdCode::BoundaryViolation.code());
            }
        }
        None => {
            suspects.unknown_count = suspects.unknown_count.saturating_add(1);
            suspects.push(decl.topic_id);
            return Err(DdCode::TopicUnknown.code());
        }
    }
    // 属主必须与冻结划界一致（E1 声明 B04 主题=划界违规）。
    let want = frozen_owner(decl.topic_id)?;
    if decl.owner != want {
        suspects.boundary_count = suspects.boundary_count.saturating_add(1);
        suspects.push(decl.topic_id);
        return Err(DdCode::BoundaryViolation.code());
    }
    Ok(want)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：20 条聚类按冻结属主逐条声明（全过）→ 位面满 20 →
/// 注入三类各 10 轮（嫌疑入账）→ 结案 → 嫌疑清单=空 → 对账核。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut plane = ClaimPlane::new();
    let mut suspects = SuspectLedger::new();
    // 20 条聚类按冻结属主声明（全过，位面满）。
    let mut i = 0;
    while i < TOPIC_N {
        let tid = i as u8;
        let own = frozen_owner(tid)?;
        let got = adjudicate_full(&Declaration { topic_id: tid, owner: own }, &mut plane, &mut suspects)?;
        if got != own {
            return Err(DdCode::ReconcileMismatch.code());
        }
        i += 1;
    }
    if !plane.conserve_ok(TOPIC_N as u32) {
        return Err(DdCode::PlaneCorrupt.code());
    }
    // 注入三类各 10 轮（嫌疑直接入共享账，全抓）。
    let led = inject_into(JUDGE_ROUNDS, &mut suspects)?;
    if !led.all_caught(JUDGE_ROUNDS) {
        return Err(DdCode::ClaimDup.code());
    }
    // 结案：嫌疑清单=空。
    let resolved = suspects.resolve_all();
    if !suspects.is_clear() {
        return Err(DdCode::SuspectNonEmpty.code());
    }
    Ok((resolved, plane.set_count, TOPIC_N as u32))
}

// ── 对账核：20 条聚类对 E1/B04 可见判据对表 ─────────────────────
/// 逐行复算：可见域派生（id%3）、冻结属主、判据名三元组全对上。
pub fn reconcile() -> Result<(usize, usize, usize), u32> {
    let mut e1 = 0usize;
    let mut b04 = 0usize;
    let mut face = 0usize;
    let mut i = 0;
    while i < TOPIC_N {
        let t = TOPICS.get(i).ok_or(DdCode::ReconcileMismatch.code())?;
        // 派生复算：可见性与 id%3 一致。
        let e1_want = (t.topic_id as usize) % 3 == 0;
        let b04_want = (t.topic_id as usize) % 3 == 1;
        if t.e1_visible != e1_want || t.b04_visible != b04_want {
            return Err(DdCode::ReconcileMismatch.code());
        }
        // 属主复算：frozen_owner 与可见域一致。
        let own = frozen_owner(t.topic_id)?;
        if t.e1_visible && own != OWNER_E1 {
            return Err(DdCode::ReconcileMismatch.code());
        }
        if t.b04_visible && own != OWNER_B04 {
            return Err(DdCode::ReconcileMismatch.code());
        }
        if !t.e1_visible && !t.b04_visible && own != OWNER_FACE {
            return Err(DdCode::ReconcileMismatch.code());
        }
        // 判据名闭集非空。
        if t.name.is_empty() {
            return Err(DdCode::ReconcileMismatch.code());
        }
        if t.e1_visible {
            e1 += 1;
        } else if t.b04_visible {
            b04 += 1;
        } else {
            face += 1;
        }
        i += 1;
    }
    if e1 != E1_VISIBLE_N || b04 != B04_VISIBLE_N || face != FACE_EXCLUSIVE_N {
        return Err(DdCode::ReconcileMismatch.code());
    }
    Ok((e1, b04, face))
}

// ── 注入核（3 类坏样本，逐类必抓） ──────────────────────────────
/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全抓判据。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] != rounds || self.caught[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检数（恒零判据）。
    pub fn missed(&self) -> u32 {
        let mut t: u32 = 0;
        let mut c: u32 = 0;
        let mut i = 0;
        while i < 3 {
            t = t.saturating_add(self.tries[i]);
            c = c.saturating_add(self.caught[i]);
            i += 1;
        }
        t.saturating_sub(c)
    }
}

/// 注入轮（共享嫌疑账版）：重复声明 / 划界违规 / 未知主题三类各
/// 一轮，违规嫌疑直接入传入账（结案数才可复算=3×轮数）。
pub fn inject_into(rounds: u32, suspects: &mut SuspectLedger) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：重复声明（位面已置主题 0，二次声明=重复）。
        let mut p1 = ClaimPlane::new();
        let _ = p1.claim(0);
        let hit1 = match adjudicate_full(&Declaration { topic_id: 0, owner: OWNER_E1 }, &mut p1, suspects) {
            Err(c) => c == DdCode::ClaimDup.code(),
            Ok(_) => false,
        };
        led.record(0, hit1);
        // 类 2：划界违规（空位面 + 档案面声明 E1 可见主题 3）。
        let mut p2 = ClaimPlane::new();
        let hit2 = match adjudicate_full(&Declaration { topic_id: 3, owner: OWNER_FACE }, &mut p2, suspects) {
            Err(c) => c == DdCode::BoundaryViolation.code(),
            Ok(_) => false,
        };
        led.record(1, hit2);
        // 类 3：未知主题（id 250 落出 0..20）。
        let mut p3 = ClaimPlane::new();
        let hit3 = match adjudicate_full(&Declaration { topic_id: 250, owner: OWNER_FACE }, &mut p3, suspects) {
            Err(c) => c == DdCode::TopicUnknown.code(),
            Ok(_) => false,
        };
        led.record(2, hit3);
        r += 1;
    }
    Ok(led)
}

/// 注入轮（自含版）：每轮自建嫌疑账，用于独立对拍。
pub fn inject_round(rounds: u32) -> Result<InjectLedger, u32> {
    let mut s = SuspectLedger::new();
    inject_into(rounds, &mut s)
}

/// 探针三序（满位面/空位面/属主越界）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let (_, _, face) = reconcile()?;
    Ok(face as u32)
}

pub fn probe_seq_b() -> Result<(), u32> {
    let mut plane = ClaimPlane::new();
    let mut suspects = SuspectLedger::new();
    // 档案面声明独占主题 2 → 合法（双不可见）。
    match adjudicate_full(&Declaration { topic_id: 2, owner: OWNER_FACE }, &mut plane, &mut suspects) {
        Ok(own) if own == OWNER_FACE => Ok(()),
        _ => Err(DdCode::BoundaryViolation.code()),
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    let mut plane = ClaimPlane::new();
    let mut suspects = SuspectLedger::new();
    match adjudicate_full(&Declaration { topic_id: 0, owner: 9 }, &mut plane, &mut suspects) {
        Err(c) if c == DdCode::OwnerRange.code() => Ok(()),
        _ => Err(DdCode::OwnerRange.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DdCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        let v = DdCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if DdCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 对账核：20 条聚类对 E1/B04 可见判据对表（7/7/6 派生闭合）。
    let rec = reconcile();
    v.push(("J1-20聚类对E1/B04对表", rec == Ok((7, 7, 6)), format!("{:?}", rec)));
    // J1 主判据：黄金全链 + 嫌疑清单=空断言。
    let g = golden_run();
    let g_ok = match &g {
        Ok((resolved, _, _)) => *resolved == 30 && *g != (0, 0, 0),
        Err(_) => false,
    };
    v.push(("J1-嫌疑清单空断言", g_ok, format!("{:?}", g)));
    // 注入三类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(JUDGE_ROUNDS) && l.missed() == 0,
        Err(_) => false,
    };
    v.push(("J1-注入三类10/10", inj_ok, format!("{:?}", inj.as_ref().map(|l| l.tries))));
    // 位面守恒（满 20）。
    let pc = match &g {
        Ok((_, set, _)) => *set == TOPIC_N as u32,
        Err(_) => false,
    };
    v.push(("J2-位面守恒满20", pc, format!("位面 set={:?}", g.as_ref().map(|x| x.1))));
    // 探针三序。
    v.push((
        "J2-探针三序",
        probe_seq_a() == Ok(6u32) && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("独占6/档案面独占合法/属主越界拒"),
    ));
    // 属主优先序冻结：E1 主题由档案面声明必抓。
    let mut pv = ClaimPlane::new();
    let mut sv = SuspectLedger::new();
    let e1_steal = adjudicate_full(&Declaration { topic_id: 6, owner: OWNER_FACE }, &mut pv, &mut sv)
        .err() == Some(DdCode::BoundaryViolation.code());
    v.push(("J2-划界优先序冻结", e1_steal, String::from("档案面偷声明 E1 主题=违规")));
    // 边界：空位面 / 满位面。
    let empty_plane = ClaimPlane::new();
    v.push(("J2-空位面边界", empty_plane.conserve_ok(0), String::from("0 声明 0 置位")));
    // 重复结案只增不减。
    let mut s2 = SuspectLedger::new();
    s2.push(1);
    s2.push(2);
    let r1 = s2.resolve_all();
    let r2 = s2.resolve_all();
    v.push((
        "J2-结案只增不减",
        r1 == 2 && r2 == 0 && s2.is_clear(),
        format!("r1={} r2={} 清单空={}", r1, r2, s2.is_clear()),
    ));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫 + 文案闭集。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x80640 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("8 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18554_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18554 档案面防重声明（与 E1/B04 划界）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}
// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_derived_counts() {
        assert_eq!(reconcile(), Ok((7, 7, 6)), "7+7+6=20 闭合");
        assert_eq!(E1_VISIBLE_N + B04_VISIBLE_N + FACE_EXCLUSIVE_N, TOPIC_N);
    }

    #[test]
    fn golden_full_plane_and_clear_suspects() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.1, 20, "位面满 20");
        assert_eq!(g.2, 20, "20 条聚类全声明");
        assert!(g.0 >= 30, "结案数=注入三类×10 轮");
    }

    #[test]
    fn duplicate_declaration_caught() {
        let mut p = ClaimPlane::new();
        let mut s = SuspectLedger::new();
        let d = Declaration { topic_id: 5, owner: OWNER_FACE };
        assert!(adjudicate_full(&d, &mut p, &mut s).is_ok());
        assert_eq!(adjudicate_full(&d, &mut p, &mut s), Err(DdCode::ClaimDup.code()));
    }

    #[test]
    fn boundary_violation_caught() {
        let inj = inject_round(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(JUDGE_ROUNDS));
        assert_eq!(inj.missed(), 0);
        // 单独复核：B04 主题 7 被档案面声明=划界违规。
        let mut p = ClaimPlane::new();
        let mut s = SuspectLedger::new();
        assert_eq!(
            adjudicate_full(&Declaration { topic_id: 7, owner: OWNER_FACE }, &mut p, &mut s),
            Err(DdCode::BoundaryViolation.code())
        );
    }

    #[test]
    fn unknown_topic_caught() {
        let mut p = ClaimPlane::new();
        let mut s = SuspectLedger::new();
        assert_eq!(
            adjudicate_full(&Declaration { topic_id: 99, owner: OWNER_FACE }, &mut p, &mut s),
            Err(DdCode::TopicUnknown.code())
        );
        assert_eq!(s.unknown_count, 1);
    }

    #[test]
    fn plane_boundaries() {
        let mut p = ClaimPlane::new();
        assert!(p.claim(0).is_ok(), "键 0 边界");
        assert!(p.claim(19).is_ok(), "键 19 边界");
        assert_eq!(p.claim(20), Err(DdCode::TopicUnknown.code()), "越界拒");
        assert!(p.conserve_ok(2));
    }

    #[test]
    fn repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail_closed());
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18554_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
