//! VE-F4009 · 输入法协同（VE-U 域 · T01 输入法组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4009`
//!
//! **判据（锚点原文）**：组合期单源、候选跟随、方向联动、切换断言、IME 协同。
//!
//! **职责定位（锚点原文）**：输入法协同——IME × i18n：多语言输入法协同
//! （CJK IME / 阿拉伯 IME / 复合语音输入）；IME 组合期渲染协同（复用 F3024
//! 组合期红线，**复述单源**）；候选窗渲染（位置跟随，**复述候选跟随**）；
//! 方向 × 输入（RTL 输入光标方向，F4028 联动前向）；输入切换（语言切换 →
//! 输入法切换联动，切换联动断言）。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：组合期快捷键 → 禁令（复用红线）；
//! 候选错位 → 跟随修正（复用）；方向错 → F4028 协同；切换失联 → 联动断案。
//!
//! **性能逐项分解（锚点原文·家族格式）**：组合 O(1) 通道；候选 O(跟随)；
//! 切换 O(联动)；协同 O(1)。
//!
//! **跨批对接点**：F3024 / F3848 复用单源声明；N03 IME 对端；F4028 前向。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 一、组合期（preedit）是**别人的字符串**，不是我们的
//!
//! 输入法在未上屏前会持有一段「组合文本」（拼音串、日文浊音序列、阿拉伯语
//! 中间态）。这段文字的**所有权不在我们**——它随时可能被输入法整段改写、
//! 插入、删除。若引擎在组合期插入自己的零宽字符、光标标记或自动纠错，
//! 会与输入法的编辑意图打架，表现为「拼音串被打断」「上屏后多了个字符」。
//!
//! 故本单把「组合期不改内容」做成**不变量**（[`CompositionGate::accept_edit`]）：
//! 组合期只允许 IME 自己的编辑，任何引擎侧的插入都留痕并可断言。
//!
//! # 二、组合期快捷键是**禁令**，不是偏好
//!
//! 组合期用户按空格/回车是在**选词**，不是在给控件发命令。若引擎在组合期
//! 响应 Enter（提交表单）或 Space（触发按钮），用户想上屏却触发了业务动作——
//! 这是真实且高频的事故。故组合期对**命令键**一律禁令
//! （[`COMPOSITION_BANNED_KEYS`]），且禁令可断言、可反查。
//!
//! # 三、候选窗跟随：错位比不显示更糟
//!
//! 候选窗不跟随光标，用户就不知道候选对应哪一段文字。跟随不能只跟 x——
//! 视口滚动、RTL、窗口边界都会让只跟 x 的候选飞出屏幕。故本单的跟随解算
//! **三个轴**（x / y / 视口夹取），且夹取是**保证**而非建议：候选窗必须
//! 完整落在视口内，夹取后仍越界的要报出来（说明视口本身放不下它）。
//!
//! **caret 必须先收敛进视口再解算**：滚动视口下组合段可能已被滚出可视区，
//! 此时 `caret` 的y 为负或超出视口高。若直接拿它算起点，负 y 会让「翻到
//! 光标上方」的分支算出更负的值，最终报「夹取后仍越界」——**把正常滚动
//! 误判成整数溢出故障**。故 [`CandidateWindow::place`] 第一步就把 `caret`
//! 钳进视口，使结果对「caret 在视口内 / 外」两种情形都恒定合法。
//!
//! # 三之二、哪些语言真的有组合期
//!
//! `LANG_IME_KINDS` 是「语言切换 → 输入法切换」的唯一决策依据（单源）。
//! `ko`（谚文）**归` Cjk` 不归 `Direct`**：谚文有组合期（读音字 → 组合字形）。
//! 把它当直通输入，等于宣称「该语言无组合期」，引擎便会在组合期插入内容——
//! 正是本单第一条不变量要防的事故，且症状是「谚文被引擎打断」，极难归因。
//!
//! # 四、方向 × 输入：RTL 下的光标不是「往右移」
//!
//! LTR 下插入点在尾部之后，RTL 下在头部之前。锚点写「RTL 输入光标方向
//! （F4028 联动前向）」——本单只做**方向契约的持有与前向声明**，光标移动的
//! 实际算法归 F4028。两处各写一套方向算法必然分叉，故本单只校验
//! 「方向 × 插入点」的一致性，不复制算法。
//!
//! # 五、语言切换 → 输入法切换：联动必须**断言**，不能想当然
//!
//! 用户把语言从「日」切到「中」，若输入法还停在日语罗马音模式，则按键被
//! 静默吞掉——**界面无任何反应**，是最难排查的一类故障。故本单要求切换
//! 联动留下**联动记录**（[`SwitchLedger`]），且每条记录能回答三个问题：
//! 切到了什么语言、输入法跟着切没切、有没有失联。
//!
//! 失联（语言切了但输入法没切、或反之）即 [`Diag::SwitchDesync`] 立案——
//! 静默是这类故障唯一的表现形式，所以必须显性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veo01_arch::StyleError;

