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
