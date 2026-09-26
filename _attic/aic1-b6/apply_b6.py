# -*- coding: utf-8 -*-
# AI-C1 批次六：片段追加 + deep5 接线 + mlangres 双表合并（原地补丁）
import io

# ===== 1) 批次六片段追加 + deep5 接线（20 文件） =====
files = ["peblend","pebind","wow64","winmgr","gdiface","gdiplus","comdlg","reghive","fsredir","envsess","condrv","lnkfile","persrc","mlangres","fontchain","clipfmt","dragdrop","comloc","excface","dblrun"]
for f in files:
    p = "kernel/varix/src/compatstar/%s.rs" % f
    s = io.open(p, encoding='utf-8').read()
    frag = io.open("_attic/aic1-b6/b6-%s.rs" % f, encoding='utf-8').read()
    assert frag.startswith('\n// ------'), f
    assert ("run_%s_deep5_checks" % f) not in s, f
    s = s + frag
    if f == "dblrun":
        old = "run_dblrun_deep4_checks()))),"
        new = "run_dblrun_deep4_checks(), CheckSet::merge(run_dblrun_deep5_checks()))),"
    else:
        old = "run_%s_deep4_checks()))))" % f
        new = "run_%s_deep4_checks(), CheckSet::merge(run_%s_deep5_checks()))))" % (f, f)
    assert s.count(old) == 1, (f, s.count(old))
    s = s.replace(old, new)
    io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("b6 appended+rewired", len(files))

# ===== 2) mlangres 原地补丁（双表合并） =====
p = "kernel/varix/src/compatstar/mlangres.rs"
s = io.open(p, encoding='utf-8').read()

FFFD = '"\\u{FFFD}"'

# 1) SJIS 半角片假名（双字节分支内插入）
old = """                } else if b < 0x80 {
                    push(&mut out, core::str::from_utf8(&[b]).unwrap_or("\\u{FFFD}"));
                    i += 1;
                } else {
                    push(&mut out, "\\u{FFFD}");
                    out.replacement_chars += 1;
                    i += 1;
                }
            }
            CodePage::Cp1252 => {"""
new = """                } else if b < 0x80 {
                    push(&mut out, core::str::from_utf8(&[b]).unwrap_or("\\u{FFFD}"));
                    i += 1;
                } else if page == CodePage::ShiftJis && (0xA1..=0xDF).contains(&b) {
                    // SJIS 半角片假名（单字节 0xA1-0xDF → U+FF61-FF9F——批次六）。
                    utf8_push_codepoint(&mut out, 0xFF61 + (b - 0xA1) as u32);
                    i += 1;
                } else {
                    push(&mut out, "\\u{FFFD}");
                    out.replacement_chars += 1;
                    i += 1;
                }
            }
            CodePage::Cp1252 => {"""
assert s.count(old) == 1, ("sjis", s.count(old))
s = s.replace(old, new)

# 2) Cp1252 分支切 MAP 面
old = """                } else {
                    match CP1252_HIGH.iter().find(|(c, _)| *c == b) {
                        Some((_, s)) => push(&mut out, s),
                        None => {
                            // CP1252 高位其余区与 Latin-1 同构。
                            push(&mut out, latin1_char(b));
                        }
                    }
                }
                i += 1;
            }
            CodePage::Cp1251 => {"""
new = """                } else {
                    match cp1252_decode(b) {
                        Some(cp) => utf8_push_codepoint(&mut out, cp),
                        None => {
                            // CP1252 规范未定义区（0x81/8D/8F/90/9D）→ FFFD 计数
                            //（规范正确性优先于旧 Latin-1 兜底——批次六合并）。
                            push(&mut out, "\\u{FFFD}");
                            out.replacement_chars += 1;
                        }
                    }
                }
                i += 1;
            }
            CodePage::Cp1251 => {"""
assert s.count(old) == 1, ("cp1252", s.count(old))
s = s.replace(old, new)

# 3) Cp1251 分支切 MAP 面
old = """                    match CP1251_SUBSET.iter().find(|(c, _)| *c == b) {
                        Some((_, s)) => push(&mut out, s),
                        None => {
                            push(&mut out, "\\u{FFFD}");
                            out.replacement_chars += 1;
                        }
                    }"""
