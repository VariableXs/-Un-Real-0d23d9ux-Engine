//! UNX-F21595 · 域收官与全局台账回写（根台账 / handoff）
//!
//! 诊断段 0x91071~0x91078（F2-B40 域收官段 15/100）。
//!
//! 判据原文（施工书 UNX-F21595-J1）：回写最小 patch 清单（AI-30 体例）。
//!
//! 设计要点（为什么是「差集 patch」而不是「全量重写」）：
//! 全量重写会把已存在的行重新落一遍，台账指纹必变，「新增了几行」这个语义就没了；
//! 差集让**第二次回写天然退化为无操作**——幂等不靠标志位、不靠调用方自觉，靠内容
//! 本身：patch 的行要么全不在台账里（整份落），要么全在（无操作），二者之外没有第三种
//! 合法状态。
//!
//! 「半应用」单列一码（0x91077）：patch 里一部分已落、一部分没落，是「上次回写到一半
//! 崩了」的现场。此时既不能整份重放（会重复），也不能当无操作（会丢行），必须让人看。
//! 把它混进 `TimelineConflict` 或 `NotIdempotent` 之外的任何一码，运维都会先去改时间
//! 序，而真正要做的其实是补落剩余行。
//!
//! 超长字段**拒收**而不是静默截断：回写端截断会让「一句话」丢掉尾部结论，而台账读者
//! 看不到截断痕迹。故分工——生成端 `clipped()` 显式截断并留 `~`，回写端只收「≤ 40 字节
//! 或已带标记」的字段。两端口径一致，谁都不能偷偷吞（0x91076 守的就是这条）。
//!
//! 台账按时间序**有序插入**而非尾部追加：并行批次的回写到达顺序不可控，尾部追回会让
//! 时间轴倒挂，后续对账只能重排，而重排会丢掉「谁先写进来」这条信息。
//!
//! 指纹按行内容算、不含行号：行号随插入整体漂移，指纹若含行号，插一行就全变，等于
//! 无法回答「到底哪一行被改过」。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 单字段字节上限（AI-30 体例的一行台账要能塞进一屏）。
pub const FIELD_CAP: usize = 40;
/// 单行字段数（时间 / 工号 / 一句话 / 落点 / 下一步）。
pub const ROW_FIELDS: usize = 5;
/// 目标台账容量（跨 40 批回写，128 行留四倍余量）。
pub const LEDGER_CAP: usize = 128;
/// 单个 patch 的行数上限（一次回写不该是整册重录）。
pub const PATCH_CAP: usize = 64;
/// 目标台账名（本单只服务根台账 handoff）。
pub const LEDGER_NAME: &str = "unxstar/handoff";
/// 截断标记（ASCII，独占一个字节，保证回写端一眼可判）。
pub const CLIP_MARK: char = '~';
/// FNV-1a 64 偏移基（来源：FNV 规范 1a 变体）。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 质数（同上）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 回写诊断码（八码闭集，成因而非现象都在码上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchCode {
    Ok = 0x91071,
    RowMalformed = 0x91072,
    EmptyPatch = 0x91073,
    TargetMismatch = 0x91074,
    TimelineConflict = 0x91075,
    OverlongUnmarked = 0x91076,
    NotIdempotent = 0x91077,
    CapacityOver = 0x91078,
}

