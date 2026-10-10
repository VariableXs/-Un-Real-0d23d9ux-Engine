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
//!   键查表：条目带当日日戳，非当日日戳即**验证过期**）；签名不符或
//!   验证过期 → **下架**留痕，不许带病出模板；日单容量满显性拒。
//! - **判据**：三模板库判据侧独立写死对拍；生成失败 → 清理半成品
//!   O(1) + 重试计数；产物**占位符零残留**（未填即败不产半成品）；
//!   分钟预算超限显性报；隐私黑名单注入实测必拒；渲染值自指不死循环。
//!
//! **错误路径与降级矩阵**：模板失效→日验证下架；问答中断→可续答；
//! 生成失败→清理重试。
//!
//! **性能逐项分解**：生成 O(模板规模)；验证 O(1)（有界 ≤MAX_DAILY 查表）；
//! 清理 O(1)。
//!
//! **跨批对接点**：F4702 口径同规上游；F4804 构建衔接；F4815 测试。
//!
//! **诊断码**：X 域 `0x3A1x` 续编（0x3A20..0x3A2A），与 F4801/F4802 不重号。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 〇、诊断码（0x3A20.. 续编，显性映射）
// ---------------------------------------------------------------------------

/// 脚手架诊断码（封闭全集十一码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScafErr {
    /// 模板未知。
    TplUnknown,
    /// 模板签名不符。
    TplSigBad,
    /// 模板含隐私残留。
    TplPrivacyLeak,
    /// 模板已下架（日验证失效/验证过期）。
    TplRetired,
    /// 问答越题（跳题/重复答题）。
    QuizJump,
    /// 分钟预算超限。
    MinuteOver,
    /// 生成失败（半成品未清理即报）。
    GenDirty,
    /// 续答令牌失效。
    ResumeBad,
    /// 占位符未填（产物零残留闸）。
    TplFill,
    /// 库/日单容量满（显性拒）。
    TplFull,
    /// 模板重名重复入册。
    TplDup,
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
            ScafErr::TplFill => 0x3A28,
            ScafErr::TplFull => 0x3A29,
            ScafErr::TplDup => 0x3A2A,
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
            ScafErr::TplFill => "占位符未填",
            ScafErr::TplFull => "库/日单已满",
            ScafErr::TplDup => "模板重名",
        }
    }
    /// 全集。
    pub const ALL: [ScafErr; 11] = [
        ScafErr::TplUnknown,
        ScafErr::TplSigBad,
        ScafErr::TplPrivacyLeak,
        ScafErr::TplRetired,
        ScafErr::QuizJump,
        ScafErr::MinuteOver,
        ScafErr::GenDirty,
        ScafErr::ResumeBad,
        ScafErr::TplFill,
        ScafErr::TplFull,
        ScafErr::TplDup,
    ];
}

// ---------------------------------------------------------------------------
// 〇 bis、跨批对接点（锚点原文三条，码面钉死防漂移）
// ---------------------------------------------------------------------------

/// 跨批对接点（锚点原文：F4702 口径同规上游；F4804 构建衔接；F4815 测试）。
pub const DOCK_UPSTREAM: &str = "VE-F4702";
/// 构建衔接对端（F4804 构建系统对接——分钟预算的构建步由其落地）。
pub const DOCK_BUILD: &str = "VE-F4804";
/// 测试对端（F4815 工具链测试——判据集由其消费）。
pub const DOCK_TEST: &str = "VE-F4815";
/// 三点全集（判据侧独立对拍）。
pub const DOCKS: [&str; 3] = [DOCK_UPSTREAM, DOCK_BUILD, DOCK_TEST];

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
            self.refused.push((t.name, ScafErr::TplFull));
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
                self.refused.push((t.name, ScafErr::TplDup));
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

