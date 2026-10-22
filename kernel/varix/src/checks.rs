//! Shared self-test plumbing for the per-domain `自检` features
//! (F275/F300/F325/F350/F375/F400/F425/F450/F475/F500) and the kernel-wide
//! closed loop (F475).
//!
//! Every domain builds a `CheckSet` locally (no allocator, fixed capacity)
//! and hands it to the aggregator. Rendering stays byte-oriented so the
//! console, the serial log and the QEMU headless assertions (F477) all read
//! the same text.

/// Maximum checks recorded by one domain self-test.
/// 检查项容量（64 → 96：H3 域 copypath/sysgov 深化至 12 层后单项
/// 检查数超 64，容量提升并留 headroom——域聚合不再截断丢红）。
pub const MAX_CHECKS: usize = 112;

/// 动态明细槽位数（`add_dyn` 专用）。
///
/// 背景：`Check.detail` 恒为 `&'static str`（内核 no_std 无堆、且 CheckSet
/// 被 4432 处按值复制，必须保持 `Copy`），但域自检常需回报**运行期算出的**
/// 诊断文本（如 `judge() -> (bool, String)` 的导出行）。既往 28 处调用直接
/// 传 `&String`，与 `&'static str` 契约错配，全量 `cargo check` 报
/// E0597/E0716 共 78 error（占全仓 90 error 的 87%）。
///
/// 修法纪律（避免把 `String` 塞进 `Check`——那会同时破坏 `Copy` 语义与
/// 4565 处栈构造的体积预算）：`Check` 字段一字不动，另设**定容侧表**。
/// 动态明细按写入序落在本表，槽满则截断并计入 `dyn_dropped`（显性不静默）。
const DYN_SLOTS: usize = 8;

/// 单条动态明细的字节容量（UTF-8 边界安全截断，见 `add_dyn`）。
const DYN_CAP: usize = 48;

/// One check result — name, verdict and a short failure detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
    pub detail: &'static str,
}

/// Fixed-capacity result set returned by a domain self-test.
#[derive(Clone, Copy, Debug)]
pub struct CheckSet {
    checks: [Option<Check>; MAX_CHECKS],
    count: usize,
    /// Results dropped because the set was already full. Tracked separately from
    /// `count` so that a域 producing *exactly* MAX_CHECKS checks is not
    /// misreported as truncated.
    dropped: usize,
    /// Domain tag, e.g. `"power"`.
    pub domain: &'static str,
    /// 动态明细侧表（`add_dyn` 写入）。定容 8×48 = 384 字节，仅动态项占用，
    /// 静态路径零开销；`add_dyn` 按写入序落槽，`detail_text` 按同序取回。
    dyn_buf: [[u8; DYN_CAP]; DYN_SLOTS],
    dyn_len: [u8; DYN_SLOTS],
    /// 每条动态明细归属的**检查项下标**（与 `dyn_buf` 同槽位）。
    ///
    /// `add_dyn` 先把明细落侧表再调 `add`，两者用的是**两套计数器**
    /// （侧表 `dyn_n`、检查项 `count`）。`render()` 按检查项下标取明细，若
    /// 只按侧表写入序取，就必须「每条检查都用 `add_dyn`」才对齐——否则动态
    /// 明细在渲染里取不回来，域自检的诊断文本等于白写。这里补一条反查表。
    dyn_at_checks: [u16; DYN_SLOTS],
    /// 动态明细已用槽数（≤ DYN_SLOTS）。
    dyn_n: usize,
    /// 槽满后被拒的动态明细条数（显性记账，不静默吞）。
    dyn_dropped: usize,
}

impl CheckSet {
    pub const fn new(domain: &'static str) -> CheckSet {
        CheckSet {
            checks: [None; MAX_CHECKS],
            count: 0,
            dropped: 0,
            domain,
            dyn_buf: [[0u8; DYN_CAP]; DYN_SLOTS],
            dyn_len: [0u8; DYN_SLOTS],
            dyn_at_checks: [0u16; DYN_SLOTS],
            dyn_n: 0,
            dyn_dropped: 0,
        }
    }

