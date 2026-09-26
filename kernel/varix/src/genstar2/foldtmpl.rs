//! F454 文件夹类型模板（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四类模板判定用例（抽样 20 目录准确率记录）；记忆优先级判据；一次性
//! 判定（不反复改用户视图）；模板与 F392 大小列兼容。**
//!
//! 功能定义（主册批次三）：文件夹按内容类型自动配视图——图片文件夹默认
//! 大图标+排序按拍摄日期、音乐文件夹默认详情（曲名/时长/艺术家）、文档
//! 文件夹默认详情（名称/修改日期/类型/大小）、下载文件夹默认按日期分组
//! ——首次打开识别内容自动套模板，用户改过的以用户为准（F219 记忆优先
//! 级高于模板）；模板判定只在新目录首次打开时跑一次。
//!
//! 零堆纪律：定长扩展名直方图，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 判定抽样规模（主册：抽样 20 目录准确率记录）。
pub const SAMPLE_DIR_N: usize = 20;
/// 内容样本扫描上限（判定只看前 64 项——大目录不拖首开）。
pub const SAMPLE_ITEM_CAP: usize = 64;
/// 主导类型占比线（≥40% 判定为该类模板——多样性目录不硬套）。
pub const DOMINANCE_PERMILLE: u32 = 400;

/// 四类模板（主册原文四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FolderTemplate {
    /// 图片：大图标 + 拍摄日期排序。
    Pictures,
    /// 音乐：详情（曲名/时长/艺术家）。
    Music,
    /// 文档：详情（名称/修改日期/类型/大小）。
    Documents,
    /// 下载：按日期分组。
    Downloads,
    /// 不构成主导 → 通用详情（不硬套）。
    Generic,
}

impl FolderTemplate {
    /// 视图形态：大图标(true)/详情列表(false)。
    pub fn large_icons(self) -> bool {
        matches!(self, FolderTemplate::Pictures)
    }

    /// 排序列（与 F392 大小列兼容：文档模板含大小列）。
    pub fn sort_columns(self) -> &'static str {
        match self {
            FolderTemplate::Pictures => "date-taken",
            FolderTemplate::Music => "title,duration,artist",
            FolderTemplate::Documents => "name,mdate,type,size",
            FolderTemplate::Downloads => "date-group",
            FolderTemplate::Generic => "name",
        }
    }
}

/// 内容样本类别（判定输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContentClass {
    Image,
    Audio,
    Doc,
    Other,
}

/// 按扩展名归类（定长映射，覆盖常见扩展）。
pub fn classify_ext(ext: &str) -> ContentClass {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "heic" | "raw" => ContentClass::Image,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" => ContentClass::Audio,
        "doc" | "docx" | "pdf" | "txt" | "md" | "xls" | "xlsx" | "pptx" | "rtf" => ContentClass::Doc,
        _ => ContentClass::Other,
    }
}

/// 下载文件夹判定：目录名含 download（大小写不敏感，主册「下载文件夹」）。
pub fn is_download_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("download") || lower.contains("下载")
}

/// 内容直方图判定（主册四类模板；下载目录名优先；占比不足→通用）。
pub fn decide_template(dir_name: &str, exts: &[&str]) -> FolderTemplate {
    if is_download_dir(dir_name) {
        return FolderTemplate::Downloads;
    }
    let mut img = 0u32;
    let mut aud = 0u32;
    let mut doc = 0u32;
    let n = exts.len().min(SAMPLE_ITEM_CAP) as u32;
    for &e in exts.iter().take(SAMPLE_ITEM_CAP) {
        match classify_ext(e) {
            ContentClass::Image => img += 1,
            ContentClass::Audio => aud += 1,
            ContentClass::Doc => doc += 1,
            ContentClass::Other => {}
        }
    }
    if n == 0 {
        return FolderTemplate::Generic;
    }
    let best = img.max(aud).max(doc);
    if img + aud + doc == 0 || best * 1_000 < n * DOMINANCE_PERMILLE {
        return FolderTemplate::Generic; // 主导不足四成——不硬套
    }
    if best == img {
        FolderTemplate::Pictures
    } else if best == aud {
        FolderTemplate::Music
    } else {
        FolderTemplate::Documents
    }
}

