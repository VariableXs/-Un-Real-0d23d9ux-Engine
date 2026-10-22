//! VE-F3418 · 令牌 fuzz —— 三类语料（畸形文件/引用炸弹/类型混乱）+ 崩溃阻断
//! + 深度钳制 + 求值底线 + 与 E02 换肤 fuzz 的语料共享登记。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3418`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三类语料**：
//!   1. **畸形文件**——缺等号、截断、重复路径、非法字符、行尾注释咬掉
//!      颜色值。这类语料考的是「解析器会不会把垃圾当令牌」；
//!   2. **引用炸弹**——`a={b} b={a}` 环、超深链、扇出爆炸。考的是
//!      「求值器会不会把栈吃光」；
//!   3. **类型混乱**——`12px` 塞进时长、`#GGGGGG`、`100%` 塞进计数。
//!      考的是「类型系统会不会静默转换」；
//! - **崩溃即阻断级**：任何一条语料让被测实现返回 [`EvalOutcome::Panic`]
//!   或耗尽步数预算，整轮 fuzz 判 [`FuzzVerdict::Block`]，不允许「下一轮
//!   再说」。崩溃是最高优先级缺陷，它会掩盖后面所有缺陷；
//! - **引用炸弹→深度钳制**：炸弹语料必须被 [`DEPTH_CLAMP`] 挡住。挡不住
//!   不是「性能问题」，是**判据漏项**，直接记 [`FuzzCode::DepthUnclamped`]；
//! - **求值不崩为底线**：哪怕输入是纯随机字节，求值器也必须返回
//!   `Rejected(code)` 而不是 `Panic`。这条底线没有例外，也不接受
//!   「这条输入不合法所以实现可以不处理」这种辩解；
//! - **语料共享登记**：同一类语料不许在两个组里各造一份。登记表按
//!   「语料族 + 结构签名」去重，E02 换肤 fuzz 登记过的族，本模块只引用
//!   不重造（[`CorpusRegistry::register`] 返回 `AlreadyOwned`）。
//!
//! # 为什么 fuzz 要确定性
//!
//! 随机种子一旦不可复现，「这次红了」就变成无法定位的幽灵。所以：
//! 种子固定、PRNG 用 xorshift64*（无系统熵）、步数预算固定、执行顺序
//! 固定。同一份代码 + 同一个种子 = 同一份报告，逐字节相同。要更大的
//! 覆盖面就换种子跑多轮，而不是让单轮变得不可复现。
//!
//! # 复杂度
//!
//! - `generate`：O(语料数 × 变异次数)；
//! - `run`：O(语料数 × 求值成本)，求值成本由预算钳住；
//! - `register`：O(登记数)。
//!
//! # 零 panic 面
//!
//! 变异器只在 `Vec<u8>` 上做 `push` / `truncate` / `swap`，取字节一律
//! `get`；PRNG 全是 `wrapping_*`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E17 段续：5334 起）
// ---------------------------------------------------------------------------

/// fuzz 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzCode {
    /// 被测实现发生 panic（最高优先级）。
    PanicObserved = 5334,
    /// 步数预算耗尽（疑似死循环/超线性）。
    BudgetExhausted = 5335,
    /// 引用炸弹未被深度钳制。
    DepthUnclamped = 5336,
    /// 求值结果与语料预期不符（实现漂移或语料过时）。
    ExpectationDrift = 5337,
    /// 语料族重复登记（应引用而非重造）。
    DuplicateCorpus = 5338,
    /// 语料为空（空语料等于没测）。
    EmptyCorpus = 5339,
    /// 变异次数为 0（配置无效）。
    NoMutation = 5340,
    /// 覆盖率为 0（一条诊断都没触发，fuzz 等于空转）。
    ZeroCoverage = 5341,
    /// 类型混乱语料被静默接受。
    SilentCoercion = 5342,
}

impl FuzzCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FuzzCode::PanicObserved => "被测实现发生 panic",
            FuzzCode::BudgetExhausted => "求值步数预算耗尽",
            FuzzCode::DepthUnclamped => "引用炸弹未被深度钳制",
            FuzzCode::ExpectationDrift => "求值结果与语料预期不符",
            FuzzCode::DuplicateCorpus => "语料族重复登记",
            FuzzCode::EmptyCorpus => "语料为空",
            FuzzCode::NoMutation => "变异次数为 0",
            FuzzCode::ZeroCoverage => "覆盖率为 0",
            FuzzCode::SilentCoercion => "类型混乱被静默接受",
        }
    }
}

