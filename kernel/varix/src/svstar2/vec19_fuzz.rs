//! VE-F0419 · 词法 fuzz 测试（VE-C 域 · 着色器系统）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0419`
//!
//! **判据（锚点原文）**：三层语料、四不变量、🔴即时修、种子可复现、判据。
//!
//! 本条交付的是**发现机制**，不是修复。词法正确性已由 F0403-F0418 六个单���
//! 保证，但「保证」只覆盖写出来的用例；本条负责回答另一个问题：**怎么知道
//! 还有哪些没写出来的用例**。做法是三层语料喂进去 + 四不变量盯着，被破坏就
//! 记账入缺陷流程。
//!
//! 1. **三层语料**（判据一）。三层各有分工，缺一层就有盲区：
//!    - **随机字节流**（`CorpusLayer::Random`）：均匀伪随机字节，能撞出
//!      「谁都没想过」的组合——包括非法 UTF-8、裸控制字符、零字节。代价是
//!      **几乎全是垃圾**，故只占小比例。
//!    - **变异种子语料**（`CorpusLayer::Mutated`）：以已知合法种子为基线做
//!      位翻转/字节替换/截断/插入。它专攻「改动很小但后果很大」的一类缺陷
//!      （差一个分号、差一个引号），这是纯随机语料几乎撞不到的。
//!    - **语法感知生成器**（`CorpusLayer::GrammarAware`）：按着色器记号的粗
//!      语法拼装「合法与半合法混合」输入。纯随机的输入往往在第一个记号就废掉，
//!      走不到深水区；半合法输入才能真正穿过词法器跑到后面几层。
//!
//! 2. **四不变量**（判据二）。每个输入都必须满足，破坏即记发现项：
//!    - **不崩溃**：扫描器对任意字节流都不得 panic / 越界（内核里 panic
//!      等于整机挂）。判据取自扫描回报的 `panic_free` 字段——**不由本域代填**，
//!      否则这一条恒真、永远抓不到崩溃。
//!    - **不挂起**：处理必须在预算步数内结束。挂起在真实设备上表现为「卡死」，
//!      比崩溃更难定位，故**用步数预算把它转成可判定的失败**。
//!    - **错误有位置**：每条诊断都必须带合法位置三元式（字节偏移 ≤ 输入长、
//!      行号 ≥ 1）。「有错误但不知道在哪」对作者毫无价值。
//!    - **恢复能终止**：F0417 的恢复流必须在有限步内不再产生新的动作。
//!      这条把 F0417 接了进来——否则 fuzz 只验词法本身，验不到恢复层的死循环。
//!
//! 3. **🔴 即时修**（判据三）。发现项按严重度分档，`🔴`（崩溃 / 挂起 / 不变量
//!    破坏）必须**即时进入修复流程**，而 `🟡`（未验证 / 语料质量下降）只记账。
//!    关键是**不允许「已知 🔴 但未修」的状态存在**——`open_critical` > 0 时门禁就
//!    该阻断，这与 F0418 的基准退化门是同一类设计。
//!
//! 4. **种子可复现**（判据四）。每个输入都由 `(层, 主种子, 序号)` 三元组
//!    唯一决定，`replay` 能从三元组重建**逐位相同**的字节流。这是 fuzz 的
//!    全部价值所在：报了个 bug 却复现不了，等于没报。用**自持的 LCG**而非
//!    任何外部随机源，保证跨机器跨版本可复现（`std` 的随机数不保证序列稳定，
//!    拿它做种子就等于放弃可复现性）。
//!
//! 5. **判据**（判据五）：三层语料配比不得退化为单层、四不变量逐条可判、
//!    🔴 未修即阻断、种子回放逐位一致、发现账如实记账不丢项。
//!
//! 零静默纪律：崩溃/挂起/不变量破坏三类发现**一律 🔴 并入账**；预算耗尽视为
//!   挂起（不当作「跑完了」）；步数超限的输入**保留在账里**以便复现；
//!   未接入恢复层**按未验证记账且不计入通过**；`open_critical` 不为空即如实报出。
//! 零 panic 面、零 IO、无全局可变状态。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、确定性伪随机（判据四：种子可复现的基础）
// ---------------------------------------------------------------------------

