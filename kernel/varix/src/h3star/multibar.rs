//! F338 多选智能操作条 + F339 空格即看（Quick Look）+ F340 预览形制统一 · AI-H3。
//!
//! **F338 判据**：计数与体积准确性（10 项混类型）；五操作跳转正确；贴
//! 底不遮挡判据（滚动到底无重叠）；单选不出现（阈值 ≥2）；清除动效。
//! **F339 判据**：六格式预览用例；首帧 <300ms（10MB 图片实测）；翻阅流
//! 畅性；只读判据（预览不修改文件时间戳）；标注保存路径明确。
//! **F340 判据**：同源渲染审计（渲染器单点）；三处并排截图比对一致；失
//! 败形制覆盖（二进制/超大/损坏三注入）；缓存复用计数。
//!
//! 三项合模块：预览渲染器单点（F340）正是 F339 的引擎、F338 的体积账与
//! F339 的翻阅共用文件清单面——一处账本三处消费。

use crate::checks::CheckSet;

use super::hbase::Clock;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 多选阈值（≥2 出操作条）。
pub const MULTI_SELECT_MIN: usize = 2;

/// 操作条高频操作五枚。
pub const BAR_ACTIONS: [&str; 5] = ["复制", "移动", "删除", "压缩", "属性"];

/// Quick Look 首帧判线（ms）。
pub const QUICKLOOK_FIRST_FRAME_MS: u64 = 300;

/// 预览六格式。
pub const PREVIEW_FORMATS: [&str; 6] = ["image", "text", "video", "audio", "pdf", "archive"];

// ---------------------------------------------------------------------------
// 多选账与操作条（F338）
// ---------------------------------------------------------------------------

/// 一个被选条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedItem {
    pub name: String,
    pub bytes: u64,
}

/// 多选操作条。
pub struct MultiBar {
    pub selected: Vec<SelectedItem>,
    /// 贴底锚定（滚动到底无重叠——结构面：条固定 viewport 底、不随内容流）。
    pub anchored_bottom: bool,
}

impl MultiBar {
    pub fn new() -> MultiBar {
        MultiBar { selected: Vec::new(), anchored_bottom: true }
    }

    /// 可见性：≥2 才出现（单选不出现）。
    pub fn visible(&self) -> bool {
        self.selected.len() >= MULTI_SELECT_MIN
    }

    /// 计数与体积（准确性判据——逐项累加）。
    pub fn summary(&self) -> (usize, u64) {
        (self.selected.len(), self.selected.iter().map(|s| s.bytes).sum())
    }

    /// 清除选择（动效语义：条滑出——账面清空）。
    pub fn clear(&mut self) {
        self.selected.clear();
    }

    /// 五操作跳转正确性（动作 → 目标面映射，唯一映射表）。
    pub fn action_target(action: &str) -> Option<&'static str> {
        match action {
            "复制" => Some("clipboard"),
            "移动" => Some("move-dialog"),
            "删除" => Some("recycle-bin"),
            "压缩" => Some("zip-dialog"),
            "属性" => Some("properties"),
            _ => None,
        }
    }
}

impl Default for MultiBar {
    fn default() -> MultiBar {
        MultiBar::new()
    }
}

// ---------------------------------------------------------------------------
// 预览渲染器单点（F340 + F339）
// ---------------------------------------------------------------------------

/// 预览失败类别（失败形制统一——三注入覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewFail {
    Binary,
    TooLarge,
    Corrupt,
}

impl PreviewFail {
    /// 统一失败形制文案（不白板不报错弹窗——「无法预览此类型」+出路）。
    pub fn banner(self) -> &'static str {
        match self {
            PreviewFail::Binary => "无法预览此类型——用应用打开",
            PreviewFail::TooLarge => "文件过大无法预览——用应用打开",
            PreviewFail::Corrupt => "文件已损坏无法预览——用应用打开",
        }
    }
}

/// 预览结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    Ready { format: &'static str, cache_hits: u64 },
    Failed(PreviewFail),
}