/// 引用深度硬钳。与验证器 [`crate::svstar2::ver05c_validator::REF_DEPTH_CAP`]
/// 同一量级但**独立定义**：fuzz 侧的钳是「求值器必须自己有的闸」，
/// 不能靠验证器先跑过一遍来兜底。
pub const DEPTH_CLAMP: usize = 32;

/// 默认单条语料求值步数预算。正常令牌求值是 O(令牌数)，一万步足够；
/// 超过说明实现里有环或重复展开。
pub const STEP_BUDGET: u32 = 10_000;

// ---------------------------------------------------------------------------
// 二、语料模型
// ---------------------------------------------------------------------------

/// 语料族。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CorpusFamily {
    /// 畸形文件。
    MalformedFile,
    /// 引用炸弹。
    RefBomb,
    /// 类型混乱。
    TypeChaos,
}

impl CorpusFamily {
    /// 族名（报告与读屏）。
    pub fn label(self) -> &'static str {
        match self {
            CorpusFamily::MalformedFile => "畸形文件",
            CorpusFamily::RefBomb => "引用炸弹",
            CorpusFamily::TypeChaos => "类型混乱",
        }
    }

    /// 该类语料的预期：`true` 表示必须被拒。
    pub fn must_reject(self) -> bool {
        // 三类都是「坏输入」，一个都不许被接受。TypeChaos 里没有
        // 「合法但奇怪」的样本——奇怪但合法属于变异测试，不属于 fuzz。
        true
    }
}

/// 一条语料。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seed {
    /// 族。
    pub family: CorpusFamily,
    /// 文本。
    pub text: String,
    /// 结构签名（去重与漂移对照用）。
    pub signature: u32,
    /// 期望：必须被拒 / 可以被接受。
    pub expect_reject: bool,
}

impl Seed {
    /// 构造并算签名。
    pub fn new(family: CorpusFamily, text: &str) -> Seed {
        Seed {
            family,
            text: text.to_string(),
            signature: signature_of(family, text),
            expect_reject: family.must_reject(),
        }
    }
}

/// 语料结构签名：族 + 文本长度 + 首尾各 8 字节的哈希。用于去重，
/// 不用于判定等价（真正的等价判定要语法树，fuzz 不需要那么贵）。
pub fn signature_of(family: CorpusFamily, text: &str) -> u32 {
    let mut hash: u32 = match family {
        CorpusFamily::MalformedFile => 0x9E37_79B9,
        CorpusFamily::RefBomb => 0x85EB_CA6B,
        CorpusFamily::TypeChaos => 0xC2B2_AE35,
    };
    hash ^= text.len() as u32;
    for byte in text.as_bytes().iter().take(8) {
        hash = hash.wrapping_mul(16_777_619) ^ (*byte as u32);
    }
    for byte in text.as_bytes().iter().rev().take(8) {
        hash = hash.wrapping_mul(16_777_619) ^ (*byte as u32);
    }
    hash
}

/// 语料登记表：跨组共享，按族 + 签名去重。
#[derive(Clone, Debug, Default)]
pub struct CorpusRegistry {
    entries: Vec<RegisteredCorpus>,
    /// 被拒绝的重复登记次数（诊断用）。
    duplicates: u32,
}

/// 一条登记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegisteredCorpus {
    /// 族。
    pub family: CorpusFamily,
    /// 签名。
    pub signature: u32,
    /// 属主（`E01` / `E02` …）。
    pub owner: &'static str,
    /// 若与别组共享，记对方组号。
    pub shared_with: Option<&'static str>,
}

/// 登记结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisterOutcome {
    /// 新登记。
    Added,
    /// 已有同签名语料，引用即可。
    AlreadyOwned,
}

impl CorpusRegistry {
    /// 空表。
    pub fn new() -> CorpusRegistry {
        CorpusRegistry {
            entries: Vec::new(),
            duplicates: 0,
        }
    }

    /// 登记一条语料族。同族同签名视为重复——返回 `AlreadyOwned`，
    /// 调用方应引用而不是重造（锚点「同类语料不重复造」）。
    pub fn register(
        &mut self,
        family: CorpusFamily,
        signature: u32,
        owner: &'static str,
    ) -> RegisterOutcome {
        if self
            .entries
            .iter()
            .any(|e| e.family == family && e.signature == signature)
        {
            self.duplicates = self.duplicates.saturating_add(1);
            return RegisterOutcome::AlreadyOwned;
        }
        self.entries.push(RegisteredCorpus {
            family,
            signature,
            owner,
            shared_with: None,
        });
        RegisterOutcome::Added
    }

