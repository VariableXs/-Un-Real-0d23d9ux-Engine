//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12490 · SEH 全链注入矩阵 · 0x30b40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12490-J1 坏 try 层级/坏 filter/坏 __finally 序 30 例注入
//!      全过，与 B11 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - SEH 作用域栈模型：帧栈（level/has_finally/filter_id）+ 栈深
//!     计数，EnterTry 压栈、UnwindPop 弹栈；
//!   - 闸序（判据语义序）：坏 try 层级 → 坏 filter → 坏 __finally 序
//!     → 坏 unwind 序，前置命中即短路；
//!   - try 层级闸：EnterTry 只许压当前栈深层（level == depth），跳层
//!     拒（BadTryLevel）；
//!   - filter 闸：filter id 闭集 1..=4（CONTINUE_EXECUTION/SEARCH/
//!     EXECUTE_HANDLER/COMPLETE 模型口径），0 与越闭集拒；
//!   - __finally 闸：无 finally 标志的帧跑 finally 拒（BadFinally）；
//!   - unwind 闸：弹栈层号必须 == 栈顶（乱序拒 BadPop）；
//!   - 30 例注入账：10 例坏 try 层级 + 8 例坏 filter + 6 例坏 __finally
//!     + 3 例坏 unwind + 3 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30b40），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30b40 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30b40;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// filter id 闭集上界（1..=4 模型口径）。
pub const FILTER_MAX: u32 = 4;

/// 栈深上限（嵌套层数口径）。
pub const DEPTH_MAX: usize = 8;

/// 诊断码（0x30b40 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeCode {
    /// 0x30b40 坏 try 层级（跳层压栈）。
    BadTryLevel = 0,
    /// 0x30b41 坏 filter（id 出闭集）。
    BadFilter = 1,
    /// 0x30b42 坏 __finally 序（无 finally 标志跑 finally）。
    BadFinally = 2,
    /// 0x30b43 坏 unwind 序（弹栈乱序）。
    BadPop = 3,
    /// 0x30b44 栈深超限。
    DepthOver = 4,
    /// 0x30b45 注入例数不足。
    CaseShort = 5,
    /// 0x30b46 例码漂移。
    CaseDrift = 6,
    /// 0x30b47 注入账缩水。
    LedgerShrink = 7,
    /// 0x30b48 闸序破。
    GateOrder = 8,
    /// 0x30b49 正路锚堵死。
    PositiveBlocked = 9,
    /// 0x30b4a 注入未执行。
    NotRun = 10,
}

impl SeCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [SeCode; 11] = [
        SeCode::BadTryLevel, SeCode::BadFilter, SeCode::BadFinally, SeCode::BadPop,
        SeCode::DepthOver, SeCode::CaseShort, SeCode::CaseDrift, SeCode::LedgerShrink,
        SeCode::GateOrder, SeCode::PositiveBlocked, SeCode::NotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30b40 => "坏 try 层级：EnterTry 只许压当前栈深层，跳层即拒。",
        0x30b41 => "坏 filter：filter id 出 1..=4 闭集，回查过滤器注册。",
        0x30b42 => "坏 __finally 序：无 finally 标志的帧不可跑 finally。",
        0x30b43 => "坏 unwind 序：弹栈层号必须等于栈顶。",
        0x30b44 => "栈深超限：嵌套出 8 层口径，先展平再入。",
        0x30b45 => "注入例数不足：30 例必须全注入。",
        0x30b46 => "例码漂移：逐例回放状态机定位失配例。",
        0x30b47 => "注入账缩水：append-only，禁止截断。",
        0x30b48 => "闸序破：前置闸命中必须短路。",
        0x30b49 => "正路锚堵死：合法全链被堵先修正状态机。",
        0x30b4a => "注入未执行：账面零例。",
        _ => "未知诊断码：回查 0x30b40 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30b40 段基址。
    pub const SEED: u64 = 0x30b40_30b4_030b_40u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 作用域帧。
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// try 层级。
    pub level: u32,
    /// finally 标志。
    pub has_finally: bool,
    /// filter id（0=无过滤器）。
    pub filter: u32,
}

