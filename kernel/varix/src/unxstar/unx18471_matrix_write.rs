//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18471 · 兼容层→矩阵写入路径 · 0x7F180段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18471-J1 写入矩阵依据字段=兼容层源断言全过，衔接 F18422 优先级第三级
//!
//! 设计要点：
//!   - 写入矩阵是「字段 × 兼容层源」的二维裁决闭集：每个（字段,源）对
//!     只有两个态——放行 / 拒收，矩阵整表写死并注释推导来源；
//!   - 源优先级三级承接 F18422：清单(manifest)=第一级、组策略(policy)=
//!     第二级、注册表蜂巢(hive)=第二级并位、兼容 API 调用(api_call)=
//!     **第三级**（F18422 优先级第三级承接点，常量 TIER_API_CALL==3 冻结）；
//!   - 字段状态机：Vacant → Held{tier} → Sealed，单调不可倒退；
//!     高优先源可压过低优先源的持有（tier 严格变小才许覆盖）；
//!   - 边界：未知字段/未知源/空值/超限值/封印后写/未写先封/重复封印
//!     七类一律显式拒，错误码闭集 code() = SEG_BASE + n；
//!   - 注入核：越权源写 / 低优先覆盖 / 封印后写 三类坏样本各 10 轮
//!     必抓（tries == caught，漏检必须为 0）；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 写入裁决矩阵 ADJ_MATRIX（6字段×4源位面账，u8 位掩码表）
//!   [状态机]   CellState::Vacant→Held→Sealed 三态单调推进
//!   [账本]     FieldCell 六槽字段账（值/持有源/写入计数/幂等命中）
//!   [错误路径] WxCode 12 码闭集 + detail() 中文建议 + ALL
//!   [注入核]   InjectLedger 三类坏样本 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F180，AI-VX11-1105 批04 独占区，A08 工单卡一）。
pub const SEG_BASE: u32 = 0x7F180;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 兼容层可写字段数（冻结：6； derivation：F18422 写面字段闭集全量）。
pub const FIELD_N: usize = 6;
/// 兼容层源数（冻结：4）。
pub const SOURCE_N: usize = 4;
/// 写入值上限（冻结：0xFFFF；字段值域均为 16 位配置槽）。
pub const VALUE_MAX: u32 = 0xFFFF;
/// 优先级第三级承接锚（冻结：3 = F18422 优先级第三级）。
pub const TIER_API_CALL: u8 = 3;
/// 判据注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// 可写字段闭集（与 F18422 写面字段一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    /// 兼容层总开关。
    LayerMode,
    /// DPI 覆写槽。
    DpiOverride,
    /// GDI 缩放策略槽。
    GdiScaling,
    /// 钩子策略槽。
    HookPolicy,
    /// 壳层覆盖标记槽。
    ShellOverlay,
    /// 持久化守卫槽。
    PersistGuard,
}

impl FieldId {
    /// 稳定槽位号（0..6），矩阵行下标的唯一合法来源。
    pub fn idx(self) -> usize {
        match self {
            FieldId::LayerMode => 0,
            FieldId::DpiOverride => 1,
            FieldId::GdiScaling => 2,
            FieldId::HookPolicy => 3,
            FieldId::ShellOverlay => 4,
            FieldId::PersistGuard => 5,
        }
    }
    pub const ALL: [FieldId; FIELD_N] = [
        FieldId::LayerMode,
        FieldId::DpiOverride,
        FieldId::GdiScaling,
        FieldId::HookPolicy,
        FieldId::ShellOverlay,
        FieldId::PersistGuard,
    ];
}

/// 兼容层源闭集（写入矩阵列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    /// 清单源：优先级第一级（最高权威）。
    Manifest,
    /// 组策略源：优先级第二级。
    PolicyGroup,
    /// 注册表蜂巢源：优先级第二级并位（与组策略同权，先到先占）。
    RegistryHive,
    /// 兼容 API 调用源：优先级第三级（F18422 承接）。
    ApiCall,
}