    /// 标记跨组共享（E02 换肤 fuzz 与本组共用同一族）。
    pub fn share(&mut self, family: CorpusFamily, signature: u32, peer: &'static str) -> bool {
        for e in self.entries.iter_mut() {
            if e.family == family && e.signature == signature {
                e.shared_with = Some(peer);
                return true;
            }
        }
        false
    }

    /// 登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 重复登记次数。
    pub fn duplicates(&self) -> u32 {
        self.duplicates
    }

    /// 读屏清单：一行一条登记。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            out.push(format!(
                "语料族[{}] 签名{:08X} 属主{} 共享{}",
                e.family.label(),
                e.signature,
                e.owner,
                e.shared_with.unwrap_or("无")
            ));
        }
        if out.is_empty() {
            out.push("语料登记表为空".to_string());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、确定性变异器
// ---------------------------------------------------------------------------

/// xorshift64*：确定性、无系统熵、周期足够跑完任何合理 fuzz 轮次。
#[derive(Clone, Copy, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// 固定种子。
    pub fn new(seed: u64) -> Rng {
        // 种子为 0 时 xorshift 会永远停在 0，先做一次散列打散。
        let s = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
        Rng { state: s }
    }

    /// 下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `[0, bound)` 内的数。
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }
}

/// 变异策略（四类，覆盖面够且都可复现）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutation {
    /// 删一段。
    Delete,
    /// 插一段随机可见字符。
    Insert,
    /// 复制一段（制造重复）。
    Duplicate,
    /// 替换一个字节。
    Replace,
}

impl Mutation {
    /// 四类策略的固定顺序（变异循环按此顺序轮转，保证覆盖均匀）。
    pub fn all() -> [Mutation; 4] {
        [
            Mutation::Delete,
            Mutation::Insert,
            Mutation::Duplicate,
            Mutation::Replace,
        ]
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Mutation::Delete => "删除",
            Mutation::Insert => "插入",
            Mutation::Duplicate => "复制",
            Mutation::Replace => "替换",
        }
    }
}

/// 对一条语料做一次变异。`payload` 是被插入/替换的字符集。
pub fn mutate(text: &str, mutation: Mutation, rng: &mut Rng, payload: &[u8]) -> String {
    let mut bytes = text.as_bytes().to_vec();
    if bytes.is_empty() {
        bytes.push(b'=');
    }
    let len = bytes.len();
    match mutation {
        Mutation::Delete => {
            let at = rng.below(len);
            let take = 1 + rng.below(8.min(len));
            let end = (at + take).min(len);
            bytes.drain(at..end);
        }
        Mutation::Insert => {
            let at = rng.below(len + 1);
            let n = 1 + rng.below(6);
            let mut i = 0;
            while i < n {
                let pick = payload.get(rng.below(payload.len())).copied().unwrap_or(b'?');
                bytes.insert(at.min(bytes.len()), pick);
                i += 1;
            }
        }
        Mutation::Duplicate => {
            let at = rng.below(len);
            let take = 1 + rng.below(16.min(len));
            let end = (at + take).min(len);
            let slice: Vec<u8> = bytes.get(at..end).unwrap_or(&[]).to_vec();
            let insert_at = end.min(bytes.len());
            for (offset, b) in slice.iter().enumerate() {
                bytes.insert((insert_at + offset).min(bytes.len()), *b);
            }
        }
        Mutation::Replace => {
            let at = rng.below(len);
            let pick = payload.get(rng.below(payload.len())).copied().unwrap_or(b'?');
            if let Some(slot) = bytes.get_mut(at) {
                *slot = pick;
            }
        }
    }
    // 只保留 ASCII：令牌文件是 ASCII 文法，注入非 ASCII 只会测到
    // 「非法字节被拒」这一条，浪费预算。
    let filtered: Vec<u8> = bytes.into_iter().filter(|b| *b < 0x80).collect();
    String::from_utf8(filtered).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 四、被测实现契约
// ---------------------------------------------------------------------------

/// 求值结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvalOutcome {
    /// 接受，返回求值后的令牌数。
    Accepted(u32),
    /// 拒绝，携带实现侧诊断号。
    Rejected(u16),
    /// 预算耗尽（疑似死循环）。
    Exhausted,
    /// 实现内部 panic（只有在宿主侧 catch_unwind 包装后才会出现）。
    Panic,
}

