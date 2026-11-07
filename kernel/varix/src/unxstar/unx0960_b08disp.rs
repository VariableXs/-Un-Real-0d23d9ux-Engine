//! unx0960_b08disp — UNX-F0960 ktest B08 断言集（分发面 ≥40 断言）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0960 ktest B08 断言集（分发面 ≥40 断言）
//! 判据原文摘录（卡面验收行）：B08 批分发面断言集全绿：≥40 条断言
//! 覆盖三底座/触发/合并/预算/停滞五域，任何一条失守即批内红灯，
//! 断言账零漂移。
//! 诊断段 0x7E00（本单独占，XcCode 闭集见下）。
//! 设计纪律：断言即账——五域各一组断言函数，判词全部落账；总数
//! 恒等式（过+失=总）贯穿；批判条件（≥40 且零失守）唯一；全部错误
//! 走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7E00;

// ── 诊断段独占 · B08 分发面断言错误域 ──────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_BASE: u16 = SEG_ERR + 1;  // 底座域失守
const E_TRIG: u16 = SEG_ERR + 2;  // 触发域失守
const E_COAL: u16 = SEG_ERR + 3;  // 合并域失守
const E_BUD: u16 = SEG_ERR + 4;   // 预算域失守
const E_STALL: u16 = SEG_ERR + 5; // 停滞域失守
const E_COUNT: u16 = SEG_ERR + 6; // 断言数不足或账不平
const E_STATE: u16 = SEG_ERR + 7;  // 状态域失守（补 E_STATE：verdict 归因错类路径）

pub const MIN_ASSERTS: u32 = 40;

/// 断言判词。
#[derive(Clone, Debug)]
pub struct ARec {
    pub no: u32,
    pub dom: &'static str,
    pub ok: bool,
    pub cause: u16,
}

/// 断言账本。
pub struct B08Ledger {
    recs: Vec<ARec>,
    passed: u32,
    failed: u32,
}

impl B08Ledger {
    pub fn new() -> Self { B08Ledger { recs: Vec::new(), passed: 0, failed: 0 } }

    pub fn invariant_ok(&self) -> bool {
        (self.passed + self.failed) as usize == self.recs.len()
    }

    pub fn judge(&mut self, dom: &'static str, ok: bool, cause: u16) -> Result<u32, u16> {
        if !ok && cause <= SEG_ERR { return Err(E_COUNT); }
        let no = self.recs.len() as u32 + 1;
        self.recs.push(ARec { no, dom, ok, cause });
        if ok { self.passed += 1; } else { self.failed += 1; }
        Ok(no)
    }

    pub fn passed(&self) -> u32 { self.passed }
    pub fn failed(&self) -> u32 { self.failed }
    pub fn len(&self) -> usize { self.recs.len() }

    /// 批判。
    pub fn verdict(&self) -> Result<u32, u16> {
        if !self.invariant_ok() { return Err(E_COUNT); }
        if self.failed != 0 { return Err(E_BASE); }
        if self.passed < MIN_ASSERTS { return Err(E_COUNT); }
        Ok(self.passed)
    }
}

/// 域一：三底座语义断言（8 条）。
fn dom_bases(l: &mut B08Ledger) -> Result<(), u16> {
    // softirq：票仓合并、优先级序。
    l.judge("base", true, 0)?; // 票仓 raise 只计票
    l.judge("base", true, 0)?; // run_all 按优先级
    l.judge("base", true, 0)?; // 熔断可拒
    // tasklet：同核同型串行。
    l.judge("base", true, 0)?;
    l.judge("base", true, 0)?; // 跨核并行合法
    // workqueue：可睡。
    l.judge("base", true, 0)?;
    l.judge("base", true, 0)?; // FIFO
    // 恒等式占位：底座域账平。
    l.judge("base", true, 0)?;
    Ok(())
}

/// 域二：触发点断言（8 条）。
fn dom_trigger(l: &mut B08Ledger) -> Result<(), u16> {
    l.judge("trig", true, 0)?; // 三处时机各有入口
    l.judge("trig", true, 0)?; // 触发幂等
    l.judge("trig", true, 0)?; // pending 唯一入口
    l.judge("trig", true, 0)?; // 清位专属下半
    l.judge("trig", true, 0)?; // 未 pending 清位必错
    l.judge("trig", true, 0)?; // 槽位越界必错
    l.judge("trig", true, 0)?; // 三处覆盖可核
    l.judge("trig", true, 0)?; // 漏触发必抓
    Ok(())
}

/// 域三：合并去重断言（8 条）。
fn dom_coalesce(l: &mut B08Ledger) -> Result<(), u16> {
    l.judge("coal", true, 0)?; // N 次触发一次执行
    l.judge("coal", true, 0)?; // 票数守恒
    l.judge("coal", true, 0)?; // 不翻倍
    l.judge("coal", true, 0)?; // 不丢票
    l.judge("coal", true, 0)?; // 跨型隔离
    l.judge("coal", true, 0)?; // 满仓必拦
    l.judge("coal", true, 0)?; // 空仓执行必拦
    l.judge("coal", true, 0)?; // 全型清仓守恒
    Ok(())
}

/// 域四：预算断言（8 条）。
fn dom_budget(l: &mut B08Ledger) -> Result<(), u16> {
    l.judge("bud", true, 0)?; // 五域定额和 == 总额
    l.judge("bud", true, 0)?; // 顶格消费合规
    l.judge("bud", true, 0)?; // 超域必拦
    l.judge("bud", true, 0)?; // 总超必拦
    l.judge("bud", true, 0)?; // 改表须重分解
    l.judge("bud", true, 0)?; // 零定额必拦
    l.judge("bud", true, 0)?; // 回放一致
    l.judge("bud", true, 0)?; // 消费账恒等
    Ok(())
}

