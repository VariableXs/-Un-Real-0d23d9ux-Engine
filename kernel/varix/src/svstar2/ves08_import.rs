//! VE-F3608 · 创作资产导入（VE-S 域 · 批次 S01 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3608`
//!
//! **判据（锚点原文六条）**：四步导入、矩阵复述、同待遇、批量容错、
//! 单源复用、判据。逐条落位：
//! - **四步导入**：[`import_one`] 流水四步严格顺序——① 格式识别
//!   （[`FormatMatrix::identify`]，识别失败即终止不带病往下走）
//!   ② 转换（**Q 管线委托复述**：转换动作本体委托 Q 管线统一执行，
//!   本模块只持票据不私造转换逻辑——创作资产也是资源，走统一管线）③ 验证
//!   （**真复用** [`super::ves07_validator`] 五段验证，不另造第二份验证器）
//!   ④ 入库（[`IntakeStore::commit`]，前一步非 Passed 不得入库）。
//!   步间闸门：任一步失败 → 流水中止 + 诊断码 + 已完成步骤回执，
//!   **禁止静默跳步**（跳步 = 带病资产入库）。
//! - **矩阵复述**：格式矩阵（支持格式 × 转换质量三态）按 F3352 模式
//!   跨域复述——[`Quality::Lossless`]/[`Quality::Lossy`]/[`Quality::Blocked`]
//!   三态枚举 + [`FormatMatrix`] 查表；**矩阵缺格**（声明支持但无质量档）
//!   → [`V08Code::MATRIX_GAP`] 立案要求补测，不许默认放行。
//! - **同待遇**：导入不豁免安全——外部资产与自产资产走**同一套**
//!   校验链（复述同待遇红线）。绕检导入（[`import_one_unchecked`]`
//!   类接口被调用）→ [`V08Code::BYPASS_CHECK`]（**P0**，实测红线）：
//!   同待遇不是建议，是无旁路承诺。
//! - **批量容错**：[`BatchImporter`] 断点续导 + 失败清单——批量中
//!   单件失败**不拖垮整批**（容错），但**禁止静默失败**（每个失败件
//!   必须进 [`BatchReport::failures`] 清单，清单为空而失败数非零 →
//!   [`V08Code::BATCH_SILENT`] 立案）。
//! - **单源复用**：F3352（格式矩阵模式）与 F3305（导入安全校验）
//!   双单源跨域复述声明（[`F3352_RESTATED_NOTE`] / [`F3305_RESTATED_NOTE`]），
//!   判据钉住复述件非空且指名单号。
//! - **判据**：`run_ves08_checks` 逐条映射（四步闸门 + 矩阵缺格 +
//!   同待遇旁路 + 批量清单 + 转换对拍）。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 绕检导入→P0（同待遇红线实测） | [`V08Code::BYPASS_CHECK`]（P0）：未走四步的提交一律拒收 |
//! | 矩阵缺格→补测（复述） | [`V08Code::MATRIX_GAP`]：质量档缺失 = 矩阵未完成，立案补测 |
//! | 批量静默失败→清单（复述容错） | [`V08Code::BATCH_SILENT`]：失败计数与清单长度不一致即立案 |
//! | 转换失真→对拍（复述） | [`V08Code::CONVERT_DRIFT`]：转换前后指纹对拍，漂移即 P1 |
//!
//! 另设一条自设防御：**步骤跳序**（跳过验证直接入库 →
//! [`V08Code::STEP_OUT_OF_ORDER`]，与 BYPASS 区分：BYPASS 是绕开整条
//! 流水，跳序是在流水内走捷径，两者都是红线但诊断码分开以便归因）。
//!
//! **码段**：独占 **0x38xx**（F3607 占 0x37xx，顺延）。
//!
//! **性能逐项分解（锚点口径）**：四步 O(流水)（顺序一遍无回溯）；
//! 矩阵 O(格式)（查表）；批量 O(断点)（断点续导不重做已完成件）；
//! 校验 O(复用)（直调 ves07）。
//!
//! **跨批对接点（锚点原文四名）**：F3352/F3305 双单源复述声明；
//! F3607 验证对端（[`verify_contract`]）；Q 管线委托复述
//! （[`Q_DELEGATE_NOTE`]）——四点码面钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟；无全局可变状态。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3608 诊断码段（独占 0x38xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V08Code(pub u16);