/// 本项版本。
pub const IME_VERSION: &str = "U01-ime-v1";

/// 组合串长度上限（防畸形 IME 无限增长；超出即立案而非静默截断）。
pub const MAX_COMPOSITION_LEN: usize = 256;

/// 候选窗最大条目数。
pub const MAX_CANDIDATES: usize = 32;

/// 联动记录条目上限。
pub const MAX_SWITCH_RECORDS: usize = 64;

/// 视口最小尺寸（夹取计算的合法下界）。
pub const MIN_VIEWPORT_DIM: u32 = 1;

/// 错误码：组合期试图由引擎侧插入内容。
pub const E_COMPOSITION_ENGINE_EDIT: &str = "E_COMPOSITION_ENGINE_EDIT";

/// 错误码：组合期快捷键未被拦截。
pub const E_COMPOSITION_BANNED_KEY: &str = "E_COMPOSITION_BANNED_KEY";

/// 错误码：视口本身装不下候选窗（**前置守卫**，在解算前判定）。
///
/// 与 [`E_CANDIDATE_CLAMP_STILL_OUT`] 必须分开：前者是「视口尺寸客观小于
/// 候选窗」的**输入问题**，给的是「降级为页脚停靠」；后者是「前置守卫被绕过
/// 后夹取仍越界」的**内部不变量破损**，给的是「复查整数溢出」。两者外部表现
/// 都是「候选窗没放下」，共用一个码就等于告诉排查者「视口太小」——而真实
/// 原因可能是守卫根本没跑。
pub const E_CANDIDATE_NO_FIT: &str = "E_CANDIDATE_NO_FIT";

/// 错误码：夹取后候选窗仍越出视口（**兜底分支**，前置守卫失效后的自检）。
///
/// 这条码存在的唯一意义是让「前置守卫被删掉/被绕过」这件事**可被外部观测**
/// （见 `O09-候选-13`）。没有它，两条错路共用 `E_CANDIDATE_NO_FIT`，删掉守卫
/// 后判据全绿——缺陷从缝里钻过去。
pub const E_CANDIDATE_CLAMP_STILL_OUT: &str = "E_CANDIDATE_CLAMP_STILL_OUT";

/// 错误码：光标方向与书写方向不符。
pub const E_DIRECTION_MISMATCH: &str = "E_DIRECTION_MISMATCH";

/// 错误码：切换联动失联。
pub const E_SWITCH_DESYNC: &str = "E_SWITCH_DESYNC";

/// 错误码：组合串超长。
pub const E_COMPOSITION_TOO_LONG: &str = "E_COMPOSITION_TOO_LONG";

/// 错误码：候选条目过多。
pub const E_CANDIDATE_CAP: &str = "E_CANDIDATE_CAP";

/// 书写方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// 从左到右。
    Ltr,
    /// 从右到左。
    Rtl,
}

impl Direction {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Direction::Ltr => "从左到右",
            Direction::Rtl => "从右到左",
        }
    }

    /// 枚举往返守卫：未知码拒绝（不留默认分支兜底）。
    pub fn from_code(code: &str) -> Option<Direction> {
        match code {
            "LTR" => Some(Direction::Ltr),
            "RTL" => Some(Direction::Rtl),
            _ => None,
        }
    }
}