impl SourceKind {
    /// 源优先级（1..3，数字越小权威越高；api_call==3 冻结）。
    pub fn tier(self) -> u8 {
        match self {
            SourceKind::Manifest => 1,
            SourceKind::PolicyGroup => 2,
            SourceKind::RegistryHive => 2,
            SourceKind::ApiCall => TIER_API_CALL,
        }
    }
    /// 稳定列位号（0..4），矩阵列下标的唯一合法来源。
    pub fn col(self) -> usize {
        match self {
            SourceKind::Manifest => 0,
            SourceKind::PolicyGroup => 1,
            SourceKind::RegistryHive => 2,
            SourceKind::ApiCall => 3,
        }
    }
    pub const ALL: [SourceKind; SOURCE_N] = [
        SourceKind::Manifest,
        SourceKind::PolicyGroup,
        SourceKind::RegistryHive,
        SourceKind::ApiCall,
    ];
}

// ── 写入裁决矩阵（冻结整表） ────────────────────────────────────
/// 每行一个 u8，bit c = 源 SourceKind::ALL[c] 是否放行。
/// 推导（F18422 写面约束 + 最小写面原则）：
///   - LayerMode  0b0011：清单+组策略可写（总开关不开放给 hive/api）；
///   - DpiOverride 0b0111：清单+组策略+hive 可写（api 只读）；
///   - GdiScaling 0b1111：全源可写（调试期宽面，保留）；
///   - HookPolicy 0b0101：清单+hive 可写（组策略默认透传不落写面）；
///   - ShellOverlay 0b0110：组策略+hive 可写；
///   - PersistGuard 0b0001：仅清单可写（守卫槽最严）。
pub const ADJ_MATRIX: [u8; FIELD_N] = [
    0b0011, // LayerMode
    0b0111, // DpiOverride
    0b1111, // GdiScaling
    0b0101, // HookPolicy
    0b0110, // ShellOverlay
    0b0001, // PersistGuard
];

/// 查矩阵：该（字段,源）对是否放行。未知槽一律 false（边界收敛）。
pub fn matrix_allows(f: FieldId, s: SourceKind) -> bool {
    let row = match ADJ_MATRIX.get(f.idx()) {
        Some(r) => *r,
        None => return false,
    };
    (row >> s.col()) & 0x1 == 0x1
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WxCode {
    /// 未知字段（矩阵行越界）
    UnknownField,
    /// 未知源（矩阵列越界）
    UnknownSource,
    /// 矩阵拒收（该字段对该源无写权）
    MatrixDeny,
    /// 优先级拒收（同级或低级不许覆盖）
    PriorityDenied,
    /// 封印后写（Sealed 态锁死）
    SealedLocked,
    /// 未写先封（Vacant 态不许封）
    SealWithoutWrite,
    /// 空值写入
    EmptyValue,
    /// 值超限（> 0xFFFF）
    ValueOverflow,
    /// 重复封印
    DuplicateSeal,
    /// 守恒破坏（注入次数 ≠ 检出+漏检）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
}

impl WxCode {
    /// 诊断码 = SEG_BASE + 枚举序号（段内 0x40 窗口）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WxCode::UnknownField => "未知字段：字段闭集外，查 FieldId::ALL",
            WxCode::UnknownSource => "未知源：源闭集外，查 SourceKind::ALL",
            WxCode::MatrixDeny => "矩阵拒收：该字段对该源无写权，查 ADJ_MATRIX",
            WxCode::PriorityDenied => "优先级拒收：同级或低级不许覆盖已有持有",
            WxCode::SealedLocked => "封印后写：字段已封印，须走解封流程",
            WxCode::SealWithoutWrite => "未写先封：Vacant 态字段不许封印",
            WxCode::EmptyValue => "空值写入：0 值不是合法配置",
            WxCode::ValueOverflow => "值超限：0xFFFF 上限，查 VALUE_MAX",
            WxCode::DuplicateSeal => "重复封印：Sealed 态再封属误用",
            WxCode::ConservationBroken => "守恒破坏：注入≠检出+漏检",
            WxCode::NotReproducible => "双跑不一致：复现红线",
            WxCode::SegmentOutside => "段位越窗：诊断码不在 0x7F180 段",
        }
    }
    pub const ALL: [WxCode; 12] = [
        WxCode::UnknownField,
        WxCode::UnknownSource,
        WxCode::MatrixDeny,
        WxCode::PriorityDenied,
        WxCode::SealedLocked,
        WxCode::SealWithoutWrite,
        WxCode::EmptyValue,
        WxCode::ValueOverflow,
        WxCode::DuplicateSeal,
        WxCode::ConservationBroken,
        WxCode::NotReproducible,
        WxCode::SegmentOutside,
    ];
}

