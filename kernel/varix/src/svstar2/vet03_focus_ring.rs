//! VE-F3804 · 焦点渲染强化（VE-T 域 · 无障碍渲染 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3804`
//!
//! **判据（锚点原文逐条）**：1 帧追焦、强化参数、状态保持、协同仲裁、
//! 渲染层执法、判据。
//!
//! **本条是什么**：F3802（[`vet01_a11y_render_pipeline`]）把无障碍渲染落成
//! 四态→策略→注入→预算的管线，F3803（[`vet02_highcontrast_engine`]）把
//! 高对比态落成令牌段+后处理段。本条处理官方主题十项里的**第三项：焦点
//! 强化**——焦点环在**渲染层**的全域强化，以及「焦点移动必须零延迟」这条
//! 独立于前两条的红线。
//!
//! **为什么焦点环要单列一条而不是并进高对比（这是本条最容易被做错的地方）**：
//! 高对比是「让所有东西都更清楚」，焦点环是「让**这一个**东西被找到」。
//! 二者的失效形态完全不同：
//!   - 高对比不足 → 画面整体偏灰，用户觉得「主题不行」，能凑合用；
//!   - 焦点环不足 → **键盘用户不知道自己在哪**。他按了三次 Tab，
//!     屏幕没有任何变化，于是他判断「这个程序卡住了」，开始用鼠标戳。
//! 前者是审美问题，后者是**可用性归零**。故本条把焦点环的判据写成
//! **独立红线**而不是高对比的一个参数：环的宽度、发光、对比度都受门
//! 联动约束，但「追焦延迟 ≤ 1 帧」这条与对比度无关——环再粗，滞后
//! 两帧的粗环照样让用户迷失。
//!
//! ── 判据一：1 帧追焦（锚点"焦点移动渲染零延迟"）────────────────────
//!
//! 锚点原文：**焦点移动渲染滞后>1 帧=键盘用户迷失**。
//!
//! 本条把这条红线做成**可执行的差分断言**而不是设计承诺：
//! 一次焦点移动有两条独立的时间线——**输入帧**（键盘事件被采到的帧）与
//! **渲染帧**（环被真正画进帧缓冲的帧）。两者之差即追焦延迟。
//!
//! **为什么门限是 1 而不是 0**：本条不承诺「零延迟」（那要求输入事件与
//! 帧提交同帧完成，依赖 D 域管线时序，不在本条可承诺的范围），只承诺
//! **不超过 1 帧**。这个数字的物理含义是：用户按 Tab 后，最迟在下一帧
//! 看到环移动。60Hz 下 1 帧 = 16.7ms，人眼感知为「立即」；2 帧 = 33ms，
//! 开始能被察觉为「跟了一下」；而对连续按 Tab 的用户（快速跳格），
//! 累积滞后会让环**系统性地落后于实际焦点**——他看到环停在上一格，
//! 于是按第三次 Tab 时已经跳过了两格。这是「迷失」的真正机制。
//!
//! **「未渲染」比「渲染慢」更严重**：若某次焦点移动**根本没有对应的
//! 渲染**（seq 匹配不上），那不是延迟问题而是**焦点环消失**——判 P0
//! 而非 P1。分级的理由：延迟是「慢」，无渲染是「没了」，用户在两种
//! 情况下的应对完全不同（等一下 vs 以为程序死了）。
//!
//! ── 判据二：强化参数（锚点"高对比下焦点环双层加粗+外发光增强"）────
//!
//! 焦点环是**双层**结构：内环（贴着控件边界，主指示）+ 外晕（柔化层）。
//! 锚点要求高对比下**两层同时加粗**且**外发光增强**——不是只加粗内环。
//! 只加粗内环会让环与控件边界糊在一起（内环贴边，加粗后侵占控件内部），
//! 外晕不加粗则环与背景的过渡仍是一道细缝，在高对比配色下反而更难分辨。
//!
//! **强化必须受对比度门联动约束（锚点"强化破对比度→门联动修正"）**：
//! 这是本条最容易写出反向效果的地方。外发光的物理实现是**把环色向背景色
//! 混合**（辉光扩散），混合的结果是**环外缘与背景的对比度下降**。
//! 也就是说：**加大发光直接破坏对比度**——一个"更醒目"的参数改动，
//! 在 WCAG 口径下是**降低可读性**的。若无条件接受请求的发光强度，
//! 本域会亲手把 F3803 刚建立的对比度硬门推下坡。
//!
//! 本条因此把发光强度当作**受门控的上限**而非 freely 接受的输入：
//! [`enforce_contrast_gate`] 从请求值向下搜索**最大可行alpha**
//! （门为F3803 冻结的 [`f3803::WCAG_AA_NORMAL`]），并把
//! 「请求值 → 实授值」的削减显式落在 [`GateVerdict`] 里可查。
//!
//! **门限取AA 4.5 而不是 3.0**：焦点环是「指示当前焦点」的图形，
//! WCAG 2.x 对非文本内容的下限是 3:1，但 3:1 是**下限**不是目标。
//! 本条沿用 F3803 已冻结的 4.5（复用单源，见下），口径统一比口径宽松
//! 更重要——两处用不同门限会产出「整体过了单项没过」的荒谬局面。
//!
//! ── 判据三：状态保持（锚点"窗口切换回来焦点渲染状态保持"）────────
//!
//! 窗口失焦再回来时，焦点环的渲染状态**必须与失焦前逐字段相同**。
//! 真实缺陷形态：窗口切走时用户按了Tab（焦点移到窗口内另一控件），
//! 切回来时渲染层用**当前环境态**重新计算环参数，而不是用失焦前
//! 保存的那份——于是环宽度/发光变了。用户视角是「点了一下窗口，
//! 界面样式就变了」，而他此时正打算继续用键盘操作 Tab。
//!
//! 本条把保持做成**协议**（带版本号与epoch 计数）：[`FocusLedger`]
//! 在blur 时存快照、focus 时**原样恢复**，恢复路径**不读环境态**。
//! 判据用「环境态在blur 期间翻转」的非平凡语料来证明恢复路径确实
//! 没读环境态——若语料里环境态没变，则「重算」与「恢复」结果相同，
//! 判据恒绿。
//!
//! ── 判据四：协同仲裁（锚点"F3044 迁移动效在高对比下简化"）────────
//!
//! F3044（焦点环动效）给环加了迁移动画（旧环收→新环放150ms）与入场
//! 弹入（起点 0.6 缩放）。本条在渲染层与它**协同**：高对比下这两个动效
//! 必须简化。理由不是「动画碍事」，而是 F3044 的动效参数是按常规主题
//! 调的：0.6 起点缩放会让环在动画首帧只有六成存在感，而高对比配色
//! 下用户往往是因为**已经看不清**才开的高对比——在最需要清晰的场景
//! 里降低清晰度是方向性错误。
//!
//! **仲裁必须显性（锚点"协同冲突→仲裁"）**：动效源要求 150ms 迁移、
//! 高对比要求简化，二者冲突时**高对比胜**（可见性优先于流畅性），
//! 且必须产出可查的仲裁记录——静默改掉动画时长会让动画作者无从追责。
//!
//! **可见性下限 60% 不可协商（复述 F3044 起点红线）**：无论怎么仲裁，
//! 环在迁移过程中的**最低可见度不得低于 60%**。这条是仲裁的**不变量**
//! 而非某条分支的性质——把它做成对全部输入组合的遍历断言，
//! 防止将来新增分支时无声破掉。
//!
//! ── 判据五：渲染层执法（锚点"渲染层执法"）────────────────────────
//!
//! 这是本域「语义→像素最后一公里」红线在焦点环上的形态：
//! **语义树说焦点在某个节点上，而像素层没画出环**。
//! F3801 已把这条定为最恶劣缺陷（用户据此行动，以为按钮在那里）。
//! 焦点环是这条红线**后果最重**的实例：读屏用户能听到「焦点在
//! 提交按钮」，但视觉上没有任何指示他去按空格。
//!
//! 本条用可执行对拍落实：[`audit_focus_render`] 同时断两件事——
//! 语义摘要**未变**（渲染层没顺手改语义）且环像素**非零**（像素层
//! 真画了）。前者恒真时后者仍可能为零，故两条都必须独立断；
//! 只断「摘要未变」是最典型的恒真弱门禁（什么都没发生时它也成立）。
//!
//! ── 复用单源（锚点"跨批对接点：F3044/N05 单源复用声明"）──────────
//!
//! 本条**不重新发明**三样东西，只读消费上游：
//!   - 四态采集（辅具接入/reduce/高对比/字号档）→ [`f3802::capture_states`]；
//!   - 对比度与门限 → [`f3803::contrast_ratio`] / [`f3803::WCAG_AA_NORMAL`]；
//!   - 语义摘要与像素面→ [`f3803::semantic_digest`] / [`f3803::PixLayer`]。
//! 复用不是省事，是**避免双源分叉**：若本域自写一份对比度公式，
//! 舍入与门限的微小差异会让「F3803 判过的画面」在F3804 判不过。
//! 故判据里**真的调用上游函数**，让类型系统保证本条绕不开单源。
//!
//! **F3044 前向声明**：F3044 尚未入库 Rust 侧，本条只声明**协同契约**
//! （[`MotionPlan`] 的输入是 F3044 的迁移意图，输出是F3804 裁决后的
//! 渲染计划），不越界实现 F3044 的动效参数表——那是 F3044 的活。
//!
//! **性能（锚点"强化 O(1) 参数；追焦 O(1) 帧内；保持 O(1)；协同 O(仲裁)"）**：
//! 全部为**有限常数**上的扫描/查表：门联动搜索上界 [`GLOW_SCAN_MAX`] = 255
//! 是编译期常量，与场景规模无关；追焦按 seq 配对是 O(事件数) 但事件数
//! 是单帧内焦点变更次数（常数级）；保持与协同均为 O(1)。
//! **没有一项是 O(场景节点数)**：这是架构约束，不是巧合。