impl PatchCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            PatchCode::Ok => "回写完成：patch 行数变化与指纹变化可对账",
            PatchCode::RowMalformed => "台账行格式不合规",
            PatchCode::EmptyPatch => "patch 无新增行仍被回写",
            PatchCode::TargetMismatch => "patch 目标台账与接收台账不符",
            PatchCode::TimelineConflict => "patch 时间轴与既有台账冲突",
            PatchCode::OverlongUnmarked => "字段超长且未带截断标记",
            PatchCode::NotIdempotent => "patch 处于半应用态",
            PatchCode::CapacityOver => "台账或 patch 容量越限",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            PatchCode::Ok => "差集非空、时间序单调、字段长度合规",
            PatchCode::RowMalformed => "字段为空、含分隔符或时间非正",
            PatchCode::EmptyPatch => "生成端没算出差集却仍走了回写路径",
            PatchCode::TargetMismatch => "patch 是按另一本台账生成的",
            PatchCode::TimelineConflict => "同一时标重复出现或时间倒退",
            PatchCode::OverlongUnmarked => "上游把原始长文本直接塞进 patch",
            PatchCode::NotIdempotent => "上次回写只落了一部分行",
            PatchCode::CapacityOver => "越过 128 行或单 patch 64 行",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PatchCode::Ok => "归档回执并推进到下一批",
            PatchCode::RowMalformed => "补齐五字段，去掉字段内竖线",
            PatchCode::EmptyPatch => "无新增行时直接返回无操作回执",
            PatchCode::TargetMismatch => "按目标台账名重新生成 patch",
            PatchCode::TimelineConflict => "改用未占用时标，或按时间序重排 patch",
            PatchCode::OverlongUnmarked => "先在生成端 clipped() 再入 patch",
            PatchCode::NotIdempotent => "对比台账补落剩余行，勿整份重放",
            PatchCode::CapacityOver => "拆分 patch 或另开续册",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会把 0x91071 截成 0x1071）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 回写故障（`Err` 侧，带码、成因与时标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fault {
    pub code: u32,
    pub kind: PatchCode,
    /// 相关时标（0 表示与该行无关）。
    pub at: u32,
}

impl Fault {
    const fn new(kind: PatchCode, at: u32) -> Fault {
        Fault { code: kind.code(), kind, at }
    }
}

/// 一行台账：时间 / 工号 / 一句话 / 落点 / 下一步。
///
/// 四个文本列用 `String` 而非 `&'static str`：回写内容由运行期算出（批号、计数都要
/// 拼进句子），借用静态串根本表达不了；而 `clipped()` 需要产出新的、被截断的串。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// 时标（单调序号，**不是墙钟**：墙钟会让同一输入两次回写得到不同指纹）。
    pub at: u32,
    pub worker: String,
    pub gist: String,
    pub target: String,
    pub next: String,
}

impl Row {
    /// 构造一行（不校验；校验在 `Patch::push` / `Ledger::seed`）。
    pub fn new(at: u32, worker: &str, gist: &str, target: &str, next: &str) -> Row {
        Row {
            at,
            worker: worker.to_string(),
            gist: gist.to_string(),
            target: target.to_string(),
            next: next.to_string(),
        }
    }

    /// 文本列（固定次序：工号 / 一句话 / 落点 / 下一步）。
    pub fn columns(&self) -> [&str; 4] {
        [self.worker.as_str(), self.gist.as_str(), self.target.as_str(), self.next.as_str()]
    }

    /// 字段数（恒为 5，含时标；写成常量而非 len() 以免「字段丢了也自洽」）。
    pub const fn field_count(&self) -> usize {
        ROW_FIELDS
    }

    /// 同键判定用的键：时标 + 工号。
    ///
    /// 只用时标不够：并行批次同一时标各写一行是常态，用「时标 + 工号」当键才能
    /// 同时拒绝「真重复」与放过「同事标不同人」。
    pub fn key(&self) -> (u32, &str) {
        (self.at, self.worker.as_str())
    }

    /// 是否可直接入账：时间正、四列非空、列内不含分隔符。
    pub fn is_shape_ok(&self) -> bool {
        self.at > 0 && self.columns().iter().all(|c| !c.is_empty() && !c.contains('|'))
    }

    /// 字段长度是否合规（全部 ≤ 40 字节）。
    pub fn fits(&self) -> bool {
        self.columns().iter().all(|c| c.len() <= FIELD_CAP)
    }

    /// 显式截断：逐列压到上限内，被切的列尾补 `~`。
    ///
    /// 截到 `cap - 1` 再补标记，是为了让「截断后」仍不超过 cap——否则带标记的行
    /// 会在下一道 `fits()` 上再次被拒，形成死循环。
    pub fn clipped(&self) -> Row {
        Row {
            at: self.at,
            worker: clip(&self.worker),
            gist: clip(&self.gist),
            target: clip(&self.target),
            next: clip(&self.next),
        }
    }

    /// 台账行文本：`at|worker|gist|target|next`。
    pub fn line(&self) -> String {
        let mut out = String::with_capacity(96);
        out.push_str(&self.at.to_string());
        for col in self.columns() {
            out.push('|');
            out.push_str(col);
        }
        out
    }
}

