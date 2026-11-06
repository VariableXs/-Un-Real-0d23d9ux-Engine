//! UNX-F50048 水印与双面组合（背面自动淡化档）（0x43220 段）
//!
//! 锚点：UNX-F50048-J1 背面淡化 ×10 一致；判据锚定 §AI-63 §1 防重声明（B24 联签）。
//!
//! 双面印刷时背面水印有两个硬约束：① 背面与正面共用同一张纸，背面水印会
//! 透过纸张在正面形成「透印」，因此背面必须**自动淡化**（档位化，而非临时
//! 手调）；② 短边翻页（ShortEdge）会把背面旋转 180°，水印锚点必须同步镜像，
//! 否则装订后背面水印会跑到切口一侧。
//!
//! 淡化用千分整数：`back = base × factor / 1000`，factor ∈ {1000, 800, 620,
//! 450} 对应 {Off, Light, Medium, Strong}。Off 档是「显式登记不淡化」而不是
//! 「悄悄跳过」——账目里仍然写入与正面相等的背面不透明度。
//!
//! 不变量（J1 的判据基础）：
//! * 同一份作业里所有「张」的背面不透明度必须**逐张相等**（同档同基值）；
//! * 背面永不变亮：`back ≤ front` 对任意合法基值与档位成立；
//! * 档位因子严格单调：Off > Light > Medium > Strong；
//! * 短边镜像互补：`x + x' + w == page_w`；长边原样。
//!
//! 数据段 `SEG_BASE = 0x43220`，诊断码闭集 `0x43221..=0x4322a`（10 条）。
//! 结构清单：`DuplexMode` / `FadeTier`（两套码闭集）、`SideStamp`、
//! `DuplexSheetPlan`（账目条目）、`DuplexStage`（只前进的状态机）、
//! `DuplexLedger`（防重 + 定容台账）、`plan_document`（计划器）、
//! `injection_core`（10 类坏样本注入核）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x43220）。
pub const SEG_BASE: u32 = 0x43220;

pub const E_EMPTY: u16 = 1;
pub const E_MODE: u16 = 2;
pub const E_ODD: u16 = 3;
pub const E_OPACITY: u16 = 4;
pub const E_TIER: u16 = 5;
pub const E_STAGE: u16 = 6;
pub const E_DUP: u16 = 7;
pub const E_CAP: u16 = 8;
pub const E_MIRROR: u16 = 9;
pub const E_PARAM: u16 = 10;

/// 双面水印域错误。`code()` 落在 `SEG_BASE + n` 闭集内。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexError {
    /// 页数为 0
    Empty,
    /// 翻页模式码非法
    Mode,
    /// 双面作业页数为奇数（本模块不替调用方补空白页，显性拒）
    PagesOdd,
    /// 基不透明度超过 1000‰
    OpacityOut,
    /// 淡化档位码非法
    FadeTier,
    /// 状态机倒退或跳级
    StageRegress,
    /// 同一张重复入账，或正反面页号相同
    Duplicate,
    /// 台账容量满
    Capacity,
    /// 镜像锚点越出纸面
    Mirror,
    /// 纸面尺寸/印章宽高非正
    Param,
}

impl DuplexError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            DuplexError::Empty => E_EMPTY,
            DuplexError::Mode => E_MODE,
            DuplexError::PagesOdd => E_ODD,
            DuplexError::OpacityOut => E_OPACITY,
            DuplexError::FadeTier => E_TIER,
            DuplexError::StageRegress => E_STAGE,
            DuplexError::Duplicate => E_DUP,
            DuplexError::Capacity => E_CAP,
            DuplexError::Mirror => E_MIRROR,
            DuplexError::Param => E_PARAM,
        };
        SEG_BASE + n as u32
    }

    /// 一句话诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            DuplexError::Empty => "no pages to plan",
            DuplexError::Mode => "duplex mode code invalid",
            DuplexError::PagesOdd => "duplex job has odd page count",
            DuplexError::OpacityOut => "base opacity over 1000 permille",
            DuplexError::FadeTier => "fade tier code invalid",
            DuplexError::StageRegress => "stage regression or skip",
            DuplexError::Duplicate => "duplicate sheet or page pair",
            DuplexError::Capacity => "ledger capacity exhausted",
            DuplexError::Mirror => "mirrored anchor leaves the sheet",
            DuplexError::Param => "sheet/stamp size not positive",
        }
    }
}

