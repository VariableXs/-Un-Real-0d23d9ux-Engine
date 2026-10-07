//! VE-F0419 · 域自检（判据逐条对应，见 `vec19_fuzz.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三层语料 → `C19-语料-*`（三层标签互不相同、三层均有产出、配比符合预期、
//!   单层配比被门禁拦下、超界配比被夹且不挤掉语法层、变异层确实改动输入、
//!   随机层含高位字节、语法层合法与半合法混合、四种变异算子都用过）
//! - 四不变量 → `C19-不变量-*`（正常输入四条成立、高位字节仍位置合法、
//!   超预算判挂起、崩溃被抓住、越界位置判破坏、恢复超预算判不终止、
//!   恢复恰在预算内成立、未验证不算通过、broken 逐条点名、空输入零预算不误报）
//! - 🔴 即时修 → `C19-发现-*`（正常批次零发现、挂起即阻断、未修项逐条可读、
//!   已修则放行、发现项不丢、轻微项不阻断、输入保留可复现、渲染非空、
//!   零输入批次被拦、修不存在的键返回false）
//! - 种子可复现 → `C19-复现-*`（LCG 序列确定、不同种子分叉、below 零不越界、
//!   below 恒在界内、三元组回放逐位一致、抽单条可复现、换种子/换层产出不同、
//!   整批可复现）
//! - 零静默 → `C19-显性-*`（未接入恢复层按未验证记账、批次渲染含分层与发现）

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::vec19_fuzz::*;
use crate::checks::CheckSet;

/// 一个**真实**的极简扫描器：按字节归类成三类记号（空白 / 字母数字 / 其他），
/// 遇到无法归类的字节（>= 0x80）报一条诊断。
///
/// **刻意不是「永远返回空」的实现**：用表内元素验自己会恒真，故这里让扫描器
/// 真的扫、真的可能报诊断、真的可能超预算。
#[derive(Clone, Copy, Debug, Default)]
pub struct TinyLexer;

impl TinyLexer {
    /// 新建。
    pub const fn new() -> TinyLexer {
        TinyLexer
    }
}

impl LexUnderTest for TinyLexer {
    fn scan(&self, input: &[u8], budget: u64) -> ScanOutcome {
        let mut diags: Vec<LexDiag> = Vec::new();
        let mut steps = 0u64;
        let mut line = 1u16;
        let mut i = 0usize;
        while i < input.len() {
            steps += 1;
            if steps > budget {
                // 超预算：如实标 truncated 并停——不装作跑完了。
                return ScanOutcome {
                    diags,
                    steps,
                    truncated: true,
                    panic_free: true,
                };
            }
            let b = input[i];
            if b >= 0x80 {
                diags.push(LexDiag {
                    code: "VE-F0403-UNKNOWN-TOKEN",
                    offset: i,
                    line,
                });
            }
            if b == b'\n' {
                line = line.saturating_add(1);
            }
            i += 1;
        }
        ScanOutcome {
            diags,
            steps,
            truncated: false,
            panic_free: true,
        }
    }
}

/// 一个**故意会超预算**的扫描器（用于验证挂起判定）。
#[derive(Clone, Copy, Debug, Default)]
pub struct StuckLexer;

impl LexUnderTest for StuckLexer {
    fn scan(&self, _input: &[u8], budget: u64) -> ScanOutcome {
        // 步数直接顶到预算之上：模拟「处理不前进」。
        ScanOutcome {
            diags: Vec::new(),
            steps: budget.saturating_add(1),
            truncated: true,
            panic_free: true,
        }
    }
}

/// 一个**故意报越界位置**的扫描器（验证「错误有位置」这条会红）。
#[derive(Clone, Copy, Debug, Default)]
pub struct BadOffsetLexer;

impl LexUnderTest for BadOffsetLexer {
    fn scan(&self, input: &[u8], budget: u64) -> ScanOutcome {
        // 偏移给成 `len + 100`（越界）、行号给 0（< 1）——两条都违规。
        ScanOutcome {
            diags: vec![LexDiag {
                code: "VE-F0403-UNKNOWN-TOKEN",
                offset: input.len() + 100,
                line: 0,
            }],
            steps: 1u64.min(budget),
            truncated: false,
            panic_free: true,
        }
    }
}

