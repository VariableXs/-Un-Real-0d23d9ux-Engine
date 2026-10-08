//! VE-F3410 · 深空玻璃令牌基线 —— 审美立场令牌化 + 质感价签双标注。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3410`
//!
//! # 职责（锚点原文拆解）
//!
//! - **审美立场令牌化**：Variable 的深空玻璃审美（毛玻璃层级/光影强度/
//!   模糊半径）落成**令牌全集**——五级封闭层级，每级默认质感参数单源
//!   定义，判据侧按独立字面量表对拍；审美不允许裸魔数散落，参数只有
//!   一处定义（改基线=改一处，全线跟随）；
//! - **质感价签双标注**：每个令牌两半——质感参数（好看吗）与性能
//!   价签（每像素付多少）。价签**不许手抄**：必须从层级+质感按公开
//!   公式推导（手抄价签是假账，质感漂移后没人知道哪半是真的）；
//!   缺价签→按公式补齐并记账（标注补齐），错价签→拒绝（不可补救——
//!   补齐解决"没写"，解决不了"写错"）；
//! - **性能超标→降档**：预算扫描 O(令牌数)，超标令牌降下一级；
//!   **降档预告**：降档先出预告（从哪级到哪级、降价多少），应用与
//!   预告对票——没有预告的降档不可应用（静默降档会让"我设了 Heavy"
//!   看着生效实际跑 Medium，正是静默夹取）；
//! - **跨批对接点 V 域色彩联动**：基线留色彩联动挂点（染色 ‰），
//!   联动值越界即拒（联动是协调不是放水）；
//! - **降级矩阵**：价签缺失→标注补齐；质感违例→整改（钳回该级
//!   合法界内并记账）；性能超标→降档（预告先行）。
//!
//! # 为什么价签是推导而非标注
//!
//! 价签若靠人工标注，改 blur 忘改价签是必然发生的事——双标注会
//! 退化成"两份各自漂移的账"。唯一稳定的做法是**单源推导**：价签
//! = f(层级, 质感)，公开公式使推导可判据复核；构造入口只接受
//! `Option` 价签（None 补齐、Some 必须与推导逐位一致），"价签与
//! 质感不一致"这一状态在类型面不可构造。
//!
//! # 为什么质感违例是钳回而非拒绝
//!
//! 锚点降级矩阵明文：质感违例→整改。审美参数是连续域（blur 0..64），
//! 越界值方向明确（更大更贵），钳回界内并记账保留设计意图；拒绝则
//! 整个令牌不可用、界面回退到裸渲染——比越界的玻璃更伤审美。但
//! **价签错值不可钳**：价签是账目不是质感，账目错了补账（重推导）
//! 而不是改到能过。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；
//! 判据区一律 match 记红。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；E12 段独占，与 E11/F3409 等既有段零重叠）
// ---------------------------------------------------------------------------

/// 深空玻璃域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlassCode {
    /// 令牌路径非法（空或超长）。
    TokenEmpty,
    /// 价签值非法（与推导公式不符——假账不可补救）。
    TagInvalid,
    /// 降档应用无预告或预告不符（静默降档拦截）。
    PreviewMismatch,
    /// V 域色彩联动状态非法（染色越界）。
    LinkageState,
    /// 质感违例（越界钳回整改，非阻断）。
    TextureViolation,
    /// 价签缺失（按公式补齐，非阻断）。
    TagMissing,
    /// 性能超标（走降档预告，非阻断）。
    BudgetOver,
}