/// 翻页模式：单面 / 长边翻 / 短边翻。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexMode {
    Simplex,
    LongEdge,
    ShortEdge,
}

impl DuplexMode {
    /// 模式码闭集：0=单面，1=长边，2=短边，其余拒。
    pub fn from_u8(v: u8) -> Result<DuplexMode, DuplexError> {
        match v {
            0 => Ok(DuplexMode::Simplex),
            1 => Ok(DuplexMode::LongEdge),
            2 => Ok(DuplexMode::ShortEdge),
            _ => Err(DuplexError::Mode),
        }
    }

    /// 模式码。
    pub fn as_u8(self) -> u8 {
        match self {
            DuplexMode::Simplex => 0,
            DuplexMode::LongEdge => 1,
            DuplexMode::ShortEdge => 2,
        }
    }

    /// 是否双面（每张两面）。
    pub fn is_duplex(self) -> bool {
        match self {
            DuplexMode::Simplex => false,
            DuplexMode::LongEdge | DuplexMode::ShortEdge => true,
        }
    }

    /// 背面是否需要水平镜像锚点：短边翻页绕纸边转 180°，长边翻页不转。
    pub fn flips_back(self) -> bool {
        match self {
            DuplexMode::ShortEdge => true,
            DuplexMode::Simplex | DuplexMode::LongEdge => false,
        }
    }
}

/// 背面淡化档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeTier {
    /// 显式关闭淡化（背面与正面同不透明度）
    Off,
    /// 轻度淡化 0.80
    Light,
    /// 中度淡化 0.62（默认推荐：肉眼几乎无透印，背面仍可辨识）
    Medium,
    /// 强力淡化 0.45
    Strong,
}

impl FadeTier {
    /// 档位码闭集：0..=3，其余拒。
    pub fn from_u8(v: u8) -> Result<FadeTier, DuplexError> {
        match v {
            0 => Ok(FadeTier::Off),
            1 => Ok(FadeTier::Light),
            2 => Ok(FadeTier::Medium),
            3 => Ok(FadeTier::Strong),
            _ => Err(DuplexError::FadeTier),
        }
    }

    /// 档位码。
    pub fn as_u8(self) -> u8 {
        match self {
            FadeTier::Off => 0,
            FadeTier::Light => 1,
            FadeTier::Medium => 2,
            FadeTier::Strong => 3,
        }
    }

    /// 背面不透明度系数（千分比）。数值来自印刷车间经验：
    /// 0.80 仍能看清反面，0.62 是 80g 双胶纸的透印临界点，0.45 用于
    /// 薄纸/彩印背面。
    pub fn factor_permille(self) -> u16 {
        match self {
            FadeTier::Off => 1000,
            FadeTier::Light => 800,
            FadeTier::Medium => 620,
            FadeTier::Strong => 450,
        }
    }
}

/// 一面水印的落位参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideStamp {
    pub opacity: u16,
    pub mirror_x: bool,
    pub x: i32,
}

/// 一张纸的双面计划（单面模式 `back_page`/`back` 为 None）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplexSheetPlan {
    pub sheet: u16,
    pub front_page: u16,
    pub back_page: Option<u16>,
    pub front: SideStamp,
    pub back: Option<SideStamp>,
}

/// 背面不透明度：`base × factor / 1000`，并保证「有水印就至少 1‰」——
/// 否则 base=1 在 Strong 档会掉到 0，等于背面水印凭空消失。
pub fn back_opacity(base: u16, tier: FadeTier) -> Result<u16, DuplexError> {
    if base > 1000 {
        return Err(DuplexError::OpacityOut);
    }
    let raw = base as u32 * tier.factor_permille() as u32 / 1000;
    let mut v = raw;
    if base > 0 && v == 0 {
        v = 1;
    }
    if v > 1000 {
        v = 1000;
    }
    Ok(v as u16)
}