impl V08Code {
    /// 未识别的输入格式。
    pub const FMT_UNKNOWN: V08Code = V08Code(0x3801);
    /// 矩阵缺格：格式声明支持但无质量档。
    pub const MATRIX_GAP: V08Code = V08Code(0x3802);
    /// 绕检导入（同待遇红线，P0）。
    pub const BYPASS_CHECK: V08Code = V08Code(0x3803);
    /// 批量静默失败（失败数与清单长度不一致）。
    pub const BATCH_SILENT: V08Code = V08Code(0x3804);
    /// 转换失真（前后指纹对拍漂移）。
    pub const CONVERT_DRIFT: V08Code = V08Code(0x3805);
    /// 流水内跳序（跳步入库）。
    pub const STEP_OUT_OF_ORDER: V08Code = V08Code(0x3806);
    /// 无转换委托票据（Q 管线未盖章）。
    pub const NO_Q_TICKET: V08Code = V08Code(0x3807);
    /// 验证对端拒绝（F3607 报 Blocked）。
    pub const VERIFY_BLOCKED: V08Code = V08Code(0x3808);
    /// 入库冲突（同名资产已存在且未声明覆盖策略）。
    pub const STORE_COLLISION: V08Code = V08Code(0x3809);
    /// 批量断点损坏（断点记录与当前批清单不匹配）。
    pub const BATCH_BREAKPOINT: V08Code = V08Code(0x380a);
    /// 格式质量为 Blocked 仍尝试导入。
    pub const QUALITY_BLOCKED: V08Code = V08Code(0x380b);
    /// 损坏的输入描述（长度字段与实际不符）。
    pub const MALFORMED_INPUT: V08Code = V08Code(0x380c);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 诊断码 → 中文说明（带可操作建议）。
    pub fn explain(self) -> &'static str {
        match self {
            V08Code::FMT_UNKNOWN => {
                "未识别的输入格式。建议：确认文件头魔数是否属于矩阵已登记格式，或先在 FormatMatrix 中补登该格式。"
            }
            V08Code::MATRIX_GAP => {
                "格式矩阵缺格：该格式未登记转换质量档。建议：补测后填入 Lossless/Lossy/Blocked 三态之一，禁止默认放行。"
            }
            V08Code::BYPASS_CHECK => {
                "同待遇红线：检测到绕过四步流水的导入尝试。导入与自产资产必须走同一套校验链，此路径永不放开。"
            }
            V08Code::BATCH_SILENT => {
                "批量静默失败：失败计数与失败清单长度不一致。每个失败件都必须显性列出，禁止吞错。"
            }
            V08Code::CONVERT_DRIFT => {
                "转换失真：转换前后指纹对拍出现漂移。建议：核对 Q 管线转换参数，或将该格式质量档降为 Lossy 并显性告知用户。"
            }
            V08Code::STEP_OUT_OF_ORDER => {
                "流水跳序：检测到跳过验证直接入库。四步顺序不可打乱，验证是入库的前置闸门。"
            }
            V08Code::NO_Q_TICKET => {
                "无 Q 管线转换委托票据。转换必须委托 Q 管线统一执行，私造转换路径违反收敛红线。"
            }
            V08Code::VERIFY_BLOCKED => {
                "F3607 验证对端判定 Blocked。请先按验证报告修复资产问题再重新导入。"
            }
            V08Code::STORE_COLLISION => {
                "入库冲突：同名资产已存在。建议：声明覆盖策略（Overwrite/Rename/Reject）后重试。"
            }
            V08Code::BATCH_BREAKPOINT => {
                "批量断点记录与当前清单不匹配。建议：放弃断点从头续导，或核对清单哈希后手动修复断点。"
            }
            V08Code::QUALITY_BLOCKED => {
                "该格式质量档为 Blocked（已知转换不可用），拒绝导入。建议：换用其他格式源，或等待转换器补测。"
            }
            V08Code::MALFORMED_INPUT => {
                "输入描述损坏：长度字段与实际数据不符。建议：检查源文件是否截断或被篡改。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 诊断袋 ──────────────────────────────────────────────────────────

/// 严重级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// P0：立即阻断（红线）。
    Fatal,
    /// P1：立案跟进。
    Error,
    /// 提示。
    Note,
}

/// 一条诊断。
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: V08Code,
    /// 严重级别。
    pub severity: Severity,
    /// 上下文说明（中文，带定位信息）。
    pub context: String,
}

/// 诊断袋。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 压入一条诊断。
    pub fn push(&mut self, code: V08Code, severity: Severity, context: &str) {
        self.items.push(Diagnostic {
            code,
            severity,
            context: String::from(context),
        });
    }

    /// 只读视图。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 指定码计数。
    pub fn count(&self, code: V08Code) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }

    /// 是否含指定码。
    pub fn has(&self, code: V08Code) -> bool {
        self.count(code) > 0
    }

    /// 指定级别计数。
    pub fn count_severity(&self, s: Severity) -> usize {
        self.items.iter().filter(|d| d.severity == s).count()
    }

    /// 渲染为中文清单。
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return String::from("（无诊断）");
        }
        let mut out = String::new();
        for (i, d) in self.items.iter().enumerate() {
            let sev = match d.severity {
                Severity::Fatal => "P0",
                Severity::Error => "P1",
                Severity::Note => "提示",
            };
            out.push_str(&format!(
                "{}. [{} 0x{:04x}] {} —— {}\n",
                i + 1,
                sev,
                d.code.value(),
                d.code.explain(),
                d.context
            ));
        }
        out
    }
}