/// 单列截断（按 UTF-8 字符边界回退，尾部补标记）。
fn clip(s: &str) -> String {
    if s.len() <= FIELD_CAP {
        return s.to_string();
    }
    let mut n = FIELD_CAP - 1;
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    let mut out = String::with_capacity(n + 1);
    out.push_str(&s[..n]);
    out.push(CLIP_MARK);
    out
}

/// 最小 patch：只含新增行的清单。
#[derive(Clone)]
pub struct Patch {
    target: String,
    rows: Vec<Row>,
}

impl Patch {
    /// 新建空 patch（绑定目标台账）。
    pub fn new(target: &str) -> Patch {
        Patch { target: target.to_string(), rows: Vec::new() }
    }

    /// 目标台账名。
    pub fn target(&self) -> &str {
        &self.target
    }

    /// 追加一行。
    ///
    /// 四道闸：格式 → 长度 → 容量 → 时间轴。格式与长度是「这一行能不能写」，容量是
    /// 「还装不装得下」，时间轴是「跟前面几行冲不冲突」——按此序，坏行永远先被自己
    /// 的问题拦住，不会被报成「跟上一行时间序冲突」这种误导性成因。
    pub fn push(&mut self, r: Row) -> Result<(), Fault> {
        if !r.is_shape_ok() || r.field_count() != ROW_FIELDS {
            return Err(Fault::new(PatchCode::RowMalformed, r.at));
        }
        if !r.fits() {
            return Err(Fault::new(PatchCode::OverlongUnmarked, r.at));
        }
        if self.rows.len() >= PATCH_CAP {
            return Err(Fault::new(PatchCode::CapacityOver, r.at));
        }
        for x in &self.rows {
            if r.at < x.at || x.key() == r.key() {
                return Err(Fault::new(PatchCode::TimelineConflict, r.at));
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否空 patch。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// 取第 i 行（越界 None，不 panic）。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).cloned()
    }

    /// patch 指纹：逐行咬合，含目标台账名。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_str(FNV_BASIS, &self.target);
        for r in &self.rows {
            h = fnv_row(h, r);
        }
        h
    }

    /// 可归档文本（每行一条，尾随换行）。
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.rows.len() * 48);
        for r in &self.rows {
            out.push_str(&r.line());
            out.push('\n');
        }
        out
    }
}

/// 回写回执（行数变化 + 指纹变化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub before_len: usize,
    pub after_len: usize,
    pub before_fp: u64,
    pub after_fp: u64,
    pub added: usize,
    pub noop: bool,
}

impl Receipt {
    /// 回执侧判决码（成功路径恒为 0x91071；失败走 `Err`，不出回执）。
    pub const fn code(&self) -> PatchCode {
        PatchCode::Ok
    }
    /// 行数变化。
    pub const fn delta(&self) -> usize {
        self.after_len.saturating_sub(self.before_len)
    }
    /// 指纹是否变化（新增 0 行时必为 false，这是回执自洽的硬约束）。
    pub const fn fingerprint_changed(&self) -> bool {
        self.after_fp != self.before_fp
    }
}

/// 目标台账（根台账 handoff）。
pub struct Ledger {
    name: String,
    rows: Vec<Row>,
}

impl Ledger {
    /// 新建空台账。
    pub fn new(name: &str) -> Ledger {
        Ledger { name: name.to_string(), rows: Vec::new() }
    }

    /// 台账名。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 直接播入一行（初始化用；同样过格式与长度闸，且拒同键）。
    pub fn seed(&mut self, r: Row) -> Result<(), Fault> {
        if !r.is_shape_ok() {
            return Err(Fault::new(PatchCode::RowMalformed, r.at));
        }
        if !r.fits() {
            return Err(Fault::new(PatchCode::OverlongUnmarked, r.at));
        }
        if self.rows.len() >= LEDGER_CAP {
            return Err(Fault::new(PatchCode::CapacityOver, r.at));
        }
        if self.contains(r.at, r.worker.as_str()) {
            return Err(Fault::new(PatchCode::TimelineConflict, r.at));
        }
        self.insert_ordered(r);
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否含同键行（时标 + 工号）。
    pub fn contains(&self, at: u32, worker: &str) -> bool {
        self.rows.iter().any(|r| r.at == at && r.worker == worker)
    }

    /// 台账指纹：按**行内容 + 行序**咬合，不含行号。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_str(FNV_BASIS, &self.name);
        for r in &self.rows {
            h = fnv_row(h, r);
        }
        h
    }