/// 镜像锚点：短边翻页时 `x' = page_w - x - w`，其余原样。
/// 越界（含镜像后越界）显性拒，绝不裁剪到纸内假装成功。
pub fn mirror_anchor(
    mode: DuplexMode,
    x: i32,
    w: i32,
    page_w: i32,
) -> Result<i32, DuplexError> {
    if page_w <= 0 || w <= 0 {
        return Err(DuplexError::Param);
    }
    if x < 0 || x + w > page_w {
        return Err(DuplexError::Mirror);
    }
    if mode.flips_back() {
        Ok(page_w - x - w)
    } else {
        Ok(x)
    }
}

/// 计划器：生成每张纸的双面水印参数。
///
/// 页号 0 基；双面模式按 (前 2k, 后 2k+1) 配对。奇数页直接拒——补空白页是
/// 调用方的排版决策，模块不替它做主。
pub fn plan_document(
    page_count: u16,
    mode: DuplexMode,
    base_opacity: u16,
    tier: FadeTier,
    page_w: i32,
    page_h: i32,
    anchor_x: i32,
    stamp_w: i32,
) -> Result<Vec<DuplexSheetPlan>, DuplexError> {
    if page_count == 0 {
        return Err(DuplexError::Empty);
    }
    if page_w <= 0 || page_h <= 0 || stamp_w <= 0 {
        return Err(DuplexError::Param);
    }
    if base_opacity > 1000 {
        return Err(DuplexError::OpacityOut);
    }
    if mode.is_duplex() && page_count % 2 != 0 {
        return Err(DuplexError::PagesOdd);
    }
    // 正面永远是「原锚点」：只做越界校验，不做翻转。
    let front_x = mirror_anchor(DuplexMode::LongEdge, anchor_x, stamp_w, page_w)?;
    // 背面按翻页模式镜像（长边档与正面同锚点）。
    let back_x = mirror_anchor(mode, anchor_x, stamp_w, page_w)?;
    let faded = back_opacity(base_opacity, tier)?;

    let mut out: Vec<DuplexSheetPlan> = Vec::new();
    if mode.is_duplex() {
        let sheets = page_count / 2;
        let mut k = 0u16;
        while k < sheets {
            let front_page = k * 2;
            let back_page = k * 2 + 1;
            out.push(DuplexSheetPlan {
                sheet: k,
                front_page,
                back_page: Some(back_page),
                front: SideStamp {
                    opacity: base_opacity,
                    mirror_x: false,
                    x: front_x,
                },
                back: Some(SideStamp {
                    opacity: faded,
                    mirror_x: mode.flips_back(),
                    x: back_x,
                }),
            });
            k += 1;
        }
    } else {
        let mut k = 0u16;
        while k < page_count {
            out.push(DuplexSheetPlan {
                sheet: k,
                front_page: k,
                back_page: None,
                front: SideStamp {
                    opacity: base_opacity,
                    mirror_x: false,
                    x: anchor_x,
                },
                back: None,
            });
            k += 1;
        }
    }
    Ok(out)
}

