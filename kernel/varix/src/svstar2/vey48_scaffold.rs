//! VE-F4803 · 脚手架生成器（VE-Y 域 · 工具链段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4803`
//!
//! **判据（锚点原文）**：三类模板、一分钟运行、可续答、日验证、判据。
//!
//! **职责定位（锚点原文）**：工具链脚手架——项目模板生成器（应用/插件/
//! 工具三类模板）+交互式问答+生成即构建即运行——从零到运行一分钟内
//! （W06 口径延伸到全域工具）；模板签名与日验证。
//!
//! # 一、三类模板是闭集而不是开放注册
//!
//! 应用/插件/工具三类覆盖 Y 域工具链的全部产出形态；模板库**齐全性
//! 是判据**（三缺一即红）而不是运行期可容忍状态——脚手架的承诺是
//! 「任何产出形态都有起点」，一个空档就是承诺破产。每模板带**签名**
//! （FNV-1a 对模板内容独立重算，同 F4010/vel11 纪律）：签名是日验证
//! 的锚——模板内容漂移而签名没更新=被篡改或坏编辑，日验证下架。
//!
//! # 二、问答可续答：答案序列即状态
//!
//! 交互问答的中断恢复不靠魔法：问答会话持有**已答序列**，中断后重开
//! 会话按同序回放即续答（确定性：同模板+同答案序列 ⇒ 同产物指纹）。
//! 生成器只在答案**齐备**时生成——半答案生成半成品比拒绝更糟。
//!
//! # 三、生成失败→清理重试（all-or-nothing）
//!
//! 生成器产出是**原子**的：校验不过不落任何文件（失败产物清单不进
//! 产物集，`cleanup` 计数显性）。「生成到一半留下残骸」是脚手架最招
//! 恨的失败模式——下次生成的起点被污染。重试语义 = 直接重新调用
//! （无隐藏状态可脏）。
//!
//! # 四、一分钟运行是预算不是口号
//!
//! 生成产物带**启动步数预算**（逻辑步：入口文件就绪+依赖闭合+构建
//! 清单齐），超预算的模板在日验证标红下架——「一分钟内运行」必须
//! 可机检，否则就是文案。
//!
//! # 五、日验证：模板失效→下架
//!
//! 每逻辑日对每模板跑两件事：①签名重算与登记一致（内容没漂移）；
//! ②生成冒烟通过（默认答案能产出可用产物）。任一不过→**下架**
//! （从可用清单摘除并计数，不是删库——修好重新上架）。下架的模板
//! 生成请求显性拒绝，不静默兜底到别的模板（用户要的是插件，给他
//! 应用模板是善意的背叛）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SCAFFOLD_PROTOCOL_VERSION: &str = "Y48-scaffold-v1";

/// 模板三类闭集长度。
pub const TEMPLATE_KIND_COUNT: usize = 3;

/// 从零到运行的启动步数预算（W06 口径——一分钟内可机检化）。
pub const RUN_BUDGET_STEPS: u32 = 60;

/// 日验证签名不匹配（下架）。
pub const E_SCAF_SIGNATURE: &str = "E_SCAF_SIGNATURE";

/// 冒烟未过（下架）。
pub const E_SCAF_SMOKE: &str = "E_SCAF_SMOKE";

/// 问答中断续答失败（答案序不齐）。
pub const E_SCAF_SESSION: &str = "E_SCAF_SESSION";

/// 生成失败（清理重试）。
pub const E_SCAF_GEN: &str = "E_SCAF_GEN";

/// 模板已下架（生成请求显性拒绝）。
pub const E_SCAF_RETIRED: &str = "E_SCAF_RETIRED";

/// FNV-1a 64 位（模板签名/产物指纹，独立可重算）。
pub const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 二、模板库（三类闭集 + 签名）
// ---------------------------------------------------------------------------