/// 段内任意码 → 中文明细（闭集外给兜底文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < WxCode::ALL.len() {
        if let Some(c) = WxCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F180 段闭集内"
}

// ── 字段状态机 ─────────────────────────────────────────────────
/// 字段三态：Vacant（未写）→ Held（有主）→ Sealed（封印）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    Vacant,
    Held,
    Sealed,
}

/// 单字段账（六槽之一）。
#[derive(Clone, Copy, Debug)]
pub struct FieldCell {
    pub state: CellState,
    /// 当前生效值（Vacant 态为 0 哨兵）。
    pub value: u32,
    /// 持有源优先级（Vacant 态为 u8::MAX 哨兵=最弱）。
    pub holder_tier: u8,
    /// 接受写入次数。
    pub writes: u32,
    /// 幂等重放命中次数（同源同值重复写）。
    pub idem_hits: u32,
}

impl FieldCell {
    pub const fn empty() -> FieldCell {
        FieldCell {
            state: CellState::Vacant,
            value: 0,
            holder_tier: u8::MAX,
            writes: 0,
            idem_hits: 0,
        }
    }
}

// ── 注入检出账 ─────────────────────────────────────────────────
/// 三类坏样本 ×（尝试/检出）二维账。漏检必须恒 0。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮：slot 越界静默不入账（边界收敛，不 panic）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    /// 守恒：注入次数 = 检出次数 + 漏检次数，且漏检为 0。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            let (t, c) = match (self.tries.get(i), self.caught.get(i)) {
                (Some(t), Some(c)) => (*t, *c),
                _ => return Err(WxCode::ConservationBroken.code()),
            };
            if t == 0 || c != t {
                return Err(WxCode::ConservationBroken.code());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 写入路径总成 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WxEffect {
    /// 全新持有。
    Accepted,
    /// 压过更低优先级持有者。
    Overrode,
    /// 同源同值幂等重放。
    Idempotent,
}

pub struct WriteMatrix {
    pub cells: [FieldCell; FIELD_N],
    pub total_writes: u32,
    pub rejected: u32,
    pub inj: InjectLedger,
}

impl WriteMatrix {
    pub fn new() -> WriteMatrix {
        WriteMatrix {
            cells: [
                FieldCell::empty(),
                FieldCell::empty(),
                FieldCell::empty(),
                FieldCell::empty(),
                FieldCell::empty(),
                FieldCell::empty(),
            ],
            total_writes: 0,
            rejected: 0,
            inj: InjectLedger::new(),
        }
    }

    fn cell(&self, f: FieldId) -> &FieldCell {
        // idx() 闭集内必在 0..6；get 兜底取第 0 槽 Vacant 哨兵（不可达分支）。
        // 禁裸下标：get 返回引用借用期安全，双 get 兜底链。
        match self.cells.get(f.idx()) {
            Some(c) => c,
            None => match self.cells.get(0) {
                Some(c0) => c0,
                None => {
                    // FIELD_N==6 恒真，静态不可达；返回静态哨兵零 panic。
                    static SENTINEL: FieldCell = FieldCell::empty();
                    &SENTINEL
                }
            },
        }
    }

    /// 写入口：矩阵裁决 → 优先级裁决 → 状态机推进。零 panic。
    pub fn write(&mut self, f: FieldId, s: SourceKind, v: u32) -> Result<WxEffect, u32> {
        // 值域闸（先于矩阵：值非法无裁决意义）。
        if v == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(WxCode::EmptyValue.code());
        }
        if v > VALUE_MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(WxCode::ValueOverflow.code());
        }
        // 矩阵闸：字段=源 断言。
        if !matrix_allows(f, s) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(WxCode::MatrixDeny.code());
        }
        let i = f.idx();
        // 闭集内必命中；越界按状态机不可达处理（Vacant 分支兜底）。
        let (state, held_tier, held_val, cur_writes, cur_idem) = match self.cells.get(i) {
            Some(c) => (c.state, c.holder_tier, c.value, c.writes, c.idem_hits),
            None => return Err(WxCode::UnknownField.code()),
        };
        match state {
            CellState::Sealed => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(WxCode::SealedLocked.code());
            }
            CellState::Held => {
                if held_val == v {
                    // 同值重放：不论源，一律幂等（写面确定性）。
                    if let Some(c) = self.cells.get_mut(i) {
                        c.idem_hits = cur_idem.saturating_add(1);
                    }
                    return Ok(WxEffect::Idempotent);
                }
                if s.tier() >= held_tier {
                    // 同级或低级不许压（严格变小才许覆盖）。
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(WxCode::PriorityDenied.code());
                }
                if let Some(c) = self.cells.get_mut(i) {
                    c.value = v;
                    c.holder_tier = s.tier();
                    c.writes = cur_writes.saturating_add(1);
                }
                self.total_writes = self.total_writes.saturating_add(1);
                Ok(WxEffect::Overrode)
            }
            CellState::Vacant => {
                if let Some(c) = self.cells.get_mut(i) {
                    c.state = CellState::Held;
                    c.value = v;
                    c.holder_tier = s.tier();
                    c.writes = cur_writes.saturating_add(1);
                }
                self.total_writes = self.total_writes.saturating_add(1);
                Ok(WxEffect::Accepted)
            }
        }
    }

    /// 封印：只许 Held → Sealed，单调不可倒退。
    pub fn seal(&mut self, f: FieldId) -> Result<(), u32> {
        let i = f.idx();
        let st = match self.cells.get(i) {
            Some(c) => c.state,
            None => return Err(WxCode::UnknownField.code()),
        };
        match st {
            CellState::Vacant => Err(WxCode::SealWithoutWrite.code()),
            CellState::Sealed => Err(WxCode::DuplicateSeal.code()),
            CellState::Held => {
                if let Some(c) = self.cells.get_mut(i) {
                    c.state = CellState::Sealed;
                }
                Ok(())
            }
        }
    }

    /// 只读快照：值与持有级（未写返回 None）。
    pub fn peek(&self, f: FieldId) -> Option<(u32, u8)> {
        let c = self.cell(f);
        if c.state == CellState::Vacant {
            None
        } else {
            Some((c.value, c.holder_tier))
        }
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────
/// 六字段按矩阵合法源各写一次 → 断言全过 → 逐字段封印。
/// 返回（接受数, 覆盖数, 幂等数）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut m = WriteMatrix::new();
    // 每字段用矩阵放行的第一源写入（从冻结矩阵推导，不硬编码例外）。
    let mut accepted = 0u32;
    let mut i = 0;
    while i < FIELD_N {
        let f = match FieldId::ALL.get(i) {
            Some(f) => *f,
            None => return Err(WxCode::UnknownField.code()),
        };
        // 找矩阵放行的最高优先源。
        let mut k = 0;
        let mut chosen: Option<SourceKind> = None;
        while k < SOURCE_N {
            let s = match SourceKind::ALL.get(k) {
                Some(s) => *s,
                None => break,
            };
            if matrix_allows(f, s) {
                chosen = Some(s);
                break;
            }
            k += 1;
        }
        let s = match chosen {
            Some(s) => s,
            None => return Err(WxCode::MatrixDeny.code()),
        };
        // 值推导：槽位号 * 0x1111，保证落在 0xFFFF 域内且全绿。
        let v = (i as u32).saturating_mul(0x1111) + 0x1111;
        match m.write(f, s, v)? {
            WxEffect::Accepted => accepted = accepted.saturating_add(1),
            _ => return Err(WxCode::NotReproducible.code()),
        }
        m.seal(f)?;
        i += 1;
    }
    if accepted != FIELD_N as u32 {
        return Err(WxCode::ConservationBroken.code());
    }
    Ok((accepted, 0, 0))
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：越权源写（矩阵拒收对）。PersistGuard 仅清单可写，
/// 用 hive/api_call 打进去必须被 MatrixDeny 拦下。
/// 注入二：低优先覆盖。先 manifest 持有，再 api_call 写异值
/// 必须被 PriorityDenied 拦下。
/// 注入三：封印后写。合法写后封印，再写必被 SealedLocked 拦下。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // —— 注入一：越权源（hive / api_call 打 PersistGuard）——
        let mut m1 = WriteMatrix::new();
        let by_hive = m1.write(FieldId::PersistGuard, SourceKind::RegistryHive, 0x0001);
        let by_api = m1.write(FieldId::PersistGuard, SourceKind::ApiCall, 0x0002);
        let hit1 = by_hive.err() == Some(WxCode::MatrixDeny.code())
            && by_api.err() == Some(WxCode::MatrixDeny.code())
            && m1.peek(FieldId::PersistGuard).is_none();
        led.record(0, hit1);
        // —— 注入二：低优先覆盖 ——
        // 选 GdiScaling（矩阵行 0b1111 全源放行）：ApiCall 能穿过矩阵闸，
        // 才能撞到优先级闸（DpiOverride 行 0b0111 会先吃 MatrixDeny）。
        let mut m2 = WriteMatrix::new();
        let _ = m2.write(FieldId::GdiScaling, SourceKind::Manifest, 0x0100);
        let low = m2.write(FieldId::GdiScaling, SourceKind::ApiCall, 0x0200);
        let hit2 = low.err() == Some(WxCode::PriorityDenied.code());
        // 持有值未被低级源污染。
        let clean2 = match m2.peek(FieldId::GdiScaling) {
            Some((v, _t)) => v == 0x0100,
            None => false,
        };
        led.record(1, hit2 && clean2);
        // —— 注入三：封印后写 ——
        let mut m3 = WriteMatrix::new();
        let _ = m3.write(FieldId::GdiScaling, SourceKind::Manifest, 0x0011);
        let _ = m3.seal(FieldId::GdiScaling);
        let after = m3.write(FieldId::GdiScaling, SourceKind::Manifest, 0x0022);
        let hit3 = after.err() == Some(WxCode::SealedLocked.code());
        led.record(2, hit3);
        r = r.saturating_add(1);
    }
    led.conserve()?;
    Ok(led)
}

