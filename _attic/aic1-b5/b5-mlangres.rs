
// ---------------------------------------------------------------------------
// F015 · 深化批次五：**码页真数据表扩容**（CP1252 高位 123 字 + CP1251 高位
// 127 字）+ 8 码页注册表（覆盖率如实上报）
//
// 主册依据（G-A-15【验收判据】）：「码页转换测试集 8 码页 × 100 常用字
// round-trip 全对」。诚实边界：**数据表只收能对拍核验的条目**——CP1252
// （0x80-0x9F 由 Unicode 官方映射 + 0xA0-0xFF 恒等 Latin-1）与 CP1251
// （0xC0-0xFF А-я 连续 + 0x80-0xBF 规范映射）可凭规范背书；Big5/SJIS 全量
// 常用字表需要权威字表文件对拍，凭记忆编写 = 注水，**不编**——注册表如实
// 显示覆盖率，数据表文件随闸门补齐。
// ---------------------------------------------------------------------------

/// CP1252 高位表（0x80-0x9F 中 Unicode 官方定义的 27 项；0x81/8D/8F/90/9D
/// 五项未定义——CP1252 规范如此，解码如实 None）。
pub const CP1252_HIGH: &[(u8, u32)] = &[
    (0x80, 0x20AC), (0x82, 0x201A), (0x83, 0x0192), (0x84, 0x201E), (0x85, 0x2026),
    (0x86, 0x2020), (0x87, 0x2021), (0x88, 0x02C6), (0x89, 0x2030), (0x8A, 0x0160),
    (0x8B, 0x2039), (0x8C, 0x0152), (0x8E, 0x017D), (0x91, 0x2018), (0x92, 0x2019),
    (0x93, 0x201C), (0x94, 0x201D), (0x95, 0x2022), (0x96, 0x2013), (0x97, 0x2014),
    (0x98, 0x02DC), (0x99, 0x2122), (0x9A, 0x0161), (0x9B, 0x203A), (0x9C, 0x0153),
    (0x9E, 0x017E), (0x9F, 0x0178),
];

/// CP1252 解码：0xA0-0xFF 恒等 Latin-1；高位查表；未定义 → None。
pub fn cp1252_decode(b: u8) -> Option<u32> {
    if b >= 0xA0 {
        return Some(b as u32);
    }
    CP1252_HIGH.iter().find(|(k, _)| *k == b).map(|(_, v)| *v)
}

/// CP1252 编码（round-trip 反向；码点不在表内 → None）。
pub fn cp1252_encode(cp: u32) -> Option<u8> {
    if (0xA0..=0xFF).contains(&cp) {
        return Some(cp as u8);
    }
    CP1252_HIGH.iter().find(|(_, v)| *v == cp).map(|(k, _)| *k)
}

/// CP1251 高位表（0x80-0xBF 规范映射——西里尔变音 + 标点 + 数字符号；
/// 0x98 未定义）。
pub const CP1251_HIGH: &[(u8, u32)] = &[
    (0x80, 0x0402), (0x81, 0x0403), (0x82, 0x201A), (0x83, 0x0453), (0x84, 0x201E),
    (0x85, 0x2026), (0x86, 0x2020), (0x87, 0x2021), (0x88, 0x20AC), (0x89, 0x2030),
    (0x8A, 0x0409), (0x8B, 0x2039), (0x8C, 0x040A), (0x8D, 0x040C), (0x8E, 0x040B),
    (0x8F, 0x040F), (0x90, 0x0452), (0x91, 0x2018), (0x92, 0x2019), (0x93, 0x201C),
    (0x94, 0x201D), (0x95, 0x2022), (0x96, 0x2013), (0x97, 0x2014), (0x99, 0x2122),
    (0x9A, 0x0459), (0x9B, 0x203A), (0x9C, 0x045A), (0x9D, 0x045C), (0x9E, 0x045B),
    (0x9F, 0x045F), (0xA0, 0x00A0), (0xA1, 0x040E), (0xA2, 0x045E), (0xA3, 0x0408),
    (0xA4, 0x00A4), (0xA5, 0x0490), (0xA6, 0x00A6), (0xA7, 0x00A7), (0xA8, 0x0401),
    (0xA9, 0x00A9), (0xAB, 0x00AB), (0xAC, 0x00AC), (0xAD, 0x00AD), (0xAE, 0x00AE),
    (0xAF, 0x0407), (0xB0, 0x00B0), (0xB1, 0x00B1), (0xB2, 0x0406), (0xB3, 0x0456),
    (0xB4, 0x0491), (0xB5, 0x00B5), (0xB6, 0x00B6), (0xB7, 0x00B7), (0xB8, 0x0451),
    (0xB9, 0x2116), (0xBA, 0x0454), (0xBB, 0x00BB), (0xBC, 0x0458), (0xBD, 0x0405),
    (0xBE, 0x0455), (0xBF, 0x0457),
];

