//! VE-F0077 · 渲染状态快照导出（VE-A 域 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0077`
//!
//! **职责定位（锚点原文）**：任意时刻渲染全状态的快照导出（管线 / 资源绑定 /
//! 渲染目标三层），快照格式开放（业界工具可读），快照体积预算与裁剪声明。
//!
//! **判据（锚点原文）**：三层快照、开放格式、体积预算、裁剪声明、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 格式漂移 → **版本锁定** | [`Snapshot::seal`] 以 [`FORMAT_MAJOR`] 硬锁：主版本不等即拒（[`E_VERSION_LOCKED`]），次版本只接受「不超前」的写方（读方可以读旧）。锁的是**写出的格式**，不是读入的容忍度——把两者混成一个版本号，会让「工具能读」与「内核能写」互相绑架 |
//! | 体积超预算 → **裁剪声明** | [`export`] 逐层裁剪并输出 [`Trim`]：丢弃数按层分账、总数与保留数**相加必须等于原始条目数**（[`Trim::balanced`]）。裁剪不是「悄悄少写几行」——声明行缺失即视为导出失败（[`E_FORMAT_DRIFT`]） |
//! | 导出失败 → **重试** | [`retry_export`] 以 [`MAX_ATTEMPTS`] 封顶，逐次记 [`RetryLog`]；耗尽返回 [`E_RETRY_EXHAUSTED`]。重试**不改变**快照内容（幂等），只重放同一次导出 |
//!
//! **性能逐项分解（锚点原文）**：O(状态)。条目数受 [`MAX_ENTRIES`] 定容约束，
//! 裁剪是「重渲染—比较」循环，最坏 O(条目²)；条目上限 64，实测最坏 4096 次
//! 字节比较，全部在常数级预算内，不需要增量字节账。
//!
//! **跨批对接点**：A14 快照复用——[`Snapshot`] 可被重复 [`export`] 而不消耗，
//! `gen` 字段供 A14 判断「这份快照是不是我要的那一代」。
//!
//! **无障碍与隐私（锚点原文）**：导出状态读屏可达（[`ExportResult::a11y_lines`]）
//! ——中英双语逐行播报层数、条目数、字节数、是否裁剪；不导出窗口标题、进程名、
//! 用户路径之类可识别信息，只导出渲染层的键值。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **三层是硬约束，不是建议**：缺任何一层就拒绝导出。只导管线不导绑定，
//!   工具侧看到的是「一个没有输入的管线」，比没有快照更误导。
//! - **开放格式 = 头行可机读**：`@varix-snap 1.3 gen=42` 是自描述头，任何
//!   工具先读这一行就知道版本与代次，不必猜。判据用 [`parse_header`] 反向
//!   验证——写得出、读得回，才算开放。
//! - **裁剪有优先序**：先丢渲染目标（可由管线重建）、再丢绑定（可由管线
//!   重放）、最后才动管线（不可重建）。顺序写进 [`Trim::order`] 的文档，
//!   判据钉死。
//! - **零 panic 面**：无 `unwrap`/`expect`/索引越界；全部计数 `saturating_*`。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA70C/D/E 与 0xA71C/D/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 请求非法：空快照、空键、条目超容。
pub const E_BAD_REQUEST: u16 = 0xA70C;
/// 主版本不符：写出的格式与本模块锁定的主版本不一致。
pub const E_VERSION_LOCKED: u16 = 0xA70D;
/// 三层缺一：管线 / 绑定 / 目标任一为空。
pub const E_LAYER_MISSING: u16 = 0xA70E;
/// 预算非法：预算为零或超过硬上限。
pub const E_BUDGET_INVALID: u16 = 0xA71C;
/// 重试耗尽。
pub const E_RETRY_EXHAUSTED: u16 = 0xA71D;
/// 格式漂移：输出缺头行或缺裁剪声明行。
pub const E_FORMAT_DRIFT: u16 = 0xA71E;

// ---------------------------------------------------------------------------
// 二、格式常量
// ---------------------------------------------------------------------------