    /// 回写最小 patch。
    ///
    /// 判定次序：目标 → 空 patch → 已落情况 → 容量 → 落账。已落情况排在容量前，
    /// 因为「全已落」是成功终态（无操作回执），不该被容量检查误报成越限。
    pub fn apply(&mut self, p: &Patch) -> Result<Receipt, Fault> {
        if p.target() != self.name() {
            return Err(Fault::new(PatchCode::TargetMismatch, 0));
        }
        if p.is_empty() {
            return Err(Fault::new(PatchCode::EmptyPatch, 0));
        }
        let mut present = 0usize;
        for i in 0..p.len() {
            if let Some(r) = p.row(i) {
                if self.contains(r.at, r.worker.as_str()) {
                    present += 1;
                }
            }
        }
        let before_len = self.rows.len();
        let before_fp = self.fingerprint();
        if present == p.len() {
            return Ok(Receipt {
                before_len,
                after_len: before_len,
                before_fp,
                after_fp: before_fp,
                added: 0,
                noop: true,
            });
        }
        if present > 0 {
            return Err(Fault::new(PatchCode::NotIdempotent, 0));
        }
        if before_len + p.len() > LEDGER_CAP {
            return Err(Fault::new(PatchCode::CapacityOver, 0));
        }
        // patch 行在 `push` 时已过格式与长度闸，此处不再复核：若在这里落一半再报错，
        // 才会造出真正的半应用态，正是 0x91077 要防的那种现场。
        for i in 0..p.len() {
            if let Some(r) = p.row(i) {
                self.insert_ordered(r);
            }
        }
        Ok(Receipt {
            before_len,
            after_len: self.rows.len(),
            before_fp,
            after_fp: self.fingerprint(),
            added: p.len(),
            noop: false,
        })
    }

    /// 可归档文本（时间序，一行一条）。
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.rows.len() * 48);
        for r in &self.rows {
            out.push_str(&r.line());
            out.push('\n');
        }
        out
    }

    /// 有序插入：找到第一条时标更大的行，插在它前面。
    fn insert_ordered(&mut self, r: Row) {
        let mut pos = self.rows.len();
        for (i, x) in self.rows.iter().enumerate() {
            if r.at < x.at {
                pos = i;
                break;
            }
        }
        self.rows.insert(pos, r);
    }
}

/// 行咬合（五字段 + 分隔标记）。
fn fnv_row(mut h: u64, r: &Row) -> u64 {
    h = fnv_u64(h, r.at as u64);
    for col in r.columns() {
        h = fnv_str(h, col);
    }
    h
}

/// FNV-1a 64 单步。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= ((v >> (i * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 字符串咬合（尾部补 0xFF 作分隔，避免 ("ab","c") 与 ("a","bc") 撞车）。
fn fnv_str(mut h: u64, s: &str) -> u64 {
    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h ^= 0xFF;
    h.wrapping_mul(FNV_PRIME)
}

/// 超长样本（60 字节 > 40，用于「截断留标记」判据）。
pub const OVERLONG: &str =
    "012345678901234567890123456789012345678901234567890123456789";

/// 标准台账（三行已落账，判据夹具）。
pub fn standard_ledger() -> Result<Ledger, Fault> {
    let mut l = Ledger::new(LEDGER_NAME);
    l.seed(Row::new(10, "ai-30", "F2-B40 满账封印签发", "kernel/varix", "转交接清单"))?;
    l.seed(Row::new(20, "ai-30", "交接清单成册待审", "kernel/varix", "转排程建议"))?;
    l.seed(Row::new(30, "ai-31", "排程建议归档", "kernel/varix", "转域总闸"))?;
    Ok(l)
}