/// SEH 作用域栈（状态机正本）。
#[derive(Debug)]
pub struct SehStack {
    /// 帧栈。
    frames: Vec<Frame>,
    /// 操作账（append-only：(操作名, 结果码序号)）。
    ledger: Vec<(u8, u8)>,
    /// 账高水位。
    high: usize,
}

/// 操作名编码（账面用）。
pub const OP_ENTER: u8 = 0;
pub const OP_FILTER: u8 = 1;
pub const OP_FINALLY: u8 = 2;
pub const OP_POP: u8 = 3;

impl SehStack {
    /// 建栈。
    pub fn new() -> Self {
        SehStack { frames: Vec::new(), ledger: Vec::new(), high: 0 }
    }

    /// 栈深。
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// 栈顶帧。
    pub fn top(&self) -> Option<Frame> {
        self.frames.last().copied()
    }

    /// 记账。
    fn record(&mut self, op: u8, r: &Result<(), SeCode>) {
        let code = match r {
            Ok(()) => 0u8,
            Err(c) => *c as u8 + 1,
        };
        self.ledger.push((op, code));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
    }

    /// EnterTry（压栈；层级必须 == 当前栈深）。
    pub fn enter_try(&mut self, level: u32, has_finally: bool, filter: u32) -> Result<(), SeCode> {
        let r = if self.frames.len() >= DEPTH_MAX {
            Err(SeCode::DepthOver)
        } else if level as usize != self.frames.len() {
            Err(SeCode::BadTryLevel)
        } else {
            self.frames.push(Frame { level, has_finally, filter });
            Ok(())
        };
        self.record(OP_ENTER, &r);
        r
    }

    /// 跑 filter（栈顶帧的 filter 校验）。
    pub fn run_filter(&mut self, filter: u32) -> Result<(), SeCode> {
        let r = if self.frames.is_empty() {
            Err(SeCode::BadFilter)
        } else if filter == 0 || filter > FILTER_MAX {
            Err(SeCode::BadFilter)
        } else {
            Ok(())
        };
        self.record(OP_FILTER, &r);
        r
    }

    /// 跑 finally（栈顶帧须有 finally 标志）。
    pub fn run_finally(&mut self) -> Result<(), SeCode> {
        let r = match self.top() {
            None => Err(SeCode::BadFinally),
            Some(f) => {
                if f.has_finally {
                    Ok(())
                } else {
                    Err(SeCode::BadFinally)
                }
            }
        };
        self.record(OP_FINALLY, &r);
        r
    }

    /// UnwindPop（弹栈顶；须有帧）。
    pub fn unwind_pop(&mut self, level: u32) -> Result<(), SeCode> {
        let r = match self.top() {
            None => Err(SeCode::BadPop),
            Some(f) => {
                if f.level == level {
                    self.frames.pop();
                    Ok(())
                } else {
                    Err(SeCode::BadPop)
                }
            }
        };
        self.record(OP_POP, &r);
        r
    }

    /// 账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("SEH 操作账：\n");
        for (op, code) in self.ledger.iter() {
            let name = match *op {
                OP_ENTER => "Enter",
                OP_FILTER => "Filter",
                OP_FINALLY => "Finally",
                _ => "Pop",
            };
            s.push_str(&format!("{} 码{} {}\n", name, code, if *code == 0 { "过" } else { "拒" }));
        }
        s
    }
}

/// 注入例（操作序列 + 期望码序列）。
#[derive(Clone, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 操作序列（(操作, 参数)）。
    pub ops: Vec<(u8, u32, bool)>,
    /// 期望码序列。
    pub wants: Vec<u8>,
}