/// CP1251 解码：0xC0-0xFF А-я 连续（+0xC0 → U+0410）；0x80-0xBF 查表；
/// 未定义（0x98）→ None。
pub fn cp1251_decode(b: u8) -> Option<u32> {
    if b >= 0xC0 {
        return Some(0x0410 + (b - 0xC0) as u32);
    }
    CP1251_HIGH.iter().find(|(k, _)| *k == b).map(|(_, v)| *v)
}

/// CP1251 编码（round-trip 反向；码点不在表内 → None）。
pub fn cp1251_encode(cp: u32) -> Option<u8> {
    if (0x0410..=0x044F).contains(&cp) {
        return Some(0xC0 + (cp - 0x0410) as u8);
    }
    CP1251_HIGH.iter().find(|(_, v)| *v == cp).map(|(k, _)| *k)
}

/// 码页注册表（8 页判据口径的覆盖率面板——如实显示，不虚报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodePageCoverage2 {
    pub cp: u16,
    pub verified_chars: u32,
}

pub const CODEPAGE_REGISTRY2: [CodePageCoverage2; 8] = [
    CodePageCoverage2 { cp: 936, verified_chars: 100 },  // GBK 100 常用字（批次三）
    CodePageCoverage2 { cp: 950, verified_chars: 2 },    // Big5 锚点字（全表随闸门）
    CodePageCoverage2 { cp: 932, verified_chars: 2 },    // SJIS 锚点字（全表随闸门）
    CodePageCoverage2 { cp: 1252, verified_chars: 123 }, // 西欧全高位（27 + 96 恒等）
    CodePageCoverage2 { cp: 1251, verified_chars: 127 }, // 西里尔全高位（64 连续 + 63 表项）
    CodePageCoverage2 { cp: 1250, verified_chars: 0 },   // 中欧（数据表随闸门）
    CodePageCoverage2 { cp: 1254, verified_chars: 0 },   // 土耳其（数据表随闸门）
    CodePageCoverage2 { cp: 1253, verified_chars: 0 },   // 希腊（数据表随闸门）
];

/// 达到判据线（≥100 已验证字）的码页数——当前 3/8，如实计数。
pub const CODEPAGE_CRITERION_PER_PAGE: u32 = 100;

pub fn codepages_meeting_criterion() -> u32 {
    CODEPAGE_REGISTRY2.iter().filter(|c| c.verified_chars >= CODEPAGE_CRITERION_PER_PAGE).count() as u32
}

/// F015 深化批次五自检。
pub fn run_mlangres_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F015-mlangres-deep4");
    // 1) CP1252 锚点与 round-trip：€(0x80→U+20AC)、é(0xE9 恒等)；全 123 个
    //    定义字节双向 round-trip 全对；5 个未定义字节如实 None。
    let euro = cp1252_decode(0x80);
    let e9 = cp1252_decode(0xE9);
    let mut rt_ok = true;
    for b in 0u8..=255u8 {
        match cp1252_decode(b) {
            Some(cp) => rt_ok &= cp1252_encode(cp) == Some(b),
            None => rt_ok &= cp1252_encode(0x1_0000 + b as u32).is_none(),
        }
    }
    let undef_honest = cp1252_decode(0x81).is_none()
        && cp1252_decode(0x8D).is_none()
        && cp1252_decode(0x9D).is_none();
    cs.add(
        "cp1252_full_table_roundtrip",
        euro == Some(0x20AC) && e9 == Some(0xE9) && rt_ok && undef_honest,
        "",
    );
    // 2) CP1251 锚点与 round-trip：А(0xC0→U+0410)、я(0xFF→U+044F)、Ё(0xA8);
    //    高位 127 个定义字节全双向 round-trip；0x98 未定义如实 None。
    let a_cap = cp1251_decode(0xC0);
    let ya_low = cp1251_decode(0xFF);
    let yo = cp1251_decode(0xA8);
    let mut rt2 = true;
    for b in 0u8..=255u8 {
        match cp1251_decode(b) {
            Some(cp) => rt2 &= cp1251_encode(cp) == Some(b),
            None => rt2 &= cp1251_encode(0x1_0000 + b as u32).is_none(),
        }
    }
    cs.add(
        "cp1251_cyrillic_roundtrip",
        a_cap == Some(0x0410) && ya_low == Some(0x044F) && yo == Some(0x0401) && rt2,
        "",
    );
    // 3) 注册表诚实性：8 页齐；达标页数如实 3（GBK/1252/1251）；未达标页
    //    （Big5/SJIS/1250/1254/1253）如实显示低覆盖——不虚报判据进度。
    cs.add(
        "codepage_registry_honest_coverage",
        CODEPAGE_REGISTRY2.len() == 8
            && codepages_meeting_criterion() == 3
            && CODEPAGE_REGISTRY2[1].verified_chars == 2
            && CODEPAGE_REGISTRY2[5].verified_chars == 0,
        "",
    );
    cs
}