/// 模板形态（三类不多不少——锚点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateKind {
    /// 应用模板。
    App,
    /// 插件模板。
    Plugin,
    /// 工具模板。
    Tool,
}

impl TemplateKind {
    /// 短码（冻结）。
    pub fn wire(self) -> &'static str {
        match self {
            TemplateKind::App => "app",
            TemplateKind::Plugin => "plugin",
            TemplateKind::Tool => "tool",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            TemplateKind::App => "应用",
            TemplateKind::Plugin => "插件",
            TemplateKind::Tool => "工具",
        }
    }

    /// 三类全集（顺序即编号）。
    pub fn all() -> [TemplateKind; TEMPLATE_KIND_COUNT] {
        [TemplateKind::App, TemplateKind::Plugin, TemplateKind::Tool]
    }

    /// 必答问题表（问答生成器的问题单，逐类冻结）。
    pub fn questions(self) -> &'static [&'static str] {
        match self {
            TemplateKind::App => &["项目名", "入口场景名", "是否启用读屏替述(y/n)"],
            TemplateKind::Plugin => &["插件名", "挂载宿主名"],
            TemplateKind::Tool => &["工具名", "输出格式"],
        }
    }
}

/// 模板文件（相对路径+内容骨架）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateFile {
    /// 相对路径。
    pub path: &'static str,
    /// 内容骨架（含 `{{占位符}}`，由答案填充）。
    pub content: &'static str,
}

/// 脚手架模板。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScaffoldTemplate {
    /// 形态。
    pub kind: TemplateKind,
    /// 模板名。
    pub name: &'static str,
    /// 文件集（生成 O(模板规模)——线性展开）。
    pub files: Vec<TemplateFile>,
    /// 登记签名（FNV-1a 对文件集独立重算；日验证对账锚）。
    pub signature: u64,
}

impl ScaffoldTemplate {
    /// 构造并独立重算签名（登记签名 ≠ 手抄——构造期即算）。
    pub fn new(kind: TemplateKind, name: &'static str, files: Vec<TemplateFile>) -> ScaffoldTemplate {
        let mut bytes: Vec<u8> = Vec::new();
        for f in files.iter() {
            bytes.extend_from_slice(f.path.as_bytes());
            bytes.extend_from_slice(f.content.as_bytes());
        }
        let sig = fnv1a(&bytes);
        ScaffoldTemplate { kind, name, files, signature: sig }
    }

    /// 当前内容签名（日验证重算口径）。
    pub fn recompute_signature(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for f in self.files.iter() {
            bytes.extend_from_slice(f.path.as_bytes());
            bytes.extend_from_slice(f.content.as_bytes());
        }
        fnv1a(&bytes)
    }

    /// 启动步数（入口文件 1 步 + 每依赖文件 1 步——可机检的「一分钟」）。
    pub fn boot_steps(&self) -> u32 {
        1u32.saturating_add(self.files.len() as u32)
    }
}

/// 模板库（三类齐全性 + 日验证下架）。
#[derive(Debug, Default)]
pub struct TemplateLib {
    templates: Vec<ScaffoldTemplate>,
    /// 已下架短码（下架 ≠ 删除：生成请求要能显性区分「没这模板」与「下架了」）。
    pub retired: Vec<&'static str>,
    /// 下架计数。
    pub retire_count: u32,
}

impl TemplateLib {
    pub fn new() -> TemplateLib {
        TemplateLib::default()
    }

    /// 上架。
    pub fn admit(&mut self, t: ScaffoldTemplate) -> Result<(), String> {
        for e in self.templates.iter() {
            if e.kind == t.kind {
                return Err(format!("{} 类已有模板 {}", e.kind.wire(), e.name));
            }
        }
        self.templates.push(t);
        Ok(())
    }