    /// Record one result (no-op when full — the tally stays honest because
    /// `truncated()` reports the overflow).
    pub fn add(&mut self, name: &'static str, passed: bool, detail: &'static str) {
        if self.count >= MAX_CHECKS {
            self.dropped += 1;
            return;
        }
        self.checks[self.count] = Some(Check { name, passed, detail });
        self.count += 1;
    }

    /// 记录一条带**运行期动态明细**的检查结果（`add` 的动态文本版本）。
    ///
    /// 为什么需要它：`add` 的 detail 契约是 `&'static str`，而域自检常需回报
    /// 算出来的诊断文本（`judge() -> (bool, String)` 等）。既往 28 处调用直传
    /// `&String`，触发 E0597/E0716 全量报错；同时把 `String` 塞进 `Check` 会
    /// 破坏 `Copy` 语义（4432 处按值复制）并撑爆 4565 处栈构造的体积预算。
    ///
    /// 行为约定：
    /// - 明细按**写入序**落定容侧表，`detail_text(idx)` 按同序取回；
    /// - 超过 `DYN_CAP` 的文本按 **UTF-8 字符边界**截断（不切碎多字节序列），
    ///   尾部补 `~` 标记截断发生；
    /// - 槽满（> `DYN_SLOTS` 条）时拒绝并计入 `dyn_dropped()`，**不静默吞**——
    ///   `truncated()` 同时覆盖它，避免明细悄悄消失。
    pub fn add_dyn(&mut self, name: &'static str, passed: bool, detail: &str) {
        if self.dyn_n >= DYN_SLOTS {
            self.dyn_dropped += 1;
        } else {
            let slot = self.dyn_n;
            let bytes = detail.as_bytes();
            // 修正记录（首版越界）：原写
            // `let mut n = if bytes.len() <= DYN_CAP { bytes.len() } else { DYN_CAP };`
            // 后接 `while n > 0 && (bytes[n] & 0xC0) == 0x80 { n -= 1; }` ——
            // 当 `bytes.len() == DYN_CAP`（48）且第 48 字节恰是 UTF-8 续字节时，
            // `bytes[n]` 中 `n == 48` 已越过 `bytes.len()`（E0507 实锤，由
            // `unxstar_domain_aggregate_all_green` 当场炸出："index out of
            // bounds: the len is 11 but the index is 11"）。
            // 根因：**用「长度」当下标探测字节类别**——`n` 既是长度又是下标，
            // 语义混用。
            //
            // 修正记录（第二版仍不彻底）：仅回退「续字节」也不够。若截断线
            // 恰好切在一个多字节序列的**首字节之后**，该首字节不是续字节，
            // 回退循环不动它，但它是序列的一部分——补上 `~` 后得到
            // `…\xE4~`，仍是**非法 UTF-8**（实测：47×'b' + '中' → 47 b + E4 + ~）。
            // 判定「切点是否落在字符边界」的**唯一权威是 `str::is_char_boundary`**，
            // 手工按字节类别猜是等价的 bug 温床（3/4 字节序列的首字节模式不同）。
            // 故最终采用：把截断点回退到最近的字符边界，`~` 只在边界之后追加。
            let mut n = DYN_CAP.min(bytes.len());
            if bytes.len() > DYN_CAP {
                // 截断点回退到字符边界（`detail` 本身是合法 `&str`，
                // 故 `n <= bytes.len()` 时下标恒合法）。
                while n > 0 && !detail.is_char_boundary(n) {
                    n -= 1;
                }
            }
            let mut buf = [0u8; DYN_CAP];
            let mut w = 0usize;
            while w < n {
                buf[w] = bytes[w];
                w += 1;
            }
            // 截断标记只在「确有截断」且「留得出 1 字节」时追加，
            // 且追加点必在字符边界上（`n` 已回退到位，`~` 是独立 ASCII）。
            if bytes.len() > DYN_CAP && n + 1 <= DYN_CAP {
                buf[n] = b'~';
                n += 1;
            }
            self.dyn_buf[slot] = buf;
            self.dyn_len[slot] = n as u8;
            // 记下这条明细属于哪个检查项（`add` 在下方以 `count` 为下标写入）。
            // `MAX_CHECKS` 恒 ≤ 65535，故 u16 收得下。
            self.dyn_at_checks[slot] = self.count as u16;
            self.dyn_n += 1;
        }
        self.add(name, passed, "");
    }

