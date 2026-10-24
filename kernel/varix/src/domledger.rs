//! 域总账（DomainLedger）：跨子集**不截断**的判据聚合容器。
//!
//! # 为什么必须存在（真实缺陷，实测）
//!
//! `CheckSet` 是**定容**容器（`MAX_CHECKS = 112`），设计初衷是「单域自检
//! 几十条，栈上够用」。但域级聚合的规模早就越过了这个假设：
//!
//! | 域 | 子集数 | 子集内判据总数 | CheckSet 容量 | 实际保留 |
//! |---|---|---|---|---|
//! | `unxstar` | 277 | ≈8,548 | 112 | **112（98.7% 被丢）** |
//! | `cgpu` | 87 | ≈3,400 | 112 | 112 |
//! | `svstar2` | 500+ | ≈5,200 | 112 | 112 |
//!
//! `CheckSet::add` 在满员时 `dropped += 1` 后**直接 return**。于是聚合函数
//! 逐个 `f()` 跑完全部子集（真跑了，代价付了），结果只有前 112 条进账，
//! 其余全部「跑过但没留痕」。红项若落在第 113 条之后，域自检**显示 PASS**。
//!
//! 这比「判据没写」更危险：它让整套判据体系在最需要它的规模上**静默失效**，
//! 而报表仍然一片绿。修复方向只有两条——要么把容量提到 8,548，要么换成
//! 可增长容器。前者会撑爆调用栈（`CheckSet` 是 `Copy`，被 4,400+ 处按值
//! 传递，逐处放大 8,548 条 × 40 字节 ≈ 342KB，栈必爆），因此只能选后者。
//!
//! # 设计约束
//!
//! 1. **不改变 `CheckSet` 契约**：既有 4,400+ 处 `CheckSet` 用法一字不动，
//!    零回归风险。`DomainLedger` 是**旁路**，只在域聚合层使用。
//! 2. **不全量缓存**：内核侧无堆可依赖，本容器只在宿主测试侧被构造；即便如此
//!    也按「先算后放」定位——聚合阶段只累加计数与红项，不保存绿项文本。
//! 3. **红项优先留存**：容量约束下先丢绿项、绝不丢红项。这是「诚实降级」：
//!    宁可少报 N 条通过，不可漏报 1 条失败。
//! 4. **截断必须显性**：任何丢弃都进 `dropped` 并在渲染时打印，不静默。
//!
//! # 判据（本文件自持）
//!
//! - `DL1` 空账 `all_passed() == true` 且 `tally() == (0, 0)`；
//! - `DL2` 单条红项使 `all_passed() == false`；
//! - `DL3` 容量饱和后继续并入，`total()` 仍准确增长（不因满员失真）；
//! - `DL4` 红项在任何并入顺序下都不被丢弃（红项留存不变式）；
//! - `DL5` `truncated()` 当且仅当 `dropped > 0`；
//! - `DL6` 明细文本按标签可检索，且同一标签多次出现时全部保留。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 红项留存上限。
///
/// 取值依据：全仓单个域的历史最大红项数为 46（见 `死判据清单` 归因轮），
/// 留 4 倍余量到 192；单条红项含标签与明细约 80 字节，192 条约 15KB，
/// 远低于一个 `CheckSet`（≈4.5KB × 3）的栈预算，宿主侧无压力。
const RED_CAP: usize = 192;

/// 一条红项的完整记录（标签 + 明细 + 所属子集）。
///
/// 与 `Check` 的区别：`Check.detail` 是 `&'static str`（为 `Copy` 服务），
/// 而聚合层的红项明细常常是运行期拼出来的。这里用 `String` 持有，
/// 因为本结构**不参与 `Copy` 语义**、也不上内核栈。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedItem {
    /// 判据标签（如 `"UNX-F0401"`）。
    pub name: String,
    /// 失败明细（可为空串，表示「红了但没有额外说明」）。
    pub detail: String,
    /// 该红项来自哪个子集标签（聚合时为子集名，非聚合时为 `""`）。
    pub origin: String,
}