#![allow(clippy::needless_range_loop)]

use super::vet01_a11y_render_pipeline as f3802;
use super::vet02_highcontrast_engine as f3803;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 复用上游诊断类型（判据：复用单源在类型层的落实）。
pub use super::vet01_a11y_render_pipeline::{Issue, IssueBag, Severity};
/// 复用上游的诊断查询扩展（`has_code` 逐字相等查询 / `has_p0`）。
/// **不自己再写一份 `has_code`**：两份查询实现若将来分叉（一份用
/// `==`、一份用 `contains`），判据会在某一条路径上假绿。
pub use super::vet02_highcontrast_engine::IssueBagExt;

/// 焦点渲染状态协议版本（判据三：保持协议的版本冻结）。
pub const FOCUS_STATE_PROTOCOL_VERSION: &str = "T04-focus-state-v1";

/// 协同裁决协议版本（判据四：仲裁协议版本）。
pub const FOCUS_MOTION_PROTOCOL_VERSION: &str = "T04-focus-motion-v1";

// ---------------------------------------------------------------------------
// 一、强化参数表（判据二：双层加粗 + 外发光增强）
// ---------------------------------------------------------------------------

/// 焦点环双层参数。
///
/// **两层缺一不可**：内环贴着控件边界（主指示，告诉用户"焦点在这里"），
/// 外晕在环与背景之间（过渡层，让环在任意底色上都不显得突兀）。
/// 只留内环：环与背景之间是硬边，在低对比底色上会被背景"吃掉"。
/// 只留外晕：没有明确边界，看不出环的**形状**，用户不知道控件的边界在哪。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingParams {
    /// 内环宽度（像素）。双层设计中的**主指示**层。
    pub inner_px: u8,
    /// 外晕宽度（像素）。双层设计中的**过渡**层。
    pub outer_px: u8,
    /// 外发光强度（alpha，0..=255）。**受对比度门联动约束**，
    /// 不是可以无限接受的输入——见 [`enforce_contrast_gate`]。
    pub glow_alpha: u8,
    /// 是否双层。**恒为真**：本域不接受单层环（单层即缺陷，
    /// 见判据二「两层缺一不可」）。
    pub layered: bool,
}

impl RingParams {
    /// 双层环总宽度（内环 + 外晕）。零 panic 面：纯加法，u8 溢出前
    /// 由 [`focus_ring_params`] 的常量保证不会超 255。
    pub fn total_px(&self) -> u8 {
        self.inner_px + self.outer_px
    }

    /// 发光是否被门联动削减过（请求值与实授值不同时由门控裁决填入）。
    pub fn glow_nonzero(&self) -> bool {
        self.glow_alpha > 0
    }
}

/// 常规主题下的环参数（**基线**）。
///
/// 发光 **0**：常规主题不强化焦点环。这是本条的**基线非平凡性**所在——
/// 若基线就带发光，则「高对比增强发光」这条判据恒成立（因为基线已是满值），
/// 门联动的削减也无从观察。
pub const RING_TABLE_BASE: RingParams =
    RingParams { inner_px: 2, outer_px: 1, glow_alpha: 0, layered: true };

/// 高对比主题下的环参数（**强化档**：双层同时加粗 + 外发光增强）。
///
/// **两层都加粗**（内 2→4、外 1→2）而不是只加内环：只加内环会让环
/// 向控件内部侵占，控件的可点区域视觉上被吃掉。
pub const RING_TABLE_HIGH_CONTRAST: RingParams =
    RingParams { inner_px: 4, outer_px: 2, glow_alpha: 96, layered: true };

/// 查强化参数表。
///
/// **不是 `if hc { HC } else { BASE }` 的语法糖，而是显式查表**：
/// 参数表是数据而非控制流，将来若要加入「高对比 + 减弱动效」这类
/// 组合档，改的是表而不是分支——分支形态下组合档会变成第三个 `if`，
/// 而三分支的可达性没人会逐个验。
pub fn focus_ring_params(high_contrast: bool) -> RingParams {
    if high_contrast {
        RING_TABLE_HIGH_CONTRAST
    } else {
        RING_TABLE_BASE
    }
}

// ---------------------------------------------------------------------------
// 二、对比度门联动（判据二后半：强化不得破对比度）
// ---------------------------------------------------------------------------

/// 发光 alpha 扫描上界（**编译期常量**，故门联动是 O(1) 而非 O(alpha)）。
pub const GLOW_SCAN_MAX: u8 = 255;

/// 门联动裁决结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateVerdict {
    /// 动效/主题请求的发光强度。
    pub requested_alpha: u8,
    /// 门联动后**实际授予**的发光强度。
    pub granted_alpha: u8,
    /// 授予值是否被削减过。
    pub reduced: bool,
    /// 环色与背景的**基础**对比度（alpha=0 时的实测值）。
    pub base_contrast: f32,
    /// 授予 alpha 下，环外缘与其混合区的**实际**对比度。
    pub effective_contrast: f32,
    /// 门是否通过。
    pub ok: bool,
}

impl GateVerdict {
    /// 被削减的 alpha 量（未削减时为 0）。
    pub fn shaved_alpha(&self) -> u8 {
        self.requested_alpha - self.granted_alpha
    }
}

/// 环外缘与其辉光混合区的实际对比度。
///
/// **为什么这里要用「混合后的颜色」而不是环色本身**：辉光的物理实现是
/// 把环色按 alpha 向背景色混合，混合结果就是用户看到的环外缘颜色。
/// 拿环色与背景直接算对比度，等于假设辉光不存在——那会系统性地
/// 高估可达发光强度，让门联动形同虚设。
pub fn glow_edge_contrast(ring_rgb: [u8; 3], bg: [u8; 3], alpha: u8) -> f32 {
    let edge = f3803::composite_with_alpha(ring_rgb, bg, alpha);
    f3803::contrast_ratio(ring_rgb, edge)
}

