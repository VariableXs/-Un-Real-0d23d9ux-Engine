//! 深化层 · F140 本地化开放（2026-09-26 回炉补深化）。
//!
//! 补深：区域格式（数字/日期随区域 F187 接口面）、RTL 前瞻接口、
//! 等义审查模板（译文审查）、翻译包 manifest（语言元数据）、
//! 复数规则最小件（中英复数差异）。

use crate::checks::CheckSet;
use crate::stareco::l10nopen::{key_ok, LangPack};

// ---------------------------------------------------------------------------
// 区域格式接口面（F187 联动）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionFormat {
    ZhCn,
    EnUs,
    DeDe,
}

/// 数字千分位：zh-CN 与 en-US 逗号，de-DE 点。
pub fn thousands_sep(r: RegionFormat) -> char {
    match r {
        RegionFormat::ZhCn | RegionFormat::EnUs => ',',
        RegionFormat::DeDe => '.',
    }
}

/// 日期短格式顺序：zh-CN 年月日 / en-US 月日年 / de-DE 日月年。
pub fn date_order(r: RegionFormat) -> (&'static str, &'static str, &'static str) {
    match r {
        RegionFormat::ZhCn => ("年", "月", "日"),
        RegionFormat::EnUs => ("月", "日", "年"),
        RegionFormat::DeDe => ("日", "月", "年"),
    }
}

// ---------------------------------------------------------------------------
// RTL 前瞻接口（布局镜像评估项——本层只做方向判定）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

/// RTL 语言前瞻清单（实现评估项，不承诺日期）。
pub fn text_direction(lang: &str) -> TextDirection {
    match lang {
        "ar" | "he" | "fa" | "ur" => TextDirection::Rtl,
        _ => TextDirection::Ltr,
    }
}

// ---------------------------------------------------------------------------
// 等义审查模板（译文审查——卷首甲三要素的译文面）
// ---------------------------------------------------------------------------

/// 等义审查三查：长度比合理（译文/源文 0.3x-3x）、无占位符丢失
/// （上层 placeholders_match）、无未译残留（译文 ≠ 源文逐字节相同
/// 除非是专名/代号）。
pub fn equivalence_ok(source: &str, translated: &str, allow_same: bool) -> Result<(), &'static str> {
    if source.is_empty() || translated.is_empty() {
        return Err("空串不参审");
    }
    let ratio = translated.chars().count() as f64 / source.chars().count() as f64;
    if !(0.3..=3.0).contains(&ratio) {
        return Err("长度比越界：疑似漏译或过译");
    }
    if source == translated && !allow_same {
        return Err("译文与源文逐字节相同：疑似未译");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 翻译包 manifest（vxtheme phrases.json 元数据）
// ---------------------------------------------------------------------------

pub struct LangManifest {
    pub lang: &'static str,
    pub display_name: &'static str,
    /// 词条 schema 版本（F126 开放格式——版本不匹配拒载）。
    pub schema_ver: u32,
    pub author: &'static str,
}

pub const SCHEMA_VER: u32 = 1;

impl LangManifest {
    pub fn loadable(&self) -> Result<(), &'static str> {
        if self.lang.len() < 2 || !self.lang.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err("语言码非法（2+ 小写字母）");
        }
        if self.display_name.is_empty() || self.author.is_empty() {
            return Err("显示名/作者必填");
        }
        if self.schema_ver != SCHEMA_VER {
            return Err("schema 版本不匹配：请按当前词条规范重打包");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 复数规则最小件（中英差异）
// ---------------------------------------------------------------------------

/// 复数选择：中文无复数变化（单 key）；英文 n=1 用 one，其余 other。
pub fn plural_category(lang: &str, n: u64) -> &'static str {
    match lang {
        "en" => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
        _ => "other",
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F140D_TAG: &str = "stareco-F140-deep";

pub fn run_f140_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F140D_TAG);

    // 区域格式
    set.add(
        "f140d separators",
        thousands_sep(RegionFormat::ZhCn) == ',' && thousands_sep(RegionFormat::DeDe) == '.',
        "随区域",
    );
    set.add(
        "f140d date orders",
        date_order(RegionFormat::ZhCn).0 == "年" && date_order(RegionFormat::EnUs).0 == "月" && date_order(RegionFormat::DeDe).0 == "日",
        "三区三序",
    );

    // RTL 前瞻
    set.add(
        "f140d rtl lookahead",
        text_direction("ar") == TextDirection::Rtl && text_direction("zh") == TextDirection::Ltr,
        "判定接口就位",
    );

    // 等义审查
    set.add(
        "f140d equivalence pass",
        equivalence_ok("Open", "打开", false).is_ok(),
        "正常译文",
    );
    set.add(
        "f140d untranslated caught",
        equivalence_ok("Open", "Open", false).is_err() && equivalence_ok("Open", "Open", true).is_ok(),
        "未译检出（专名豁免）",
    );
    set.add(
        "f140d over-translation caught",
        equivalence_ok("OK", "这是一段远远超出原文长度的所谓翻译内容一定有问题", false).is_err(),
        "长度比越界",
    );

    // 包 manifest
    let good = LangManifest { lang: "ja", display_name: "日本語", schema_ver: 1, author: "community" };
    set.add("f140d manifest ok", good.loadable().is_ok(), "合法");
    let bad_ver = LangManifest { schema_ver: 99, ..good };
    set.add("f140d schema mismatch", bad_ver.loadable().is_err(), "版本闸");
    let bad_lang = LangManifest { lang: "JP", ..good };
    set.add("f140d lang code law", bad_lang.loadable().is_err(), "小写语言码");

    // 复数
    set.add(
        "f140d plural rules",
        plural_category("en", 1) == "one" && plural_category("en", 2) == "other" && plural_category("zh", 1) == "other",
        "中英差异",
    );

    // key 规范联动（深化层复诵）
    set.add("f140d key law intact", key_ok("a.b.c") && !key_ok("A.b.c"), "三级小写");

    // 语言包词条 key 全过 schema（装载前扫描）
    let mut pk = LangPack::new("ja");
    pk.put(crate::stareco::l10nopen::Entry { key: "ui.main.open", value: "開く", context: "button" }).ok();
    set.add("f140d pack put ok", pk.len() == 1, "schema gate reused");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn direction_and_plural() {
        assert_eq!(text_direction("he"), TextDirection::Rtl);
        assert_eq!(plural_category("en", 0), "other");
    }
}