/// 输入法类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeKind {
    /// CJK 拼音/假名/谚文组合 IME（组合串是拉丁转写或谚文字母 jamo）。
    Cjk,
    /// 阿拉伯 IME（连写形+组合态）。
    Arabic,
    /// 复合语音输入（语音转写，组合期长且会整段改写）。
    VoiceCompose,
    /// 直通输入（无组合期，如英文键盘直打）。
    Direct,
}

impl ImeKind {
    /// 该输入法是否有组合期。
    pub fn has_composition(self) -> bool {
        match self {
            ImeKind::Cjk | ImeKind::Arabic | ImeKind::VoiceCompose => true,
            ImeKind::Direct => false,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ImeKind::Cjk => "中日韩拼音/假名",
            ImeKind::Arabic => "阿拉伯连写",
            ImeKind::VoiceCompose => "复合语音输入",
            ImeKind::Direct => "直通输入",
        }
    }
}

/// 组合期诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 引擎侧在组合期插入了内容。
    EngineEditDuringComposition,
    /// 组合期命令键未被拦截。
    BannedKeyDuringComposition,
    /// 候选窗夹取后仍越出视口。
    CandidateNoFit,
    /// 语言与输入法切换失联。
    SwitchDesync,
    /// 组合串超长。
    CompositionTooLong,
    /// 候选条目超上限。
    CandidateCap,
}

impl Diag {
    /// 是否阻断（阻断项必须阻断，不得只告警）。
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            Diag::EngineEditDuringComposition | Diag::BannedKeyDuringComposition
        )
    }

    /// 诊断码字符串。
    pub fn code(self) -> &'static str {
        match self {
            Diag::EngineEditDuringComposition => E_COMPOSITION_ENGINE_EDIT,
            Diag::BannedKeyDuringComposition => E_COMPOSITION_BANNED_KEY,
            Diag::CandidateNoFit => E_CANDIDATE_NO_FIT,
            Diag::SwitchDesync => E_SWITCH_DESYNC,
            Diag::CompositionTooLong => E_COMPOSITION_TOO_LONG,
            Diag::CandidateCap => E_CANDIDATE_CAP,
        }
    }

    /// 人话说明（要说清后果，不能只说「失败」）。
    pub fn explain(self) -> &'static str {
        match self {
            Diag::EngineEditDuringComposition => {
                "引擎侧在组合期插入内容：会与输入法的整段改写打架，拼音串被打断且上屏多字符"
            }
            Diag::BannedKeyDuringComposition => {
                "组合期命令键未被拦截：用户想选词却触发了业务动作（提交表单/触发按钮）"
            }
            Diag::CandidateNoFit => "候选窗夹取后仍越出视口：视口尺寸放不下候选窗，需降级为页脚停靠",
            Diag::SwitchDesync => "语言与输入法切换失联：按键被静默吞掉，界面无任何反应",
            Diag::CompositionTooLong => "组合串超长：畸形 IME 或语音误触发，继续增长会拖垮每帧排版",
            Diag::CandidateCap => "候选条目超上限：超限部分丢弃会让用户看不到想要的词",
        }
    }
}

/// 一条诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// 诊断码。
    pub code: Diag,
    /// 关联键（IME 类别 / 语言 / 键名）。
    pub subject: String,
    /// 人话详情。
    pub detail: String,
}

/// 诊断袋。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagBag {
    notes: Vec<Note>,
}

impl DiagBag {
    /// 空袋。
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new() }
    }

    /// 记一条。
    pub fn push(&mut self, code: Diag, subject: &str, detail: &str) {
        self.notes.push(Note {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    /// 是否含阻断项。
    pub fn has_blocking(&self) -> bool {
        self.notes.iter().any(|n| n.code.is_blocking())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// 全部诊断。
    pub fn all(&self) -> &[Note] {
        &self.notes
    }
}

/// 组合期文本（**所有权在 IME**，引擎不得改写）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composition {
    /// IME 类别。
    pub kind: ImeKind,
    /// 组合串（IME 自持）。
    pub text: String,
    /// 组合段在控件内的起始偏移（候选窗跟随的基准点）。
    pub caret_offset: u32,
}

impl Composition {
    /// 新建空组合（**空组合不是「无组合」**——直通输入法也可能有空组合态）。
    pub fn new(kind: ImeKind, caret_offset: u32) -> Self {
        Composition {
            kind,
            text: String::new(),
            caret_offset,
        }
    }

    /// 是否处于组合期。
    pub fn active(&self) -> bool {
        self.kind.has_composition() && !self.text.is_empty()
    }

    /// 组合串长度。
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "组合中（{}）：{} 光标偏移 {}",
            self.kind.zh(),
            self.text,
            self.caret_offset
        )
    }
}

