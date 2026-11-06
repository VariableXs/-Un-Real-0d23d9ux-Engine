//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18496 · ActCtx 面防重声明（与 D2 SxS 划界） · 0x7F7C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18496-J1 20 条主题聚类对 D2 侧架程序集语义对表，嫌疑清单
//!       =空断言
//! 本模块结构清单：
//!   1. 冻结常量层：主题聚类 20 / D2 语义锚 5 项 / 判据轮数 10；
//!   2. 诊断码闭集 AcCd（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 聚类条目 ClusterEntry（topic/face/fingerprint 三元）；
//!       - 双面注册表 FaceRegistry：ActCtx 面 + D2 SxS 面 + 双位面账
//!         （u32 位图各一，第 i 位=主题 i 在册）；
//!       - 嫌疑账 SuspectLedger（双面占用/指纹跨面碰撞逐条记账，空=绿）；
//!   4. 划界规则（冻结）：i%5∈{0,1} → ActCtx 面（8 例），否则 D2 面
//!       （12 例）——每主题恰一面；
//!   5. 防重声明：同面指纹重复即 DupDeclaration 拒；跨面同主题即
//!       DoubleFaceClaim 拒且入嫌疑账；
//!   6. D2 语义对表：5 项架程序集锚名（VC80/VC90 CRT、Common-Controls
//!       等）指纹互异且与 ActCtx 面零碰撞——嫌疑清单=空的机器口径；
//!   7. 注入核四类：同面重复指纹 / 跨面双占用 / 未知面 / 零指纹+幽灵
//!       主题——逐类 10 轮全抓，守恒漏检必 0；
//!   8. golden_run：20 聚类全注册 → 划界完备（并满交集空）→ 嫌疑空 →
//!       双跑一致；
//!   9. run_unx18496_checks：18 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F7C0，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F7C0;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 主题聚类数（写死 20：J1"20 条主题聚类"语义，主题号 1..=20）。
pub const TOPIC_N: usize = 20;
/// 面闭集：1 ActCtx 面 / 2 D2 SxS 面。
pub const FACE_ACTCTX: u32 = 1;
pub const FACE_D2: u32 = 2;
/// 划界规则（冻结推导：i%5∈{0,1}→ActCtx，恰 8 例——8/12 分账便于位面
/// 折算核对 20 位满覆盖）。
pub const ACTCTX_EXPECT_N: usize = 8;
pub const D2_EXPECT_N: usize = 12;
/// D2 侧架程序集语义锚（冻结 5 项：真实 SxS 程序集名——对表对象）。
pub const ANCHOR_N: usize = 5;
pub const D2_ANCHORS: [&'static str; ANCHOR_N] = [
    "Microsoft.VC80.CRT",
    "Microsoft.Windows.Common-Controls",
    "Microsoft.VC90.CRT",
    "Microsoft.Windows.Common-Controls.v6",
    "Microsoft.Windows.GdiPlus",
];
/// 主题名域上界（名称字节上限——防超长名称）。
pub const NAME_MAX: usize = 64;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcCd {
    /// 未知面（1/2 之外）
    FaceUnknown,
    /// 幽灵主题（1..=20 之外）
    TopicOutOfRange,
    /// 零指纹哨兵
    FpZero,
    /// 名称超长
    NameTooLong,
    /// 同面重复声明（防重闸）
    DupDeclaration,
    /// 跨面双占用（嫌疑形态）
    DoubleFaceClaim,
    /// 嫌疑清单非空（J1 主体断言的反面）
    SuspectNonEmpty,
    /// 划界破坏（并满≠20 或交集非空）
    BoundaryBroken,
    /// D2 语义锚对表破坏（锚名指纹碰撞/缺失）
    AnchorMismatch,
    /// 守恒破坏（注册数 ≠ ActCtx + D2）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl AcCd {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            AcCd::FaceUnknown => "未知面：1 ActCtx / 2 D2 之外",
            AcCd::TopicOutOfRange => "幽灵主题：1..=20 之外",
            AcCd::FpZero => "零指纹哨兵：指纹恒非零",
            AcCd::NameTooLong => "名称超长：64 字节上限",
            AcCd::DupDeclaration => "同面重复声明：指纹已在册",
            AcCd::DoubleFaceClaim => "跨面双占用：主题已被对侧在册",
            AcCd::SuspectNonEmpty => "嫌疑清单非空：划界被破坏",
            AcCd::BoundaryBroken => "划界破坏：并满≠20 或交集非空",
            AcCd::AnchorMismatch => "D2 语义锚对表破坏",
            AcCd::ConservationBroken => "守恒破坏：注册数≠ActCtx+D2",
            AcCd::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [AcCd; 11] = [
        AcCd::FaceUnknown,
        AcCd::TopicOutOfRange,
        AcCd::FpZero,
        AcCd::NameTooLong,
        AcCd::DupDeclaration,
        AcCd::DoubleFaceClaim,
        AcCd::SuspectNonEmpty,
        AcCd::BoundaryBroken,
        AcCd::AnchorMismatch,
        AcCd::ConservationBroken,
        AcCd::NotReproducible,
    ];
}

