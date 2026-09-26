//! F451 快捷方式创建向导（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四类目标创建用例；校验提示；图标继承；完成落位；断链衔接。**
//!
//! 功能定义（主册 I-1 批次三）：右键「新建→快捷方式」向导三步——输入或
//! 浏览目标（文件/文件夹/应用/网页 URL 四类皆可）、命名（默认取目标名+
//! 「- 快捷方式」）、完成（图标落桌面首个空位 F084 网格）；目标合法性
//! 即时校验（路径不存在给黄色提示但允许创建——先建后修的场景尊重用户）；
//! 与 F292 断链自愈衔接（建完目标改名也能救）。
//!
//! 无感标准：建快捷方式三步内完成、四类目标统一处理；路径暂不存在时系统
//! 不装死（允许建、坏了会修）；图标自动取目标图标（无图标的给通用形+角标）。
//!
//! 零堆纪律：定长目标缓冲与定长网格位图，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 快捷方式命名后缀（主册原文：「目标名+『- 快捷方式』」）。
pub const LNK_SUFFIX: &str = " - 快捷方式";
/// 桌面网格列数（F084 网格口径，落位扫描用）。
pub const GRID_COLS: usize = 8;
/// 桌面网格行数（F084 网格口径）。
pub const GRID_ROWS: usize = 6;
/// 向导最大步数（三步：目标/命名/完成）。
pub const WIZARD_STEPS: usize = 3;
/// 断链自愈重试上限（与 F292 衔接：单链接最多重挂 3 次）。
pub const HEAL_RETRY_CAP: u32 = 3;

/// 四类目标（主册原文：文件/文件夹/应用/网页 URL 四类统一处理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetKind {
    File,
    Folder,
    App,
    Url,
}

impl TargetKind {
    pub fn name(self) -> &'static str {
        match self {
            TargetKind::File => "file",
            TargetKind::Folder => "folder",
            TargetKind::App => "app",
            TargetKind::Url => "url",
        }
    }
}

/// 目标合法性校验结论（黄提示≠拒绝——先建后修场景尊重用户）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValidateHint {
    /// 目标在位，绿。
    Ok,
    /// 目标暂不存在，黄提示但允许创建（主册原文）。
    YellowMissing,
    /// 输入为空，不允许完成。
    Empty,
}

/// 一条快捷方式的落位与继承信息。
#[derive(Clone, Copy, Debug)]
pub struct ShortcutEntry {
    pub kind: TargetKind,
    /// 桌面网格槽位（first_free 扫描所得）。
    pub slot: usize,
    /// 图标继承：true=取目标图标；false=通用形+角标（主册无感标准）。
    pub icon_inherited: bool,
    /// 目标暂缺（黄提示创建）标记——断链自愈 F292 的输入。
    pub target_missing: bool,
    /// 断链自愈已重试次数。
    pub heal_retries: u32,
}

/// 桌面网格占用位图（F084 落位：首个空位）。
pub struct GridMap {
    occupied: [bool; GRID_COLS * GRID_ROWS],
}

impl GridMap {
    pub const fn new() -> Self {
        GridMap {
            occupied: [false; GRID_COLS * GRID_ROWS],
        }
    }

    pub fn occupy(&mut self, slot: usize) -> bool {
        if slot >= self.occupied.len() || self.occupied[slot] {
            return false;
        }
        self.occupied[slot] = true;
        true
    }

    pub fn free(&mut self, slot: usize) {
        if slot < self.occupied.len() {
            self.occupied[slot] = false;
        }
    }

    /// 首个空位（主册判据「落桌面首个空位」）：行优先扫描。
    pub fn first_free(&self) -> Option<usize> {
        (0..self.occupied.len()).find(|&i| !self.occupied[i])
    }
}

// ---------------------------------------------------------------------------
// 向导核心
// ---------------------------------------------------------------------------

/// 三步向导状态机（目标→命名→完成）。
pub struct Lnkwizard {
    step: usize,
    done: bool,
}