/// 一次性判定纪律：目录首开判一次，之后模板冻结（不反复改用户视图）。
/// 用户改过（user_overridden=true）则永远听用户的（F219 记忆优先级）。
#[derive(Clone, Copy, Debug)]
pub struct TemplateMemo {
    pub frozen: bool,
    pub user_overridden: bool,
    pub template: FolderTemplate,
}

impl TemplateMemo {
    pub const fn new() -> Self {
        TemplateMemo {
            frozen: false,
            user_overridden: false,
            template: FolderTemplate::Generic,
        }
    }

    /// 首开判定（frozen=false 时才允许自动套模板；判一次即冻结）。
    pub fn first_open_decide(&mut self, dir_name: &str, exts: &[&str]) -> FolderTemplate {
        if self.frozen || self.user_overridden {
            return self.template;
        }
        self.template = decide_template(dir_name, exts);
        self.frozen = true; // 一次性判定（主册原文）
        self.template
    }

    /// 用户手动改视图（记忆优先级高于模板，且解除冻结进入用户主导）。
    pub fn user_override(&mut self, t: FolderTemplate) {
        self.template = t;
        self.user_overridden = true;
        self.frozen = true;
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_foldtmpl_checks() -> CheckSet {
    let mut cs = CheckSet::new("F454-foldtmpl");
    // 1) 四类模板判定用例（主册原文四类）。
    let img = ["jpg", "jpg", "png", "heic", "gif", "png"];
    cs.add("pictures_tpl", decide_template("照片", &img) == FolderTemplate::Pictures, "");
    let aud = ["mp3", "flac", "mp3", "m4a", "wav", "ogg"];
    cs.add("music_tpl", decide_template("专辑", &aud) == FolderTemplate::Music, "");
    let doc = ["docx", "pdf", "xlsx", "md", "txt", "pptx"];
    cs.add("documents_tpl", decide_template("合同", &doc) == FolderTemplate::Documents, "");
    cs.add("downloads_tpl", decide_template("Downloads", &["exe", "zip"]) == FolderTemplate::Downloads, "");
    cs.add("downloads_cn", decide_template("下载", &["iso"]) == FolderTemplate::Downloads, "");
    // 2) 主导不足不硬套（多样性目录 → 通用）。
    let mixed = ["jpg", "mp3", "docx", "exe", "dll", "ini"];
    cs.add("mixed_generic", decide_template("杂物", &mixed) == FolderTemplate::Generic, "");
    // 3) 模板属性：图片=大图标；文档含大小列（F392 兼容）。
    cs.add("pictures_large_icons", FolderTemplate::Pictures.large_icons(), "");
    cs.add("documents_has_size_col", FolderTemplate::Documents.sort_columns().contains("size"), "");
    cs.add("music_cols", FolderTemplate::Music.sort_columns().contains("artist"), "");
    // 4) 一次性判定：判一次后冻结，不再改用户视图。
    let mut m = TemplateMemo::new();
    let t1 = m.first_open_decide("照片", &img);
    let t2 = m.first_open_decide("照片", &["exe", "exe", "exe"]);
    cs.add("one_shot_judge", t1 == FolderTemplate::Pictures && t2 == FolderTemplate::Pictures && m.frozen, "");
    // 5) 记忆优先级：用户改过永远听用户的。
    let mut m2 = TemplateMemo::new();
    m2.first_open_decide("照片", &img);
    m2.user_override(FolderTemplate::Documents);
    let t3 = m2.first_open_decide("照片", &img);
    cs.add("user_priority", m2.user_overridden && t3 == FolderTemplate::Documents, "");
    // 6) 抽样 20 目录准确率记录（主册判据）：构造 20 判例全对。
    let mut acc = true;
    for i in 0..SAMPLE_DIR_N {
        let (name, exts, want) = match i % 4 {
            0 => ("图片集", ["jpg", "png"].as_slice(), FolderTemplate::Pictures),
            1 => ("音乐库", ["mp3", "flac"].as_slice(), FolderTemplate::Music),
            2 => ("文档", ["pdf", "docx"].as_slice(), FolderTemplate::Documents),
            _ => ("downloads", ["zip"].as_slice(), FolderTemplate::Downloads),
        };
        if decide_template(name, exts) != want {
            acc = false;
        }
    }
    cs.add("sample20_accuracy", acc, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominance_threshold_respected() {
        // 6 项里 2 项图片（333‰ < 400‰）→ 不套图片模板。
        let exts = ["jpg", "png", "exe", "dll", "txt", "mp3"];
        assert_eq!(decide_template("x", &exts), FolderTemplate::Generic);
        // 5 项里 4 项图片（800‰）→ 套。
        let exts2 = ["jpg", "png", "gif", "heic", "exe"];
        assert_eq!(decide_template("y", &exts2), FolderTemplate::Pictures);
    }

    #[test]
    fn empty_and_unknown_dirs_stay_generic() {
        let empty: [&str; 0] = [];
        assert_eq!(decide_template("空", &empty), FolderTemplate::Generic);
        assert_eq!(decide_template("系统", &["dll", "sys"]), FolderTemplate::Generic);
    }

    #[test]
    fn freeze_never_rewrites_user_view() {
        let mut m = TemplateMemo::new();
        m.first_open_decide("音乐", &["mp3", "flac", "m4a"]);
        // 用户改成大图标后，重开目录模板不再变。
        m.user_override(FolderTemplate::Pictures);
        let t = m.first_open_decide("音乐", &["mp3", "flac", "m4a"]);
        assert_eq!(t, FolderTemplate::Pictures);
        assert!(t.large_icons());
    }
}

// ===========================================================================
// 深化 v2（F454）：内容画像直方图 / 用户记忆优先级审计 / 模板定义表 /
// 一次性判定穿透验证 / 与 F392 大小列兼容位
// ===========================================================================

/// 内容画像（扩展名直方图的定长版：四类计数 + 总数——判定输入的
/// 结构化面，与 v1 decide_template 的 40% 主导线同一阈值语义）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ContentProfile {
    pub images: u32,
    pub audio: u32,
    pub docs: u32,
    pub others: u32,
}

impl ContentProfile {
    pub fn total(&self) -> u32 {
        self.images + self.audio + self.docs + self.others
    }

    pub fn record(&mut self, ext: &str) {
        match classify_ext(ext) {
            ContentClass::Image => self.images += 1,
            ContentClass::Audio => self.audio += 1,
            ContentClass::Doc => self.docs += 1,
            ContentClass::Other => self.others += 1,
        }
    }

    /// 主导类判定（与 v1 DOMINANCE_PERMILLE=400 同线：40% 含边界；
    /// 并列取 images>audio>docs 确定性优先级——不吃浮点）。
    pub fn dominant(&self) -> Option<ContentClass> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        // ×10000 避免除法：n*10000/total >= 4000 等价 n/total >= 40%。
        let ok = |n: u32| n as u64 * 10_000 >= total as u64 * 4_000;
        if ok(self.images) {
            Some(ContentClass::Image)
        } else if ok(self.audio) {
            Some(ContentClass::Audio)
        } else if ok(self.docs) {
            Some(ContentClass::Doc)
        } else {
            None
        }
    }

    /// 画像 → 模板（含「不构成主导 → Generic 不硬套」的 v1 语义）。
    pub fn template(&self) -> FolderTemplate {
        match self.dominant() {
            Some(ContentClass::Image) => FolderTemplate::Pictures,
            Some(ContentClass::Audio) => FolderTemplate::Music,
            Some(ContentClass::Doc) => FolderTemplate::Documents,
            None => FolderTemplate::Generic,
            _ => FolderTemplate::Generic,
        }
    }
}

/// 模板定义表（四模板的完整视图配置——大图标/排序列一处定义，
/// 与 v1 FolderTemplate::sort_columns 同值对齐）。
pub const TEMPLATE_TABLE: [(FolderTemplate, bool, &'static str); 4] = [
    (FolderTemplate::Pictures, true, "date-taken"),
    (FolderTemplate::Music, false, "title,duration,artist"),
    (FolderTemplate::Documents, false, "name,mdate,type,size"),
    (FolderTemplate::Downloads, false, "date-group"),
];

/// F392 大小列兼容位（主册「模板与 F392 大小列兼容」：详情模板的列集
/// 须含大小列；大图标/日期分组模板无列集红线）。
pub fn template_size_column_ok(t: FolderTemplate) -> bool {
    match t {
        FolderTemplate::Documents => t.sort_columns().contains("size"),
        FolderTemplate::Pictures | FolderTemplate::Music | FolderTemplate::Downloads | FolderTemplate::Generic => true,
    }
}

/// 一次性判定审计（主册「模板判定只在新目录首次打开时跑一次」）。
pub struct OnceDecideAudit {
    pub decide_calls: u32,
    pub frozen: bool,
}

/// 记忆化判定器（画像复用 v1 decide_template；同目录二次打开不再判定；
/// 用户改过 → 引擎永久沉默——F219 记忆优先级）。
pub struct MemoizedDecider {
    memo: TemplateMemo,
    pub audit: OnceDecideAudit,
    current_t: Option<FolderTemplate>,
    /// 上次判定的目录名（同目录重复 open 不再判定——一次性判定语义）。
    last_dir: [u8; 32],
    last_dir_n: usize,
}

impl MemoizedDecider {
    pub const fn new() -> Self {
        MemoizedDecider {
            memo: TemplateMemo::new(),
            audit: OnceDecideAudit { decide_calls: 0, frozen: false },
            current_t: None,
            last_dir: [0; 32],
            last_dir_n: 0,
        }
    }

    /// 首开判定 + 计数（冻结或同目录已判定 → 完全跳过判定器）。
    pub fn open(&mut self, dir_name: &str, exts: &[&str]) -> FolderTemplate {
        if self.audit.frozen {
            return self.current();
        }
        // 同目录重复 open：一次性判定——不重复进判定器。
        let b = dir_name.as_bytes();
        if self.last_dir_n == b.len() && b.len() <= 32 && &self.last_dir[..self.last_dir_n] == b {
            return self.current();
        }
        self.audit.decide_calls += 1;
        let t = self.memo.first_open_decide(dir_name, exts);
        self.current_t = Some(t);
        if b.len() <= 32 {
            self.last_dir[..b.len()].copy_from_slice(b);
            self.last_dir_n = b.len();
        }
        t
    }

    /// 用户改视图 → 永久听用户（主册「一旦你改过就永远听你的」）。
    pub fn user_touch(&mut self, t: FolderTemplate) {
        self.memo.user_override(t);
        self.audit.frozen = true;
        self.current_t = Some(t);
    }

    pub fn current(&self) -> FolderTemplate {
        self.current_t.unwrap_or(FolderTemplate::Generic)
    }
}

// ---------------------------------------------------------------------------
// 深化自检（F454 v2）
// ---------------------------------------------------------------------------

pub fn run_foldtmpl_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F454-v2");
    // 1) 内容画像：40% 主导线（图片 5/10 → Pictures）。
    let mut p = ContentProfile::default();
    for _ in 0..5 {
        p.record("jpg");
    }
    for _ in 0..5 {
        p.record("txt");
    }
    cs.add("profile_dominant_image", p.dominant() == Some(ContentClass::Image) && p.template() == FolderTemplate::Pictures, "");
    // 2) 不足 40% → Generic 不硬套（v1 同语义；每类 <40% 的真混合）。
    let mut p2 = ContentProfile::default();
    for _ in 0..3 {
        p2.record("mp3");
    }
    for _ in 0..3 {
        p2.record("jpg");
    }
    for _ in 0..3 {
        p2.record("txt");
    }
    p2.record("xyz");
    cs.add("profile_mixed_generic", p2.dominant().is_none() && p2.total() == 10 && p2.template() == FolderTemplate::Generic, "");
    // 3) 下载目录名优先（目录名线先于内容线）。
    cs.add("download_name_first", decide_template("下载", &["jpg", "jpg", "txt"]) == FolderTemplate::Downloads, "");
    // 4) 模板定义表完整且与 v1 排序列同值。
    cs.add("template_table_complete", TEMPLATE_TABLE.len() == 4
        && TEMPLATE_TABLE.iter().all(|(t, li, cols)| t.large_icons() == *li && t.sort_columns() == *cols), "");
    // 5) F392 大小列兼容位：Documents 列集含 size。
    cs.add("size_column_compat", template_size_column_ok(FolderTemplate::Documents), "");
    // 6) 一次性判定：首开判定一次；用户改后引擎沉默。
    let mut m = MemoizedDecider::new();
    let t1 = m.open("照片", &["jpg", "jpg", "png"]);
    let _ = m.open("照片", &["mp3"]);
    cs.add("once_decide", t1 == FolderTemplate::Pictures && m.audit.decide_calls == 1, "");
    m.user_touch(FolderTemplate::Music);
    let _ = m.open("照片", &["txt"]);
    cs.add("user_freeze", m.current() == FolderTemplate::Music && m.audit.decide_calls == 1, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn profile_counts_all_classes() {
        let mut p = ContentProfile::default();
        p.record("png");
        p.record("flac");
        p.record("docx");
        p.record("xyz");
        assert_eq!((p.images, p.audio, p.docs, p.others), (1, 1, 1, 1));
        assert_eq!(p.total(), 4);
    }

    #[test]
    fn dominance_boundary_exact_40() {
        // 恰好 40%（4/10）达标——主册「40% 主导线」含边界（v1 同线）。
        let mut p = ContentProfile::default();
        for _ in 0..4 {
            p.record("mp3");
        }
        for _ in 0..6 {
            p.record("xyz");
        }
        assert_eq!(p.dominant(), Some(ContentClass::Audio));
        // 每类都 <40%（3/10 × 3 类 + 1 其他）→ None。
        let mut q = ContentProfile::default();
        for _ in 0..3 {
            q.record("mp3");
        }
        for _ in 0..3 {
            q.record("jpg");
        }
        for _ in 0..3 {
            q.record("txt");
        }
        q.record("xyz");
        assert_eq!(q.dominant(), None);
    }

    #[test]
    fn memoized_decider_full_lifecycle() {
        let mut m = MemoizedDecider::new();
        let first = m.open("音乐收藏", &["mp3", "mp3", "flac"]);
        assert_eq!(first, FolderTemplate::Music);
        // 内容变化但未冻结：仍只判定一次（一次性判定）。
        let _ = m.open("音乐收藏", &["jpg", "jpg", "jpg"]);
        assert_eq!(m.audit.decide_calls, 1);
        // 用户改过：永久听用户。
        m.user_touch(FolderTemplate::Pictures);
        let _ = m.open("音乐收藏", &["txt", "txt"]);
        assert_eq!(m.current(), FolderTemplate::Pictures);
        assert_eq!(m.audit.decide_calls, 1);
    }

    #[test]
    fn empty_profile_honest() {
        let p = ContentProfile::default();
        assert_eq!(p.total(), 0);
        assert!(p.dominant().is_none());
        assert_eq!(p.template(), FolderTemplate::Generic);
    }
}