/// 回写候选（六条：四条新、两条已存在；且刻意乱序，逼生成端排序）。
pub fn candidates() -> Vec<Row> {
    vec![
        Row::new(25, "ai-30", "补记交接回执", "kernel/varix", "转复核"),
        Row::new(10, "ai-30", "已在账的重复行", "kernel/varix", "无"),
        Row::new(50, "ai-32", OVERLONG, "kernel/varix", "转复核"),
        Row::new(40, "ai-31", "排程两级排序", "kernel/varix", "转总闸"),
        Row::new(20, "ai-30", "已在账的重复行", "kernel/varix", "无"),
        Row::new(60, "ai-33", "冻结面清单冻结", "kernel/varix", "转归档"),
    ]
}

/// 最小 patch 生成：剔已存在、剔重复、显式截断、按时间序稳定排序。
///
/// 与 `apply` 的分工：生成端**宽容**（已存在的静默剔除，因为「没有新行」是正常业务
/// 结论），回写端**严格**（遇到已存在就判半应用态）。宽容留给「算差集」，严格留给
/// 「改台账」——这是本模块唯一的分层原则。
pub fn build_patch(target: &str, ledger: &Ledger, cand: &[Row]) -> Result<Patch, Fault> {
    let mut kept: Vec<Row> = Vec::new();
    for r in cand {
        if !r.is_shape_ok() || r.field_count() != ROW_FIELDS {
            return Err(Fault::new(PatchCode::RowMalformed, r.at));
        }
        let c = r.clipped();
        let seen = ledger.contains(c.at, c.worker.as_str())
            || kept.iter().any(|x| x.key() == c.key());
        if !seen {
            kept.push(c);
        }
    }
    // 稳定插入排序：同事标多行保留候选顺序（谁先写谁在上），不引入批号等外部次序。
    for i in 1..kept.len() {
        let mut j = i;
        while j > 0 && kept[j].at < kept[j - 1].at {
            kept.swap(j, j - 1);
            j -= 1;
        }
    }
    let mut p = Patch::new(target);
    for r in &kept {
        p.push(r.clone())?;
    }
    Ok(p)
}