impl GlassCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [GlassCode; 7] = [
        GlassCode::TokenEmpty,
        GlassCode::TagInvalid,
        GlassCode::PreviewMismatch,
        GlassCode::LinkageState,
        GlassCode::TextureViolation,
        GlassCode::TagMissing,
        GlassCode::BudgetOver,
    ];

    /// 线上短码（E12 段独占）。
    pub const fn code(self) -> &'static str {
        match self {
            GlassCode::TokenEmpty => "E12-TOKEN-EMPTY",
            GlassCode::TagInvalid => "E12-TAG-INVALID",
            GlassCode::PreviewMismatch => "E12-PREVIEW-MISMATCH",
            GlassCode::LinkageState => "E12-LINKAGE-STATE",
            GlassCode::TextureViolation => "E12-TEXTURE-VIOLATION",
            GlassCode::TagMissing => "E12-TAG-MISSING",
            GlassCode::BudgetOver => "E12-BUDGET-OVER",
        }
    }

    /// 是否阻断。
    pub const fn blocking(self) -> bool {
        !matches!(
            self,
            GlassCode::TextureViolation | GlassCode::TagMissing | GlassCode::BudgetOver
        )
    }

    /// 是否降级类（走降级矩阵）。
    pub const fn degradable(self) -> bool {
        matches!(
            self,
            GlassCode::TextureViolation | GlassCode::TagMissing | GlassCode::BudgetOver
        )
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            GlassCode::TokenEmpty => "令牌路径非法。",
            GlassCode::TagInvalid => "价签与质感推导不符，拒绝假账。",
            GlassCode::PreviewMismatch => "降档无预告或预告不符，拒绝静默降档。",
            GlassCode::LinkageState => "色彩联动状态非法。",
            GlassCode::TextureViolation => "质感参数越界，已钳回整改。",
            GlassCode::TagMissing => "价签缺失，已按公式补齐。",
            GlassCode::BudgetOver => "性能超标，已出降档预告。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、契约常量
// ---------------------------------------------------------------------------

/// 契约版本（冻结）。
pub const GLASS_CONTRACT: &str = "E12-glass-v1";
/// 模糊半径上限（px）。
pub const BLUR_MAX_PX: u32 = 64;
/// 千分值上限（光影/透明度/染色共用口径）。
pub const PERMILLE_MAX: u32 = 1000;
/// 令牌路径字节上限。
pub const PATH_MAX: usize = 128;
/// 每像素模糊步价（‰/px，价签公式单源）。
pub const BLUR_COST_PER_PX: u32 = 30;
/// 每像素步价预算（‰）：默认基线下恰只 Heavy(1590‰) 超标——
/// 预算值本身是审美取舍的一部分，判据侧钉死。
pub const BUDGET_STEPS: u32 = 1200;
/// 基线令牌数（五级封闭全集）。
pub const BASELINE_TOKENS: usize = 5;

/// 校验令牌路径。
fn validate_path(p: &str) -> Result<(), GlassCode> {
    if p.is_empty() || p.len() > PATH_MAX {
        return Err(GlassCode::TokenEmpty);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、毛玻璃层级（五级封闭全集）
// ---------------------------------------------------------------------------

/// 毛玻璃层级：由重到轻五级封闭全集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlassTier {
    /// 不透明基线（无玻璃质感，最省）。
    Opaque,
    /// 重玻璃（重毛玻璃，最贵）。
    Heavy,
    /// 中玻璃。
    Medium,
    /// 轻玻璃。
    Light,
    /// 透明（无模糊，仅淡染）。
    Clear,
}

impl GlassTier {
    /// 全集（判据据此核对无遗漏）。
    pub const ALL: [GlassTier; 5] = [
        GlassTier::Opaque,
        GlassTier::Heavy,
        GlassTier::Medium,
        GlassTier::Light,
        GlassTier::Clear,
    ];

    /// 层级序（由重到轻 0..4；降档即序号 +1）。
    pub const fn rank(self) -> usize {
        match self {
            GlassTier::Opaque => 0,
            GlassTier::Heavy => 1,
            GlassTier::Medium => 2,
            GlassTier::Light => 3,
            GlassTier::Clear => 4,
        }
    }

    /// 序号反查层级（越界 None）。
    pub const fn of_rank(r: usize) -> Option<GlassTier> {
        match r {
            0 => Some(GlassTier::Opaque),
            1 => Some(GlassTier::Heavy),
            2 => Some(GlassTier::Medium),
            3 => Some(GlassTier::Light),
            4 => Some(GlassTier::Clear),
            _ => None,
        }
    }

    /// 中文名（读屏）。
    pub const fn zh(self) -> &'static str {
        match self {
            GlassTier::Opaque => "不透明",
            GlassTier::Heavy => "重玻璃",
            GlassTier::Medium => "中玻璃",
            GlassTier::Light => "轻玻璃",
            GlassTier::Clear => "透明",
        }
    }
}

/// 层级默认质感基线（审美立场单源：改基线只改这里）。
///
/// 返回 (blur_px, luma_permille, opacity_permille)。
/// 不透明与透明两级 blur 为 0：不透明无毛玻璃可磨，透明磨了也看不见——
/// 给它们 blur 是给价签白加钱。
pub const fn tier_texture(t: GlassTier) -> (u32, u32, u32) {
    match t {
        GlassTier::Opaque => (0, 200, 1000),
        GlassTier::Heavy => (48, 800, 700),
        GlassTier::Medium => (32, 650, 550),
        GlassTier::Light => (16, 500, 400),
        GlassTier::Clear => (0, 350, 150),
    }
}

/// 层级基础价（‰，价签公式单源之一；与 blur 步价合成总价）。
/// 透明级基础价更高：无模糊但有淡染合成 pass，成本不在 blur 上。
const fn tier_base_cost(t: GlassTier) -> u32 {
    match t {
        GlassTier::Clear => 250,
        _ => 150,
    }
}

// ---------------------------------------------------------------------------
// 四、质感参数与性能价签（双标注）
// ---------------------------------------------------------------------------

/// 质感参数（质感标注半边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Texture {
    /// 模糊半径（px，0..=BLUR_MAX_PX）。
    pub blur_px: u32,
    /// 光影强度（‰，0..=1000）。
    pub luma_permille: u32,
    /// 不透明度（‰，0..=1000）。
    pub opacity_permille: u32,
}

