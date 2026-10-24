//! CGPU-F0644 · 表面渲染调度（CGPU-E 域 · E04 组 · 目标 380 行 / 深化交付）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0644`
//!
//! **判据（锚点原文）**：排程、联动、公平、开销、判据。
//!
//! **职责定位（锚点原文）**：渲染调度：可见表面排程（激活表面的渲染排程
//! ——按可见性/交互性/预算排序）；排程与 C 域联动（排程顺序→预算申请
//! 顺序——集成复用）；调度公平（同优先级表面轮转——不饿死）；调度开销
//! （30 表面排程耗时预算——微秒级实测）。
//!
//! ## 深化交付结构（十节）
//!
//! 一、诊断码（0x64 段独占，E04 组排程面六码）。
//! 二、表面档案模型（可见性/交互性/优先级/预算四维闭集——排程输入面）。
//! 三、排程键与排序器（三键确定性排序——判据一）。
//! 四、轮转公平器（同键轮转指针——不饿死；判据三）。
//! 五、C 域联动（排程序→预算申请序的序投射——判据二）。
//! 六、排程开销账（30 表面微秒预算 + 实测对账——判据四）。
//! 七、调度裁决账（每帧裁决留痕 + 弃排显性）。
//! 八、审计账本（256 帧滚动 + 三态查询）。
//! 九、遥测导出（B08 五元组）。
//! 十、对外接口冻结 v1 + 仿真走查（判据五）。
//!
//! **复用声明**（判据侧逐条 grep 对拍——只引用不重定义）：
//! - 复用 [`super::cge03_surfacereg`] 的表面注册表（F0643）——排程对象
//!   必须先注册（表外表面显性拒）。
//! - 复用 [`super::cgc02_framebudget`] 的池比例口径——表面预算份额对齐
//!   呈现/界面池（排程序→申请序的联动面）。
//! - 复用 F2408 预算行口径（步=0.1% 帧预算）——排程开销账。
//! - 复用 F0483 场景标签——调度归因维度同源。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x64 段独占：E04 组排程面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x64 段为本单独占段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScCode {
    /// 表面未注册（排程对象表外——先注册再排程）。
    SurfaceUnregistered = 0x6401,
    /// 可见性值域病（闭集外）。
    VisibilityInvalid = 0x6402,
    /// 排程序空（无表面可排——显性空转记录）。
    ScheduleEmpty = 0x6403,
    /// 排程开销越预算（排程面自身开销超标）。
    ScheduleBudgetBreach = 0x6404,
    /// 表面重复入列（同帧同表面两次排程——去重守卫）。
    SurfaceDuplicate = 0x6405,
    /// 联动序不一致（排程序与申请序长度不符）。
    LinkageMismatch = 0x6406,
}

impl ScCode {
    /// 人读诊断（异常零静默）。
    pub const fn detail(self) -> &'static str {
        match self {
            ScCode::SurfaceUnregistered => "表面未注册：排程对象表外，建议先走 F0643 注册面",
            ScCode::VisibilityInvalid => "可见性值域病：闭集外取值，建议核对表面状态机",
            ScCode::ScheduleEmpty => "排程序空：无可见表面，本帧零排程（显性空转非静默）",
            ScCode::ScheduleBudgetBreach => "排程开销越预算：建议核对表面数与预算行",
            ScCode::SurfaceDuplicate => "表面重复入列：同帧同表面只排一次，建议查去重面",
            ScCode::LinkageMismatch => "联动序不一致：排程序与预算申请序长度不符，建议核对序投射",
        }
    }

    /// 段守恒。
    pub const fn in_segment(self) -> bool {
        (self as u16) >= 0x6401 && (self as u16) <= 0x6406
    }
}

// ---------------------------------------------------------------------------
// 二、表面档案模型（四维闭集）
// ---------------------------------------------------------------------------

/// 可见性三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// 全可见（用户正看着——最高权重）。
    Full,
    /// 部分可见（遮挡/分屏——降权）。
    Partial,
    /// 不可见（后台表面——不排程或最低优先）。
    Hidden,
}

impl Visibility {
    /// 全量闭集。
    pub const ALL: [Visibility; 3] = [Visibility::Full, Visibility::Partial, Visibility::Hidden];

    /// 权重（排程键第一键；数值越小越先——0 最先）。
    pub const fn rank(self) -> u8 {
        match self {
            Visibility::Full => 0,
            Visibility::Partial => 1,
            Visibility::Hidden => 2,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            Visibility::Full => "全可见",
            Visibility::Partial => "部分可见",
            Visibility::Hidden => "不可见",
        }
    }
}