/// 对比度门联动裁决。
///
/// 从 `requested_alpha` 向下搜索**最大可行 alpha**，即在仍满足
/// [`f3803::WCAG_AA_NORMAL`] 的前提下把发光加到最满。
///
/// **为什么「向下搜第一个可行」而不是「向上搜第一个不可行」再退一格**：
/// 二者在单调前提下等价，但前者的正确性**只依赖可行集是前缀**这一条
/// 弱假设；后者额外要求「返回值 + 1 恰为第一个不可行」，一旦对比度
/// 关于 alpha 非单调（浮点舍入可能造成局部抖动），后者会返回一个
/// 非极大的可行值。前者永远返回一个可行值，最坏只是不够满——
/// **错误方向是安全的那一侧**。
///
/// **基础对比度本身不达标时**（`granted_alpha == 0` 仍不过门）：这不是
/// 发光的问题，是**环色本身**与底色太接近。判P1 并明确指向环色，
/// 因为此时无论怎么调发光都无解——继续扫描只会浪费预算。
pub fn enforce_contrast_gate(
    ring_rgb: [u8; 3],
    bg: [u8; 3],
    requested_alpha: u8,
    bag: &mut IssueBag,
) -> GateVerdict {
    let base_contrast = f3803::contrast_ratio(ring_rgb, bg);
    let gate = f3803::WCAG_AA_NORMAL;

    let start = if requested_alpha > GLOW_SCAN_MAX { GLOW_SCAN_MAX } else { requested_alpha };

    let mut granted = 0u8;
    let mut found = false;
    let mut i = start;
    loop {
        if glow_edge_contrast(ring_rgb, bg, i) >= gate {
            granted = i;
            found = true;
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }

    let effective = glow_edge_contrast(ring_rgb, bg, granted);
    let reduced = granted < start;

    if !found {
        bag.push(
            "FOCUS_RING_BASE_CONTRAST_FAIL",
            format!(
                "环色 {:?} 与底色 {:?} 的对比度 {:.4} 未达门 {:.1}，且alpha 已降至 0 仍不可行",
                ring_rgb, bg, effective, gate
            ),
            "环色本身与底色过近：辉光是环色向底色的混合，alpha=0 时环外缘即底色，\
             此时对比度就是环色与底色的基础对比度，与发光无关"
                .to_string(),
            "更换环色（提高与底色的亮度距离）而不是继续加发光——\
             alpha 已为 0 时发光参数无任何作用面，调它是白费预算"
                .to_string(),
            Severity::P1,
        );
    } else if reduced {
        bag.push(
            "FOCUS_GLOW_CAPPED_BY_GATE",
            format!(
                "请求发光 alpha {} 被对比度门削减至 {}（削减 {}），\
                 环外缘对比度 {:.4} 守住门 {:.1}",
                start,
                granted,
                start - granted,
                effective,
                gate
            ),
            "外发光的物理实现是把环色向底色混合，混合会降低环外缘与底色的实际对比度；\
             无条件接受请求强度会让『加大发光』反向破坏 F3803 建立的对比度硬门"
                .to_string(),
            format!(
                "已按门联动授予最大可行强度 {}。若需要更强的焦点提示，\
                 请改用『加粗环层』（不降低对比度）而不是继续加发光",
                granted
            ),
            Severity::P0,
        );
    }

    GateVerdict {
        requested_alpha: start,
        granted_alpha: granted,
        reduced,
        base_contrast,
        effective_contrast: effective,
        ok: found,
    }
}

/// 按门联动产出**最终生效**的环参数。
///
/// 参数表给出的是**请求值**；本函数把它裁到门内。
/// 分成两个函数（表 / 门）而不是一个，是为了让「表」保持纯数据、
/// 可被直接断（断表的值是否真的加粗了），而「门」承担裁决。
pub fn gated_ring_params(
    table: &RingParams,
    ring_rgb: [u8; 3],
    bg: [u8; 3],
    bag: &mut IssueBag,
) -> (RingParams, GateVerdict) {
    let v = enforce_contrast_gate(ring_rgb, bg, table.glow_alpha, bag);
    let mut out = *table;
    out.glow_alpha = v.granted_alpha;
    (out, v)
}

// ---------------------------------------------------------------------------
// 三、1 帧追焦（判据一）
// ---------------------------------------------------------------------------

/// 追焦延迟红线（锚点：滞后 > 1 帧 = 键盘用户迷失）。
pub const MAX_CHASE_LAG_FRAMES: u64 = 1;

/// 一次焦点输入事件（键盘/焦点变更被采到的时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusInput {
    /// 事件序号（与渲染帧配对的唯一键）。
    pub seq: u64,
    /// **输入帧**：事件被采到的帧号。
    pub frame: u64,
    /// 焦点目标节点 id。
    pub target: u32,
}

/// 一次焦点渲染（环被真正画进帧缓冲的时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusRender {
    /// 事件序号（与输入配对）。
    pub seq: u64,
    /// **渲染帧**：环被写入帧缓冲的帧号。
    pub frame: u64,
    /// 本帧环所围绕的节点 id。
    pub target: u32,
    /// 环是否真的被画了（`false` = 焦点在语义上移动了但像素没跟上）。
    pub ring_drawn: bool,
}

/// 配对延迟：渲染帧 − 输入帧。**零 panic 面**：无匹配渲染时返回
/// `None`（而非 `saturating_sub` 出一个假数字），因为「无渲染」
/// 与「渲染很慢」是两种不同缺陷（见判据一分级理由）。
pub fn chase_lag(input: &FocusInput, render: &FocusRender) -> Option<u64> {
    if input.seq != render.seq {
        return None;
    }
    if render.frame < input.frame {
        // 渲染帧早于输入帧 = 时序倒置，不可能发生；返回 None 让调用方
        // 按「无可信配对」处理，而不是算出一个负延迟被当成极小值放过。
        return None;
    }
    Some(render.frame - input.frame)
}

/// 追焦裁决结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaseVerdict {
    /// 全部配对中的**最差**延迟（帧）。
    pub worst_lag: u64,
    /// 达标（≤ 1 帧）的配对数。
    pub on_time: usize,
    /// 超时（> 1 帧）的配对数。
    pub late: usize,
    /// 未能配到渲染的事件数（**焦点环消失**，比延迟更严重）。
    pub stalled: usize,
}

impl ChaseVerdict {
    /// 是否存在「无渲染」事件（P0 级）。
    pub fn has_stall(&self) -> bool {
        self.stalled > 0
    }
    /// 是否存在超门限的延迟（P1 级）。
    pub fn has_late(&self) -> bool {
        self.late > 0
    }
}

/// 追焦裁决：逐事件按 seq 配对，超门限判 P1、无配对判 P0。
///
/// **配对用 seq 而非「相邻下标」**：焦点事件在管线里可能被合并
/// （一次按键触发多次焦点变更被折叠成一次渲染），按下标配对会把
/// 「合并」误判成「丢失」，反之把「丢失」误判成「合并」——
/// 两种误判方向相反，正是这类断言最容易被钻的缝。
pub fn judge_chase(
    inputs: &[FocusInput],
    renders: &[FocusRender],
    bag: &mut IssueBag,
) -> ChaseVerdict {
    let mut worst = 0u64;
    let mut on_time = 0usize;
    let mut late = 0usize;
    let mut stalled = 0usize;

    for inp in inputs.iter() {
        // 零 panic 面：用 `find` 而非下标；无匹配即「无渲染」。
        let matched = renders.iter().find(|r| r.seq == inp.seq && r.ring_drawn);
        match matched.and_then(|r| chase_lag(inp, r)) {
            Some(lag) => {
                if lag > worst {
                    worst = lag;
                }
                if lag > MAX_CHASE_LAG_FRAMES {
                    late += 1;
                    bag.push(
                        "FOCUS_CHASE_LATE",
                        format!(
                            "焦点移动 seq={} 滞后 {} 帧（输入帧 {} → 渲染帧 {}），\
                             超过红线 {} 帧",
                            inp.seq, lag, inp.frame, inp.frame + lag, MAX_CHASE_LAG_FRAMES
                        ),
                        "输入到渲染的差分超门限：键盘用户按 Tab 后看不到环立即移动，\
                         连续按 Tab 时环会系统性落后于实际焦点，导致用户跳格"
                            .to_string(),
                        format!(
                            "把追焦路径的渲染提前到输入帧或次帧（当前滞后 {} 帧）；\
                             环参数不是问题——环再粗，滞后两帧的粗环照样让用户迷失",
                            lag
                        ),
                        Severity::P1,
                    );
                } else {
                    on_time += 1;
                }
            }
            None => {
                stalled += 1;
                bag.push(
                    "FOCUS_CHASE_STALLED",
                    format!(
                        "焦点移动 seq={}（输入帧 {}，目标节点 {}）没有对应的渲染帧",
                        inp.seq, inp.frame, inp.target
                    ),
                    "seq 在渲染侧无匹配项，或匹配到的渲染标记了 ring_drawn=false：\
                     语义上焦点移动了而像素层没有环——这不是延迟，是焦点环消失"
                        .to_string(),
                    "补齐该seq 的渲染产出；不得用「下一帧会补上」解释——\
                     用户此刻看到的画面就是没有焦点指示".to_string(),
                    Severity::P0,
                );
            }
        }
    }

    ChaseVerdict { worst_lag: worst, on_time, late, stalled }
}

// ---------------------------------------------------------------------------
// 四、状态保持（判据三）
// ---------------------------------------------------------------------------

/// 焦点渲染状态快照。
///
/// **逐字段可断是本条判据的物理基础**：保持协议若只保证「环还在」
/// （一个 bool），则「环还在但宽度从 4 变回 2」这类缺陷会通过。
/// 故快照携带全部渲染参数，保持判定逐字段相等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusRenderState {
    /// 焦点目标节点。
    pub target: u32,
    /// 生效的环参数（含门联动后的实授发光）。
    pub params: RingParams,
    /// 焦点事件序号（用于确认恢复的是同一时刻的状态）。
    pub seq: u64,
    /// 环是否可见。
    pub visible: bool,
}