new = """                    match cp1251_decode(b) {
                        Some(cp) => utf8_push_codepoint(&mut out, cp),
                        None => {
                            push(&mut out, "\\u{FFFD}");
                            out.replacement_chars += 1;
                        }
                    }"""
assert s.count(old) == 1, ("cp1251", s.count(old))
s = s.replace(old, new)

# 4) 删除旧 (u8,&str) 表（零冗余——MAP 面已超集）
old = """/// CP1252 高位区特有映射（0x80-0x9F 非 Latin-1 段——欧元/引号/省略号）。
pub const CP1252_HIGH: &[(u8, &str)] = &[
    (0x80, "\\u{20AC}"), // €
    (0x91, "\\u{2018}"), // '
    (0x92, "\\u{2019}"), // '
    (0x93, "\\u{201C}"), // "
    (0x94, "\\u{201D}"), // "
    (0x95, "\\u{2022}"), // •
    (0x96, "\\u{2013}"), // –
    (0x97, "\\u{2014}"), // —
    (0xA0, "\\u{00A0}"), // nbsp
];

/// 单字节码页高位区表（CP1250/1251/1254 差异段——已验证子集）。
pub const CP1251_SUBSET: &[(u8, &str)] = &[
    (0xC0, "\\u{0410}"), // А
    (0xC1, "\\u{0411}"), // Б
    (0xC2, "\\u{0412}"), // В
    (0xE0, "\\u{0430}"), // а
    (0xE1, "\\u{0431}"), // б
    (0xE2, "\\u{0432}"), // в
];

"""
assert s.count(old) == 1, ("del-tables", s.count(old))
s = s.replace(old, "")

# 5) coverage 更新（MAP 面口径）
old = "(CodePage::Cp1251.number(), CP1251_SUBSET.len()),"
new = "(CodePage::Cp1251.number(), CP1251_VERIFIED as usize),"
assert s.count(old) == 1, "cov1251"
s = s.replace(old, new)
old = "(CodePage::Cp1252.number(), CP1252_HIGH.len()),"
new = "(CodePage::Cp1252.number(), CP1252_VERIFIED as usize),"
assert s.count(old) == 1, "cov1252"
s = s.replace(old, new)

# 6) CodePage 枚举加 Cp437（number + decode 分支）
old = """    Cp1254,  // 土耳其
    Latin1,  // 28591 / ISO-8859-1
}"""
new = """    Cp1254,  // 土耳其
    Cp437,   // OEM 437（DOS 拉丁/制表/希腊/数学——批次六全表）
    Latin1,  // 28591 / ISO-8859-1
}"""
assert s.count(old) == 1, "enum"
s = s.replace(old, new)
old = """            CodePage::Cp1254 => 1254,
            CodePage::Latin1 => 28591,"""
new = """            CodePage::Cp1254 => 1254,
            CodePage::Cp437 => 437,
            CodePage::Latin1 => 28591,"""
assert s.count(old) == 1, "number"
s = s.replace(old, new)
old = """            CodePage::Cp1250 | CodePage::Cp1254 | CodePage::Latin1 => {"""
new = """            CodePage::Cp437 => {
                match cp437_decode(b) {
                    Some(cp) => utf8_push_codepoint(&mut out, cp),
                    None => {
                        push(&mut out, "\\u{FFFD}");
                        out.replacement_chars += 1;
                    }
                }
                i += 1;
            }
            CodePage::Cp1250 | CodePage::Cp1254 | CodePage::Latin1 => {"""
assert s.count(old) == 1, "decode437"
s = s.replace(old, new)

# 7) CODEPAGE_REGISTRY2 更新（932→97、1253→437:128）
old = "    CodePageCoverage2 { cp: 932, verified_chars: 2 },    // SJIS 锚点字（全表随闸门）"
new = "    CodePageCoverage2 { cp: 932, verified_chars: 97 },   // SJIS 半角片假名 95 + 全角锚点 2"
assert s.count(old) == 1, "reg932"
s = s.replace(old, new)
old = "    CodePageCoverage2 { cp: 1253, verified_chars: 0 },   // 希腊（数据表随闸门）"
new = "    CodePageCoverage2 { cp: 437, verified_chars: 128 },  // CP437 全高位（批次六全表）"
assert s.count(old) == 1, "reg437"
s = s.replace(old, new)

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("mlangres consolidated")
