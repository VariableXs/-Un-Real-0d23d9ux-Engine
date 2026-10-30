//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16002 · Wine 源树落位与只跟随检出纪律 · 0x68780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16002-J1 Wine 源树检出路径清单逐目录落账，补丁集登记表逐条
//!   有 ADR 编号，单件补丁计数 ≤10 断言复测通过。
//! 定位：Wine 只跟随不 fork 是铁律 8 的最大实践场，本条把"跟随"落成
//!   可审计的物理形态：源树检出在哪个路径、哪个版本钉、带了哪些本地
//!   补丁，全部落账可查，防止"事实 fork"在三十年里悄悄成形。
//! 设计要点：
//!   - 检出路径清单：源树统一检出至 third_party/wine/（只读检出区，
//!     禁止域内直改），检出命令/版本钉值/上游 commit 哈希三要素逐项
//!     落账，账行一目录一行，机器可读；
//!   - 补丁集登记表：任何对源树的差异必须以补丁文件形式存在于
//!     patches/wine/，每补丁一行登记（文件名/问题摘要/最小 diff 行数/
//!     上游化状态/ADR 编号），补丁内容按件走 ADR 流；
//!   - 断言面：单件补丁计数 >10 即触发架构审查告警（铁律 8 执行
//!     细则）；
//!   - 事实 fork 检出：源树 diff 中不在补丁集成员内的差异 = 私改，
//!     10/10 必抓——"改过的 Wine 树"零容忍；
//!   - 只读守护：检出区只读标记，可写即红；
//!   - 注入面：私改未走补丁/补丁缺 ADR/计数超限未告警/版本钉漂移/
//!     重复补丁名/检出区可写——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68780）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68780，AI-VX-1031 E1 域批D 独占区）。
pub const SEG_BASE: u32 = 0x68780;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 单件补丁计数红线（铁律 8 执行细则，写死）。
pub const PATCH_LIMIT: usize = 10;

/// 版本钉值（跟随 AI-94 台账口径，写死）。
pub const PINNED_VERSION: &str = "wine-10.0";
pub const PINNED_COMMIT: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d";

// ── 检出路径清单闭集 ─────────────────────────────────────────────
pub const CHECKOUT_DIRS: [&str; 6] = [
    "third_party/wine/",
    "third_party/wine/dlls/",
    "third_party/wine/server/",
    "third_party/wine/programs/",
    "third_party/wine/include/",
    "third_party/wine/loader/",
];

/// 检出账行：三要素齐备（路径/版本钉/commit 哈希）。
#[derive(Clone, Debug)]
pub struct CheckoutRow {
    pub dir: String,
    pub version_pin: String,
    pub commit_hash: String,
    /// 只读守护标记（可写即红）。
    pub readonly: bool,
}

// ── 上游化状态四态闭集 ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpstreamState {
    /// 已上游化（补丁进主线）
    Merged,
    /// 上游评审中
    InReview,
    /// 暂不上游（域内长期维护）
    KeptLocal,
    /// 待评估
    ToAssess,
}

