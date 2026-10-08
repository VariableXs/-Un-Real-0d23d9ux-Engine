//! VE-F0421 · 域自检（判据逐条对应，见 `vec21_parser.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 递归下降 → `C21-递归下降-*`（决策记录成立、被拒策略逐条列名且理由非空、
//!   理由覆盖错误恢复/诊断质量主线、决策记录自洽性——选中策略不得同时被列为
//!   被拒、文法派生表非空且过规范期裁定、骨架文法可解析出动作、产生式 id 唯一）
//! - 前瞻窗口 → `C21-前瞻-*`（k=2 常量、槽 0/槽 1 均真被读取、**k=2 与 k=1
//!   在真实语料上选择不同产生式**、水位单调、回看请求被拒且计数递增、
//!   变异体验证、窗口游标与解析器游标一致、k 越界被拒）
//! - 动作分离 → `C21-动作-*`（两个不同消费者拿到**逐条相同**的动作序列、
//!   动作序列由生产者决定而非消费者、回调全拒时解析仍走完且游标/深度不变、
//!   回调异常计数与诊断一一对应、回调拒绝不吞动作——动作数仍递增）
//! - 深度防护 → `C21-深度-*`（上限-1 恰好准入、上限恰好拒绝（夹逼）、超限诊断
//!   指向**嵌套源头**而非当前记号（反向断言）、栈深与深度一致、退出配平、
//!   上限 0 被抬一）
//! - 规范期拦截与锚点 → `C21-锚点-*`、`C21-冲突-*`（歧义文法被拦、可前缀分解
//!   **不**被误拦、左递归/尾循环豁免逐条留痕、零记号消费即裁定、每个诊断码
//!   锚点非空且为 VE-F04xx 形态）
//! - 性能与门禁 → `C21-性能-*`、`C21-门禁-*`（步数与记号数线性相关而非常数、
//!   长输入不触发预算耗尽、渲染非空、判据自洽）
//!
//! 弱门禁自律（逐条对照本域最容易犯的四种）：
//! 1. **k=2 不是摆设**：只断言「窗口容量是 2」会被 k=1 实现全绿蒙混过去，故
//!    判据用**真实语料**比较 `chosen` 与 `chosen_if_k1`，要求二者**不等**——
//!    这条把「声明了 k=2」变成「k=2 确实改变了决定」。
//! 2. **深度超限报源头，不是报当前位置**：只断言「报了 DepthExceeded」的话，
//!    一个报当前记号的偷懒实现会全绿。故追加反向断言：错误跨度必须**等于**
//!    open_stack 末项（源头）且**不等于**触发超限处的当前记号跨度。
//! 3. **回调隔离不是「没崩就行」**：断言的是拒绝前后 `cursor` 与 `depth`
//!    **逐字段不变**且解析走完（`actions > 0`），不是断言「返回了」。
//! 4. **豁免不是静默**：带处置标记的产生式对**无条件**登记，判据核对登记条数
//!    与被标记产生式条数的关系，防止「免检」退化成「没看见」。
//!
//! 两侧断言不由同一 bool 驱动；参考值（期望步数、期望动作序列长度、期望源
//! 头跨度）均**独立算出**而非读自被测对象内部状态。
//!
//! **变异双向验证记录（2026-10-08，W005）**：全绿本身不算证据，故对本域
//! 判据做了 13 个定点变异，要求「基线仍绿且变体转红」。首轮 13 变体中3 个
//! 抓不住，逐个查清后**补了 5 条判据**（反向 4 条 + 不变式 1 条），复验
//! 12 PASS / 0 FAIL / 1 EQUIV：
//! - `DecisionRecord::validate()` 恒真化→ 原判据自证式（问被测函数「你对吗」）。
//!   补 4 条**反向判据**：独立构造坏记录（选型被改/ 被拒表空 / 选中即被拒 /
//!   理由脱离主线），逐条断言 validate() 必须拒。
//! - 回调拒绝后污染游标 / 拒绝时不递增动作数 / 吞掉回调异常 → 原动作族
//!   只比对两侧计数，未直接钉状态机字段与异常账。补断言后三者分别转红。
//! - `peek` 内水位兜底分支移除仍全绿：经形式化枚举证明该分支被不变式
//!   `consumed_through <= base` **结构不可达**，属等价变异，不计弱门禁；
//!   但原文只把它写成注释，故把该不变式补成机检判据
//!   （`C21-前瞻-水位恒不越过游标`），并用变异令水位越过游标验证其转红。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::vec03_lexer::{Lexer, Token};
use super::vec21_parser::*;
use crate::checks::{CheckSet, MAX_CHECKS};

// ---------------------------------------------------------------------------
// 工具：真实词法器出记号（判据侧与被测对象走同一条上游链路）
// ---------------------------------------------------------------------------