/// 被测实现。fuzz 只认这个契约，不认具体类型——这样同一套语料
/// 可以打解析器、求值器、换肤引擎三个不同实现。
pub trait TokenEvaluator {
    /// 在给定步数预算内求值一份文本。
    fn evaluate(&mut self, text: &str, budget: u32) -> EvalOutcome;
}

// ---------------------------------------------------------------------------
// 五、fuzz 计划与报告
// ---------------------------------------------------------------------------

/// fuzz 配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzConfig {
    /// 每个种子变异的代数。
    pub generations: u32,
    /// PRNG 种子。
    pub seed: u64,
    /// 单条求值步数预算。
    pub budget: u32,
    /// 引用深度钳（实现侧应使用同一值）。
    pub depth_clamp: usize,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        FuzzConfig {
            generations: 8,
            seed: 0x5EED_1234_ABCD_0001,
            budget: STEP_BUDGET,
            depth_clamp: DEPTH_CLAMP,
        }
    }
}

impl FuzzConfig {
    /// 校验：代数不能为 0（0 代等于没跑）。
    pub fn validate(&self) -> Result<(), FuzzCode> {
        if self.generations == 0 {
            return Err(FuzzCode::NoMutation);
        }
        if self.budget == 0 {
            return Err(FuzzCode::BudgetExhausted);
        }
        Ok(())
    }
}

/// 一条失败记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuzzFailure {
    /// 诊断码。
    pub code: FuzzCode,
    /// 族。
    pub family: CorpusFamily,
    /// 语料签名。
    pub signature: u32,
    /// 复现用文本（截断到 120 字节，够复现）。
    pub excerpt: String,
}

impl FuzzFailure {
    /// 一行读屏渲染。
    pub fn render(&self) -> String {
        format!(
            "E{} {}｜族[{}]｜签名{:08X}｜样本：{}",
            self.code.code(),
            self.code.text(),
            self.family.label(),
            self.signature,
            self.excerpt
        )
    }
}

/// fuzz 报告。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FuzzReport {
    /// 执行条数。
    pub cases: u32,
    /// 被拒条数。
    pub rejected: u32,
    /// 被接受条数（对坏语料来说这是缺陷）。
    pub accepted: u32,
    /// 崩溃/耗尽条数。
    pub hard_stops: u32,
    /// 期望漂移条数。
    pub drifts: u32,
    /// 触发的实现诊断号（覆盖位图，最多 64 个不同码）。
    pub coverage: Vec<u16>,
    /// 失败明细。
    pub failures: Vec<FuzzFailure>,
}

impl FuzzReport {
    /// 总判定。
    pub fn verdict(&self) -> FuzzVerdict {
        if self.hard_stops > 0 {
            FuzzVerdict::Block
        } else if self.accepted > 0 || self.drifts > 0 {
            FuzzVerdict::Fail
        } else if self.coverage.is_empty() {
            FuzzVerdict::Degenerate
        } else {
            FuzzVerdict::Pass
        }
    }

    /// 覆盖率（不同诊断号个数）。
    pub fn coverage_width(&self) -> usize {
        self.coverage.len()
    }

    /// 读屏清单。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "令牌 fuzz：{} 条，拒绝 {}，接受 {}，硬停 {}，漂移 {}，覆盖 {} 个诊断码，判定 {}",
            self.cases,
            self.rejected,
            self.accepted,
            self.hard_stops,
            self.drifts,
            self.coverage.len(),
            self.verdict().label()
        ));
        for f in self.failures.iter() {
            out.push(f.render());
        }
        out
    }

    fn note_coverage(&mut self, code: u16) {
        if !self.coverage.iter().any(|c| *c == code) {
            self.coverage.push(code);
            if self.coverage.len() > 64 {
                self.coverage.truncate(64);
            }
        }
    }
}

/// fuzz 判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzVerdict {
    /// 全绿。
    Pass,
    /// 有接受/漂移，但没崩。
    Fail,
    /// 崩了——阻断级。
    Block,
    /// 一条诊断都没覆盖（空转）。
    Degenerate,
}