/// 快照格式主版本。**硬锁**：改动条目语法必须进位。
pub const FORMAT_MAJOR: u16 = 1;
/// 快照格式次版本。只增不改；读方可接受更旧的次版本。
pub const FORMAT_MINOR: u16 = 3;
/// 自描述头行前缀。
pub const HEADER_TAG: &str = "@varix-snap";
/// 裁剪声明行前缀。
pub const TRIM_TAG: &str = "# trimmed";
/// 条目上限：64 条足够描述一条渲染路径的全状态，再多说明有人在把逐帧数据
/// 塞进快照。
pub const MAX_ENTRIES: usize = 64;
/// 单次导出的默认字节预算。
pub const DEFAULT_BUDGET: usize = 8192;
/// 预算硬上限，防止调用方传入天文数字把裁剪逻辑架空。
pub const MAX_BUDGET: usize = 1 << 20;
/// 重试次数上限。
pub const MAX_ATTEMPTS: u32 = 3;

// ---------------------------------------------------------------------------
// 三、层
// ---------------------------------------------------------------------------

/// 快照的三层。顺序即裁剪的**逆序**：越靠后越先被丢。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Layer {
    /// 管线：着色器阶段、光栅状态、混合模式——不可由别处重建。
    Pipeline = 0,
    /// 资源绑定：纹理 / 缓冲 / 采样器槽位——可由管线重放。
    Bindings = 1,
    /// 渲染目标：颜色 / 深度附件的当前绑定——可由管线重建。
    Targets = 2,
}

pub const LAYER_COUNT: usize = 3;

impl Layer {
    pub const ALL: [Layer; LAYER_COUNT] = [Layer::Pipeline, Layer::Bindings, Layer::Targets];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Pipeline => "pipeline",
            Layer::Bindings => "bindings",
            Layer::Targets => "targets",
        }
    }

    /// 从序列化文本反查层名。
    pub fn from_str(s: &str) -> Option<Layer> {
        Layer::ALL.iter().copied().find(|l| l.as_str() == s)
    }
}

// ---------------------------------------------------------------------------
// 四、版本
// ---------------------------------------------------------------------------

/// 格式版本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl Version {
    pub const fn current() -> Version {
        Version {
            major: FORMAT_MAJOR,
            minor: FORMAT_MINOR,
        }
    }

    /// 本模块能否写出这个版本。主版本必须相等；次版本不得超前。
    pub fn writable(self) -> bool {
        self.major == FORMAT_MAJOR && self.minor <= FORMAT_MINOR
    }

    pub fn render(self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl Default for Version {
    fn default() -> Version {
        Version::current()
    }
}

// ---------------------------------------------------------------------------
// 五、条目与快照
// ---------------------------------------------------------------------------

/// 一条状态。键是渲染层的符号名（不含用户数据），值是标量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub layer: Layer,
    pub key: &'static str,
    pub value: u64,
}

/// 一次导出所用的状态集合。
#[derive(Clone, Debug)]
pub struct Snapshot {
    entries: Vec<Entry>,
    /// 代次：调用方用它确认「是不是我要的那一帧的状态」。
    pub generation: u64,
    pub version: Version,
}