    /// 按形态取可用模板（已下架返回显性 Err）。
    pub fn get(&self, kind: TemplateKind) -> Result<&ScaffoldTemplate, String> {
        for r in self.retired.iter() {
            if *r == kind.wire() {
                return Err(E_SCAF_RETIRED.to_string());
            }
        }
        self.templates
            .iter()
            .find(|t| t.kind == kind)
            .ok_or_else(|| format!("{} 类模板缺失", kind.wire()))
    }

    /// 日验证：签名重算一致 + 冒烟通过 → 保留；否则下架。
    /// 返回 `Ok(())`=验证通过，`Err(码)`=已下架及原因。
    pub fn daily_verify(&mut self, kind: TemplateKind, smoke_ok: bool) -> Result<(), String> {
        let sig_ok = match self.templates.iter().find(|t| t.kind == kind) {
            Some(t) => t.recompute_signature() == t.signature,
            None => return Err(format!("{} 类模板缺失", kind.wire())),
        };
        if !sig_ok {
            self.retired.push(kind.wire());
            self.retire_count = self.retire_count.saturating_add(1);
            return Err(E_SCAF_SIGNATURE.to_string());
        }
        if !smoke_ok {
            self.retired.push(kind.wire());
            self.retire_count = self.retire_count.saturating_add(1);
            return Err(E_SCAF_SMOKE.to_string());
        }
        Ok(())
    }

    /// 在册可用模板数（不含下架）。
    pub fn available(&self) -> usize {
        self.templates
            .iter()
            .filter(|t| !self.retired.iter().any(|r| *r == t.kind.wire()))
            .count()
    }

    /// 全部模板数。
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、问答生成器（可续答）
// ---------------------------------------------------------------------------

/// 问答会话（答案序列即状态——中断后按同序回放续答）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QaSession {
    /// 模板形态。
    pub kind: TemplateKind,
    /// 已答序列（顺序敏感——第 i 个答案回答第 i 问）。
    pub answers: Vec<String>,
}

impl QaSession {
    /// 新会话。
    pub fn new(kind: TemplateKind) -> QaSession {
        QaSession { kind, answers: Vec::new() }
    }

    /// 答一问。
    pub fn answer(&mut self, text: &str) {
        self.answers.push(text.to_string());
    }

    /// 已答数。
    pub fn answered(&self) -> usize {
        self.answers.len()
    }

    /// 必答总数。
    pub fn total_questions(&self) -> usize {
        self.kind.questions().len()
    }

    /// 是否齐备（齐备才允许生成——半答案不产半成品）。
    pub fn complete(&self) -> bool {
        self.answered() >= self.total_questions()
    }

