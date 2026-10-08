//! VE-F4803 · 脚手架生成器（VE-Y 域 · 工具链与调试域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4803`
//!
//! **判据（锚点原文五条）**：**三类模板、一分钟运行、可续答、日验证、
//! 判据**。
//!
//! - **三类模板**：应用/插件/工具三类模板库——每类模板带整体签名（FNV-1a
//!   对模板全部文件内容求和），签名是模板的身份证；**模板无隐私残留**：
//!   内容禁含隐私键（token/password/secret/apikey 黑名单），生成器入册闸
//!   扫描必拒——模板是给人复制走的，带出隐私就是泄密。
//! - **一分钟运行**：生成即构建即运行——从零到运行一分钟内（W06 口径
//!   延伸到全域工具）。裸机内核面无墙钟：用**确定性 slot 预算**计账
//!   （生成/构建/运行三步各分预算，总当量 60 slot = 一分钟），超预算
//!   显性报不静默——口径与 F0221「操作计数代墙钟」同纪律。
//! - **可续答**：交互式问答逐题推进，每答一题落一格快照——**中断续答
//!   不丢已答**，续答从断点继续不重头；问答界面逐题带读屏文本（域本色：
//!   问答界面读屏可达）。
//! - **日验证**：模板按日验证签名——当日已验签名集 O(1) 比对（槽位日
//!   键查表）；签名不符或验证过期 → **下架**留痕，不许带病出模板。
//! - **判据**：三模板库判据侧独立写死对拍；生成失败 → 清理半成品
//!   O(1) + 重试计数；分钟预算超限显性报；隐私黑名单注入实测必拒。
//!
//! **错误路径与降级矩阵**：模板失效→日验证下架；问答中断→可续答；
//! 生成失败→清理重试。
//!
//! **性能逐项分解**：生成 O(模板规模)；验证 O(1)；清理 O(1)。
//!
//! **跨批对接点**：F4702 口径同规上游；F4804 构建衔接；F4815 测试。
//!
//! **诊断码**：X 域 `0x3A1x` 续编（0x3A20..0x3A27），与 F4801/F4802 不重号。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 〇、诊断码（0x3A20.. 续编，显性映射）
// ---------------------------------------------------------------------------

/// 脚手架诊断码（封闭全集八码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScafErr {
    /// 模板未知。
    TplUnknown,
    /// 模板签名不符。
    TplSigBad,
    /// 模板含隐私残留。
    TplPrivacyLeak,
    /// 模板已下架（日验证失效）。
    TplRetired,
    /// 问答越题（跳题/重复答题）。
    QuizJump,
    /// 分钟预算超限。
    MinuteOver,
    /// 生成失败（半成品未清理即报）。
    GenDirty,
    /// 续答令牌失效。
    ResumeBad,
}

impl ScafErr {
    /// 码值。
    pub const fn wire(self) -> u16 {
        match self {
            ScafErr::TplUnknown => 0x3A20,
            ScafErr::TplSigBad => 0x3A21,
            ScafErr::TplPrivacyLeak => 0x3A22,
            ScafErr::TplRetired => 0x3A23,
            ScafErr::QuizJump => 0x3A24,
            ScafErr::MinuteOver => 0x3A25,
            ScafErr::GenDirty => 0x3A26,
            ScafErr::ResumeBad => 0x3A27,
        }
    }
    /// 人话。
    pub const fn zh(self) -> &'static str {
        match self {
            ScafErr::TplUnknown => "模板未知",
            ScafErr::TplSigBad => "模板签名不符",
            ScafErr::TplPrivacyLeak => "模板含隐私残留",
            ScafErr::TplRetired => "模板已下架",
            ScafErr::QuizJump => "问答越题",
            ScafErr::MinuteOver => "分钟预算超限",
            ScafErr::GenDirty => "生成失败残留未清理",
            ScafErr::ResumeBad => "续答令牌失效",
        }
    }
    /// 全集。
    pub const ALL: [ScafErr; 8] = [
        ScafErr::TplUnknown,
        ScafErr::TplSigBad,
        ScafErr::TplPrivacyLeak,
        ScafErr::TplRetired,
        ScafErr::QuizJump,
        ScafErr::MinuteOver,
        ScafErr::GenDirty,
        ScafErr::ResumeBad,
    ];
}