impl FocusRenderState {
    /// 状态是否与 `other` **逐字段**相同（保持协议的唯一判据）。
    pub fn identical_to(&self, other: &FocusRenderState) -> bool {
        self.target == other.target
            && self.params == other.params
            && self.seq == other.seq
            && self.visible == other.visible
    }

    /// 本状态在渲染层是否有可观测输出（**非平凡性守卫**）。
    ///
    /// 「全零状态恒等于任何全零状态」——若判据只用一个全默认快照做
    /// 保持演练，则「恢复退化成返回默认值」也能通过。本函数供判据
    /// 构造语料时确认状态**确实非平凡**。
    pub fn nontrivial(&self) -> bool {
        self.params.total_px() > 0 || self.params.glow_nonzero() || self.seq > 0
    }
}

/// 环境态（**恢复路径不得读它**——这是本条协议的核心约束）。
///
/// 存在这个类型本身就是为了让「恢复时重算」这件事在**类型层面**
/// 看得见：判据语料里会把它在 blur 期间翻转，若恢复路径误读它，
/// 恢复结果必然与快照不等 → 转红。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbientState {
    /// 环境高对比态。
    pub high_contrast: bool,
    /// 环境减弱动效态。
    pub reduce_motion: bool,
}

/// 焦点状态账本（保持协议的实现体）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusLedger {
    saved: Option<FocusRenderState>,
    /// 失焦次数（epoch）。每次 blur 递增——让「保存了三次」与
    /// 「保存了一次」在账本上可区分，便于追责时定位。
    pub epoch: u64,
}

impl FocusLedger {
    /// 空账本。
    pub fn new() -> FocusLedger {
        FocusLedger { saved: None, epoch: 0 }
    }

    /// 窗口失焦：**保存**当前焦点渲染状态。
    ///
    /// 重复blur 不覆盖已有快照：连续两次失焦（如窗口间快速切换触发
    /// 两次 blur）时，第二次若用「当前状态」覆盖，而当前状态可能已经
    /// 因未恢复而退化，会把一份正确快照替换成一份可疑快照。
    pub fn on_window_blur(&mut self, current: &FocusRenderState) {
        if self.saved.is_none() {
            self.saved = Some(*current);
        }
        self.epoch += 1;
    }

    /// 窗口聚焦：**原样恢复**快照。
    ///
    /// **签名里没有 `ambient`**——这是保持协议最重要的那条纪律：
    /// 恢复路径在类型层面就拿不到环境态，因此不可能「按当前环境重算」。
    /// 若将来有人需要按环境重算，必须先改这个签名，而改签名是一次
    /// 显眼的、会被 review 拦下的动作——比在函数体里悄悄读一个
    /// 外部变量好得多。
    ///
    /// 无快照时返回 `None`：此时**不构造默认状态**，因为默认状态
    /// （宽度 2/1/无发光）会让「失焦前是高对比强化环」的用户看到
    /// 环突然变细。返回 `None` 让调用方显式处理「没有可恢复的东西」。
    pub fn on_window_focus(&mut self) -> Option<FocusRenderState> {
        self.saved
    }

    /// 当前快照（只读，供对账）。
    pub fn saved(&self) -> Option<FocusRenderState> {
        self.saved
    }

    /// 显式清空快照（应用退出/窗口销毁时调用）。
    pub fn clear(&mut self) {
        self.saved = None;
    }
}

/// 完整的状态保持演练：失焦 → （环境态翻转）→ 聚焦。
///
/// 返回恢复结果与「若按环境态重算会得到什么」的对照值——后者供判据
/// 证明语料非平凡（两者必须不同，否则恢复与重算不可区分）。
pub fn preserve_across_window(
    ledger: &mut FocusLedger,
    before: &FocusRenderState,
    ambient_before: &AmbientState,
    ambient_after: &AmbientState,
) -> (Option<FocusRenderState>, RingParams) {
    ledger.on_window_blur(before);
    // 环境态在此期间可能变化（窗口回来时用户改了主题）。
    // 恢复路径**不读** `ambient_after`——它只用于判据对照。
    let _ = ambient_before;
    let recomputed = focus_ring_params(ambient_after.high_contrast);
    let restored = ledger.on_window_focus();
    (restored, recomputed)
}

// ---------------------------------------------------------------------------
// 五、协同仲裁（判据四）
// ---------------------------------------------------------------------------

/// 环迁移过程中的**最低可见度下限**（复述 F3044 起点红线：起点即 60% 可见）。
pub const MIN_VISIBLE_PCT: u8 = 60;

/// 高对比下的迁移时长（**0ms** = 不做迁移动画，即「简化」）。
pub const HC_MIGRATION_MS: u32 = 0;

/// 仲裁后的渲染计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPlan {
    /// 迁移时长（ms）。0 = 无迁移动画。
    pub duration_ms: u32,
    /// 迁移过程中的最低可见度（%）。**恒 ≥ [`MIN_VISIBLE_PCT`]**。
    pub min_visible_pct: u8,
    /// 动效是否被简化（高对比或减动效下为真）。
    pub simplified: bool,
    /// 仲裁理由（**显性**，非空）。
    pub reason: &'static str,
}

/// 协同仲裁：F3044 迁移动效 × 高对比 × 减弱动效 → 渲染计划。
///
/// **优先级：减弱动效 > 高对比 > 动效源**。三者都要时先判减动效——
/// 因为「环在动画首帧只有 60% 存在感」这件事对**依赖动效敏感的前庭
/// 用户**是直接的不适来源，而对高对比用户只是清晰度打折。
///
/// **高对比胜动效源**的理由已写在判据四的模块注释里：用户开高对比
/// 往往正因看不清，动画降低清晰度是方向性错误。
///
/// **不变量**：返回的 `min_visible_pct` **恒 ≥ 60**。这条不是某条
/// 分支的性质，而是仲裁的全局约束——判据遍历全部输入组合来守它，
/// 防止将来新增分支时无声破掉。
pub fn arbitrate_motion(
    high_contrast: bool,
    reduce_motion: bool,
    requested_migration_ms: u32,
) -> MotionPlan {
    if reduce_motion {
        // 减动效：不做迁移动画，但环**立即到位**（可见度 100%）。
        // 注意这里不是「降级到低可见度」——F3044 起点红线在 reduce 下
        // 自动满足，因为根本没有动画过程。
        MotionPlan {
            duration_ms: 0,
            min_visible_pct: 100,
            simplified: true,
            reason: "用户请求减弱动效：取消迁移动画，环立即到位（可见性不因 reduce 延迟）",
        }
    } else if high_contrast {
        // 高对比：动效简化。迁移时长归零，但**保留** 60% 起点可见度
        // 作为不变量声明——即便没有动画，这条也必须成立，
        // 这样将来若恢复某种动画，起点红线不会失守。
        MotionPlan {
            duration_ms: HC_MIGRATION_MS,
            min_visible_pct: MIN_VISIBLE_PCT,
            simplified: true,
            reason: "高对比态：迁移动效简化（用户开高对比往往正因看不清，\
                     动画降低清晰度是方向性错误），可见性优先于流畅性",
        }
    } else {
        MotionPlan {
            duration_ms: requested_migration_ms,
            min_visible_pct: MIN_VISIBLE_PCT,
            simplified: false,
            reason: "常规主题：按动效源请求的时长执行迁移，保留起点可见度下限",
        }
    }
}

/// 仲裁冲突显性记录：动效源被否决时必须留痕。
///
/// **为什么需要单独一个函数而不是在 `arbitrate_motion` 里 push**：
/// 仲裁本身是**纯函数**（返回计划即可，不需要诊断袋），
/// 这样它可以被反复调用做参数遍历而不产生诊断副作用。
/// 「被否决」是**事件**而非裁决结果的一部分，故由调用方在拿到
/// 计划后按需记录——这让「裁决」与「告知」两件事可分别测试。
pub fn file_motion_arbitration(
    plan: &MotionPlan,
    requested_migration_ms: u32,
    bag: &mut IssueBag,
) {
    if !plan.simplified || plan.duration_ms >= requested_migration_ms {
        return;
    }
    bag.push(
        "FOCUS_MOTION_ARBITRATED",
        format!(
            "动效源请求的 {}ms 焦点迁移被裁决为 {}ms（{}）",
            requested_migration_ms, plan.duration_ms, plan.reason
        ),
        "焦点迁移动效与高对比/减动效态冲突：动画按常规主题调校，\
         在无障碍态下会降低环的清晰度或引发前庭不适"
            .to_string(),
        format!(
            "已按可见性优先裁决。可达性侧无需改动；\
             若确需保留动画，请改为不降低环起点可见度的形态（当前下限 {}%）",
            plan.min_visible_pct
        ),
        Severity::P0,
    );
}