/// 自持 LCG（数值配方 constants：Knuth 的 64 位乘加常数）。
///
/// **不用 `std` 的随机源**——标准库的随机数实现不保证跨版本序列稳定，用它做
/// 种子就等于放弃可复现性：同一个种子在别人机器上跑出不同输入，发现项就再也
/// 复现不了。fuzz 的全部价值建立在「能复现」上，故随机源必须自持且写死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

/// LCG 乘数（A = 6364136223846793005）。
const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（C = 1442695040888963407）。
const LCG_C: u64 = 1442695040888963407;

impl Rng {
    /// 以种子初始化。
    pub const fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    /// 取下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        // LCG 用 wrapping_mul/wrapping_add：溢出是算法定义的一部分，
        // 用 `checked_*` 会在大种子下「卡住不动」或 panic。
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    /// 取 `[0, n)` 内的数。
    ///
    /// `n == 0` 返回 0（不是 panic 也不是 `n-1` 的回绕值）：调用方传 0 通常
    /// 意味着「这一层没打算取随机数」，回绕出一个接近 `usize::MAX` 的下标
    /// 会直接越界。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }

    /// 当前内部状态（供归档复现时核对）。
    pub const fn state(&self) -> u64 {
        self.state
    }
}

// ---------------------------------------------------------------------------
// 二、三层语料（判据一）
// ---------------------------------------------------------------------------

/// 语料层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusLayer {
    /// 随机字节流：能撞出「谁都没想过」的组合。
    Random,
    /// 变异种子语料：以合法种子为基线做局部破坏。
    Mutated,
    /// 语法感知生成器：合法与半合法记号混合，能穿过词法器到深水区。
    GrammarAware,
}

impl CorpusLayer {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            CorpusLayer::Random => "随机字节流",
            CorpusLayer::Mutated => "变异种子语料",
            CorpusLayer::GrammarAware => "语法感知生成器",
        }
    }

    /// 三层是否齐全（缺一层就有盲区）。
    pub const ALL: [CorpusLayer; 3] = [
        CorpusLayer::Random,
        CorpusLayer::Mutated,
        CorpusLayer::GrammarAware,
    ];
}

/// 一条语料（字节 + 生成它的三元组）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusEntry {
    /// 字节流。
    pub bytes: Vec<u8>,
    /// 来源层。
    pub layer: CorpusLayer,
    /// 主种子。
    pub seed: u64,
    /// 批内序号（`replay` 按它定位，不靠字节内容反查）。
    pub index: u64,
}

impl CorpusEntry {
    /// 复现键（唯一标识这条输入）。
    ///
    /// 三元组齐全才够——只靠字节内容去「猜」是复现不了生成过程的，而报告
    /// 缺陷时需要的是「怎么生成的」而不只是「输入是什么」。
    pub fn replay_key(&self) -> ReplayKey {
        ReplayKey {
            layer: self.layer,
            seed: self.seed,
            index: self.index,
        }
    }
}

/// 复现键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayKey {
    /// 层。
    pub layer: CorpusLayer,
    /// 主种子。
    pub seed: u64,
    /// 批内序号。
    pub index: u64,
}

/// 变异算子（用于 `Mutated` 层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationOp {
    /// 位翻转一个位。
    FlipBit,
    /// 替换一个字节。
    ReplaceByte,
    /// 截断。
    Truncate,
    /// 插入一个字节。
    InsertByte,
}

/// 一批合法种子（变异层的基线）。
pub const SEED_SNIPPETS: [&[u8]; 3] = [b"float4 g = 1.0;", b"vec3 v(1,2,3);", b"#if DEF\n#endif\n"];