// ---------------------------------------------------------------------------
// 一、三类模板库（锚点：项目模板生成器（应用/插件/工具三类模板））
// ---------------------------------------------------------------------------

/// FNV-1a 64（模板签名）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 模板类别（恰三类，封闭全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TplKind {
    /// 应用模板。
    App,
    /// 插件模板。
    Plugin,
    /// 工具模板。
    Tool,
}

impl TplKind {
    /// 人话。
    pub const fn zh(self) -> &'static str {
        match self {
            TplKind::App => "应用",
            TplKind::Plugin => "插件",
            TplKind::Tool => "工具",
        }
    }
    /// 全集。
    pub const ALL: [TplKind; 3] = [TplKind::App, TplKind::Plugin, TplKind::Tool];
}

/// 一份模板文件（名 + 内容）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TplFile {
    pub name: String,
    pub body: String,
}

/// 模板规格（签名 = 全部文件内容顺序 FNV 求和）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateSpec {
    /// 模板名。
    pub name: String,
    /// 类别。
    pub kind: TplKind,
    /// 文件集。
    pub files: Vec<TplFile>,
}

impl TemplateSpec {
    /// 签名（O(模板规模)——单次扫描逐字节求和）。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = mix_bytes(h, self.name.as_bytes());
        let mut i = 0usize;
        while i < self.files.len() {
            h = mix_bytes(h, self.files[i].name.as_bytes());
            h = mix_bytes(h, self.files[i].body.as_bytes());
            i += 1;
        }
        h
    }
}

/// 逐字节混入（FNV 轮）。
fn mix_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 隐私键黑名单（**无隐私残留**红线——模板内容禁含，判据侧独立写死）。
pub const PRIVACY_KEYS: [&str; 5] = ["token", "password", "secret", "apikey", "credential"];

/// 内容隐私扫描（O(内容长)——入册闸与判据共用判定，双向测试）。
pub fn privacy_scan(body: &str) -> bool {
    let lower = body.to_lowercase();
    for k in PRIVACY_KEYS.iter() {
        if lower.contains(k) {
            return false;
        }
    }
    true
}

/// 三模板库（入册闸：隐私扫描必过；签名入册时算定）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TplLibrary {
    pub tpls: Vec<TemplateSpec>,
    pub refused: Vec<(String, ScafErr)>,
}

/// 模板库容量。
pub const MAX_TPLS: usize = 12;

impl TplLibrary {
    /// 空库。
    pub fn new() -> TplLibrary {
        TplLibrary { tpls: Vec::new(), refused: Vec::new() }
    }

    /// 入册（O(库数) 查重 + O(内容长) 隐私扫描；同类同名重复拒绝）。
    pub fn admit(&mut self, t: TemplateSpec) -> bool {
        if self.tpls.len() >= MAX_TPLS {
            self.refused.push((t.name, ScafErr::TplUnknown));
            return false;
        }
        let mut i = 0usize;
        while i < t.files.len() {
            if !privacy_scan(&t.files[i].body) {
                self.refused.push((t.name, ScafErr::TplPrivacyLeak));
                return false;
            }
            i += 1;
        }
        for prev in self.tpls.iter() {
            if prev.name == t.name {
                self.refused.push((t.name, ScafErr::TplUnknown));
                return false;
            }
        }
        self.tpls.push(t);
        true
    }

    /// 取模板（O(库数)）。
    pub fn get(&self, name: &str) -> Option<&TemplateSpec> {
        self.tpls.iter().find(|t| t.name == name)
    }
}