/// 双跑确定性：同输入两次 golden_run 逐值相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WxCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：闭集全部落在本段 0x40 窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WxCode::ALL.len() {
        if let Some(c) = WxCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    // SEG 对齐 0x40 且落在本批次 0x7F000..0x7F400 窗口。
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 探针一：合法链（hive 写 DpiOverride 接受，api 幂等重放）。
pub fn probe_chain() -> Result<(), u32> {
    let mut m = WriteMatrix::new();
    let a = m.write(FieldId::DpiOverride, SourceKind::RegistryHive, 0x00FF)?;
    let b = m.write(FieldId::DpiOverride, SourceKind::RegistryHive, 0x00FF)?;
    let c = m.write(FieldId::DpiOverride, SourceKind::Manifest, 0x00EE)?;
    if a != WxEffect::Accepted || b != WxEffect::Idempotent || c != WxEffect::Overrode {
        return Err(WxCode::NotReproducible.code());
    }
    Ok(())
}

/// 探针二：未写先封 / 重复封印 双拒。
pub fn probe_seal_edges() -> Result<(), u32> {
    let mut m = WriteMatrix::new();
    let e1 = m.seal(FieldId::HookPolicy).err();
    let _ = m.write(FieldId::HookPolicy, SourceKind::Manifest, 0x0007);
    let _ = m.seal(FieldId::HookPolicy)?;
    let e2 = m.seal(FieldId::HookPolicy).err();
    if e1 != Some(WxCode::SealWithoutWrite.code())
        || e2 != Some(WxCode::DuplicateSeal.code())
    {
        return Err(WxCode::NotReproducible.code());
    }
    Ok(())
}