/// 用问答值填充模板（占位符 `{{key}}` → 值；O(内容长) 单趟扫描）。
///
/// 单趟推进不回扫已填入的值——**值里自指键**（值含 `{{key}}` 自身）也
/// 必然终止，不会把替换当新占位符无限循环；未命中的键原样留下，交由
/// 产物零残留闸（[`no_residue`]）判失败，不静默吞半成品。
pub fn render(body: &str, answers: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(pos) = rest.find("{{") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = &after[..end];
                let mut hit = false;
                for kv in answers.iter() {
                    if kv.0.as_str() == key {
                        out.push_str(&kv.1);
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    // 未命中：原样留键，零残留闸会拒（显性失败优于静默半成品）。
                    out.push_str("{{");
                    out.push_str(key);
                    out.push_str("}}");
                }
                rest = &after[end + 2..];
            }
            None => {
                // 没有闭合括号的 "{{"：原样输出剩余全部（同样过零残留闸）。
                out.push_str(&rest[pos..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 占位符零残留判定（产物里不许留下未填的 `{{…}}`——生成即败的判据）。
pub fn no_residue(body: &str) -> bool {
    !body.contains("{{")
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

    /// 总消耗与总预算比对（一分钟口径：无超支记账且总当量 ≤60 才算
    /// 「从零到运行一分钟内」——分账超步即越口径，总和小也不放水）。
    pub fn within_minute(&self) -> bool {
        self.over.is_empty() && self.gen + self.build + self.run <= MINUTE_SLOTS
    }
}

// ---------------------------------------------------------------------------
// 四、日验证单（锚点：模板签名与日验证；模板失效→日验证下架）
// ---------------------------------------------------------------------------

/// 日验证单条目（槽位日键：条目带验证当日日戳，非当日即过期）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyEntry {
    /// 模板名。
    pub name: String,
    /// 验证当时的签名。
    pub sig: u64,
    /// 验证当日日戳（与单的 `day` 相等才算「当日已验」）。
    pub day: u32,
}

/// 日验证单（当日已验签名集，有界 ≤MAX_DAILY 槽位查表 = 常量时间）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyCheck {
    /// 槽位日（day slot 递增计数）。
    pub day: u32,
    /// 当日已验（条目带日戳——昨日的通过不算今日的）。
    pub verified: Vec<DailyEntry>,
    /// 下架留痕（签名漂移/验证过期/显性下架）。
    pub retired: Vec<(String, ScafErr)>,
    /// 拒绝留痕（日单容量满——显性拒不静默丢）。
    pub refused: Vec<(String, ScafErr)>,
}

/// 日验证容量。
pub const MAX_DAILY: usize = 16;

impl DailyCheck {
    /// 新单。
    pub fn new(day: u32) -> DailyCheck {
        DailyCheck { day, verified: Vec::new(), retired: Vec::new(), refused: Vec::new() }
    }

    /// 翻日（日键推进：旧日戳条目全部作废——下次 check 即判过期下架）。
    pub fn roll_day(&mut self, day: u32) {
        self.day = day;
    }

    /// 日验证（当日已验且签名相等才算过；签名漂移或**验证过期** →
    /// 下架留痕；日单满 → 显性拒并留痕）。
    pub fn check(&mut self, name: &str, sig: u64) -> bool {
        let mut i = 0usize;
        while i < self.verified.len() {
            if self.verified[i].name == name {
                if self.verified[i].day != self.day {
                    // 验证过期：昨日的通过不赊给今日——下架留痕等重验。
                    self.verified.remove(i);
                    self.retired.push((name.to_string(), ScafErr::TplRetired));
                    return false;
                }
                if self.verified[i].sig == sig {
                    return true;
                }
                self.retired.push((name.to_string(), ScafErr::TplSigBad));
                return false;
            }
            i += 1;
        }
        if self.verified.len() >= MAX_DAILY {
            self.refused.push((name.to_string(), ScafErr::TplFull));
            return false;
        }
        self.verified.push(DailyEntry { name: name.to_string(), sig, day: self.day });
        true
    }

    /// 显性下架（日验证失效处置，留痕）。
    pub fn retire(&mut self, name: &str) {
        self.retired.push((name.to_string(), ScafErr::TplRetired));
        let mut i = 0usize;
        while i < self.verified.len() {
            if self.verified[i].name == name {
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
            if !no_residue(&body) {
                // 占位符未填即败：产物零残留是 all-or-nothing 的一环。
                drop(files);
                self.dirty_cleaned += 1;
                self.retries += 1;
                return GenOutcome::Failed(ScafErr::TplFill);
            }
            if !privacy_scan(&body) {
                // 失败即清理半成品（O(1)：整集丢弃 + 计数），再计数重试。
                drop(files);
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
        let gone = !dc2.verified.iter().any(|e| e.name == "tool-x");
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
        // 未知模板拒；诊断码十一码互异且落在 0x3A2x 段（与 Y01/Y02 不重号）。
        let mut g = Generator::new();
        let lib = TplLibrary::new();
        let unknown = g.generate(&lib, "ghost", &[]);
        let mut uniq = true;
        let mut i = 0usize;
        while i < ScafErr::ALL.len() {
            let w = ScafErr::ALL[i].wire();
            if w < 0x3A20 || w > 0x3A2A {
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
    {
        // 日验证过期：翻日后同签名的旧通过作废——下架留痕等重验。
        let mut dc = DailyCheck::new(7);
        let first = dc.check("app-basic", 0xA11CE);
        dc.roll_day(8);
        let expired = dc.check("app-basic", 0xA11CE);
        let traced = dc.retired.iter().any(|(n, e)| n == "app-basic" && *e == ScafErr::TplRetired);
        // 重验放行：过期下架后按当日重新入单（下架 ≠ 删库）。
        let reok = dc.check("app-basic", 0xA11CE);
        s.add(
            "Y03-日验证-验证过期下架留痕+可重验",
            first && !expired && traced && reok,
            "",
        );
    }
    {
        // 日单满显性拒：第 MAX_DAILY+1 个模板被拒且留痕，不静默丢。
        let mut dc = DailyCheck::new(1);
        let mut all_ok = true;
        let mut i = 0usize;
        while i < MAX_DAILY {
            let mut nm = String::from("t");
            nm.push_str(&i.to_string());
            if !dc.check(&nm, i as u64) {
                all_ok = false;
            }
            i += 1;
        }
        let full_rej = !dc.check("one-more", 0xFF);
        let full_traced = dc.refused.iter().any(|(n, e)| n == "one-more" && *e == ScafErr::TplFull);
        s.add(
            "Y03-日验证-日单满显性拒留痕",
            all_ok && full_rej && full_traced,
            "",
        );
    }
    {
        // 库满显性拒 + 重名入册拒：拒绝必留痕可审计（重名在未满时先判，
        // 库满时容量先判——两者都只认显性码，不静默吞）。
        let mut lib = TplLibrary::new();
        let mk = || vec![TplFile { name: "m.rs".to_string(), body: "fn main() {}".to_string() }];
        // 未满时重名：TplDup。
        let a1 = lib.admit(TemplateSpec { name: "a".to_string(), kind: TplKind::App, files: mk() });
        let dup = lib.admit(TemplateSpec { name: "a".to_string(), kind: TplKind::App, files: mk() });
        let dup_traced = lib.refused.iter().any(|(n, e)| n == "a" && *e == ScafErr::TplDup);
        // 补满到 MAX_TPLS（异名）。
        let mut filled = true;
        let mut i = 1usize;
        while i < MAX_TPLS {
            let mut nm = String::from("t");
            nm.push_str(&i.to_string());
            if !lib.admit(TemplateSpec { name: nm, kind: TplKind::App, files: mk() }) {
                filled = false;
            }
            i += 1;
        }
        // 已满再入册：TplFull。
        let full = lib.admit(TemplateSpec { name: "z".to_string(), kind: TplKind::App, files: mk() });
        let full_traced = lib.refused.iter().any(|(n, e)| n == "z" && *e == ScafErr::TplFull);
        s.add(
            "Y03-三类模板-库满显性拒+重名入册拒均留痕",
            a1 && !dup && dup_traced && filled && !full && full_traced && lib.tpls.len() == MAX_TPLS,
            "",
        );
    }
    {
        // 占位符未填即败：答案缺键 → 生成失败（TplFill）+ 半成品清理 + 重试计数。
        let mut lib = TplLibrary::new();
        let files = vec![TplFile {
            name: "meta.rs".to_string(),
            body: "pub const NAME: &str = \"{{name}}\";\npub const NS: &str = \"{{ns}}\";".to_string(),
        }];
        let _ = lib.admit(TemplateSpec { name: "plugin-basic".to_string(), kind: TplKind::Plugin, files });
        let mut g = Generator::new();
        let half = g.generate(&lib, "plugin-basic", &[("name".to_string(), "hello".to_string())]);
        s.add(
            "Y03-判据-占位符未填即败零残留",
            matches!(half, GenOutcome::Failed(ScafErr::TplFill))
                && g.dirty_cleaned == 1
                && g.retries == 1,
            "",
        );
    }
    {
        // 渲染自指值不死循环（回归）：值里含键自身也必然终止且结果确定。
        let answers = [("name".to_string(), "x{{name}}".to_string())];
        let out = render("a={{name}}", &answers);
        s.add(
            "Y03-判据-渲染自指值终止不死循环",
            out == "a=x{{name}}" && !no_residue(&out),
            "",
        );
    }
    {
        // 跨批对接三点：非空互异且都是 VE-F 编号（锚点原文钉死）。
        let mut docks_ok = DOCKS.len() == 3;
        let mut i = 0usize;
        while i < DOCKS.len() {
            if !DOCKS[i].starts_with("VE-F") {
                docks_ok = false;
            }
            let mut j = i + 1;
            while j < DOCKS.len() {
                if DOCKS[i] == DOCKS[j] {
                    docks_ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add(
            "Y03-跨批对接-F4702同规上游+F4804构建衔接+F4815测试",
            docks_ok && DOCK_BUILD == "VE-F4804",
            "",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_lib() -> TplLibrary {
        let mut lib = TplLibrary::new();
        assert!(lib.admit(TemplateSpec {
            name: "app-basic".to_string(),
            kind: TplKind::App,
            files: vec![
                TplFile { name: "main.rs".to_string(), body: "fn main() {}".to_string() },
                TplFile {
                    name: "state".to_string().to_string(),
                    body: "pub struct State;".to_string(),
                },
            ],
        }));
        assert!(lib.admit(TemplateSpec {
            name: "plugin-basic".to_string(),
            kind: TplKind::Plugin,
            files: vec![TplFile {
                name: "lib.rs".to_string(),
                body: "pub fn init() {}".to_string(),
            }],
        }));
        assert!(lib.admit(TemplateSpec {
            name: "tool-basic".to_string(),
            kind: TplKind::Tool,
            files: vec![TplFile {
                name: "main.rs".to_string(),
                body: "fn main() {}".to_string(),
            }],
        }));
        lib
    }

    #[test]
    fn y03_all_criteria_pass() {
        let s = run_vey03_checks();
        let (_p, f) = s.tally();
        assert_eq!(f, 0, "VE-F4803 判据存在红项");
        assert!(!s.truncated(), "判据集不应被截断");
    }

    #[test]
    fn y03_three_kinds_library_exact_and_closed() {
        let lib = app_lib();
        assert_eq!(TplKind::ALL.len(), 3, "三类恰为三类");
        let mut seen: Vec<TplKind> = Vec::new();
        for k in TplKind::ALL.iter() {
            assert!(
                lib.tpls.iter().any(|t| t.kind == *k),
                "{} 类模板缺位（齐全性是判据）",
                k.zh()
            );
            assert!(!seen.contains(k), "同类模板重复计册");
            seen.push(*k);
        }
        assert_eq!(lib.tpls.len(), 3, "库恰三类");
    }

    #[test]
    fn y03_signature_detects_single_byte_drift() {
        let lib = app_lib();
        let t = lib.get("app-basic").unwrap();
        let mut drifted = t.clone();
        drifted.files[0].body.push(' ');
        assert_ne!(t.signature(), drifted.signature(), "单字节漂移签名必变");
        assert_eq!(t.signature(), t.signature(), "同内容签名可复现");
    }

    #[test]
    fn y03_privacy_template_refused_with_trace() {
        let mut lib = TplLibrary::new();
        assert!(!lib.admit(TemplateSpec {
            name: "leaky".to_string(),
            kind: TplKind::Tool,
            files: vec![TplFile {
                name: "cfg.rs".to_string(),
                body: "const token = \"t\";".to_string(),
            }],
        }));
        assert!(lib.get("leaky").is_none(), "带隐私模板不得入册");
        assert!(
            lib.refused.iter().any(|(n, e)| n == "leaky" && *e == ScafErr::TplPrivacyLeak),
            "拒载须留痕可审计"
        );
    }

    #[test]
    fn y03_quiz_resume_replays_from_breakpoint() {
        let qs = vec![
            Question { id: 0, prompt_key: "q.name".to_string(), screen: "项目名".to_string() },
            Question { id: 1, prompt_key: "q.kind".to_string(), screen: "类别".to_string() },
            Question { id: 2, prompt_key: "q.ns".to_string(), screen: "命名空间".to_string() },
        ];
        let mut q = Quiz::new(qs.clone()).unwrap();
        assert!(!q.answer(2, "越题"), "越题必拒");
        assert!(q.answer(0, "demo"), "顺序作答应过");
        assert!(q.answer(1, "plugin"), "顺序作答应过");
        assert!(!q.done(), "未答毕不得判完成");
        let snap = q.snap.clone();
        let mut q2 = Quiz::new(qs).unwrap();
        q2.snap = snap;
        assert!(q2.answer(2, "mkt"), "断点续答应继续");
        assert!(q2.done());
        assert_eq!(q2.snap.answered[0].1, "demo", "续答不丢已答");
        assert!(q.refused.iter().any(|(id, e)| *id == 2 && *e == ScafErr::QuizJump));
    }

    #[test]
    fn y03_quiz_requires_screen_text() {
        let bad = Quiz::new(vec![Question {
            id: 0,
            prompt_key: "q.x".to_string(),
            screen: String::new(),
        }]);
        assert!(bad.is_none(), "读屏文本缺一拒建");
    }

    #[test]
    fn y03_minute_slot_budget_explicit_over() {
        let mut led = RunLedger::new();
        led.charge_gen(8);
        led.charge_build(22);
        led.charge_run(4);
        assert!(led.within_minute(), "标准流水恰在预算内");
        assert!(led.over.is_empty(), "标准流水不应有超支");
        assert_eq!(MINUTE_SLOTS, 60, "总当量恰一分钟");
        assert_eq!(SLOTS_GENERATE + SLOTS_BUILD + SLOTS_RUN, 60, "三分账恰闭合");
        let mut bad = RunLedger::new();
        bad.charge_build(SLOTS_BUILD + 1);
        assert!(!bad.within_minute(), "超步即越分钟口径");
        assert!(bad.over.contains(&ScafErr::MinuteOver), "超支显性记账");
    }

    #[test]
    fn y03_daily_drift_retires_with_trace() {
        let mut dc = DailyCheck::new(7);
        assert!(dc.check("app-basic", 0xA11CE));
        assert!(dc.check("app-basic", 0xA11CE), "同日复验命中");
        assert!(!dc.check("app-basic", 0xA11CE ^ 1), "签名漂移必下架");
        assert!(
            dc.retired.iter().any(|(n, e)| n == "app-basic" && *e == ScafErr::TplSigBad),
            "下架留痕"
        );
        dc.retire("app-basic");
        assert!(!dc.verified.iter().any(|e| e.name == "app-basic"), "retire 清册");
        assert!(dc.retired.iter().any(|(n, e)| n == "app-basic" && *e == ScafErr::TplRetired));
    }

    #[test]
    fn y03_daily_expiry_retires_and_allows_reverify() {
        let mut dc = DailyCheck::new(7);
        assert!(dc.check("app-basic", 0xA11CE));
        dc.roll_day(8);
        assert!(!dc.check("app-basic", 0xA11CE), "昨日的通过不赊给今日");
        assert!(
            dc.retired.iter().any(|(n, e)| n == "app-basic" && *e == ScafErr::TplRetired),
            "验证过期下架留痕"
        );
        assert!(dc.check("app-basic", 0xA11CE), "下架 ≠ 删库：可重验");
    }

    #[test]
    fn y03_daily_full_sheet_refuses_with_trace() {
        let mut dc = DailyCheck::new(1);
        let mut i = 0usize;
        while i < MAX_DAILY {
            let mut nm = String::from("t");
            nm.push_str(&i.to_string());
            assert!(dc.check(&nm, i as u64), "容量内逐个放行");
            i += 1;
        }
        assert!(!dc.check("one-more", 0xFF), "日单满显性拒");
        assert!(
            dc.refused.iter().any(|(n, e)| n == "one-more" && *e == ScafErr::TplFull),
            "拒绝留痕不静默"
        );
    }

    #[test]
    fn y03_library_rejects_full_and_duplicate_with_trace() {
        let mut lib = TplLibrary::new();
        let mk = || vec![TplFile { name: "m.rs".to_string(), body: "fn main() {}".to_string() }];
        // 未满时重名先判：TplDup。
        assert!(lib.admit(TemplateSpec { name: "a".to_string(), kind: TplKind::App, files: mk() }));
        assert!(!lib.admit(TemplateSpec { name: "a".to_string(), kind: TplKind::App, files: mk() }));
        assert!(lib.refused.iter().any(|(n, e)| n == "a" && *e == ScafErr::TplDup), "重名留痕");
        // 补满（异名）后库满先判：TplFull。
        let mut i = 1usize;
        while i < MAX_TPLS {
            let mut nm = String::from("t");
            nm.push_str(&i.to_string());
            assert!(lib.admit(TemplateSpec { name: nm, kind: TplKind::App, files: mk() }));
            i += 1;
        }
        assert!(!lib.admit(TemplateSpec { name: "z".to_string(), kind: TplKind::App, files: mk() }));
        assert!(lib.refused.iter().any(|(n, e)| n == "z" && *e == ScafErr::TplFull), "库满留痕");
        assert_eq!(lib.tpls.len(), MAX_TPLS, "库恰满不超容");
    }

    #[test]
    fn y03_generate_fails_on_unfilled_placeholder() {
        let mut lib = TplLibrary::new();
        assert!(lib.admit(TemplateSpec {
            name: "plugin-basic".to_string(),
            kind: TplKind::Plugin,
            files: vec![TplFile {
                name: "meta.rs".to_string(),
                body: "pub const NAME: &str = \"{{name}}\";\npub const NS: &str = \"{{ns}}\";".to_string(),
            }],
        }));
        let mut g = Generator::new();
        let half = g.generate(&lib, "plugin-basic", &[("name".to_string(), "hello".to_string())]);
        assert!(matches!(half, GenOutcome::Failed(ScafErr::TplFill)), "占位符未填即败");
        assert_eq!(g.dirty_cleaned, 1, "半成品必清理");
        assert_eq!(g.retries, 1, "重试计数显性");
    }

    #[test]
    fn y03_render_self_referential_value_terminates() {
        let answers = [("name".to_string(), "x{{name}}".to_string())];
        let out = render("a={{name}}", &answers);
        assert_eq!(out, "a=x{{name}}", "自指值单趟替换不死循环");
        assert!(!no_residue(&out), "自指值不算未填残留");
        let miss = render("a={{nope}}", &answers);
        assert_eq!(miss, "a={{nope}}", "未命中键原样留下交零残留闸");
    }

    #[test]
    fn y03_docks_non_empty_distinct() {
        assert_eq!(DOCKS.len(), 3);
        assert_eq!(DOCK_UPSTREAM, "VE-F4702");
        assert_eq!(DOCK_BUILD, "VE-F4804");
        assert_eq!(DOCK_TEST, "VE-F4815");
        assert_ne!(DOCK_BUILD, DOCK_TEST);
    }

    #[test]
    fn y03_generate_all_or_nothing_on_privacy_value() {
        let mut lib = TplLibrary::new();
        assert!(lib.admit(TemplateSpec {
            name: "plugin-basic".to_string(),
            kind: TplKind::Plugin,
            files: vec![TplFile {
                name: "meta.rs".to_string(),
                body: "pub const NAME: &str = \"{{name}}\";".to_string(),
            }],
        }));
        let mut g = Generator::new();
        let ok = g.generate(&lib, "plugin-basic", &[("name".to_string(), "hello".to_string())]);
        assert!(matches!(ok, GenOutcome::Done(ref fs) if fs.len() == 1 && fs[0].body.contains("\"hello\"")));
        let bad = g.generate(
            &lib,
            "plugin-basic",
            &[("name".to_string(), "x\"; const SECRET = 1".to_string())],
        );
        assert!(matches!(bad, GenOutcome::Failed(ScafErr::TplPrivacyLeak)));
        assert_eq!(g.dirty_cleaned, 1, "半成品必清理");
        assert_eq!(g.retries, 1, "重试计数显性");
        let ghost = g.generate(&lib, "ghost", &[]);
        assert!(matches!(ghost, GenOutcome::Failed(ScafErr::TplUnknown)), "未知模板拒");
    }

    #[test]
    fn y03_diag_codes_unique_in_segment() {
        for w in ScafErr::ALL.iter().map(|e| e.wire()) {
            assert!((0x3A20..=0x3A2A).contains(&w), "诊断码越段");
        }
        let mut i = 0usize;
        while i < ScafErr::ALL.len() {
            let mut j = i + 1;
            while j < ScafErr::ALL.len() {
                assert_ne!(ScafErr::ALL[i].wire(), ScafErr::ALL[j].wire(), "诊断码互异");
                j += 1;
            }
            i += 1;
        }
        assert!(!ScafErr::TplPrivacyLeak.zh().is_empty(), "人话非空");
    }
}