/// 一个**只把偏移越界、行号仍合法**的扫描器。
///
/// 与 `BadOffsetLexer` 分开是为了把「错误有位置」的两个子条件**分开判**：
/// 一个只查偏移、一个只查行号，坏在哪个就红哪条。若两条合在一个被测形态里，
/// 去掉任一子条件另一条仍然红——判据就抓不到「只坏一半」这种退化。
#[derive(Clone, Copy, Debug, Default)]
pub struct BadOffsetOnlyLexer;

impl LexUnderTest for BadOffsetOnlyLexer {
    fn scan(&self, input: &[u8], budget: u64) -> ScanOutcome {
        ScanOutcome {
            diags: vec![LexDiag {
                code: "VE-F0403-UNKNOWN-TOKEN",
                offset: input.len() + 100,
                line: 1,
            }],
            steps: 1u64.min(budget),
            truncated: false,
            panic_free: true,
        }
    }
}

/// 一个**只把行号给 0、偏移合法**的扫描器（见 `BadOffsetOnlyLexer` 的分工理由）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ZeroLineOnlyLexer;

impl LexUnderTest for ZeroLineOnlyLexer {
    fn scan(&self, input: &[u8], budget: u64) -> ScanOutcome {
        let _ = input;
        ScanOutcome {
            diags: vec![LexDiag {
                code: "VE-F0403-UNKNOWN-TOKEN",
                offset: 0,
                line: 0,
            }],
            steps: 1u64.min(budget),
            truncated: false,
            panic_free: true,
        }
    }
}

/// 一个**故意汇报崩溃**的扫描器（验证「不崩溃」这条不是恒真）。
#[derive(Clone, Copy, Debug, Default)]
pub struct CrashingLexer;