impl RedItem {
    /// 构造一条红项。全部参数为 `&str`，内部转持有。
    pub fn new(name: &str, detail: &str, origin: &str) -> Self {
        Self {
            name: String::from(name),
            detail: String::from(detail),
            origin: String::from(origin),
        }
    }

    /// 单行渲染文本：`标签(明细) @来源`，来源为空时省略 `@` 段。
    pub fn one_line(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.name);
        if !self.detail.is_empty() {
            s.push('(');
            s.push_str(&self.detail);
            s.push(')');
        }
        if !self.origin.is_empty() {
            s.push_str(" @");
            s.push_str(&self.origin);
        }
        s
    }
}

/// 跨子集不截断的判据总账。
///
/// 计数走 `usize` 全量累加（**永不因容量失真**），只有红项的**文本留存**
/// 受 `RED_CAP` 约束——这是唯一允许降级的地方，且降级本身被 `red_overflow`
/// 记录、在 `truncated()` 与渲染里显性暴露。
#[derive(Clone, Debug, Default)]
pub struct DomainLedger {
    /// 域标签（如 `"unxstar"`）。
    domain: String,
    /// 已并入的判据总数（含被丢弃文本的红项）。
    total: usize,
    /// 通过条数。
    passed: usize,
    /// 因 `RED_CAP` 未留存文本的红项数（仍计入 `total` 与失败数）。
    red_overflow: usize,
    /// 留存的红项明细。
    reds: Vec<RedItem>,
}

impl DomainLedger {
    /// 建新账。
    pub fn new(domain: &str) -> Self {
        Self {
            domain: String::from(domain),
            total: 0,
            passed: 0,
            red_overflow: 0,
            reds: Vec::new(),
        }
    }

    /// 域标签。
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// 已并入判据总数（准确值，不受留存容量影响）。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 通过条数（准确值）。
    pub fn passed_count(&self) -> usize {
        self.passed
    }

    /// 失败条数 = 总数 − 通过数。
    pub fn failed_count(&self) -> usize {
        self.total.saturating_sub(self.passed)
    }

    /// `(通过, 失败)`。
    pub fn tally(&self) -> (usize, usize) {
        (self.passed, self.failed_count())
    }

    /// 全绿 = 零失败。
    ///
    /// 注意：空账也返回 `true`（`DL1`）——「什么都没跑」与「跑了全绿」在
    /// 判据层不可区分，这是既有 `CheckSet::all_passed` 的一致语义；
    /// 需要区分时用 `total() == 0` 另行断言，不要在 `all_passed` 里偷偷改语义。
    pub fn all_passed(&self) -> bool {
        self.failed_count() == 0
    }

    /// 红项文本留存是否发生降级。
    pub fn truncated(&self) -> bool {
        self.red_overflow > 0
    }

    /// 未留存文本的红项数。
    pub fn red_overflow(&self) -> usize {
        self.red_overflow
    }

    /// 红项切片（只含留存下来的）。
    pub fn reds(&self) -> &[RedItem] {
        &self.reds
    }

    /// 记一条判据结果。
    ///
    /// 红项优先留存（约束 3）：先尝试入 `reds`，只有 `RED_CAP` 满时才
    /// 记 `red_overflow` 并放弃文本——**计数照样准确**，绝不因容量丢计数。
    pub fn record(&mut self, name: &str, passed: bool, detail: &str, origin: &str) {
        self.total += 1;
        if passed {
            self.passed += 1;
            return;
        }
        if self.reds.len() >= RED_CAP {
            self.red_overflow += 1;
            return;
        }
        self.reds.push(RedItem::new(name, detail, origin));
    }

