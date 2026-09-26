
// ---------------------------------------------------------------------------
// F015 · 深化批次六：**双表合并**（decode() 切到 MAP 面，消重）+ SJIS 半角
// 片假名（0xA1-0xDF → U+FF61-FF9F，95 字）+ **CP437 全高位表**（128 字）+
// 注册表更新（达标 4/8）
//
// 主册依据（G-A-15【验收判据】「8 码页 × 100 常用字 round-trip 全对」；批次五
// 缺陷 #2 登记：decode() (u8,&str) 面与 MAP (u8,u32) 面两套并存——本批合并：
// decode() 切 MAP 面消重，旧 (u8,&str) 表删除）。
// ---------------------------------------------------------------------------

/// 码点 → UTF-8 推入（DecodeResult 消费；本域码点域 ≤ U+FF9F，实现全 4 字节
/// 形态以通用）。
fn utf8_push_codepoint(out: &mut DecodeResult, cp: u32) {
    let enc: [u8; 4] = match cp {
        0..=0x7F => [cp as u8, 0, 0, 0],
        0x80..=0x7FF => {
            [0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8, 0, 0]
        }
        0x800..=0xFFFF => [
            0xE0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
            0,
        ],
        _ => [
            0xF0 | ((cp >> 18) & 0x07) as u8,
            0x80 | ((cp >> 12) & 0x3F) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ],
    };
    let n = if cp <= 0x7F {
        1
    } else if cp <= 0x7FF {
        2
    } else if cp <= 0xFFFF {
        3
    } else {
        4
    };
    for &b in enc.iter().take(n) {
        if out.len < out.utf8.len() {
            out.utf8[out.len] = b;
            out.len += 1;
        }
    }
}

/// CP437 全高位表（0x80-0xFF 128 项——DOS 拉丁/制表/希腊/数学字符，规范钉值）。
pub const CP437_MAP: &[(u8, u32)] = &[
    (0x80, 0x00C7), (0x81, 0x00FC), (0x82, 0x00E9), (0x83, 0x00E2), (0x84, 0x00E4),
    (0x85, 0x00E0), (0x86, 0x00E5), (0x87, 0x00E7), (0x88, 0x00EA), (0x89, 0x00EB),
    (0x8A, 0x00E8), (0x8B, 0x00EF), (0x8C, 0x00EE), (0x8D, 0x00EC), (0x8E, 0x00C4),
    (0x8F, 0x00C5), (0x90, 0x00C9), (0x91, 0x00E6), (0x92, 0x00C6), (0x93, 0x00F4),
    (0x94, 0x00F6), (0x95, 0x00F2), (0x96, 0x00FB), (0x97, 0x00F9), (0x98, 0x00FF),
    (0x99, 0x00D6), (0x9A, 0x00DC), (0x9B, 0x00A2), (0x9C, 0x00A3), (0x9D, 0x00A5),
    (0x9E, 0x20A7), (0x9F, 0x0192), (0xA0, 0x00E1), (0xA1, 0x00ED), (0xA2, 0x00F3),
    (0xA3, 0x00FA), (0xA4, 0x00F1), (0xA5, 0x00D1), (0xA6, 0x00AA), (0xA7, 0x00BA),
    (0xA8, 0x00BF), (0xA9, 0x2310), (0xAA, 0x00AC), (0xAB, 0x00BD), (0xAC, 0x00BC),
    (0xAD, 0x00A1), (0xAE, 0x00AB), (0xAF, 0x00BB), (0xB0, 0x2591), (0xB1, 0x2592),
    (0xB2, 0x2593), (0xB3, 0x2502), (0xB4, 0x2524), (0xB5, 0x2561), (0xB6, 0x2562),
    (0xB7, 0x2556), (0xB8, 0x2555), (0xB9, 0x2563), (0xBA, 0x2551), (0xBB, 0x2557),
    (0xBC, 0x255D), (0xBD, 0x255C), (0xBE, 0x255B), (0xBF, 0x2510), (0xC0, 0x2514),
    (0xC1, 0x2534), (0xC2, 0x252C), (0xC3, 0x251C), (0xC4, 0x2500), (0xC5, 0x253C),
    (0xC6, 0x255E), (0xC7, 0x255F), (0xC8, 0x255A), (0xC9, 0x2554), (0xCA, 0x2569),
    (0xCB, 0x2566), (0xCC, 0x2560), (0xCD, 0x2550), (0xCE, 0x256C), (0xCF, 0x2567),
    (0xD0, 0x2568), (0xD1, 0x2564), (0xD2, 0x2565), (0xD3, 0x2559), (0xD4, 0x2558),
    (0xD5, 0x2552), (0xD6, 0x2553), (0xD7, 0x256B), (0xD8, 0x256A), (0xD9, 0x2518),
    (0xDA, 0x250C), (0xDB, 0x2588), (0xDC, 0x2584), (0xDD, 0x258C), (0xDE, 0x2590),
    (0xDF, 0x2580), (0xE0, 0x03B1), (0xE1, 0x00DF), (0xE2, 0x0393), (0xE3, 0x03C0),
    (0xE4, 0x03A3), (0xE5, 0x03C3), (0xE6, 0x00B5), (0xE7, 0x03C4), (0xE8, 0x03A6),
    (0xE9, 0x0398), (0xEA, 0x03A9), (0xEB, 0x03B4), (0xEC, 0x221E), (0xED, 0x03C6),
    (0xEE, 0x03B5), (0xEF, 0x2229), (0xF0, 0x2261), (0xF1, 0x00B1), (0xF2, 0x2265),
    (0xF3, 0x2264), (0xF4, 0x2320), (0xF5, 0x2321), (0xF6, 0x00F7), (0xF7, 0x2248),
    (0xF8, 0x00B0), (0xF9, 0x2219), (0xFA, 0x00B7), (0xFB, 0x221A), (0xFC, 0x207F),
    (0xFD, 0x00B2), (0xFE, 0x25A0), (0xFF, 0x00A0),
];