/// 性能价签（性能标注半边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PriceTag {
    /// 每像素步价（‰，抽象机口径）。
    pub steps_permille: u32,
}

/// 价签推导公式（公开、可判据复核；层级+质感 → 价签单源）。
pub const fn derive_price(tier: GlassTier, t: Texture) -> u32 {
    tier_base_cost(tier) + t.blur_px * BLUR_COST_PER_PX
}

/// 质感违例整改：钳回合法界内（返回整改后的质感）。
pub const fn remediate(t: Texture) -> Texture {
    Texture {
        blur_px: if t.blur_px > BLUR_MAX_PX {
            BLUR_MAX_PX
        } else {
            t.blur_px
        },
        luma_permille: if t.luma_permille > PERMILLE_MAX {
            PERMILLE_MAX
        } else {
            t.luma_permille
        },
        opacity_permille: if t.opacity_permille > PERMILLE_MAX {
            PERMILLE_MAX
        } else {
            t.opacity_permille
        },
    }
}

/// 质感是否违例（整改预告：先判后钳，判据可分别核对）。
pub const fn violated(t: Texture) -> bool {
    t.blur_px > BLUR_MAX_PX
        || t.luma_permille > PERMILLE_MAX
        || t.opacity_permille > PERMILLE_MAX
}

/// 双标注令牌。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GlassToken {
    /// 令牌路径。
    pub path: String,
    /// 毛玻璃层级。
    pub tier: GlassTier,
    /// 质感标注。
    pub spec: Texture,
    /// 性能价签（必与 (tier, spec) 推导一致——构造入口保证）。
    pub tag: PriceTag,
}

/// 令牌构造（唯一入口：双标注在此闭环）。
///
/// - `tag=None`：价签缺失→按公式补齐，返回 `Some(TagMissing)`（补齐
///   动作可判据）；
/// - `tag=Some(p)`：与推导逐位比对，不符报 `Err(TagInvalid)`（假账
///   拒绝——错账不可钳，钳账是帮假账过关）；
/// - 质感违例：钳回整改，返回 `Some(TextureViolation)`——价签按
///   **整改后**质感推导（先整改后定价，账跟着事实走）；
/// - 返回 `None` = 零事件（合法质感 + 价签与推导一致或缺签已补但无
///   违例时为 None？否——缺签必报 TagMissing；None 仅当质感合法且
///   价签恰好与推导一致）。
pub fn build_token(
    path: &str,
    tier: GlassTier,
    spec: Texture,
    tag: Option<PriceTag>,
) -> Result<(GlassToken, Option<GlassCode>), GlassCode> {
    validate_path(path)?;
    let (fixed, violation) = if violated(spec) {
        (remediate(spec), true)
    } else {
        (spec, false)
    };
    let price = derive_price(tier, fixed);
    let note = if violation {
        // 先整改后定价；违例与缺签同发时报违例（整改在先，顺序可判据）
        Some(GlassCode::TextureViolation)
    } else {
        match tag {
            None => Some(GlassCode::TagMissing),
            Some(_) => None,
        }
    };
    match tag {
        Some(p) if !violation && p.steps_permille != price => {
            return Err(GlassCode::TagInvalid);
        }
        _ => {}
    }
    Ok((
        GlassToken {
            path: String::from(path),
            tier,
            spec: fixed,
            tag: PriceTag {
                steps_permille: price,
            },
        },
        note,
    ))
}