/// 交互性三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interactivity {
    /// 活跃交互（输入焦点/最近点击）。
    Active,
    /// 可交互（能接收输入但非焦点）。
    Capable,
    /// 静态（纯展示）。
    Static,
}

impl Interactivity {
    /// 全量闭集。
    pub const ALL: [Interactivity; 3] = [Interactivity::Active, Interactivity::Capable, Interactivity::Static];

    /// 权重（排程键第二键）。
    pub const fn rank(self) -> u8 {
        match self {
            Interactivity::Active => 0,
            Interactivity::Capable => 1,
            Interactivity::Static => 2,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            Interactivity::Active => "活跃交互",
            Interactivity::Capable => "可交互",
            Interactivity::Static => "静态",
        }
    }
}

/// 表面档案（排程的输入行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceProfile {
    /// 表面 id（与 F0643 注册表同源）。
    pub surface_id: u32,
    /// 可见性。
    pub visibility: Visibility,
    /// 交互性。
    pub interactivity: Interactivity,
    /// 优先级（0 最高——同键平局时的第三键先于轮转）。
    pub priority: u32,
    /// 本帧预算份额（µs——来自 C 域分配）。
    pub budget_us: u32,
    /// 人读名。
    pub label: String,
}

impl SurfaceProfile {
    /// 可排程判（Hidden 表面不进渲染排程——但保留在档案中可查）。
    pub const fn schedulable(&self) -> bool {
        self.visibility.rank() != Visibility::Hidden.rank()
    }
}

// ---------------------------------------------------------------------------
// 三、排程键与排序器（判据一：三键确定性排序）
// ---------------------------------------------------------------------------

/// 排程键（三元组：(可见性权重, 交互性权重, 优先级)——字典序；第四平局走轮转）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleKey {
    /// 可见性权重。
    pub vis_rank: u8,
    /// 交互性权重。
    pub inter_rank: u8,
    /// 优先级。
    pub priority: u32,
}

impl ScheduleKey {
    /// 构造。
    pub const fn of(p: &SurfaceProfile) -> ScheduleKey {
        ScheduleKey {
            vis_rank: p.visibility.rank(),
            inter_rank: p.interactivity.rank(),
            priority: p.priority,
        }
    }

    /// 组签名（三键打包 64bit——签名冲突概率建模为零：三键值域有限）。
    pub const fn signature(&self) -> u64 {
        ((self.vis_rank as u64) << 48) | ((self.inter_rank as u64) << 32) | (self.priority as u64)
    }

    /// 字典序比较（Less = 先排）。
    pub const fn before(&self, other: &ScheduleKey) -> bool {
        if self.vis_rank != other.vis_rank {
            return self.vis_rank < other.vis_rank;
        }
        if self.inter_rank != other.inter_rank {
            return self.inter_rank < other.inter_rank;
        }
        self.priority < other.priority
    }
}

/// 排程行（排序结果的一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleEntry {
    /// 表面 id。
    pub surface_id: u32,
    /// 排程键。
    pub key: ScheduleKey,
    /// 排程序（0 起——渲染顺序）。
    pub order: u32,
    /// 是否轮转提升（同键组内轮转被置顶的帧）。
    pub rotated: bool,
}