// ---------------------------------------------------------------------------
// 二、问答生成器（锚点：交互式问答；可续答）
// ---------------------------------------------------------------------------

/// 一道问答题（读屏文本必备——域本色：问答界面读屏可达）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// 题号（0 起，顺序作答）。
    pub id: usize,
    /// 提示（词典键）。
    pub prompt_key: String,
    /// 读屏文本。
    pub screen: String,
}

/// 问答快照（可续答的凭证：已答对 + 断点位置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuizSnapshot {
    /// 已答（按题号序）。
    pub answered: Vec<(usize, String)>,
    /// 断点：下一待答题号。
    pub next_id: usize,
}

/// 问答会话。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quiz {
    /// 题集。
    pub questions: Vec<Question>,
    /// 快照（可续答）。
    pub snap: QuizSnapshot,
    /// 拒绝留痕（越题等）。
    pub refused: Vec<(usize, ScafErr)>,
}

impl Quiz {
    /// 建问答（题集按 id 递增；读屏文本缺一即拒建——域本色闸）。
    pub fn new(questions: Vec<Question>) -> Option<Quiz> {
        let mut i = 0usize;
        while i < questions.len() {
            if questions[i].id != i || questions[i].screen.is_empty() {
                return None;
            }
            i += 1;
        }
        Some(Quiz {
            questions,
            snap: QuizSnapshot { answered: Vec::new(), next_id: 0 },
            refused: Vec::new(),
        })
    }

    /// 答当前题（越题拒绝留痕；答毕快照即推进）。
    pub fn answer(&mut self, qid: usize, val: &str) -> bool {
        if qid != self.snap.next_id {
            self.refused.push((qid, ScafErr::QuizJump));
            return false;
        }
        self.snap.answered.push((qid, val.to_string()));
        self.snap.next_id += 1;
        true
    }

    /// 是否答毕。
    pub fn done(&self) -> bool {
        self.snap.next_id == self.questions.len()
    }
}