/// 每 10 条语料里随机层之外还能放多少（`10 - 变异`）。
const PER_GROUP: u64 = 10;

/// 生成一批语料（三层按配比混合）。
///
/// 配比 `mutated_per_10 / grammar_per_10` 以「每 10 条」为单位表示，避免浮点
/// 配比带来的「到底该生成几条」的歧义：随机层拿剩下的。
pub fn generate_corpus(
    seed: u64,
    count: usize,
    mutated_per_10: u64,
    grammar_per_10: u64,
) -> Vec<CorpusEntry> {
    let mut out: Vec<CorpusEntry> = Vec::new();
    let mut i = 0usize;
    while i < count {
        let layer = pick_layer(seed, i, mutated_per_10, grammar_per_10);
        let bytes = build_layer(seed, layer, i);
        out.push(CorpusEntry {
            bytes,
            layer,
            seed,
            index: i as u64,
        });
        i += 1;
    }
    out
}

/// 按配比选层。
///
/// `slot` 取值恒在 `0..PER_GROUP`，所以配比超过 10 的部分**自动失效**：配比
/// 「变异 6 + 语法 9」会占满全部槽位、随机层拿 0 条，而不是凭空多出槽位或
/// 改写别的层的份额。这里**刻意不做夹取**——夹了与不夹行为完全一致
/// （`slot < 99` 与 `slot < 10` 在 `slot < 10` 下同真），加一层「看起来在防
/// 越界」的死代码只会让人误以为这里有边界防护。配比是否因此退化由
/// `FuzzBatch::covers_all_layers` 与门禁如实报出。
pub fn pick_layer(
    _seed: u64,
    index: usize,
    mutated_per_10: u64,
    grammar_per_10: u64,
) -> CorpusLayer {
    let slot = (index % PER_GROUP as usize) as u64;
    if slot < mutated_per_10 {
        CorpusLayer::Mutated
    } else if slot < mutated_per_10 + grammar_per_10 {
        CorpusLayer::GrammarAware
    } else {
        CorpusLayer::Random
    }
}

/// 按层生成字节流。
pub fn build_layer(seed: u64, layer: CorpusLayer, index: usize) -> Vec<u8> {
    // 每条语料用「主种子 + 层 + 序号」派生子种子：这样单独抽第 N 条也能
    // 复现，不必重跑前面所有条目。
    let mut r = Rng::new(mix_seed(seed, layer, index as u64));
    match layer {
        CorpusLayer::Random => {
            let len = 1 + r.below(24) as usize;
            let mut v: Vec<u8> = Vec::with_capacity(len);
            let mut i = 0;
            while i < len {
                v.push(r.next_u64() as u8);
                i += 1;
            }
            v
        }
        CorpusLayer::Mutated => {
            let base = SEED_SNIPPETS[index % SEED_SNIPPETS.len()];
            let mut v: Vec<u8> = base.to_vec();
            let _ = apply_mutation(&mut v, &mut r);
            v
        }
        CorpusLayer::GrammarAware => grammar_input(&mut r),
    }
}

/// 派生子种子（把三层与序号混进主种子）。
pub fn mix_seed(seed: u64, layer: CorpusLayer, index: u64) -> u64 {
    let tag = match layer {
        CorpusLayer::Random => 0x11u64,
        CorpusLayer::Mutated => 0x22,
        CorpusLayer::GrammarAware => 0x33,
    };
    seed ^ tag.wrapping_mul(LCG_C) ^ index.wrapping_mul(LCG_A)
}