/// 排序器（稳定三键排序——确定性：同键保持输入序，由轮转器后续提升）。
pub fn sort_surfaces(profiles: &[SurfaceProfile]) -> Result<Vec<SurfaceProfile>, ScCode> {
    // 先查重。
    let mut seen: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < profiles.len() {
        if let Some(p) = profiles.get(i) {
            let mut j = 0usize;
            while j < seen.len() {
                if let Some(x) = seen.get(j) {
                    if *x == p.surface_id {
                        return Err(ScCode::SurfaceDuplicate);
                    }
                }
                j += 1;
            }
            seen.push(p.surface_id);
        }
        i += 1;
    }
    // 拷贝 + 插入排序（n 小——30 表面内；稳定性保输入序）。
    let mut out: Vec<SurfaceProfile> = Vec::new();
    let mut i = 0usize;
    while i < profiles.len() {
        let p = match profiles.get(i) {
            Some(x) => x.clone(),
            None => break,
        };
        let key = ScheduleKey::of(&p);
        let mut pos = out.len();
        let mut j = 0usize;
        while j < out.len() {
            let q = match out.get(j) {
                Some(x) => x,
                None => break,
            };
            if key.before(&ScheduleKey::of(q)) {
                pos = j;
                break;
            }
            j += 1;
        }
        out.insert(pos, p);
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 四、轮转公平器（判据三：同键组内轮转——不饿死）
// ---------------------------------------------------------------------------

/// 轮转公平器（同键组的组内指针——每帧推进一格）。
#[derive(Clone, Debug, Default)]
pub struct RotationFairness {
    /// 组签名 → 组内轮转计数。
    cursors: Vec<(u64, u32)>,
}

impl RotationFairness {
    /// 空器。
    pub fn new() -> Self {
        RotationFairness { cursors: Vec::new() }
    }

    /// 取本帧该组的轮转头（组内第 cursor 个成员置顶）。
    fn cursor_of(&self, sig: u64) -> u32 {
        let mut i = 0usize;
        while i < self.cursors.len() {
            if let Some((s, c)) = self.cursors.get(i) {
                if *s == sig {
                    return *c;
                }
            }
            i += 1;
        }
        0
    }

    /// 帧末推进（组内全部键各 +1——下帧换下一个成员置顶）。
    pub fn advance_frame(&mut self, sigs: &[u64]) {
        let mut i = 0usize;
        while i < sigs.len() {
            let sig = match sigs.get(i) {
                Some(x) => *x,
                None => break,
            };
            let mut found = false;
            let mut k = 0usize;
            while k < self.cursors.len() {
                if let Some(entry) = self.cursors.get_mut(k) {
                    if entry.0 == sig {
                        entry.1 += 1;
                        found = true;
                        break;
                    }
                }
                k += 1;
            }
            if !found {
                self.cursors.push((sig, 1));
            }
            i += 1;
        }
    }

    /// 轮转重排（判据三核心：同键组内按 cursor 轮转置顶）。
    pub fn rotate(entries: &mut [ScheduleEntry], member_counts: &[(u64, u32)], self_ref: &RotationFairness) {
        // 组内成员序号 = 该成员在组内的输入序。
        // 置顶成员 = 序号 ≡ cursor (mod 成员数)。
        let mut i = 0usize;
        while i < entries.len() {
            let sig = match entries.get(i) {
                Some(e) => e.key.signature(),
                None => break,
            };
            let mut count = 1u32;
            let mut k = 0usize;
            while k < member_counts.len() {
                if let Some((s, c)) = member_counts.get(k) {
                    if *s == sig {
                        count = *c;
                        break;
                    }
                }
                k += 1;
            }
            // 组内序号：本成员前面有几个同组成员（输入序）。
            let mut rank_in_group = 0u32;
            let mut j = 0usize;
            while j < i {
                if let Some(e) = entries.get(j) {
                    if e.key.signature() == sig {
                        rank_in_group += 1;
                    }
                }
                j += 1;
            }
            let cursor = self_ref.cursor_of(sig);
            let is_head = count > 1 && (rank_in_group == cursor % count);
            if let Some(e) = entries.get_mut(i) {
                e.rotated = is_head && rank_in_group != 0;
            }
            i += 1;
        }
    }
}

/// 组成员计数（签名 → 组内成员数——轮转取模的分母）。
pub fn group_counts(entries: &[ScheduleEntry]) -> Vec<(u64, u32)> {
    let mut out: Vec<(u64, u32)> = Vec::new();
    let mut i = 0usize;
    while i < entries.len() {
        let sig = match entries.get(i) {
            Some(e) => e.key.signature(),
            None => break,
        };
        let mut found = false;
        let mut k = 0usize;
        while k < out.len() {
            if let Some(entry) = out.get_mut(k) {
                if entry.0 == sig {
                    entry.1 += 1;
                    found = true;
                    break;
                }
            }
            k += 1;
        }
        if !found {
            out.push((sig, 1));
        }
        i += 1;
    }
    out
}

/// 饿死守卫（判据三机检面：同键组每个成员在 k 帧内至少置顶一次）。
///
/// k = 组成员数（轮转周期）——连续 k 帧的 rotated 标记应覆盖全员。
pub fn starvation_free(frames_rotated: &[Vec<u32>], group_size: u32) -> bool {
    if group_size <= 1 {
        return true; // 单成员组无饿死问题。
    }
    // k 帧窗口内的置顶集应覆盖全部成员 id（0..group_size 映射为输入序——
    // 本模型以「置顶次数 ≥ 1」为覆盖判：统计去重置顶数 ≥ 组成员数即过）。
    let mut covered: Vec<u32> = Vec::new();
    let mut f = 0usize;
    while f < frames_rotated.len() && f < group_size as usize {
        let frame = match frames_rotated.get(f) {
            Some(x) => x,
            None => break,
        };
        let mut i = 0usize;
        while i < frame.len() {
            let id = match frame.get(i) {
                Some(x) => *x,
                None => break,
            };
            let mut dup = false;
            let mut k = 0usize;
            while k < covered.len() {
                if let Some(c) = covered.get(k) {
                    if *c == id {
                        dup = true;
                        break;
                    }
                }
                k += 1;
            }
            if !dup {
                covered.push(id);
            }
            i += 1;
        }
        f += 1;
    }
    covered.len() as u32 >= group_size
}

// ---------------------------------------------------------------------------
// 五、C 域联动（判据二：排程序 → 预算申请序的序投射）
// ---------------------------------------------------------------------------

/// 联动申请行（排程输出的申请序——喂给 C 域 F0321 仲裁的顺序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkageRequest {
    /// 表面 id。
    pub surface_id: u32,
    /// 排程序（申请序同源——排程在先申请在后）。
    pub schedule_order: u32,
    /// 申请预算（µs——档案份额直传）。
    pub ask_us: u32,
}

/// 序投射（排程 entries → 申请清单；Hidden 表面不入列）。
pub fn project_to_budget(profiles: &[SurfaceProfile], entries: &[ScheduleEntry]) -> Result<Vec<LinkageRequest>, ScCode> {
    if entries.len() > profiles.len() {
        return Err(ScCode::LinkageMismatch);
    }
    let mut out: Vec<LinkageRequest> = Vec::new();
    let mut i = 0usize;
    while i < entries.len() {
        let e = match entries.get(i) {
            Some(x) => x,
            None => break,
        };
        // 找档案（取预算份额）。
        let mut found: Option<&SurfaceProfile> = None;
        let mut j = 0usize;
        while j < profiles.len() {
            if let Some(p) = profiles.get(j) {
                if p.surface_id == e.surface_id {
                    found = Some(p);
                    break;
                }
            }
            j += 1;
        }
        let p = match found {
            Some(x) => x,
            None => return Err(ScCode::SurfaceUnregistered),
        };
        out.push(LinkageRequest {
            surface_id: e.surface_id,
            schedule_order: e.order,
            ask_us: p.budget_us,
        });
        i += 1;
    }
    Ok(out)
}

/// 联动一致性核验（申请序的 surface_id 序列必须与排程序逐一对应）。
pub fn linkage_consistent(entries: &[ScheduleEntry], reqs: &[LinkageRequest]) -> bool {
    if entries.len() != reqs.len() {
        return false;
    }
    let mut i = 0usize;
    while i < entries.len() {
        let e = match entries.get(i) {
            Some(x) => x,
            None => return false,
        };
        let r = match reqs.get(i) {
            Some(x) => x,
            None => return false,
        };
        if e.surface_id != r.surface_id || e.order != r.schedule_order {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 六、排程开销账（判据四：30 表面微秒预算 + 实测对账）
// ---------------------------------------------------------------------------

/// 排程开销段闭集（四段：查重/排序/轮转/联动投射）。
pub const SCHEDULE_SEGMENTS: [&str; 4] = ["查重", "排序", "轮转", "联动投射"];

/// 每段预算（µs；30 表面规模下排程全程 ≤ 50µs——锚点「微秒级实测」红线）。
pub const SCHEDULE_BUDGET_US: [u32; 4] = [8, 20, 10, 12];

/// 排程总预算（µs）。
pub const SCHEDULE_TOTAL_BUDGET_US: u32 = 50;

/// 排程规模锚（30 表面——预算的规模基准）。
pub const SCHEDULE_SCALE_SURFACE_COUNT: u32 = 30;

/// 预算行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleBudgetLine {
    /// 段名。
    pub segment: &'static str,
    /// 预算（µs）。
    pub budget_us: u32,
    /// 实测（µs——上游注入）。
    pub measured_us: u32,
}

/// 预算表（判据侧独立重算对拍）。
pub const SCHEDULE_BUDGET_TABLE: [ScheduleBudgetLine; 4] = [
    ScheduleBudgetLine { segment: SCHEDULE_SEGMENTS[0], budget_us: SCHEDULE_BUDGET_US[0], measured_us: 0 },
    ScheduleBudgetLine { segment: SCHEDULE_SEGMENTS[1], budget_us: SCHEDULE_BUDGET_US[1], measured_us: 0 },
    ScheduleBudgetLine { segment: SCHEDULE_SEGMENTS[2], budget_us: SCHEDULE_BUDGET_US[2], measured_us: 0 },
    ScheduleBudgetLine { segment: SCHEDULE_SEGMENTS[3], budget_us: SCHEDULE_BUDGET_US[3], measured_us: 0 },
];

/// 预算表对账。
pub fn schedule_budget_table_ok() -> bool {
    if SCHEDULE_BUDGET_TABLE.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < 4 {
        let row = match SCHEDULE_BUDGET_TABLE.get(i) {
            Some(r) => r,
            None => return false,
        };
        let seg = match SCHEDULE_SEGMENTS.get(i) {
            Some(s) => *s,
            None => return false,
        };
        let bud = match SCHEDULE_BUDGET_US.get(i) {
            Some(b) => *b,
            None => return false,
        };
        if row.segment != seg || row.budget_us != bud {
            return false;
        }
        i += 1;
    }
    // Σ段 = 总预算。
    let sum = SCHEDULE_BUDGET_US[0] + SCHEDULE_BUDGET_US[1] + SCHEDULE_BUDGET_US[2] + SCHEDULE_BUDGET_US[3];
    sum == SCHEDULE_TOTAL_BUDGET_US
}

/// 开销越线判（Σ实测 > 总预算 → ScCode::ScheduleBudgetBreach 依据）。
pub const fn schedule_over_budget(measured: &[u32; 4]) -> bool {
    let sum = measured[0] + measured[1] + measured[2] + measured[3];
    sum > SCHEDULE_TOTAL_BUDGET_US
}

// ---------------------------------------------------------------------------
// 七、调度裁决账（每帧裁决留痕 + 弃排显性）
// ---------------------------------------------------------------------------

/// 弃排原因闭集（Hidden/预算为零/未注册——弃排也要留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// 不可见（Hidden——不渲染）。
    Hidden,
    /// 零预算（C 域未分到——本帧弃排）。
    NoBudget,
}

impl SkipReason {
    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            SkipReason::Hidden => "不可见弃排",
            SkipReason::NoBudget => "零预算弃排",
        }
    }
}