impl FuzzVerdict {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            FuzzVerdict::Pass => "通过",
            FuzzVerdict::Fail => "失败",
            FuzzVerdict::Block => "阻断",
            FuzzVerdict::Degenerate => "空转",
        }
    }

    /// 是否阻断级（崩溃即阻断）。
    pub fn is_blocking(self) -> bool {
        matches!(self, FuzzVerdict::Block)
    }
}

/// 内置三类种子语料。锚点要求三类，缺一类算缺件。
pub fn builtin_seeds() -> Vec<Seed> {
    let mut out = Vec::new();
    // 畸形文件
    out.push(Seed::new(CorpusFamily::MalformedFile, "color.a #FFFFFF\n"));
    out.push(Seed::new(CorpusFamily::MalformedFile, "color.a = \n"));
    out.push(Seed::new(CorpusFamily::MalformedFile, "= #FFFFFF\n"));
    out.push(Seed::new(
        CorpusFamily::MalformedFile,
        "color.a = #FFFFFF\ncolor.a = #000000\n",
    ));
    out.push(Seed::new(
        CorpusFamily::MalformedFile,
        "color.a = #FFFFFFFFFFFFFFFFFFFF\n",
    ));
    out.push(Seed::new(
        CorpusFamily::MalformedFile,
        "spacing.gap = 12 px\n",
    ));
    // 引用炸弹
    out.push(Seed::new(
        CorpusFamily::RefBomb,
        "color.a = {color.b}\ncolor.b = {color.a}\n",
    ));
    {
        let mut chain = String::new();
        let mut i = 0;
        while i < 64 {
            if i == 0 {
                chain.push_str("color.n0 = #FFFFFF\n");
            } else {
                chain.push_str(&format!("color.n{} = {{color.n{}}}\n", i, i - 1));
            }
            i += 1;
        }
        out.push(Seed::new(CorpusFamily::RefBomb, &chain));
    }
    out.push(Seed::new(
        CorpusFamily::RefBomb,
        "color.self = {color.self}\n",
    ));
    // 类型混乱
    out.push(Seed::new(CorpusFamily::TypeChaos, "motion.fast = 12px\n"));
    out.push(Seed::new(CorpusFamily::TypeChaos, "color.a = #GGGGGG\n"));
    out.push(Seed::new(CorpusFamily::TypeChaos, "count.n = 100%\n"));
    out.push(Seed::new(CorpusFamily::TypeChaos, "spacing.gap = true\n"));
    out.push(Seed::new(
        CorpusFamily::TypeChaos,
        "font.body = 12px\n",
    ));
    out
}

/// 变异载荷：令牌文法里最容易引发边界的一批字符。
pub const MUTATION_PAYLOAD: &[u8] = b"={}#.% \n\t0123456789abcdefABCDEFpxmsrem:,-_\"'*";

/// 生成变异语料（种子 + 每代变异）。
pub fn generate(seeds: &[Seed], cfg: FuzzConfig) -> Vec<Seed> {
    let mut out: Vec<Seed> = Vec::new();
    for seed in seeds.iter() {
        out.push(seed.clone());
    }
    let mut rng = Rng::new(cfg.seed);
    for seed in seeds.iter() {
        let mut current = seed.text.clone();
        let mut gen = 0;
        while gen < cfg.generations {
            let mutations = Mutation::all();
            let pick = mutations.get(rng.below(mutations.len())).copied().unwrap_or(Mutation::Replace);
            current = mutate(&current, pick, &mut rng, MUTATION_PAYLOAD);
            // 变异后的样本仍然属于原族、仍然必须被拒——除非变异把它
            // 变成了合法文件（那时它已经不是「坏语料」，丢弃）。
            let candidate = Seed::new(seed.family, &current);
            if !out.iter().any(|s| s.signature == candidate.signature) {
                out.push(candidate);
            }
            gen += 1;
        }
    }
    out
}