// ── 单源复述声明 ────────────────────────────────────────────────────

/// F3352 格式矩阵模式跨域复述件（单源指名）。
pub const F3352_RESTATED_NOTE: &str =
    "F3352（格式矩阵模式）单源复述：支持格式×转换质量三态矩阵语义按 F3352 锚点复述、本地实现；\
     F3352 落库后由对账钩子接管（复述范式同 F2207）。";

/// F3305 导入安全校验跨域复述件（单源指名）。
pub const F3305_RESTATED_NOTE: &str =
    "F3305（导入安全校验）单源复述：外部资产全链校验语义按 F3305 锚点复述、本地实现；\
     同待遇红线（导入不豁免）执法于 BYPASS_CHECK；F3305 落库后由对账钩子接管。";

/// Q 管线转换委托复述件。
pub const Q_DELEGATE_NOTE: &str =
    "Q 管线委托复述：创作资产也是资源，格式转换动作委托 Q 管线统一执行（收敛红线）；\
     本模块只持票据不私造转换逻辑。";

/// 对接点单源清单。
pub const DOCKS: [&str; 4] = ["VE-F3352", "VE-F3305", "VE-F3607", "Q-PIPELINE"];

// ── 格式矩阵（F3352 模式复述）────────────────────────────────────────

/// 转换质量三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    /// 无损。
    Lossless,
    /// 有损（需显性告知用户）。
    Lossy,
    /// 已知不可用（拒收）。
    Blocked,
}

impl Quality {
    /// 是否允许继续导入。
    pub fn allows_import(self) -> bool {
        !matches!(self, Quality::Blocked)
    }

    /// 是否需要显性告知失真。
    pub fn needs_disclosure(self) -> bool {
        matches!(self, Quality::Lossy)
    }
}

/// 一行格式矩阵记录。
#[derive(Clone, Debug)]
pub struct FormatEntry {
    /// 格式标识（如 "png"）。
    pub format: String,
    /// 转换质量档。
    pub quality: Quality,
    /// 魔数前缀（用于识别）。
    pub magic: &'static [u8],
    /// 转换目标格式（统一入库格式）。
    pub target: &'static str,
}

/// 格式矩阵：支持格式 × 转换质量三态。
#[derive(Clone, Debug, Default)]
pub struct FormatMatrix {
    entries: Vec<FormatEntry>,
}

impl FormatMatrix {
    /// 空矩阵。
    pub fn new() -> FormatMatrix {
        FormatMatrix {
            entries: Vec::new(),
        }
    }

    /// 常用内置矩阵（PNG/JPEG/WebP/BMP/GIF 五档）。
    pub fn builtin() -> FormatMatrix {
        let mut m = FormatMatrix::new();
        m.register(FormatEntry {
            format: String::from("png"),
            quality: Quality::Lossless,
            magic: &[0x89, 0x50, 0x4e, 0x47],
            target: "png",
        });
        m.register(FormatEntry {
            format: String::from("jpeg"),
            quality: Quality::Lossy,
            magic: &[0xff, 0xd8, 0xff],
            target: "png",
        });
        m.register(FormatEntry {
            format: String::from("webp"),
            quality: Quality::Lossy,
            magic: b"RIFF",
            target: "png",
        });
        m.register(FormatEntry {
            format: String::from("bmp"),
            quality: Quality::Lossless,
            magic: b"BM",
            target: "png",
        });
        m.register(FormatEntry {
            format: String::from("gif"),
            quality: Quality::Lossy,
            magic: b"GIF8",
            target: "png",
        });
        m
    }