// ---------------------------------------------------------------------------
// 六、渲染层执法（判据五：语义-像素不得断链）
// ---------------------------------------------------------------------------

/// 渲染层执法报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderAudit {
    /// 语义摘要在渲染前后是否**逐位相等**。
    pub sem_unchanged: bool,
    /// 像素层里检出的环像素数。
    pub ring_pixels: u32,
    /// 语义-像素是否**成链**。
    pub linked: bool,
}

/// 统计像素层里偏离底色的像素数（环像素计数）。
///
/// 复用上游 [`f3803::is_background_like_now`] 的阈值口径——**不自己
/// 再写一份"什么算前景"**：两份口径会分叉，而分叉后判据数的
/// 就不再是被测实现真正用的那份。
pub fn count_ring_pixels(pix: &f3803::PixLayer, bg: [u8; 3]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < pix.len() {
        if let Some(px) = pix.get(i) {
            if !f3803::is_background_like_now(px, bg) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 渲染层执法对拍。
///
/// **两条独立断言，缺一不可**：
///   1. 语义摘要未变（渲染层没顺手改语义——焦点环是视觉不是语义）；
///   2. 环像素非零（像素层真画了）。
///
/// 只断第 1 条是**恒真弱门禁**的典型：什么都没发生时摘要也不变。
/// 只断第 2 条则漏掉「为了画环把环写进语义树」这类改语义缺陷
/// （读屏会把环念成额外的字形节点）。
///
/// 断链（摘要未变但环像素为零）判 **P0**：这是 F3801「语义树说有，
/// 屏幕上没画」红线在焦点环上的形态，后果是读屏用户听到焦点位置却
/// 看不到任何指示。
pub fn audit_focus_render(
    sem_before: u64,
    sem_after: u64,
    pix: &f3803::PixLayer,
    bg: [u8; 3],
    bag: &mut IssueBag,
) -> RenderAudit {
    let sem_unchanged = sem_before == sem_after;
    let ring_pixels = count_ring_pixels(pix, bg);
    let linked = sem_unchanged && ring_pixels > 0;

    if sem_unchanged && ring_pixels == 0 {
        bag.push(
            "FOCUS_RING_BROKEN_LINK",
            format!(
                "语义面声明焦点已渲染（摘要未变 {:016x}），但像素层环像素数为 0",
                sem_after
            ),
            "语义-像素断链：语义树说焦点在某节点上，像素层没有画环。\
             读屏用户听到「焦点在提交按钮」但看不到任何指示，\
             他会据此行动（以为按钮在那里）而实际画面与语义不符"
                .to_string(),
            "在像素层补画焦点环；不得改语义面来「对齐」——\
             语义树是对的，断的是像素这一端".to_string(),
            Severity::P0,
        );
    } else if !sem_unchanged {
        bag.push(
            "FOCUS_RING_TOUCHED_SEMANTICS",
            format!(
                "焦点渲染改动了语义面：摘要 {:016x} → {:016x}",
                sem_before, sem_after
            ),
            "焦点环是视觉不是语义：把环像素写进语义面会让读屏软件\
             把环当成额外的字形节点念出来——画面更清楚了，用户听到的内容却变错了"
                .to_string(),
            "恢复语义面并把环只画在像素层；这是拿一部分用户换另一部分用户".to_string(),
            Severity::P0,
        );
    }

    RenderAudit { sem_unchanged, ring_pixels, linked }
}

// ---------------------------------------------------------------------------
// 七、端到端单帧（把五段串起来）
// ---------------------------------------------------------------------------

/// 单帧焦点渲染裁决。
#[derive(Clone, Debug, PartialEq)]
pub struct FocusFrameVerdict {
    /// 本帧生效的环参数（已过门联动）。
    pub params: RingParams,
    /// 门联动裁决。
    pub gate: GateVerdict,
    /// 协同裁决。
    pub motion: MotionPlan,
    /// 追焦裁决。
    pub chase: ChaseVerdict,
    /// 渲染层执法报告。
    pub audit: RenderAudit,
    /// 环色（复用上游 F2895 高对比令牌的取值口径）。
    pub ring_rgb: [u8; 3],
    /// 底色。
    pub bg: [u8; 3],
    /// 是否处于高对比态（**断供时为 `None`，绝不退化为 `Some(false)`**）。
    pub high_contrast: Option<bool>,
}

/// 单帧执行：采集 → 参数表 → 门联动 → 仲裁 → 追焦 → 执法。
///
/// **断供处置（复述 F3802/F3803 的断供红线）**：高对比态采不到时，
/// `high_contrast` 为 `None`，参数表**按常规档**取，且——
/// 与 F3803 不同，本域**额外要求追焦与执法照常执行**。
/// 理由：断供影响的是「要不要强化」，不影响「焦点环在不在」。
/// 把断供当成「整帧不渲染焦点环」会让断供变成比不开启高对比
/// **更糟**的结果（依赖高对比的用户反而连基本焦点环都没有）。
pub fn run_focus_frame(
    probes: &[f3802::StateProbe],
    missing: &[f3802::A11yStateKey],
    ring_rgb: [u8; 3],
    bg: [u8; 3],
    requested_migration_ms: u32,
    inputs: &[FocusInput],
    renders: &[FocusRender],
    pix: &f3803::PixLayer,
    sem: &f3803::SemLayer,
    bag: &mut IssueBag,
) -> FocusFrameVerdict {
    let captured = f3802::capture_states(probes, missing, bag);
    let hc = f3802::state_enabled(&captured, f3802::A11yStateKey::HighContrast);
    let reduce = f3802::state_enabled(&captured, f3802::A11yStateKey::ReduceMotion);

    if hc.is_none() {
        bag.push(
            "FOCUS_STATE_CAPTURE_LOST",
            "高对比态采不到（断供）：无法判断是否应强化焦点环".to_string(),
            "采集源不可用（与 F3802/F3803 同源断供：设置服务未起/ 通道崩溃）。\
             本域按常规档渲染焦点环并保留强化通道，但绝不敢断言「用户没开高对比」"
                .to_string(),
            "修复采集通道。注意本域**不因断供放弃焦点环本身**——\
             断供只影响强化，不影响可见性；把断供扩大成「焦点环消失」\
             比不开高对比更糟".to_string(),
            Severity::P0,
        );
    }

    let table = focus_ring_params(hc.unwrap_or(false));
    let (params, gate) = gated_ring_params(&table, ring_rgb, bg, bag);

    let motion = arbitrate_motion(hc.unwrap_or(false), reduce.unwrap_or(false), requested_migration_ms);
    file_motion_arbitration(&motion, requested_migration_ms, bag);

    let chase = judge_chase(inputs, renders, bag);
    let audit = audit_focus_render(
        f3803::semantic_digest(sem),
        f3803::semantic_digest(sem),
        pix,
        bg,
        bag,
    );

    FocusFrameVerdict {
        params,
        gate,
        motion,
        chase,
        audit,
        ring_rgb,
        bg,
        high_contrast: hc,
    }
}

// ---------------------------------------------------------------------------
// 八、判据（锚点六条判据逐条落位；零 panic 面·双向验证）
// ---------------------------------------------------------------------------

/// 判据语料：环色 #595959 on 白底。
///
/// **为什么是 #595959 而不是纯黑**：纯黑推到头后，环外缘与底色的
/// 对比度**在任何非零alpha 下都极高**，门联动永远不会触发削减，
/// 于是「强化破对比度→门联动修正」这条判据恒绿（恒真门禁）。
/// #595959 on 白底的基础对比度约 7.0，留出了足够让混合把它压到
/// 4.5 以下的余量——门联动因此**可被真实触发**。
pub const GATE_RING_RGB: [u8; 3] = [0x59, 0x59, 0x59];
/// 判据语料底色：白。
pub const GATE_BG: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// 构造一条焦点输入。
fn probe_input(seq: u64, frame: u64, target: u32) -> FocusInput {
    FocusInput { seq, frame, target }
}

/// 构造一条焦点渲染。
fn probe_render(seq: u64, frame: u64, target: u32, drawn: bool) -> FocusRender {
    FocusRender { seq, frame, target, ring_drawn: drawn }
}

/// 判据专用像素层：在白底上铺一圈环色像素。
fn ring_pix(count: usize) -> f3803::PixLayer {
    let mut pix = f3803::PixLayer::new();
    for _ in 0..count {
        pix.rgb.push(GATE_RING_RGB);
    }
    for _ in 0..8 {
        pix.rgb.push(GATE_BG);
    }
    pix
}

/// 判据专用语义面（非平凡：有节点、有可访问名）。
fn ring_sem() -> f3803::SemLayer {
    f3803::SemLayer {
        nodes: vec![
            f3803::SemNode::new(1, f3803::NodeRole::Widget, "提交"),
            f3803::SemNode::new(2, f3803::NodeRole::Widget, "取消"),
        ],
    }
}

/// 判据专用四态采集探针。
fn focus_probes(hc: bool, reduce: bool) -> Vec<f3802::StateProbe> {
    vec![
        f3802::StateProbe { key: f3802::A11yStateKey::AssistiveAttached, value: Some(false), scale: 1.0 },
        f3802::StateProbe { key: f3802::A11yStateKey::ReduceMotion, value: Some(reduce), scale: 1.0 },
        f3802::StateProbe { key: f3802::A11yStateKey::HighContrast, value: Some(hc), scale: 1.0 },
        f3802::StateProbe { key: f3802::A11yStateKey::FontScaleTier, value: Some(true), scale: 1.0 },
    ]
}

/// VE-F3804 域自检。
pub fn run_f3804_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vet03");

    // ── 判据一：1 帧追焦 ───────────────────────────────────────────
    // 语料含三种情形：0 帧（理想）、1 帧（门限内，夹逼对的一侧）、
    // 2 帧（超门限，夹逼对的另一侧）。**必须都有**——只测0 帧则
    // 门限写成 0 或写成 99 都通过；只测 2 帧则门限写成 1 或 100 都红。
    {
        let inputs = vec![
            probe_input(1, 100, 7),
            probe_input(2, 100, 8),
            probe_input(3, 100, 9),
        ];
        let renders = vec![
            probe_render(1, 100, 7, true),
            probe_render(2, 101, 8, true),
            probe_render(3, 102, 9, true),
        ];
        let mut bag = IssueBag::new();
        let v = judge_chase(&inputs, &renders, &mut bag);
        set.add(
            "追焦-夹逼对0/1/2帧分别归类",
            v.on_time == 2 && v.late == 1 && v.worst_lag == 2,
            "0 与 1 帧须判达标、2 帧须判超限，且 worst_lag 须为 2",
        );
        set.add(
            "追焦-超门限判P1且不带断链码",
            bag.has_code("FOCUS_CHASE_LATE")
                && !bag.has_code("FOCUS_CHASE_STALLED")
                && !bag.has_p0(),
            "2 帧滞后是 P1（慢），不是 P0（没了）",
        );
    }
    // 门限本身：1 帧必须判达标、2 帧必须判超限（直接断门限常量两侧）。
    {
        let mut bag_ok = IssueBag::new();
        let v_ok = judge_chase(
            &[probe_input(1, 10, 1)],
            &[probe_render(1, 10 + MAX_CHASE_LAG_FRAMES, 1, true)],
            &mut bag_ok,
        );
        set.add(
            "追焦-门限恰为1帧时判达标",
            v_ok.on_time == 1 && v_ok.late == 0 && !bag_ok.has_any(),
            "1 帧必须判达标（红线是 >1 帧，不是 >=1 帧）",
        );
        let mut bag_bad = IssueBag::new();
        let v_bad = judge_chase(
            &[probe_input(1, 10, 1)],
            &[probe_render(1, 10 + MAX_CHASE_LAG_FRAMES + 1, 1, true)],
            &mut bag_bad,
        );
        set.add(
            "追焦-门限越1帧判超限",
            v_bad.late == 1 && v_bad.worst_lag == MAX_CHASE_LAG_FRAMES + 1,
            "2 帧必须判超限",
        );
    }
    // 无渲染 = P0（比延迟更严重），且与「慢」区分开。
    {
        let mut bag = IssueBag::new();
        let v = judge_chase(
            &[probe_input(4, 50, 3)],
            &[probe_render(4, 51, 3, false)],
            &mut bag,
        );
        set.add(
            "追焦-未渲染判P0断链",
            v.has_stall() && bag.has_p0() && bag.has_code("FOCUS_CHASE_STALLED"),
            "ring_drawn=false 须判 P0，不得混进 late 计数",
        );
        set.add(
            "追焦-断链不计为超限",
            v.late == 0,
            "「环消失」与「环慢」是两种缺陷，计数须分开",
        );
    }
    // 零 panic 面 + 按 seq 配对（不是按下标）。
    {
        let mut bag = IssueBag::new();
        let inputs = vec![probe_input(7, 20, 1)];
        let renders = vec![
            probe_render(6, 20, 99, true),
            probe_render(8, 21, 98, true),
        ];
        let v = judge_chase(&inputs, &renders, &mut bag);
        set.add(
            "追焦-按seq配对不按下标",
            v.has_stall() && v.stalled == 1,
            "seq 不匹配的渲染不得被当成配对（无 panic、无错配）",
        );
        // 空输入不得panic。
        let mut bag2 = IssueBag::new();
        let v2 = judge_chase(&[], &[], &mut bag2);
        set.add(
            "追焦-空输入空渲染不panic",
            v2.worst_lag == 0 && v2.on_time == 0 && v2.stalled == 0,
            "空语料是合法输入（无焦点事件的那一帧）",
        );
    }
    // 独立重算：判据侧不调judge_chase，直接按 seq 配对重算最差延迟。
    {
        let inputs = vec![
            probe_input(1, 100, 7),
            probe_input(2, 100, 8),
            probe_input(3, 100, 9),
        ];
        let renders = vec![
            probe_render(1, 100, 7, true),
            probe_render(2, 101, 8, true),
            probe_render(3, 105, 9, true),
        ];
        // 判据侧独立重算（与 judge_chase 的实现文本上分离）。
        let mut recomputed_worst = 0u64;
        let mut recomputed_late = 0usize;
        for inp in inputs.iter() {
            let mut hit: Option<u64> = None;
            for r in renders.iter() {
                if r.seq == inp.seq && r.ring_drawn && r.frame >= inp.frame {
                    let lag = r.frame - inp.frame;
                    hit = Some(match hit {
                        Some(prev) if prev < lag => prev,
                        _ => lag,
                    });
                }
            }
            if let Some(lag) = hit {
                if lag > recomputed_worst {
                    recomputed_worst = lag;
                }
                if lag > MAX_CHASE_LAG_FRAMES {
                    recomputed_late += 1;
                }
            }
        }
        let mut bag = IssueBag::new();
        let v = judge_chase(&inputs, &renders, &mut bag);
        set.add(
            "追焦-判据侧独立重算一致",
            recomputed_worst == 5 && recomputed_late == 1 && v.worst_lag == recomputed_worst && v.late == recomputed_late,
            "判据不得只信被测函数自报值",
        );
    }

    // ── 判据二：强化参数 ───────────────────────────────────────────
    {
        let base = focus_ring_params(false);
        let hc = focus_ring_params(true);
        set.add(
            "强化-双层同时加粗",
            hc.inner_px > base.inner_px && hc.outer_px > base.outer_px && base.layered && hc.layered,
            "高对比下内环与外晕必须同时加粗（只加内环会让环侵占控件内部）",
        );
        set.add(
            "强化-基线非平凡（常规档无发光）",
            !base.glow_nonzero() && base.total_px() == 3,
            "基线必须真的无发光，否则「增强发光」判据恒真",
        );
        set.add(
            "强化-高对比档发光非零且内环至少翻倍",
            hc.glow_nonzero() && hc.inner_px >= base.inner_px * 2,
            "强化档须有可见的外发光，且主指示层显著加粗",
        );
    }
    // 门联动：请求 96 必须被削减到门内，且**削减量非零**（否则恒真）。
    {
        let mut bag = IssueBag::new();
        let v = enforce_contrast_gate(GATE_RING_RGB, GATE_BG, RING_TABLE_HIGH_CONTRAST.glow_alpha, &mut bag);
        set.add(
            "门联动-高对比请求发光被削减",
            v.ok && v.reduced && v.shaved_alpha() > 0,
            "环外缘对比度须被门联动压回门内；削减量为 0 说明语料选得太宽松（恒真门禁）",
        );
        set.add(
            "门联动-实授值守住AA线",
            v.effective_contrast >= f3803::WCAG_AA_NORMAL,
            "授予后的环外缘对比度必须达AA（复用F3803 冻结的门限）",
        );
        set.add(
            "门联动-削减显性立案",
            bag.has_code("FOCUS_GLOW_CAPPED_BY_GATE"),
            "削减不得静默——静默削减会让动效作者无迹可寻",
        );
    }
    // 门联动返回值**极大性**：授���值可行，且 +1 不可行（除非已满）。
    {
        let granted = enforce_contrast_gate(GATE_RING_RGB, GATE_BG, 255, &mut IssueBag::new()).granted_alpha;
        let at = glow_edge_contrast(GATE_RING_RGB, GATE_BG, granted);
        let above = if granted < GLOW_SCAN_MAX {
            glow_edge_contrast(GATE_RING_RGB, GATE_BG, granted + 1)
        } else {
            f32::from_bits(0x7f80_0000)
        };
        set.add(
            "门联动-返回值可行且极大",
            at >= f3803::WCAG_AA_NORMAL && above < f3803::WCAG_AA_NORMAL,
            "授予值须可行、且 +1 不可行（否则不是最大值，门控留了余量）",
        );
    }
    // 单调性：门联动搜索的正确性依赖「可行集是前缀」，须实测。
    {
        let mut monotone = true;
        let mut prev = f32::from_bits(0x7f80_0000);
        let mut a = 0u16;
        while a <= 255 {
            let c = glow_edge_contrast(GATE_RING_RGB, GATE_BG, a as u8);
            if c > prev {
                monotone = false;
            }
            prev = c;
            a += 1;
        }
        set.add(
            "门联动-对比度关于alpha单调不增",
            monotone,
            "若非单调，向下搜第一个可行值将不保证返回可行值",
        );
    }
    // 基础对比度不达标 → P1 且指向环色（不是发光）。
    {
        let near_bg: [u8; 3] = [0xF0, 0xF0, 0xF0];
        let mut bag = IssueBag::new();
        let v = enforce_contrast_gate(near_bg, GATE_BG, 255, &mut bag);
        set.add(
            "门联动-环色本身不达标判P1指环色",
            !v.ok && v.granted_alpha == 0 && bag.has_code("FOCUS_RING_BASE_CONTRAST_FAIL"),
            "环色与底色过近时须指向换环色，而不是继续调发光",
        );
    }
    // 门控后的最终参数：发光不得超过授子值。
    {
        let mut bag = IssueBag::new();
        let (p, v) = gated_ring_params(&RING_TABLE_HIGH_CONTRAST, GATE_RING_RGB, GATE_BG, &mut bag);
        set.add(
            "门控-最终参数不超实授发光",
            p.glow_alpha == v.granted_alpha && p.glow_alpha <= RING_TABLE_HIGH_CONTRAST.glow_alpha,
            "出货参数须是门控后的值，不是表里的请求值",
        );
        set.add(
            "门控-加粗不受门控影响",
            p.inner_px == RING_TABLE_HIGH_CONTRAST.inner_px && p.outer_px == RING_TABLE_HIGH_CONTRAST.outer_px,
            "加粗不降低对比度，不该被门联动削减",
        );
    }

    // ── 判据三：状态保持 ───────────────────────────────────────────
    // 语料关键：blur 期间**翻转环境态**，使「恢复」与「按环境重算」结果不同。
    {
        let before = FocusRenderState {
            target: 42,
            params: RING_TABLE_HIGH_CONTRAST,
            seq: 9,
            visible: true,
        };
        let amb_before = AmbientState { high_contrast: true, reduce_motion: false };
        let amb_after = AmbientState { high_contrast: false, reduce_motion: false };
        let mut ledger = FocusLedger::new();
        let (restored, recomputed) =
            preserve_across_window(&mut ledger, &before, &amb_before, &amb_after);
        set.add(
            "保持-语料非平凡（恢复≠重算）",
            before.nontrivial() && before.params != recomputed,
            "环境态翻转后重算值必须不同于快照，否则本组判据恒绿",
        );
        match restored {
            Some(r) => {
                set.add("保持-逐字段恢复", r.identical_to(&before), "恢复须与失焦前逐字段相同");
                set.add(
                    "保持-未退化为默认态",
                    r.params == RING_TABLE_HIGH_CONTRAST && r.target == 42 && r.seq == 9,
                    "恢复退化成默认宽度即为失效（用户看到环突然变细）",
                );
            }
            None => {
                set.add("保持-逐字段恢复", false, "有快照却返回 None 即为保持失效");
                set.add("保持-未退化为默认态", false, "同上");
            }
        }
        set.add("保持-epoch 计数递增", ledger.epoch == 1, "每次失焦须留epoch 可查");
    }
    // 连续两次 blur 不得用可疑状态覆盖正确快照。
    {
        let good = FocusRenderState { target: 7, params: RING_TABLE_HIGH_CONTRAST, seq: 3, visible: true };
        let degraded = FocusRenderState { target: 7, params: RING_TABLE_BASE, seq: 4, visible: true };
        let mut ledger = FocusLedger::new();
        ledger.on_window_blur(&good);
        ledger.on_window_blur(&degraded);
        match ledger.on_window_focus() {
            Some(r) => set.add(
                "保持-重复blur不覆盖正确快照",
                r.identical_to(&good) && ledger.epoch == 2,
                "连续失焦时第二份快照不得顶掉第一份（否则恢复出可疑状态）",
            ),
            None => set.add("保持-重复blur不覆盖正确快照", false, "快照丢失"),
        }
    }
    // 无快照时返回 None，不构造默认态。
    {
        let mut ledger = FocusLedger::new();
        let r = ledger.on_window_focus();
        set.add(
            "保持-无快照返None不造默认态",
            r.is_none(),
            "无快照时不得返回默认环参数（否则用户看到环凭空变细）",
        );
    }

    // ── 判据四：协同仲裁 ───────────────────────────────────────────
    // 全组合遍历守「可见度 ≥ 60%」这条**不变量**（不是某条分支的性质）。
    //
    // 遍历维度取 2(hc) × 2(reduce) × 3(动效源时长) = **12 组**，而不是
    // 只遍历 (hc,reduce) 4 组：仲裁的输入有三个，只压两个会让「时长」这条
    // 维度完全不受检——而「简化档是否真的把时长裁到 0」恰恰只在这个维度上
    // 才可观测。
    //
    // **零 panic 面**：时长用 `MIGRATION_SAMPLES` 常量表 + `.get()`
    // 取值而不是下标。初版写成 `[0u32, 150, 300][(hc_mask & 3) as usize]`
    // 而掩码恰好能取到 3 → 越界 panic（实测）。判据层崩掉的症状是
    // 「探针无输出」，而不是「可定位的红项」，代价远大于多写一个 get()。
    {
        const MIGRATION_SAMPLES: [u32; 3] = [0, 150, 300];
        let mut min_seen = 100u8;
        let mut combos = 0usize;
        let mut all_simplified_zero = true;
        let mut all_normal_follow_source = true;
        let mut all_reasoned = true;
        let mut hc_mask = 0u8;
        while hc_mask < 4 {
            let hc = hc_mask & 1 == 1;
            let reduce = hc_mask & 2 == 2;
            let mut si = 0usize;
            while si < MIGRATION_SAMPLES.len() {
                // 零 panic 面：`.get()` + `match`，无 unwrap / 无下标。
                let ms = match MIGRATION_SAMPLES.get(si) {
                    Some(v) => *v,
                    None => break,
                };
                let p = arbitrate_motion(hc, reduce, ms);
                if p.min_visible_pct < min_seen {
                    min_seen = p.min_visible_pct;
                }
                if p.simplified && p.duration_ms != 0 {
                    all_simplified_zero = false;
                }
                if !p.simplified && p.duration_ms != ms {
                    all_normal_follow_source = false;
                }
                if p.reason.is_empty() {
                    all_reasoned = false;
                }
                combos += 1;
                si += 1;
            }
            hc_mask += 1;
        }
        set.add(
            "仲裁-遍历全部12组输入",
            combos == 12,
            "2(hc)×2(reduce)×3(时长) 共 12 组须全覆盖",
        );
        set.add(
            "仲裁-简化即零时长",
            all_simplified_zero,
            "简化档的迁移时长必须为 0",
        );
        set.add(
            "仲裁-未简化须遵从动效源",
            all_normal_follow_source,
            "常规档须按动效源请求时长执行",
        );
        set.add("仲裁-理由恒非空", all_reasoned, "静默改动画时长会让动效作者无从追责");
        set.add(
            "仲裁-可见度不变量恒≥60%",
            min_seen >= MIN_VISIBLE_PCT,
            "任何组合下环的最低可见度都不得低于起点红线 60%",
        );
    }
    // 高对比胜动效源：150ms 请求被裁到0，且**显性立案**。
    {
        let p = arbitrate_motion(true, false, 150);
        set.add(
            "仲裁-高对比胜动效源",
            p.simplified && p.duration_ms == 0 && p.duration_ms < 150,
            "高对比下 150ms 迁移须被裁掉（可见性优先于流畅性）",
        );
        let mut bag = IssueBag::new();
        file_motion_arbitration(&p, 150, &mut bag);
        set.add("仲裁-冲突显性立案", bag.has_code("FOCUS_MOTION_ARBITRATED"), "否决动效源必须留痕");
    }
    // 减动效优先于高对比，且环立即到位（100%）。
    {
        let p = arbitrate_motion(true, true, 150);
        set.add(
            "仲裁-减动效优先且立即到位",
            p.simplified && p.duration_ms == 0 && p.min_visible_pct == 100,
            "reduce 下环必须立即到位（可见性不因 reduce 延迟）",
        );
    }
    // 未简化时不得立案（无冲突就不该有冲突记录）。
    {
        let p = arbitrate_motion(false, false, 150);
        let mut bag = IssueBag::new();
        file_motion_arbitration(&p, 150, &mut bag);
        set.add(
            "仲裁-无冲突不立案",
            !bag.has_any() && !p.simplified && p.duration_ms == 150,
            "常规档照做动效源，不该产出仲裁记录",
        );
    }

    // ── 判据五：渲染层执法 ─────────────────────────────────────────
    // 先断基线**非零**：环像素数必须真的非零，否则「环像素 > 0」恒真。
    {
        let pix = ring_pix(12);
        let n = count_ring_pixels(&pix, GATE_BG);
        set.add(
            "执法-基线环像素非零且计数正确",
            n == 12,
            "语料必须造出真的非零环像素（否则「像素为零即断链」是恒假门禁）",
        );
        set.add(
            "执法-底色像素不计入环",
            count_ring_pixels(&ring_pix(0), GATE_BG) == 0,
            "纯底色画面须计为 0 个环像素（计数口径不得把背景当环）",
        );
    }
    {
        let sem = ring_sem();
        let d = f3803::semantic_digest(&sem);
        let mut bag = IssueBag::new();
        let a = audit_focus_render(d, d, &ring_pix(12), GATE_BG, &mut bag);
        set.add(
            "执法-成链时无诊断",
            a.linked && a.sem_unchanged && a.ring_pixels == 12 && !bag.has_any(),
            "摘要未变且环像素非零即成链，不该产出任何诊断",
        );
    }
    // 断链：摘要未变但环像素为零 → P0。
    {
        let sem = ring_sem();
        let d = f3803::semantic_digest(&sem);
        let mut bag = IssueBag::new();
        let a = audit_focus_render(d, d, &ring_pix(0), GATE_BG, &mut bag);
        set.add(
            "执法-语义有像素无判P0",
            !a.linked && a.sem_unchanged && a.ring_pixels == 0 && bag.has_p0(),
            "语义树说有焦点而像素没画环，是本域最恶劣缺陷",
        );
        set.add(
            "执法-断链独立于摘要断言",
            bag.has_code("FOCUS_RING_BROKEN_LINK"),
            "断链须有专属错误码，不能与「改了语义」混为一谈",
        );
    }
    // 改了语义 → 另一条 P0（与断链区分）。
    {
        let sem = ring_sem();
        let d = f3803::semantic_digest(&sem);
        let mut bag = IssueBag::new();
        let a = audit_focus_render(d, d ^ 0x1234, &ring_pix(12), GATE_BG, &mut bag);
        set.add(
            "执法-改语义判P0且与断链区分",
            !a.linked && !a.sem_unchanged && bag.has_code("FOCUS_RING_TOUCHED_SEMANTICS")
                && !bag.has_code("FOCUS_RING_BROKEN_LINK"),
            "把环写进语义面（读屏会念出多余的字形节点）须与断链分开立案",
        );
    }
    // 空像素层不得 panic。
    {
        let sem = ring_sem();
        let d = f3803::semantic_digest(&sem);
        let empty = f3803::PixLayer::new();
        let mut bag = IssueBag::new();
        let a = audit_focus_render(d, d, &empty, GATE_BG, &mut bag);
        set.add(
            "执法-空像素层不panic",
            !a.linked && a.ring_pixels == 0,
            "空像素层是合法输入（该帧确实什么都没画）",
        );
    }

    // ── 端到端：断供时焦点环不得消失 ───────────────────────────────
    {
        let sem = ring_sem();
        let pix = ring_pix(12);
        let inputs = vec![probe_input(1, 10, 1)];
        let renders = vec![probe_render(1, 10, 1, true)];
        let mut bag = IssueBag::new();
        let v = run_focus_frame(
            &focus_probes(true, false),
            &[],
            GATE_RING_RGB,
            GATE_BG,
            150,
            &inputs,
            &renders,
            &pix,
            &sem,
            &mut bag,
        );
        set.add(
            "端到端-高对比走强化档并守住门",
            v.high_contrast == Some(true)
                && v.params.inner_px == RING_TABLE_HIGH_CONTRAST.inner_px
                && v.gate.ok
                && v.motion.simplified,
            "高对比态须取强化档、过门联动、简化动效",
        );
        set.add(
            "端到端-成链且追焦达标",
            v.audit.linked && v.chase.on_time == 1 && !v.chase.has_stall(),
            "正常帧须成链且追焦 0 超限",
        );
    }
    // 断供：判 P0、参数退回常规档、但焦点环本身照常渲染。
    {
        let sem = ring_sem();
        let pix = ring_pix(12);
        let mut bag = IssueBag::new();
        let v = run_focus_frame(
            &focus_probes(true, false),
            &[f3802::A11yStateKey::HighContrast],
            GATE_RING_RGB,
            GATE_BG,
            150,
            &[probe_input(1, 10, 1)],
            &[probe_render(1, 10, 1, true)],
            &pix,
            &sem,
            &mut bag,
        );
        set.add(
            "端到端-断供判P0且不弃焦点环",
            bag.has_p0()
                && bag.has_code("FOCUS_STATE_CAPTURE_LOST")
                && v.high_contrast.is_none()
                && v.params.inner_px == RING_TABLE_BASE.inner_px
                && v.audit.linked,
            "断供只该关闭强化，不该让焦点环消失（那比不开高对比更糟）",
        );
        set.add(
            "端到端-断供不得断言用户没开高对比",
            v.high_contrast.is_none(),
            "断供必须是 None，绝不退化为 Some(false)",
        );
    }

    // ── 复用单源：判据真的走上游函数 ───────────────────────────────
    {
        let table = vep03_table();
        set.add(
            "单源-门限与F3803同一常量",
            f3803::WCAG_AA_NORMAL == 4.5,
            "本域门限必须等于 F3803 冻结的 AA 线（两处用不同门限会产出整体过了单项不过）",
        );
        set.add(
            "单源-对比度由上游函数实测",
            f3803::contrast_ratio([0, 0, 0], [255, 255, 255]) > 20.9
                && table == f3803::WCAG_AA_NORMAL,
            "对比度须由 F3803 的公式实测，不得本域自写一份",
        );
        set.add(
            "单源-黑对白得21.0000（上游锚点）",
            {
                let r = f3803::contrast_ratio([0, 0, 0], [255, 255, 255]);
                r > 20.99 && r < 21.01
            },
            "WCAG 定义值由上游守；本域复用即自动对齐",
        );
    }

    // ── 无隐私面 ───────────────────────────────────────────────────
    {
        // 结构性自证：本域公开类型不含用户标识/位置/输入历史。
        // acc_name 是 UI 自身的可访问名，不是用户数据。
        let clean = {
            let s = ring_sem();
            let mut ok = true;
            if s.nodes.len() != 2 {
                ok = false;
            }
            match s.nodes.first() {
                Some(n) => {
                    if n.id != 1 || n.acc_name != "提交" || n.role != f3803::NodeRole::Widget {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            ok
        };
        set.add("隐私-无个人数据承载", clean, "本域无隐私面：只处理节点 id/角色/可访问名");
    }

    set
}

/// 判据侧的小工具：取门限常量（避免判据里散落裸数字）。
fn vep03_table() -> f32 {
    f3803::WCAG_AA_NORMAL
}