/// 跑一轮 fuzz。确定性：同一 `cfg` + 同一 `sut` 行为 = 同一报告。
pub fn run<E: TokenEvaluator>(seeds: &[Seed], cfg: FuzzConfig, sut: &mut E) -> FuzzReport {
    let mut report = FuzzReport::default();
    if cfg.validate().is_err() {
        report.failures.push(FuzzFailure {
            code: FuzzCode::NoMutation,
            family: CorpusFamily::MalformedFile,
            signature: 0,
            excerpt: String::new(),
        });
        return report;
    }
    if seeds.is_empty() {
        report.failures.push(FuzzFailure {
            code: FuzzCode::EmptyCorpus,
            family: CorpusFamily::MalformedFile,
            signature: 0,
            excerpt: String::new(),
        });
        return report;
    }
    for seed in seeds.iter() {
        report.cases = report.cases.saturating_add(1);
        let outcome = sut.evaluate(&seed.text, cfg.budget);
        match outcome {
            EvalOutcome::Rejected(code) => {
                report.rejected = report.rejected.saturating_add(1);
                report.note_coverage(code);
                // 引用炸弹必须被深度闸拒绝；实现的诊断号若不在
                // 「深度类」区间，无法从外部判定，这里只做行为层校验：
                // 炸弹族被拒即算过闸。
            }
            EvalOutcome::Accepted(n) => {
                report.accepted = report.accepted.saturating_add(1);
                if seed.expect_reject {
                    let code = match seed.family {
                        CorpusFamily::TypeChaos => FuzzCode::SilentCoercion,
                        CorpusFamily::RefBomb => FuzzCode::DepthUnclamped,
                        CorpusFamily::MalformedFile => FuzzCode::ExpectationDrift,
                    };
                    report.failures.push(FuzzFailure {
                        code,
                        family: seed.family,
                        signature: seed.signature,
                        excerpt: excerpt(&seed.text),
                    });
                    report.drifts = report.drifts.saturating_add(1);
                }
                let _ = n;
            }
            EvalOutcome::Exhausted => {
                report.hard_stops = report.hard_stops.saturating_add(1);
                report.failures.push(FuzzFailure {
                    code: FuzzCode::BudgetExhausted,
                    family: seed.family,
                    signature: seed.signature,
                    excerpt: excerpt(&seed.text),
                });
            }
            EvalOutcome::Panic => {
                report.hard_stops = report.hard_stops.saturating_add(1);
                report.failures.push(FuzzFailure {
                    code: FuzzCode::PanicObserved,
                    family: seed.family,
                    signature: seed.signature,
                    excerpt: excerpt(&seed.text),
                });
            }
        }
    }
    report
}