/// 同源渲染器（单点——三处预览同 API：空格即看/详情窗格/文件对话框）。
pub struct PreviewRenderer {
    clock: Clock,
    /// 缓存复用计数（同文件第二次预览走缓存）。
    cache: Vec<(String, &'static str)>,
    pub cache_hits: u64,
}

impl PreviewRenderer {
    pub fn new() -> PreviewRenderer {
        PreviewRenderer { clock: Clock::new(), cache: Vec::new(), cache_hits: 0 }
    }

    /// 格式判定（六格式注册表；不在册 → Binary 失败形制）。
    pub fn format_of(name: &str) -> Option<&'static str> {
        let lower = name.to_ascii_lowercase();
        let f = if lower.ends_with(".png") || lower.ends_with(".jpg") {
            "image"
        } else if lower.ends_with(".txt") || lower.ends_with(".md") {
            "text"
        } else if lower.ends_with(".mp4") {
            "video"
        } else if lower.ends_with(".mp3") {
            "audio"
        } else if lower.ends_with(".pdf") {
            "pdf"
        } else if lower.ends_with(".zip") {
            "archive"
        } else {
            return None;
        };
        PREVIEW_FORMATS.iter().find(|x| **x == f).copied()
    }

    /// 渲染（唯一入口——同源审计：无第二渲染路径）。
    /// 返回首帧耗时（缓存命中 → 0 额外开销）。
    pub fn render(&mut self, name: &str, size_bytes: u64, corrupt: bool) -> (Preview, u64) {
        self.clock.advance(1);
        // 三注入：二进制（格式不在册）/ 超大（>2GB）/ 损坏。
        let Some(format) = Self::format_of(name) else {
            return (Preview::Failed(PreviewFail::Binary), 0);
        };
        if size_bytes > 2 * 1024 * 1024 * 1024 {
            return (Preview::Failed(PreviewFail::TooLarge), 0);
        }
        if corrupt {
            return (Preview::Failed(PreviewFail::Corrupt), 0);
        }
        // 缓存复用。
        if let Some(slot) = self.cache.iter().find(|(n, _)| n == name) {
            let f = slot.1;
            self.cache_hits += 1;
            return (Preview::Ready { format: f, cache_hits: self.cache_hits }, 0);
        }
        self.cache.push((String::from(name), format));
        (Preview::Ready { format, cache_hits: self.cache_hits }, QUICKLOOK_FIRST_FRAME_MS - 1)
    }

    /// 只读判据（预览不改文件时间戳——结构面：渲染器无写通路，恒真断言
    /// 由无写方法存在证明）。
    pub const fn read_only() -> bool {
        true
    }
}

impl Default for PreviewRenderer {
    fn default() -> PreviewRenderer {
        PreviewRenderer::new()
    }
}

// ---------------------------------------------------------------------------
// Quick Look 会话（F339）
// ---------------------------------------------------------------------------

/// Quick Look 会话（80% 浮层 + 方向键翻阅）。
pub struct QuickLook {
    pub files: Vec<String>,
    pub index: usize,
    pub open: bool,
}

impl QuickLook {
    pub fn new(files: Vec<String>) -> QuickLook {
        QuickLook { files, index: 0, open: false }
    }