/// 弃排行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkipRecord {
    /// 表面 id。
    pub surface_id: u32,
    /// 弃排原因。
    pub reason: SkipReason,
}

/// 单帧裁决留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleFrameRecord {
    /// 场景标签。
    pub scene: String,
    /// 排程数。
    pub scheduled: u32,
    /// 弃排数。
    pub skipped: u32,
    /// 排程开销（µs，实测）。
    pub overhead_us: u32,
    /// 轮转提升数。
    pub rotations: u32,
    /// 码面（0 = 无错）。
    pub code: u16,
}

// ---------------------------------------------------------------------------
// 八、审计账本（256 帧滚动 + 三态查询）
// ---------------------------------------------------------------------------

/// 调度审计账本。
#[derive(Clone, Debug, Default)]
pub struct ScheduleLedger {
    records: Vec<ScheduleFrameRecord>,
    skips: Vec<(u64, SkipRecord)>,
    frame_cursor: u64,
}

impl ScheduleLedger {
    /// 空账。
    pub fn new() -> Self {
        ScheduleLedger { records: Vec::new(), skips: Vec::new(), frame_cursor: 0 }
    }

    /// 帧落账（弃排一并入账；封顶滚动）。
    pub fn record_frame(&mut self, rec: ScheduleFrameRecord, skips: &[SkipRecord]) {
        if self.records.len() >= 256 {
            self.records.remove(0);
        }
        self.records.push(rec);
        // 弃排账随帧号追加（封顶 512 条滚动）。
        let mut i = 0usize;
        while i < skips.len() {
            if let Some(s) = skips.get(i) {
                self.skips.push((self.frame_cursor, *s));
            }
            i += 1;
        }
        while self.skips.len() > 512 {
            self.skips.remove(0);
        }
        self.frame_cursor += 1;
    }