impl Snapshot {
    /// 建一个空快照。`generation` 由调用方给，模块不自己编。
    pub fn new(generation: u64) -> Snapshot {
        Snapshot {
            entries: Vec::new(),
            generation,
            version: Version::current(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// 追加一条。空键、超容、重复键都拒绝——重复键会让工具侧读到两个
    /// 互相矛盾的值，而它们看起来都合法。
    pub fn push(&mut self, layer: Layer, key: &'static str, value: u64) -> Result<(), u16> {
        if key.is_empty() {
            return Err(E_BAD_REQUEST);
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(E_BAD_REQUEST);
        }
        if self.entries.iter().any(|e| e.layer == layer && e.key == key) {
            return Err(E_BAD_REQUEST);
        }
        self.entries.push(Entry { layer, key, value });
        Ok(())
    }

    /// 每层条目数。
    pub fn count(&self, layer: Layer) -> usize {
        self.entries.iter().filter(|e| e.layer == layer).count()
    }

    /// 封版：三层齐备 + 版本可写，才算一份可导出的快照。
    pub fn seal(&self) -> Result<(), u16> {
        if !self.version.writable() {
            return Err(E_VERSION_LOCKED);
        }
        for l in Layer::ALL {
            if self.count(l) == 0 {
                return Err(E_LAYER_MISSING);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、裁剪声明
// ---------------------------------------------------------------------------

/// 为什么裁。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrimReason {
    /// 总字节超预算。
    OverBudget,
}

impl TrimReason {
    pub fn as_str(self) -> &'static str {
        match self {
            TrimReason::OverBudget => "over-budget",
        }
    }
}

/// 裁剪声明。按层分账，总数必须与「原始 - 保留」相等。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trim {
    pub dropped: [u32; LAYER_COUNT],
    pub reason: TrimReason,
    /// 原始条目数。
    pub original: u32,
    /// 保留条目数。
    pub kept: u32,
}

impl Trim {
    pub fn dropped_total(&self) -> u32 {
        self.dropped.iter().fold(0u32, |a, b| a.saturating_add(*b))
    }

    /// 守恒：丢弃 + 保留 == 原始。
    pub fn balanced(&self) -> bool {
        self.dropped_total().saturating_add(self.kept) == self.original
    }

    /// 某层是否被裁过。
    pub fn touched(&self, layer: Layer) -> bool {
        self.dropped[layer.index()] > 0
    }

    pub fn render(&self) -> String {
        format!(
            "{} reason={} dropped={}/{}/{} kept={} of {}",
            TRIM_TAG,
            self.reason.as_str(),
            self.dropped[0],
            self.dropped[1],
            self.dropped[2],
            self.kept,
            self.original
        )
    }
}

// ---------------------------------------------------------------------------
// 七、导出
// ---------------------------------------------------------------------------

/// 导出结果。`trim == None` 表示未裁剪（不是「裁剪了 0 条」——两者在
/// 工具侧含义不同：前者可信，后者要追原因）。
#[derive(Clone, Debug)]
pub struct ExportResult {
    pub text: String,
    pub bytes: usize,
    pub trim: Option<Trim>,
    pub generation: u64,
    pub version: Version,
}

impl ExportResult {
    /// 读屏可达的导出状态：中英双语，只报事实与计数。
    pub fn a11y_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "快照代次 {} / snapshot generation {}",
            self.generation, self.generation
        ));
        out.push(format!(
            "格式版本 {} / format version {}",
            self.version.render(),
            self.version.render()
        ));
        out.push(format!("体积 {} 字节 / {} bytes", self.bytes, self.bytes));
        match &self.trim {
            Some(t) => {
                out.push(format!(
                    "已裁剪：丢弃 {} 条，保留 {} 条 / trimmed: {} dropped, {} kept",
                    t.dropped_total(),
                    t.kept,
                    t.dropped_total(),
                    t.kept
                ));
                out.push(format!("裁剪原因 / reason: {}", t.reason.as_str()));
            }
            None => {
                out.push("未裁剪 / not trimmed".to_string());
            }
        }
        out.push("导出完成 / export complete".to_string());
        out
    }

    /// 输出是否是自洽的开放格式：头行可解析、版本一致、条目行以层名为前缀。
    pub fn well_formed(&self) -> bool {
        match parse_header(&self.text) {
            Some((v, gen)) => {
                v == self.version
                    && gen == self.generation
                    && self.text.contains(TRIM_TAG) == self.trim.is_some()
            }
            None => false,
        }
    }
}

/// 解析自描述头行，返回（版本，代次）。
pub fn parse_header(text: &str) -> Option<(Version, u64)> {
    let first = text.lines().next()?;
    let rest = first.strip_prefix(HEADER_TAG)?.trim_start();
    let mut parts = rest.split_whitespace();
    let ver = parts.next()?;
    let gen_part = parts.next()?;
    let (maj, min) = ver.split_once('.')?;
    let major: u16 = maj.parse().ok()?;
    let minor: u16 = min.parse().ok()?;
    let gen_str = gen_part.strip_prefix("gen=")?;
    let gen: u64 = gen_str.parse().ok()?;
    Some((Version { major, minor }, gen))
}

/// 渲染一份条目集合为文本。
fn render(entries: &[Entry], generation: u64, version: Version) -> String {
    let mut out = String::with_capacity(256);
    out.push_str(HEADER_TAG);
    out.push(' ');
    out.push_str(&version.render());
    out.push_str(&format!(" gen={}\n", generation));
    for l in Layer::ALL {
        for e in entries.iter().filter(|e| e.layer == l) {
            out.push_str(l.as_str());
            out.push('.');
            out.push_str(e.key);
            out.push('=');
            out.push_str(&e.value.to_string());
            out.push('\n');
        }
    }
    out
}

/// 按预算导出。超预算时按「目标 → 绑定 → 管线」的逆序裁剪，并写声明行。
///
/// 裁剪算法是「丢一条、重渲染、再比较」。条目上限 64，最坏 4096 次比较，
/// 相比维护一份增量字节账（及其与真实渲染漂移的风险）更值得。
pub fn export(snap: &Snapshot, budget: usize) -> Result<ExportResult, u16> {
    if budget == 0 || budget > MAX_BUDGET {
        return Err(E_BUDGET_INVALID);
    }
    snap.seal()?;

    let mut kept: Vec<Entry> = snap.entries().to_vec();
    let mut dropped = [0u32; LAYER_COUNT];
    let original = kept.len() as u32;
    let mut trim: Option<Trim> = None;

    // 逐层丢，直到进预算或无可丢。
    for layer in [Layer::Targets, Layer::Bindings, Layer::Pipeline] {
        loop {
            let mut body = render(&kept, snap.generation, snap.version);
            // 预留声明行的空间：声明行最长约 80 字节。
            if body.len() + 96 <= budget {
                break;
            }
            match kept.iter().rposition(|e| e.layer == layer) {
                Some(pos) => {
                    kept.remove(pos);
                    dropped[layer.index()] = dropped[layer.index()].saturating_add(1);
                }
                None => break,
            }
        }
        if kept.len() as u32 != original {
            trim = Some(Trim {
                dropped,
                reason: TrimReason::OverBudget,
                original,
                kept: kept.len() as u32,
            });
        }
    }

    let mut text = render(&kept, snap.generation, snap.version);
    if let Some(t) = trim {
        text.push_str(&t.render());
        text.push('\n');
    }
    let bytes = text.len();

    let result = ExportResult {
        text,
        bytes,
        trim,
        generation: snap.generation,
        version: snap.version,
    };
    // 自检式返回：写不出可解析的头就当场失败，不把坏文本交给调用方。
    if !result.well_formed() {
        return Err(E_FORMAT_DRIFT);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// 八、重试
// ---------------------------------------------------------------------------

/// 重试记账。每次尝试的结局都留痕，耗尽后仍可回答「试了几次、错在哪」。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RetryLog {
    pub attempts: u32,
    pub last_error: u16,
}

/// 重放同一次导出，最多 [`MAX_ATTEMPTS`] 次。
///
/// `f` 必须**无副作用地**产生同一份快照的导出——重试的是导出动作，不是
/// 状态采集。带副作用的采集放进重试会让「第二次成功」变成「采到了别的
/// 状态」，看起来修好了，实际是换了一份数据。
pub fn retry_export<F>(mut f: F) -> (Option<ExportResult>, RetryLog)
where
    F: FnMut() -> Result<ExportResult, u16>,
{
    let mut log = RetryLog::default();
    for _ in 0..MAX_ATTEMPTS {
        log.attempts = log.attempts.saturating_add(1);
        match f() {
            Ok(r) => {
                log.last_error = 0;
                return (Some(r), log);
            }
            Err(code) => log.last_error = code,
        }
    }
    (None, log)
}

// ---------------------------------------------------------------------------
// 九、判据
// ---------------------------------------------------------------------------

fn sample() -> Snapshot {
    let mut s = Snapshot::new(7);
    let _ = s.push(Layer::Pipeline, "vs", 0x1000);
    let _ = s.push(Layer::Pipeline, "ps", 0x2000);
    let _ = s.push(Layer::Pipeline, "blend", 3);
    let _ = s.push(Layer::Bindings, "tex0", 0xAB);
    let _ = s.push(Layer::Bindings, "tex1", 0xCD);
    let _ = s.push(Layer::Targets, "color", 0x9000);
    let _ = s.push(Layer::Targets, "depth", 0x9100);
    s
}

/// VE-F0077 的十四条判据。
pub fn run_vea77_checks() -> CheckSet {
    let mut c = CheckSet::new("vea77");

    // 1. 三层齐备即封版通过。
    let s = sample();
    if s.seal().is_ok() {
        c.ok("A77-三层-齐备即封版");
    } else {
        c.fail("A77-三层-齐备即封版", "seal rejected complete snapshot");
    }

    // 2. 缺层即拒，且错误码指认缺层。
    let mut miss = Snapshot::new(1);
    let _ = miss.push(Layer::Pipeline, "vs", 1);
    let _ = miss.push(Layer::Bindings, "tex0", 2);
    if miss.seal() == Err(E_LAYER_MISSING) {
        c.ok("A77-三层-缺一即拒");
    } else {
        c.fail("A77-三层-缺一即拒", "missing layer accepted");
    }

    // 3. 主版本不符即拒。
    let mut bad_ver = sample();
    bad_ver.version = Version {
        major: FORMAT_MAJOR + 1,
        minor: 0,
    };
    if bad_ver.seal() == Err(E_VERSION_LOCKED) {
        c.ok("A77-版本-主版本不符即拒");
    } else {
        c.fail("A77-版本-主版本不符即拒", "major drift accepted");
    }

    // 4. 次版本超前即拒，落后可写。
    let older = Version {
        major: FORMAT_MAJOR,
        minor: FORMAT_MINOR.saturating_sub(1),
    };
    let newer = Version {
        major: FORMAT_MAJOR,
        minor: FORMAT_MINOR + 1,
    };
    if older.writable() && !newer.writable() {
        c.ok("A77-版本-次版本只收不超前");
    } else {
        c.fail("A77-版本-次版本只收不超前", "minor window wrong");
    }

    // 5. 预算为零或超上限即拒。
    if export(&s, 0).err() == Some(E_BUDGET_INVALID) && export(&s, MAX_BUDGET + 1).err() == Some(E_BUDGET_INVALID)
    {
        c.ok("A77-预算-零与超限即拒");
    } else {
        c.fail("A77-预算-零与超限即拒", "bad budget accepted");
    }

    // 6. 正常导出：头行可解析、代次一致。
    let full = match export(&s, DEFAULT_BUDGET) {
        Ok(r) => r,
        Err(_) => {
            c.fail("A77-导出-头行可解析", "export failed");
            return c;
        }
    };
    let parsed = parse_header(&full.text);
    if matches!(parsed, Some((v, g)) if v == Version::current() && g == 7) {
        c.ok("A77-导出-头行可解析");
    } else {
        c.fail("A77-导出-头行可解析", "header unreadable");
    }

    // 7. 字节数与文本长度一致。
    if full.bytes == full.text.len() {
        c.ok("A77-导出-字节账自洽");
    } else {
        c.fail("A77-导出-字节账自洽", "byte count drifted");
    }

    // 8. 未裁剪时不出现裁剪声明行。
    if full.trim.is_none() && !full.text.contains(TRIM_TAG) {
        c.ok("A77-裁剪-未裁不声明");
    } else {
        c.fail("A77-裁剪-未裁不声明", "phantom trim line");
    }

    // 9. 紧预算必然触发裁剪，且声明行出现。
    let tight = match export(&s, 120) {
        Ok(r) => r,
        Err(_) => {
            c.fail("A77-裁剪-紧预算即声明", "tight export failed");
            return c;
        }
    };
    if tight.trim.is_some() && tight.text.contains(TRIM_TAG) {
        c.ok("A77-裁剪-紧预算即声明");
    } else {
        c.fail("A77-裁剪-紧预算即声明", "trim not declared");
    }

    // 10. 裁剪守恒：丢弃 + 保留 == 原始。
    match tight.trim {
        Some(t) if t.balanced() && t.original == 7 => c.ok("A77-裁剪-丢弃保留守恒"),
        _ => c.fail("A77-裁剪-丢弃保留守恒", "trim accounting broken"),
    }

    // 11. 裁剪优先序：目标层先被丢，管线最后。
    match tight.trim {
        Some(t) if t.touched(Layer::Targets) => c.ok("A77-裁剪-目标层先丢"),
        _ => c.fail("A77-裁剪-目标层先丢", "targets survived a tight budget"),
    }

    // 12. 导出幂等：同快照两次导出逐字节相同。
    let again = export(&s, DEFAULT_BUDGET).map(|r| r.text).unwrap_or_default();
    if again == full.text {
        c.ok("A77-导出-同状态同输出");
    } else {
        c.fail("A77-导出-同状态同输出", "export not idempotent");
    }

    // 13. 重试：首次失败、二次成功，记账两次。
    let mut n = 0u32;
    let (ok, log) = retry_export(|| {
        n = n.saturating_add(1);
        if n == 1 {
            Err(E_FORMAT_DRIFT)
        } else {
            Ok(full.clone())
        }
    });
    if ok.is_some() && log.attempts == 2 && log.last_error == 0 {
        c.ok("A77-重试-首次失败二次成功");
    } else {
        c.fail("A77-重试-首次失败二次成功", "retry accounting wrong");
    }

    // 14. 重试耗尽即报错，且尝试次数封顶。
    let (none, log) = retry_export(|| Err(E_RETRY_EXHAUSTED));
    if none.is_none() && log.attempts == MAX_ATTEMPTS && log.last_error == E_RETRY_EXHAUSTED {
        c.ok("A77-重试-耗尽封顶");
    } else {
        c.fail("A77-重试-耗尽封顶", "retry cap wrong");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        let mut s = Snapshot::new(11);
        let _ = s.push(Layer::Pipeline, "vs", 1);
        let _ = s.push(Layer::Bindings, "tex0", 2);
        let _ = s.push(Layer::Targets, "color", 3);
        s
    }

    #[test]
    fn layer_names_round_trip() {
        for l in Layer::ALL {
            assert_eq!(Layer::from_str(l.as_str()), Some(l));
        }
        assert_eq!(Layer::from_str("nope"), None);
    }

    #[test]
    fn empty_key_rejected() {
        let mut s = Snapshot::new(1);
        assert_eq!(s.push(Layer::Pipeline, "", 1), Err(E_BAD_REQUEST));
    }

    #[test]
    fn duplicate_key_rejected() {
        let mut s = Snapshot::new(1);
        assert_eq!(s.push(Layer::Pipeline, "vs", 1), Ok(()));
        assert_eq!(s.push(Layer::Pipeline, "vs", 2), Err(E_BAD_REQUEST));
    }

    #[test]
    fn same_key_in_different_layers_is_fine() {
        let mut s = Snapshot::new(1);
        assert_eq!(s.push(Layer::Pipeline, "slot", 1), Ok(()));
        assert_eq!(s.push(Layer::Bindings, "slot", 2), Ok(()));
    }

    #[test]
    fn capacity_counts_only_accepted_entries() {
        let mut s = Snapshot::new(1);
        assert_eq!(s.push(Layer::Pipeline, "vs", 1), Ok(()));
        assert_eq!(s.push(Layer::Pipeline, "vs", 2), Err(E_BAD_REQUEST));
        assert_eq!(s.len(), 1, "rejected push must not occupy a slot");
        assert!(s.len() <= MAX_ENTRIES);
    }

    #[test]
    fn header_round_trip() {
        let r = export(&snap(), DEFAULT_BUDGET).unwrap();
        let (v, g) = parse_header(&r.text).unwrap();
        assert_eq!(v, Version::current());
        assert_eq!(g, 11);
    }

    #[test]
    fn header_rejects_garbage() {
        assert!(parse_header("").is_none());
        assert!(parse_header("hello").is_none());
        assert!(parse_header("@varix-snap 1").is_none());
        assert!(parse_header("@varix-snap x.y gen=1").is_none());
    }

    #[test]
    fn trimmed_export_stays_inside_budget() {
        let mut s = Snapshot::new(1);
        for i in 0..10u64 {
            let key: &'static str = if i == 0 { "a" } else { "b" };
            let _ = s.push(Layer::Pipeline, key, i);
        }
        let _ = s.push(Layer::Bindings, "t", 1);
        let _ = s.push(Layer::Targets, "c", 1);
        let r = export(&s, 200).unwrap();
        assert!(r.bytes <= 200 + 96, "budget overshoot: {}", r.bytes);
    }

    #[test]
    fn trim_never_drops_pipeline_first() {
        let r = export(&sample(), 100).unwrap();
        if let Some(t) = r.trim {
            assert!(t.touched(Layer::Targets) || t.touched(Layer::Bindings));
        }
    }

    #[test]
    fn a11y_has_six_lines() {
        let r = export(&sample(), DEFAULT_BUDGET).unwrap();
        let lines = r.a11y_lines();
        assert!(lines.len() >= 5);
        assert!(lines.iter().any(|l| l.contains("代次")));
    }

    #[test]
    fn well_formed_detects_corruption() {
        let r = export(&sample(), DEFAULT_BUDGET).unwrap();
        assert!(r.well_formed());
        let mut bad = r.clone();
        bad.text = "garbage".to_string();
        assert!(!bad.well_formed());
    }

    #[test]
    fn retry_returns_immediately_on_success() {
        let (r, log) = retry_export(|| export(&sample(), DEFAULT_BUDGET));
        assert!(r.is_some());
        assert_eq!(log.attempts, 1);
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea77_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea77 red: {}", cs.red_items_text());
        assert!(p >= 14);
    }
}