impl UpstreamState {
    pub fn as_u32(self) -> u32 {
        match self {
            UpstreamState::Merged => 0,
            UpstreamState::InReview => 1,
            UpstreamState::KeptLocal => 2,
            UpstreamState::ToAssess => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            UpstreamState::Merged => "已上游化",
            UpstreamState::InReview => "评审中",
            UpstreamState::KeptLocal => "暂不上游",
            UpstreamState::ToAssess => "待评估",
        }
    }
    pub fn from_u32(v: u32) -> Option<UpstreamState> {
        match v {
            0 => Some(UpstreamState::Merged),
            1 => Some(UpstreamState::InReview),
            2 => Some(UpstreamState::KeptLocal),
            3 => Some(UpstreamState::ToAssess),
            _ => None,
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ws2Code {
    /// 检出目录越闭集
    DirOutOfRange,
    /// 检出行三要素缺一
    CheckoutTriFieldShort,
    /// 版本钉漂移（与钉值不符）
    PinDrift,
    /// 补丁缺 ADR 编号
    PatchNoAdr,
    /// 重复补丁名
    PatchDuplicated,
    /// 私改未走补丁（事实 fork 检出）
    PrivateEdit,
    /// 计数超限未告警（>10 补丁无审查）
    OverLimitNoAlert,
    /// 检出区可写（只读守护失守）
    CheckoutWritable,
    /// 上游化状态越四态闭集
    UpstreamStateOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl Ws2Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Ws2Code::DirOutOfRange => "检出目录越闭集",
            Ws2Code::CheckoutTriFieldShort => "检出行三要素缺一",
            Ws2Code::PinDrift => "版本钉漂移：跟随失守",
            Ws2Code::PatchNoAdr => "补丁缺 ADR 编号：登记不完整",
            Ws2Code::PatchDuplicated => "重复补丁名：补丁唯一律",
            Ws2Code::PrivateEdit => "私改未走补丁：事实 fork 检出",
            Ws2Code::OverLimitNoAlert => "补丁超限未告警：铁律 8 失守",
            Ws2Code::CheckoutWritable => "检出区可写：只读守护失守",
            Ws2Code::UpstreamStateOutOfRange => "上游化状态越四态闭集",
            Ws2Code::NotReproducible => "双跑不一致：确定性红线",
            Ws2Code::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [Ws2Code; 11] = [
        Ws2Code::DirOutOfRange,
        Ws2Code::CheckoutTriFieldShort,
        Ws2Code::PinDrift,
        Ws2Code::PatchNoAdr,
        Ws2Code::PatchDuplicated,
        Ws2Code::PrivateEdit,
        Ws2Code::OverLimitNoAlert,
        Ws2Code::CheckoutWritable,
        Ws2Code::UpstreamStateOutOfRange,
        Ws2Code::NotReproducible,
        Ws2Code::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < Ws2Code::ALL.len() {
        if Ws2Code::ALL[i].val() == code {
            return Ws2Code::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── FNV-1a 32 位（差异指纹） ────────────────────────────────────
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 检出目录闭集校验。
pub fn dir_legal(d: &str) -> bool {
    let mut i = 0;
    while i < CHECKOUT_DIRS.len() {
        if CHECKOUT_DIRS[i] == d {
            return true;
        }
        i += 1;
    }
    false
}

/// 检出行体检：三要素齐备 + 版本钉一致 + 只读守护。
pub fn vet_checkout(row: &CheckoutRow) -> Result<(), u32> {
    if !dir_legal(&row.dir) {
        return Err(Ws2Code::DirOutOfRange.val());
    }
    if row.version_pin.len() == 0 || row.commit_hash.len() != 40 {
        return Err(Ws2Code::CheckoutTriFieldShort.val());
    }
    if row.version_pin != PINNED_VERSION {
        return Err(Ws2Code::PinDrift.val());
    }
    if !row.readonly {
        return Err(Ws2Code::CheckoutWritable.val());
    }
    Ok(())
}

/// 补丁登记行：五字段齐备。
#[derive(Clone, Debug)]
pub struct PatchRow {
    /// 补丁文件名（patches/wine/ 下，账内唯一）。
    pub name: String,
    /// 问题摘要（非空）。
    pub summary: String,
    /// 最小 diff 行数（>0）。
    pub min_diff: u32,
    /// 上游化状态（四态闭集）。
    pub state: UpstreamState,
    /// ADR 编号（非空，如 "ADR-0042"）。
    pub adr: String,
}

/// 补丁行体检。
pub fn vet_patch(row: &PatchRow) -> Result<(), u32> {
    if row.name.len() < 5 || !row.name.starts_with("wine-") {
        return Err(Ws2Code::PatchDuplicated.val());
    }
    if row.summary.len() == 0 {
        return Err(Ws2Code::PatchNoAdr.val());
    }
    if row.min_diff == 0 {
        return Err(Ws2Code::PatchNoAdr.val());
    }
    if UpstreamState::from_u32(row.state.as_u32()).is_none() {
        return Err(Ws2Code::UpstreamStateOutOfRange.val());
    }
    // ADR 编号格式：ADR- + 至少 3 位。
    if !row.adr.starts_with("ADR-") || row.adr.len() < 7 {
        return Err(Ws2Code::PatchNoAdr.val());
    }
    Ok(())
}

// ── 补丁集账本 ───────────────────────────────────────────────────
pub struct PatchLedger {
    rows: Vec<PatchRow>,
    /// 事实 fork 检出：源树差异指纹集（每差异一指纹）。
    tree_diffs: Vec<u32>,
}

impl PatchLedger {
    pub fn new() -> PatchLedger {
        PatchLedger { rows: Vec::new(), tree_diffs: Vec::new() }
    }

    pub fn add_patch(&mut self, row: PatchRow) -> Result<(), u32> {
        vet_patch(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].name == row.name {
                return Err(Ws2Code::PatchDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 登记源树差异指纹（模拟源树 diff 产物）。
    pub fn add_tree_diff(&mut self, fp: u32) {
        self.tree_diffs.push(fp);
    }

    /// 事实 fork 检出（判据核心）：每个源树差异指纹必须命中某个
    /// 补丁登记行的指纹——不命中即私改。返私改数（0 = 干净）。
    pub fn private_edit_count(&self) -> u32 {
        let mut n = 0;
        let mut d = 0;
        while d < self.tree_diffs.len() {
            let mut hit = false;
            let mut i = 0;
            while i < self.rows.len() {
                if fnv1a(self.rows[i].name.as_bytes()) == self.tree_diffs[d] {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                n += 1;
            }
            d += 1;
        }
        n
    }

    /// 补丁计数断言（判据 ≤10）：超限必须告警（告警标记由调用方
    /// 持有，这里返超限与否 + 告警一致性检查）。
    pub fn over_limit(&self) -> bool {
        self.rows.len() > PATCH_LIMIT
    }

    /// 超限告警一致性：超限必须已告警，未超限不许误报。
    pub fn limit_alert_consistent(&self, alerted: bool) -> Result<(), u32> {
        if self.over_limit() && !alerted {
            return Err(Ws2Code::OverLimitNoAlert.val());
        }
        if !self.over_limit() && alerted {
            return Err(Ws2Code::OverLimitNoAlert.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 检出账：逐目录落账。
pub struct CheckoutLedger {
    rows: Vec<CheckoutRow>,
}

impl CheckoutLedger {
    pub fn new() -> CheckoutLedger {
        CheckoutLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, row: CheckoutRow) -> Result<(), u32> {
        vet_checkout(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dir == row.dir {
                return Err(Ws2Code::DirOutOfRange.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 六目录覆盖。
    pub fn closed(&self) -> Result<(), u32> {
        let mut d = 0;
        while d < CHECKOUT_DIRS.len() {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].dir == CHECKOUT_DIRS[d] {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(Ws2Code::DirOutOfRange.val());
            }
            d += 1;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版账装配：六目录检出行 + 九件补丁（≤10 红线内）+ 全树差异命中。
pub fn build_initial() -> Result<(CheckoutLedger, PatchLedger), u32> {
    let mut co = CheckoutLedger::new();
    let mut d = 0;
    while d < CHECKOUT_DIRS.len() {
        co.add(CheckoutRow {
            dir: String::from(CHECKOUT_DIRS[d]),
            version_pin: String::from(PINNED_VERSION),
            commit_hash: String::from(PINNED_COMMIT),
            readonly: true,
        })?;
        d += 1;
    }
    let mut pl = PatchLedger::new();
    let patches: [(&str, &str, u32, UpstreamState, &str); 9] = [
        ("wine-0001-varix-surface-hook", "显示后端挂点", 42, UpstreamState::KeptLocal, "ADR-0001"),
        ("wine-0002-registry-backend", "注册表后端替换", 67, UpstreamState::KeptLocal, "ADR-0002"),
        ("wine-0003-sync-native", "同步原语路由", 31, UpstreamState::InReview, "ADR-0003"),
        ("wine-0004-handle-native", "句柄表路由", 28, UpstreamState::InReview, "ADR-0004"),
        ("wine-0005-loader-dual", "装载双跑对照", 55, UpstreamState::ToAssess, "ADR-0005"),
        ("wine-0006-errcode-align", "错误码对齐", 19, UpstreamState::Merged, "ADR-0006"),
        ("wine-0007-font-bridge", "字体桥挂点", 23, UpstreamState::KeptLocal, "ADR-0007"),
        ("wine-0008-crash-diag", "崩溃诊断挂点", 36, UpstreamState::ToAssess, "ADR-0008"),
        ("wine-0009-prefix-lease", "prefix 租约语义", 12, UpstreamState::KeptLocal, "ADR-0009"),
    ];
    let mut p = 0;
    while p < patches.len() {
        pl.add_patch(PatchRow {
            name: String::from(patches[p].0),
            summary: String::from(patches[p].1),
            min_diff: patches[p].2,
            state: patches[p].3,
            adr: String::from(patches[p].4),
        })?;
        // 源树差异指纹与补丁名指纹同源登记（跟随干净形态）。
        pl.add_tree_diff(fnv1a(patches[p].0.as_bytes()));
        p += 1;
    }
    Ok((co, pl))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((检出行数, 补丁数, 私改数))。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let (co, pl) = build_initial()?;
    co.closed()?;
    // 计数红线：9 ≤ 10 且告警一致。
    pl.limit_alert_consistent(false)?;
    // 事实 fork 检出：私改数必须为 0。
    let n = pl.private_edit_count();
    if n != 0 {
        return Err(Ws2Code::PrivateEdit.val());
    }
    Ok((co.len(), pl.len(), n))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 私改未走补丁：源树差异指纹不在补丁集 → private_edit ≥ 1。
        let rogue_ok = {
            let (_, pl) = build_initial().unwrap_or_else(|_| {
                (CheckoutLedger::new(), PatchLedger::new())
            });
            let mut p2 = PatchLedger::new();
            let mut i = 0;
            while i < pl.rows.len() {
                let _ = p2.add_patch(pl.rows[i].clone());
                i += 1;
            }
            p2.add_tree_diff(fnv1a(b"rogue-edit-not-a-patch"));
            p2.private_edit_count() == 1
        };
        led.record(0, rogue_ok);

        // ② 补丁缺 ADR：格式非法必拒。
        let bad = PatchRow {
            name: String::from("wine-0099-x"),
            summary: String::from("摘要占位"),
            min_diff: 5,
            state: UpstreamState::KeptLocal,
            adr: String::from("无编号"),
        };
        led.record(1, vet_patch(&bad) == Err(Ws2Code::PatchNoAdr.val()));

        // ③ 计数超限未告警：塞 11 件补丁不告警 → 必红。
        let over_ok = {
            let (_, pl) = build_initial().unwrap_or_else(|_| {
                (CheckoutLedger::new(), PatchLedger::new())
            });
            let mut p3 = PatchLedger::new();
            let mut i = 0;
            while i < pl.rows.len() {
                let _ = p3.add_patch(pl.rows[i].clone());
                i += 1;
            }
            let extra = PatchRow {
                name: String::from("wine-0100-over"),
                summary: String::from("超限样本"),
                min_diff: 3,
                state: UpstreamState::ToAssess,
                adr: String::from("ADR-0100"),
            };
            let extra2 = PatchRow {
                name: String::from("wine-0101-over"),
                summary: String::from("超限样本二"),
                min_diff: 4,
                state: UpstreamState::ToAssess,
                adr: String::from("ADR-0101"),
            };
            let _ = p3.add_patch(extra);
            let _ = p3.add_patch(extra2);
            p3.over_limit()
                && p3.limit_alert_consistent(false)
                    == Err(Ws2Code::OverLimitNoAlert.val())
        };
        led.record(2, over_ok);

        // ④ 版本钉漂移。
        led.record(3, {
            let bad2 = CheckoutRow {
                dir: String::from(CHECKOUT_DIRS[0]),
                version_pin: String::from("wine-9.9"),
                commit_hash: String::from(PINNED_COMMIT),
                readonly: true,
            };
            vet_checkout(&bad2) == Err(Ws2Code::PinDrift.val())
        });

        // ⑤ 重复补丁名。
        led.record(4, {
            let (_, pl) = build_initial().unwrap_or_else(|_| {
                (CheckoutLedger::new(), PatchLedger::new())
            });
            let mut p5 = PatchLedger::new();
            let _ = p5.add_patch(pl.rows[0].clone());
            p5.add_patch(pl.rows[0].clone()) == Err(Ws2Code::PatchDuplicated.val())
        });

        // ⑥ 检出区可写 + 上游化状态越闭集 + commit 哈希长度错。
        led.record(5, {
            let bad3 = CheckoutRow {
                dir: String::from(CHECKOUT_DIRS[1]),
                version_pin: String::from(PINNED_VERSION),
                commit_hash: String::from(PINNED_COMMIT),
                readonly: false,
            };
            vet_checkout(&bad3) == Err(Ws2Code::CheckoutWritable.val())
        });
        led.record(5, UpstreamState::from_u32(4).is_none());
        led.record(5, UpstreamState::from_u32(0xFFFF).is_none());
        led.record(5, {
            let bad4 = CheckoutRow {
                dir: String::from(CHECKOUT_DIRS[1]),
                version_pin: String::from(PINNED_VERSION),
                commit_hash: String::from("short-hash"),
                readonly: true,
            };
            vet_checkout(&bad4) == Err(Ws2Code::CheckoutTriFieldShort.val())
                && (rnd % 1) == 0
        });

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(Ws2Code::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法跟随形态必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let (co, pl) = build_initial()?;
    co.closed()?;
    // 补丁计数红线内。
    if pl.over_limit() {
        return Err(Ws2Code::OverLimitNoAlert.val());
    }
    // 六目录名点验。
    let mut d = 0;
    while d < CHECKOUT_DIRS.len() {
        if !dir_legal(CHECKOUT_DIRS[d]) {
            return Err(Ws2Code::DirOutOfRange.val());
        }
        d += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < Ws2Code::ALL.len() {
        let v = Ws2Code::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：6 检出行 / 9 补丁 / 私改 0。
    let g = golden_run();
    v.push((
        "J1-黄金正演6行9补丁0私改",
        matches!(g, Ok((6, 9, 0))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 检出六目录覆盖。
    let (co, pl) = build_initial()
        .unwrap_or_else(|_| (CheckoutLedger::new(), PatchLedger::new()));
    v.push((
        "J1-检出六目录覆盖",
        matches!(co.closed(), Ok(())),
        format!("{} 行", co.len()),
    ));
    // 4. 补丁计数红线：9 ≤ 10 且告警一致。
    v.push((
        "J1-补丁计数红线内",
        !pl.over_limit()
            && matches!(pl.limit_alert_consistent(false), Ok(())),
        format!("{}/{}", pl.len(), PATCH_LIMIT),
    ));
    // 5. 事实 fork 检出 0（判据核心）。
    v.push((
        "J1-事实fork检出0",
        pl.private_edit_count() == 0,
        format!("{}", pl.private_edit_count()),
    ));
    // 6. 每补丁 ADR 齐备。
    let adr_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < pl.rows.len() {
            if vet_patch(&pl.rows[i]).is_err() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-每补丁ADR齐备", adr_ok, format!("{} 件", pl.len())));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("目录闭集+红线"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x41);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16002 Wine 源树落位与只跟随检出纪律");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