/// 组合期快捷键禁令表。
///
/// 这些键在组合期的语义属于**输入法**（选词/上屏），不属于控件。
/// 不拦的后果不是「多输一个字符」，是触发业务动作。
pub const COMPOSITION_BANNED_KEYS: [&str; 5] = ["Enter", "Space", "Tab", "Escape", "ContextMenu"];

/// 组合期闸门：引擎侧编辑的准入判定。
#[derive(Clone, Debug, Default)]
pub struct CompositionGate {
    /// 引擎侧编辑留痕（每次都记，便于事后归因「多出来的字符哪来的」）。
    pub engine_edits: u32,
}

impl CompositionGate {
    /// 新建闸门。
    pub fn new() -> Self {
        CompositionGate { engine_edits: 0 }
    }

    /// 判定一次引擎侧编辑在组合期是否准入（**恒不准入**——组合串是 IME 的）。
    ///
    /// 返回 `Err` 并给出**可执行**的下一步：想改内容只能等上屏后改，
    /// 或由 IME 自己改。这一条把「组合期我们不动手」落成代码而非注释。
    pub fn accept_edit(&mut self, c: &Composition) -> Result<(), StyleError> {
        if !c.active() {
            return Ok(());
        }
        self.engine_edits = self.engine_edits.saturating_add(1);
        Err(StyleError::new(
            E_COMPOSITION_ENGINE_EDIT,
            "组合期引擎侧编辑被拒",
            "组合串所有权在输入法：它随时会整段改写，引擎插入会与之打架",
            "等上屏后再改；若确需引擎侧标记，走组合串外的独立渲染层",
            "IME 协同负责人",
        ))
    }

    /// 组合期按键判定（返回该键是否应被拦截）。
    pub fn key_banned(&self, c: &Composition, key: &str) -> bool {
        c.active() && COMPOSITION_BANNED_KEYS.iter().any(|k| *k == key)
    }

    /// 组合串长度守卫（超长即拒，不静默截断）。
    pub fn check_len(&self, c: &Composition) -> Result<(), StyleError> {
        if c.text.len() > MAX_COMPOSITION_LEN {
            return Err(StyleError::new(
                E_COMPOSITION_TOO_LONG,
                "组合串超长被拒",
                &format!(
                    "组合串 {} 字节超上限 {}；继续增长会拖垮每帧排版",
                    c.text.len(),
                    MAX_COMPOSITION_LEN
                ),
                "取消组合或分段落屏；语音输入的长转写应走异步上屏",
                "IME 协同负责人",
            ));
        }
        Ok(())
    }
}

/// 视口（候选窗夹取的合法域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// 宽。
    pub w: u32,
    /// 高。
    pub h: u32,
}

impl Viewport {
    /// 新建视口（**尺寸非法即拒**——宽或高为 0 时夹取无解）。
    pub fn new(w: u32, h: u32) -> Result<Self, StyleError> {
        if w < MIN_VIEWPORT_DIM || h < MIN_VIEWPORT_DIM {
            return Err(StyleError::new(
                E_CANDIDATE_NO_FIT,
                "视口尺寸非法",
                &format!("视口 {}×{} 有一轴为 0，夹取无解", w, h),
                "取真实视口尺寸；0 尺寸视口不应进入渲染路径",
                "IME 协同负责人",
            ));
        }
        Ok(Viewport { w, h })
    }
}

/// 候选窗几何解算结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateRect {
    /// 左上x。
    pub x: i32,
    /// 左上 y。
    pub y: i32,
    /// 宽。
    pub w: i32,
    /// 高。
    pub h: i32,
}