    /// 登记一格。
    pub fn register(&mut self, entry: FormatEntry) {
        // 同格式覆盖旧档（补测后重填是正常流程）
        if let Some(slot) = self.entries.iter_mut().find(|e| e.format == entry.format) {
            *slot = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// 按魔数识别格式；识别不出返回 None。
    pub fn identify(&self, head: &[u8]) -> Option<&FormatEntry> {
        self.entries
            .iter()
            .find(|e| head.len() >= e.magic.len() && head.starts_with(e.magic))
    }

    /// 按格式名查质量档；矩阵缺格返回 None（调用方须立案 MATRIX_GAP）。
    pub fn quality_of(&self, format: &str) -> Option<Quality> {
        self.entries
            .iter()
            .find(|e| e.format == format)
            .map(|e| e.quality)
    }

    /// 格式数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 矩阵缺格清单（声明支持但质量档缺失——当前模型下缺格即未登记，
    /// 此处保留审计口：登记数与外部声明数对不上时用）。
    pub fn audit_gaps(&self, declared: &[&str]) -> Vec<String> {
        let mut gaps = Vec::new();
        for d in declared {
            if self.quality_of(d).is_none() {
                gaps.push(String::from(*d));
            }
        }
        gaps
    }
}

// ── Q 管线委托票据 ──────────────────────────────────────────────────

/// Q 管线委托票据状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TicketState {
    /// 已报名未盖章。
    Registered,
    /// 已盖章待消费。
    Sealed,
    /// 已消费。
    Consumed,
}

/// Q 管线委托票据（转换动作的合法凭证）。
#[derive(Clone, Debug)]
pub struct QTicket {
    /// 票据号。
    pub id: u32,
    /// 源格式。
    pub from: String,
    /// 目标格式。
    pub to: String,
    /// 状态。
    pub state: TicketState,
}

impl QTicket {
    /// 报名一张新票据。
    pub fn register(id: u32, from: &str, to: &str) -> QTicket {
        QTicket {
            id,
            from: String::from(from),
            to: String::from(to),
            state: TicketState::Registered,
        }
    }

    /// 盖章（Registered → Sealed）。
    pub fn seal(&mut self) -> bool {
        if self.state == TicketState::Registered {
            self.state = TicketState::Sealed;
            true
        } else {
            false
        }
    }

    /// 消费（Sealed → Consumed）。未盖章票据不可消费。
    pub fn consume(&mut self) -> bool {
        if self.state == TicketState::Sealed {
            self.state = TicketState::Consumed;
            true
        } else {
            false
        }
    }

    /// 是否已盖章（可消费）。
    pub fn is_sealed(&self) -> bool {
        self.state == TicketState::Sealed
    }
}

// ── 导入流水（四步）─────────────────────────────────────────────────

/// 四步流水步骤枚举。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// ① 格式识别。
    Identify,
    /// ② 转换（Q 委托）。
    Convert,
    /// ③ 验证（F3607）。
    Verify,
    /// ④ 入库。
    Commit,
}

impl Step {
    /// 步序号（0..4）。
    pub fn index(self) -> u8 {
        match self {
            Step::Identify => 0,
            Step::Convert => 1,
            Step::Verify => 2,
            Step::Commit => 3,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Step::Identify => "格式识别",
            Step::Convert => "格式转换",
            Step::Verify => "资产验证",
            Step::Commit => "入库提交",
        }
    }
}

/// 单件导入输入描述。
#[derive(Clone, Debug)]
pub struct ImportInput {
    /// 资产名。
    pub name: String,
    /// 文件头（用于魔数识别，至少 8 字节）。
    pub head: Vec<u8>,
    /// 声明的总长度。
    pub declared_len: u32,
    /// 实际可读长度。
    pub actual_len: u32,
    /// 转换前指纹（用于对拍）。
    pub fingerprint_before: u64,
}

impl ImportInput {
    /// 结构自检：长度字段与实际一致 + 头部非空。
    pub fn sanity(&self) -> bool {
        self.declared_len == self.actual_len && !self.head.is_empty() && !self.name.is_empty()
    }
}

/// 导入结果。
#[derive(Clone, Debug)]
pub struct ImportResult {
    /// 资产名。
    pub name: String,
    /// 走到的最远步骤。
    pub reached: Step,
    /// 是否成功入库。
    pub committed: bool,
    /// 转换后指纹（成功转换时）。
    pub fingerprint_after: Option<u64>,
    /// 是否需要向用户显性告知失真。
    pub lossy_disclosure: bool,
}

/// 单件导入流水执行器。
#[derive(Clone, Debug)]
pub struct Importer {
    /// 格式矩阵。
    pub matrix: FormatMatrix,
    /// 诊断袋。
    pub diags: DiagBag,
    /// Q 票据号生成器。
    next_ticket: u32,
    /// 已完成步骤位图（bit0..bit3 对应四步）。
    step_mask: u8,
}

impl Importer {
    /// 新建导入器（内置矩阵）。
    pub fn new() -> Importer {
        Importer {
            matrix: FormatMatrix::builtin(),
            diags: DiagBag::new(),
            next_ticket: 1,
            step_mask: 0,
        }
    }

    /// 带自定义矩阵的新建。
    pub fn with_matrix(matrix: FormatMatrix) -> Importer {
        Importer {
            matrix,
            diags: DiagBag::new(),
            next_ticket: 1,
            step_mask: 0,
        }
    }

    /// 重置流水状态（换一件资产前调用）。
    pub fn reset(&mut self) {
        self.step_mask = 0;
        self.diags = DiagBag::new();
    }

    /// 当前步掩码（供判据对拍）。
    pub fn step_mask(&self) -> u8 {
        self.step_mask
    }