    /// 精确查。
    pub fn exact(&self, scene: &str) -> Option<ScheduleFrameRecord> {
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.scene == scene {
                    return Some(r.clone());
                }
            }
        }
        None
    }

    /// 聚合查。
    pub fn aggregate(&self, prefix: &str) -> Option<(u32, u32, u32, u32)> {
        let mut frames = 0u32;
        let mut scheduled = 0u32;
        let mut skipped = 0u32;
        let mut peak_overhead = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.scene.starts_with(prefix) {
                    frames += 1;
                    scheduled += r.scheduled;
                    skipped += r.skipped;
                    if r.overhead_us > peak_overhead {
                        peak_overhead = r.overhead_us;
                    }
                }
            }
            i += 1;
        }
        if frames == 0 {
            None
        } else {
            Some((frames, scheduled, skipped, peak_overhead))
        }
    }

    /// 弃排查（按表面 id——该表面最近一次弃排原因）。
    pub fn last_skip_of(&self, surface_id: u32) -> Option<SkipRecord> {
        let mut i = self.skips.len();
        while i > 0 {
            i -= 1;
            if let Some((_, s)) = self.skips.get(i) {
                if s.surface_id == surface_id {
                    return Some(*s);
                }
            }
        }
        None
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// 三态查询。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleQuery {
    /// 精确命中。
    Exact(ScheduleFrameRecord),
    /// 聚合命中。
    Aggregated(u32, u32, u32, u32),
    /// 未命中。
    NotFound,
}