/// 判据入口（UNX-F21595-J1 及其配套十三条）。
///
/// 判据常量独立写死：`3` / `4` / `7` / `40` / `0x91071`…`0x91078` 在断言里以字面量
/// 出现，避免「实现改了常量，判据跟着改」的自证循环。
pub fn run_unx21595_ledgerpatch_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21595");
    let ledger = standard_ledger();
    let cand = candidates();
    let patch = match &ledger {
        Ok(l) => build_patch(LEDGER_NAME, l, &cand).ok(),
        Err(_) => None,
    };

    // 1) 夹具可构造：台账三行、候选六条、差集四条（两条重复必被剔除）。
    cs.add(
        "f21595.fixture_buildable",
        ledger.as_ref().map_or(0, |l| l.len()) == 3
            && cand.len() == 6
            && patch.as_ref().map_or(9, |p| p.len()) == 4,
        "标准台账 3 行 / 候选 6 条 / 差集 4 条三者有一不符",
    );

    // 2) 码表钉死：八码值不许漂移，三要素非空。
    {
        let table = [
            PatchCode::Ok, PatchCode::RowMalformed, PatchCode::EmptyPatch, PatchCode::TargetMismatch,
            PatchCode::TimelineConflict, PatchCode::OverlongUnmarked, PatchCode::NotIdempotent,
            PatchCode::CapacityOver,
        ];
        let want = [0x91071u32, 0x91072, 0x91073, 0x91074, 0x91075, 0x91076, 0x91077, 0x91078];
        let mut ok = table.len() == want.len();
        for (i, c) in table.iter().enumerate() {
            ok = ok
                && c.code() == want[i]
                && !c.what().is_empty()
                && !c.why().is_empty()
                && !c.advice().is_empty();
        }
        cs.add("f21595.code_table_pinned", ok, PatchCode::Ok.advice());
    }

    // 3) 正向：回写四条，行数 3→7，指纹必变，回执码 0x91071。
    {
        let mut l = match standard_ledger() {
            Ok(x) => x,
            Err(_) => Ledger::new(LEDGER_NAME),
        };
        let p = match patch.clone() {
            Some(x) => x,
            None => Patch::new(LEDGER_NAME),
        };
        match l.apply(&p) {
            Ok(r) => cs.add(
                "f21595.positive_apply",
                r.added == 4
                    && r.before_len == 3
                    && r.after_len == 7
                    && r.delta() == 4
                    && r.fingerprint_changed()
                    && !r.noop
                    && r.code().code() == 0x91071,
                "回写成功但回执数值不符（应 3→7、added=4、指纹变化）",
            ),
            Err(_) => cs.add("f21595.positive_apply", false, "标准 patch 被回写端拒绝"),
        }
    }

    // 4) 最小性：差集只含新增行，已在账的 10 / 20 不得出现。
    cs.add(
        "f21595.minimal_only_new",
        patch.as_ref().map_or(false, |p| {
            let ats: Vec<u32> = (0..p.len()).filter_map(|i| p.row(i)).map(|r| r.at).collect();
            ats == vec![25u32, 40, 50, 60]
        }),
        "差集未剔除已存在行，或顺序不是 [25,40,50,60]",
    );

    // 5) 时间序：patch 内部严格递增（乱序候选必须被排好）。
    cs.add(
        "f21595.patch_ordered",
        patch.as_ref().map_or(false, |p| {
            let mut ok = true;
            for i in 1..p.len() {
                if let (Some(a), Some(b)) = (p.row(i - 1), p.row(i)) {
                    ok = ok && a.at < b.at;
                }
            }
            ok && p.digest() != 0
        }),
        "patch 行时间序非严格递增，或指纹为零",
    );

    // 6) 反向：行格式不合规（字段含分隔符）必拒 0x91072。
    {
        let mut p = Patch::new(LEDGER_NAME);
        let bad = p.push(Row::new(1, "ai-30", "a|b", "kernel/varix", "无"));
        let mut l = Ledger::new(LEDGER_NAME);
        let seed_bad = l.seed(Row::new(2, "", "空工号", "kernel/varix", "无"));
        cs.add(
            "f21595.malformed_rejected",
            bad.err().map(|f| (f.code, f.kind)) == Some((0x91072, PatchCode::RowMalformed))
                && seed_bad.err().map(|f| (f.code, f.kind)) == Some((0x91072, PatchCode::RowMalformed)),
            PatchCode::RowMalformed.advice(),
        );
    }

    // 7) 反向：空 patch 回写必拒 0x91073（无新增行不是回写，是无操作）。
    {
        let mut l = Ledger::new(LEDGER_NAME);
        let empty = l.apply(&Patch::new(LEDGER_NAME));
        cs.add(
            "f21595.empty_patch_rejected",
            empty.err().map(|f| f.code) == Some(0x91073) && l.len() == 0,
            PatchCode::EmptyPatch.advice(),
        );
    }

    // 8) 反向：目标台账不符必拒 0x91074。
    {
        let mut l = match standard_ledger() {
            Ok(x) => x,
            Err(_) => Ledger::new(LEDGER_NAME),
        };
        let mut p = Patch::new("unxstar/other");
        let _ = p.push(Row::new(80, "ai-90", "错本台账", "kernel/varix", "无"));
        let before = l.len();
        cs.add(
            "f21595.target_mismatch_rejected",
            l.apply(&p).err().map(|f| f.code) == Some(0x91074) && l.len() == before,
            PatchCode::TargetMismatch.advice(),
        );
    }

    // 9) 反向：时间倒挂与同键重复都判 0x91075。
    {
        let mut p = Patch::new(LEDGER_NAME);
        let first = p.push(Row::new(40, "ai-31", "先写四十", "kernel/varix", "无")).is_ok();
        let back = p.push(Row::new(30, "ai-31", "再写三十", "kernel/varix", "无"));
        let mut q = Patch::new(LEDGER_NAME);
        let _ = q.push(Row::new(50, "ai-32", "第一行", "kernel/varix", "无"));
        let twin = q.push(Row::new(50, "ai-32", "同键第二行", "kernel/varix", "无"));
        cs.add(
            "f21595.timeline_conflict",
            first
                && back.err().map(|f| f.code) == Some(0x91075)
                && twin.err().map(|f| f.code) == Some(0x91075),
            PatchCode::TimelineConflict.advice(),
        );
    }

    // 10) 反向 + 正向：裸超长字段拒 0x91076；clipped() 后收且带 ~ 标记。
    {
        let mut p = Patch::new(LEDGER_NAME);
        let raw = p.push(Row::new(1, "ai-30", OVERLONG, "kernel/varix", "无"));
        let ok = p.push(Row::new(2, "ai-30", OVERLONG, "kernel/varix", "无").clipped()).is_ok();
        let marked = p
            .row(0)
            .map_or(false, |r| r.gist.len() == FIELD_CAP && r.gist.ends_with('~'));
        cs.add(
            "f21595.overlong_unmarked",
            raw.err().map(|f| f.code) == Some(0x91076) && ok && marked && OVERLONG.len() == 60,
            PatchCode::OverlongUnmarked.advice(),
        );
    }

    // 11) 反向：半应用态（一行已落 + 一行未落）判 0x91077。
    {
        let mut l = match standard_ledger() {
            Ok(x) => x,
            Err(_) => Ledger::new(LEDGER_NAME),
        };
        let p = match patch.clone() {
            Some(x) => x,
            None => Patch::new(LEDGER_NAME),
        };
        let _ = l.apply(&p);
        let mut half = Patch::new(LEDGER_NAME);
        let _ = half.push(Row::new(25, "ai-30", "已在账", "kernel/varix", "无"));
        let _ = half.push(Row::new(70, "ai-34", "未在账", "kernel/varix", "无"));
        let after = l.len();
        cs.add(
            "f21595.half_applied_rejected",
            l.apply(&half).err().map(|f| f.code) == Some(0x91077) && l.len() == after,
            PatchCode::NotIdempotent.advice(),
        );
    }

    // 12) 幂等：同一 patch 两次回写，第二次为无操作且台账不动。
    {
        let mut l = match standard_ledger() {
            Ok(x) => x,
            Err(_) => Ledger::new(LEDGER_NAME),
        };
        let p = match patch.clone() {
            Some(x) => x,
            None => Patch::new(LEDGER_NAME),
        };
        let first = l.apply(&p);
        let fp_after_first = l.fingerprint();
        let second = l.apply(&p);
        let again = second.as_ref().ok().map_or(false, |r| {
            r.noop && r.added == 0 && !r.fingerprint_changed() && r.before_len == 7
        });
        cs.add(
            "f21595.idempotent_twice",
            first.is_ok() && again && l.len() == 7 && l.fingerprint() == fp_after_first,
            "二次回写不是无操作，或台账指纹被改写",
        );
    }

    // 13) 边界：127 + 1 = 128 收；128 + 1 越限判 0x91078。
    {
        let mut l = Ledger::new(LEDGER_NAME);
        let mut built = true;
        for at in 1..=127u32 {
            if l.seed(Row::new(at, "ai-00", "填充行", "kernel/varix", "无")).is_err() {
                built = false;
                break;
            }
        }
        let mut one = Patch::new(LEDGER_NAME);
        let _ = one.push(Row::new(500, "ai-30", "补最后一行", "kernel/varix", "无"));
        let fill = l.apply(&one);
        let mut two = Patch::new(LEDGER_NAME);
        let _ = two.push(Row::new(501, "ai-30", "再补一行", "kernel/varix", "无"));
        cs.add(
            "f21595.capacity_bound",
            built
                && fill.as_ref().ok().map(|r| r.after_len) == Some(128)
                && l.apply(&two).err().map(|f| f.code) == Some(0x91078),
            "127+1 未被收，或 128+1 未报容量越限",
        );
    }

    // 14) 空台账边界 + 确定性：首行为 1 行，两台同构台账逐字节相同。
    {
        let mut a = Ledger::new(LEDGER_NAME);
        let mut p = Patch::new(LEDGER_NAME);
        let _ = p.push(Row::new(90, "ai-30", "首行入账", "kernel/varix", "转复核"));
        let first = a.apply(&p);
        let mut b = Ledger::new(LEDGER_NAME);
        let _ = b.apply(&p);
        cs.add(
            "f21595.empty_ledger_and_deterministic",
            first.as_ref().ok().map(|r| (r.before_len, r.added)) == Some((0, 1))
                && a.len() == 1
                && a.fingerprint() != 0xcbf2_9ce4_8422_2325
                && a.fingerprint() == b.fingerprint()
                && a.render() == b.render()
                && a.render().starts_with("90|ai-30|首行入账|kernel/varix|转复核")
                && a.name() == LEDGER_NAME
                && p.render().lines().count() == 1,
            "空台账首写不成立，或同构台账指纹/渲染不一致",
        );
    }

    cs
}