impl CandidateRect {
    /// 右边界。
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    /// 下边界。
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// 是否完整落在视口内（**判据用的唯一真相**，不是「大致在屏幕里」）。
    pub fn within(&self, v: Viewport) -> bool {
        self.x >= 0 && self.y >= 0 && self.right() <= v.w as i32 && self.bottom() <= v.h as i32
    }
}

/// 候选窗：位置跟随光标 + 视口夹取。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateWindow {
    /// 条目数。
    pub count: u32,
}

impl CandidateWindow {
    /// 新建（**条目上限即拒绝理由**——超限丢弃会让用户看不到想要的词）。
    pub fn new(count: u32) -> Result<Self, StyleError> {
        if count as usize > MAX_CANDIDATES {
            return Err(StyleError::new(
                E_CANDIDATE_CAP,
                "候选窗条目超上限被拒",
                &format!("{} 条超上限 {}", count, MAX_CANDIDATES),
                "收窄候选集或分页；静默截断会让用户看不到想要的词",
                "IME 协同负责人",
            ));
        }
        Ok(CandidateWindow { count })
    }

    /// 跟随解算（复杂度 O(1)：三轴夹取，无循环）。
    ///
    /// `caret` 是光标在视口坐标系里的位置。RTL 下候选窗**向左展开**
    /// （否则会盖住刚输入的组合串），这是方向与输入的联动点。
    ///
    /// 夹取后仍放不下（候选窗比视口还大）时返回 `Err`——不能返回一个
    /// 越界的矩形「装作成功」，那正是「候选错位」投诉的来源。
    ///
    /// **滚动视口下的 caret 可能在视口外**（组合段被滚出可视区）。此时不能
    /// 因为 `caret` 落在视口外就报「无解」——那是正常滚动，不是缺陷。故先
    /// 把 `caret` 收敛进视口（clamp），再解算；这样 `caret` 在视口内外都得到
    /// 一个合法的候选窗位置，且结果恒在视口内。
    pub fn place(
        &self,
        caret: (i32, i32),
        size: (i32, i32),
        view: Viewport,
        dir: Direction,
    ) -> Result<CandidateRect, StyleError> {
        let (cw, ch) = size;
        let (vw, vh) = (view.w as i32, view.h as i32);
        // 视口比候选窗还小 ⇒ 无解，须降级。
        // 本分支与下方夹取后的兜底分支用**不同错误码**：前者是输入问题
        // （视口客观装不下），后者是内部不变量破损。共用一个码会让「守卫
        // 被删掉」这类缺陷伪装成「视口太小」，判据便抓不住。
        if cw > vw || ch > vh {
            return Err(StyleError::new(
                E_CANDIDATE_NO_FIT,
                "候选窗无解：视口装不下",
                &format!(
                    "候选窗 {}×{} 超出视口 {}×{}，任何平移都放不下",
                    cw, ch, vw, vh
                ),
                "降级为页脚停靠候选栏（视口内固定位置），不要越界浮层",
                "IME 协同负责人",
            ));
        }
        // 先把光标收敛进视口（滚动视口下组合段可能已滚出可视区）。
        let cx = clamp_i32(caret.0, 0, vw - 1);
        let cy = clamp_i32(caret.1, 0, vh - 1);
        // 起点：LTR 向右下展开，RTL 向左下展开（不遮组合串）。
        let mut x = match dir {
            Direction::Ltr => cx,
            Direction::Rtl => cx - cw,
        };
        let mut y = cy + 2; // 光标下方 2px，避免压住下划线
        // 三轴夹取（保证，不是建议）。
        if x < 0 {
            x = 0;
        }
        if x + cw > vw {
            x = vw - cw;
        }
        if y + ch > vh {
            y = cy - ch - 2; // 翻到光标上方
            if y < 0 {
                y = 0;
            }
        }
        let r = CandidateRect { x, y, w: cw, h: ch };
        // 夹取后仍越界＝**前置守卫失效或视口尺寸异常**，必须报出而不是放行。
        // 用**专属码**（区别于上面的 E_CANDIDATE_NO_FIT）：这样「守卫被删掉」
        // 这类缺陷能被外部观测到，而不是伪装成「视口太小」。
        if !r.within(view) {
            return Err(StyleError::new(
                E_CANDIDATE_CLAMP_STILL_OUT,
                "候选窗夹取后仍越界（前置守卫未生效）",
                &format!(
                    "夹取后 x{}..{} y{}..{} 视口 {}×{}；视口 {}×{} 已通过前置守卫，\
                     故此处越界说明夹取逻辑本身有整数溢出或守卫被绕过",
                    r.x,
                    r.right(),
                    r.y,
                    r.bottom(),
                    vw,
                    vh,
                    vw,
                    vh
                ),
                "复查前置守卫是否仍在本函数内、以及 x+cw / y+ch 是否溢出 i32",
                "IME 协同负责人",
            ));
        }
        Ok(r)
    }
}