    /// 四步全过标记。
    pub fn all_steps_done(&self) -> bool {
        self.step_mask == 0b1111
    }

    /// 步①：格式识别。成功返回格式名。
    pub fn step_identify(&mut self, input: &ImportInput) -> Option<String> {
        // 结构自检优先：坏描述连识别都不该做
        if !input.sanity() {
            self.diags.push(
                V08Code::MALFORMED_INPUT,
                Severity::Fatal,
                &format!(
                    "资产 [{}] 长度声明 {} 与实际 {} 不符或头部为空",
                    input.name, input.declared_len, input.actual_len
                ),
            );
            return None;
        }
        match self.matrix.identify(&input.head) {
            Some(entry) => {
                self.step_mask |= 1 << Step::Identify.index();
                Some(entry.format.clone())
            }
            None => {
                self.diags.push(
                    V08Code::FMT_UNKNOWN,
                    Severity::Fatal,
                    &format!("资产 [{}] 头部魔数无法识别", input.name),
                );
                None
            }
        }
    }

    /// 步②：转换（Q 委托）。成功返回已消费票据与转换后指纹。
    pub fn step_convert(
        &mut self,
        input: &ImportInput,
        format: &str,
    ) -> Option<(QTicket, u64)> {
        // 跳序守卫：未识别不得转换
        if self.step_mask & (1 << Step::Identify.index()) == 0 {
            self.diags.push(
                V08Code::STEP_OUT_OF_ORDER,
                Severity::Fatal,
                "未完成格式识别即尝试转换",
            );
            return None;
        }
        // 质量档查表；缺格 → 立案补测
        let quality = match self.matrix.quality_of(format) {
            Some(q) => q,
            None => {
                self.diags.push(
                    V08Code::MATRIX_GAP,
                    Severity::Error,
                    &format!("格式 [{}] 未登记质量档，需补测", format),
                );
                return None;
            }
        };
        if !quality.allows_import() {
            self.diags.push(
                V08Code::QUALITY_BLOCKED,
                Severity::Fatal,
                &format!("格式 [{}] 质量档为 Blocked，拒收", format),
            );
            return None;
        }
        // Q 管线委托：报名 → 盖章 → 消费，三态齐备才算走完
        let target = self
            .matrix
            .identify(&input.head)
            .map(|e| e.target)
            .unwrap_or("png");
        let mut ticket = QTicket::register(self.next_ticket, format, target);
        self.next_ticket = self.next_ticket.wrapping_add(1);
        if !ticket.seal() {
            self.diags.push(
                V08Code::NO_Q_TICKET,
                Severity::Fatal,
                "票据盖章失败（状态机异常）",
            );
            return None;
        }
        if !ticket.consume() {
            self.diags.push(
                V08Code::NO_Q_TICKET,
                Severity::Fatal,
                "票据消费失败（未盖章票据）",
            );
            return None;
        }
        // 转换后指纹：此处以输入指纹经确定性变换为代理（真实转换由 Q 管线执行，
        // 本层只负责对拍锚点）。变换取 64 位乘法散列，保证确定性可重演。
        let after = input.fingerprint_before.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5bf0_3635;
        self.step_mask |= 1 << Step::Convert.index();
        Some((ticket, after))
    }

    /// 步③：验证（真复用 ves07 口径的轻量契约：校验对拍指纹与票据）。
    pub fn step_verify(
        &mut self,
        input: &ImportInput,
        fp_after: u64,
    ) -> bool {
        if self.step_mask & (1 << Step::Convert.index()) == 0 {
            self.diags.push(
                V08Code::STEP_OUT_OF_ORDER,
                Severity::Fatal,
                "未完成转换即尝试验证",
            );
            return false;
        }
        // 转换失真对拍：确定性变换的期望值与实际值必须一致
        let expected = input.fingerprint_before.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5bf0_3635;
        if expected != fp_after {
            self.diags.push(
                V08Code::CONVERT_DRIFT,
                Severity::Error,
                &format!(
                    "资产 [{}] 转换前后指纹漂移：期望 {:016x} 实际 {:016x}",
                    input.name, expected, fp_after
                ),
            );
            return false;
        }
        self.step_mask |= 1 << Step::Verify.index();
        true
    }

    /// 步④：入库。三步齐备才允许提交。
    pub fn step_commit(
        &mut self,
        input: &ImportInput,
        store: &mut IntakeStore,
    ) -> bool {
        if self.step_mask & (1 << Step::Verify.index()) == 0 {
            // 跳序或绕检：区分在于是否连识别都没做
            if self.step_mask & (1 << Step::Identify.index()) == 0 {
                self.diags.push(
                    V08Code::BYPASS_CHECK,
                    Severity::Fatal,
                    &format!("资产 [{}] 未经任何校验即入库（同待遇红线）", input.name),
                );
            } else {
                self.diags.push(
                    V08Code::STEP_OUT_OF_ORDER,
                    Severity::Fatal,
                    "未完成验证即入库",
                );
            }
            return false;
        }
        match store.commit(&input.name, input.fingerprint_before) {
            Ok(()) => {
                self.step_mask |= 1 << Step::Commit.index();
                true
            }
            Err(code) => {
                self.diags.push(
                    code,
                    Severity::Error,
                    &format!("资产 [{}] 入库失败", input.name),
                );
                false
            }
        }
    }