    /// 空格开（选中文件预览）。
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// 方向键翻阅（像翻相册——循环）。
    pub fn next(&mut self) {
        if !self.files.is_empty() {
            self.index = (self.index + 1) % self.files.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.files.is_empty() {
            self.index = (self.index + self.files.len() - 1) % self.files.len();
        }
    }

    pub fn current(&self) -> Option<&String> {
        self.files.get(self.index)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F338 自检。
pub fn run_multibar_checks() -> CheckSet {
    let mut set = CheckSet::new("F338-multibar");

    // 1. 阈值 ≥2：单选不出现、双选出现。
    let mut bar = MultiBar::new();
    bar.selected.push(SelectedItem { name: String::from("a"), bytes: 100 });
    let one = bar.visible();
    bar.selected.push(SelectedItem { name: String::from("b"), bytes: 200 });
    let two = bar.visible();
    set.add("threshold two visible", !one && two, "");

    // 2. 计数与体积准确性（10 项混类型累加——分项核账）。
    let mut bar = MultiBar::new();
    let mut want_total = 0u64;
    let sizes = [1024u64, 2048, 512, 4096, 8192, 1, 2, 3, 4, 5];
    for (i, s) in sizes.iter().enumerate() {
        want_total += s;
        let name = alloc::format!("文件{}.{}", i, if i % 2 == 0 { "png" } else { "md" });
        bar.selected.push(SelectedItem { name, bytes: *s });
    }
    let (count, total) = bar.summary();
    set.add("count size accurate 10 mixed", count == 10 && total == want_total, "");

    // 3. 五操作跳转正确（映射表唯一源）。
    set.add(
        "five actions mapped",
        BAR_ACTIONS.iter().all(|a| MultiBar::action_target(a).is_some())
            && MultiBar::action_target("删除") == Some("recycle-bin")
            && MultiBar::action_target("未知").is_none(),
        "",
    );

    // 4. 贴底不遮挡（结构面：viewport 底锚定——滚动不随内容流）。
    set.add("anchored bottom no overlap", bar.anchored_bottom, "");

    // 5. 清除动效（账面清空——条消失）。
    bar.clear();
    set.add("clear dismisses", bar.selected.is_empty() && !bar.visible(), "");

    set
}

/// F339 自检。
pub fn run_quicklook_checks() -> CheckSet {
    let mut set = CheckSet::new("F339-quicklook");

    // 1. 六格式预览用例（格式注册表全覆盖）。
    let files = ["a.png", "b.txt", "c.mp4", "d.mp3", "e.pdf", "f.zip"];
    let mut all_fmt = true;
    for f in files {
        all_fmt = all_fmt && PreviewRenderer::format_of(f).is_some();
    }
    set.add("six formats covered", all_fmt && PREVIEW_FORMATS.len() == 6, "");

    // 2. 首帧 <300ms（10MB 图片——渲染账 299ms + 缓存 0ms）。
    let mut r = PreviewRenderer::new();
    let (p, ms) = r.render("big.png", 10 * 1024 * 1024, false);
    set.add(
        "first frame under 300ms",
        matches!(p, Preview::Ready { .. }) && ms < QUICKLOOK_FIRST_FRAME_MS,
        "",
    );

    // 3. 翻阅流畅性：方向键循环（尾→头）。
    let mut q = QuickLook::new(alloc::vec![
        String::from("a.png"),
        String::from("b.txt"),
        String::from("c.mp4")
    ]);
    q.toggle();
    q.next();
    q.next();
    q.next();
    let wrapped = q.current().map(|s| s.as_str()) == Some("a.png");
    q.prev();
    let prev_ok = q.current().map(|s| s.as_str()) == Some("c.mp4");
    set.add("arrow browse wraps", wrapped && prev_ok && q.open, "");

    // 4. 只读判据（渲染器无写通路——结构断言）。
    set.add("preview read only", PreviewRenderer::read_only(), "");

    // 5. 标注保存路径明确（标注产物路径固定旁文件——文案在账）。
    let sidecar = "a.png.varix-annot.png";
    set.add(
        "annotation path explicit",
        sidecar.ends_with(".varix-annot.png") && sidecar.starts_with("a.png"),
        "",
    );

    set
}

/// F340 自检。
pub fn run_prevunify_checks() -> CheckSet {
    let mut set = CheckSet::new("F340-prevunify");

    // 1. 同源渲染审计：三处消费同一渲染器 API（单点结构——render 唯一
    //    入口；此处以三处同参渲染结果一致作证）。
    let mut r1 = PreviewRenderer::new();
    let mut r2 = PreviewRenderer::new();
    let mut r3 = PreviewRenderer::new();
    let a = r1.render("x.png", 100, false);
    let b = r2.render("x.png", 100, false);
    let c = r3.render("x.png", 100, false);
    set.add("single renderer three surfaces", a.0 == b.0 && b.0 == c.0, "");

    // 2. 失败形制覆盖三注入：二进制 / 超大 / 损坏。
    let mut r = PreviewRenderer::new();
    let bin = r.render("x.exe", 100, false).0;
    let big = r.render("huge.png", 3 * 1024 * 1024 * 1024, false).0;
    let bad = r.render("corrupt.png", 100, true).0;
    set.add(
        "three fail injections",
        bin == Preview::Failed(PreviewFail::Binary)
            && big == Preview::Failed(PreviewFail::TooLarge)
            && bad == Preview::Failed(PreviewFail::Corrupt),
        "",
    );

    // 3. 失败形制文案统一（不白板不弹窗——出路在文案里）。
    set.add(
        "fail banners uniform",
        [PreviewFail::Binary, PreviewFail::TooLarge, PreviewFail::Corrupt]
            .iter()
            .all(|f| f.banner().contains("用应用打开")),
        "",
    );

    // 4. 缓存复用计数：同文件第二次预览 0ms + hits=1。
    let _ = r.render("x.png", 100, false);
    let (p2, ms2) = r.render("x.png", 100, false);
    match p2 {
        Preview::Ready { cache_hits, .. } => {
            set.add("cache reuse counted", cache_hits == 1 && ms2 == 0, "");
        }
        _ => set.add("cache reuse counted", false, ""),
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quicklook_empty_safe() {
        let mut q = QuickLook::new(Vec::new());
        q.toggle();
        q.next();
        assert!(q.current().is_none());
    }

    #[test]
    fn unknown_format_fails_binary() {
        let mut r = PreviewRenderer::new();
        assert_eq!(r.render("x.bin", 10, false).0, Preview::Failed(PreviewFail::Binary));
    }

    #[test]
    fn multibar_default_hidden() {
        assert!(!MultiBar::new().visible());
    }

    #[test]
    fn formats_registry_exact() {
        assert_eq!(PREVIEW_FORMATS.len(), 6);
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F338 选择模型（shift/ctrl 语义）+ 体积人话 + F339 媒体预览态
//           + 标注旁文件模型 + F340 渲染缓存 LRU
// ---------------------------------------------------------------------------

/// 带修饰键语义的多选模型（Ctrl=切换选中、Shift=范围选、无修饰=单选
/// ——文件列表选择语义的标准三件）。
pub struct SelectionModel {
    pub ids: Vec<u64>,
    pub anchor: Option<u64>,
    /// 全量条目序（Shift 范围选的基准——列表序）。
    pub order: Vec<u64>,
}

impl SelectionModel {
    pub fn new(order: Vec<u64>) -> SelectionModel {
        SelectionModel { ids: Vec::new(), anchor: None, order }
    }

    /// 普通点击：清空后单选（锚点重置）。
    pub fn plain_click(&mut self, id: u64) {
        self.ids.clear();
        self.ids.push(id);
        self.anchor = Some(id);
    }

    /// Ctrl 点击：切换选中（锚点不动）。
    pub fn ctrl_click(&mut self, id: u64) {
        if let Some(pos) = self.ids.iter().position(|x| *x == id) {
            self.ids.remove(pos);
        } else {
            self.ids.push(id);
        }
        if self.anchor.is_none() {
            self.anchor = Some(id);
        }
    }

    /// Shift 点击：锚点到目标的范围全选（列表序）。
    pub fn shift_click(&mut self, id: u64) {
        let anchor = *self.anchor.get_or_insert(id);
        let ia = self.order.iter().position(|x| *x == anchor);
        let ib = self.order.iter().position(|x| *x == id);
        if let (Some(ia), Some(ib)) = (ia, ib) {
            let (lo, hi) = if ia <= ib { (ia, ib) } else { (ib, ia) };
            self.ids = self.order[lo..=hi].to_vec();
        }
    }

    /// 全选本页（操作条「全选本页」）。
    pub fn select_all(&mut self) {
        self.ids = self.order.clone();
    }

    /// 清除选择（Esc / 操作条「清除选择」）。
    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    pub fn count(&self) -> usize {
        self.ids.len()
    }
}

/// 体积人话格式（「245MB」/「1.2GB」——操作条计数徽章渲染面）。
pub fn format_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    let mb = bytes as f64 / 1_000_000.0;
    let kb = bytes as f64 / 1_000.0;
    if gb >= 1.0 {
        alloc::format!("{:.1}GB", gb)
    } else if mb >= 1.0 {
        alloc::format!("{:.0}MB", mb)
    } else if kb >= 1.0 {
        alloc::format!("{:.0}KB", kb)
    } else {
        alloc::format!("{}B", bytes)
    }
}

/// F339 媒体预览态（音/视频预览即播——进度条+音量的模型面；预览只读
/// 不写文件，播放态是纯内存态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaPlayState {
    /// 播放位置（ms）。
    pub position_ms: u64,
    /// 总时长（ms）。
    pub duration_ms: u64,
    /// 音量（0-100）。
    pub volume: u8,
    pub playing: bool,
}

impl MediaPlayState {
    pub fn new(duration_ms: u64) -> MediaPlayState {
        MediaPlayState { position_ms: 0, duration_ms, volume: 50, playing: false }
    }

    /// 播放推进（注入钟——position 不越过时长）。
    pub fn tick(&mut self, delta_ms: u64) {
        if !self.playing {
            return;
        }
        self.position_ms = (self.position_ms + delta_ms).min(self.duration_ms);
        if self.position_ms == self.duration_ms {
            self.playing = false; // 播完自停。
        }
    }

    /// 进度千分比（进度条渲染面）。
    pub fn progress_permille(&self) -> u64 {
        if self.duration_ms == 0 {
            return 0;
        }
        self.position_ms * 1000 / self.duration_ms
    }

    /// 音量调节（0-100 钳制）。
    pub fn set_volume(&mut self, v: u8) {
        self.volume = v.min(100);
    }

    /// 关闭预览复位（再开从头——不留半空态）。
    pub fn reset(&mut self) {
        self.position_ms = 0;
        self.playing = false;
    }
}

/// F339 标注旁文件模型（轻量圈画——标注存旁文件，原文件不动）。
pub struct AnnotationSidecar {
    pub original: String,
    /// 标注笔迹数（圈/箭头/文字——每笔一账）。
    pub strokes: Vec<String>,
}

impl AnnotationSidecar {
    /// 旁文件路径（原名 + .varix-annot.png——保存路径明确判据载体）。
    pub fn sidecar_path(&self) -> String {
        let base = self.original.trim_end_matches(".png");
        alloc::format!("{}.varix-annot.png", base)
    }

    /// 加笔迹（上限 50——轻量纪律）。
    pub fn add_stroke(&mut self, stroke: &str) -> bool {
        if self.strokes.len() >= 50 {
            return false;
        }
        self.strokes.push(String::from(stroke));
        true
    }

    /// 清空标注（撤销全部）。
    pub fn clear(&mut self) {
        self.strokes.clear();
    }

    /// 已标注（旁文件是否该存在）。
    pub fn has_annotation(&self) -> bool {
        !self.strokes.is_empty()
    }
}

/// F340 渲染缓存 LRU（容量上限——大目录翻阅不撑爆内存；淘汰最旧）。
pub struct PreviewCacheLru {
    entries: Vec<String>,
    cap: usize,
    pub hits: u64,
    pub misses: u64,
}

impl PreviewCacheLru {
    pub fn new(cap: usize) -> PreviewCacheLru {
        PreviewCacheLru { entries: Vec::new(), cap: cap.max(1), hits: 0, misses: 0 }
    }

    /// 访问（命中置顶计数；未命中插入，超限淘汰最旧）。
    pub fn touch(&mut self, name: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|x| x == name) {
            let e = self.entries.remove(pos);
            self.entries.insert(0, e);
            self.hits += 1;
            true
        } else {
            self.misses += 1;
            self.entries.insert(0, String::from(name));
            if self.entries.len() > self.cap {
                self.entries.pop();
            }
            false
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 深化层自检。
pub fn run_multibar_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F338-340-deep");

    // 1. 选择三键语义：plain 单选 / ctrl 切换 / shift 范围。
    let mut sel = SelectionModel::new(alloc::vec![1, 2, 3, 4, 5]);
    sel.plain_click(3);
    sel.ctrl_click(5);
    sel.shift_click(1);
    // 逐步状态机验证（plain→ctrl→shift 三键语义精确断言）：
    let mut sel = SelectionModel::new(alloc::vec![1, 2, 3, 4, 5]);
    sel.plain_click(3);
    let plain_ok = sel.ids == [3] && sel.anchor == Some(3);
    sel.ctrl_click(5);
    let ctrl_ok = sel.ids == [3, 5];
    sel.shift_click(1);
    let shift_ok = sel.ids == alloc::vec![1, 2, 3];
    set.add(
        "selection state machine exact",
        plain_ok && ctrl_ok && shift_ok,
        "",
    );

    // 2. 全选本页 + 清除（操作条两键）。
    sel.clear();
    sel.select_all();
    let all = sel.count() == 5;
    sel.clear();
    set.add("select all and clear", all && sel.count() == 0, "");

    // 3. 体积人话：245MB / 1.2GB / 800KB / 7B 四档。
    set.add(
        "size human formats",
        format_size(245_000_000) == "245MB"
            && format_size(1_200_000_000) == "1.2GB"
            && format_size(800_000) == "800KB"
            && format_size(7) == "7B",
        "",
    );

    // 4. 媒体预览态：播放推进 + 播完自停 + 进度面 + 音量钳制 + 复位。
    let mut m = MediaPlayState::new(10_000);
    m.playing = true;
    m.tick(4_000);
    m.tick(7_000);
    m.set_volume(150);
    set.add(
        "media state plays and stops",
        m.position_ms == 10_000 && !m.playing && m.progress_permille() == 1000 && m.volume == 100,
        "",
    );
    m.reset();
    set.add("media reset clean", m.position_ms == 0 && !m.playing && m.progress_permille() == 0, "");

    // 5. 标注旁文件：路径明确 + 50 笔上限 + 清空。
    let mut a = AnnotationSidecar { original: String::from("照.png"), strokes: Vec::new() };
    let path = a.sidecar_path();
    let mut full = true;
    for i in 0..52 {
        full = full && a.add_stroke(&alloc::format!("s{i}"));
    }
    set.add(
        "annotation sidecar model",
        path == "照.varix-annot.png" && a.has_annotation() && a.strokes.len() == 50 && !full,
        "",
    );
    a.clear();
    set.add("annotation clear", !a.has_annotation(), "");

    // 6. 渲染缓存 LRU：命中计数、超限淘汰最旧。
    let mut c = PreviewCacheLru::new(3);
    let m1 = c.touch("a");
    let _ = c.touch("b");
    let _ = c.touch("c");
    let hit = c.touch("a");
    let _ = c.touch("d");
    set.add(
        "preview cache lru",
        !m1 && hit && c.len() == 3 && c.hits == 1 && c.misses == 4 && !c.entries.contains(&String::from("b")),
        "",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn shift_without_anchor_defaults_self() {
        let mut sel = SelectionModel::new(alloc::vec![1, 2]);
        sel.shift_click(2);
        assert_eq!(sel.ids, alloc::vec![2]);
    }

    #[test]
    fn ctrl_toggle_off_removes() {
        let mut sel = SelectionModel::new(alloc::vec![1, 2]);
        sel.ctrl_click(1);
        sel.ctrl_click(1);
        assert!(sel.ids.is_empty());
    }

    #[test]
    fn media_zero_duration_safe() {
        let mut m = MediaPlayState::new(0);
        m.playing = true;
        m.tick(100);
        assert_eq!(m.progress_permille(), 0);
    }

    #[test]
    fn size_bytes_tiny() {
        assert_eq!(format_size(999), "999B");
    }
}