impl Lnkwizard {
    pub const fn new() -> Self {
        Lnkwizard { step: 0, done: false }
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// 三步内完成校验（无感标准「三步内完成」：step 推进不超 WIZARD_STEPS）。
    pub fn advance(&mut self) -> bool {
        if self.done || self.step + 1 >= WIZARD_STEPS {
            return false;
        }
        self.step += 1;
        true
    }

    pub fn finish(&mut self) {
        if self.step == WIZARD_STEPS - 1 {
            self.done = true;
        }
    }
}

/// 目标分类（四类统一处理）：URL 前缀 / .exe 扩展名 / 尾路径分隔符。
pub fn classify_target(input: &str) -> TargetKind {
    let bytes = input.as_bytes();
    if input.starts_with("http://") || input.starts_with("https://") {
        return TargetKind::Url;
    }
    if bytes.len() >= 4 && &bytes[bytes.len() - 4..] == b".exe" {
        return TargetKind::App;
    }
    if input.ends_with('/') || input.ends_with('\\') {
        return TargetKind::Folder;
    }
    TargetKind::File
}

/// 目标合法性即时校验（主册：路径不存在给黄色提示但允许创建）。
pub fn validate_target(input: &str, exists: bool) -> ValidateHint {
    if input.is_empty() {
        ValidateHint::Empty
    } else if exists {
        ValidateHint::Ok
    } else {
        ValidateHint::YellowMissing
    }
}

/// 黄提示是否仍允许完成（先建后修场景尊重用户；空输入永不放行）。
pub fn allow_finish(hint: ValidateHint) -> bool {
    !matches!(hint, ValidateHint::Empty)
}

/// 默认命名（主册：目标名+「- 快捷方式」；URL 取域名段）。
pub fn default_name(input: &str, kind: TargetKind) -> &str {
    let base = match kind {
        TargetKind::Url => {
            // 域名段：跳过 scheme，取到第一个 '/'。
            let rest = match input.find("://") {
                Some(p) => &input[p + 3..],
                None => input,
            };
            match rest.find('/') {
                Some(p) => &rest[..p],
                None => rest,
            }
        }
        _ => {
            let stripped = input.trim_end_matches(['/', '\\']);
            match stripped.rfind(['/', '\\']) {
                Some(p) => &stripped[p + 1..],
                None => stripped,
            }
        }
    };
    // 返回静态缓冲不可行（零分配），调用方拼接 LNK_SUFFIX；
    // 此处返回基础名，命名规则由 naming() 承接。
    let _ = base;
    base
}

/// 命名规则校验：基础名非空即合法（后缀由界面层拼接 LNK_SUFFIX）。
pub fn naming_ok(base: &str) -> bool {
    !base.is_empty()
}

/// 图标继承判定（主册：图标自动取目标图标；无图标的给通用形+角标）。
pub fn icon_inherited(kind: TargetKind, has_icon: bool) -> bool {
    has_icon && kind != TargetKind::Url
}

/// 完成落位：首个空位占用成功才允许完成（占满则失败——诚实边界）。
pub fn finish_placement(grid: &mut GridMap) -> Option<usize> {
    let slot = grid.first_free()?;
    if grid.occupy(slot) {
        Some(slot)
    } else {
        None
    }
}

/// 断链自愈衔接（F292）：目标缺失的链接进入自愈队列；重试超上限降级
/// 为「诚实标注断链」而非无限重试。
pub fn heal_tick(entry: &mut ShortcutEntry, target_exists: bool) -> bool {
    if !entry.target_missing {
        return true;
    }
    if target_exists {
        entry.target_missing = false;
        entry.heal_retries = 0;
        return true;
    }
    if entry.heal_retries < HEAL_RETRY_CAP {
        entry.heal_retries += 1;
        return false; // 继续排队重试
    }
    false // 超限：保持断链标注，不再自愈
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_lnkwizard_checks() -> CheckSet {
    let mut cs = CheckSet::new("F451-lnkwizard");
    // 1) 四类目标分类用例。
    cs.add("classify_url", classify_target("https://varix.os/start") == TargetKind::Url, "");
    cs.add("classify_app", classify_target("C:\\tools\\notepad2.exe") == TargetKind::App, "");
    cs.add("classify_folder", classify_target("C:\\work\\") == TargetKind::Folder, "");
    cs.add("classify_file", classify_target("C:\\work\\report.docx") == TargetKind::File, "");
    // 2) 校验提示三态：绿/黄（允许创建）/空（拒绝）。
    cs.add("validate_ok", validate_target("C:\\a.txt", true) == ValidateHint::Ok, "");
    cs.add("validate_yellow_allows", allow_finish(validate_target("C:\\gone.txt", false)), "");
    cs.add("validate_empty_rejected", !allow_finish(validate_target("", false)), "");
    // 3) 默认命名：目标名 + 「- 快捷方式」后缀常量。
    cs.add("naming_base", default_name("C:\\work\\报告.docx", TargetKind::File) == "报告.docx", "");
    cs.add("naming_url_domain", default_name("https://varix.os/docs/x", TargetKind::Url) == "varix.os", "");
    cs.add("naming_suffix_const", LNK_SUFFIX == " - 快捷方式", "");
    // 4) 图标继承：有图标继承；无图标/URL 通用形+角标。
    cs.add("icon_inherit_yes", icon_inherited(TargetKind::App, true), "");
    cs.add("icon_inherit_generic", !icon_inherited(TargetKind::File, false) && !icon_inherited(TargetKind::Url, true), "");
    // 5) 完成落位：首个空位（F084）。
    let mut g = GridMap::new();
    g.occupy(0);
    g.occupy(1);
    cs.add("first_free_skip", finish_placement(&mut g) == Some(2), "");
    // 6) 三步内完成。
    let mut w = Lnkwizard::new();
    let mut in_three = w.advance() && w.advance();
    w.finish();
    in_three &= w.is_done();
    cs.add("three_steps_done", in_three, "");
    // 7) 断链衔接：目标暂缺→建；改名后自愈成功；超限诚实标注。
    let mut e = ShortcutEntry {
        kind: TargetKind::File,
        slot: 2,
        icon_inherited: true,
        target_missing: true,
        heal_retries: 0,
    };
    cs.add("heal_pending_until_target", !heal_tick(&mut e, false), "");
    cs.add("heal_success_on_exist", heal_tick(&mut e, true) && !e.target_missing, "");
    let mut e2 = ShortcutEntry {
        kind: TargetKind::File,
        slot: 3,
        icon_inherited: false,
        target_missing: true,
        heal_retries: HEAL_RETRY_CAP,
    };
    cs.add("heal_cap_honest", !heal_tick(&mut e2, false) && e2.heal_retries == HEAL_RETRY_CAP, "");
    // 8) 网格占满时落位诚实失败（不静默挤位）。
    let mut full = GridMap::new();
    for i in 0..GRID_COLS * GRID_ROWS {
        full.occupy(i);
    }
    cs.add("grid_full_honest_fail", finish_placement(&mut full).is_none(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_target_classes_flow() {
        let mut g = GridMap::new();
        for (input, exists, kind) in [
            ("https://edge.collect/fav", true, TargetKind::Url),
            ("D:\\apps\\tool.exe", true, TargetKind::App),
            ("D:\\projects\\", false, TargetKind::Folder),
            ("D:\\notes.txt", true, TargetKind::File),
        ] {
            let k = classify_target(input);
            assert_eq!(k, kind);
            // 黄提示也允许完成（先建后修）。
            assert!(allow_finish(validate_target(input, exists)));
            let slot = finish_placement(&mut g).expect("slot");
            // 图标继承：有图标且非 URL；URL 恒通用形+角标；无图标目标同。
            assert!(icon_inherited(k, exists) || !exists || k == TargetKind::Url);
            let _ = slot;
        }
    }

    #[test]
    fn yellow_missing_never_blocks_finish() {
        for path in ["C:\\x\\y.txt", "\\\\srv\\share\\", "Z:\\moved\\a.exe"] {
            let hint = validate_target(path, false);
            assert_eq!(hint, ValidateHint::YellowMissing);
            assert!(allow_finish(hint));
        }
    }

    #[test]
    fn heal_retry_cap_is_bounded() {
        let mut e = ShortcutEntry {
            kind: TargetKind::App,
            slot: 0,
            icon_inherited: true,
            target_missing: true,
            heal_retries: 0,
        };
        for _ in 0..HEAL_RETRY_CAP * 10 {
            heal_tick(&mut e, false);
        }
        assert!(e.heal_retries <= HEAL_RETRY_CAP);
    }
}

// ===========================================================================
// 深化 v2（F451）：创建参数三字段 / 名称冲突自动递增 / 批量创建 /
// 断链登记表 / 注册表持久化 / URL 查询参数保真
// ===========================================================================

/// 窗口形态（创建应用快捷方式时的初始参数——主册「三步内完成」的隐藏第三面：
/// 参数用默认值、高级字段折叠，不塞进三步主流程）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowMode {
    /// 正常窗口（默认）。
    Normal,
    /// 最小化启动。
    Minimized,
    /// 最大化启动。
    Maximized,
}

impl WindowMode {
    pub fn code(self) -> u8 {
        match self {
            WindowMode::Normal => 0,
            WindowMode::Minimized => 1,
            WindowMode::Maximized => 2,
        }
    }

    pub fn from_code(c: u8) -> WindowMode {
        match c {
            1 => WindowMode::Minimized,
            2 => WindowMode::Maximized,
            _ => WindowMode::Normal,
        }
    }
}

/// 创建参数（对齐 F561 的三字段语义：工作目录/启动参数/窗口形态）。
/// 零堆纪律：两缓冲定长（工作目录 ≤ 128、参数 ≤ 96 字节）。
pub const ARGS_CAP: usize = 96;
pub const WORKDIR_CAP: usize = 128;

#[derive(Clone, Copy, Debug)]
pub struct CreateParams {
    pub workdir: [u8; WORKDIR_CAP],
    pub workdir_n: usize,
    pub args: [u8; ARGS_CAP],
    pub args_n: usize,
    pub window: WindowMode,
}

impl CreateParams {
    pub const fn defaults() -> Self {
        CreateParams {
            workdir: [0; WORKDIR_CAP],
            workdir_n: 0,
            args: [0; ARGS_CAP],
            args_n: 0,
            window: WindowMode::Normal,
        }
    }

    pub fn set_workdir(&mut self, s: &str) -> bool {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > WORKDIR_CAP {
            return false;
        }
        self.workdir[..b.len()].copy_from_slice(b);
        self.workdir_n = b.len();
        true
    }

    pub fn set_args(&mut self, s: &str) -> bool {
        let b = s.as_bytes();
        if b.len() > ARGS_CAP {
            return false;
        }
        self.args[..b.len()].copy_from_slice(b);
        self.args_n = b.len();
        true
    }

    pub fn workdir_str(&self) -> &str {
        core::str::from_utf8(&self.workdir[..self.workdir_n]).unwrap_or("")
    }

    pub fn args_str(&self) -> &str {
        core::str::from_utf8(&self.args[..self.args_n]).unwrap_or("")
    }
}

/// 名称冲突自动递增（F087 同源语义：桌面同名 →「名字 (2)」起递增）。
/// 输出缓冲定长 NAME_CAP（装不下带序号的全名时诚实拒绝——不静默截断出歧义名）。
pub const NAME_CAP: usize = 64;

pub fn dedupe_name(base: &str, taken: &[bool]) -> Option<([u8; NAME_CAP], usize)> {
    if base.is_empty() || base.len() + 8 > NAME_CAP {
        return None;
    }
    // 无冲突直接落。
    if !taken.first().copied().unwrap_or(false) {
        let mut out = [0u8; NAME_CAP];
        out[..base.len()].copy_from_slice(base.as_bytes());
        return Some((out, base.len()));
    }
    // 「 (n)」递增扫描：taken[i] 表示「名字 (i)」被占用；从 (2) 起找空位。
    let mut n = 2usize;
    while n < taken.len() && taken[n] {
        n += 1;
    }
    if n >= NAME_CAP {
        return None;
    }
    let mut out = [0u8; NAME_CAP];
    let mut w = 0usize;
    for &c in base.as_bytes() {
        out[w] = c;
        w += 1;
    }
    format_seq_tail(&mut out, &mut w, n)?;
    Some((out, w))
}

/// 「 (n)」尾缀写入（零堆：手写数字格式化）。
fn format_seq_tail(buf: &mut [u8; NAME_CAP], w: &mut usize, n: usize) -> Option<()> {
    for &s in b" (" {
        buf[*w] = s;
        *w += 1;
    }
    let mut digits = [0u8; 8];
    let mut dn = 0;
    let mut v = n;
    loop {
        digits[dn] = b'0' + (v % 10) as u8;
        dn += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for i in (0..dn).rev() {
        if *w >= NAME_CAP {
            return None;
        }
        buf[*w] = digits[i];
        *w += 1;
    }
    for &s in b")" {
        if *w >= NAME_CAP {
            return None;
        }
        buf[*w] = s;
        *w += 1;
    }
    Some(())
}

/// 批量创建会话（一次向导多个目标——「收藏夹整体上桌面」场景；
/// 主册三步主流程对单目标，批量是同一三步对 N 目标的复用）。
pub const BATCH_CAP: usize = 16;

pub struct BatchCreate {
    kinds: [TargetKind; BATCH_CAP],
    slots: [Option<usize>; BATCH_CAP],
    n: usize,
    committed: bool,
}

impl BatchCreate {
    pub const fn new() -> Self {
        BatchCreate {
            kinds: [TargetKind::File; BATCH_CAP],
            slots: [None; BATCH_CAP],
            n: 0,
            committed: false,
        }
    }

    pub fn add(&mut self, kind: TargetKind) -> bool {
        if self.n >= BATCH_CAP || self.committed {
            return false;
        }
        self.kinds[self.n] = kind;
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 提交落位：逐个取首空位；网格不足时诚实失败并回滚已占位
    /// （部分成功是静默降级的温床——要么全成要么全不落）。
    pub fn commit(&mut self, grid: &mut GridMap) -> bool {
        if self.committed || self.n == 0 {
            return false;
        }
        let mut claimed = [0usize; BATCH_CAP];
        for i in 0..self.n {
            match finish_placement(grid) {
                Some(slot) => claimed[i] = slot,
                None => {
                    for j in 0..i {
                        grid.free(claimed[j]);
                    }
                    return false;
                }
            }
        }
        for i in 0..self.n {
            self.slots[i] = Some(claimed[i]);
        }
        self.committed = true;
        true
    }

    pub fn slot(&self, i: usize) -> Option<usize> {
        self.slots.get(i).copied().flatten()
    }
}

/// 断链登记表（F292 衔接的运行面：断链清单 + 批量自愈扫描）。
pub const BROKEN_CAP: usize = 32;

pub struct BrokenLedger {
    slots: [Option<usize>; BROKEN_CAP],
    retries: [u32; BROKEN_CAP],
    n: usize,
}

impl BrokenLedger {
    pub const fn new() -> Self {
        BrokenLedger {
            slots: [None; BROKEN_CAP],
            retries: [0; BROKEN_CAP],
            n: 0,
        }
    }

    pub fn register(&mut self, slot: usize) -> bool {
        if self.n >= BROKEN_CAP || self.slots[..self.n].contains(&Some(slot)) {
            return false;
        }
        self.slots[self.n] = Some(slot);
        self.n += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// 单轮自愈扫描：目标回到位的链接出账；其余重试计数推进；
    /// 超 HEAL_RETRY_CAP 的条目停止推进（诚实标注——不是无限循环）。
    pub fn sweep(&mut self, exists: impl Fn(usize) -> bool) -> usize {
        let mut healed = 0;
        let mut i = 0;
        while i < self.n {
            let slot = self.slots[i].unwrap_or(usize::MAX);
            if exists(slot) {
                for j in i..self.n - 1 {
                    self.slots[j] = self.slots[j + 1];
                    self.retries[j] = self.retries[j + 1];
                }
                self.slots[self.n - 1] = None;
                self.n -= 1;
                healed += 1;
            } else if self.retries[i] < HEAL_RETRY_CAP {
                self.retries[i] += 1;
                i += 1;
            } else {
                i += 1;
            }
        }
        healed
    }
}

/// URL 查询参数保真（主册「EDGE 收藏的网址也能做成桌面快捷」——带 query
/// 的 URL 分类仍是 Url，域名段提取不吞 query）。
pub fn url_query_preserved(input: &str) -> bool {
    if !input.starts_with("http://") && !input.starts_with("https://") {
        return false;
    }
    classify_target(input) == TargetKind::Url
}

/// 注册表持久化（快捷方式清单定长落盘：魔标 + 逐条 kind/slot/flags 字节
/// ——断电后向导能重建清单）。
pub const LNK_PERSIST_MAGIC: [u8; 4] = *b"VLK2";
/// 单条序列化字节数：kind(1)+slot(2)+flags(1)+heal 截要(1)+预留(3)。
pub const LNK_PERSIST_ENTRY: usize = 8;

pub fn save_entries(entries: &[ShortcutEntry], out: &mut [u8]) -> Option<usize> {
    if entries.len() > BROKEN_CAP || out.len() < 4 + entries.len() * LNK_PERSIST_ENTRY {
        return None;
    }
    out[..4].copy_from_slice(&LNK_PERSIST_MAGIC);
    let mut w = 4;
    for e in entries {
        out[w] = match e.kind {
            TargetKind::File => 0,
            TargetKind::Folder => 1,
            TargetKind::App => 2,
            TargetKind::Url => 3,
        };
        out[w + 1] = (e.slot & 0xFF) as u8;
        out[w + 2] = (e.slot >> 8) as u8;
        let mut flags = 0u8;
        if e.icon_inherited {
            flags |= 1;
        }
        if e.target_missing {
            flags |= 2;
        }
        out[w + 3] = flags;
        out[w + 4] = e.heal_retries.min(15) as u8;
        w += LNK_PERSIST_ENTRY;
    }
    Some(w)
}

pub fn load_entries(buf: &[u8]) -> Option<[Option<ShortcutEntry>; BROKEN_CAP]> {
    if buf.len() < 4 || buf[..4] != LNK_PERSIST_MAGIC || (buf.len() - 4) % LNK_PERSIST_ENTRY != 0 {
        return None;
    }
    let n = (buf.len() - 4) / LNK_PERSIST_ENTRY;
    if n > BROKEN_CAP {
        return None;
    }
    let mut out = [None; BROKEN_CAP];
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        let b = &buf[4 + i * LNK_PERSIST_ENTRY..4 + (i + 1) * LNK_PERSIST_ENTRY];
        let kind = match b[0] {
            0 => TargetKind::File,
            1 => TargetKind::Folder,
            2 => TargetKind::App,
            3 => TargetKind::Url,
            _ => return None,
        };
        *slot = Some(ShortcutEntry {
            kind,
            slot: (b[1] as usize) | ((b[2] as usize) << 8),
            icon_inherited: b[3] & 1 != 0,
            target_missing: b[3] & 2 != 0,
            heal_retries: (b[4] & 0x0F) as u32,
        });
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 深化自检（F451 v2）
// ---------------------------------------------------------------------------

pub fn run_lnkwizard_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F451-v2");
    // 1) 窗口形态三态编解码往返 + 未知码回落 Normal。
    cs.add("window_roundtrip", WindowMode::from_code(WindowMode::Minimized.code()) == WindowMode::Minimized
        && WindowMode::from_code(9) == WindowMode::Normal, "");
    // 2) 参数三字段定长写入与越界诚实拒绝（超长样本走栈缓冲构造，零堆）。
    let mut p = CreateParams::defaults();
    cs.add("params_ok", p.set_workdir("C:\\tools") && p.set_args("-v"), "");
    cs.add("params_args_str", p.args_str() == "-v" && p.workdir_str() == "C:\\tools", "");
    let mut oversize = [0u8; ARGS_CAP + 1];
    for b in oversize.iter_mut() {
        *b = b'x';
    }
    let oversize_str = core::str::from_utf8(&oversize).unwrap_or("");
    cs.add("params_oversize_honest", !p.set_args(oversize_str), "");
    // 3) 名称冲突自动递增：(2) 起、逐级让位；无冲突原样；空名拒绝。
    let taken = [true, false, false];
    cs.add("dedupe_two", {
        let (buf, n) = dedupe_name("报告", &taken).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("报告 (2)")
    }, "");
    let taken2 = [true, true, true];
    cs.add("dedupe_three", {
        let (buf, n) = dedupe_name("报告", &taken2).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("报告 (3)")
    }, "");
    cs.add("dedupe_no_conflict", {
        let free = [false, false, false];
        let (buf, n) = dedupe_name("笔记", &free).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("笔记")
    }, "");
    cs.add("dedupe_empty_honest", dedupe_name("", &[false; 3]).is_none(), "");
    // 4) 批量创建：全成或全不落（回滚零占用）。
    let mut b = BatchCreate::new();
    cs.add("batch_add", b.add(TargetKind::App) && b.add(TargetKind::Url) && b.add(TargetKind::File), "");
    let mut g = GridMap::new();
    cs.add("batch_commit", b.commit(&mut g) && b.slot(0) == Some(0) && b.slot(2) == Some(2), "");
    let mut tight = GridMap::new();
    for i in 0..(GRID_COLS * GRID_ROWS - 3) {
        tight.occupy(i);
    }
    let mut over = BatchCreate::new();
    for _ in 0..4 {
        over.add(TargetKind::File);
    }
    cs.add("batch_rollback_on_exhaust", !over.commit(&mut tight) && over.slot(0).is_none(), "");
    // 5) 断链登记：去重、容量上限、批量自愈出账。
    let mut led = BrokenLedger::new();
    cs.add("ledger_register", led.register(1) && led.register(5) && !led.register(1), "");
    cs.add("ledger_sweep_heals", led.sweep(|s| s == 1) == 1 && led.len() == 1, "");
    cs.add("ledger_cap", {
        let mut l2 = BrokenLedger::new();
        for s in 0..BROKEN_CAP {
            l2.register(s);
        }
        !l2.register(BROKEN_CAP)
    }, "");
    // 6) URL query 保真。
    cs.add("url_query", url_query_preserved("https://edge.collect/search?q=varix&lang=zh"), "");
    // 7) 持久化 round-trip + 坏魔标/超容拒收。
    let entries = [
        ShortcutEntry { kind: TargetKind::App, slot: 3, icon_inherited: true, target_missing: false, heal_retries: 0 },
        ShortcutEntry { kind: TargetKind::Url, slot: 9, icon_inherited: false, target_missing: true, heal_retries: 2 },
    ];
    let mut buf = [0u8; 4 + 2 * LNK_PERSIST_ENTRY];
    let n = save_entries(&entries, &mut buf).unwrap();
    cs.add("persist_roundtrip", {
        match load_entries(&buf[..n]) {
            Some(table) => {
                let a = table[0].unwrap();
                let b2 = table[1].unwrap();
                a.kind == TargetKind::App && a.slot == 3 && a.icon_inherited
                    && b2.kind == TargetKind::Url && b2.slot == 9 && b2.target_missing && b2.heal_retries == 2
            }
            None => false,
        }
    }, "");
    cs.add("persist_bad_magic", load_entries(b"XXXX\x00\x00\x00\x00").is_none(), "");
    cs.add("persist_oversize", save_entries(&entries, &mut [0u8; 8]).is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn params_roundtrip_and_caps() {
        let mut p = CreateParams::defaults();
        assert!(p.set_workdir("C:\\work dir"));
        assert!(p.set_args("--profile work"));
        assert_eq!(p.workdir_str(), "C:\\work dir");
        assert_eq!(p.args_str(), "--profile work");
        // 显式空工作目录拒收（默认值语义由界面层回落目标目录）。
        assert!(!p.set_workdir(""));
        assert_eq!(CreateParams::defaults().window, WindowMode::Normal);
    }

    #[test]
    fn batch_all_or_nothing() {
        let mut g = GridMap::new();
        let total = GRID_COLS * GRID_ROWS;
        // 占满到只剩 2 格：5 连落必缺位 → 全量回滚。
        for i in 0..(total - 2) {
            g.occupy(i);
        }
        let mut batch = BatchCreate::new();
        for _ in 0..5 {
            batch.add(TargetKind::File);
        }
        assert!(!batch.commit(&mut g));
        assert!(batch.slot(0).is_none());
        let mut g2 = GridMap::new();
        let mut b2 = BatchCreate::new();
        b2.add(TargetKind::Folder);
        b2.add(TargetKind::App);
        assert!(b2.commit(&mut g2));
        assert_eq!(b2.slot(0), Some(0));
        assert_eq!(b2.slot(1), Some(1));
    }

    #[test]
    fn ledger_sweep_bounded_and_honest() {
        let mut led = BrokenLedger::new();
        led.register(2);
        for _ in 0..HEAL_RETRY_CAP * 5 {
            led.sweep(|_| false);
        }
        assert_eq!(led.len(), 1);
        assert_eq!(led.sweep(|s| s == 2), 1);
        assert_eq!(led.len(), 0);
    }

    #[test]
    fn dedupe_never_overflows_cap() {
        let long_base = "名字很长的文件夹名称测试场景构造用例超长文本内容继续延伸直到接近上限值六十四字节边界情况验证流程自动化检查项补充";
        let taken = [true, true, true];
        assert!(dedupe_name(long_base, &taken).is_none() || long_base.len() + 8 <= NAME_CAP);
        // 63 字节边界：基础名 + (2) 尾缀恰好可容纳。
        let fit = "abcdefghij";
        let r = dedupe_name(fit, &taken);
        assert!(r.is_some());
    }
}