/// 对种子做一次变异，返回**实际用的算子**。
///
/// 返回算子不是装饰：自检要据此核对四种算子是否都被真正用到过（而不是
/// 「代码里写着四种」就算数）。
pub fn apply_mutation(v: &mut Vec<u8>, r: &mut Rng) -> MutationOp {
    if v.is_empty() {
        v.push(r.next_u64() as u8);
        return MutationOp::InsertByte;
    }
    let op = match r.below(4) {
        0 => MutationOp::FlipBit,
        1 => MutationOp::ReplaceByte,
        2 => MutationOp::Truncate,
        _ => MutationOp::InsertByte,
    };
    let pos = r.below(v.len() as u64) as usize;
    match op {
        MutationOp::FlipBit => {
            let b = v[pos];
            let bit = 1u8 << (r.below(8) as u32);
            v[pos] = b ^ bit;
        }
        MutationOp::ReplaceByte => {
            v[pos] = replace_byte_at(v[pos], r.next_u64() as u8);
        }
        MutationOp::Truncate => {
            let cut = pos.min(v.len() - 1);
            v.truncate(cut);
        }
        MutationOp::InsertByte => {
            v.insert(pos, r.next_u64() as u8);
        }
    }
    op
}

/// 字节替换的取值规则：抽到的字节与原字节相同时改用其按位取反。
///
/// 抽成独立纯函数是为了让「替换必须真的改动字节」这条**性质**可被表外形态
/// 直接验证：直接验「跑一万次变异有没有恰好抽回原字节」只能靠概率碰
/// （LCG 低 8 位与任意给定字节的映射没有交点，一万次里命中 0 次），那种判据
/// 是空断言。这里 (5, 5) 一调即知，不必赌概率。
pub fn replace_byte_at(original: u8, drawn: u8) -> u8 {
    if drawn == original {
        original ^ 0xFF
    } else {
        drawn
    }
}

/// 语法感知输入：合法片段 + 半合法片段交替。
///
/// 目的是让输入能**穿过词法器到后面几层**——纯随机输入往往在第一个记号就废掉，
/// 根本走不到深水区。
fn grammar_input(r: &mut Rng) -> Vec<u8> {
    // 合法记号：标识符/数字/分号/花括号/注释起始
    const LEGAL: [&[u8]; 5] = [b"id", b"42", b";", b"{", b"/*c*/"];
    // 半合法记号：单边花括号、裸引号、缺分号、多余运算符
    const SEMI: [&[u8]; 5] = [b"\"", b"}", b"1.2.3", b"=", b"@"];
    let mut v: Vec<u8> = Vec::new();
    let mut n = 0usize;
    while n < 8 {
        if r.below(2) == 0 {
            v.extend_from_slice(LEGAL[r.below(5) as usize]);
        } else {
            v.extend_from_slice(SEMI[r.below(5) as usize]);
        }
        n += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 三、四不变量（判据二）
// ---------------------------------------------------------------------------

/// 一条诊断（词法侧最小形：码 + 字节偏移 + 行号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexDiag {
    /// 错误码。
    pub code: &'static str,
    /// 字节偏移。
    pub offset: usize,
    /// 行号（≥1）。
    pub line: u16,
}

/// 一次扫描的回报。
///
/// `panic_free` 由**被测扫描器**如实填写（真 panic 在 no_std 下由调用方的
/// 看门狗/测试壳捕获后置 false）。本域不代填——代填等于把「不崩溃」这条
/// 恒真化，崩溃就再也抓不到了。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOutcome {
    /// 诊断序列。
    pub diags: Vec<LexDiag>,
    /// 消耗步数。
    pub steps: u64,
    /// 是否因超预算而中止。
    pub truncated: bool,
    /// 本次扫描是否全程无 panic / 无越界。
    pub panic_free: bool,
}

/// 词法扫描的最小契约（本域只验这四条，其余归 F0403-F0417）。
pub trait LexUnderTest {
    /// 扫描：返回诊断序列、消耗步数与是否超预算中止。
    ///
    /// `budget` 是步数上限——实现方**必须**在超预算时停止并返回
    /// `truncated = true`，而不是继续跑（那就是挂起）。
    fn scan(&self, input: &[u8], budget: u64) -> ScanOutcome;
}