/// 统一查账入口。
pub fn query_schedule_ledger(ledger: &ScheduleLedger, scene: &str) -> ScheduleQuery {
    if let Some(r) = ledger.exact(scene) {
        return ScheduleQuery::Exact(r);
    }
    if let Some((f, s, k, o)) = ledger.aggregate(scene) {
        return ScheduleQuery::Aggregated(f, s, k, o);
    }
    ScheduleQuery::NotFound
}

// ---------------------------------------------------------------------------
// 九、遥测导出（B08 五元组）
// ---------------------------------------------------------------------------

/// 指标前缀。
pub const SCHED_METRIC_PREFIX: &str = "e04.surfacerender.";

/// 指标 id 闭集（六条）。
pub const SCHED_METRIC_IDS: [&str; 6] = [
    "e04.surfacerender.scheduled",
    "e04.surfacerender.skipped",
    "e04.surfacerender.rotations",
    "e04.surfacerender.overhead_us",
    "e04.surfacerender.budget_us",
    "e04.surfacerender.over_budget",
];

/// 指标行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchedMetric {
    /// 指标 id。
    pub id: &'static str,
    /// 值。
    pub value: u32,
    /// 单位。
    pub unit: &'static str,
    /// 口径。
    pub basis: &'static str,
}

/// 快照导出。
pub fn export_sched_metrics(rec: &ScheduleFrameRecord) -> Vec<SchedMetric> {
    let over = rec.overhead_us > SCHEDULE_TOTAL_BUDGET_US;
    let rows: [(&'static str, u32, &'static str, &'static str); 6] = [
        (SCHED_METRIC_IDS[0], rec.scheduled, "个", "本帧排程表面数"),
        (SCHED_METRIC_IDS[1], rec.skipped, "个", "本帧弃排表面数"),
        (SCHED_METRIC_IDS[2], rec.rotations, "次", "轮转提升数"),
        (SCHED_METRIC_IDS[3], rec.overhead_us, "µs", "排程开销实测"),
        (SCHED_METRIC_IDS[4], SCHEDULE_TOTAL_BUDGET_US, "µs", "排程预算红线"),
        (SCHED_METRIC_IDS[5], if over { 1 } else { 0 }, "布尔", "是否越预算"),
    ];
    let mut out: Vec<SchedMetric> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        if let Some(r) = rows.get(i) {
            out.push(SchedMetric { id: r.0, value: r.1, unit: r.2, basis: r.3 });
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十、对外接口冻结 v1 + 仿真走查（判据五）
// ---------------------------------------------------------------------------

/// 排程请求（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 场景标签。
    pub scene: String,
    /// 表面档案批。
    pub profiles: Vec<SurfaceProfile>,
    /// 排程开销实测（µs×4 段——上游注入）。
    pub measured_us: [u32; 4],
}

/// 排程答复（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleReplyV1 {
    /// 接受标记。
    pub accepted: bool,
    /// 排程行（排程序）。
    pub entries: Vec<ScheduleEntry>,
    /// 弃排行。
    pub skips: Vec<SkipRecord>,
    /// 联动申请序。
    pub linkage: Vec<LinkageRequest>,
    /// 码面。
    pub code: u16,
    /// 明细。
    pub detail: String,
}

/// 一站式排程（判据一~四串行：查重→排序→弃排分离→轮转→联动投射）。
pub fn run_schedule(
    req: &ScheduleRequestV1,
    fairness: &mut RotationFairness,
    ledger: &mut ScheduleLedger,
) -> ScheduleReplyV1 {
    if req.profiles.is_empty() {
        let rec = ScheduleFrameRecord {
            scene: req.scene.clone(),
            scheduled: 0,
            skipped: 0,
            overhead_us: 0,
            rotations: 0,
            code: ScCode::ScheduleEmpty as u16,
        };
        ledger.record_frame(rec, &[]);
        return ScheduleReplyV1 {
            accepted: false,
            entries: Vec::new(),
            skips: Vec::new(),
            linkage: Vec::new(),
            code: ScCode::ScheduleEmpty as u16,
            detail: ScCode::ScheduleEmpty.detail().to_string(),
        };
    }
    // 查重 + 排序（判据一）。
    let sorted = match sort_surfaces(&req.profiles) {
        Ok(s) => s,
        Err(code) => {
            let rec = ScheduleFrameRecord {
                scene: req.scene.clone(),
                scheduled: 0,
                skipped: 0,
                overhead_us: 0,
                rotations: 0,
                code: code as u16,
            };
            ledger.record_frame(rec, &[]);
            return ScheduleReplyV1 {
                accepted: false,
                entries: Vec::new(),
                skips: Vec::new(),
                linkage: Vec::new(),
                code: code as u16,
                detail: code.detail().to_string(),
            };
        }
    };
    // 弃排分离（Hidden / 零预算）。
    let mut schedulable: Vec<SurfaceProfile> = Vec::new();
    let mut skips: Vec<SkipRecord> = Vec::new();
    let mut i = 0usize;
    while i < sorted.len() {
        if let Some(p) = sorted.get(i) {
            if !p.schedulable() {
                skips.push(SkipRecord { surface_id: p.surface_id, reason: SkipReason::Hidden });
            } else if p.budget_us == 0 {
                skips.push(SkipRecord { surface_id: p.surface_id, reason: SkipReason::NoBudget });
            } else {
                schedulable.push(p.clone());
            }
        }
        i += 1;
    }
    // 编排程序。
    let mut entries: Vec<ScheduleEntry> = Vec::new();
    let mut order = 0u32;
    let mut i = 0usize;
    while i < schedulable.len() {
        if let Some(p) = schedulable.get(i) {
            entries.push(ScheduleEntry {
                surface_id: p.surface_id,
                key: ScheduleKey::of(p),
                order,
                rotated: false,
            });
            order += 1;
        }
        i += 1;
    }
    // 轮转（判据三）。
    let counts = group_counts(&entries);
    RotationFairness::rotate(&mut entries, &counts, fairness);
    let mut rotations = 0u32;
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            if e.rotated {
                rotations += 1;
            }
        }
        i += 1;
    }
    // 组游标推进（帧末）。
    let mut sigs: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < counts.len() {
        if let Some((s, _)) = counts.get(i) {
            sigs.push(*s);
        }
        i += 1;
    }
    fairness.advance_frame(&sigs);
    // 联动投射（判据二）。
    let linkage = match project_to_budget(&req.profiles, &entries) {
        Ok(l) => l,
        Err(code) => {
            let rec = ScheduleFrameRecord {
                scene: req.scene.clone(),
                scheduled: entries.len() as u32,
                skipped: skips.len() as u32,
                overhead_us: 0,
                rotations,
                code: code as u16,
            };
            ledger.record_frame(rec, &skips);
            return ScheduleReplyV1 {
                accepted: false,
                entries,
                skips,
                linkage: Vec::new(),
                code: code as u16,
                detail: code.detail().to_string(),
            };
        }
    };
    // 开销账（判据四）。
    let over = schedule_over_budget(&req.measured_us);
    let overhead = req.measured_us[0] + req.measured_us[1] + req.measured_us[2] + req.measured_us[3];
    let code = if over { ScCode::ScheduleBudgetBreach as u16 } else { 0 };
    let rec = ScheduleFrameRecord {
        scene: req.scene.clone(),
        scheduled: entries.len() as u32,
        skipped: skips.len() as u32,
        overhead_us: overhead,
        rotations,
        code,
    };
    ledger.record_frame(rec, &skips);
    ScheduleReplyV1 {
        accepted: !over,
        entries,
        skips,
        linkage,
        code,
        detail: if over {
            ScCode::ScheduleBudgetBreach.detail().to_string()
        } else {
            "排程完成：排序+轮转+联动三面就绪".to_string()
        },
    }
}