    /// 取第 `idx` 条动态明细（写入序）。越界或未写入返回 `""`。
    pub fn detail_text(&self, idx: usize) -> &str {
        if idx >= self.dyn_n {
            return "";
        }
        self.dyn_detail_at_ordinal(idx)
    }

    /// 取检查项 `check_idx` 的动态明细（**按检查项下标反查**）。
    ///
    /// `render()` 是按检查项下标渲染的，而动态明细按侧表写入序落槽——
    /// 两套计数器只在「每条检查都用 `add_dyn`」时才恰好重合。于是这里按
    /// `dyn_at_checks` 反查：混用 `add` / `add_dyn` 时也能取回属于自己的那条。
    pub fn detail_for_check(&self, check_idx: usize) -> &str {
        let mut k = 0usize;
        while k < self.dyn_n {
            if self.dyn_at_checks[k] as usize == check_idx {
                return self.dyn_detail_at_ordinal(k);
            }
            k += 1;
        }
        ""
    }

    fn dyn_detail_at_ordinal(&self, slot: usize) -> &str {
        let n = self.dyn_len[slot] as usize;
        match core::str::from_utf8(&self.dyn_buf[slot][..n]) {
            Ok(s) => s,
            // 写入期已保证边界安全；此处为防御性回退，不 panic。
            Err(_) => "",
        }
    }

    /// 动态明细槽已用条数。
    pub fn dyn_used(&self) -> usize {
        self.dyn_n
    }

    /// 因槽满被拒的动态明细条数（显性记账）。
    pub fn dyn_dropped(&self) -> usize {
        self.dyn_dropped
    }

    /// Record a passing check with no detail.
    pub fn ok(&mut self, name: &'static str) {
        self.add(name, true, "");
    }

    /// 合并两个自检集（AI-U2 深化聚合用：主检 + 深化检并为一行；
    /// 超容如实记 dropped——不静默吞结果）。
    pub fn merge(a: CheckSet, b: CheckSet) -> CheckSet {
        let mut out = CheckSet::new(a.domain);
        for i in 0..a.count {
            if let Some(c) = a.checks[i] {
                out.add(c.name, c.passed, c.detail);
            }
        }
        for i in 0..b.count {
            if let Some(c) = b.checks[i] {
                out.add(c.name, c.passed, c.detail);
            }
        }
        out.dropped += a.dropped + b.dropped;
        // 动态明细侧表随合并一并迁移——否则 a/b 的运行期诊断文本会在
        // 聚合时静默消失（`add_dyn` 写入序即索引序，直接按序追加）。
        let mut k = 0usize;
        while k < a.dyn_n && out.dyn_n < DYN_SLOTS {
            out.dyn_buf[out.dyn_n] = a.dyn_buf[k];
            out.dyn_len[out.dyn_n] = a.dyn_len[k];
            out.dyn_n += 1;
            k += 1;
        }
        let mut j = 0usize;
        while j < b.dyn_n && out.dyn_n < DYN_SLOTS {
            out.dyn_buf[out.dyn_n] = b.dyn_buf[j];
            out.dyn_len[out.dyn_n] = b.dyn_len[j];
            out.dyn_n += 1;
            j += 1;
        }
        // 侧表装不下的余量如实记账（不静默吞）。
        out.dyn_dropped += a.dyn_dropped + b.dyn_dropped + (a.dyn_n - k) + (b.dyn_n - j);
        out
    }

    /// 深化调试面（AI-U2）：逐行枚举检查项（红项定位用——聚合器不调）。
    pub fn red_items(&self) -> ([Option<Check>; MAX_CHECKS], usize) {
        (self.checks, self.count)
    }