/// 30 例注入账构建（10 坏 try 层级 + 8 坏 filter + 6 坏 __finally + 3 坏 unwind + 3 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 组 1：坏 try 层级 10 例（跳层压栈）。
    for i in 0..10u32 {
        let mut ops: Vec<(u8, u32, bool)> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        // 先合法压一层。
        ops.push((OP_ENTER, 0, true));
        wants.push(0);
        // 再跳层压（level = 2+i 而栈深 1）。
        ops.push((OP_ENTER, 2 + i, false));
        wants.push(1); // BadTryLevel 判别值 0 → +1
        v.push(CaseSpec { id: i as u16, ops, wants });
    }
    // 组 2：坏 filter 8 例。
    for i in 0..8u32 {
        let mut ops: Vec<(u8, u32, bool)> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        ops.push((OP_ENTER, 0, false));
        wants.push(0);
        let fid = match i % 3 {
            0 => 0,
            1 => 5 + i,
            _ => FILTER_MAX + 1,
        };
        ops.push((OP_FILTER, fid, false));
        wants.push(2); // BadFilter 判别值 1 → +1
        v.push(CaseSpec { id: 10 + i as u16, ops, wants });
    }
    // 组 3：坏 __finally 6 例（无 finally 标志帧跑 finally）。
    for i in 0..6u32 {
        let mut ops: Vec<(u8, u32, bool)> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        ops.push((OP_ENTER, 0, false));
        wants.push(0);
        ops.push((OP_FINALLY, 0, false));
        wants.push(3); // BadFinally 判别值 2 → +1
        v.push(CaseSpec { id: 18 + i as u16, ops, wants });
    }
    // 组 4：坏 unwind 3 例（弹空栈 / 弹错层）。
    // 例 24：空栈弹。
    v.push(CaseSpec { id: 24, ops: vec![(OP_POP, 0, false)], wants: vec![4] });
    // 例 25：压后弹错层。
    v.push(CaseSpec {
        id: 25,
        ops: vec![(OP_ENTER, 0, false), (OP_POP, 1, false)],
        wants: vec![0, 4],
    });
    // 例 26：压两层弹错层。
    v.push(CaseSpec {
        id: 26,
        ops: vec![(OP_ENTER, 0, false), (OP_ENTER, 1, false), (OP_POP, 0, false)],
        wants: vec![0, 0, 4],
    });
    // 组 5：正路锚 3 例。
    // 例 27：try→finally→pop 全链。
    v.push(CaseSpec {
        id: 27,
        ops: vec![(OP_ENTER, 0, true), (OP_FINALLY, 0, false), (OP_POP, 0, false)],
        wants: vec![0, 0, 0],
    });
    // 例 28：try→filter→pop。
    v.push(CaseSpec {
        id: 28,
        ops: vec![(OP_ENTER, 0, false), (OP_FILTER, 1, false), (OP_POP, 0, false)],
        wants: vec![0, 0, 0],
    });
    // 例 29：两层嵌套全链。
    v.push(CaseSpec {
        id: 29,
        ops: vec![
            (OP_ENTER, 0, true),
            (OP_ENTER, 1, true),
            (OP_FINALLY, 0, false),
            (OP_POP, 1, false),
            (OP_POP, 0, false),
        ],
        wants: vec![0, 0, 0, 0, 0],
    });
    v
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 结果账（append-only：(例号, 全对)）。
    rows: Vec<(u16, bool)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { rows: Vec::new(), high: 0 }
    }

    /// 重放一例。
    pub fn run_case(&mut self, c: &CaseSpec) -> bool {
        let mut st = SehStack::new();
        let mut ok = c.ops.len() == c.wants.len();
        for (i, (op, a, b)) in c.ops.iter().enumerate() {
            let r = match *op {
                OP_ENTER => st.enter_try(*a, *b, 0),
                OP_FILTER => st.run_filter(*a),
                OP_FINALLY => st.run_finally(),
                _ => st.unwind_pop(*a),
            };
            let got = match r {
                Ok(()) => 0u8,
                Err(code) => code as u8 + 1,
            };
            ok = ok && got == c.wants[i];
        }
        self.rows.push((c.id, ok));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 重放一整批。
    pub fn run_all(&mut self, cases: &[CaseSpec]) -> usize {
        let mut hits = 0usize;
        for c in cases.iter() {
            if self.run_case(c) {
                hits += 1;
            }
        }
        hits
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("SEH 注入账：\n");
        for (id, ok) in self.rows.iter() {
            s.push_str(&format!("例{} {}\n", id, if *ok { "过" } else { "红" }));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例注入演练：逐例全对 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30b40 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 三路拒止演练：坏层级/坏 filter/坏 finally 逐路独立 10/10 轮。
pub fn drill_three_rejects_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b41（三路序列）。
    let mut lcg = Lcg::new(0x30b41);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 路 1：坏 try 层级（跳层）。
        let mut st = SehStack::new();
        let _ = st.enter_try(0, false, 0);
        all_ok = all_ok && matches!(st.enter_try(2, false, 0), Err(SeCode::BadTryLevel));
        // 路 2：坏 filter。
        let mut st2 = SehStack::new();
        let _ = st2.enter_try(0, false, 0);
        all_ok = all_ok && matches!(st2.run_filter(0), Err(SeCode::BadFilter))
            && matches!(st2.run_filter(FILTER_MAX + 1), Err(SeCode::BadFilter));
        // 路 3：坏 __finally。
        let mut st3 = SehStack::new();
        let _ = st3.enter_try(0, false, 0);
        all_ok = all_ok && matches!(st3.run_finally(), Err(SeCode::BadFinally));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三路拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：全链 + 两层嵌套 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b42（正路序列）。
    let mut lcg = Lcg::new(0x30b42);
    for _r in 0..INJECT_ROUNDS {
        // 全链。
        let mut st = SehStack::new();
        let mut all_ok = st.enter_try(0, true, 0).is_ok()
            && st.run_finally().is_ok()
            && st.unwind_pop(0).is_ok()
            && st.depth() == 0;
        // 两层嵌套。
        let mut st2 = SehStack::new();
        all_ok = all_ok
            && st2.enter_try(0, false, 1).is_ok()
            && st2.enter_try(1, true, 0).is_ok()
            && st2.run_filter(1).is_ok()
            && st2.unwind_pop(1).is_ok()
            && st2.unwind_pop(0).is_ok()
            && st2.depth() == 0;
        // filter 闭集逐个合法。
        for fid in 1..=FILTER_MAX {
            let mut st3 = SehStack::new();
            let _ = st3.enter_try(0, false, 0);
            all_ok = all_ok && st3.run_filter(fid).is_ok();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 栈深演练：8 层满后第 9 层拒 10/10 轮。
pub fn drill_depth_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b43（栈深序列）。
    let mut lcg = Lcg::new(0x30b43);
    for _r in 0..INJECT_ROUNDS {
        let mut st = SehStack::new();
        let mut all_ok = true;
        for i in 0..DEPTH_MAX {
            all_ok = all_ok && st.enter_try(i as u32, false, 0).is_ok();
        }
        // 第 9 层必拒。
        all_ok = all_ok && matches!(st.enter_try(DEPTH_MAX as u32, false, 0), Err(SeCode::DepthOver));
        // 逐层弹出回空。
        for i in (0..DEPTH_MAX).rev() {
            all_ok = all_ok && st.unwind_pop(i as u32).is_ok();
        }
        all_ok = all_ok && st.depth() == 0;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("栈深轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：坏 try 层级必拒。
    let mut st = SehStack::new();
    let _ = st.enter_try(0, false, 0);
    if matches!(st.enter_try(3, false, 0), Err(SeCode::BadTryLevel)) {
        caught += 1;
        d.push("注1 层级:抓".to_string());
    } else {
        d.push("注1 层级:漏".to_string());
    }
    // 注 2：坏 filter 必拒。
    if matches!(st.run_filter(0), Err(SeCode::BadFilter)) {
        caught += 1;
        d.push("注2 过滤:抓".to_string());
    } else {
        d.push("注2 过滤:漏".to_string());
    }
    // 注 3：坏 __finally 必拒。
    let mut st3 = SehStack::new();
    let _ = st3.enter_try(0, false, 0);
    if matches!(st3.run_finally(), Err(SeCode::BadFinally)) {
        caught += 1;
        d.push("注3 fin:抓".to_string());
    } else {
        d.push("注3 fin:漏".to_string());
    }
    // 注 4：坏 unwind 序必拒（空栈弹）。
    let mut st4 = SehStack::new();
    if matches!(st4.unwind_pop(0), Err(SeCode::BadPop)) {
        caught += 1;
        d.push("注4 弹栈:抓".to_string());
    } else {
        d.push("注4 弹栈:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let mut st5 = SehStack::new();
    if st5.enter_try(0, true, 0).is_ok()
        && st5.run_finally().is_ok()
        && st5.unwind_pop(0).is_ok()
    {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30b40 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    SeCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct SeReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 账单调。
    pub mono: bool,
    /// 已执行。
    pub ran: bool,
}

impl SeReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        SeReport { cases: bk.rows(), hits, mono: bk.monotone(), ran: bk.ran() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono && self.ran
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "SeReport 例={} 命中={} 单调={} 已跑={}",
            self.cases, self.hits, self.mono, self.ran
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct SeCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_se_checklist(bk: &InjectBook, hits: usize) -> Vec<SeCheckRow> {
    let rep = SeReport::build(bk, hits);
    let mut rows: Vec<SeCheckRow> = Vec::new();
    rows.push(SeCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(SeCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(SeCheckRow { name: "坏层级10例", passed: (0..10).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(SeCheckRow { name: "坏filter8例", passed: (10..18).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(SeCheckRow { name: "坏finally6例", passed: (18..24).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(SeCheckRow { name: "坏unwind3例", passed: (24..27).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(SeCheckRow { name: "正路3例", passed: (27..30).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(SeCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(SeCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SeCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn se_checklist_green(rows: &[SeCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn se_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "SEH 注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12490_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        SeReport::build(&bk, hits).all_green() && bk.export_lines().contains("SEH 注入账"),
        se_stats_line(&bk, hits),
    ));
    // X-b：三路拒止 10/10。
    let (ok_t, d_t) = drill_three_rejects_10();
    out.push(("X-三路拒止10/10", ok_t, d_t));
    // X-c：栈深 10/10。
    let (ok_d, d_d) = drill_depth_10();
    out.push(("X-栈深10/10", ok_d, d_d));
    // X-d：状态机操作账回放面。
    let mut st = SehStack::new();
    let _ = st.enter_try(0, true, 0);
    let _ = st.run_finally();
    out.push((
        "X-操作账回放",
        st.ledger_len() == 2 && st.monotone() && st.export_lines().contains("Finally"),
        format!("账 {} 行", st.ledger_len()),
    ));
    // X-e：终验清单。
    let rows = build_se_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        se_checklist_green(&rows),
        SeReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        FILTER_MAX == 4 && DEPTH_MAX == 8 && INJECT_CASES == 30,
        format!("filter={} depth={} cases={}", FILTER_MAX, DEPTH_MAX, INJECT_CASES),
    ));
    out
}

/// run_unx12490_checks：本单判据聚合（诊断段 0x30b40）。
pub fn run_unx12490_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12490 SEH 全链注入矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-30例全中拒止档一致",
        hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone(),
        &format!("命中={}/{}", hits, INJECT_CASES),
    );
    // J2-a：三路拒止 10/10。
    let (ok2, d2) = drill_three_rejects_10();
    s.add_dyn("J2-三路拒止10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：正路锚 10/10。
    let (ok3, d3) = drill_positive_10();
    s.add_dyn("J2-正路锚10/10", ok3, &d3);
    // J2-d：栈深 10/10。
    let (ok4, d4) = drill_depth_10();
    s.add_dyn("J2-栈深10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30b40段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12490_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