/// 计划指纹（判据用它做确定性对平）。
pub fn plan_digest(plans: &[DuplexSheetPlan]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < plans.len() {
        if let Some(p) = plans.get(i) {
            h = (h ^ p.sheet as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ p.front_page as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ p.front.opacity as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ p.front.x as u32 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            if let Some(b) = p.back {
                h = (h ^ b.opacity as u64).wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ b.x as u32 as u64).wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ 0x9e37u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        i += 1;
    }
    h
}

/// 计划摘要（日志/提示用）。
pub fn plan_summary(plans: &[DuplexSheetPlan]) -> String {
    let mut backs = 0usize;
    let mut i = 0usize;
    while i < plans.len() {
        if let Some(p) = plans.get(i) {
            if p.back.is_some() {
                backs += 1;
            }
        }
        i += 1;
    }
    format!("sheets={} backs={}", plans.len(), backs)
}

/// 双面作业阶段。只许前进或同阶段幂等；倒退与跳级都拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexStage {
    Draft,
    Planned,
    Emitted,
    Sealed,
}

impl DuplexStage {
    /// 阶段序（用于单调比较）。
    pub fn rank(self) -> u8 {
        match self {
            DuplexStage::Draft => 0,
            DuplexStage::Planned => 1,
            DuplexStage::Emitted => 2,
            DuplexStage::Sealed => 3,
        }
    }

    /// 迁移：同阶段幂等（重试安全），前进一格允许，其余一律 `StageRegress`。
    pub fn advance(self, to: DuplexStage) -> Result<DuplexStage, DuplexError> {
        let from = self.rank();
        let t = to.rank();
        if t == from {
            return Ok(self);
        }
        if t == from + 1 {
            return Ok(to);
        }
        Err(DuplexError::StageRegress)
    }
}

/// 台账容量（16 张纸，覆盖 32 页以内的常见作业）。
pub const LEDGER_CAP: usize = 16;

/// 双面落位台账：定容数组 + 状态机 + 防重账目。
pub struct DuplexLedger {
    entries: [Option<DuplexSheetPlan>; LEDGER_CAP],
    len: usize,
    stage: DuplexStage,
    rejected: u16,
    digest: u64,
}

impl DuplexLedger {
    /// 空台账（Draft 阶段）。
    pub const fn new() -> DuplexLedger {
        DuplexLedger {
            entries: [None; LEDGER_CAP],
            len: 0,
            stage: DuplexStage::Draft,
            rejected: 0,
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> DuplexStage {
        self.stage
    }

    /// 已入账张数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 被拒条数。
    pub fn rejected(&self) -> u16 {
        self.rejected
    }

    /// 台账指纹。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 推进阶段；倒退/跳级被拒。
    pub fn advance(&mut self, to: DuplexStage) -> Result<(), DuplexError> {
        match self.stage.advance(to) {
            Ok(next) => {
                self.stage = next;
                Ok(())
            }
            Err(e) => {
                self.rejected += 1;
                Err(e)
            }
        }
    }

    /// 入账一张；Draft 阶段不许入账（乱序拒），同张或同页对重复拒，容量满拒。
    pub fn push(&mut self, plan: DuplexSheetPlan) -> Result<(), DuplexError> {
        if self.stage.rank() < DuplexStage::Planned.rank() {
            self.rejected += 1;
            return Err(DuplexError::StageRegress);
        }
        if let Some(b) = plan.back_page {
            if b == plan.front_page {
                self.rejected += 1;
                return Err(DuplexError::Duplicate);
            }
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i).and_then(|v| *v) {
                if e.sheet == plan.sheet
                    || e.front_page == plan.front_page
                    || (e.back_page.is_some() && e.back_page == plan.back_page)
                {
                    self.rejected += 1;
                    return Err(DuplexError::Duplicate);
                }
            }
            i += 1;
        }
        if self.len >= LEDGER_CAP {
            self.rejected += 1;
            return Err(DuplexError::Capacity);
        }
        if let Some(slot) = self.entries.get_mut(self.len) {
            *slot = Some(plan);
            self.len += 1;
        }
        self.digest = (self.digest ^ plan.sheet as u64).wrapping_mul(0x0000_0100_0000_01b3);
        self.digest = (self.digest ^ plan.front.opacity as u64).wrapping_mul(0x0000_0100_0000_01b3);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 注入核：10 类坏样本逐类命中指定诊断码
// ---------------------------------------------------------------------------

/// 构造 10 类坏样本并逐类断言诊断码，返回 (命中数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let total = 10usize;
    let mut caught = 0usize;

    // ① 空作业
    if plan_document(0, DuplexMode::LongEdge, 300, FadeTier::Medium, 595_000, 842_000, 0, 1000)
        .err()
        == Some(DuplexError::Empty)
    {
        caught += 1;
    }
    // ② 模式码非法
    if DuplexMode::from_u8(9).err() == Some(DuplexError::Mode) {
        caught += 1;
    }
    // ③ 双面奇数页
    if plan_document(7, DuplexMode::LongEdge, 300, FadeTier::Medium, 595_000, 842_000, 0, 1000)
        .err()
        == Some(DuplexError::PagesOdd)
    {
        caught += 1;
    }
    // ④ 基不透明度越界
    if back_opacity(1001, FadeTier::Light).err() == Some(DuplexError::OpacityOut) {
        caught += 1;
    }
    // ⑤ 档位码非法
    if FadeTier::from_u8(7).err() == Some(DuplexError::FadeTier) {
        caught += 1;
    }
    // ⑥ 镜像锚点越出纸面
    if mirror_anchor(DuplexMode::ShortEdge, 500_000, 200_000, 595_000).err()
        == Some(DuplexError::Mirror)
    {
        caught += 1;
    }
    // ⑦ 纸面尺寸非正
    if plan_document(4, DuplexMode::LongEdge, 300, FadeTier::Medium, 0, 842_000, 0, 1000)
        .err()
        == Some(DuplexError::Param)
    {
        caught += 1;
    }
    // ⑧ Draft 阶段入账（乱序）
    let mut led = DuplexLedger::new();
    let sample = DuplexSheetPlan {
        sheet: 0,
        front_page: 0,
        back_page: Some(1),
        front: SideStamp { opacity: 300, mirror_x: false, x: 10 },
        back: Some(SideStamp { opacity: 186, mirror_x: true, x: 10 }),
    };
    if led.push(sample).err() == Some(DuplexError::StageRegress) {
        caught += 1;
    }
    // ⑨ 重复张 + ⑩ 容量
    if led.advance(DuplexStage::Planned).is_ok() {
        let dup_hit = led.push(sample).is_ok() && led.push(sample).err() == Some(DuplexError::Duplicate);
        let mut cap_hit = false;
        let mut k = 1u16;
        while k < 20 {
            let p = DuplexSheetPlan {
                sheet: k,
                front_page: k * 2,
                back_page: Some(k * 2 + 1),
                front: SideStamp { opacity: 300, mirror_x: false, x: 10 },
                back: Some(SideStamp { opacity: 186, mirror_x: true, x: 10 }),
            };
            match led.push(p) {
                Err(DuplexError::Capacity) => {
                    cap_hit = true;
                    break;
                }
                _ => {}
            }
            k += 1;
        }
        if dup_hit {
            caught += 1;
        }
        if cap_hit && led.len() == LEDGER_CAP {
            caught += 1;
        }
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50048 的 14 条判据。
pub fn run_unx50048_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50048");

    // J1-1 模式闭集：3 合法 + 越界拒
    let modes_ok = DuplexMode::from_u8(0).is_ok()
        && DuplexMode::from_u8(1).is_ok()
        && DuplexMode::from_u8(2).is_ok()
        && DuplexMode::from_u8(3).err() == Some(DuplexError::Mode);
    s.add("50048-模式闭集", modes_ok, "模式码闭集漂移");

    // J1-2 档位闭集 + 因子单调
    let f0 = FadeTier::Off.factor_permille();
    let f1 = FadeTier::Light.factor_permille();
    let f2 = FadeTier::Medium.factor_permille();
    let f3 = FadeTier::Strong.factor_permille();
    let tier_ok = f0 == 1000
        && f0 > f1
        && f1 > f2
        && f2 > f3
        && f3 > 0
        && FadeTier::from_u8(4).err() == Some(DuplexError::FadeTier);
    let det = format!("{}/{}/{}/{}", f0, f1, f2, f3);
    s.add_dyn("50048-档位单调", tier_ok, &det);

    // J1-3 双面页序配对：10 张 = 20 页，正反面连续、互不重叠
    let plans = plan_document(
        20,
        DuplexMode::LongEdge,
        400,
        FadeTier::Medium,
        595_000,
        842_000,
        100_000,
        200_000,
    );
    let pair_ok = match &plans {
        Ok(v) => {
            let mut ok = v.len() == 10;
            let mut k = 0usize;
            while k < v.len() {
                if let Some(p) = v.get(k) {
                    if p.sheet as usize != k || p.front_page != (k as u16) * 2 {
                        ok = false;
                    }
                    match p.back_page {
                        Some(b) if b == p.front_page + 1 => {}
                        _ => ok = false,
                    }
                }
                k += 1;
            }
            ok
        }
        Err(_) => false,
    };
    s.add("50048-页序配对", pair_ok, "双面配对漂移");

    // J1-4 背面淡化 ×10 一致：10 张背面不透明度全等且等于公式值
    let fade10_ok = match &plans {
        Ok(v) => {
            let want = back_opacity(400, FadeTier::Medium);
            let mut all_same = true;
            let mut k = 0usize;
            while k < v.len() {
                if let Some(p) = v.get(k) {
                    match p.back {
                        Some(b) => {
                            if Some(b.opacity) != want.ok() {
                                all_same = false;
                            }
                        }
                        None => all_same = false,
                    }
                }
                k += 1;
            }
            all_same && v.len() == 10
        }
        Err(_) => false,
    };
    let det = match back_opacity(400, FadeTier::Medium) {
        Ok(v) => format!("back={}x10", v),
        Err(_) => String::from("fade failed"),
    };
    s.add_dyn("50048-J1-背面淡化10一致", fade10_ok, &det);

    // J1-5 背面不亮于正面（逐张）
    let never_brighter = match &plans {
        Ok(v) => {
            let mut ok = true;
            let mut k = 0usize;
            while k < v.len() {
                if let Some(p) = v.get(k) {
                    if let Some(b) = p.back {
                        if b.opacity > p.front.opacity {
                            ok = false;
                        }
                    }
                }
                k += 1;
            }
            ok
        }
        Err(_) => false,
    };
    s.add("50048-背面不变亮", never_brighter, "背面比正面更亮");

    // J1-6 四档在同一基值下严格递减
    let mut v = [0u16; 4];
    let mut i = 0u8;
    while i < 4 {
        let t = match FadeTier::from_u8(i) {
            Ok(t) => t,
            Err(_) => FadeTier::Off,
        };
        if let Some(slot) = v.get_mut(i as usize) {
            *slot = match back_opacity(1000, t) {
                Ok(x) => x,
                Err(_) => 0,
            };
        }
        i += 1;
    }
    let strict = v[0] == 1000;
    let mut k = 1usize;
    while k < v.len() {
        let (a, b) = (v[k - 1], v[k]);
        if !(a > b) {
            // 严格递减被破坏
            break;
        }
        k += 1;
    }
    let strict = strict && k == v.len();
    let det = format!("{}/{}/{}/{}", v[0], v[1], v[2], v[3]);
    s.add_dyn("50048-四档序", strict, &det);

    // J1-7 最低位保护：base=1 在 Strong 档不归零
    let floor_ok = back_opacity(1, FadeTier::Strong) == Ok(1)
        && back_opacity(0, FadeTier::Strong) == Ok(0);
    s.add("50048-防归零", floor_ok, "低基值背面归零");

    // J1-8 单面档不产生背面，且不淡化
    let simplex = plan_document(
        5,
        DuplexMode::Simplex,
        400,
        FadeTier::Strong,
        595_000,
        842_000,
        100_000,
        200_000,
    );
    let simplex_ok = match &simplex {
        Ok(v) => {
            let mut ok = v.len() == 5;
            let mut k = 0usize;
            while k < v.len() {
                if let Some(p) = v.get(k) {
                    if p.back.is_some() || p.front.opacity != 400 || p.front_page != k as u16 {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok
        }
        Err(_) => false,
    };
    s.add("50048-单面不淡化", simplex_ok, "单面档出现背面或淡化");

    // J1-9 短边镜像互补、长边原样
    let a = mirror_anchor(DuplexMode::ShortEdge, 100_000, 200_000, 595_000);
    let b = mirror_anchor(DuplexMode::LongEdge, 100_000, 200_000, 595_000);
    let mirror_ok = match (a, b) {
        (Ok(x1), Ok(x2)) => x2 == 100_000 && x1 + 100_000 + 200_000 == 595_000,
        _ => false,
    };
    let det = match a {
        Ok(x) => format!("short={} long=100000", x),
        Err(_) => String::from("mirror failed"),
    };
    s.add_dyn("50048-镜像互补", mirror_ok, &det);

    // J1-10 计划里短边背面 mirror_x=true，长边 false
    let short_plan = plan_document(
        4,
        DuplexMode::ShortEdge,
        300,
        FadeTier::Light,
        595_000,
        842_000,
        50_000,
        100_000,
    );
    let flags_ok = match (&short_plan, &plans) {
        (Ok(sp), Ok(lp)) => {
            let mut ok = true;
            let mut k = 0usize;
            while k < sp.len() {
                if let Some(p) = sp.get(k) {
                    match p.back {
                        Some(b) if b.mirror_x => {}
                        _ => ok = false,
                    }
                }
                k += 1;
            }
            let mut j = 0usize;
            while j < lp.len() {
                if let Some(p) = lp.get(j) {
                    match p.back {
                        Some(b) if b.mirror_x => ok = false,
                        _ => {}
                    }
                }
                j += 1;
            }
            ok && !sp.is_empty() && !lp.is_empty()
        }
        _ => false,
    };
    s.add("50048-镜像标志", flags_ok, "镜像标志与模式不符");

    // J1-11 确定性：两次计划指纹一致且非零
    let d1 = match plan_document(
        12,
        DuplexMode::ShortEdge,
        350,
        FadeTier::Medium,
        595_000,
        842_000,
        60_000,
        120_000,
    ) {
        Ok(v) => plan_digest(&v),
        Err(_) => 0,
    };
    let d2 = match plan_document(
        12,
        DuplexMode::ShortEdge,
        350,
        FadeTier::Medium,
        595_000,
        842_000,
        60_000,
        120_000,
    ) {
        Ok(v) => plan_digest(&v),
        Err(_) => 1,
    };
    s.add("50048-确定性", d1 == d2 && d1 != 0, "两次计划不一致");

    // J1-12 状态机：幂等 + 前进 + 倒退/跳级拒
    let st = DuplexStage::Draft;
    let stage_ok = st.advance(DuplexStage::Draft) == Ok(DuplexStage::Draft)
        && st.advance(DuplexStage::Planned) == Ok(DuplexStage::Planned)
        && st.advance(DuplexStage::Sealed).err() == Some(DuplexError::StageRegress)
        && DuplexStage::Emitted.advance(DuplexStage::Planned).err()
            == Some(DuplexError::StageRegress);
    s.add("50048-阶段单调", stage_ok, "状态机允许倒退或跳级");

    // J1-13 台账：去重、容量、Draft 拒入账
    let mut led = DuplexLedger::new();
    let one = DuplexSheetPlan {
        sheet: 0,
        front_page: 0,
        back_page: Some(1),
        front: SideStamp { opacity: 400, mirror_x: false, x: 0 },
        back: Some(SideStamp { opacity: 248, mirror_x: false, x: 0 }),
    };
    let draft_reject = led.push(one).err() == Some(DuplexError::StageRegress);
    let adv = led.advance(DuplexStage::Planned).is_ok();
    let first = led.push(one).is_ok();
    let dup = led.push(one).err() == Some(DuplexError::Duplicate);
    let mut cap_hit = false;
    let mut k = 1u16;
    while k < 20 {
        let p = DuplexSheetPlan {
            sheet: k,
            front_page: k * 2,
            back_page: Some(k * 2 + 1),
            front: SideStamp { opacity: 400, mirror_x: false, x: 0 },
            back: Some(SideStamp { opacity: 248, mirror_x: false, x: 0 }),
        };
        match led.push(p) {
            Err(DuplexError::Capacity) => {
                cap_hit = true;
                break;
            }
            _ => {}
        }
        k += 1;
    }
    let ledger_ok = draft_reject && adv && first && dup && cap_hit && led.len() == LEDGER_CAP;
    let det = format!("len={} rej={}", led.len(), led.rejected());
    s.add_dyn("50048-台账纪律", ledger_ok, &det);

    // J1-14 注入核 10/10
    let (caught, total) = injection_core();
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50048-注入必抓", caught == total, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50048_checks_green() {
        let set = run_unx50048_checks();
        assert!(set.all_passed(), "UNX-F50048 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50048_ten_sheets_same_back() {
        let plans = match plan_document(
            20,
            DuplexMode::ShortEdge,
            500,
            FadeTier::Strong,
            595_000,
            842_000,
            120_000,
            180_000,
        ) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(plans.len(), 10);
        let want = back_opacity(500, FadeTier::Strong);
        let mut i = 0usize;
        while i < plans.len() {
            if let Some(p) = plans.get(i) {
                assert_eq!(p.back.map(|b| b.opacity), want.ok());
                match p.back {
                    Some(b) => assert!(b.mirror_x),
                    None => assert!(false, "back side missing"),
                }
            }
            i += 1;
        }
    }

    #[test]
    fn unx50048_monotone_tiers() {
        let a = back_opacity(1000, FadeTier::Off);
        let b = back_opacity(1000, FadeTier::Light);
        let c = back_opacity(1000, FadeTier::Medium);
        let d = back_opacity(1000, FadeTier::Strong);
        match (a, b, c, d) {
            (Ok(a), Ok(b), Ok(c), Ok(d)) => assert!(a > b && b > c && c > d),
            _ => assert!(false, "fade failed"),
        }
    }
}