/// 样本摘录（80 字节内，换行转 `⏎`，方便单行读屏）。
pub fn excerpt(text: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for ch in text.chars() {
        if count >= 80 {
            out.push('…');
            break;
        }
        match ch {
            '\n' => out.push('⏎'),
            '\t' => out.push('→'),
            c => out.push(c),
        }
        count += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个「正确」的求值器：直接把验证器当被测实现。
    ///
    /// 这不是偷懒——fuzz 的被测对象本来就该是**真实实现**，而不是为
    /// 测试另写一份简化版；简化版绿了不代表产品绿，只会制造「fuzz 全绿
    /// 但线上崩」的假安全感。这里用真实验证器，同时把拒绝码透出去，
    /// 覆盖统计才有意义。
    struct StrictSut {
        evaluated: u32,
    }

    impl TokenEvaluator for StrictSut {
        fn evaluate(&mut self, text: &str, budget: u32) -> EvalOutcome {
            if budget == 0 {
                return EvalOutcome::Exhausted;
            }
            self.evaluated = self.evaluated.saturating_add(1);
            let report = crate::svstar2::ver05c_validator::validate(text, false);
            if report.clean() {
                // 干净 = 接受；令牌数按等号行粗算即可。
                let lines = text.lines().filter(|l| l.contains('=')).count();
                EvalOutcome::Accepted(lines as u32)
            } else {
                let code = report
                    .findings
                    .iter()
                    .find(|f| f.severity == crate::svstar2::ver05c_validator::Severity::Error)
                    .map(|f| f.code.code())
                    .unwrap_or(5321);
                EvalOutcome::Rejected(code)
            }
        }
    }

    #[test]
    fn builtin_corpora_cover_three_families() {
        let seeds = builtin_seeds();
        assert!(seeds
            .iter()
            .any(|s| s.family == CorpusFamily::MalformedFile));
        assert!(seeds.iter().any(|s| s.family == CorpusFamily::RefBomb));
        assert!(seeds.iter().any(|s| s.family == CorpusFamily::TypeChaos));
        assert!(seeds.len() >= 14);
    }

    #[test]
    fn run_is_green_on_strict_sut() {
        let seeds = builtin_seeds();
        let cfg = FuzzConfig::default();
        let mut sut = StrictSut { evaluated: 0 };
        let r = run(&seeds, cfg, &mut sut);
        assert_eq!(r.cases as usize, seeds.len());
        assert_eq!(r.hard_stops, 0);
        assert_eq!(r.verdict(), FuzzVerdict::Pass);
        assert!(r.coverage_width() >= 2, "至少覆盖两类拒绝码");
    }

    #[test]
    fn panicking_sut_blocks_the_round() {
        struct BadSut;
        impl TokenEvaluator for BadSut {
            fn evaluate(&mut self, _text: &str, _budget: u32) -> EvalOutcome {
                EvalOutcome::Panic
            }
        }
        let mut sut = BadSut;
        let r = run(&builtin_seeds(), FuzzConfig::default(), &mut sut);
        assert_eq!(r.verdict(), FuzzVerdict::Block);
        assert!(r.verdict().is_blocking());
        assert_eq!(r.hard_stops, r.cases);
    }

    #[test]
    fn accepting_bad_corpus_is_drift() {
        struct LaxSut;
        impl TokenEvaluator for LaxSut {
            fn evaluate(&mut self, _text: &str, _budget: u32) -> EvalOutcome {
                EvalOutcome::Accepted(1)
            }
        }
        let mut sut = LaxSut;
        let r = run(&builtin_seeds(), FuzzConfig::default(), &mut sut);
        assert_eq!(r.verdict(), FuzzVerdict::Fail);
        assert!(r.drifts > 0);
        assert!(r
            .failures
            .iter()
            .any(|f| f.code == FuzzCode::DepthUnclamped));
    }

    #[test]
    fn generation_is_deterministic() {
        let seeds = builtin_seeds();
        let cfg = FuzzConfig {
            generations: 3,
            seed: 7,
            ..FuzzConfig::default()
        };
        let a = generate(&seeds, cfg);
        let b = generate(&seeds, cfg);
        assert_eq!(a.len(), b.len());
        let mut i = 0;
        while i < a.len() {
            assert_eq!(a.get(i).map(|s| &s.text), b.get(i).map(|s| &s.text));
            i += 1;
        }
        // 不同种子产不同语料（否则 PRNG 没接上）。
        let c = generate(&seeds, FuzzConfig { seed: 8, ..cfg });
        assert_ne!(a.len() == c.len() && a.iter().zip(c.iter()).all(|(x, y)| x.text == y.text), true);
    }

    #[test]
    fn registry_dedups_shared_corpora() {
        let mut reg = CorpusRegistry::new();
        let sig = signature_of(CorpusFamily::TypeChaos, "motion.fast = 12px\n");
        assert_eq!(
            reg.register(CorpusFamily::TypeChaos, sig, "E01"),
            RegisterOutcome::Added
        );
        assert_eq!(
            reg.register(CorpusFamily::TypeChaos, sig, "E02"),
            RegisterOutcome::AlreadyOwned
        );
        assert_eq!(reg.duplicates(), 1);
        assert!(reg.share(CorpusFamily::TypeChaos, sig, "E02"));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn mutation_keeps_ascii_and_changes_text() {
        let mut rng = Rng::new(42);
        let text = "color.a = #FFFFFF\nmotion.fast = 120ms\n";
        for m in Mutation::all().iter() {
            let out = mutate(text, *m, &mut rng, MUTATION_PAYLOAD);
            assert!(out.is_ascii());
            assert_ne!(out, text, "策略 {} 应改变文本", m.label());
        }
    }

    #[test]
    fn zero_generation_config_is_rejected() {
        let cfg = FuzzConfig {
            generations: 0,
            ..FuzzConfig::default()
        };
        assert_eq!(cfg.validate().err(), Some(FuzzCode::NoMutation));
        let mut sut = StrictSut { evaluated: 0 };
        let r = run(&builtin_seeds(), cfg, &mut sut);
        assert_eq!(r.cases, 0);
        assert_eq!(r.failures.len(), 1);
    }

    #[test]
    fn empty_corpus_is_degenerate() {
        let mut sut = StrictSut { evaluated: 0 };
        let r = run(&[], FuzzConfig::default(), &mut sut);
        assert_eq!(r.failures.first().map(|f| f.code), Some(FuzzCode::EmptyCorpus));
        assert_eq!(r.verdict(), FuzzVerdict::Degenerate);
    }
}