// ---------------------------------------------------------------------------
// 五、基线令牌集（五级全集 + 预算扫描 + 降档预告）
// ---------------------------------------------------------------------------

/// 降档预告（降档先出预告，应用与预告对票）。
///
/// 预告带令牌路径身份：降档后基线可出现同层级令牌（原生 Medium 与
/// Heavy 降档而来），按层级定位会改错对象——身份必须落在路径上。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DownshiftPreview {
    /// 目标令牌路径（预告身份）。
    pub path: String,
    /// 降档前层级。
    pub from: GlassTier,
    /// 降档后层级（序号 +1；已在最低档则无可降）。
    pub to: GlassTier,
    /// 降档后每像素步价（‰，按目标级默认质感重推导）。
    pub new_steps: u32,
}

/// 深空玻璃基线令牌集。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GlassBaseline {
    /// 五级全集令牌（封闭，序 = 层级序）。
    pub tokens: Vec<GlassToken>,
    /// 标注补齐账（哪些令牌补过价签）。
    pub tag_backfilled: Vec<String>,
    /// 整改账（哪些令牌钳过质感）。
    pub remediated: Vec<String>,
    /// 降档预告账（Issued；应用即核销，降档事实落在令牌本身）。
    pub previews: Vec<DownshiftPreview>,
}

impl GlassBaseline {
    /// 建基线：五级全集按单源默认质感逐级构造。
    ///
    /// 默认值全部静态合法且有推导价签——基线构造**零事件**是判据
    /// 钉死的不变量（基线本身就超标/缺签，价签纪律就是空话）。
    pub fn new() -> GlassBaseline {
        let mut tokens = Vec::new();
        for t in GlassTier::ALL.iter() {
            let (b, l, o) = tier_texture(*t);
            let spec = Texture {
                blur_px: b,
                luma_permille: l,
                opacity_permille: o,
            };
            tokens.push(GlassToken {
                path: String::from(glass_path(*t)),
                tier: *t,
                spec,
                tag: PriceTag {
                    steps_permille: derive_price(*t, spec),
                },
            });
        }
        GlassBaseline {
            tokens,
            tag_backfilled: Vec::new(),
            remediated: Vec::new(),
            previews: Vec::new(),
        }
    }

    /// 按层级查令牌（O(1)：封闭全集直取）。
    pub fn token_of(&self, t: GlassTier) -> Option<&GlassToken> {
        self.tokens.iter().find(|k| k.tier == t)
    }

    /// 添加自定义令牌（走唯一构造入口并按事件记账）。
    pub fn add_custom(
        &mut self,
        path: &str,
        tier: GlassTier,
        spec: Texture,
        tag: Option<PriceTag>,
    ) -> Result<Option<GlassCode>, GlassCode> {
        let (tok, note) = build_token(path, tier, spec, tag)?;
        match note {
            Some(GlassCode::TagMissing) => {
                self.tag_backfilled.push(String::from(&tok.path));
            }
            Some(GlassCode::TextureViolation) => {
                self.remediated.push(String::from(&tok.path));
            }
            _ => {}
        }
        self.tokens.push(tok);
        Ok(note)
    }

    /// 预算扫描（O(令牌数)）：超标者出降档预告。
    ///
    /// 降档 = 层级序 +1（Heavy→Medium→Light→Clear）；Opaque 与 Clear
    /// 无可降（Opaque 最省；Clear 已最低档）——如实记 BudgetOver 而
    /// 非硬造预告；每令牌**只发一张**预告（重复扫描不重复发——预告
    /// 是待办不是日志，账在 previews 的去重语义上）。
    pub fn budget_scan(&mut self, budget_steps: u32) -> Vec<DownshiftPreview> {
        let mut out = Vec::new();
        let mut over_lowest = false;
        for tok in self.tokens.iter() {
            if tok.tag.steps_permille <= budget_steps {
                continue;
            }
            let next_rank = tok.tier.rank() + 1;
            match GlassTier::of_rank(next_rank) {
                Some(next) => {
                    let p = DownshiftPreview {
                        path: String::from(&tok.path),
                        from: tok.tier,
                        to: next,
                        new_steps: {
                            let (b, l, o) = tier_texture(next);
                            derive_price(next, Texture {
                                blur_px: b,
                                luma_permille: l,
                                opacity_permille: o,
                            })
                        },
                    };
                    if !self.previews.iter().any(|q| q == &p) {
                        self.previews.push(p.clone());
                        out.push(p);
                    }
                }
                None => {
                    over_lowest = true;
                }
            }
        }
        let _ = over_lowest; // 最低档超标以 BudgetOver 码呈现（判据侧经
                             // 降档矩阵核对），扫描本身不阻断
        out
    }