/// 把 `v` 钳制到 `[lo, hi]`（`lo > hi` 时返回 `lo`，不 panic）。
fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 光标方向契约（**只持有与前向声明，不复制 F4028 的算法**）。
///
/// 本单只回答一个问题：「在方向 D 下，一次插入后插入点应落在哪一侧」。
/// 实际移动算法归 F4028——两处各写一套必然分叉。
///
/// **返回值是有符号的位移量，必须在有符号空间里应用**：
/// `caret_step(Direction::Rtl)` 是 `-1`，若调用方写成
/// `caret as u32 + caret_step(..) as u32` 就会溢出成天文数字（`-1 as u32`
/// = 4294967295），表现为 `attempt to add with overflow` 恐慌。
/// 位移的施加方是 F4028 的插入点计算，本单只提供符号契约。
pub fn caret_step(dir: Direction) -> i32 {
    match dir {
        Direction::Ltr => 1,
        Direction::Rtl => -1,
    }
}

/// 光标位置一致性校验（方向 × 插入点）。
///
/// 判据是**方向与位置自洽**：RTL 下插入点不应大于起点，LTR 下不应小于起点。
/// 违反即 [`E_DIRECTION_MISMATCH`]——这类错的现场表现是「RTL 文字光标往右跑」。
pub fn check_caret(start: u32, caret: u32, dir: Direction) -> Result<(), StyleError> {
    let ok = match dir {
        Direction::Ltr => caret >= start,
        Direction::Rtl => caret <= start,
    };
    if ok {
        return Ok(());
    }
    Err(StyleError::new(
        E_DIRECTION_MISMATCH,
        "光标方向与位置不自洽",
        &format!(
            "{}方向下起点 {} 光标 {} 不成立",
            dir.zh(),
            start,
            caret
        ),
        "按 caret_step 的符号重算插入点；RTL 插入点在头部之前",
        "IME 协同负责人",
    ))
}

/// 一次语言切换联动记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchRecord {
    /// 切换到的语言。
    pub lang: String,
    /// 联动后应使用的输入法类别。
    pub expect_kind: ImeKind,
    /// 联动后实际生效的输入法类别。
    pub actual_kind: ImeKind,
}

impl SwitchRecord {
    /// 是否联动成功。
    pub fn in_sync(&self) -> bool {
        self.expect_kind == self.actual_kind
    }
}

/// 切换联动台账（**每条记录都要能回答三问**：切到哪、跟着切没切、有没有失联）。
#[derive(Clone, Debug, Default)]
pub struct SwitchLedger {
    /// 记录条目。
    pub records: Vec<SwitchRecord>,
    /// 失联次数。
    pub desyncs: u32,
}

impl SwitchLedger {
    /// 空台账。
    pub fn new() -> Self {
        SwitchLedger {
            records: Vec::new(),
            desyncs: 0,
        }
    }

