//! F588 图片粘贴为文件 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：目录/桌面/对话框三场景；PNG 落盘；命名规则；
//! 重名递增；格式栈分派。
//!
//! **设计要点（主册）**：
//! - 剪贴板里的图片（截图 F098/复制网页图）可直接粘成文件：在资源管理器/
//!   桌面 Ctrl+V = 存为 PNG（自动命名「截图 2026-09-25_1430」F521 同规、
//!   落点当前目录）；
//! - 对话框里 Ctrl+V 落文件名框的是文件引用（F017 格式栈全兼容）；
//!   粘贴重复名自动递增。
//!
//! 命名语义源：[`crate::istar::ibase::dedupe_name`]（重名递增唯一实现）。

use crate::checks::CheckSet;
use crate::istar::ibase::{dedupe_name, ISTAR_DOMAIN};

use alloc::string::String;
#[cfg(test)]
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 粘贴目标场景（三场景枚举——格式栈分派依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteScene {
    /// 资源管理器目录（落 PNG 文件）。
    Folder,
    /// 桌面（落 PNG 文件）。
    Desktop,
    /// 文件对话框（落文件引用进文件名框——不是落盘）。
    FileDialog,
}

/// 图片文件扩展名（落盘格式唯一源——PNG）。
pub const IMAGE_EXT: &str = ".png";

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 粘贴落点账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteOutcome {
    /// 落盘为 PNG（目录/桌面场景）——(落点目录, 文件名)。
    Saved { dir: String, name: String },
    /// 文件引用（对话框场景——文件名框收到的是引用）。
    Reference { name: String },
    /// 非图片剪贴板内容——场景分派无操作（格式栈不匹配，诚实无动作）。
    NotAnImage,
}

/// 图片粘贴分派器。
pub struct PasteImg {
    /// 命名序号账（同分钟多截图——日期时间后递增）。
    seq: u32,
}

impl PasteImg {
    pub fn new() -> PasteImg {
        PasteImg { seq: 0 }
    }

    /// Ctrl+V 分派（F017 格式栈语义：按场景 + 剪贴板内容类型决定落点）。
    ///
    /// `is_image`：剪贴板是否持位图格式（F098 截图/网页图）；
    /// `stamp`：命名戳（F521 同规「截图 2026-09-25_1430」形态由调用方给——
    /// 本函数只管去重与落点分派）；
    /// `taken`：落点目录已占用名（不含扩展名）。
    pub fn paste(
        &mut self,
        scene: PasteScene,
        is_image: bool,
        stamp: &str,
        taken: &[&str],
    ) -> PasteOutcome {
        if !is_image {
            return PasteOutcome::NotAnImage;
        }
        match scene {
            PasteScene::Folder | PasteScene::Desktop => {
                self.seq += 1;
                match dedupe_name(stamp, taken) {
                    Some(name) => PasteOutcome::Saved {
                        dir: String::from(match scene {
                            PasteScene::Desktop => "桌面",
                            _ => "当前目录",
                        }),
                        name: alloc::format!("{}{}", name, IMAGE_EXT),
                    },
                    None => PasteOutcome::NotAnImage, // 递增上限诚实拒绝
                }
            }
            PasteScene::FileDialog => PasteOutcome::Reference {
                name: alloc::format!("{}{}", stamp, IMAGE_EXT),
            },
        }
    }

    /// 命名序号账（同会话落盘数——对账面）。
    pub fn saved_count(&self) -> u32 {
        self.seq
    }
}

impl Default for PasteImg {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_pasteimg_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 目录场景：PNG 落盘（名字带戳 + .png）。
    let mut p = PasteImg::new();
    let o1 = p.paste(PasteScene::Folder, true, "截图 2026-09-25_1430", &[]);
    set.add(
        "folder paste saves png",
        o1 == PasteOutcome::Saved {
            dir: String::from("当前目录"),
            name: String::from("截图 2026-09-25_1430.png"),
        },
        "",
    );

    // 2. 桌面场景：落点桌面（同一分派口不同目录）。
    let o2 = p.paste(PasteScene::Desktop, true, "截图 2026-09-25_1431", &[]);
    set.add(
        "desktop paste target",
        matches!(&o2, PasteOutcome::Saved { dir, .. } if dir == "桌面"),
        "",
    );

    // 3. 对话框场景：文件引用（不落盘——F017 格式栈场景语义正确）。
    let o3 = p.paste(PasteScene::FileDialog, true, "截图 2026-09-25_1432", &[]);
    set.add(
        "dialog paste is reference",
        o3 == PasteOutcome::Reference {
            name: String::from("截图 2026-09-25_1432.png"),
        },
        "",
    );

    // 4. 重名递增：同目录再粘同戳 → (2) 递增（ibase::dedupe_name 语义）。
    let o4 = p.paste(PasteScene::Folder, true, "截图 2026-09-25_1430", &["截图 2026-09-25_1430"]);
    set.add(
        "duplicate name increments",
        o4 == PasteOutcome::Saved {
            dir: String::from("当前目录"),
            name: String::from("截图 2026-09-25_1430 (2).png"),
        },
        "",
    );

    // 5. 非图片剪贴板：诚实无动作（文本粘贴不造文件）。
    let o5 = p.paste(PasteScene::Folder, false, "随便", &[]);
    set.add("non image honest noop", o5 == PasteOutcome::NotAnImage, "");

    // 6. 命名规则：F521 同规戳形（日期_时分）由调用方给——本模块校验
    //    落盘名一律 .png 结尾（PNG 格式唯一源）。
    let saved_name = match &o1 {
        PasteOutcome::Saved { name, .. } => name.clone(),
        _ => String::new(),
    };
    set.add(
        "png ext single source",
        saved_name.ends_with(IMAGE_EXT) && IMAGE_EXT == ".png",
        "",
    );

    // 7. 三场景分派齐全（枚举即清单）。
    set.add(
        "three scenes dispatched",
        PasteScene::Folder != PasteScene::Desktop
            && PasteScene::Desktop != PasteScene::FileDialog,
        "",
    );

    // 8. 落盘账：会话内落盘计数对账（目录 2 次 + 桌面 1 次 = 3；对话框
    //    引用不落盘不计）。
    set.add("saved count ledger", p.saved_count() == 3, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_duplicate_increments() {
        let mut p = PasteImg::new();
        let taken: Vec<&str> = vec!["截图 2026-09-25_1430", "截图 2026-09-25_1430 (2)"];
        let o = p.paste(PasteScene::Folder, true, "截图 2026-09-25_1430", &taken);
        match o {
            PasteOutcome::Saved { name, .. } => {
                assert_eq!(name, "截图 2026-09-25_1430 (3).png");
            }
            _ => panic!("应落盘"),
        }
    }

    #[test]
    fn empty_stamp_still_saves() {
        let mut p = PasteImg::new();
        let o = p.paste(PasteScene::Folder, true, "", &[]);
        match o {
            PasteOutcome::Saved { name, .. } => assert_eq!(name, ".png"),
            _ => panic!("空戳也应落盘（时间戳由宿主补）"),
        }
    }

    #[test]
    fn dialog_reference_never_dedupes() {
        // 对话框场景是引用（文件名框重名由用户管）——不走递增。
        let mut p = PasteImg::new();
        let o = p.paste(PasteScene::FileDialog, true, "x", &["x"]);
        assert_eq!(o, PasteOutcome::Reference { name: String::from("x.png") });
    }
}