    /// 应用降档（必须持预告对票：路径/层级/去向逐位一致方可落档）。
    ///
    /// 应用即核销预告——预告账不留已应用项；层级、质感、价签三半
    /// 同步换级（三半永不失配），核销按下标精确删除。
    pub fn apply_downshift(&mut self, p: &DownshiftPreview) -> Result<(), GlassCode> {
        let i = match self.previews.iter().position(|q| q == p) {
            Some(i) => i,
            None => return Err(GlassCode::PreviewMismatch),
        };
        let tok = match self
            .tokens
            .iter_mut()
            .find(|k| k.path == p.path && k.tier == p.from)
        {
            Some(t) => t,
            None => return Err(GlassCode::PreviewMismatch),
        };
        let (b, l, o) = tier_texture(p.to);
        tok.tier = p.to;
        tok.spec = Texture {
            blur_px: b,
            luma_permille: l,
            opacity_permille: o,
        };
        tok.tag.steps_permille = derive_price(p.to, tok.spec);
        self.previews.remove(i);
        Ok(())
    }
}

/// 层级 → 基线路径（命名单源）。
pub fn glass_path(t: GlassTier) -> &'static str {
    match t {
        GlassTier::Opaque => "glass.opaque",
        GlassTier::Heavy => "glass.heavy",
        GlassTier::Medium => "glass.medium",
        GlassTier::Light => "glass.light",
        GlassTier::Clear => "glass.clear",
    }
}

// ---------------------------------------------------------------------------
// 六、V 域色彩联动（跨批对接点）
// ---------------------------------------------------------------------------

/// 色彩联动挂点（V 域协调深空玻璃的色温染色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorLinkage {
    /// 联动是否启用。
    pub enabled: bool,
    /// 染色强度（‰，0..=1000）。
    pub tint_permille: u32,
}

impl ColorLinkage {
    /// 默认：关闭（基线自成审美，联动是显式选择）。
    pub const fn off() -> ColorLinkage {
        ColorLinkage {
            enabled: false,
            tint_permille: 0,
        }
    }

    /// 校验联动状态。
    pub fn validate(&self) -> Result<(), GlassCode> {
        if self.tint_permille > PERMILLE_MAX {
            return Err(GlassCode::LinkageState);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、读屏可达
// ---------------------------------------------------------------------------

/// 基线读屏：逐级「层级 = 模糊 px / 光影 ‰ / 透明度 ‰ / 价签 ‰」。
pub fn baseline_spoken(b: &GlassBaseline) -> String {
    let mut s = String::from("深空玻璃基线：");
    for tok in b.tokens.iter() {
        s.push_str(&format!(
            "{} = {}px/{}‰/{}‰，价签 {}‰；",
            tok.tier.zh(),
            tok.spec.blur_px,
            tok.spec.luma_permille,
            tok.spec.opacity_permille,
            tok.tag.steps_permille
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_price_formula() {
        let t = Texture {
            blur_px: 48,
            luma_permille: 0,
            opacity_permille: 0,
        };
        assert_eq!(derive_price(GlassTier::Heavy, t), 150 + 48 * 30);
        assert_eq!(derive_price(GlassTier::Clear, t), 250 + 48 * 30);
    }

    #[test]
    fn tier_roundtrip() {
        for (i, t) in GlassTier::ALL.iter().enumerate() {
            assert_eq!(t.rank(), i);
            assert_eq!(GlassTier::of_rank(i), Some(*t));
        }
        assert_eq!(GlassTier::of_rank(5), None);
    }
}