    /// 记一次联动。
    pub fn record(&mut self, r: SwitchRecord) -> Result<(), StyleError> {
        if self.records.len() >= MAX_SWITCH_RECORDS {
            return Err(StyleError::new(
                E_SWITCH_DESYNC,
                "切换记录被拒：台账已满",
                "台账满说明切换在高频发生；联动断案需要一个可查的台账",
                "归档旧记录，并复查是否存在语言/输入法来回抖动",
                "IME 协同负责人",
            ));
        }
        if !r.in_sync() {
            self.desyncs = self.desyncs.saturating_add(1);
            return Err(StyleError::new(
                E_SWITCH_DESYNC,
                "语言与输入法切换失联",
                &format!(
                    "语言切到 {} 但输入法是 {}，应为 {}：按键被静默吞掉",
                    r.lang,
                    r.actual_kind.zh(),
                    r.expect_kind.zh()
                ),
                "以语言为准重设输入法；失联期须显式提示用户切换输入法",
                "IME 协同负责人",
            ));
        }
        self.records.push(r);
        Ok(())
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 读屏摘要（无障碍）。
    pub fn screen_summary(&self) -> String {
        format!(
            "输入切换联动：{} 次成功，{} 次失联",
            self.records.len(),
            self.desyncs
        )
    }
}

/// 语言 → 输入法类别的映射表（**语言切换的决策依据，单源**）。
///
/// `ko` 归` Cjk` 而非 `Direct`：谚文有**组合期**（读音字 → 组合字形），把它
/// 当直通输入会让引擎在组合期插入内容——正是本单第一条不变量要防的事故。
pub const LANG_IME_KINDS: [(&str, ImeKind); 8] = [
    ("zh-Hans", ImeKind::Cjk),
    ("zh-Hant", ImeKind::Cjk),
    ("ja", ImeKind::Cjk),
    ("ko", ImeKind::Cjk),
    ("ar", ImeKind::Arabic),
    ("fa", ImeKind::Arabic),
    ("ur", ImeKind::Arabic),
    ("en", ImeKind::Direct),
];

/// 查某语言应使用的输入法类别（**未知语言返回 `None`，不静默兜底**）。
///
/// 静默兜底到 `Direct` 的后果是：切到一门没登记的语言，输入法静默退化为直通，
/// 用户按键无反应——这正是本单要防的失联。
pub fn ime_kind_for(lang: &str) -> Option<ImeKind> {
    LANG_IME_KINDS
        .iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, k)| *k)
}

/// 读屏单行（组合 + 候选 + 联动的整体替述）。
pub fn screen_line(c: &Composition, w: &CandidateWindow, l: &SwitchLedger) -> String {
    format!(
        "{}；候选 {} 条；{}",
        c.screen_line(),
        w.count,
        l.screen_summary()
    )
}

/// 组合期三类失联的**统一立案入口**。
///
/// 分散立案的坏处是：调用方只查了其中一类，另一类静默漏网。故三类合成一次
/// 收集，且**同一袋里可同时出现多条**（引擎编辑 + 命令键未拦可以并存）。
pub fn diagnose_composition(
    bag: &mut DiagBag,
    gate: &CompositionGate,
    c: &Composition,
    key: &str,
) {
    if c.active() {
        if gate.engine_edits > 0 {
            bag.push(
                Diag::EngineEditDuringComposition,
                c.kind.zh(),
                Diag::EngineEditDuringComposition.explain(),
            );
        }
        if gate.key_banned(c, key) {
            bag.push(
                Diag::BannedKeyDuringComposition,
                key,
                Diag::BannedKeyDuringComposition.explain(),
            );
        }
    }
    if c.text.len() > MAX_COMPOSITION_LEN {
        bag.push(
            Diag::CompositionTooLong,
            c.kind.zh(),
            Diag::CompositionTooLong.explain(),
        );
    }
}

/// 候选窗落位失败的立案（调用方在 [`CandidateWindow::place`] 返回 `Err` 时调用）。
pub fn diagnose_candidate(bag: &mut DiagBag, err: &StyleError) {
    bag.push(Diag::CandidateNoFit, err.code, &err.why);
}

/// 切换失联的立案（判定与 [`SwitchLedger::record`] 的拒绝分支同源）。
pub fn diagnose_switch(bag: &mut DiagBag, r: &SwitchRecord) {
    if !r.in_sync() {
        bag.push(Diag::SwitchDesync, &r.lang, Diag::SwitchDesync.explain());
    }
}