/// 四不变量的检查结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvariantResult {
    /// 不崩溃（取自扫描回报的 `panic_free`）。
    pub no_panic: bool,
    /// 不挂起（未超预算）。
    pub no_hang: bool,
    /// 错误有位置（每条诊断的偏移 ≤ 输入长、行号 ≥ 1）。
    pub positioned: bool,
    /// 恢复能终止（恢复动作数在有限步内收敛）。
    pub recovery_terminates: bool,
    /// 第四不变量**是否真的验过**（未接入恢复层时为 false）。
    pub recovery_verified: bool,
}

impl InvariantResult {
    /// 四条全成立**且**第四不变量确实验过。
    ///
    /// 「没验」不算通过：把未验证当已验证通过，是本条最不能犯的错。
    pub const fn all_hold(&self) -> bool {
        self.no_panic
            && self.no_hang
            && self.positioned
            && self.recovery_terminates
            && self.recovery_verified
    }

    /// 被破坏的不变量名（逐条点名，不静默）。
    pub fn broken(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        if !self.no_panic {
            v.push("不崩溃");
        }
        if !self.no_hang {
            v.push("不挂起");
        }
        if !self.positioned {
            v.push("错误有位置");
        }
        if !self.recovery_terminates {
            v.push("恢复能终止");
        }
        v
    }

    /// 第四不变量是否处于「未验证」状态（记账用，不是不变量破坏）。
    pub const fn recovery_unverified(&self) -> bool {
        !self.recovery_verified
    }
}

/// 步数预算默认档（超预算即判挂起）。
pub const DEFAULT_STEP_BUDGET: u64 = 4096;

/// 恢复动作预算（恢复层的步数上限，与 F0417 的上界语义一致）。
pub const RECOVERY_BUDGET: u64 = 64;

/// 检查一条语料的四不变量。
///
/// `recovered_steps` 是恢复层实际产生的动作数——**由调用方实测传入**而非本域
/// 重算（重算等于拿本域的模型去验本域的模型，验不到真实恢复层）。
/// `recovery_verified = false` 时第四不变量按**未验证**处理：不计入通过，
/// 也不谎称成立。
pub fn check_invariants(
    lex: &impl LexUnderTest,
    input: &[u8],
    budget: u64,
    recovered_steps: u64,
    recovery_verified: bool,
) -> InvariantResult {
    let out = lex.scan(input, budget);
    InvariantResult {
        no_panic: out.panic_free,
        no_hang: !out.truncated && out.steps <= budget,
        positioned: out
            .diags
            .iter()
            .all(|d| d.offset <= input.len() && d.line >= 1),
        recovery_terminates: recovered_steps <= RECOVERY_BUDGET,
        recovery_verified,
    }
}

// ---------------------------------------------------------------------------
// 四、发现账（判据三：🔴 即时修）
// ---------------------------------------------------------------------------

/// 发现项严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 🔴 即时修：崩溃 / 挂起 / 不变量破坏。
    Critical,
    /// 🟡 记账不阻断：未验证 / 语料质量类问题。
    Minor,
}

impl Severity {
    /// 人话标签（含 emoji，与锚点表述一致）。
    pub const fn label(self) -> &'static str {
        match self {
            Severity::Critical => "🔴 即时修",
            Severity::Minor => "🟡 记账",
        }
    }
}

/// 一条发现项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// 严重度。
    pub severity: Severity,
    /// 被破坏的不变量（或缺陷摘要）。
    pub reason: &'static str,
    /// 复现键。
    pub key: ReplayKey,
    /// 触发时的输入（**保留下来**以便复现——报了个复现不了的 bug 等于没报）。
    pub input: Vec<u8>,
}

/// 发现账：如实记账，不丢项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindingLog {
    findings: Vec<Finding>,
    fixed: Vec<ReplayKey>,
}

impl FindingLog {
    /// 新建空账。
    pub fn new() -> FindingLog {
        FindingLog {
            findings: Vec::new(),
            fixed: Vec::new(),
        }
    }