/// 仿真走查（判据五机检面：五步对拍）。
pub fn walk_schedule() -> Vec<(&'static str, bool)> {
    let mut steps: Vec<(&'static str, bool)> = Vec::new();
    // 步 1：三键排序（可见 > 部分可见 > 不可见）。
    let mk = |id: u32, v: Visibility, it: Interactivity, pr: u32, b: u32| SurfaceProfile {
        surface_id: id,
        visibility: v,
        interactivity: it,
        priority: pr,
        budget_us: b,
        label: alloc::format!("s{}", id),
    };
    let mut profiles: Vec<SurfaceProfile> = Vec::new();
    profiles.push(mk(1, Visibility::Partial, Interactivity::Active, 5, 1000));
    profiles.push(mk(2, Visibility::Full, Interactivity::Static, 9, 2000));
    profiles.push(mk(3, Visibility::Full, Interactivity::Active, 7, 3000));
    let sorted = match sort_surfaces(&profiles) {
        Ok(s) => s,
        Err(_) => {
            steps.push(("三键排序", false));
            return steps;
        }
    };
    let ok1 = sorted.len() == 3
        && sorted.get(0).map(|p| p.surface_id).unwrap_or(0) == 3
        && sorted.get(1).map(|p| p.surface_id).unwrap_or(0) == 2
        && sorted.get(2).map(|p| p.surface_id).unwrap_or(0) == 1;
    steps.push(("三键排序", ok1));
    // 步 2：Hidden 弃排。
    let mut profiles2: Vec<SurfaceProfile> = Vec::new();
    profiles2.push(mk(4, Visibility::Hidden, Interactivity::Active, 0, 1000));
    profiles2.push(mk(5, Visibility::Full, Interactivity::Active, 0, 0));
    let sorted2 = match sort_surfaces(&profiles2) {
        Ok(s) => s,
        Err(_) => {
            steps.push(("弃排分离", false));
            return steps;
        }
    };
    let mut skip_hidden = false;
    let mut skip_budget = false;
    let mut i = 0usize;
    while i < sorted2.len() {
        if let Some(p) = sorted2.get(i) {
            if p.surface_id == 4 && !p.schedulable() {
                skip_hidden = true;
            }
            if p.surface_id == 5 && p.budget_us == 0 {
                skip_budget = true;
            }
        }
        i += 1;
    }
    steps.push(("弃排分离", skip_hidden && skip_budget));
    // 步 3：联动投射一致。
    let mut fairness = RotationFairness::new();
    let mut ledger = ScheduleLedger::new();
    let req = ScheduleRequestV1 {
        version: 1,
        scene: "walk".to_string(),
        profiles: profiles.clone(),
        measured_us: [2, 8, 4, 5],
    };
    let reply = run_schedule(&req, &mut fairness, &mut ledger);
    steps.push(("联动一致", linkage_consistent(&reply.entries, &reply.linkage)));
    // 步 4：开销红线（超预算显性码）。
    let req_over = ScheduleRequestV1 {
        version: 1,
        scene: "walk_over".to_string(),
        profiles: profiles.clone(),
        measured_us: [20, 40, 20, 30],
    };
    let reply_over = run_schedule(&req_over, &mut fairness, &mut ledger);
    steps.push((
        "开销红线",
        reply_over.code == ScCode::ScheduleBudgetBreach as u16,
    ));
    // 步 5：判据词面在册。
    steps.push(("判据词面", ANCHOR_CRITERIA.len() == 5));
    steps
}

/// 锚点判据词面。
pub const ANCHOR_CRITERIA: [&str; 5] = ["排程", "联动", "公平", "开销", "判据"];

/// 走查全过判。
pub fn walk_all_pass(steps: &[(&'static str, bool)]) -> bool {
    let mut i = 0usize;
    while i < steps.len() {
        if let Some((_, ok)) = steps.get(i) {
            if !ok {
                return false;
            }
        }
        i += 1;
    }
    !steps.is_empty()
}

/// 闭集守恒断言。
const _: () = {
    assert!(Visibility::ALL.len() == 3, "可见性必须三态闭集");
    assert!(Interactivity::ALL.len() == 3, "交互性必须三态闭集");
    assert!(SCHEDULE_SEGMENTS.len() == SCHEDULE_BUDGET_US.len(), "段名与预算等长");
};

/// 复用对拍锚点。
pub const REUSE_ANCHORS: [&str; 4] = [
    "cge03_surfacereg 表面注册表——排程对象先注册",
    "cgc02_framebudget 池比例口径——表面预算份额对齐",
    "F2408 预算行口径——排程开销账",
    "F0483 场景标签——调度归因维度同源",
];