    /// 迭代器视图：按写入序产出全部已记录检查项（测试辅助面）。
    ///
    /// 补记：`red_items()` 返回的是 `(数组, 计数)` 元组，直接 `for … in`
    /// 会报「不是迭代器」（E0277 实锤）。此处另给一个真正的迭代入口，
    /// 避免调用方各自手工按下标遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Check> + '_ {
        self.checks[..self.count].iter().filter_map(|c| c.as_ref())
    }

    /// 通过项计数（`tally().0` 的直读别名）。
    pub fn passed_count(&self) -> usize {
        self.tally().0
    }

    /// 未通过项计数（`tally().1` 的直读别名）。
    pub fn failed_count(&self) -> usize {
        self.tally().1
    }

    /// **红项文本**：把全部未通过项的「标签(明细)」拼成一段可读文本。
    ///
    /// 与 [`CheckSet::red_items`] 的分工：后者返回**原始数组**（调用方自行
    /// 遍历），本方法返回**拼好的可读文本**（直接进 panic 消息）。
    ///
    /// 修正记录（诊断盲区，本批最值得沉淀的一条）：域聚合判据历来只报
    /// 「N/M 绿」，**不报是哪几条红**。而折叠式域聚合（`cgpu` 50 子集/
    /// `unxstar` 69 子集/`svstar2` 数百子集）里定位一个红项要靠反复二分
    /// 重跑，每轮都是分钟级——**红项定位成本远高于修复成本**。
    ///
    /// 有了这个入口，各域聚合判据只需在失败分支拼一句即可，不必各自重写
    /// 遍历（此前 cgpu 处写过一版局部拷贝，属重复代码；本方法取代它）。
    ///
    /// 成本纪律：只在**失败路径**构造（调用方务必先判 `all_passed()`），
    /// 绿路径零开销、不分配。
    pub fn red_items_text(&self) -> alloc::string::String {
        let mut out = alloc::string::String::new();
        for (i, c) in self.iter().enumerate() {
            if !c.passed {
                if !out.is_empty() {
                    out.push_str("; ");
                }
                out.push_str(c.name);
                // 动态明细优先（按检查项下标反查），静态 detail 次之。
                let d = self.detail_for_check(i);
                let text = if d.is_empty() { c.detail } else { d };
                if !text.is_empty() {
                    out.push('(');
                    out.push_str(text);
                    out.push(')');
                }
            }
        }
        out
    }

    /// Record a failing check.
    pub fn fail(&mut self, name: &'static str, detail: &'static str) {
        self.add(name, false, detail);
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, index: usize) -> Option<Check> {
        if index < self.count {
            self.checks[index]
        } else {
            None
        }
    }

    /// True when more results were produced than fit (i.e. at least one was
    /// dropped). A set that is merely *full* is not truncated.
    /// 动态侧表丢弃同样计入——明细悄悄消失与检查项被截断同属信息损失。
    pub fn truncated(&self) -> bool {
        self.dropped > 0 || self.dyn_dropped > 0
    }

    /// How many results were dropped past the capacity.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn all_passed(&self) -> bool {
        (0..self.count).all(|i| self.get(i).map(|c| c.passed).unwrap_or(true))
    }

    pub fn tally(&self) -> (usize, usize) {
        let mut passed = 0usize;
        for i in 0..self.count {
            if let Some(c) = self.get(i) {
                if c.passed {
                    passed += 1;
                }
            }
        }
        (passed, self.count - passed)
    }

    /// Render `domain PASS 12/12` style line plus failures into `out`.
    /// Returns bytes written.
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_str(out, &mut n, self.domain);
        push_str(out, &mut n, if self.all_passed() { " PASS " } else { " FAIL " });
        let (passed, failed) = self.tally();
        push_usize(out, &mut n, passed);
        push_str(out, &mut n, "/");
        push_usize(out, &mut n, passed + failed);
        push_str(out, &mut n, "\n");
        for i in 0..self.count {
            if let Some(c) = self.get(i) {
                if !c.passed {
                    push_str(out, &mut n, "  - ");
                    push_str(out, &mut n, c.name);
                    push_str(out, &mut n, ": ");
                    // 动态明细优先（按**检查项下标**反查，混用 add/add_dyn
                    // 也能取回属于自己的那条），静态 detail 次之；两者皆空
                    // 则不追加尾冒号噪声。
                    let d = self.detail_for_check(i);
                    if !d.is_empty() {
                        push_str(out, &mut n, d);
                    } else if !c.detail.is_empty() {
                        push_str(out, &mut n, c.detail);
                    }
                    push_str(out, &mut n, "\n");
                }
            }
        }
        n
    }
}