    /// 记一条发现项。
    pub fn record(&mut self, f: Finding) {
        self.findings.push(f);
    }

    /// 发现总数。
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// 取全部发现。
    pub fn findings(&self) -> &[Finding] {
        self.findings.as_slice()
    }

    /// 标记某条发现已修（按复现键）。修完即从未修账里移走并计入已修数。
    pub fn mark_fixed(&mut self, key: ReplayKey) -> bool {
        let idx = self.findings.iter().position(|f| f.key == key);
        match idx {
            Some(i) => {
                self.fixed.push(self.findings[i].key);
                self.findings.remove(i);
                true
            }
            None => false,
        }
    }

    /// **未修的 🔴 数量**（>0 即门禁阻断——这是「不允许已知 🔴 未修」的执行点）。
    pub fn open_critical(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.findings.len() {
            if self.findings[i].severity == Severity::Critical {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 未修的 🔴 发现（逐条列出，不只给个数——作者要知道修哪个）。
    pub fn open_critical_items(&self) -> Vec<&Finding> {
        let mut v: Vec<&Finding> = Vec::new();
        let mut i = 0usize;
        while i < self.findings.len() {
            if self.findings[i].severity == Severity::Critical {
                v.push(&self.findings[i]);
            }
            i += 1;
        }
        v
    }

    /// 已修数量。
    pub fn fixed_count(&self) -> usize {
        self.fixed.len()
    }

    /// 人话呈现（缺陷流程入口用的清单）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("发现账：共 ");
        s.push_str(self.findings.len().to_string().as_str());
        s.push_str(" 项未修（🔴 ");
        s.push_str(self.open_critical().to_string().as_str());
        s.push_str(" / 已修 ");
        s.push_str(self.fixed.len().to_string().as_str());
        s.push_str("）\n");
        let mut i = 0usize;
        while i < self.findings.len() {
            let f = &self.findings[i];
            s.push_str("  [");
            s.push_str(f.severity.label());
            s.push_str("] ");
            s.push_str(f.reason);
            s.push_str(" · 层 ");
            s.push_str(f.key.layer.label());
            s.push_str(" 种子 ");
            s.push_str(f.key.seed.to_string().as_str());
            s.push('/');
            s.push_str(f.key.index.to_string().as_str());
            s.push('\n');
            i += 1;
        }
        s
    }
}

impl Default for FindingLog {
    fn default() -> FindingLog {
        FindingLog::new()
    }
}

// ---------------------------------------------------------------------------
// 五、fuzz 批次与回放（判据四）
// ---------------------------------------------------------------------------

/// 一批 fuzz 的结果。
#[derive(Clone, Debug)]
pub struct FuzzBatch {
    /// 实际处理的条目数。
    pub processed: usize,
    /// 通过四不变量的条目数。
    pub passed: usize,
    /// 发现账（本批共用）。
    pub log: FindingLog,
    /// 各层产出计数（用于抓「配比退化为单层」）。
    pub per_layer: [usize; 3],
}

impl FuzzBatch {
    /// 本批是否有 🔴 未修（门禁阻断点）。
    pub fn has_open_critical(&self) -> bool {
        self.log.open_critical() > 0
    }

    /// 语料是否覆盖三层（缺层即退化）。
    pub fn covers_all_layers(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if self.per_layer[i] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        format!(
            "fuzz 批次：处理 {} 条，通过 {} 条；分层 随机 {} / 变异 {} / 语法 {}\n{}",
            self.processed,
            self.passed,
            self.per_layer[0],
            self.per_layer[1],
            self.per_layer[2],
            self.log.render()
        )
    }
}

/// 跑一批 fuzz。
///
/// `recovery_steps` 由调用方提供（实测恢复层动作数）；`None` 表示未接入恢复层，
/// 此时第四不变量**按未验证记账**：记 🟡、**且不计入通过**——把「没验」记成
/// 「验过且没问题」是本条最不能犯的错。
#[allow(clippy::too_many_arguments)]
pub fn run_fuzz_batch(
    lex: &impl LexUnderTest,
    seed: u64,
    count: usize,
    mutated_per_10: u64,
    grammar_per_10: u64,
    budget: u64,
    recovery_steps: Option<u64>,
) -> FuzzBatch {
    let corpus = generate_corpus(seed, count, mutated_per_10, grammar_per_10);
    let mut log = FindingLog::new();
    let mut per_layer = [0usize; 3];
    let mut passed = 0usize;

    let mut i = 0usize;
    while i < corpus.len() {
        let e = &corpus[i];
        per_layer[layer_index(e.layer)] += 1;

        // 未接入恢复层 → 第四不变量按「未验证」处理：记 🟡 且不算通过。
        let (steps, verified) = match recovery_steps {
            Some(v) => (v, true),
            None => (RECOVERY_BUDGET, false),
        };

        let r = check_invariants(lex, &e.bytes, budget, steps, verified);
        if r.all_hold() {
            passed += 1;
        } else if r.recovery_unverified() && r.broken().is_empty() {
            // 只差「没验」→ 🟡 记账，不阻断。
            log.record(Finding {
                severity: Severity::Minor,
                reason: "恢复层未接入，第四不变量未验证",
                key: e.replay_key(),
                input: e.bytes.clone(),
            });
        } else {
            // 不变量被破坏 → 🔴 即时修，并保留输入供复现。
            log.record(Finding {
                severity: Severity::Critical,
                reason: reason_of(&r.broken()),
                key: e.replay_key(),
                input: e.bytes.clone(),
            });
        }
        i += 1;
    }

    FuzzBatch {
        processed: corpus.len(),
        passed,
        log,
        per_layer,
    }
}

/// 取第一条被破坏的不变量名作为发现项 reason。
///
/// 只取第一条：一条输入的一次失败在账里就该是一条记录，逐条列会让同一次
/// 失败在账里重复出现多次，反而看不出「有几处不同的失败」。
fn reason_of(broken: &[&'static str]) -> &'static str {
    let mut i = 0usize;
    while i < broken.len() {
        if broken[i] == "不崩溃" {
            return "不崩溃";
        }
        if broken[i] == "不挂起" {
            return "不挂起";
        }
        if broken[i] == "错误有位置" {
            return "错误有位置";
        }
        if broken[i] == "恢复能终止" {
            return "恢复能终止";
        }
        i += 1;
    }
    "不变量破坏"
}

/// 层 → 数组下标。
fn layer_index(l: CorpusLayer) -> usize {
    match l {
        CorpusLayer::Random => 0,
        CorpusLayer::Mutated => 1,
        CorpusLayer::GrammarAware => 2,
    }
}

/// 从复现键重建字节流（判据四：逐位一致即可复现）。
pub fn replay(key: ReplayKey) -> Vec<u8> {
    build_layer(key.seed, key.layer, key.index as usize)
}

/// 门禁裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzGate {
    /// 放行。
    Allow,
    /// 阻断：有未修 🔴（带上条数，作者要知道欠几笔）。
    Block {
        /// 未修 🔴 条数。
        open: usize,
    },
    /// 语料退化：三层没齐，等于 fuzz 白跑。
    DegenerateCorpus,
    /// 没输入（一条都没跑）。
    NoInput,
}

impl FuzzGate {
    /// 是否阻断。
    pub const fn blocked(self) -> bool {
        !matches!(self, FuzzGate::Allow)
    }
}

/// 门禁裁定。
pub fn gate_of(batch: &FuzzBatch) -> FuzzGate {
    if batch.processed == 0 {
        return FuzzGate::NoInput;
    }
    if !batch.covers_all_layers() {
        return FuzzGate::DegenerateCorpus;
    }
    let open = batch.log.open_critical();
    if open > 0 {
        return FuzzGate::Block { open };
    }
    FuzzGate::Allow
}
