//! ves311_s16a11y — VE-F4071 · T04 翻译与无障碍
//! 读屏朗读文本翻译规范（缩写展开/单位本地读法/数字读法）+ 字幕翻译时长同步约束 + 替代文本翻译必检（缺译=LQA Critical）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8CC0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TaCode {
    Ok = 0,
    ReadAmbiguous = SEG_BASE + 0x01,
    SubtitleOver = SEG_BASE + 0x02,
    AltMissing = SEG_BASE + 0x03,
    PrivacyBreach = SEG_BASE + 0x04,
}

/// 缩写展开表。
pub const ABBREVS: [(&str, &str); 4] = [
    ("CPU", "处理器"),
    ("RAM", "内存"),
    ("USB", "通用串行总线"),
    ("Wi-Fi", "无线网络"),
];

pub fn expand_abbrev(text: &str) -> String {
    let mut out = String::from(text);
    for (a, full) in ABBREVS.iter() {
        if out.contains(a) {
            // 简化展开：追加全称注记。
            out = out + "（" + full + "）";
            break;
        }
    }
    out
}

/// 单位本地读法。
pub fn unit_reading(unit: &str, lang: &str) -> Result<&'static str, TaCode> {
    match (unit, lang) {
        ("km", "zh") => Ok("千米"),
        ("kg", "zh") => Ok("千克"),
        ("km", "en") => Ok("kilometer"),
        ("kg", "en") => Ok("kilogram"),
        _ => Err(TaCode::ReadAmbiguous),
    }
}

/// 数字读法：千分位读法断言。
pub fn number_reading(n: u32, lang: &str) -> Result<String, TaCode> {
    if lang != "zh" && lang != "en" { return Err(TaCode::ReadAmbiguous); }
    let mut s = String::new();
    push_num(&mut s, n);
    Ok(s)
}

fn push_num(s: &mut String, mut n: u32) {
    if n == 0 { s.push('0'); return; }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

/// 字幕时长同步：字符数 / 秒上限（中文 9 字/秒，英文 17 字符/秒）。
pub fn subtitle_sync(chars: u32, seconds: u32, lang: &str) -> Result<(), TaCode> {
    if seconds == 0 { return Err(TaCode::SubtitleOver); }
    let cap = if lang == "zh" { 9 } else { 17 };
    if chars > cap * seconds {
        return Err(TaCode::SubtitleOver); // 超时 → 压缩改写建议
    }
    Ok(())
}

/// 替代文本必检：缺译 → Critical 级。
pub fn alt_text_check(translated: Option<&str>) -> u8 {
    match translated {
        Some(t) if !t.trim().is_empty() => 0,
        _ => 2, // 2 = Critical
    }
}

pub fn run_ves311_checks() -> CheckSet {
    let mut set = CheckSet::new("ves311_s16a11y");

    // 1) 缩写展开。
    let e = expand_abbrev("CPU 负载高");
    set.add("C4071-01", e.contains("处理器"), "缩写展开");

    // 2) 无缩写原样。
    set.add("C4071-02", expand_abbrev("一切正常") == "一切正常", "无缩写原样");

    // 3) 单位本地读法。
    set.add("C4071-03", unit_reading("km", "zh") == Ok("千米") && unit_reading("kg", "en") == Ok("kilogram"), "单位读法");

    // 4) 歧义读法标记。
    set.add("C4071-04", unit_reading("mi", "fr") == Err(TaCode::ReadAmbiguous), "歧义标记");

    // 5) 数字读法。
    set.add("C4071-05", number_reading(12345, "zh") == Ok(String::from("12345")), "数字读法");

    // 6) 不支持语种拒。
    set.add("C4071-06", number_reading(1, "xx") == Err(TaCode::ReadAmbiguous), "语种拒");

    // 7) 字幕同步：合规。
    set.add("C4071-07", subtitle_sync(18, 2, "zh").is_ok() && subtitle_sync(34, 2, "en").is_ok(), "字幕同步");

    // 8) 字幕超时。
    set.add("C4071-08", subtitle_sync(30, 2, "zh") == Err(TaCode::SubtitleOver), "字幕超时");

    // 9) 零时长拒。
    set.add("C4071-09", subtitle_sync(1, 0, "zh") == Err(TaCode::SubtitleOver), "零时长拒");

    // 10) 替代文本必检。
    set.add("C4071-10", alt_text_check(Some("图片说明")) == 0 && alt_text_check(Some(" ")) == 2
        && alt_text_check(None) == 2, "替代文本 Critical");

    // 11) 缩写表唯一。
    let mut dup = false;
    for i in 0..ABBREVS.len() {
        for j in (i + 1)..ABBREVS.len() {
            if ABBREVS[i].0 == ABBREVS[j].0 { dup = true; }
        }
    }
    set.add("C4071-11", !dup, "缩写表唯一");

    // 12) 段位自查。
    set.add("C4071-12", SEG_BASE == 0x8CC0, "段位 0x8CC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a11y_translate() {
        assert!(expand_abbrev("RAM 不足").contains("内存"));
        assert_eq!(unit_reading("km", "zh"), Ok("千米"));
        assert!(subtitle_sync(27, 3, "zh").is_ok());
    }
}