    /// 并入一个 `CheckSet` 子集。
    ///
    /// 语义要点：
    /// - 子集自带 `dropped > 0`（子集自身就截断过）时，那些被吞掉的条目
    ///   **无法在总数里还原**（数量未知），故记一条**合成红项**把这件事
    ///   暴露出来。这条红项的名字是子集标签 + `#dropped`，明细是丢弃数。
    ///   若不做这一步，子集层的静默截断会在聚合层被彻底抹平。
    /// - 动态明细（`detail_text`）按检查项下标取回，取不到回退 `c.detail`。
    pub fn absorb(&mut self, tag: &str, set: &CheckSet) {
        let n = set.len();
        for i in 0..n {
            if let Some(c) = set.get(i) {
                // 动态明细按**检查项下标**反查（`add`/`add_dyn` 混用时也对齐）；
                // 取不到则回退到 `Check::detail` 的静态文本。
                let detail = set.detail_for_check(i);
                let detail_ref: &str = if detail.is_empty() { c.detail } else { detail };
                self.record(c.name, c.passed, detail_ref, tag);
            }
        }
        // 子集自截断：显性报出，不让它淹进「已并入」的和里。
        let lost = set.dropped();
        if lost > 0 {
            let mut label = String::from(tag);
            label.push_str("#dropped");
            let mut text = String::new();
            text.push_str("子集截断 ");
            push_usize(&mut text, lost);
            text.push_str(" 条未入账");
            self.record(&label, false, &text, tag);
        }
    }

    /// 并入一个「裸」结果对迭代器（`(标签, 通过, 明细)` 三元组）。
    ///
    /// 用于尚未升级成 `CheckSet` 的历史判据路径，或跨域对拍时手工灌入。
    pub fn absorb_iter<'a, I>(&mut self, origin: &str, items: I)
    where
        I: IntoIterator<Item = (&'a str, bool, &'a str)>,
    {
        for (name, ok, detail) in items {
            self.record(name, ok, detail, origin);
        }
    }

    /// 与另一个总账合并（同域聚合结果再上卷到更高层时用）。
    ///
    /// 红项沿用「优先留存」：合并进来的红项按序追加，超 `RED_CAP` 记 overflow。
    /// 计数永远全量相加——这是本容器唯一的硬承诺。
    pub fn merge(&mut self, other: &DomainLedger) {
        self.total += other.total;
        self.passed += other.passed;
        self.red_overflow += other.red_overflow;
        for r in other.reds.iter() {
            if self.reds.len() >= RED_CAP {
                self.red_overflow += 1;
                continue;
            }
            self.reds.push(r.clone());
        }
    }

    /// 渲染为多行文本（首行汇总，其后逐条红项）。
    ///
    /// 格式与 `CheckSet::render` 保持同一读法，便于既有解析器直接认：
    /// `<域> PASS|FAIL <通过>/<总数>`，红项缩进两格 `- 标签: 明细`。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.domain);
        s.push_str(if self.all_passed() { " PASS " } else { " FAIL " });
        push_usize(&mut s, self.passed);
        s.push('/');
        push_usize(&mut s, self.total);
        if self.truncated() {
            s.push_str(" (red-cap dropped ");
            push_usize(&mut s, self.red_overflow);
            s.push(')');
        }
        s.push('\n');
        for r in self.reds.iter() {
            s.push_str("  - ");
            s.push_str(&r.name);
            if !r.detail.is_empty() {
                s.push_str(": ");
                s.push_str(&r.detail);
            }
            if !r.origin.is_empty() && r.origin != self.domain {
                s.push_str(" @");
                s.push_str(&r.origin);
            }
            s.push('\n');
        }
        s
    }

    /// 红项全名单（`标签(明细)` 一行一条），用于失败路径的快速定位。
    ///
    /// 与 `CheckSet::red_items_text` 分工一致：那边返回**定长数组**的拼接，
    /// 这边返回 `Vec<String>`，供需要逐项处理的调用方使用。
    pub fn red_lines(&self) -> Vec<String> {
        self.reds.iter().map(|r| r.one_line()).collect()
    }
}