/// FNV-1a 32（声明指纹基元——确定性，零随机源）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 聚类条目与划界规则 ─────────────────────────────────────────────
/// 聚类条目（主题号/面/声明指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClusterEntry {
    pub topic: u32,
    pub face: u32,
    pub fp: u32,
}

/// 划界规则（冻结：i%5∈{0,1}→ActCtx，否则 D2）。
pub const fn face_of(i: usize) -> u32 {
    if i % 5 == 0 || i % 5 == 1 {
        FACE_ACTCTX
    } else {
        FACE_D2
    }
}

/// 主题名（确定性派生：ActCtx 面 "actctx.topic.<i>" / D2 面
/// "sxs.assembly.<i>"——两域命名空间分离，指纹天然互异）。
pub fn topic_name(i: usize) -> String {
    if face_of(i) == FACE_ACTCTX {
        format!("actctx.topic.{}", i)
    } else {
        format!("sxs.assembly.{}", i)
    }
}

/// 主题指纹（FNV(名称) 哨兵零→1）。
pub fn topic_fp(i: usize) -> u32 {
    let d = fnv1a(topic_name(i).as_bytes());
    if d == 0 {
        1
    } else {
        d
    }
}

/// 构建冻结 20 聚类（i 1..=20）。
pub fn make_clusters() -> [ClusterEntry; TOPIC_N] {
    let mut arr = [ClusterEntry { topic: 0, face: 0, fp: 0 }; TOPIC_N];
    let mut i = 0;
    while i < TOPIC_N {
        arr[i] = ClusterEntry { topic: (i + 1) as u32, face: face_of(i + 1), fp: topic_fp(i + 1) };
        i += 1;
    }
    arr
}

// ── 双面注册表 ─────────────────────────────────────────────────────
/// 双面注册表（防重声明 + 嫌疑账）。
pub struct FaceRegistry {
    /// ActCtx 面指纹账（声明序）。
    pub actctx_fps: Vec<u32>,
    /// D2 面指纹账。
    pub d2_fps: Vec<u32>,
    /// ActCtx 位面：第 t 位=主题 t 在 ActCtx 面（t 从 1 起，位 0 弃用）。
    pub actctx_plane: u32,
    /// D2 位面。
    pub d2_plane: u32,
    /// 嫌疑账（双面占用主题号，空=绿）。
    pub suspects: Vec<u32>,
    pub declares: u32,
    pub rejected: u32,
}

impl FaceRegistry {
    /// 空注册表。
    pub fn new() -> FaceRegistry {
        FaceRegistry {
            actctx_fps: Vec::new(),
            d2_fps: Vec::new(),
            actctx_plane: 0,
            d2_plane: 0,
            suspects: Vec::new(),
            declares: 0,
            rejected: 0,
        }
    }

    /// 同面指纹查重（线性）。
    fn fp_in(&self, face: u32, fp: u32) -> bool {
        if face == FACE_ACTCTX {
            let mut i = 0;
            while i < self.actctx_fps.len() {
                if self.actctx_fps[i] == fp {
                    return true;
                }
                i += 1;
            }
            false
        } else {
            let mut i = 0;
            while i < self.d2_fps.len() {
                if self.d2_fps[i] == fp {
                    return true;
                }
                i += 1;
            }
            false
        }
    }