/// Copy `s` into `out` at `n`, clamped.
pub fn push_str(out: &mut [u8], n: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    }
}

/// Decimal rendering without a formatter.
pub fn push_usize(out: &mut [u8], n: &mut usize, mut v: usize) {
    if v == 0 {
        push_str(out, n, "0");
        return;
    }
    let mut digits = [0u8; 20];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        if *n < out.len() {
            out[*n] = digits[w];
            *n += 1;
        }
    }
}

/// Lowercase hex rendering (minimal width, no `0x` prefix).
pub fn push_hex_u64(out: &mut [u8], n: &mut usize, mut v: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if v == 0 {
        push_str(out, n, "0");
        return;
    }
    let mut digits = [0u8; 16];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        digits[w] = HEX[(v & 0xF) as usize];
        v >>= 4;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        if *n < out.len() {
            out[*n] = digits[w];
            *n += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// F475 — kernel-wide closed loop: every domain registers here, one verdict.
// ---------------------------------------------------------------------------

/// Maximum domains in the kernel checkup registry. WP-301 后 96 域恰满容量，
/// register 超容静默丢域（KernelCheckup 无 truncated 预警）比恰满更危险——
/// 按"不够即扩"纪律扩容，WP-403 撞 128 余量 2 后 WP-404 八域前扩到 144，
/// STAR I 分工阶段（AI-K1 收口实测）注册面已达 258 域 > 144——既有 114 个
/// 注册被静默丢弃，AI-K1 追加 B 性能域 17 域（F041~F057，robust.rs）后
/// 共 275，按同一纪律扩到 288（余量 13 供后续收尾域）；AI-K2 追加 18 域后
/// 逼近 288 上沿，AI-C1 追加 A 兼容域 20 域（F001~F020）→ 294+，按同一
/// 「不够即扩」纪律扩到 320（余量供后续分队收口）；AI-H1 收口实测注册面
/// 已达 364 域（domains 表 350 + 合成器九族 + U2/I4/H1/J2 单聚合）> 320
/// ——H1 批次前按同一纪律扩到 384（余量 20 供 J2/I 域收尾批次）。
pub const MAX_DOMAINS: usize = 384;

/// 每域聚合摘要（register 时从 CheckSet 提取）。CheckSet 全量值拷贝入
/// `[Option<CheckSet>; MAX_DOMAINS]` 会让 KernelCheckup 达 ~870KB——栈上
/// 构造（run_kernel_checkup / aggregate_checksets 的局部量 + 返回槽三份）
/// 直接爆测试线程栈（STATUS_STACK_OVERFLOW 实锤，MAX_DOMAINS=288 后暴露）。
/// 失败明细的 name/detail 恒为 'static 字面量，以引用保留前 8 条；超出
/// render 中如实标注（不静默吞）。
#[derive(Clone, Copy, Debug)]
struct DomainRecord {
    domain: &'static str,
    passed: u16,
    failed: u16,
    dropped: u32,
    fail_name: [&'static str; 8],
    fail_detail: [&'static str; 8],
    fail_n: usize,
}

/// 每域保留的失败明细条数。
const FAIL_DETAIL_KEEP: usize = 8;

/// Aggregate result of `run_kernel_checkup()`.
#[derive(Clone, Copy, Debug)]
pub struct KernelCheckup {
    records: [Option<DomainRecord>; MAX_DOMAINS],
    count: usize,
}

impl KernelCheckup {
    pub const fn new() -> KernelCheckup {
        KernelCheckup {
            records: [None; MAX_DOMAINS],
            count: 0,
        }
    }

    /// Register one domain's result. 提取摘要而非拷贝整个 CheckSet——
    /// 275+ 域 × ~3KB/域的值数组在栈上不可承受（见 DomainRecord 注释）。
    pub fn register(&mut self, set: CheckSet) {
        if self.count < MAX_DOMAINS {
            let mut r = DomainRecord {
                domain: set.domain,
                passed: 0,
                failed: 0,
                dropped: set.dropped as u32,
                fail_name: [""; FAIL_DETAIL_KEEP],
                fail_detail: [""; FAIL_DETAIL_KEEP],
                fail_n: 0,
            };
            for i in 0..set.count {
                if let Some(c) = set.checks[i] {
                    if c.passed {
                        r.passed += 1;
                    } else {
                        r.failed += 1;
                        if r.fail_n < FAIL_DETAIL_KEEP {
                            r.fail_name[r.fail_n] = c.name;
                            r.fail_detail[r.fail_n] = c.detail;
                            r.fail_n += 1;
                        }
                    }
                }
            }
            self.records[self.count] = Some(r);
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn tally(&self) -> (usize, usize) {
        let mut passed = 0usize;
        let mut failed = 0usize;
        for i in 0..self.count {
            if let Some(r) = self.records[i] {
                passed += r.passed as usize;
                failed += r.failed as usize;
            }
        }
        (passed, failed)
    }

    pub fn all_passed(&self) -> bool {
        (0..self.count).all(|i| self.records[i].map(|r| r.failed == 0).unwrap_or(true))
    }

    /// Render every domain line into `out`（格式与 CheckSet::render 一致：
    /// `domain PASS p/n` / `domain FAIL p/n` + `  - name: detail` 失败行；
    /// 超出 FAIL_DETAIL_KEEP 的失败与被 drop 的检查如实标注，不留诊断盲区）。
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if n >= out.len() {
                break;
            }
            if let Some(r) = self.records[i] {
                push_str(out, &mut n, r.domain);
                push_str(out, &mut n, if r.failed == 0 { " PASS " } else { " FAIL " });
                push_usize(out, &mut n, r.passed as usize);
                push_str(out, &mut n, "/");
                push_usize(out, &mut n, (r.passed + r.failed) as usize);
                push_str(out, &mut n, "\n");
                for k in 0..r.fail_n {
                    push_str(out, &mut n, "  - ");
                    push_str(out, &mut n, r.fail_name[k]);
                    push_str(out, &mut n, ": ");
                    push_str(out, &mut n, r.fail_detail[k]);
                    push_str(out, &mut n, "\n");
                }
                if r.failed as usize > r.fail_n {
                    push_str(out, &mut n, "  - ... and ");
                    push_usize(out, &mut n, r.failed as usize - r.fail_n);
                    push_str(out, &mut n, " more failed checks\n");
                }
                if r.dropped > 0 {
                    push_str(out, &mut n, "  - truncated: ");
                    push_usize(out, &mut n, r.dropped as usize);
                    push_str(out, &mut n, " results dropped\n");
                }
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `add_dyn` 截断边界用例需要拼多字节文本（`String::collect` 与
    // `repeat().take()`），no_std 下须显式引入 `String`。
    use alloc::string::String;

    #[test]
    fn set_records_and_tallies() {
        let mut s = CheckSet::new("power");
        s.ok("acpi");
        s.ok("battery");
        s.fail("thermal", "no sensor");
        assert_eq!(s.len(), 3);
        assert_eq!(s.tally(), (2, 1));
        assert!(!s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.get(2).unwrap().detail, "no sensor");
        assert!(s.get(3).is_none());
    }

    #[test]
    fn set_renders_failures_only() {
        let mut s = CheckSet::new("audio");
        s.ok("hda");
        s.fail("usb", "no controller");
        let mut buf = [0u8; 256];
        let n = s.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.starts_with("audio FAIL 1/2\n"));
        assert!(text.contains("- usb: no controller"));
        assert!(!text.contains("hda"));
    }

    #[test]
    fn set_capacity_is_bounded() {
        let mut s = CheckSet::new("x");
        for _ in 0..MAX_CHECKS + 4 {
            s.ok("y");
        }
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(s.truncated());
        assert!(s.all_passed());
    }

    #[test]
    fn exactly_full_is_not_truncated() {
        // 正好 MAX_CHECKS 项不是"被截断"；只有真的丢掉了才算。
        let mut s = CheckSet::new("x");
        for _ in 0..MAX_CHECKS {
            s.ok("y");
        }
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(!s.truncated());
        assert_eq!(s.dropped(), 0);

        s.ok("z"); // 第 MAX_CHECKS+1 项被丢弃
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(s.truncated());
        assert_eq!(s.dropped(), 1);
    }

    #[test]
    fn numbers_render_as_decimal() {
        let mut buf = [0u8; 32];
        let mut n = 0;
        push_usize(&mut buf, &mut n, 0);
        push_str(&mut buf, &mut n, " ");
        push_usize(&mut buf, &mut n, 1234);
        assert_eq!(core::str::from_utf8(&buf[..n]).unwrap(), "0 1234");
    }

    // ---- `add_dyn` 截断边界回归（本批修的真越界的锁） ----------------------
    //
    // 越界成因：首版 `let mut n = min(len, DYN_CAP)` 之后用 `bytes[n]` 探测
    // 字节类别。当 `len == DYN_CAP`（48）且第 48 字节恰是 UTF-8 续字节时，
    // `bytes[48]` 越过 `bytes.len()`（实测炸点："the len is 11 but the
    // index is 11"）。修法：探测下标恒取 `n - 1`（本字节），`n` 只作长度。
    //
    // 下面四组把「长度恰好等于容量」「容量处截断」「末字节是续字节」
    // 「多字节序列正好跨截断线」全钉住——判据的价值在钉边界，不在跑顺路。
    #[test]
    fn add_dyn_at_exact_capacity_does_not_overflow() {
        // 长度恰等于 DYN_CAP，全 ASCII：既不越界也不该被标截断。
        let text: String = core::iter::repeat('a').take(DYN_CAP).collect();
        let mut s = CheckSet::new("cap");
        s.add_dyn("exact", true, &text);
        assert_eq!(s.detail_text(0), text.as_str(), "恰容文本须原样保留");
    }

    #[test]
    fn add_dyn_truncates_over_capacity_at_char_boundary() {
        // 超容 + 切点落在多字节序列内：这是原越界的精确复现路径。
        // 47 个 ASCII + 1 个三字节字符（'中'）= 50 字节 > 48。
        // 切点 48 落在 '中' 的首字节上 → 回退到 47 → 47 个 'b' + '~' = 48 字节。
        let mut text: String = core::iter::repeat('b').take(47).collect();
        text.push('中');
        assert!(text.len() > DYN_CAP);
        let mut s = CheckSet::new("cut");
        s.add_dyn("cut", true, &text);
        let got = s.detail_text(0);
        // 必须退回到合法 UTF-8 边界（不能是半个多字节序列）。
        assert!(core::str::from_utf8(got.as_bytes()).is_ok());
        // 期望值：截断点回退掉 '中' 的首字节，补 '~' 标记。
        // （`&str + &str` 不成立，E0369 —— Rust 无 C 风格字符串相加。）
        let mut want: String = core::iter::repeat('b').take(DYN_CAP - 1).collect();
        want.push('~');
        assert_eq!(got, want.as_str(), "切点落在多字节序列内须整体回退到边界");
        assert_eq!(got.len(), DYN_CAP, "回退后补标记恰好占满容量");
    }

    #[test]
    fn add_dyn_multibyte_crossing_boundary_stays_valid() {
        // 多字节序列正好跨过截断线：3 字节字符的 2 字节会被切掉。
        let mut text: String = core::iter::repeat('c').take(46).collect();
        text.push('中');
        let mut s = CheckSet::new("mb");
        s.add_dyn("mb", true, &text);
        let got = s.detail_text(0);
        assert!(core::str::from_utf8(got.as_bytes()).is_ok(), "跨线截断须是合法 UTF-8");
        assert!(got.len() <= DYN_CAP, "截断结果不得超过容量");
    }

    #[test]
    fn add_dyn_never_panics_across_all_lengths() {
        // 长度 0..=DYN_CAP+8 全遍历：越界是长度相关的，单点用例盖不住。
        for len in 0..=(DYN_CAP + 8) {
            let mut s = CheckSet::new("sweep");
            // 交替 ASCII 与多字节，覆盖「末字节为续字节」的每种奇偶。
            let text: String = (0..len)
                .map(|i| if i % 3 == 0 { '中' } else { 'x' })
                .collect();
            s.add_dyn("sweep", true, &text);
            let got = s.detail_text(0);
            assert!(
                core::str::from_utf8(got.as_bytes()).is_ok(),
                "len={} 截断后非法 UTF-8：{:?}",
                len,
                got
            );
            assert!(got.len() <= DYN_CAP, "len={} 截断结果超容", len);
        }
    }

    #[test]
    fn dyn_slot_overflow_is_accounted_not_swallowed() {
        // 槽满必须显性记账，不静默吞。
        let mut s = CheckSet::new("slots");
        for i in 0..(DYN_SLOTS + 3) {
            s.add_dyn("k", true, "d");
            let _ = i;
        }
        assert_eq!(s.dyn_used(), DYN_SLOTS);
        assert_eq!(s.dyn_dropped(), 3);
        assert!(s.truncated(), "动态丢弃必须计入 truncated");
    }

    #[test]
    fn dyn_detail_is_reachable_when_add_and_add_dyn_are_mixed() {
        // 混用 add / add_dyn 时，`render()` 按**检查项下标**也要取回动态明细。
        // 侧表计数器（dyn_n）与检查项计数器（count）只在全 add_dyn 时才重合，
        // 所以反查必须按 `dyn_at_checks` 而非写入序。
        let mut s = CheckSet::new("mix");
        s.ok("第一项-静态");
        s.add_dyn("第二项-动态", false, "真正的诊断文本");
        s.ok("第三项-静态");
        s.add_dyn("第四项-动态", false, "又一条诊断文本");
        assert_eq!(s.len(), 4);
        // 按下标反查：第 1、3 项是静态明细（空），第 1 动态项落在检查项 1。
        assert_eq!(s.detail_for_check(1), "真正的诊断文本");
        assert_eq!(s.detail_for_check(3), "又一条诊断文本");
        assert_eq!(s.detail_for_check(0), "", "静态项没有动态明细");
        assert_eq!(s.detail_for_check(2), "", "静态项没有动态明细");
        // render 出来的红项必须带上动态诊断文本。
        let mut buf = [0u8; 512];
        let n = s.render(&mut buf);
        let txt = core::str::from_utf8(&buf[..n]).unwrap_or("");
        assert!(txt.contains("第二项-动态: 真正的诊断文本"), "render={}", txt);
        assert!(txt.contains("第四项-动态: 又一条诊断文本"), "render={}", txt);
    }

    #[test]
    fn kernel_checkup_aggregates() {
        let mut k = KernelCheckup::new();
        let mut a = CheckSet::new("power");
        a.ok("s3");
        a.ok("s4");
        let mut b = CheckSet::new("audio");
        b.ok("hda");
        b.fail("mixer", "clipping");
        k.register(a);
        k.register(b);
        assert_eq!(k.len(), 2);
        assert_eq!(k.tally(), (3, 1));
        assert!(!k.all_passed());
        let mut buf = [0u8; 512];
        let n = k.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.contains("power PASS 2/2"));
        assert!(text.contains("audio FAIL 1/2"));
    }
}