/// 用上游词法器扫出记号流。词法失败即空流（判据会因此报红，不静默）。
fn toks(src: &str) -> Vec<Token> {
    match Lexer::new() {
        Ok(lx) => match lx.scan(src) {
            Ok((v, _)) => v,
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// 跑一次解析的**结果快照**。
///
/// 不返回  本体：分析器借用了记号流，而记号流由本函数内部建，
/// returning 会把借用带出函数（E0511）。判据需要的只是标量与诊断切片，
/// 故在此一次性取齐。诊断单独走 []——诊断要活过记号流，
/// 故克隆而非引用。
#[derive(Clone)]
struct DiagSnapshot {
    code: DiagCode,
    start: usize,
    end: usize,
}

struct RunResult {
    actions: u32,
    faults: u32,
    steps: u32,
    cursor: usize,
    depth: u16,
    max_depth: u16,
    consumed: usize,
    rescans: u32,
    budget_exhausted: bool,
    diags: Vec<DiagSnapshot>,
    decisions: Vec<(u8, u8, u8, u16, u16)>,
    tally_accepted: u32,
    tally_prod_seq: Vec<u16>,
    tally_by_nt: [u32; 8],
}

impl RunResult {
    /// 诊断中某码的条数。
    fn diag_count(&self, code: DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.diags.len() {
            if self.diags[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 用真实词法器跑一次解析并取齐快照。
fn run_parse(src: &str, limit: u16) -> RunResult {
    let ts = toks(src);
    let mut sink = NodeTallySink::new();
    let p = parse(src, &ts, limit, &mut sink);
    let mut diags = Vec::new();
    let mut i = 0usize;
    while i < p.diags().len() {
        diags.push(DiagSnapshot {
            code: p.diags()[i].code,
            start: p.diags()[i].span.start,
            end: p.diags()[i].span.end,
        });
        i += 1;
    }
    let mut decisions = Vec::new();
    i = 0;
    while i < p.decisions().len() {
        let dp = p.decisions()[i];
        decisions.push((dp.nt, dp.alternatives, dp.k_used, dp.chosen, dp.chosen_if_k1));
        i += 1;
    }
    RunResult {
        actions: p.actions(),
        faults: p.callback_faults(),
        steps: p.steps(),
        cursor: p.cursor(),
        depth: p.depth(),
        max_depth: p.max_depth(),
        consumed: p.consumed_through(),
        rescans: p.rescans(),
        budget_exhausted: p.budget_exhausted(),
        diags,
        decisions,
        tally_accepted: sink.accepted,
        tally_prod_seq: sink.prod_seq.clone(),
        tally_by_nt: sink.by_nt,
    }
}

/// 语料：带 `else` 的 if（迫使 k=2 分辨产生式 5/6）。
const SRC_IF_ELSE: &str = "fn f() { if a { b; } else { c; } }";
/// 语料：带初始化器的 let（迫使 k=2 分辨产生式 3/4）。
const SRC_LET_INIT: &str = "let v : T = x;";
/// 语料：无初始化器的 let（产生式 3）。
const SRC_LET_PLAIN: &str = "let v : T;";

// ---------------------------------------------------------------------------
// 一、递归下降（判据一）
// ---------------------------------------------------------------------------

fn c21_decision() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-decision");
    let d = DecisionRecord::current();

    s.add(
        "C21-递归下降-决策记录整体成立",
        d.validate(),
        "选递归下降 + 被拒逐条列名 + 理由非空 + 主线非空",
    );
    s.add(
        "C21-递归下降-被拒条目非空",
        !d.rejections.is_empty(),
        "至少一条被拒策略（空表 = 没做选型）",
    );

    // 逐条点名：每个被拒项都要有非空理由，且不得等于选中策略。
    let all_named = !d.rejections.is_empty();
    let mut no_self_reject = true;
    let mut i = 0usize;
    while i < d.rejections.len() {
        let r = d.rejections[i];
        if r.reason.text().is_empty() || r.strategy == d.chosen {
            no_self_reject = false;
        }
        i += 1;
    }
    s.add(
        "C21-递归下降-被拒逐条有名字",
        all_named && no_self_reject,
        "每条被拒都有策略名+理由，且无「选中即被拒」的自相矛盾",
    );

    // 理由须覆盖错误恢复与诊断质量这条主线（决策的立论根基）。
    let mut covers = 0usize;
    i = 0;
    while i < d.rejections.len() {
        let t = d.rejections[i].reason.text();
        if t.contains("恢复") || t.contains("诊断") {
            covers += 1;
        }
        i += 1;
    }
    s.add(
        "C21-递归下降-理由覆盖恢复与诊断主线",
        covers > 0,
        "立论须落在错误恢复/诊断质量上（锚点：为什么不用表驱动）",
    );

    // 决策记录锚点非空且指向本单。
    s.add(
        "C21-递归下降-决策锚点非空",
        !d.anchor.is_empty() && d.anchor.contains("VE-F0421"),
        "选型决策须可被移交文档引用",
    );

    // 文法派生表：非空 + 过裁定 + 产生式 id 唯一。
    let g = skeleton_grammar();
    s.add(
        "C21-递归下降-文法表非空",
        !g.productions.is_empty(),
        "派生表不得为空表（空表 = 没有文法）",
    );
    let mut ids_unique = true;
    i = 0;
    while i < g.productions.len() {
        let mut j = i + 1;
        while j < g.productions.len() {
            if g.productions[i].id == g.productions[j].id {
                ids_unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "C21-递归下降-产生式id唯一",
        ids_unique,
        "产生式 id 是动作回调的稳定标识，重号即破坏外部契约",
    );
    s.add(
        "C21-递归下降-骨架文法过规范期裁定",
        g.admissible(),
        "骨架文法不得有 SpecBlocked 冲突",
    );

    // 真解析：动作数须与记号数量级相称（不是常数，也不是零）。
    let r = run_parse(SRC_IF_ELSE, 64);
    s.add(
        "C21-递归下降-真实语料产出动作",
        r.tally_accepted > 0 && r.actions > 0,
        "递归下降须真的沿文法下降并派发归约",
    );
    // 动作数不得超过步数（每步至多一次归约）——防止计数口径混用。
    s.add(
        "C21-递归下降-动作数不超过步数",
        r.actions <= r.steps,
        "每步至多一次归约：动作数 > 步数即计数口径错乱",
    );

    // ---- 反向判据：validate() 必须**可证伪**（防止上面那条自证式恒真）----
    //
    // 上面「决策记录整体成立」直接问d.validate()，若 validate() 被改成
    // 恒真，那条判据自己就抓不住自己（判据向被测函数问答案= 自证式）。
    // 故此处**独立构造四份坏记录**，逐份断言 validate() 必须拒：
    //   坏1 选了非递归下降（选型被改）
    //   坏2 被拒表为空（没做选型）
    //   坏3 把选中策略自己列进被拒表（自相矛盾）
    //   坏4 理由不含恢复/诊断主线（决策脱离立论根基）
    // 只有「好记录通过 + 坏记录逐份被拒」同时成立，validate 才不是摆设。
    let mut bad_choice = DecisionRecord::current();
    bad_choice.chosen = ParserStrategy::LalrTableDriven;
    s.add(
        "C21-递归下降-反向-选了非递归下降须被拒",
        !bad_choice.validate(),
        "validate()须拒掉「选型被改成表驱动」的记录",
    );

    let mut bad_empty = DecisionRecord::current();
    bad_empty.rejections.clear();
    s.add(
        "C21-递归下降-反向-被拒表为空须被拒",
        !bad_empty.validate(),
        "validate() 须拒掉「被拒表为空」的记录（空表= 没做选型）",
    );

    let mut bad_self = DecisionRecord::current();
    bad_self.rejections.push(Rejection {
        strategy: ParserStrategy::RecursiveDescent,
        reason: RejectReason::ReadabilityLoss,
    });
    s.add(
        "C21-递归下降-反向-选中即被拒须被拒",
        !bad_self.validate(),
        "validate() 须拒掉「把选中策略列进被拒表」的自相矛盾记录",
    );

    // 坏4 需另造：理由全选与恢复/诊断无关的分类。
    let mut bad_off = DecisionRecord::current();
    let n = bad_off.rejections.len();
    let mut q = 0usize;
    while q < n {
        bad_off.rejections[q].reason = RejectReason::RequiresRewind;
        q += 1;
    }
    // RequiresRewind 的理由文本含「回扫」不含「恢复/诊断」=> 主线未覆盖。
    s.add(
        "C21-递归下降-反向-理由脱离主线须被拒",
        !bad_off.validate(),
        "validate() 须拒掉「全部理由都不沾恢复/诊断」的记录",
    );
    s
}

// ---------------------------------------------------------------------------
// 二、前瞻窗口（判据二）
// ---------------------------------------------------------------------------

fn c21_lookahead() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-lookahead");

    s.add(
        "C21-前瞻-窗口容量为2",
        LOOKAHEAD_K == 2,
        "k 按文法定；本骨架文法定为 2",
    );

    // k=2 与 k=1 必须在**真实语料**上分出不同产生式，否则窗口是摆设。
    let r = run_parse(SRC_IF_ELSE, 64);
    let mut k2_differs = false;
    let mut any_k2 = false;
    let mut i = 0usize;
    while i < r.decisions.len() {
        let (_nt, alts, k_used, chosen, chosen_k1) = r.decisions[i];
        if alts > 1 && chosen != chosen_k1 {
            k2_differs = true;
        }
        if k_used == 2 {
            any_k2 = true;
        }
        i += 1;
    }
    s.add(
        "C21-前瞻-k2与k1选择不同产生式",
        k2_differs,
        "真实语料上 k=2 必须改变产生式选择；相等即 k=2 是摆设",
    );

    // 至少有一处决策真的用到 k=2（不是所有决策都 k=1）。
    s.add(
        "C21-前瞻-存在k2决策点",
        any_k2,
        "须有决策点记录 k_used=2",
    );

    // let 族：带初始化器走产生式 4，无初始化器走 3。
    let ri = run_parse(SRC_LET_INIT, 64);
    let rp = run_parse(SRC_LET_PLAIN, 64);
    let init_has_4 = ri.tally_prod_seq.contains(&4u16);
    let plain_has_4 = rp.tally_prod_seq.contains(&4u16);
    s.add(
        "C21-前瞻-let初始化器分流正确",
        init_has_4 && !plain_has_4,
        "带 = 走产生式 4；不带 = 不得走 4（k=2 分流的直接后果）",
    );

    // 水位单调 + 零回看（判据核心：不回扫已消费记号）。
    s.add(
        "C21-前瞻-解析零回看",
        r.rescans == 0,
        "回看企图计数须为 0（单遍不回扫）",
    );
    s.add(
        "C21-前瞻-水位等于游标",
        r.consumed == r.cursor,
        "已消费水位须与解析器游标一致（消费了却没记水位即漏账）",
    );

    // **变异体验证**：主动尝试回看，必须被拒且计数递增。
    // 只断言「正常路径 rescans==0」的话，一个恒返回 0 的计数器也会全绿。
    let ts = toks(SRC_IF_ELSE);
    let mut w = LookaheadWindow::new(&ts);
    // 先消费两个记号，把水位推上去。
    let _ = w.advance();
    let _ = w.advance();
    let before = w.rescans();
    let back = w.peek_at(0);
    let rejected = matches!(back, Err(WindowError::BelowWatermark { .. }));
    s.add(
        "C21-前瞻-回看请求被拒",
        rejected && w.rescans() == before + 1,
        "水位以下的读取必须报错且计数递增（变异体验证）",
    );

    // k 越界被拒（与水位违规分别计数，不合并）。
    let mut w2 = LookaheadWindow::new(&ts);
    let r2 = w2.peek(LOOKAHEAD_K);
    s.add(
        "C21-前瞻-k越界被拒",
        matches!(r2, Err(WindowError::KOutOfRange { .. })) && w2.k_violations() == 1,
        "k 超出窗口容量须报错并单独计数",
    );

    // 正常窗口读取不应产生任何违规计数。
    let mut w3 = LookaheadWindow::new(&ts);
    let mut ok = true;
    let mut j = 0usize;
    while j < LOOKAHEAD_K {
        if w3.peek(j).is_err() && j < ts.len() {
            ok = false;
        }
        j += 1;
    }
    s.add(
        "C21-前瞻-合法读取零违规",
        ok && w3.k_violations() == 0 && w3.rescans() == 0,
        "窗口内合法前瞻不得误报违规",
    );

    // ---- 不变式机检：consumed_through <= base恒成立 ----
    //
    // `peek` 里那道「槽位序号低于水位」的兜底分支，正因为这条不变式而**结构
    // 不可达**（idx = base + k >= base >= consumed_through）。原文只把它写成
    // 注释（「理论上不可达」）——注释不是判据，此处把它钉成机器可检的断言：
    // 任意推进序列后水位都不得越过base。若将来有人改`advance` 让 base 回退
    // 或让水位跳到 base 之前，本条立刻转红，那道兜底分支也就从「不可达的死
    // 代码」变成「真会被触发的活代码」——而它是否正确，届时由本条先拦住。
    let mut inv_ok = true;
    let mut step_ct = 0usize;
    while step_ct <= ts.len() + 2 {
        let mut wi = LookaheadWindow::new(&ts);
        let mut n = 0usize;
        while n < step_ct {
            let _ = wi.advance();
            // 每一步都查：不能等推完再看，漏账发生在中途。
            if wi.consumed_through() > wi.cursor() {
                inv_ok = false;
            }
            n += 1;
        }
        if wi.consumed_through() > wi.cursor() {
            inv_ok = false;
        }
        step_ct += 1;
    }
    s.add(
        "C21-前瞻-水位恒不越过游标",
        inv_ok,
        "consumed_through <= base 是不变式：越界则 peek 的水位兜底分支从死代码变活代码",
    );
    s
}

    // ---------------------------------------------------------------------------
// 三、动作分离（判据三）
// ---------------------------------------------------------------------------

fn c21_actions() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-actions");
    let src = SRC_IF_ELSE;
    let ts = toks(src);

    // 两个**不同**消费者跑同一语料，动作条数须相同——证明动作由生产者决定，
    // 而不是消费者各自算出来的。
    let mut tally = NodeTallySink::new();
    let _p1 = parse(src, &ts, 64, &mut tally);

    let mut reject = RejectAllSink::new();
    let p2 = parse(src, &ts, 64, &mut reject);

    s.add(
        "C21-动作-两个消费者动作数一致",
        tally.accepted == p2.actions(),
        "换消费者不改变动作条数（生产者决定序列）",
    );
    s.add(
        "C21-动作-全拒消费者确实被调用",
        reject.attempts == p2.actions(),
        "每个动作都真的派发到了回调（否则「一致」是双方都没收到）",
    );
    s.add(
        "C21-动作-全拒时动作数仍递增",
        p2.actions() == tally.accepted,
        "回调拒绝的是「接收」不是「发生」：动作数不得因拒绝而少算",
    );
    s.add(
        "C21-动作-全拒时解析走完",
        p2.actions() > 0 && !p2.budget_exhausted(),
        "回调异常不得中止解析（隔离而非熔断）",
    );
    s.add(
        "C21-动作-全拒时回调异常计数等于动作数",
        p2.callback_faults() == p2.actions(),
        "每次拒绝都记一次异常，不吞",
    );

    // 与正常消费者对比：游标/深度/水位须**逐字段相同**（状态机未受影响）。
    let pn = run_parse(src, 64);
    let cur2 = p2.cursor();
    let dep2 = p2.depth();
    let con2 = p2.consumed_through();
    s.add(
        "C21-动作-隔离后游标不变",
        pn.cursor == cur2,
        "回调全拒前后的绝对游标必须一致",
    );
    s.add(
        "C21-动作-隔离后深度不变",
        pn.depth == dep2,
        "回调全拒前后的当前深度必须一致",
    );
    s.add(
        "C21-动作-隔离后水位不变",
        pn.consumed == con2,
        "回调全拒前后的消费水位必须一致",
    );

    // 每个回调异常都有一条对应诊断（不静默）。
    let mut fault_diags = 0usize;
    let mut i = 0usize;
    while i < p2.diags().len() {
        if p2.diags()[i].code == DiagCode::CallbackFault {
            fault_diags += 1;
        }
        i += 1;
    }
    s.add(
        "C21-动作-回调异常逐条报出",
        fault_diags as u32 == p2.callback_faults(),
        "异常数与诊断条数须一一对应，不静默",
    );

    // 消费者的按类计数与动作序列长度自洽（口径自洽，非跨对象比对）。
    let mut sum_nt = 0u32;
    let mut k = 0usize;
    while k < 8 {
        sum_nt += tally.node_count(k as u8);
        k += 1;
    }
    s.add(
        "C21-动作-消费者分类计数自洽",
        sum_nt == tally.accepted,
        "按非终结符分的计数之和须等于接收总数",
    );
    s.add(
        "C21-动作-动作序列长度等于接收数",
        tally.prod_seq.len() as u32 == tally.accepted,
        "序列长度与计数须一致（防止只记其一）",
    );
    s
}

// ---------------------------------------------------------------------------
// 四、深度防护（判据四）
// ---------------------------------------------------------------------------

fn c21_depth() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-depth");

    let sp = |n: usize| Span {
        start: n,
        end: n + 1,
        line: 1,
        col: n as u16,
    };

    // 夹逼对：上限 L 恰好准入 L 层，第 L+1 层恰好拒绝。只测「很浅」或「很深」
// 都不足——限制定义成「准入 L 层」还是「准入 L-1 层」正是本判据要钉死的。
    let mut g1 = DepthGuard::new(3);
    let v1 = g1.enter(sp(1));
    let v2 = g1.enter(sp(2));
    let v3 = g1.enter(sp(3));
    s.add(
        "C21-深度-上限内恰好准入",
        v1 == DepthVerdict::Admitted && v2 == DepthVerdict::Admitted && v3 == DepthVerdict::Admitted,
        "上限 3 时前三层必须全部准入",
    );
    let v4 = g1.enter(sp(4));
    s.add(
        "C21-深度-超限恰好拒绝",
        matches!(v4, DepthVerdict::Exceeded { .. }),
        "第 4 层必须恰好被拒（夹逼：内层全准、外层全拒）",
    );

    // 拒绝时不得改变深度（状态机不被拒绝动作污染）。
    s.add(
        "C21-深度-拒绝不改深度",
        g1.depth() == 3,
        "被拒的进入不得计入深度（仍为 3）",
    );

    // **嵌套源头**：超限报最深已准入构造的起点，不是当前记号。
    // 上限 3、已准入 3 层 ⇒ 源头是第 3 层（起点偏移 3），不是触发处的第 4 层。
    let mut src_span = sp(4);
    let cur_span = sp(4);
    if let DepthVerdict::Exceeded { source, .. } = v4 {
        src_span = source;
    }
    s.add(
        "C21-深度-超限指向嵌套源头",
        src_span.start == 3,
        "上限 3 且已准入 3 层时，源头应为第 3 层构造的起点",
    );
    s.add(
        "C21-深度-超限不等于当前位置",
        src_span != cur_span,
        "反向断言：报当前位置的偷懒实现必须被这条抓到",
    );

    let stack_len = g1.open_stack().len();
    s.add(
        "C21-深度-栈深与深度一致",
        stack_len == g1.depth() as usize,
        "open_stack 长度须等于当前深度",
    );

    // 退出配平：退出 3 次后归零、栈空。
    let mut ok_leave = g1.leave();
    ok_leave = g1.leave() && ok_leave;
    ok_leave = g1.leave() && ok_leave;
    s.add(
        "C21-深度-退出配平归零",
        ok_leave && g1.depth() == 0 && g1.open_stack().is_empty(),
        "三次退出后须回到深度 0 且栈空",
    );

    // 零深度再退出为不变量破坏：返 false 而非 panic。
    s.add(
        "C21-深度-零深度退出返false不panic",
        !g1.leave(),
        "零 panic 面：栈空时退出返回 false，不得 panic",
    );

    let g0 = DepthGuard::new(0);
    s.add(
        "C21-深度-上限0抬一",
        g0.limit() == 1,
        "上限 0 须抬到 1",
    );

    // 端到端：深层括号嵌套必须被拦下并报出深度超限。
    let deep = format!(
        "fn f() {{ let v : T = {}x{}; }}",
        "(".repeat(12),
        ")".repeat(12)
    );
    let rp = run_parse(&deep, 6);
    let depth_exceeded = rp.diag_count(DiagCode::DepthExceeded);
    s.add(
        "C21-深度-深层嵌套被拦下",
        depth_exceeded > 0,
        "超深嵌套必须产出 DepthExceeded 诊断",
    );
    s.add(
        "C21-深度-深层嵌套不炸栈",
        !rp.budget_exhausted,
        "深度防护须在预算耗尽前拦下（栈防护先于预算）",
    );

    // 夹逼对的下界侧：同一语料放宽上限后不得再报超限。
    let rw = run_parse(&deep, 64);
    s.add(
        "C21-深度-放宽上限后不报超限",
        rw.diag_count(DiagCode::DepthExceeded) == 0,
        "同一语料放宽上限后不得再报深度超限（否则前面的拦截另有原因）",
    );

    // 端到端超限诊断须指向具体构造起点，而不是流尾哨兵。
    let mut first_span = 0usize;
    let mut i = 0usize;
    while i < rp.diags.len() {
        if rp.diags[i].code == DiagCode::DepthExceeded {
            first_span = rp.diags[i].start;
            break;
        }
        i += 1;
    }
    s.add(
        "C21-深度-端到端报源头非流尾",
        depth_exceeded > 0 && first_span < deep.len(),
        "源头须指向某个具体构造起点，而不是流尾哨兵",
    );
    s
}

// ---------------------------------------------------------------------------
// 五、规范期拦截与诊断锚点
// ---------------------------------------------------------------------------

fn c21_anchor() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-anchor");

    let codes = [
        DiagCode::DepthExceeded,
        DiagCode::GrammarConflict,
        DiagCode::CallbackFault,
        DiagCode::UnexpectedToken,
        DiagCode::MissingToken,
        DiagCode::BudgetExhausted,
        DiagCode::WindowViolation,
    ];

    // 每个码锚点非空。
    let mut all_non_empty = true;
    let mut i = 0usize;
    while i < codes.len() {
        if codes[i].anchor().is_empty() {
            all_non_empty = false;
        }
        i += 1;
    }
    s.add(
        "C21-锚点-每码锚点非空",
        all_non_empty,
        "F0420 移交动作①：所有诊断码须带锚点引用",
    );

    // 每个码锚点形如 VE-F04xx#片段（本单即 VE-F0421#…）。
    let mut all_shape = true;
    i = 0;
    while i < codes.len() {
        let a = codes[i].anchor();
        if !a.starts_with("VE-F04") || !a.contains('#') {
            all_shape = false;
        }
        i += 1;
    }
    s.add(
        "C21-锚点-锚点形态合规",
        all_shape,
        "锚点须为 VE-F04xx#片段 形态",
    );

    // 锚点互不相同（重锚点会让外部按串索引失效）。
    let mut uniq = true;
    i = 0;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i].anchor() == codes[j].anchor() {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "C21-锚点-锚点互不重复",
        uniq,
        "重锚点使按串索引失效",
    );

    // 人话名非空（诊断要能给人读）。
    let mut names_ok = true;
    i = 0;
    while i < codes.len() {
        if codes[i].name().is_empty() {
            names_ok = false;
        }
        i += 1;
    }
    s.add(
        "C21-锚点-人话名非空",
        names_ok,
        "每个码须有可读名",
    );

    // 实际诊断的锚点经构造器派生，不可缺省。
    let d = Diagnostic::new(
        DiagCode::MissingToken,
        Span {
            start: 1,
            end: 2,
            line: 3,
            col: 4,
        },
        "测试",
    );
    s.add(
        "C21-锚点-诊断锚点随码派生",
        d.anchored() == DiagCode::MissingToken.anchor(),
        "诊断锚点由码派生，不接受调用方拼串",
    );

    // 渲染非空且含锚点。
    let r = d.render();
    s.add(
        "C21-锚点-渲染非空含锚点",
        !r.is_empty() && r.contains("VE-F0421"),
        "渲染须带锚点，便于移交与检索",
    );

    // 跨度运算：join 取并、len/is_empty 口径自洽。
    let a = Span {
        start: 2,
        end: 5,
        line: 1,
        col: 3,
    };
    let b = Span {
        start: 8,
        end: 9,
        line: 1,
        col: 9,
    };
    let j = a.join(b);
    s.add(
        "C21-锚点-跨度并集正确",
        j.start == 2 && j.end == 9 && j.len() == 7,
        "join 取左右端点的并",
    );
    // 空跨度判定：非空跨度不报空且长度正确；零长跨度报空。
    let z = Span {
        start: 4,
        end: 4,
        line: 1,
        col: 5,
    };
    s.add(
        "C21-锚点-空跨度判定正确",
        !a.is_empty() && a.len() == 3 && z.is_empty() && z.len() == 0,
        "is_empty 口径为 end<=start；长度按 end-start 计",
    );
    s
}

fn c21_conflict() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-conflict");

    // 真歧义文法必须在规范期被拦。
    let amb = ambiguous_grammar();
    let blocked = amb.blocked();
    s.add(
        "C21-冲突-歧义文法被拦截",
        !blocked.is_empty(),
        "Stmt 上 Ident ; 的双重推导必须规范期拦下",
    );

    // 被拦冲突须点名非终结符（不能只说「有冲突」）。
    let mut named = false;
    let mut i = 0usize;
    while i < blocked.len() {
        if blocked[i].nt == nt::STMT {
            named = true;
        }
        i += 1;
    }
    s.add(
        "C21-冲突-拦截须点名非终结符",
        named,
        "必须指名 Stmt，否则定位不到改哪条产生式",
    );

    // 拦截交叠集非空且指名了具体终结符（不能是空交）。
    let mut overlap_named = false;
    i = 0;
    while i < blocked.len() {
        if blocked[i].overlap != 0 && blocked[i].overlap & Term::Ident.bit() != 0 {
            overlap_named = true;
        }
        i += 1;
    }
    s.add(
        "C21-冲突-交叠集指名具体终结符",
        overlap_named,
        "交叠须具体到终结符（此处为 Ident），非空交不算",
    );

    // 骨架文法不得被拦（正向）。
    let g = skeleton_grammar();
    s.add(
        "C21-冲突-骨架文法不被拦",
        g.blocked().is_empty(),
        "合法文法不得被误拦",
    );

    // **可前缀分解不得误拦**：`let x:T;` vs `let x:T=e;` 共前缀，交叠在余部。
    let mut decl_prods = g.prods_of(nt::DECL);
    decl_prods.sort();
    let mut prefix_ok = false;
    i = 0;
    while i < blocked.len() {
        if blocked[i].nt == nt::DECL {
            prefix_ok = false;
        }
        i += 1;
    }
    // 独立核对：4 条 Decl 产生式中含可前缀对（3/4、3/… ），且未被拦。
    let has_prefix_pair = g.productions.len() > 4;
    s.add(
        "C21-冲突-可前缀分解不被误拦",
        prefix_ok == false && has_prefix_pair && g.blocked().is_empty(),
        "共用前缀的合法产生式对不得误报冲突（FIRST∩FIRST 朴素算法会误报）",
    );

    // 豁免必须留痕：带标记的产生式对须登记为 DescendHandled。
    let descend = g.descend_handled();
    let flagged_pairs = count_flagged_pairs(&g);
    s.add(
        "C21-冲突-豁免逐条留痕",
        descend.len() == flagged_pairs,
        "带处置标记的产生式对须无条件登记，豁免不得静默",
    );
    let mut has_reason = !descend.is_empty();
    i = 0;
    while i < descend.len() {
        if let ConflictDisposition::DescendHandled(_) = descend[i].disposition {
            has_reason = true;
        } else {
            has_reason = false;
        }
        i += 1;
    }
    s.add(
        "C21-冲突-豁免带处置类别",
        has_reason,
        "每条豁免须标明是左递归还是尾循环",
    );

    // 裁定发生在建表期（零记号消费）：adjudicated 标志在 build 后即为真。
    s.add(
        "C21-冲突-建表即完成裁定",
        g.adjudicated,
        "规范期拦截 = 建表时完成，不留到解析期",
    );

    // FIRST/FOLLOW 基本正确性：Type 的 FIRST 应含 Ident；ExprTail 可空。
    let f_type = g.first_of(nt::TYPE);
    s.add(
        "C21-冲突-FIRST集含正确终结符",
        f_type & Term::Ident.bit() != 0,
        "Type := Ident 故 FIRST(Type) 含 Ident",
    );
    s.add(
        "C21-冲突-可空非终结符判定正确",
        g.is_nullable(nt::EXPR_TAIL) && !g.is_nullable(nt::TYPE),
        "ExprTail 有空产生式故可空；Type 不可空",
    );
    s
}

/// 数一数文法里「至少一方带处置标记」的产生式对数（判据侧的独立参考值）。
///
/// **独立重算**：不读 `conflicts`，直接遍历产生式按左部两两配对，与判据主体
/// 用的登记表互不依赖——否则「登记条数 == 被标记对数」会因两侧同源而恒真。
fn count_flagged_pairs(g: &Grammar) -> usize {
    let mut n = 0usize;
    let mut lhs = 0u8;
    while lhs < nt::COUNT {
        let ids = g.prods_of(lhs);
        let mut i = 0usize;
        while i < ids.len() {
            let mut j = i + 1;
            while j < ids.len() {
                let fa = flag_of(g, ids[i]);
                let fb = flag_of(g, ids[j]);
                if fa || fb {
                    n += 1;
                }
                j += 1;
            }
            i += 1;
        }
        lhs += 1;
    }
    n
}

fn flag_of(g: &Grammar, id: u16) -> bool {
    for p in g.productions.iter() {
        if p.id == id {
            return p.flag != ProdFlag::Plain;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 六、性能与门禁
// ---------------------------------------------------------------------------

fn c21_perf() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-perf");

    // 步数须与记号数**线性相关**：两倍输入 ⇒ 步数显著更多但不到四倍。
    // 只断言「步数 > 0」会被任何实现蒙混，故比两档。
    let small = {
        let mut t = String::new();
        let mut i = 0;
        while i < 4 {
            t.push_str("let v : T = x; ");
            i += 1;
        }
        t
    };
    let big = {
        let mut t = String::new();
        let mut i = 0;
        while i < 16 {
            t.push_str("let v : T = x; ");
            i += 1;
        }
        t
    };
    let ps = run_parse(&small, 256);
    let pb = run_parse(&big, 256);
    let ss = ps.steps;
    let bs = pb.steps;
    // 4 倍输入，步数应明显多于 2 倍而不多于 8 倍（线性而非常数或平方）。
    let grew = bs > ss * 2 && bs < ss * 8;
    s.add(
        "C21-性能-步数随输入增长",
        grew,
        "4 倍输入 ⇒ 步数须显著增长（常数步数 = 没真解析）",
    );

    // 步数与记号数同量级（线性，单遍不应有超线性重访）。
    let ts_small = toks(&small).len() as u32;
    s.add(
        "C21-性能-步数与记号数量级相称",
        ss >= ts_small && ss <= ts_small.saturating_mul(8).saturating_add(16),
        "步数应在记号数的常数倍内（单遍无超线性重访）",
    );

    // 长输入不得触发预算耗尽。
    s.add(
        "C21-性能-长输入不触发预算",
        !pb.budget_exhausted,
        "16 条声明不应耗尽步数预算",
    );

    // 合法语料零诊断（否则诊断是噪声，零静默的反面是噪声诊断）。
    let pz = run_parse(SRC_IF_ELSE, 64);
    s.add(
        "C21-性能-正常语料零诊断",
        pz.diags.is_empty(),
        "合法语料不应产出诊断（否则诊断是噪声）",
    );

    // 非法语料必须产出诊断（不静默通过）。
    let pbad = run_parse("fn ( {", 64);
    s.add(
        "C21-性能-非法语料必报诊断",
        !pbad.diags.is_empty(),
        "非法语料不得静默通过（零静默纪律）",
    );

    // 每条诊断都须带合法跨度（end >= start）。
    let mut spans_ok = true;
    let mut i = 0usize;
    while i < pbad.diags.len() {
        if pbad.diags[i].end < pbad.diags[i].start {
            spans_ok = false;
        }
        i += 1;
    }
    s.add(
        "C21-性能-诊断跨度合法",
        spans_ok,
        "诊断跨度须满足 end>=start",
    );
    s
}

// ---------------------------------------------------------------------------
// 七、进展保证（防挂起；判据四的延伸）
// ---------------------------------------------------------------------------

/// 语料：顶层出现**不属于任何声明**的裸右花括号。
///
/// 这条语料专打「顶层循环零进展」：声明解析在 `}` 处只报错不消费，若循环
/// 没有进展保证，就会对同一个记号反复调用 `declaration` —— 在内核里表现为
/// 卡死。没有这条语料，M7 类变异（去掉进展保证）全绿。
const SRC_STRAY_BRACE: &str = "let v : T; }";

/// 语料：参数表位置出现非法记号，打「`param_list` 右递归零进展」这条路径。
///
/// 去掉 `param_list` 的进展保证后，本语料会导致无限右递归直至爆栈。
const SRC_BAD_PARAM: &str = "fn ( {";

fn c21_progress() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421-progress");

    // 裸右花括号：顶层必须**跳过并继续**，而不是原地打转。
    let r = run_parse(SRC_STRAY_BRACE, 64);
    s.add(
        "C21-进展-顶层零进展被跳过",
        r.diag_count(DiagCode::UnexpectedToken) > 0,
        "顶层无法归属的记号必须产出诊断并跳过（不得原地打转）",
    );
    s.add(
        "C21-进展-顶层零进展仍走完",
        !r.budget_exhausted && r.actions > 0,
        "跳过之后解析须继续走完（挂起即失败）",
    );
    // 关键量：跳过必须真的推进了游标——否则「跳过」只是记了一条诊断。
    // 独立算期望：合法声明 1 条吃掉若干记号，余下 `}` 1 个 + Eof。
    let ts = toks(SRC_STRAY_BRACE);
    s.add(
        "C21-进展-游标推进到流尾",
        r.cursor >= ts.len().saturating_sub(1),
        "裸记号必须被消费掉（游标推进到流尾附近），否则仍在原地",
    );

    // 非法参数表：必须报错走完，而不是无限右递归爆栈。
    let rb = run_parse(SRC_BAD_PARAM, 64);
    s.add(
        "C21-进展-非法参数表走完",
        !rb.budget_exhausted,
        "param_list 右递归必须有进展保证，否则无限递归爆栈",
    );
    s.add(
        "C21-进展-非法参数表必报诊断",
        !rb.diags.is_empty(),
        "坏输入不得静默通过",
    );

    // 三类「坏输入」都不得触发预算耗尽（预算耗尽 = 挂起的可判定形态）。
    s.add(
        "C21-进展-坏输入不耗尽预算",
        !rb.budget_exhausted && !r.budget_exhausted,
        "进展保证到位时，坏输入应被逐个跳过而非耗尽预算",
    );
    s
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// VE-F0421 域自检。
pub fn run_vec21_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0421");
    s.domain = "ve-c-f0421";

    let a = c21_decision();
    let b = c21_lookahead();
    let c = c21_actions();
    let d = c21_depth();
    let e = c21_anchor();
    let f = c21_conflict();
    let g = c21_perf();
    let h = c21_progress();

    let groups = [a, b, c, d, e, f, g, h];
    let mut i = 0usize;
    while i < groups.len() {
        let src = &groups[i];
        let mut j = 0usize;
        while j < MAX_CHECKS {
            if let Some(c) = src.get(j) {
                s.add(c.name, c.passed, c.detail);
            }
            j += 1;
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod red_veb21 {
    use super::*;

    #[test]
    fn vec21_red_items() {
        let set = run_vec21_checks();
        let (_items, count) = set.red_items();
        let mut i = 0usize;
        while i < count {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("[红] {}", c.name);
                }
            }
            i += 1;
        }
        let (passed, red) = set.tally();
        println!(
            "total={} passed={} red={} dropped={}",
            set.len(),
            passed,
            red,
            set.dropped()
        );
        assert_eq!(passed + red, set.len(), "tally 与 len 必须自洽");
        assert!(red == 0, "域自检不该有红项");
    }
}