    /// 续答：把中断前已答的序列回放进新会话（可续答的核心——
    /// 中断后调用方拿旧 answers 重建，接着答剩余问题）。
    pub fn resume(kind: TemplateKind, prior_answers: &[String]) -> Result<QaSession, String> {
        let total = kind.questions().len();
        if prior_answers.len() > total {
            return Err(E_SCAF_SESSION.to_string());
        }
        let mut s = QaSession::new(kind);
        for a in prior_answers.iter() {
            s.answer(a);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// 四、生成器（原子产出 + 一分钟预算）
// ---------------------------------------------------------------------------

/// 生成产物文件（占位符已填充——owned，区别于模板骨架 &'static str）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedFile {
    /// 相对路径。
    pub path: String,
    /// 填充后内容。
    pub content: String,
}

/// 生成产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedProject {
    /// 模板形态。
    pub kind: TemplateKind,
    /// 产物文件集（占位符已填充）。
    pub files: Vec<GeneratedFile>,
    /// 产物指纹（内容 FNV-1a——同模板同答案同指纹，确定性可复现）。
    pub fingerprint: u64,
    /// 启动步数（≤ [`RUN_BUDGET_STEPS`] 才算「一分钟内可运行」）。
    pub boot_steps: u32,
}

impl GeneratedProject {
    /// 可运行判定：入口文件在产物中 + 步数预算内。
    pub fn runnable(&self) -> bool {
        let has_entry = self.files.iter().any(|f| f.path.contains("main") || f.path.contains("entry"));
        has_entry && self.boot_steps <= RUN_BUDGET_STEPS
    }
}

/// 生成统计（清理显性计数）。
#[derive(Debug, Default, Clone)]
pub struct GenStats {
    /// 成功生成次数。
    pub ok: u32,
    /// 失败后清理次数（all-or-nothing：失败不留残骸）。
    pub cleaned: u32,
    /// 拒绝次数（答案不齐/模板下架——显性原因）。
    pub rejected: u32,
}

/// 生成：占位符填充 + 指纹 + 预算判定。
///
/// 占位符形如 `{{q0}}`（第 0 问的答案）。答案不足的占位符 = 生成失败
/// （返回 Err，调用方清理重试——不产半成品）。
pub fn generate(lib: &TemplateLib, session: &QaSession, stats: &mut GenStats) -> Result<GeneratedProject, String> {
    if !session.complete() {
        stats.rejected += 1;
        return Err(E_SCAF_SESSION.to_string());
    }
    let tpl = match lib.get(session.kind) {
        Ok(t) => t,
        Err(e) => {
            stats.rejected += 1;
            return Err(e);
        }
    };
    if tpl.boot_steps() > RUN_BUDGET_STEPS {
        stats.rejected += 1;
        return Err(E_SCAF_GEN.to_string());
    }
    let mut files: Vec<GeneratedFile> = Vec::new();
    for f in tpl.files.iter() {
        let mut content = f.content.to_string();
        for (qi, a) in session.answers.iter().enumerate() {
            let ph = format!("{{{{q{}}}}}", qi);
            content = content.replace(&ph, a);
        }
        if content.contains("{{q") {
            // 有占位符没被填（答案序列与模板不匹配）——失败清理。
            stats.cleaned = stats.cleaned.saturating_add(1);
            return Err(E_SCAF_GEN.to_string());
        }
        files.push(GeneratedFile { path: f.path.to_string(), content });
    }
    let mut bytes: Vec<u8> = Vec::new();
    for f in files.iter() {
        bytes.extend_from_slice(f.path.as_bytes());
        bytes.extend_from_slice(f.content.as_bytes());
    }
    let fp = fnv1a(&bytes);
    stats.ok = stats.ok.saturating_add(1);
    Ok(GeneratedProject {
        kind: session.kind,
        files,
        fingerprint: fp,
        boot_steps: tpl.boot_steps(),
    })
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// F4803 域自检（判据逐条映射；三模板+续答+生成+日验证全链真跑）。
pub fn run_vey48_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4803");

    // --- 三类模板 ---
    // Y48-模板-01：三类闭集短码互异。
    let kinds = TemplateKind::all();
    s.add(
        "Y48-模板-01",
        kinds.len() == TEMPLATE_KIND_COUNT
            && kinds[0].wire() != kinds[1].wire()
            && kinds[1].wire() != kinds[2].wire(),
        "三类短码互异（闭集）",
    );
    // Y48-模板-02：标准库三模板齐全（缺类即红）。
    let mut lib = std_lib();
    s.add(
        "Y48-模板-02",
        kinds.iter().all(|k| lib.get(*k).is_ok()) && lib.available() == 3,
        "三模板齐全（应用/插件/工具）",
    );
    // Y48-模板-03：签名构造期即算且重算一致。
    let t_app_sig = match lib.get(TemplateKind::App) {
        Ok(t) => {
            let sig = t.signature;
            (sig != 0, t.recompute_signature() == sig)
        }
        Err(_) => (false, false),
    };
    s.add(
        "Y48-模板-03",
        t_app_sig.0 && t_app_sig.1,
        "签名非零且重算一致",
    );
    // Y48-模板-04：同类重复上架拒绝。
    let dup = ScaffoldTemplate::new(TemplateKind::App, "dup", Vec::new());
    s.add("Y48-模板-04", lib.admit(dup).is_err(), "同类模板唯一（重复拒）");
    // Y48-模板-05：问题表非空且齐备判定正确（未答完 ≠ complete）。
    let mut sess = QaSession::new(TemplateKind::Tool);
    s.add(
        "Y48-模板-05",
        TemplateKind::Tool.questions().len() == 2 && !sess.complete(),
        "问题表非空且未答不齐备",
    );

    // --- 可续答 ---
    // Y48-续答-01：答齐后 complete。
    sess.answer("mytool");
    sess.answer("json");
    s.add("Y48-续答-01", sess.complete() && sess.answered() == 2, "答齐判定（2/2）");
    // Y48-续答-02：中断续答回放——resume 后接着答产物与连续作答一致。
    let mut prior = Vec::new();
    prior.push("mytool".to_string());
    let resumed = QaSession::resume(TemplateKind::Tool, &prior);
    let mut resumed = match resumed {
        Ok(r) => r,
        Err(_) => QaSession::new(TemplateKind::Tool),
    };
    resumed.answer("json");
    // 续答回放后状态与连续作答等价（已答数齐备、答案逐位一致）。
    s.add(
        "Y48-续答-02",
        resumed.complete()
            && resumed.answered() == sess.answered()
            && resumed.answers == sess.answers,
        "中断续答回放与连续作答等价",
    );
    // Y48-续答-03：超量答案拒（答案序与问题表不匹配是错误不是截断）。
    let mut over = Vec::new();
    over.push("a".to_string());
    over.push("b".to_string());
    over.push("c".to_string());
    s.add("Y48-续答-03", QaSession::resume(TemplateKind::Tool, &over).is_err(), "超量答案拒绝");

    // --- 生成 ---
    let mut stats = GenStats::default();
    let p1 = generate(&lib, &sess, &mut stats);
    // Y48-生成-01：齐备答案生成成功。
    s.add("Y48-生成-01", p1.is_ok() && stats.ok == 1, "齐备答案生成成功");
    // Y48-生成-02：占位符全部填充（产物无 {{q 残留）。
    let proj = match p1 {
        Ok(p) => p,
        Err(_) => GeneratedProject {
            kind: TemplateKind::Tool,
            files: Vec::new(),
            fingerprint: 0,
            boot_steps: 0,
        },
    };
    let no_residual = proj.files.iter().all(|f| !f.content.contains("{{q"));
    s.add("Y48-生成-02", no_residual && !proj.files.is_empty(), "占位符全填充（零残留）");
    let sess2 = QaSession::resume(TemplateKind::Tool, &["mytool".to_string(), "json".to_string()])
        .unwrap_or_else(|_| QaSession::new(TemplateKind::Tool));
    let p2 = generate(&lib, &sess2, &mut stats);
    s.add(
        "Y48-生成-03",
        matches!(p2, Ok(ref x) if x.fingerprint == proj.fingerprint),
        "同答案序列同指纹（可复现）",
    );
    // Y48-生成-04：不同答案不同指纹（可分性下界）。
    let sess3 = QaSession::resume(TemplateKind::Tool, &["other".to_string(), "json".to_string()])
        .unwrap_or_else(|_| QaSession::new(TemplateKind::Tool));
    let p3 = generate(&lib, &sess3, &mut stats);
    s.add(
        "Y48-生成-04",
        matches!(p3, Ok(ref x) if x.fingerprint != proj.fingerprint),
        "不同答案指纹可分",
    );
    // Y48-生成-05：半答案显性拒（不产半成品）。
    let mut half = QaSession::new(TemplateKind::Plugin);
    half.answer("only-one");
    let before_rej = stats.rejected;
    s.add(
        "Y48-生成-05",
        generate(&lib, &half, &mut stats).is_err() && stats.rejected == before_rej + 1,
        "答案不齐显性拒（计数）",
    );
    // Y48-生成-06：产物可运行判定（入口文件+一分钟预算内）。
    s.add(
        "Y48-生成-06",
        proj.runnable() && proj.boot_steps <= RUN_BUDGET_STEPS && RUN_BUDGET_STEPS == 60,
        "可运行判定（入口在+预算内 60 步）",
    );

    // --- 日验证 ---
    // Y48-验证-01：签名一致+冒烟过 → 保留（available 不减）。
    let mut lib2 = std_lib();
    s.add(
        "Y48-验证-01",
        lib2.daily_verify(TemplateKind::App, true).is_ok() && lib2.available() == 3,
        "日验证通过保留",
    );
    // Y48-验证-02：冒烟不过 → 下架且生成显性拒（不是静默兜底）。
    let r2 = lib2.daily_verify(TemplateKind::Plugin, false);
    let get_after = lib2.get(TemplateKind::Plugin);
    s.add(
        "Y48-验证-02",
        r2.is_err() && r2.unwrap_err() == E_SCAF_SMOKE && get_after.is_err() && lib2.available() == 2,
        "冒烟不过下架（生成面显性拒）",
    );
    // Y48-验证-03：签名漂移 → 下架（篡改检出）。
    let mut lib3 = std_lib();
    if let Some(t) = lib3.templates.iter_mut().find(|t| t.kind == TemplateKind::Tool) {
        t.files.push(TemplateFile { path: "evil.txt", content: "tampered" });
    }
    let r3 = lib3.daily_verify(TemplateKind::Tool, true);
    s.add(
        "Y48-验证-03",
        r3.is_err() && r3.unwrap_err() == E_SCAF_SIGNATURE,
        "签名漂移检出下架",
    );
    // Y48-验证-04：下架计数显性。
    s.add(
        "Y48-验证-04",
        lib3.retire_count == 1 && lib2.retire_count == 1,
        "下架计数显性（两库各一次）",
    );
    // Y48-验证-05：错误码非空互异。
    s.add(
        "Y48-验证-05",
        E_SCAF_SIGNATURE != E_SCAF_SMOKE
            && E_SCAF_SESSION != E_SCAF_GEN
            && E_SCAF_RETIRED != E_SCAF_SMOKE
            && !E_SCAF_SIGNATURE.is_empty(),
        "错误码非空互异",
    );
    // Y48-验证-06：版本指纹（FNV-1a 运行期重算对账）。
    let fp = {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in SCAFFOLD_PROTOCOL_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    s.add("Y48-验证-06", fp == fnv1a(b"Y48-scaffold-v1") && fp != 0, "版本指纹一致");
    // Y48-验证-07：判据条数对账。
    s.add("Y48-验证-07", s.len() == 20, "判据条数对账（本条为第 21 条）");

    s
}

/// 标准三模板库（应用/插件/工具各一；判据语料单源）。
fn std_lib() -> TemplateLib {
    let mut lib = TemplateLib::new();
    let _ = lib.admit(ScaffoldTemplate::new(
        TemplateKind::App,
        "app-starter",
        vec![
            TemplateFile {
                path: "main.entry",
                content: "app {{q0}} scene {{q1}} a11y {{q2}}",
            },
            TemplateFile {
                path: "scene.cfg",
                content: "name={{q0}}",
            },
        ],
    ));
    let _ = lib.admit(ScaffoldTemplate::new(
        TemplateKind::Plugin,
        "plugin-starter",
        vec![TemplateFile {
            path: "plugin.entry",
            content: "plugin {{q0}} host {{q1}}",
        }],
    ));
    let _ = lib.admit(ScaffoldTemplate::new(
        TemplateKind::Tool,
        "tool-starter",
        vec![TemplateFile {
            path: "tool.entry",
            content: "tool {{q0}} fmt {{q1}}",
        }],
    ));
    lib
}

use alloc::vec;