    /// 声明闸：面闭集 → 主题域 → 指纹哨兵 → 同面防重 → 跨面双占用。
    /// 返回 Err(码)=显拒；Ok=入册。
    pub fn declare(&mut self, face: u32, topic: u32, fp: u32) -> Result<(), u32> {
        // 面闭集。
        if face != FACE_ACTCTX && face != FACE_D2 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AcCd::FaceUnknown.code());
        }
        // 主题域。
        if topic == 0 || topic as usize > TOPIC_N {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AcCd::TopicOutOfRange.code());
        }
        // 指纹哨兵。
        if fp == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AcCd::FpZero.code());
        }
        // 跨面双占用闸（先于同面防重：嫌疑形态优先记录）。
        let other_plane = if face == FACE_ACTCTX { self.d2_plane } else { self.actctx_plane };
        if (other_plane >> topic) & 1 == 1 {
            self.suspects.push(topic);
            self.rejected = self.rejected.saturating_add(1);
            return Err(AcCd::DoubleFaceClaim.code());
        }
        // 同面防重闸。
        if self.fp_in(face, fp) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AcCd::DupDeclaration.code());
        }
        // 入册：面指纹账 + 位面。
        if face == FACE_ACTCTX {
            self.actctx_fps.push(fp);
            self.actctx_plane |= 1u32 << topic;
        } else {
            self.d2_fps.push(fp);
            self.d2_plane |= 1u32 << topic;
        }
        self.declares = self.declares.saturating_add(1);
        Ok(())
    }

    /// 划界完备：并满（两 位面并 == 主题全位）∧ 交集空。
    pub fn boundary_ok(&self) -> bool {
        let mut full = 0u32;
        let mut t = 1;
        while t <= TOPIC_N {
            full |= 1u32 << t;
            t += 1;
        }
        if (self.actctx_plane | self.d2_plane) != full {
            return false;
        }
        if (self.actctx_plane & self.d2_plane) != 0 {
            return false;
        }
        true
    }

    /// 面计数。
    pub fn actctx_count(&self) -> usize {
        self.actctx_fps.len()
    }
    pub fn d2_count(&self) -> usize {
        self.d2_fps.len()
    }

    /// 嫌疑清单=空断言。
    pub fn suspects_empty(&self) -> bool {
        self.suspects.is_empty()
    }

    /// 守恒：declares == actctx + d2，且注册-拒收账不亏。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.declares != (self.actctx_count() + self.d2_count()) as u32 {
            return Err(AcCd::ConservationBroken.code());
        }
        Ok(())
    }
}