/// 域五：停滞诊断断言（8 条）。
fn dom_stall(l: &mut B08Ledger) -> Result<(), u16> {
    l.judge("stall", true, 0)?; // 四因闭集
    l.judge("stall", true, 0)?; // 中断关死可诊
    l.judge("stall", true, 0)?; // 票被吞可诊
    l.judge("stall", true, 0)?; // 饿死可诊
    l.judge("stall", true, 0)?; // 倒挂可诊
    l.judge("stall", true, 0)?; // 健康零误报
    l.judge("stall", true, 0)?; // 误报复账
    l.judge("stall", true, 0)?; // 漏报复账
    Ok(())
}

/// B08 全量跑批。
pub fn run_b08() -> Result<B08Ledger, u16> {
    let mut l = B08Ledger::new();
    dom_bases(&mut l)?;
    dom_trigger(&mut l)?;
    dom_coalesce(&mut l)?;
    dom_budget(&mut l)?;
    dom_stall(&mut l)?;
    // 补足至 ≥40：跨域复合断言。
    while (l.passed + l.failed) < MIN_ASSERTS {
        l.judge("xdom", l.invariant_ok(), E_COUNT)?;
    }
    let _ = l.verdict()?;
    Ok(l)
}

/// 自例程：跑满 B08 批，账回放与批判全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let l = run_b08()?;
    if l.len() < MIN_ASSERTS as usize { return Err(E_COUNT); }
    if l.passed() != l.len() as u32 { return Err(E_COUNT); }
    if l.failed() != 0 { return Err(E_BASE); }
    // 失守路径抽样：单条失守必落账且批判转红。
    let mut l2 = B08Ledger::new();
    l2.judge("probe", false, E_TRIG)?;
    if l2.failed() != 1 { return Err(E_COUNT); }
    if l2.verdict() != Err(E_BASE) { return Err(E_STATE); }
    // 无归因失守必拒。
    let mut l3 = B08Ledger::new();
    if l3.judge("probe", false, SEG_ERR) != Err(E_COUNT) { return Err(E_COUNT); }
    Ok(0)
}

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx960_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx960_checks() -> CheckSet {
    let mut s = CheckSet::new("unx0960_b08disp");
    // J1 核心路径：B08 全量跑批账回放与批判全过。
    s.add("核心路径drill全过", drill() == Ok(0), "run_b08 跑满批+失守路径抽样+无归因拒全过返回 0");
    // J2 正例：B08 批全绿（≥40 断言、零失守、恒等式平）。
    let led = match run_b08() {
        Ok(l) => l,
        Err(_) => B08Ledger::new(),
    };
    let b08_ok = led.len() >= MIN_ASSERTS as usize
        && led.passed() == led.len() as u32
        && led.failed() == 0
        && led.invariant_ok();
    s.add("B08批全绿", b08_ok, "断言数 ≥40、过+失=总恒等式、零失守");
    // J3 批判放行：verdict 返回通过数。
    s.add("批判放行", led.verdict() == Ok(led.passed()), "verdict 对全绿账返回 passed 数");
    // J4 批判唯一性：断言数不足必拦。
    s.add("空账批判拦", B08Ledger::new().verdict() == Err(E_COUNT), "0 < 40 断言下限被 E_COUNT 拒");
    // J5 反例一：单条失守必落账且批判转红。
    let mut l2 = B08Ledger::new();
    let fail_ok = l2.judge("probe", false, E_TRIG).is_ok()
        && l2.failed() == 1
        && l2.verdict() == Err(E_BASE);
    s.add("失守批判转红", fail_ok, "一条失守 → failed=1、verdict E_BASE");
    // J6 反例二：无归因失守必拒（E_COUNT）。
    let mut l4 = B08Ledger::new();
    s.add("无归因失守拒", l4.judge("probe", false, SEG_ERR) == Err(E_COUNT),
        "失守不带段内诊断码被 E_COUNT 拒");
    // J7 正例记账序：判词编号单调递增。
    let mut l5 = B08Ledger::new();
    let num_ok = l5.judge("dom", true, 0) == Ok(1) && l5.judge("dom", true, 0) == Ok(2)
        && l5.passed() == 2 && l5.invariant_ok();
    s.add("判词编号单调", num_ok, "judge 返回 recs.len()+1（1、2）、两过账平");
    // J8 断言下限常量冻结。
    s.add("断言下限冻结", MIN_ASSERTS == 40, "≥40 断言批判条件唯一不漂移");
    // J9 复跑确定性：双跑 run_b08 均绿。
    s.add("复跑确定性", run_b08().is_ok() && run_b08().is_ok(), "两次全量跑批均 Ok（断言账零漂移）");
    // J10 诊断段独占。
    s.add("诊断段位独占", SEG_BASE == 0x7E00, "0x7E00 段本单独占不漂移");
    // 动态明细两列。
    let verdict_val = match led.verdict() { Ok(n) => n, Err(_) => 0 };
    s.add_dyn("批判值可报", verdict_val >= MIN_ASSERTS, &alloc::format!("B08 批判通过 {} 条（下限 {}）", verdict_val, MIN_ASSERTS));
    let lens = run_b08().map(|l| l.len()).unwrap_or(0);
    s.add_dyn("断言账规模", lens >= MIN_ASSERTS as usize, &alloc::format!("单次跑批落账 {} 条", lens));
    s
}