/// CP437 解码：ASCII 直通外的 0x80-0xFF 全表；表外无未定义项（128 全覆盖）。
pub fn cp437_decode(b: u8) -> Option<u32> {
    if b < 0x80 {
        return Some(b as u32);
    }
    CP437_MAP.iter().find(|(k, _)| *k == b).map(|(_, v)| *v)
}

pub fn cp437_encode(cp: u32) -> Option<u8> {
    if cp < 0x80 {
        return Some(cp as u8);
    }
    CP437_MAP.iter().find(|(_, v)| *v == cp).map(|(k, _)| *k)
}

/// SJIS 半角片假名（单字节 0xA1-0xDF → U+FF61-FF9F——规范连续映射，95 字）。
pub const SJIS_HALFWIDTH_KANA_COUNT: u32 = 95;

pub fn sjis_halfwidth_decode(b: u8) -> Option<u32> {
    if (0xA1..=0xDF).contains(&b) {
        Some(0xFF61 + (b - 0xA1) as u32)
    } else {
        None
    }
}

/// 达标线（≥100 已验证字）——沿用批次五口径。
pub const CP437_VERIFIED: u32 = 128;
pub const CP1251_VERIFIED: u32 = 127;
pub const CP1252_VERIFIED: u32 = 123;
pub const SJIS_VERIFIED: u32 = SJIS_HALFWIDTH_KANA_COUNT + 2; // 2 = 既有全角锚点

/// F015 深化批次六自检。
pub fn run_mlangres_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F015-mlangres-deep5");
    // 1) 双表合并：decode(Cp1252) 走 MAP 面——0x80 欧元、0xE9 恒等、0x81 未
    //    定义如实 FFFD 计数（规范正确性优先于旧 Latin-1 兜底）。
    let e0 = decode(CodePage::Cp1252, &[0x80]);
    let e1 = decode(CodePage::Cp1252, &[0xE9]);
    let e2 = decode(CodePage::Cp1252, &[0x81]);
    cs.add(
        "decode_uses_map_cp1252",
        &e0.utf8[..e0.len] == "\u{20AC}".as_bytes()
            && &e1.utf8[..e1.len] == "\u{E9}".as_bytes()
            && e2.replacement_chars == 1,
        "",
    );
    // 2) decode(Cp1251) 走 MAP 面：А/я/Ё 全出（旧 6 字子集 → 127 字超集，
    //    既有锚点测试不破坏）；0x98 未定义 FFFD。
    let r1 = decode(CodePage::Cp1251, &[0xC0]);
    let r2 = decode(CodePage::Cp1251, &[0xFF]);
    let r3 = decode(CodePage::Cp1251, &[0xA8]);
    let r4 = decode(CodePage::Cp1251, &[0x98]);
    cs.add(
        "decode_uses_map_cp1251",
        &r1.utf8[..r1.len] == "\u{0410}".as_bytes()
            && &r2.utf8[..r2.len] == "\u{044F}".as_bytes()
            && &r3.utf8[..r3.len] == "\u{0401}".as_bytes()
            && r4.replacement_chars == 1,
        "",
    );
    // 3) SJIS 半角片假名：单字节 0xA1→｡、0xDF→ﾟ（U+FF9F）；全角锚点不破坏。
    let k1 = decode(CodePage::ShiftJis, &[0xA1]);
    let k2 = decode(CodePage::ShiftJis, &[0xDF]);
    let k3 = decode(CodePage::ShiftJis, &[0x82, 0xA0]);
    cs.add(
        "sjis_halfwidth_kana",
        &k1.utf8[..k1.len] == "\u{FF61}".as_bytes()
            && &k2.utf8[..k2.len] == "\u{FF9F}".as_bytes()
            && &k3.utf8[..k3.len] == "あ".as_bytes(),
        "",
    );
    // 4) CP437：制表符/希腊/数学锚点 + 全 256 字节 round-trip（128 全覆盖）。
    let c1 = cp437_decode(0xB3);
    let c2 = cp437_decode(0xE0);
    let c3 = cp437_decode(0xF8);
    let mut rt = true;
    for b in 0u8..=255u8 {
        rt &= cp437_encode(cp437_decode(b).unwrap()) == Some(b);
    }
    cs.add(
        "cp437_full_table_roundtrip",
        c1 == Some(0x2502) && c2 == Some(0x03B1) && c3 == Some(0x25A0) && rt,
        "",
    );
    // 5) 注册表更新：达标 4/8（936/1252/1251/437）；932 升至 97 如实差 3 字。
    cs.add(
        "registry_progress_4_of_8",
        codepages_meeting_criterion() == 4
            && SJIS_VERIFIED == 97
            && CODEPAGE_REGISTRY2.iter().any(|c| c.cp == 437 && c.verified_chars == 128),
        "",
    );
    cs
}