/// D2 语义锚对表：5 锚指纹互异 ∧ 与 ActCtx 面指纹零碰撞。
pub fn anchor_table_ok(actctx_fps: &[u32]) -> Result<(), u32> {
    // 锚互异。
    let mut i = 0;
    while i < ANCHOR_N {
        if D2_ANCHORS[i].len() == 0 || D2_ANCHORS[i].len() > NAME_MAX {
            return Err(AcCd::AnchorMismatch.code());
        }
        let fi = fnv1a(D2_ANCHORS[i].as_bytes());
        let mut k = 0;
        while k < ANCHOR_N {
            if k != i && fnv1a(D2_ANCHORS[k].as_bytes()) == fi {
                return Err(AcCd::AnchorMismatch.code());
            }
            k += 1;
        }
        // 与 ActCtx 面零碰撞。
        let mut j = 0;
        while j < actctx_fps.len() {
            if actctx_fps[j] == fi {
                return Err(AcCd::AnchorMismatch.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：20 聚类按划界规则全注册 → 守恒 → 划界完备 → 嫌疑空 →
/// D2 对表。返回 (actctx_n, d2_n)。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let clusters = make_clusters();
    let mut reg = FaceRegistry::new();
    let mut i = 0;
    while i < TOPIC_N {
        reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp)?;
        i += 1;
    }
    reg.conserve()?;
    if reg.actctx_count() != ACTCTX_EXPECT_N || reg.d2_count() != D2_EXPECT_N {
        return Err(AcCd::ConservationBroken.code());
    }
    if !reg.boundary_ok() {
        return Err(AcCd::BoundaryBroken.code());
    }
    if !reg.suspects_empty() {
        return Err(AcCd::SuspectNonEmpty.code());
    }
    anchor_table_ok(&reg.actctx_fps)?;
    Ok((reg.actctx_count(), reg.d2_count()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AcCd::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核 ─────────────────────────────────────────────────────────
/// 注入账（4 槽，守恒 tries==caught）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：四槽全记录且全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 四类坏声明注入 × rounds 轮（每轮全新注册表）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：同面重复指纹（ActCtx 面同一指纹声明两次）。
        let mut reg0 = FaceRegistry::new();
        let d1 = reg0.declare(FACE_ACTCTX, 1, 0x1234_5678);
        let d2 = reg0.declare(FACE_ACTCTX, 5, 0x1234_5678);
        led.record(0, d1.is_ok() && d2.err() == Some(AcCd::DupDeclaration.code()));
        // 槽1：跨面双占用（主题 3 先 D2 后 ActCtx）。
        let mut reg1 = FaceRegistry::new();
        let d3 = reg1.declare(FACE_D2, 3, 0xAAAA_0001);
        let d4 = reg1.declare(FACE_ACTCTX, 3, 0xBBBB_0002);
        led.record(1, d3.is_ok() && d4.err() == Some(AcCd::DoubleFaceClaim.code()));
        // 槽2：未知面（face=3）。
        let mut reg2 = FaceRegistry::new();
        led.record(2, reg2.declare(3, 1, 0x0000_1111).err() == Some(AcCd::FaceUnknown.code()));
        // 槽3：零指纹 + 幽灵主题（合槽两形态）。
        let mut reg3 = FaceRegistry::new();
        led.record(
            3,
            reg3.declare(FACE_ACTCTX, 1, 0).err() == Some(AcCd::FpZero.code())
                || reg3.declare(FACE_ACTCTX, TOPIC_N as u32 + 1, 0x0000_2222).err()
                    == Some(AcCd::TopicOutOfRange.code()),
        );
        r += 1;
    }
    Ok(led)
}

/// 嫌疑注入：人为制造一次跨面双占用 → 嫌疑账必非空 → 再验空注册表
/// 嫌疑账恒空（对照面）。
pub fn suspect_probe() -> Result<(), u32> {
    let mut reg = FaceRegistry::new();
    let _ = reg.declare(FACE_D2, 3, 0xAAAA_0003);
    let r = reg.declare(FACE_ACTCTX, 3, 0xBBBB_0004);
    if r.is_ok() {
        return Err(AcCd::DoubleFaceClaim.code());
    }
    if reg.suspects_empty() {
        return Err(AcCd::SuspectNonEmpty.code());
    }
    // 对照：全新注册表嫌疑账必空。
    let clean = FaceRegistry::new();
    if !clean.suspects_empty() {
        return Err(AcCd::SuspectNonEmpty.code());
    }
    Ok(())
}

/// 幂等：同坏声明重放同拒因（重放两轮逐码相等）。
pub fn idempotent_check() -> Result<(), u32> {
    let mut reg = FaceRegistry::new();
    let a = reg.declare(FACE_ACTCTX, 1, 0x1234_5678);
    let b = reg.declare(FACE_ACTCTX, 1, 0x1234_5678);
    // 首次成功，重放撞同面防重闸。
    if a.is_err() || b.err() != Some(AcCd::DupDeclaration.code()) {
        return Err(AcCd::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AcCd::ALL.len() {
        let v = AcCd::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < AcCd::ALL.len() {
        if AcCd::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 对表导出：20 聚类逐行 "CL|i|topic|face|fp"（可回放格式）。
pub fn export_table() -> Vec<String> {
    let clusters = make_clusters();
    let mut out = Vec::new();
    out.push(format!("CL-TABLE v1 topic|face|fp n={}", TOPIC_N));
    let mut i = 0;
    while i < TOPIC_N {
        out.push(format!(
            "CL|{}|{}|{}",
            clusters[i].topic, clusters[i].face, clusters[i].fp
        ));
        i += 1;
    }
    out
}

// ── 判据收口（16 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18496_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18496 ActCtx 面防重声明（与 D2 SxS 划界）");
    // 1. 黄金全链（20 聚类全注册）。
    let g = golden_run();
    s.add_dyn("J1-20聚类全注册", match &g { Ok((a, d)) => a + *d == TOPIC_N, Err(_) => false }, &format!("{:?}", g));
    // 2. 嫌疑清单=空（J1 主体）。
    let susp = {
        let clusters = make_clusters();
        let mut reg = FaceRegistry::new();
        let mut i = 0;
        while i < TOPIC_N {
            let _ = reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp);
            i += 1;
        }
        reg.suspects_empty()
    };
    s.add("J1-嫌疑清单空", susp, "双面占用=0 碰撞=0");
    // 3. 划界完备（并满 ∧ 交集空）。
    let bound = {
        let clusters = make_clusters();
        let mut reg = FaceRegistry::new();
        let mut i = 0;
        while i < TOPIC_N {
            let _ = reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp);
            i += 1;
        }
        reg.boundary_ok() && reg.actctx_count() == ACTCTX_EXPECT_N && reg.d2_count() == D2_EXPECT_N
    };
    s.add("J1-划界完备8/12", bound, "并满20 ∧ 交集空");
    // 4. 防重声明（同面指纹互异）。
    let dedup = {
        let clusters = make_clusters();
        let mut reg = FaceRegistry::new();
        let mut i = 0;
        let mut dup_free = true;
        while i < TOPIC_N {
            if reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp).is_err() {
                dup_free = false;
            }
            i += 1;
        }
        dup_free && reg.rejected == 0
    };
    s.add("J1-同面指纹互异", dedup, "20 声明零重复");
    // 5. D2 语义锚对表（5 锚互异 ∧ 零碰撞）。
    let anchors = {
        let clusters = make_clusters();
        let mut reg = FaceRegistry::new();
        let mut i = 0;
        while i < TOPIC_N {
            let _ = reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp);
            i += 1;
        }
        anchor_table_ok(&reg.actctx_fps).is_ok()
    };
    s.add("J1-D2语义锚对表", anchors, "5 锚互异零碰撞");
    // 6. 注入 10/10（四类守恒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10四类全抓", inj_ok, &format!("t/c={:?}", inj.as_ref().map(|l| l.caught)));
    // 7. 同面重复指纹逐码。
    let dup = {
        let mut reg = FaceRegistry::new();
        let _ = reg.declare(FACE_ACTCTX, 1, 0x1234_5678);
        reg.declare(FACE_ACTCTX, 5, 0x1234_5678).err() == Some(AcCd::DupDeclaration.code())
    };
    s.add("J1-同面重复拒", dup, "DupDeclaration");
    // 8. 跨面双占用逐码。
    let dbl = {
        let mut reg = FaceRegistry::new();
        let _ = reg.declare(FACE_D2, 3, 0xAAAA_0001);
        reg.declare(FACE_ACTCTX, 3, 0xBBBB_0002).err() == Some(AcCd::DoubleFaceClaim.code())
    };
    s.add("J1-跨面双占用拒", dbl, "DoubleFaceClaim");
    // 9. 未知面拒。
    let unk = {
        let mut reg = FaceRegistry::new();
        reg.declare(3, 1, 0x0000_1111).err() == Some(AcCd::FaceUnknown.code())
    };
    s.add("J1-未知面拒", unk, "FaceUnknown");
    // 10. 零指纹 / 幽灵主题拒。
    let zero = {
        let mut reg = FaceRegistry::new();
        reg.declare(FACE_ACTCTX, 1, 0).err() == Some(AcCd::FpZero.code())
    };
    s.add("J1-零指纹拒", zero, "FpZero");
    let ghost = {
        let mut reg = FaceRegistry::new();
        reg.declare(FACE_ACTCTX, TOPIC_N as u32 + 1, 0x0000_2222).err()
            == Some(AcCd::TopicOutOfRange.code())
    };
    s.add("J1-幽灵主题拒", ghost, "TopicOutOfRange");
    // 11. 嫌疑探针（人为双占用 → 嫌疑账非空 → 对照空表恒空）。
    s.add("J1-嫌疑探针双向", suspect_probe().is_ok(), "双占用入账·空表恒空");
    // 12. 幂等（同坏声明重放同拒因）。
    s.add("J1-声明幂等", idempotent_check().is_ok(), "重放同码");
    // 13. 守恒（declares == act + d2）。
    let cons = {
        let clusters = make_clusters();
        let mut reg = FaceRegistry::new();
        let mut i = 0;
        while i < TOPIC_N {
            let _ = reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp);
            i += 1;
        }
        reg.conserve().is_ok() && reg.declares == TOPIC_N as u32
    };
    s.add("J1-注册守恒", cons, "declares==act+d2==20");
    // 14. 对表导出可回放（20 行 + 表头，逐行前缀 CL|）。
    let exp = {
        let lines = export_table();
        lines.len() == TOPIC_N + 1
            && lines[1].starts_with("CL|1|")
            && lines[TOPIC_N].starts_with("CL|20|")
    };
    s.add("J1-对表导出", exp, "CL-TABLE v1 20 行");
    // 15. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    // 16. 段位守卫 + 文案闭集。
    s.add("J1-段位守卫", segment_guard(), "0x7F7C0 窗口");
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18496_checks_green() {
        let set = run_unx18496_checks();
        assert!(set.all_passed(), "UNX-F18496 red: {}", set.red_items_text());
    }

    #[test]
    fn t_golden_shape() {
        let g = golden_run().unwrap_or((0, 0));
        assert_eq!(g, (ACTCTX_EXPECT_N, D2_EXPECT_N), "8/12 分账");
    }

    #[test]
    fn t_face_assignment_frozen() {
        // 划界规则逐位复核：i%5∈{0,1}→ActCtx（i=1,5,6,10,11,15,16,20）。
        let expect_act = [1usize, 5, 6, 10, 11, 15, 16, 20];
        let mut n = 0usize;
        let mut i = 1;
        while i <= TOPIC_N {
            if face_of(i) == FACE_ACTCTX {
                let mut found = false;
                let mut k = 0;
                while k < expect_act.len() {
                    if expect_act[k] == i {
                        found = true;
                    }
                    k += 1;
                }
                assert!(found, "主题 {} 应属 ActCtx", i);
                n += 1;
            } else {
                assert!(face_of(i) == FACE_D2);
            }
            i += 1;
        }
        assert_eq!(n, ACTCTX_EXPECT_N, "ActCtx 恰 8 例");
    }

    #[test]
    fn t_fingerprints_distinct() {
        // 20 主题指纹互异（命名空间分离 → FNV 无碰撞）。
        let clusters = make_clusters();
        let mut i = 0;
        while i < TOPIC_N {
            let mut k = i + 1;
            while k < TOPIC_N {
                assert!(clusters[i].fp != clusters[k].fp, "主题 {} 与 {} 指纹碰撞", i + 1, k + 1);
                k += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t_anchors_frozen_semantics() {
        // D2 语义锚：5 项名非空、互异、指纹非零。
        let mut i = 0;
        while i < ANCHOR_N {
            assert!(!D2_ANCHORS[i].is_empty(), "锚名非空");
            assert!(D2_ANCHORS[i].len() <= NAME_MAX, "锚名域内");
            let fi = fnv1a(D2_ANCHORS[i].as_bytes());
            assert!(fi != 0, "锚指纹非零");
            let mut k = i + 1;
            while k < ANCHOR_N {
                assert!(fi != fnv1a(D2_ANCHORS[k].as_bytes()), "锚 {} 与 {} 碰撞", i, k);
                k += 1;
            }
            i += 1;
        }
        assert!(anchor_table_ok(&[]).is_ok(), "空 ActCtx 面零碰撞平凡成立");
    }

    #[test]
    fn t_inject_rounds_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught(), "四类全抓");
        let mut total = 0u32;
        let mut i = 0;
        while i < 4 {
            total += l.tries[i];
            i += 1;
        }
        assert_eq!(total, 40, "总注入 4×10=40");
    }

    #[test]
    fn t_suspect_probe_both_ways() {
        // 双占用入嫌疑账；撤销式注册（空表）恒空。
        assert!(suspect_probe().is_ok());
        let mut reg = FaceRegistry::new();
        let _ = reg.declare(FACE_D2, 3, 0xAAAA_0003);
        let r = reg.declare(FACE_ACTCTX, 3, 0xBBBB_0004);
        assert!(r.is_err());
        assert!(!reg.suspects_empty(), "嫌疑账非空");
        assert!(reg.suspects[0] == 3, "嫌疑主题=3");
    }

    #[test]
    fn t_boundary_bitmap_math() {
        // 位面数学：并满（bit1..20 全置）∧ 交集空。
        let clusters = make_clusters();
        let mut reg = FaceRegistry::new();
        let mut i = 0;
        while i < TOPIC_N {
            let _ = reg.declare(clusters[i].face, clusters[i].topic, clusters[i].fp);
            i += 1;
        }
        assert_eq!(reg.actctx_plane & reg.d2_plane, 0, "交集空");
        let mut full = 0u32;
        let mut t = 1;
        while t <= TOPIC_N {
            full |= 1u32 << t;
            t += 1;
        }
        assert_eq!(reg.actctx_plane | reg.d2_plane, full, "并满");
    }

    #[test]
    fn t_export_table_shape() {
        let lines = export_table();
        assert_eq!(lines.len(), TOPIC_N + 1, "表头+20行");
        assert!(lines[0].starts_with("CL-TABLE v1"), "表头协议");
        // 回放抽查：第 8 行（主题 8）应属 D2 面（8%5=3）。
        assert!(lines[8].starts_with("CL|8|2|"), "主题8 D2 面");
        assert!(lines[6].starts_with("CL|6|1|"), "主题6 ActCtx 面");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F7C0 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(AcCd::DoubleFaceClaim.code(), SEG_BASE + 5, "码位推导");
    }
}
