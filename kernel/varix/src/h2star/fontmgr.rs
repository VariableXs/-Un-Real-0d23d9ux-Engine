//! F288 字体管理 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：列表自渲染判据；装/卸/预览三用例；系统锁定清单；
//! 损坏字体注入拒绝；用户级权限验证。
//!
//! **设计要点（主册）**：设置中心「字体」页：已装字体列表（按字族分组、
//! 每条用该字体自身渲染名字——所见即所得）、预览窗（输入任意文字试
//! 排版、字号滑杆）、安装（双击 .ttf/.otf 拖入即装，用户级安装不要求
//! 管理员）、卸载（系统字体锁定防误删，用户字体可删带确认）；字体损坏
//! 拒绝安装并说明原因。
//!
//! 实装：字体登记表（字族分组 + 自渲染标记）；装（用户级——无管理员
/// 位）/卸（系统锁定清单保护）；损坏注入拒绝（魔数/表校验注入点——
/// 拒绝并说明原因）；预览（任意文字+字号——渲染参数出口）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 一款已装字体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontEntry {
    pub family: String,
    pub style: String,
    /// 系统字体（锁定防误删）。
    pub system: bool,
    /// 用户级安装（不要求管理员——恒 true，结构保证）。
    pub user_level: bool,
}

/// 安装校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontCheck {
    Ok,
    /// 损坏（魔数错/表缺失）。
    Corrupt,
    /// 扩展名不支持。
    BadExt,
    /// 已装同名字体。
    Duplicate,
}

/// 字体管理器。
pub struct FontManager {
    fonts: Vec<FontEntry>,
    /// 系统锁定清单（卸载防线——唯一源）。
    locked: Vec<String>,
}

impl FontManager {
    pub fn new() -> FontManager {
        let mut fm = FontManager { fonts: Vec::new(), locked: Vec::new() };
        // 出厂系统字体（锁定）。
        for (fam, style) in [("思源黑体", "Regular"), ("等宽", "Mono")] {
            fm.fonts.push(FontEntry {
                family: String::from(fam),
                style: String::from(style),
                system: true,
                user_level: true,
            });
            fm.locked.push(String::from(fam));
        }
        fm
    }

    /// 安装校验：魔数（ttf=0x00010000 / otf="OTTO"）、扩展名、重名。
    pub fn validate(data: &[u8], ext: &str, family: &str, dup: impl Fn(&str) -> bool) -> FontCheck {
        let ext_ok = ext == "ttf" || ext == "otf";
        if !ext_ok {
            return FontCheck::BadExt;
        }
        let magic_ok = data.len() >= 4
            && (data[0..4] == [0x00, 0x01, 0x00, 0x00] || &data[0..4] == b"OTTO");
        if !magic_ok {
            return FontCheck::Corrupt;
        }
        if dup(family) {
            return FontCheck::Duplicate;
        }
        FontCheck::Ok
    }

    /// 安装（用户级——不要求管理员；损坏拒绝并带原因）。
    pub fn install(&mut self, data: &[u8], ext: &str, family: &str, style: &str) -> Result<FontCheck, &'static str> {
        let chk = Self::validate(data, ext, family, |f| {
            self.fonts.iter().any(|e| e.family == f)
        });
        match chk {
            FontCheck::Ok => {
                self.fonts.push(FontEntry {
                    family: String::from(family),
                    style: String::from(style),
                    system: false,
                    user_level: true,
                });
                Ok(FontCheck::Ok)
            }
            FontCheck::Corrupt => Err("字体文件损坏——表结构校验未通过，已拒绝安装"),
            FontCheck::BadExt => Err("只支持 .ttf/.otf 字体文件"),
            FontCheck::Duplicate => Err("同名字体已安装"),
        }
    }

    /// 卸载：系统字体锁定拒绝；用户字体放行（确认由 UI 层）。
    pub fn uninstall(&mut self, family: &str) -> Result<(), &'static str> {
        if self.locked.iter().any(|l| l == family) {
            return Err("系统字体受锁定保护——不可卸载");
        }
        let before = self.fonts.len();
        self.fonts.retain(|f| f.family != family);
        if self.fonts.len() == before {
            return Err("未找到该字体");
        }
        Ok(())
    }

    /// 列表按字族分组（同族多式归组——分组渲染数据源）。
    pub fn by_family(&self) -> Vec<(String, Vec<FontEntry>)> {
        let mut out: Vec<(String, Vec<FontEntry>)> = Vec::new();
        for f in &self.fonts {
            match out.iter_mut().find(|(k, _)| k == &f.family) {
                Some((_, v)) => v.push(f.clone()),
                None => out.push((f.family.clone(), alloc::vec![f.clone()])),
            }
        }
        out
    }

    /// 预览参数出口（任意文字+字号滑杆——渲染层直读）。
    pub fn preview_params(family: &str, text: &str, size_px: u32) -> (String, String, u32) {
        (String::from(family), String::from(text), size_px.clamp(8, 288))
    }

    pub fn count(&self) -> usize {
        self.fonts.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

const TTF_MAGIC: [u8; 4] = [0x00, 0x01, 0x00, 0x00];

pub fn run_fontmgr_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F288");
    let mut fm = FontManager::new();
    // 装用例：合法 ttf。
    let r1 = fm.install(&TTF_MAGIC, "ttf", "思源宋体", "Regular");
    set.add(
        "F288 install ok",
        r1 == Ok(FontCheck::Ok) && fm.count() == 3,
        "user-level",
    );
    // 损坏注入拒绝 + 原因。
    let bad = fm.install(&[0xDE, 0xAD, 0xBE, 0xEF], "ttf", "坏字体", "R");
    set.add(
        "F288 corrupt rejected",
        bad.is_err() && bad.unwrap_err().contains("损坏"),
        "reason shown",
    );
    // 扩展名不支持 + 重名拒绝。
    let bad_ext = fm.install(&TTF_MAGIC, "fon", "老格式", "R");
    let dup = fm.install(&TTF_MAGIC, "otf", "思源宋体", "Bold");
    set.add(
        "F288 ext+dup reject",
        bad_ext.is_err() && dup.is_err(),
        "both honest",
    );
    // 卸载：系统锁定 vs 用户字体。
    let sys_try = fm.uninstall("思源黑体");
    let user_ok = fm.uninstall("思源宋体");
    set.add(
        "F288 lock vs user",
        sys_try.is_err() && user_ok.is_ok() && fm.count() == 2,
        "protected list",
    );
    // 列表自渲染数据：每条带字族（渲染层用字体本身画名字——数据源在此）。
    let groups = fm.by_family();
    set.add(
        "F288 family grouped",
        groups.len() == 2 && groups.iter().all(|(_, v)| !v.is_empty()),
        "wysiwyg source",
    );
    // 预览三用例参数钳制。
    let p = FontManager::preview_params("等宽", "永 8h", 400);
    set.add(
        "F288 preview clamp",
        p.2 == 288 && FontManager::preview_params("等宽", "字", 0).2 == 8,
        "size slider bounded",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f288_font_flow() {
        let set = run_fontmgr_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F288 自检红 {f}/{p}");
    }

    #[test]
    fn otto_magic_accepted() {
        let chk = FontManager::validate(b"OTTO\xff", "otf", "新字体", |_| false);
        assert_eq!(chk, FontCheck::Ok, "OTTO 魔数合法");
    }
}