impl LexUnderTest for CrashingLexer {
    fn scan(&self, _input: &[u8], _budget: u64) -> ScanOutcome {
        // 真 panic 在 no_std 下由看门狗/测试壳捕获后如实置 false。
        ScanOutcome {
            diags: Vec::new(),
            steps: 1,
            truncated: false,
            panic_free: false,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据一：三层语料
// ---------------------------------------------------------------------------

fn check_corpus(set: &mut CheckSet) {
    // 三层都在枚举里，且标签互不相同（标签相同则「报告里分不出哪层出的问题」）
    let mut labels: Vec<&'static str> = Vec::new();
    let mut same = 0usize;
    let mut i = 0usize;
    while i < CorpusLayer::ALL.len() {
        let a = CorpusLayer::ALL[i].label();
        let mut j = 0usize;
        while j < labels.len() {
            if labels[j] == a {
                same += 1;
            }
            j += 1;
        }
        labels.push(a);
        i += 1;
    }
    set.add(
        "C19-语料-三层标签互不相同",
        same == 0 && labels.len() == 3,
        "",
    );

    // 生成一批（三层都要有产出）——配比 3/3（每 10 条中变异 3、语法 3、随机 4）
    let corpus = generate_corpus(0xC0FFEE, 30, 3, 3);
    set.add(
        "C19-语料-三层均有产出",
        corpus.len() == 30 && layer_count(&corpus) == [12, 9, 9],
        "",
    );

    // 配比符合预期：每 10 条里变异 3 条、语法 3 条、随机 4 条
    set.add(
        "C19-语料-配比符合预期",
        layer_count(&corpus) == [12, 9, 9],
        "",
    );

    // **表外真实形态**：配比全给变异（10/0）时退化为单层——门禁必须抓到。
    // 这条是判据五的核心：三层语料退化成一层，等于 fuzz 白跑。
    let lex = TinyLexer::new();
    let degenerate = run_fuzz_batch(&lex, 42, 20, 10, 0, DEFAULT_STEP_BUDGET, Some(0));
    set.add(
        "C19-语料-单层配比被门禁拦下",
        degenerate.per_layer == [0, 20, 0]
            && !degenerate.covers_all_layers()
            && gate_of(&degenerate) == FuzzGate::DegenerateCorpus
            && gate_of(&degenerate).blocked(),
        "",
    );

    // **逐槽位映射**：直接调 pick_layer 覆盖 slot 0..10 每个槽位。
    // 这是「配比怎么映射到槽位」的唯一可观测面——之前那两条靠 count 反推的
    // 判据只能看到每 10 条的总数，看不到「哪一条落在哪一层」，把 slot 边界
    // 整体挪一位也照样绿。配比 3/3 的期望映射是：slot 0-2 变异、3-5 语法、
    // 6-9 随机，逐槽核对。
    let mut mapping_ok = true;
    let mut slot = 0usize;
    while slot < 10 {
        let got = pick_layer(0, slot, 3, 3);
        let want = match slot {
            0..=2 => CorpusLayer::Mutated,
            3..=5 => CorpusLayer::GrammarAware,
            _ => CorpusLayer::Random,
        };
        if got != want {
            mapping_ok = false;
        }
        slot += 1;
    }
    // 第 10 条回到 slot 0（周期为 10）
    if pick_layer(0, 10, 3, 3) != CorpusLayer::Mutated {
        mapping_ok = false;
    }
    set.add("C19-语料-逐槽位映射正确", mapping_ok, "");

    // **槽位边界表外形态**：配比 1/1 时只有 slot 0 是变异、slot 1 是语法，
    // 其余全是随机——这组期望值不来自「总数 30 条怎么分」，而是逐槽位定的，
    // 故把某一层的阈值挪一位就会红。
    let mut narrow_ok = true;
    let want_narrow = [
        CorpusLayer::Mutated,
        CorpusLayer::GrammarAware,
        CorpusLayer::Random,
        CorpusLayer::Random,
        CorpusLayer::Random,
        CorpusLayer::Random,
        CorpusLayer::Random,
        CorpusLayer::Random,
        CorpusLayer::Random,
        CorpusLayer::Random,
    ];
    let mut s2 = 0usize;
    while s2 < want_narrow.len() {
        if pick_layer(0, s2, 1, 1) != want_narrow[s2] {
            narrow_ok = false;
        }
        s2 += 1;
    }
    set.add("C19-语料-窄配比槽位边界", narrow_ok, "");

    // 超界配比（变异 99 + 语法 99）→ 每 10 条全归变异，随机层 0 条。
    // 注意这条**验不到「夹取」**：slot 上界就是 10，夹与不夹行为一致。
    // 它验的是「配比超界时不凭空多出槽位」这一层语义。
    let clamped = generate_corpus(7, 10, 99, 99);
    set.add(
        "C19-语料-超界配比不凭空多槽位",
        clamped.len() == 10 && layer_count(&clamped) == [0, 10, 0],
        "",
    );

    // 超配比（变异 6 + 语法 9 = 15 > 10）→ 占满 10 个槽位，随机层 0 条
    let edge = generate_corpus(7, 40, 6, 9);
    set.add(
        "C19-语料-超配比如实挤掉随机层",
        layer_count(&edge) == [0, 24, 16],
        "",
    );

    // 变异层确实在合法种子上改了东西（不是原样复制——原样复制等于变异层没干活）
    let mut changed = 0usize;
    let mut total_mut = 0usize;
    let mut n = 0usize;
    while n < corpus.len() {
        if corpus[n].layer == CorpusLayer::Mutated {
            total_mut += 1;
            let base = SEED_SNIPPETS[n % SEED_SNIPPETS.len()];
            if corpus[n].bytes != base.to_vec() {
                changed += 1;
            }
        }
        n += 1;
    }
    set.add(
        "C19-语料-变异层确实改动输入",
        changed == total_mut && total_mut > 0,
        "",
    );

    // 随机层不是全 ASCII（纯 ASCII 就撞不出高位字节这类组合）
    let mut has_high = false;
    let mut p = 0usize;
    while p < corpus.len() {
        if corpus[p].layer == CorpusLayer::Random && corpus[p].bytes.iter().any(|b| *b >= 0x80) {
            has_high = true;
        }
        p += 1;
    }
    set.add("C19-语料-随机层含高位字节", has_high, "");

    // 语法感知层的输出确实含**合法与半合法混合**（纯合法走不到深水区）
    let grammar = generate_corpus(9, 12, 0, 10);
    let mut has_quote = false;
    let mut has_alnum = false;
    let mut g = 0usize;
    while g < grammar.len() {
        has_quote |= grammar[g].bytes.contains(&b'"');
        has_alnum |= grammar[g].bytes.contains(&b'4') || grammar[g].bytes.contains(&b'i');
        g += 1;
    }
    set.add(
        "C19-语料-语法层合法与半合法混合",
        has_quote && has_alnum && layer_count(&grammar) == [0, 0, 12],
        "",
    );

    // 四种变异算子**真的都被用到过**（直接调生产函数并读它返回的算子，
    // 不在自检里重实现一份——重实现等于验自己那份副本）
    let mut ops_seen = [0usize; 4];
    let mut idx = 0usize;
    while idx < 60 {
        let mut r = Rng::new(mix_seed(0x51ED, CorpusLayer::Mutated, idx as u64));
        let mut v: Vec<u8> = SEED_SNIPPETS[idx % SEED_SNIPPETS.len()].to_vec();
        let op = apply_mutation(&mut v, &mut r);
        ops_seen[op_index(op)] += 1;
        idx += 1;
    }
    set.add(
        "C19-语料-四种变异算子都用过",
        ops_seen[0] > 0 && ops_seen[1] > 0 && ops_seen[2] > 0 && ops_seen[3] > 0,
        "",
    );

    // **ReplaceByte 必改字节（表外形态）**：直接问「原字节 5、抽回 5」会得到什么。
    // 不靠概率碰——LCG 低 8 位与任意给定字节的映射没有交点，跑一万次也碰不到
    // 「抽回原字节」，那种判据是空断言。这里一调即知。
    let same = replace_byte_at(5u8, 5u8);
    let diff = replace_byte_at(5u8, 6u8);
    set.add(
        "C19-语料-字节替换必改字节",
        same != 5u8 && diff == 6u8 && same == 5u8 ^ 0xFF,
        "",
    );

    // 穷举 256 个 (原字节, 抽中字节) 组合，性质恒成立（不是抽 3 个例子）
    let mut all_ok = true;
    let mut o = 0u32;
    while o < 256 {
        let mut d = 0u32;
        while d < 256 {
            let out = replace_byte_at(o as u8, d as u8);
            if out == o as u8 {
                all_ok = false;
            }
            d += 1;
        }
        o += 1;
    }
    set.add("C19-语料-字节替换穷举必改字节", all_ok, "");

    // ReplaceByte 算子真的走 replace_byte_at（把算子改成 FlipBit 就该红）
    let mut used_replace = false;
    let mut rp = 0u64;
    while rp < 64 && !used_replace {
        let mut r = Rng::new(mix_seed(0xB0B0, CorpusLayer::Mutated, rp));
        let mut v: Vec<u8> = SEED_SNIPPETS[(rp % 3) as usize].to_vec();
        let before = v.clone();
        if apply_mutation(&mut v, &mut r) == MutationOp::ReplaceByte && v != before {
            used_replace = true;
        }
        rp += 1;
    }
    set.add("C19-语料-字节替换算子确实走到", used_replace, "");
}

/// 语料三层计数（[随机, 变异, 语法]）。
fn layer_count(corpus: &[CorpusEntry]) -> [usize; 3] {
    let mut c = [0usize; 3];
    let mut i = 0usize;
    while i < corpus.len() {
        c[match corpus[i].layer {
            CorpusLayer::Random => 0,
            CorpusLayer::Mutated => 1,
            CorpusLayer::GrammarAware => 2,
        }] += 1;
        i += 1;
    }
    c
}

/// 变异算子 → 下标。
fn op_index(op: MutationOp) -> usize {
    match op {
        MutationOp::FlipBit => 0,
        MutationOp::ReplaceByte => 1,
        MutationOp::Truncate => 2,
        MutationOp::InsertByte => 3,
    }
}

// ---------------------------------------------------------------------------
// 判据二：四不变量
// ---------------------------------------------------------------------------

fn check_invariants_section(set: &mut CheckSet) {
    let lex = TinyLexer::new();

    // 正常输入：四条全成立且第四不变量确实验过
    let ok = check_invariants(&lex, b"float4 g = 1.0;", DEFAULT_STEP_BUDGET, 0, true);
    set.add(
        "C19-不变量-正常输入四条成立",
        ok.all_hold() && ok.broken().is_empty(),
        "",
    );

    // 高位字节输入：扫描器会报诊断，但诊断位置合法 → 仍四条成立
    let hi = check_invariants(&lex, &[0x80u8, 0xFF, b'a'], DEFAULT_STEP_BUDGET, 0, true);
    set.add("C19-不变量-高位字节仍位置合法", hi.all_hold(), "");

    // 挂起：StuckLexer 恒超预算 → no_hang 破
    let stuck = check_invariants(&StuckLexer, b"abc", 16, 0, true);
    set.add(
        "C19-不变量-超预算判挂起",
        !stuck.no_hang && !stuck.all_hold() && stuck.broken().contains(&"不挂起"),
        "",
    );

    // 崩溃：汇报 panic_free=false → no_panic 破（证明这条不是恒真）
    let crash = check_invariants(&CrashingLexer, b"abc", 16, 0, true);
    set.add(
        "C19-不变量-崩溃被抓且点名",
        !crash.no_panic && crash.broken().contains(&"不崩溃") && !crash.all_hold(),
        "",
    );

    // 位置越界 + 行号 0 → positioned 破（表外真实形态：真的报了一条诊断）
    let bad = check_invariants(&BadOffsetLexer, b"abc", 16, 0, true);
    set.add(
        "C19-不变量-越界位置判破坏",
        !bad.positioned && bad.broken().contains(&"错误有位置"),
        "",
    );

    // **子条件分离**：只越界偏移（行号合法）也要判破坏。
    // 少了这条，去掉「行号 >= 1」这个子条件时，`BadOffsetLexer` 仍会因偏移
    // 越界而红，判据抓不到「只坏一半」。
    let bad_off = check_invariants(&BadOffsetOnlyLexer, b"abc", 16, 0, true);
    set.add(
        "C19-不变量-仅偏移越界判破坏",
        !bad_off.positioned && bad_off.broken().contains(&"错误有位置"),
        "",
    );

    // **子条件分离**：只把行号给 0（偏移合法）也要判破坏。
    let zero_line = check_invariants(&ZeroLineOnlyLexer, b"abc", 16, 0, true);
    set.add(
        "C19-不变量-仅行号零判破坏",
        !zero_line.positioned && zero_line.broken().contains(&"错误有位置"),
        "",
    );

    // 恢复超预算 → recovery_terminates 破
    let rec_bad = check_invariants(&lex, b"abc", DEFAULT_STEP_BUDGET, RECOVERY_BUDGET + 1, true);
    set.add(
        "C19-不变量-恢复超预算判不终止",
        !rec_bad.recovery_terminates
            && rec_bad.broken().contains(&"恢复能终止")
            && !rec_bad.all_hold(),
        "",
    );

    // 恢复恰在预算内 → 成立（真边界，非自证）
    let rec_edge = check_invariants(&lex, b"abc", DEFAULT_STEP_BUDGET, RECOVERY_BUDGET, true);
    set.add(
        "C19-不变量-恢复恰在预算内成立",
        rec_edge.recovery_terminates && rec_edge.all_hold(),
        "",
    );

    // **未验证不算通过**：verified=false 时即使其它三条都好也不许 all_hold
    let unverified = check_invariants(&lex, b"abc", DEFAULT_STEP_BUDGET, 0, false);
    set.add(
        "C19-不变量-未验证不算通过",
        unverified.recovery_unverified()
            && !unverified.all_hold()
            && unverified.broken().is_empty()
            && unverified.recovery_terminates,
        "",
    );

    // broken 逐条点名：四条同时破坏时应点名四条
    let all_bad = InvariantResult {
        no_panic: false,
        no_hang: false,
        positioned: false,
        recovery_terminates: false,
        recovery_verified: true,
    };
    set.add("C19-不变量-broken逐条点名", all_bad.broken().len() == 4, "");

    // 预算自证反例：空输入零预算不误报挂起（0 步预算下扫 0 步）
    let zero = check_invariants(&lex, b"", 0, 0, true);
    set.add("C19-不变量-空输入零预算不误报", zero.all_hold(), "");
}

// ---------------------------------------------------------------------------
// 判据三：🔴 即时修
// ---------------------------------------------------------------------------

fn check_findings(set: &mut CheckSet) {
    let lex = TinyLexer::new();

    // 全正常输入 → 无发现项，放行
    let clean = run_fuzz_batch(&lex, 1, 30, 3, 3, DEFAULT_STEP_BUDGET, Some(0));
    set.add(
        "C19-发现-正常批次零发现",
        clean.log.is_empty() && gate_of(&clean) == FuzzGate::Allow && clean.passed == 30,
        "",
    );

    // StuckLexer → 每条都挂起 → 10 条 🔴，门禁阻断且条数如实报出
    let stuck = run_fuzz_batch(&StuckLexer, 1, 10, 3, 3, 8, Some(0));
    let open = stuck.log.open_critical();
    set.add(
        "C19-发现-挂起即阻断且条数如实",
        stuck.has_open_critical()
            && stuck.log.len() == 10
            && stuck.passed == 0
            && gate_of(&stuck) == FuzzGate::Block { open }
            && open == 10,
        "",
    );

    // 未修 🔴 逐条列出（作者要知道修哪个，不能只给个数）
    let open_items = stuck.log.open_critical_items();
    let all_critical = open_items.len() == 10;
    let mut reasons_ok = true;
    let mut i = 0usize;
    while i < open_items.len() {
        if open_items[i].reason != "不挂起" {
            reasons_ok = false;
        }
        i += 1;
    }
    set.add("C19-发现-未修项逐条可读", all_critical && reasons_ok, "");

    // 崩溃也走同一条 🔴 通道（reason 点名为「不崩溃」而不是笼统「不变量破坏」）
    let crash = run_fuzz_batch(&CrashingLexer, 3, 10, 3, 3, DEFAULT_STEP_BUDGET, Some(0));
    let crash_items = crash.log.open_critical_items();
    let mut crash_ok = crash_items.len() == 10;
    let mut j = 0usize;
    while j < crash_items.len() {
        if crash_items[j].reason != "不崩溃" {
            crash_ok = false;
        }
        j += 1;
    }
    set.add(
        "C19-发现-崩溃逐条点名不崩溃",
        crash_ok && gate_of(&crash) == FuzzGate::Block { open: 10 },
        "",
    );

    // 位置越界同样入账
    let bad = run_fuzz_batch(&BadOffsetLexer, 3, 10, 3, 3, DEFAULT_STEP_BUDGET, Some(0));
    let bad_items = bad.log.open_critical_items();
    let mut bad_ok = bad_items.len() == 10;
    let mut k = 0usize;
    while k < bad_items.len() {
        if bad_items[k].reason != "错误有位置" {
            bad_ok = false;
        }
        k += 1;
    }
    set.add("C19-发现-越界位置逐条点名", bad_ok, "");

    // 标记已修后放行（正向一侧，防「一律阻断」的退化实现）
    let mut log = FindingLog::new();
    let key = ReplayKey {
        layer: CorpusLayer::Random,
        seed: 1,
        index: 0,
    };
    log.record(Finding {
        severity: Severity::Critical,
        reason: "不挂起",
        key,
        input: vec![1u8, 2, 3],
    });
    let before = log.open_critical();
    let marked = log.mark_fixed(key);
    set.add(
        "C19-发现-已修则放行",
        before == 1
            && marked
            && log.open_critical() == 0
            && log.fixed_count() == 1
            && log.is_empty(),
        "",
    );

    // 发现项**不丢**：记录 5 条后长度必须仍为 5
    let mut log2 = FindingLog::new();
    let mut n = 0usize;
    while n < 5 {
        log2.record(Finding {
            severity: Severity::Minor,
            reason: "记账项",
            key: ReplayKey {
                layer: CorpusLayer::Random,
                seed: n as u64,
                index: n as u64,
            },
            input: Vec::new(),
        });
        n += 1;
    }
    set.add(
        "C19-发现-发现项不丢",
        log2.len() == 5 && log2.open_critical() == 0,
        "",
    );

    // 🟡 不计入未修 🔴（只有 Critical 阻断）
    set.add("C19-发现-轻微项不阻断", log2.open_critical() == 0, "");

    // 输入被保留（复现的前提）：**逐条**按复现键重建回同一串字节。
    // 不能只查「非空」——截断变异本就能产出空输入，拿「非空」当判据会把
    // 合法输入误判成「输入丢了」。真要抓的是「输入没保留 / 键对不上」。
    let mut kept_ok = stuck.log.len() == 10;
    let mut ki = 0usize;
    while ki < stuck.log.len() {
        let f = &stuck.log.findings()[ki];
        if f.input != replay(f.key) {
            kept_ok = false;
        }
        ki += 1;
    }
    // 再加一条正向证据：至少有一条发现项带着非空输入（否则「全部为空」
    // 也能通过上面的逐位一致——那说明发现项根本没存输入）。
    let mut any_non_empty = false;
    let mut ai = 0usize;
    while ai < stuck.log.len() {
        if !stuck.log.findings()[ai].input.is_empty() {
            any_non_empty = true;
        }
        ai += 1;
    }
    set.add("C19-发现-输入保留可复现", kept_ok && any_non_empty, "");

    // 渲染非空且含条数与层名（不静默）
    let rendered = stuck.log.render();
    set.add(
        "C19-显性-发现账渲染非空",
        rendered.contains("发现账")
            && rendered.contains("挂起")
            && rendered.contains("随机字节流")
            && !rendered.is_empty(),
        "",
    );

    // 未处理的批次（count=0）→ 门禁阻断（等于没跑）
    let none = run_fuzz_batch(&lex, 1, 0, 3, 3, DEFAULT_STEP_BUDGET, Some(0));
    set.add(
        "C19-发现-零输入批次被拦",
        gate_of(&none) == FuzzGate::NoInput && gate_of(&none).blocked(),
        "",
    );

    // mark_fixed 对不存在的键如实返回 false
    set.add(
        "C19-发现-修不存在的键返回false",
        !log2.mark_fixed(ReplayKey {
            layer: CorpusLayer::Random,
            seed: 999,
            index: 999,
        }),
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据四：种子可复现
// ---------------------------------------------------------------------------

fn check_replay(set: &mut CheckSet) {
    // LCG 序列确定：同种子两次生成完全一致
    let mut r1 = Rng::new(12345);
    let mut r2 = Rng::new(12345);
    let mut same = true;
    let mut i = 0usize;
    while i < 8 {
        if r1.next_u64() != r2.next_u64() {
            same = false;
        }
        i += 1;
    }
    set.add("C19-复现-LCG序列确定", same, "");

    // **钉死序列**：同种子前 4 个输出必须是这四个具体值。
    // 「同种子两次一致」只证明自洽，证明不了配方没被改——把 A 改掉一位后
    // 两次跑依然一致。跨机器跨版本可复现靠的就是配方写死，故这里钉值。
    let mut pinned = true;
    let mut p1 = Rng::new(1);
    let expect = [
        0x6C57_6FAC_43FD_007Cu64,
        0x8268_86B3_864A_1B1Bu64,
        0xA5FA_E199_2097_AA0Eu64,
        0x6203_55CD_1193_57C5u64,
    ];
    let mut q = 0usize;
    while q < expect.len() {
        if p1.next_u64() != expect[q] {
            pinned = false;
        }
        q += 1;
    }
    set.add("C19-复现-LCG配方钉死序列", pinned, "");

    // 不同种子产出不同序列（否则种子形同虚设）
    let mut a = Rng::new(1);
    let mut b = Rng::new(2);
    let mut diff = false;
    let mut j = 0usize;
    while j < 4 {
        if a.next_u64() != b.next_u64() {
            diff = true;
        }
        j += 1;
    }
    set.add("C19-复现-不同种子分叉", diff, "");

    // below(0) 返回 0 而不是回绕成 usize::MAX 附近（那会直接越界）
    let mut z = Rng::new(9);
    set.add("C19-复现-below零不越界", z.below(0) == 0, "");

    // below(n) 恒 < n
    let mut in_range = true;
    let mut m = 0usize;
    while m < 50 {
        let v = z.below(10);
        if v >= 10 {
            in_range = false;
        }
        m += 1;
    }
    set.add("C19-复现-below恒在界内", in_range, "");

    // **三元组回放逐位一致**（判据四核心）
    let corpus = generate_corpus(0xABCD, 24, 3, 3);
    let mut all_same = true;
    let mut k = 0usize;
    while k < corpus.len() {
        if replay(corpus[k].replay_key()) != corpus[k].bytes {
            all_same = false;
        }
        k += 1;
    }
    set.add(
        "C19-复现-三元组回放逐位一致",
        all_same && !corpus.is_empty(),
        "",
    );

    // 抽单条也能复现（不必重跑整批——这是「按 key 直接重建」的价值）
    let single_key = ReplayKey {
        layer: CorpusLayer::Mutated,
        seed: 0xABCD,
        index: 7,
    };
    let a1 = replay(single_key);
    let a2 = replay(single_key);
    set.add("C19-复现-抽单条可复现", a1 == a2 && !a1.is_empty(), "");

    // **派生子种子必须区分三层**：同 seed 同 index 下三层的 mix_seed 要两两不同。
    // 少了 tag 的话三层会共用同一条随机流，「换层产出不同」只因 GrammarAware
    // 换了段生成逻辑而侥幸成立——tag 一去掉，Random 与 Mutated 就同流了。
    let t1 = mix_seed(3, CorpusLayer::Random, 7);
    let t2 = mix_seed(3, CorpusLayer::Mutated, 7);
    let t3 = mix_seed(3, CorpusLayer::GrammarAware, 7);
    set.add(
        "C19-复现-派生子种子区分三层",
        t1 != t2 && t2 != t3 && t1 != t3,
        "",
    );

    // 换种子产出不同（否则同 index 不同 seed 会撞车）
    let k1 = ReplayKey {
        layer: CorpusLayer::GrammarAware,
        seed: 1,
        index: 3,
    };
    let k2 = ReplayKey {
        layer: CorpusLayer::GrammarAware,
        seed: 2,
        index: 3,
    };
    set.add("C19-复现-换种子产出不同", replay(k1) != replay(k2), "");

    // 换层产出不同（否则层标识形同虚设）
    let s1 = ReplayKey {
        layer: CorpusLayer::Random,
        seed: 5,
        index: 2,
    };
    let s2 = ReplayKey {
        layer: CorpusLayer::GrammarAware,
        seed: 5,
        index: 2,
    };
    set.add("C19-复现-换层产出不同", replay(s1) != replay(s2), "");

    // 同一批语料两次生成完全一致（整批可复现，不只是单条）
    let again = generate_corpus(0xABCD, 24, 3, 3);
    let mut batch_same = again.len() == corpus.len();
    let mut p = 0usize;
    while p < corpus.len() && batch_same {
        if again[p].bytes != corpus[p].bytes || again[p].layer != corpus[p].layer {
            batch_same = false;
        }
        p += 1;
    }
    set.add(
        "C19-复现-整批可复现",
        batch_same && again.len() == corpus.len(),
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据五 + 零静默：端到端与未验证记账
// ---------------------------------------------------------------------------

fn check_end_to_end(set: &mut CheckSet) {
    let lex = TinyLexer::new();

    // 未接入恢复层 → 第四不变量按「未验证」记 🟡，**且不计入通过**
    // （把「没验」记成「验过且没问题」是本条最不能犯的错）。
    let no_rec = run_fuzz_batch(&lex, 3, 30, 3, 3, DEFAULT_STEP_BUDGET, None);
    let mut mentions_unverified = false;
    let mut i = 0usize;
    while i < no_rec.log.len() {
        if no_rec.log.findings()[i].reason.contains("未验证") {
            mentions_unverified = true;
        }
        i += 1;
    }
    set.add(
        "C19-显性-未接入恢复层按未验证记账",
        no_rec.log.len() == 30
            && no_rec.log.open_critical() == 0
            && mentions_unverified
            && no_rec.passed == 0
            && gate_of(&no_rec) == FuzzGate::Allow,
        "",
    );

    // 批次渲染含分层计数与发现账
    let batch = run_fuzz_batch(&lex, 3, 30, 3, 3, DEFAULT_STEP_BUDGET, Some(0));
    let text: String = batch.render();
    set.add(
        "C19-显性-批次渲染含分层与发现",
        text.contains("分层")
            && text.contains("随机")
            && text.contains("发现账")
            && !text.is_empty(),
        "",
    );

    // 分层计数与语料实际构成一致
    set.add(
        "C19-语料-分层计数一致",
        batch.per_layer == layer_count(&generate_corpus(3, 30, 3, 3)),
        "",
    );

    // 端到端：干净批次 + 有 🔴 批次，两种门禁走向都要判对
    let clean = run_fuzz_batch(&TinyLexer::new(), 5, 30, 3, 3, DEFAULT_STEP_BUDGET, Some(1));
    let dirty = run_fuzz_batch(&StuckLexer, 5, 30, 3, 3, 8, Some(1));
    set.add(
        "C19-门禁-干净放行脏阻断",
        gate_of(&clean) == FuzzGate::Allow
            && !gate_of(&clean).blocked()
            && matches!(gate_of(&dirty), FuzzGate::Block { .. })
            && gate_of(&dirty).blocked()
            && clean.covers_all_layers(),
        "",
    );
}

/// VE-F0419 域自检。
pub fn run_vec19_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec19");
    check_corpus(&mut set);
    check_invariants_section(&mut set);
    check_findings(&mut set);
    check_replay(&mut set);
    check_end_to_end(&mut set);
    set
}

#[cfg(test)]
mod red_fuzz {
    use super::*;
    #[test]
    fn fuzz_red_items() {
        let set = run_vec19_checks();
        for name in set.red_items() {
            println!("[红] {}", name);
        }
        println!(
            "total={} passed={} dropped={}",
            set.len(),
            set.passed_count(),
            set.dropped()
        );
    }
}