/// 探针三：矩阵全表对账——六行四列逐项复算（位面=裁决表一致性）。
pub fn probe_matrix_audit() -> Result<u32, u32> {
    let mut allow_count = 0u32;
    let mut fi = 0;
    while fi < FIELD_N {
        let row = match ADJ_MATRIX.get(fi) {
            Some(r) => *r,
            None => return Err(WxCode::UnknownField.code()),
        };
        // 行号 → 字段闭集绑定（双通道复算需要字段实参）。
        let f = match FieldId::ALL.get(fi) {
            Some(v) => *v,
            None => return Err(WxCode::UnknownField.code()),
        };
        let mut si = 0;
        while si < SOURCE_N {
            let s = match SourceKind::ALL.get(si) {
                Some(s) => *s,
                None => break,
            };
            let bit = (row >> si) & 0x1 == 0x1;
            // 双通道复算：位面直接取 与 matrix_allows 查询必须一致。
            if bit != matrix_allows(f, s) {
                return Err(WxCode::NotReproducible.code());
            }
            if bit {
                allow_count = allow_count.saturating_add(1);
            }
            si += 1;
        }
        fi += 1;
    }
    // 冻结推导：0b0011+0b0111+0b1111+0b0101+0b0110+0b0001 = 2+3+4+2+2+1 = 14。
    if allow_count != 14 {
        return Err(WxCode::ConservationBroken.code());
    }
    Ok(allow_count)
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18471 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18471_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18471");
    // 1. 矩阵冻结：6 行 × 4 源、放行总数 14（位面推导冻结）。
    s.add("J1-矩阵冻结6x4", FIELD_N == 6 && SOURCE_N == 4, "字段6源4闭集");
    s.add("J1-矩阵放行14格", probe_matrix_audit().is_ok(), "位面与裁决通道逐格一致");
    // 2. 优先级第三级承接：api_call tier == 3（F18422 衔接锚）。
    s.add(
        "J1-第三级承接",
        SourceKind::ApiCall.tier() == TIER_API_CALL && TIER_API_CALL == 3,
        "api_call=优先级第三级冻结",
    );
    // 3. 黄金全链：六字段合法源写+封印全过。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!("接受/覆盖/幂等={:?}", g),
    );
    // 4. 合法链探针：接受→幂等→覆盖三效应。
    s.add("J1-三效应链", probe_chain().is_ok(), "Accepted→Idempotent→Overrode");
    // 5. 封印边界：未写先封、重复封印双拒。
    s.add("J1-封印双拒", probe_seal_edges().is_ok(), "SealWithoutWrite/DuplicateSeal");
    // 6. 注入三类 ×10 轮全抓。
    let led = inject_rounds(INJECT_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.conserve().is_ok(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入三类10/10",
        led_ok,
        &format!(
            "t={:?} c={:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    );
    // 7. 值域边界：0 值拒 / 超限拒 / 上限值收。
    {
        let mut m = WriteMatrix::new();
        let e0 = m.write(FieldId::LayerMode, SourceKind::Manifest, 0).err();
        let ov = m
            .write(FieldId::LayerMode, SourceKind::Manifest, VALUE_MAX + 1)
            .err();
        let okv = m.write(FieldId::LayerMode, SourceKind::Manifest, VALUE_MAX);
        s.add(
            "J1-值域三边界",
            e0 == Some(WxCode::EmptyValue.code())
                && ov == Some(WxCode::ValueOverflow.code())
                && okv.is_ok(),
            "0拒/超限拒/上限收",
        );
    }
    // 8. 守卫槽最严：PersistGuard 非清单源全拒。
    {
        let mut m = WriteMatrix::new();
        let a = m.write(FieldId::PersistGuard, SourceKind::PolicyGroup, 1).is_err();
        let b = m.write(FieldId::PersistGuard, SourceKind::RegistryHive, 1).is_err();
        let c = m.write(FieldId::PersistGuard, SourceKind::ApiCall, 1).is_err();
        let d = m.write(FieldId::PersistGuard, SourceKind::Manifest, 1).is_ok();
        s.add("J1-守卫槽最严", a && b && c && d, "PersistGuard 仅清单放行");
    }
    // 9. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 10. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F180 窗口对齐 0x40");
    // 11. 诊断码闭集：全码明细非空。
    {
        let mut all_nonempty = true;
        let mut i = 0;
        while i < WxCode::ALL.len() {
            if let Some(c) = WxCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    all_nonempty = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", all_nonempty, "12 码中文明细全非空");
    }
    // 12. 账本守恒：黄金链后 total_writes==6 且封印 6 格。
    {
        let mut m = WriteMatrix::new();
        let mut i = 0;
        while i < FIELD_N {
            if let Some(f) = FieldId::ALL.get(i) {
                let f = *f;
                // 找放行首源写一次并封印。
                let mut k = 0;
                while k < SOURCE_N {
                    if let Some(src) = SourceKind::ALL.get(k) {
                        let src = *src;
                        if matrix_allows(f, src) {
                            let v = (i as u32).saturating_mul(0x1111) + 0x1111;
                            let _ = m.write(f, src, v);
                            let _ = m.seal(f);
                            break;
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        let sealed = {
            let mut n = 0u32;
            let mut j = 0;
            while j < FIELD_N {
                if let Some(f) = FieldId::ALL.get(j) {
                    if let Some(c) = m.cells.get(f.idx()) {
                        if c.state == CellState::Sealed {
                            n = n.saturating_add(1);
                        }
                    }
                }
                j += 1;
            }
            n
        };
        s.add(
            "J1-账本守恒",
            m.total_writes == FIELD_N as u32 && sealed == FIELD_N as u32,
            "写6封6账平",
        );
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        assert!(probe_chain().is_ok());
    }

    #[test]
    fn t02_boundary_value_domain() {
        let mut m = WriteMatrix::new();
        assert_eq!(
            m.write(FieldId::LayerMode, SourceKind::Manifest, 0).err(),
            Some(WxCode::EmptyValue.code())
        );
        assert_eq!(
            m.write(FieldId::LayerMode, SourceKind::Manifest, 0x1_0000).err(),
            Some(WxCode::ValueOverflow.code())
        );
        assert!(m.write(FieldId::LayerMode, SourceKind::Manifest, 0xFFFF).is_ok());
    }

    #[test]
    fn t03_inject_unauthorized_source() {
        let mut m = WriteMatrix::new();
        assert_eq!(
            m.write(FieldId::PersistGuard, SourceKind::ApiCall, 1).err(),
            Some(WxCode::MatrixDeny.code())
        );
    }

    #[test]
    fn t04_inject_low_priority_override() {
        // GdiScaling 全源放行：ApiCall 过矩阵闸后撞优先级闸。
        let mut m = WriteMatrix::new();
        let _ = m.write(FieldId::GdiScaling, SourceKind::Manifest, 0x100);
        assert_eq!(
            m.write(FieldId::GdiScaling, SourceKind::ApiCall, 0x200).err(),
            Some(WxCode::PriorityDenied.code())
        );
    }

    #[test]
    fn t05_inject_write_after_seal() {
        let mut m = WriteMatrix::new();
        let _ = m.write(FieldId::GdiScaling, SourceKind::Manifest, 1);
        let _ = m.seal(FieldId::GdiScaling);
        assert_eq!(
            m.write(FieldId::GdiScaling, SourceKind::Manifest, 2).err(),
            Some(WxCode::SealedLocked.code())
        );
    }

    #[test]
    fn t06_state_machine_order() {
        let mut m = WriteMatrix::new();
        assert_eq!(
            m.seal(FieldId::HookPolicy).err(),
            Some(WxCode::SealWithoutWrite.code())
        );
        let _ = m.write(FieldId::HookPolicy, SourceKind::Manifest, 5);
        let _ = m.seal(FieldId::HookPolicy);
        assert_eq!(
            m.seal(FieldId::HookPolicy).err(),
            Some(WxCode::DuplicateSeal.code())
        );
    }

    #[test]
    fn t07_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t08_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t09_checks_all_green() {
        let set = run_unx18471_checks();
        assert!(set.all_passed(), "UNX-F18471 自检须全绿：{}", set.red_items_text());
    }
}