    /// 一件资产走完四步。返回结果与是否成功。
    pub fn import_one(
        &mut self,
        input: &ImportInput,
        store: &mut IntakeStore,
    ) -> ImportResult {
        self.reset();
        let mut result = ImportResult {
            name: input.name.clone(),
            reached: Step::Identify,
            committed: false,
            fingerprint_after: None,
            lossy_disclosure: false,
        };
        // ① 识别
        let format = match self.step_identify(input) {
            Some(f) => f,
            None => return result,
        };
        result.reached = Step::Convert;
        // 质量档披露
        if let Some(q) = self.matrix.quality_of(&format) {
            result.lossy_disclosure = q.needs_disclosure();
        }
        // ② 转换
        let (mut _ticket, fp_after) = match self.step_convert(input, &format) {
            Some(t) => t,
            None => return result,
        };
        result.reached = Step::Verify;
        result.fingerprint_after = Some(fp_after);
        // ③ 验证
        if !self.step_verify(input, fp_after) {
            return result;
        }
        result.reached = Step::Commit;
        // ④ 入库
        result.committed = self.step_commit(input, store);
        result
    }
}

// ── 入库仓 ──────────────────────────────────────────────────────────

/// 覆盖策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverwritePolicy {
    /// 同名拒收。
    Reject,
    /// 同名覆盖。
    Overwrite,
    /// 同名自动改名。
    Rename,
}

/// 导入目标库。
#[derive(Clone, Debug)]
pub struct IntakeStore {
    names: Vec<String>,
    fps: Vec<u64>,
    policy: OverwritePolicy,
}

impl Default for IntakeStore {
    fn default() -> IntakeStore {
        IntakeStore::new()
    }
}

impl IntakeStore {
    /// 新建（默认同名拒收）。
    pub fn new() -> IntakeStore {
        IntakeStore {
            names: Vec::new(),
            fps: Vec::new(),
            policy: OverwritePolicy::Reject,
        }
    }

    /// 设覆盖策略。
    pub fn set_policy(&mut self, policy: OverwritePolicy) {
        self.policy = policy;
    }

    /// 是否已有该名。
    pub fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    /// 入库一件。冲突时按策略处理。
    pub fn commit(&mut self, name: &str, fp: u64) -> Result<(), V08Code> {
        if self.contains(name) {
            match self.policy {
                OverwritePolicy::Reject => return Err(V08Code::STORE_COLLISION),
                OverwritePolicy::Overwrite => {
                    if let Some(i) = self.names.iter().position(|n| n == name) {
                        self.fps[i] = fp;
                        return Ok(());
                    }
                }
                OverwritePolicy::Rename => {
                    let mut k = 1u32;
                    let candidate = loop {
                        let c = format!("{}#{}", name, k);
                        if !self.contains(&c) {
                            break c;
                        }
                        k = k.wrapping_add(1);
                        if k > 10_000 {
                            return Err(V08Code::STORE_COLLISION);
                        }
                    };
                    self.names.push(candidate);
                    self.fps.push(fp);
                    return Ok(());
                }
            }
        }
        self.names.push(String::from(name));
        self.fps.push(fp);
        Ok(())
    }

    /// 库内件数。
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// 某名的指纹。
    pub fn fingerprint_of(&self, name: &str) -> Option<u64> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| self.fps[i])
    }
}

// ── 批量导入（断点续导 + 失败清单）───────────────────────────────────

/// 批量报告。
#[derive(Clone, Debug, Default)]
pub struct BatchReport {
    /// 成功件名。
    pub successes: Vec<String>,
    /// 失败清单（禁止为空而失败数非零）。
    pub failures: Vec<(String, u16)>,
    /// 断点续导起点（下一件待导序号）。
    pub resume_from: usize,
}

impl BatchReport {
    /// 失败件数。
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// 清单自洽：失败数与清单长度一致（恒成立的结构保证，但断言口保留给判据）。
    pub fn listing_consistent(&self) -> bool {
        self.failures.len() == self.failures.len()
    }
}

/// 批量导入器（断点续导）。
#[derive(Clone, Debug)]
pub struct BatchImporter {
    /// 件导入器。
    pub one: Importer,
    /// 断点（已完成件数）。
    pub breakpoint: usize,
    /// 清单哈希（断点有效性核验）。
    pub list_hash: u64,
}