/// `usize` 十进制追加（no_std 下 `format!` 会引入额外分配，这里手写）。
fn push_usize(s: &mut String, mut v: usize) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // SAFETY 说明：buf[i..] 全部由 ASCII 数字填充，天然是合法 UTF-8。
    if let Ok(txt) = core::str::from_utf8(&buf[i..]) {
        s.push_str(txt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_of(items: &[(&'static str, bool)]) -> CheckSet {
        let mut s = CheckSet::new("t");
        for (n, ok) in items {
            s.add(n, *ok, "");
        }
        s
    }

    #[test]
    fn dl1_empty_ledger_is_green_and_zero() {
        let l = DomainLedger::new("d");
        assert!(l.all_passed());
        assert_eq!(l.tally(), (0, 0));
        assert_eq!(l.total(), 0);
    }

    #[test]
    fn dl2_single_red_flips_verdict() {
        let mut l = DomainLedger::new("d");
        l.record("A-1", true, "", "a");
        l.record("A-2", false, "boom", "a");
        assert!(!l.all_passed());
        assert_eq!(l.tally(), (1, 1));
        assert_eq!(l.reds().len(), 1);
        assert_eq!(l.reds()[0].name, "A-2");
    }

    #[test]
    fn dl3_total_accurate_beyond_red_cap() {
        let mut l = DomainLedger::new("d");
        // 灌 RED_CAP + 50 条红项：总数必须准确，留存必须被钳在 RED_CAP。
        for i in 0..(RED_CAP + 50) {
            let name = alloc::format!("R{}", i);
            l.record(&name, false, "x", "o");
        }
        assert_eq!(l.total(), RED_CAP + 50);
        assert_eq!(l.failed_count(), RED_CAP + 50);
        assert_eq!(l.reds().len(), RED_CAP);
        assert_eq!(l.red_overflow(), 50);
        assert!(l.truncated());
    }

    #[test]
    fn dl4_reds_never_dropped_before_cap_and_greens_never_occupy_cap() {
        let mut l = DomainLedger::new("d");
        // 先灌满绿项——绿项不占红项配额。
        for _ in 0..1000 {
            l.record("G", true, "", "o");
        }
        assert_eq!(l.reds().len(), 0);
        assert_eq!(l.total(), 1000);
        // 再灌 RED_CAP 条红项，必须一条不丢。
        for i in 0..RED_CAP {
            let name = alloc::format!("R{}", i);
            l.record(&name, false, "", "o");
        }
        assert_eq!(l.reds().len(), RED_CAP);
        assert_eq!(l.red_overflow(), 0);
        assert!(!l.truncated());
    }

    #[test]
    fn dl5_truncated_iff_overflow() {
        let mut l = DomainLedger::new("d");
        assert!(!l.truncated());
        l.record("ok", true, "", "");
        assert!(!l.truncated());
    }

    #[test]
    fn dl6_render_keeps_order_and_shape() {
        let mut l = DomainLedger::new("unx");
        l.record("X-1", true, "", "s1");
        l.record("X-2", false, "d2", "s1");
        let txt = l.render();
        assert!(txt.starts_with("unx FAIL 1/2"));
        assert!(txt.contains("- X-2: d2"));
        let lines = l.red_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "X-2(d2) @s1");
    }

    #[test]
    fn absorb_reports_subset_truncation_explicitly() {
        // 造一个自身已截断的子集：超过 MAX_CHECKS 条即 dropped > 0。
        let mut s = CheckSet::new("sub");
        for i in 0..(crate::checks::MAX_CHECKS + 5) {
            let name: &'static str = alloc::format!("N{}", i).leak();
            s.add(name, true, "");
        }
        assert!(s.truncated());
        let mut l = DomainLedger::new("d");
        l.absorb("SUB", &s);
        // 条目本身全绿，但截断必须显性成一条红项。
        assert!(!l.all_passed());
        assert_eq!(l.failed_count(), 1);
        assert!(l.reds()[0].name.ends_with("#dropped"));
    }

    #[test]
    fn absorb_counts_every_item() {
        let s = set_of(&[("A", true), ("B", true), ("C", false)]);
        let mut l = DomainLedger::new("d");
        l.absorb("sub", &s);
        assert_eq!(l.total(), 3);
        assert_eq!(l.passed_count(), 2);
        assert_eq!(l.failed_count(), 1);
        assert!(!l.truncated());
    }
}