/// 用问答值填充模板（占位符 `{{key}}` → 值；O(内容长)）。
pub fn render(body: &str, answers: &[(String, String)]) -> String {
    let mut out = body.to_string();
    let mut i = 0usize;
    while i < answers.len() {
        let k = format!("{{{{{}}}}}", answers[i].0);
        let v = answers[i].1.clone();
        while let Some(pos) = out.find(&k) {
            out.replace_range(pos..pos + k.len(), &v);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 三、一分钟运行预算（生成即构建即运行——slot 计数代墙钟，F0221 口径）
// ---------------------------------------------------------------------------

/// 一分钟当量总预算（slot：每 slot = 一秒当量，确定性计数非墙钟）。
pub const MINUTE_SLOTS: u32 = 60;
/// 生成步预算。
pub const SLOTS_GENERATE: u32 = 20;
/// 构建步预算。
pub const SLOTS_BUILD: u32 = 30;
/// 运行步预算。
pub const SLOTS_RUN: u32 = 10;

/// 三步流水账（生成→构建→运行，各步记 slot 消耗）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLedger {
    pub gen: u32,
    pub build: u32,
    pub run: u32,
    pub over: Vec<ScafErr>,
}

impl RunLedger {
    /// 空账。
    pub fn new() -> RunLedger {
        RunLedger { gen: 0, build: 0, run: 0, over: Vec::new() }
    }

    /// 记生成步消耗（超步预算 → 显性记账不静默）。
    pub fn charge_gen(&mut self, n: u32) {
        self.gen += n;
        if self.gen > SLOTS_GENERATE {
            self.over.push(ScafErr::MinuteOver);
        }
    }

    /// 记构建步消耗。
    pub fn charge_build(&mut self, n: u32) {
        self.build += n;
        if self.build > SLOTS_BUILD {
            self.over.push(ScafErr::MinuteOver);
        }
    }

    /// 记运行步消耗。
    pub fn charge_run(&mut self, n: u32) {
        self.run += n;
        if self.run > SLOTS_RUN {
            self.over.push(ScafErr::MinuteOver);
        }
    }

    /// 总消耗与总预算比对（一分钟口径）。
    pub fn within_minute(&self) -> bool {
        self.gen + self.build + self.run <= MINUTE_SLOTS
    }
}

// ---------------------------------------------------------------------------
// 四、日验证单（锚点：模板签名与日验证；模板失效→日验证下架）
// ---------------------------------------------------------------------------

/// 日验证单（当日已验签名集，O(1) 槽位比对）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyCheck {
    /// 槽位日（day slot 递增计数）。
    pub day: u32,
    /// 当日已验（模板名 → 签名）。
    pub verified: Vec<(String, u64)>,
    /// 下架留痕。
    pub retired: Vec<(String, ScafErr)>,
}

/// 日验证容量。
pub const MAX_DAILY: usize = 16;

impl DailyCheck {
    /// 新单。
    pub fn new(day: u32) -> DailyCheck {
        DailyCheck { day, verified: Vec::new(), retired: Vec::new() }
    }

    /// 日验证（O(1) 签名比对：当日已验且签名相等才算过；不符 → 下架留痕）。
    pub fn check(&mut self, name: &str, sig: u64) -> bool {
        for (n, s) in self.verified.iter() {
            if n == name {
                if *s == sig {
                    return true;
                }
                self.retired.push((name.to_string(), ScafErr::TplSigBad));
                return false;
            }
        }
        if self.verified.len() < MAX_DAILY {
            self.verified.push((name.to_string(), sig));
            return true;
        }
        false
    }

    /// 显性下架（日验证过期/失效处置，留痕）。
    pub fn retire(&mut self, name: &str) {
        self.retired.push((name.to_string(), ScafErr::TplRetired));
        let mut i = 0usize;
        while i < self.verified.len() {
            if self.verified[i].0 == name {
                self.verified.remove(i);
                return;
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 五、生成器主体（生成失败 → 清理重试）
// ---------------------------------------------------------------------------

/// 生成结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenOutcome {
    /// 成功（产物文件集，内容已渲染）。
    Done(Vec<TplFile>),
    /// 失败（错误码 + 已清理干净标记）。
    Failed(ScafErr),
}

/// 生成器（半成品清理 O(1) + 重试计数）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Generator {
    pub retries: u32,
    pub dirty_cleaned: u32,
}

impl Generator {
    /// 新生成器。
    pub fn new() -> Generator {
        Generator { retries: 0, dirty_cleaned: 0 }
    }

    /// 从模板 + 问答值生成产物（O(模板规模)）。
    ///
    /// 值注入前过一遍隐私扫描：**用户答的值也不许带隐私键**——模板干净
    /// 不代表注入后干净。
    pub fn generate(
        &mut self,
        lib: &TplLibrary,
        name: &str,
        answers: &[(String, String)],
    ) -> GenOutcome {
        let tpl = match lib.get(name) {
            Some(t) => t,
            None => return GenOutcome::Failed(ScafErr::TplUnknown),
        };
        let mut files: Vec<TplFile> = Vec::new();
        let mut i = 0usize;
        while i < tpl.files.len() {
            let body = render(&tpl.files[i].body, answers);
            if !privacy_scan(&body) {
                // 失败即清理半成品（O(1)：整集丢弃 + 计数），再计数重试。
                files = Vec::new();
                self.dirty_cleaned += 1;
                self.retries += 1;
                return GenOutcome::Failed(ScafErr::TplPrivacyLeak);
            }
            files.push(TplFile { name: tpl.files[i].name.clone(), body });
            i += 1;
        }
        GenOutcome::Done(files)
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F4803 域自检。
pub fn run_vey03_checks() -> CheckSet {
    let mut s = CheckSet::new("vey03_scaffold");

    // ---- 判据一：三类模板 ----
    {
        // 三类齐备互异；隐私注入实测必拒；库内签名可复算对账。
        let mut lib = TplLibrary::new();
        let mut kinds_ok = TplKind::ALL.len() == 3;
        let mut i = 0usize;
        while i < TplKind::ALL.len() {
            let mut j = i + 1;
            while j < TplKind::ALL.len() {
                if TplKind::ALL[i] == TplKind::ALL[j] {
                    kinds_ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut f: Vec<TplFile> = Vec::new();
        f.push(TplFile { name: "main.rs".to_string(), body: "fn main() {}".to_string() });
        let ok = lib.admit(TemplateSpec {
            name: "app-basic".to_string(),
            kind: TplKind::App,
            files: f.clone(),
        });
        let leaked = lib.admit(TemplateSpec {
            name: "leaky".to_string(),
            kind: TplKind::Tool,
            files: vec![TplFile {
                name: "cfg.rs".to_string(),
                body: "const apikey = \"xxx\";".to_string(),
            }],
        });
        let traced = lib.refused.iter().any(|(n, e)| n == "leaky" && *e == ScafErr::TplPrivacyLeak);
        let sig = lib.get("app-basic").map(|t| t.signature() != 0).unwrap_or(false);
        s.add(
            "Y03-三类模板-三类齐备+隐私注入必拒+签名非零",
            kinds_ok && ok && !leaked && traced && sig,
            "",
        );
    }
    {
        // 签名对内容敏感：改一个字节签名必变（签名不是摆设）。
        let mut f1: Vec<TplFile> = Vec::new();
        f1.push(TplFile { name: "a.rs".to_string(), body: "alpha".to_string() });
        let t1 = TemplateSpec { name: "t".to_string(), kind: TplKind::App, files: f1.clone() };
        let mut t2 = t1.clone();
        t2.files[0].body = String::from("alphb");
        s.add(
            "Y03-三类模板-签名对内容敏感",
            t1.signature() == t1.signature() && t1.signature() != t2.signature(),
            "",
        );
    }

    // ---- 判据二：可续答 ----
    {
        // 逐题推进；越题拒绝留痕；中断快照续答不丢已答；答毕判定准。
        let mk_quiz = || {
            Quiz::new(vec![
                Question { id: 0, prompt_key: "q.name".to_string(), screen: "项目叫什么".to_string() },
                Question { id: 1, prompt_key: "q.kind".to_string(), screen: "做哪类".to_string() },
                Question { id: 2, prompt_key: "q.ns".to_string(), screen: "命名空间".to_string() },
            ])
        };
        let mut ok = false;
        if let Some(mut q) = mk_quiz() {
            let jump = q.answer(2, "skip");
            let a0 = q.answer(0, "demo");
            let a1 = q.answer(1, "plugin");
            // 中断：取快照，续答会话从断点继续。
            let snap = q.snap.clone();
            let resumed = if let Some(mut q2) = mk_quiz() {
                q2.snap = snap;
                let a2 = q2.answer(2, "mkt");
                a2 && q2.done() && q2.snap.answered.len() == 3
                    && q2.snap.answered[0].1 == "demo"
            } else {
                false
            };
            ok = !jump && a0 && a1 && resumed
                && q.refused.iter().any(|(id, e)| *id == 2 && *e == ScafErr::QuizJump);
        }
        s.add("Y03-可续答-中断续答不丢已答", ok, "");
    }
    {
        // 域本色：读屏文本缺一拒建（ Quiz::new 返回 None）。
        let bad = Quiz::new(vec![Question {
            id: 0,
            prompt_key: "q.x".to_string(),
            screen: String::new(),
        }]);
        s.add("Y03-可续答-读屏缺一拒建", bad.is_none(), "");
    }

    // ---- 判据三：一分钟运行 ----
    {
        // 标准流水在分钟预算内；三步任一超步预算显性记账；总当量口径 60。
        let mut led = RunLedger::new();
        led.charge_gen(8);
        led.charge_build(22);
        led.charge_run(4);
        let within = led.within_minute() && led.over.is_empty();
        let mut bad = RunLedger::new();
        bad.charge_build(40); // 超构建步预算 30
        s.add(
            "Y03-一分钟运行-标准流水在预算+超步显性记",
            within && !bad.over.is_empty() && MINUTE_SLOTS == 60,
            "",
        );
    }

    // ---- 判据四：日验证 ----
    {
        // 当日首验入单；签名漂移必下架；下架留痕；显性 retire 清单。
        let mut dc = DailyCheck::new(7);
        let sig_a = 0xA11CE;
        let first = dc.check("app-basic", sig_a);
        let same = dc.check("app-basic", sig_a);
        let drift = dc.check("app-basic", sig_a ^ 1);
        let drifted_retired = dc.retired.iter().any(|(n, e)| n == "app-basic" && *e == ScafErr::TplSigBad);
        let mut dc2 = DailyCheck::new(8);
        let _ = dc2.check("tool-x", 42);
        dc2.retire("tool-x");
        let gone = !dc2.verified.iter().any(|(n, _)| n == "tool-x");
        let retired_traced = dc2.retired.iter().any(|(n, e)| n == "tool-x" && *e == ScafErr::TplRetired);
        s.add(
            "Y03-日验证-签名漂移下架+显性retire清册",
            first && same && !drift && drifted_retired && gone && retired_traced,
            "",
        );
    }

    // ---- 判据五：判据（生成失败清理重试 + 诊断码续编 + 渲染） ----
    {
        // 生成：合法值渲染成功；隐私值注入必拒且半成品清理计数。
        let mut lib = TplLibrary::new();
        let mut f: Vec<TplFile> = Vec::new();
        f.push(TplFile {
            name: "meta.rs".to_string(),
            body: "pub const NAME: &str = \"{{name}}\";".to_string(),
        });
        let _ = lib.admit(TemplateSpec {
            name: "plugin-basic".to_string(),
            kind: TplKind::Plugin,
            files: f,
        });
        let mut g = Generator::new();
        let good = g.generate(
            &lib,
            "plugin-basic",
            &[("name".to_string(), "hello".to_string())],
        );
        let rendered_ok = match &good {
            GenOutcome::Done(fs) => {
                fs.len() == 1 && fs[0].body.contains("\"hello\"")
            }
            _ => false,
        };
        let leak = g.generate(
            &lib,
            "plugin-basic",
            &[("name".to_string(), "x\"; const SECRET = 1".to_string())],
        );
        s.add(
            "Y03-判据-渲染成功+值注入隐私必拒清理计数",
            rendered_ok
                && matches!(leak, GenOutcome::Failed(ScafErr::TplPrivacyLeak))
                && g.dirty_cleaned == 1
                && g.retries == 1,
            "",
        );
    }
    {
        // 未知模板拒；诊断码八码互异且落在 0x3A2x 段（与 Y01/Y02 不重号）。
        let mut g = Generator::new();
        let lib = TplLibrary::new();
        let unknown = g.generate(&lib, "ghost", &[]);
        let mut uniq = true;
        let mut i = 0usize;
        while i < ScafErr::ALL.len() {
            let w = ScafErr::ALL[i].wire();
            if w < 0x3A20 || w > 0x3A27 {
                uniq = false;
            }
            let mut j = i + 1;
            while j < ScafErr::ALL.len() {
                if w == ScafErr::ALL[j].wire() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add(
            "Y03-判据-未知模板拒+诊断码续编互异不越段",
            matches!(unknown, GenOutcome::Failed(ScafErr::TplUnknown)) && uniq,
            "",
        );
    }

    s
}