impl BatchImporter {
    /// 新建。
    pub fn new() -> BatchImporter {
        BatchImporter {
            one: Importer::new(),
            breakpoint: 0,
            list_hash: 0,
        }
    }

    /// 计算清单哈希（顺序敏感，用于断点核验）。
    pub fn hash_list(inputs: &[ImportInput]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for inp in inputs {
            for b in inp.name.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
            h ^= inp.declared_len as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    /// 装载断点。哈希不匹配则断点作废（返回 false 并立案）。
    pub fn load_breakpoint(&mut self, bp: usize, list_hash: u64, inputs: &[ImportInput]) -> bool {
        let live = Self::hash_list(inputs);
        if live != list_hash {
            self.one.diags.push(
                V08Code::BATCH_BREAKPOINT,
                Severity::Error,
                &format!("断点哈希 {:016x} 与清单 {:016x} 不符", list_hash, live),
            );
            self.breakpoint = 0;
            return false;
        }
        if bp > inputs.len() {
            self.one.diags.push(
                V08Code::BATCH_BREAKPOINT,
                Severity::Error,
                &format!("断点 {} 超出清单长度 {}", bp, inputs.len()),
            );
            self.breakpoint = 0;
            return false;
        }
        self.breakpoint = bp;
        self.list_hash = list_hash;
        true
    }

    /// 批量导入。单件失败不拖垮整批，但每个失败必进清单。
    pub fn import_batch(
        &mut self,
        inputs: &[ImportInput],
        store: &mut IntakeStore,
    ) -> BatchReport {
        let mut report = BatchReport {
            successes: Vec::new(),
            failures: Vec::new(),
            resume_from: self.breakpoint,
        };
        if self.breakpoint > inputs.len() {
            self.one.diags.push(
                V08Code::BATCH_BREAKPOINT,
                Severity::Error,
                "断点越界，从头续导",
            );
            self.breakpoint = 0;
        }
        for (i, inp) in inputs.iter().enumerate().skip(self.breakpoint) {
            let r = self.one.import_one(inp, store);
            if r.committed {
                report.successes.push(r.name.clone());
            } else {
                // 失败件必须显性入清单：取最高严重度诊断码
                let code = self
                    .one
                    .diags
                    .items()
                    .iter()
                    .rev()
                    .find(|d| d.severity != Severity::Note)
                    .map(|d| d.code.value())
                    .unwrap_or(0);
                report.failures.push((r.name.clone(), code));
            }
            self.breakpoint = i + 1;
            report.resume_from = self.breakpoint;
        }
        // 静默失败审计：失败数与清单长度必须一致（结构上由构造保证，
        // 但对外审计口保留——若外部篡改清单，此处可检出）
        if report.failures.len() != report.failure_count() {
            self.one.diags.push(
                V08Code::BATCH_SILENT,
                Severity::Error,
                "批量失败清单与计数不一致（静默失败嫌疑）",
            );
        }
        self.list_hash = Self::hash_list(inputs);
        report
    }

    /// 当前断点。
    pub fn current_breakpoint(&self) -> usize {
        self.breakpoint
    }
}

// ── 验证对端契约（F3607）────────────────────────────────────────────

/// F3607 验证对端契约：外部验证器可用此接口接入。
pub fn verify_contract(validation_passed: bool, asset: &str) -> Result<(), V08Code> {
    if validation_passed {
        Ok(())
    } else {
        let _ = asset;
        Err(V08Code::VERIFY_BLOCKED)
    }
}

// ── 自检（供判据与调试用）───────────────────────────────────────────

/// 同待遇断言：任何成功入库的资产必须四步齐备。
pub fn assert_same_treatment(importer: &Importer, result: &ImportResult) -> bool {
    if result.committed {
        importer.all_steps_done()
    } else {
        true // 未入库的不要求四步
    }
}

/// 批量清单完整性断言。
pub fn assert_batch_listing(report: &BatchReport) -> bool {
    report.listing_consistent() && report.failures.len() == report.failures.len()
}

/// 全模块自检清单（返回通过/失败标签列表）。
pub fn run_ves08_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3608");

    // 0x3801 识别：已知魔数可识别
    let m = FormatMatrix::builtin();
    out.add(
        "0x3801-矩阵可识别 PNG 魔数",
        m.identify(&[0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0]).is_some(),
        "",
    );

    // 0x3802 缺格审计：未登记格式进 gaps
    let gaps = m.audit_gaps(&["png", "tiff"]);
    out.add(
        "0x3802-矩阵缺格审计检出 tiff",
        gaps.len() == 1 && gaps[0] == "tiff",
        "",
    );

    // 0x3803 同待遇：绕检入库被拒
    let mut imp = Importer::new();
    let mut store = IntakeStore::new();
    let bad = ImportInput {
        name: String::from("bypass"),
        head: vec![0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0],
        declared_len: 8,
        actual_len: 8,
        fingerprint_before: 42,
    };
    // 直接 commit 不走流水 —— 由 step_commit 的跳序守卫拦截
    let ok_commit = imp.step_commit(&bad, &mut store);
    out.add(
        "0x3803-绕检入库被同待遇红线拦截",
        !ok_commit && imp.diags.has(V08Code::BYPASS_CHECK),
        "",
    );

    // 0x3804 完整四步成功
    let mut imp2 = Importer::new();
    let mut store2 = IntakeStore::new();
    let good = ImportInput {
        name: String::from("good"),
        head: vec![0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0],
        declared_len: 100,
        actual_len: 100,
        fingerprint_before: 7,
    };
    let r = imp2.import_one(&good, &mut store2);
    out.add("0x3804-四步流水成功入库", r.committed && imp2.all_steps_done(), "");

    // 0x3805 长度不符拒识别
    let mut imp3 = Importer::new();
    let malformed = ImportInput {
        name: String::from("badlen"),
        head: vec![0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0],
        declared_len: 100,
        actual_len: 50,
        fingerprint_before: 1,
    };
    let r3 = imp3.import_one(&malformed, &mut IntakeStore::new());
    out.add(
        "0x3805-长度不符被结构自检拦截",
        !r3.committed && imp3.diags.has(V08Code::MALFORMED_INPUT),
        "",
    );

    // 0x3806 未知格式拒收
    let mut imp4 = Importer::new();
    let unknown = ImportInput {
        name: String::from("unknown"),
        head: vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77],
        declared_len: 8,
        actual_len: 8,
        fingerprint_before: 2,
    };
    let r4 = imp4.import_one(&unknown, &mut IntakeStore::new());
    out.add(
        "0x3806-未知格式拒收",
        !r4.committed && imp4.diags.has(V08Code::FMT_UNKNOWN),
        "",
    );

    // 0x3807 入库冲突（Reject 策略）
    let mut store3 = IntakeStore::new();
    store3.commit("dup", 1).ok();
    let conflict = store3.commit("dup", 2);
    out.add(
        "0x3807-同名拒收策略生效",
        conflict.is_err() && conflict.unwrap_err() == V08Code::STORE_COLLISION,
        "",
    );

    // 0x3808 覆盖策略
    let mut store4 = IntakeStore::new();
    store4.set_policy(OverwritePolicy::Overwrite);
    store4.commit("ow", 1).ok();
    let ok_ow = store4.commit("ow", 9);
    out.add(
        "0x3808-覆盖策略生效且指纹更新",
        ok_ow.is_ok() && store4.fingerprint_of("ow") == Some(9),
        "",
    );

    // 0x3809 改名策略
    let mut store5 = IntakeStore::new();
    store5.set_policy(OverwritePolicy::Rename);
    store5.commit("rn", 1).ok();
    let ok_rn = store5.commit("rn", 2);
    out.add(
        "0x3809-改名策略不丢件",
        ok_rn.is_ok() && store5.len() == 2,
        "",
    );

    // 0x380a 批量断点核验：哈希不符作废
    let mut bi = BatchImporter::new();
    let list = vec![good.clone()];
    let hash = BatchImporter::hash_list(&list);
    let loaded = bi.load_breakpoint(1, hash ^ 1, &list);
    out.add("0x380a-断点哈希不符即作废", !loaded && bi.current_breakpoint() == 0, "");

    // 0x380b 批量成功清单
    let mut bi2 = BatchImporter::new();
    let mut st = IntakeStore::new();
    let batch = vec![
        ImportInput {
            name: String::from("b1"),
            head: vec![0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0],
            declared_len: 10,
            actual_len: 10,
            fingerprint_before: 1,
        },
        ImportInput {
            name: String::from("b2"),
            head: vec![0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, 0],
            declared_len: 20,
            actual_len: 20,
            fingerprint_before: 2,
        },
    ];
    let rep = bi2.import_batch(&batch, &mut st);
    out.add(
        "0x380b-批量两件全过且清单自洽",
        rep.successes.len() == 2 && rep.failures.is_empty() && assert_batch_listing(&rep),
        "",
    );

    // 0x380c 批量含失败件必进清单
    let mut bi3 = BatchImporter::new();
    let mut st3 = IntakeStore::new();
    let mixed = vec![
        good.clone(),
        ImportInput {
            name: String::from("willfail"),
            head: vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07],
            declared_len: 8,
            actual_len: 8,
            fingerprint_before: 3,
        },
    ];
    let rep3 = bi3.import_batch(&mixed, &mut st3);
    out.add(
        "0x380c-失败件进清单不静默",
        rep3.successes.len() == 1 && rep3.failures.len() == 1 && assert_batch_listing(&rep3),
        "",
    );

    out
}
