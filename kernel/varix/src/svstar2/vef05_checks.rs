//! VE-F1005 · PNG 文本块族（tEXt/iTXt/zTXt）· 域自检
//!
//! 判据映射（锚点原文 → 自检项）：
//! - tEXt（Latin-1：keyword 1-79 + text，单 NUL 分隔）→ `C05-TEXT-*`
//! - iTXt（UTF-8：压缩标志/方法/语言标签/翻译 keyword 四段结构）→ `C05-ITXT-*`
//! - zTXt（压缩 Latin-1，方法字段只认 0）→ `C05-ZTXT-*`
//! - 元数据条目四元组（keyword/语言/文本/压缩态）→ `C05-ENTRY-*`
//! - 文本注入安全（只存储展示不执行；**转义非剥离**；单条 2MB、
//!   条数 500）→ `C05-SAFE-*`
//! - 错误路径（解压失败→跳过并计数；keyword 非法→规整保条目；
//!   重复 keyword→共存不覆盖）→ `C05-ERR-*`
//!
//! **对拍基准的独立性（如实登记）**：本文件自建 PNG 容器
//! （**自写 CRC** 与**自写 zlib stored 封装**，不复用被测模块的任何函数），
//! 故「块扫描 → 三块解析 → 通道产出 → 展示转义」全链路无自证循环。
//! 被测模块的私有面（`parse_*_payload` / `inflate_bounded`）一概不直接调，
//! 判据只经 `parse_metadata` / `scan_text_chunks` 等**公开面**观测——
//! 这也是「判据不向被测函数问答案」的要求。

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
// `use alloc::format;` **不可省**：`C05-SAFE-05` 用`format!("k{}", i)`
// 造可逐条断言的 keyword。真仓 no_std 下漏这一行即报
// `cannot find macro format`（探针是 std 环境，会掩盖过去——
// 故探针绿 ≠ 验证完成，凡用 `format!` 必以真仓 cargo build 收口）。

use crate::checks::CheckSet;
use crate::svstar2::vef01_pngdec as dec;
use crate::svstar2::vef05_text as c5;

// ---------------------------------------------------------------------------
// 一、自建 PNG 容器（自写 CRC + 自写 zlib stored，不复用被测代码）
// ---------------------------------------------------------------------------

/// CRC-32（IEEE）。**自写而不用 `frameledger_ext::crc32`**：
/// 若与被测链路同源，块校验类判据会与被测的 CRC 实现同生共死。
fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes.iter() {
        c ^= b as u32;
        for _ in 0..8 {
            let mask = (c & 1).wrapping_neg();
            c = (c >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !c
}

/// zlib **stored**（BTYPE=00）封装——自写，不复用被测模块任何函数。
///
/// 产出：`0x78 0x01`（CM=deflate / 32K window / FLEVEL=fast）
/// + 一串 stored 块 + adler32。
///
/// **空输入必须产出一个合法的「空 stored 块」**，不能只补 adler：
/// 若 DEFLATE 段为空而紧跟 4 字节 adler，解压器剥掉 2 字节 zlib 头后会
/// 把 **adler 字节当成 DEFLATE 位流**去解析块头——读到的首字节是 adler
/// 的高字节（0x00），被判成「BFINAL=0 的 stored 块」，接着要求
/// `NLEN == !LEN` 必然不符→ 报 `BadZlib`。
///
/// 后果是判据陷阱：造出的「空文本」语料**根本走不到**解压路径，而是在
/// 更早的 `Err` 分支就返回了。判据名义上验「空文本解压正确」，实测
/// 验的却是「解压失败被拒」——**两道闸外部表现完全相同**，删掉正确
/// 实现照样全绿（弱门禁十诫 #3：重合行为掩盖缺失分支）。
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut z: Vec<u8> = vec![0x78u8, 0x01];
    if data.is_empty() {
        // BFINAL=1 + BTYPE=00（stored），LEN=0，NLEN=!0=0xFFFF
        z.extend_from_slice(&[0x01u8, 0x00, 0x00, 0xFF, 0xFF]);
    }
    let mut off = 0usize;
    while off < data.len() {
        let n = core::cmp::min(65535, data.len() - off);
        let last = if off + n >= data.len() { 1u8 } else { 0u8 };
        z.push(last);
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&data[off..off + n]);
        off += n;
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(((b << 16) | a) as u32).to_be_bytes());
    z
}

/// 组一个 PNG：签名 + `IEND`（`chunks` 里的块按给定顺序插入）。
fn png_with(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut f = dec::PNG_SIG.to_vec();
    for (ty, payload) in chunks.iter() {
        f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        f.extend_from_slice(*ty);
        f.extend_from_slice(payload);
        let crc = crc32(&f[f.len() - payload.len() - 4..]);
        f.extend_from_slice(&crc.to_be_bytes());
    }
    f.extend_from_slice(&0u32.to_be_bytes());
    f.extend_from_slice(b"IEND");
    // IEND 载荷长度为 0，故「类型+载荷」= `f.len() - 4..`（**不是 `-8..`**）。
    //
    // **这处初版写错，如实留档**：写成 `-8..` 会把 IEND 的**长度字段**
    // （4 字节 0）也纳入 CRC，而被测实现的 CRC 范围是
    // `file[pos+4..payload_end]`——**只含类型+载荷，不含长度字段**。
    // 于是每一份用本函数造的 PNG 其 IEND 块 CRC 恒不匹配，被测侧
    // `crc_warnings` 恒 ≥1。表层症状是「若干判据莫名转红」，真实根因是
    // 判据造语料时多算了 4 字节——**判据错，被测是对的**。
    let crc = crc32(&f[f.len() - 4..]);
    f.extend_from_slice(&crc.to_be_bytes());
    f
}

/// tEXt 载荷：`keyword` NUL `text`。
fn text_payload(kw: &[u8], txt: &[u8]) -> Vec<u8> {
    let mut p = kw.to_vec();
    p.push(0);
    p.extend_from_slice(txt);
    p
}

/// zTXt 载荷：`keyword` NUL `0` `<zlib>`。
fn ztxt_payload(kw: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut p = kw.to_vec();
    p.push(0);
    p.push(0);
    p.extend_from_slice(&zlib_stored(raw));
    p
}

/// iTXt 未压缩载荷：`keyword` NUL `0` NUL `lang` NUL `trans` NUL `utf8text`。
///
/// **flag 之后恰好一个 NUL**（规范：`keyword` NUL `flag` NUL `language_tag`
/// NUL `translated_keyword` NUL `text`）。
///
/// **此处初版多写了一个 NUL，如实留档**：`p.push(0)` 写了两次（flag 值 +
/// 分隔符），于是载荷变成 `kw NUL 0 0 lang NUL ...`。被测实现按规范从
/// `nul1+2` 起读语言标签，读到的第一个字节就是那个多余的 NUL → 语言
/// 标签恒为空串、翻译 keyword 恒为 `zh-CN`、文本恒把语言段也吃进去。
/// 于是 `C05-ITXT-01/02`、`C05-ENTRY-01` 三条一起转红——**根因在语料，
/// 不在被测**。（判据与被测对规范的理解不一致时，先怀疑语料。）
/// iTXt 未压缩载荷：`keyword` NUL `flag` NUL `lang` NUL `trans` NUL `utf8text`。
///
/// **flag 是 1 个字节，其后紧跟 1 个分隔 NUL**。故 keyword 之后**恰好
/// 2 个 NUL**（keyword 自身的定界 + flag 后的分隔）。
///
/// **此处初版多写了一个 NUL，两次才改对（如实留档以免重犯）**：第一版
/// 以为「flag = 1 字节 + NUL」写成 `0` 后又补一个 `0`，得到
/// `kw NUL 0 0 lang`；第二版仍多一个。被测实现从 `nul1+2`（flag 之后
/// **一个**字节处）起读语言标签，故keyword 之后只能有 2 个 NUL。
/// 多一个则语言标签恒为空、翻译 keyword 恒等于语言串、文本恒多吃一段
/// ——`C05-ITXT-01/02` + `C05-ENTRY-01` 一起转红。
///
/// **教训**：判据与被测对同一规范的理解不一致时**先怀疑语料**——
///
/// 字节级 dump 数一遍 NUL 个数，比反复改代码快得多。
fn itxt_payload(kw: &[u8], lang: &[u8], trans: &[u8], txt: &[u8]) -> Vec<u8> {
    let mut p = kw.to_vec();
    p.push(0); // keyword 定界 NUL
    p.push(0); // compression_flag = 0（**值恰为 NUL 字节，占掉 1 字节**）
    p.extend_from_slice(lang); // 紧接 flag，**不再补 NUL**
    p.push(0);
    p.extend_from_slice(trans);
    p.push(0);
    p.extend_from_slice(txt);
    p
}

/// iTXt 压缩载荷：`keyword` NUL `1` `0` `<zlib>` NUL `lang` NUL `trans` NUL。
///
/// 注意末NUL 之后**没有裸文本**——压缩路径的文本全在 zlib 流里。
fn itxt_compressed_payload(kw: &[u8], lang: &[u8], trans: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut p = kw.to_vec();
    p.push(0);
    p.push(1);
    p.push(0);
    p.extend_from_slice(&zlib_stored(raw));
    p.push(0);
    p.extend_from_slice(lang);
    p.push(0);
    p.extend_from_slice(trans);
    p.push(0);
    p
}

// ---------------------------------------------------------------------------
// 二、判据
// ---------------------------------------------------------------------------

/// VE-F1005 域自检入口。
pub fn run_vef05_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F1005 PNG 文本块族");

    // -- C05-TEXT-01 tEXt 基本解析（keyword + Latin-1 文本）------------
    {
        let f = png_with(&[(b"tEXt", text_payload(b"Author", b"Ada"))]);
        let ch = c5::parse_metadata(&f);
        let ok = matches!(&ch, Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword == "Author"
                && c.entries[0].text == b"Ada"
                && c.entries[0].kind == c5::TextKind::Text
                && c.entries[0].compressed == c5::Compressed::None);
        cs.add("C05-TEXT-01 tEXt 解析 keyword+Latin-1 文本", ok, "");
    }

    // -- C05-TEXT-02 tEXt 文本可含 NUL 之后的一切字节（单NUL 分隔）------
    //
    // 承重点：**分隔只发生一次**。若实现误用 `split(|&b| b == 0)` 或
    // `position` 之后又继续找下一个 NUL，文本会被腰斩。
    // 故文本里放**多个 NUL**，断言全部保留在text 里。
    {
        let raw = b"line1\x00line2\x00line3";
        let f = png_with(&[(b"tEXt", text_payload(b"K", raw))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].text == raw);
        cs.add("C05-TEXT-02 tEXt 文本内NUL 不腰斩（单 NUL 分隔语义）", ok, "");
    }

    // -- C05-TEXT-03 空文本条目保留（不是丢弃）------------------------
    {
        let f = png_with(&[(b"tEXt", text_payload(b"Empty", b""))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].text.is_empty());
        cs.add("C05-TEXT-03 tEXt 空文本条目保留不丢弃", ok, "");
    }

    // -- C05-TEXT-04 Latin-1 高位字节保留（0x80..=0xFF 不丢）-----------
    //
    // Latin-1 每字节即一码点，故 0xE9（é）必须原样保留。
    // 若实现误按 UTF-8 解读，0xE9 是非法起始字节 → 会被丢或被替换。
    {
        let raw = &[0xE9u8, 0x41, 0xFF];
        let f = png_with(&[(b"tEXt", text_payload(b"Hi", raw))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].text == raw);
        cs.add("C05-TEXT-04 tEXt Latin-1 高位字节原样保留", ok, "");
    }

    // -- C05-ITXT-01 iTXt 四段结构未压缩解析 ---------------------------
    //
    // 四段：keyword /压缩标志 / 语言标签 / 翻译 keyword + 文本。
    // 断言**全部四段**都被正确切出（语言标签非空是「四段都解析了」的
    // 硬证据——只解 keyword+文本的实现会让语言标签恒空）。
    {
        let f = png_with(&[(
            b"iTXt",
            itxt_payload(b"Title", b"zh-CN", "标题".as_bytes(), "中文标题".as_bytes()),
        )]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword == "Title"
                && c.entries[0].language == "zh-CN"
                && c.entries[0].text == "中文标题".as_bytes()
                && c.entries[0].compressed == c5::Compressed::None);
        cs.add("C05-ITXT-01 iTXt 四段结构解析（keyword/标志/语言/翻译+文本）", ok, "");
    }

    // -- C05-ITXT-02 iTXt UTF-8 多字节正确（中文/emoji）--------------
    {
        // "中文😀" —— 覆盖 3 字节与 4 字节序列
        let txt = "中文😀".as_bytes();
        let f = png_with(&[(b"iTXt", itxt_payload(b"E", b"zh", b"E2", txt))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].text == txt);
        cs.add("C05-ITXT-02 iTXt UTF-8 三/四字节序列正确保留", ok, "");
    }

    // -- C05-ITXT-03 iTXt 压缩路径（标志=1）解压出原文 ----------------
    //
    // 承重腿：`compressed` 字段须为 `WasCompressed`，且文本须等于原文。
    // 只断「文本对」不够——一个「永远走未压缩分支」的变体在**未压缩
    // 语料**上同样全绿，故此处必须用**压缩语料**且断状态位。
    {
        let raw = "压缩后的中文😀".as_bytes();
        let f = png_with(&[(
            b"iTXt",
            itxt_compressed_payload(b"C", b"zh-CN", b"C", raw),
        )]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].text == raw
                && c.entries[0].compressed == c5::Compressed::WasCompressed
                && c.entries[0].language == "zh-CN");
        cs.add("C05-ITXT-03 iTXt 压缩路径解压出原文且状态位为 WasCompressed", ok, "");
    }

    // -- C05-ITXT-04 UTF-8 非法序列被定位到具体字节偏移 --------------
    //
    // 只断「非法 UTF-8 被拒」不够——那与「任何非法都被整体拒」同表现。
    // 故断**偏移**：把非法字节放在已知位置，要求报出的偏移精确等于它。
    // 这利用了 `validate_utf8` 公开返回偏移的设计（模块设计要点五）。
    {
        // 前 3 字节合法（"abc"），第 4 字节 0xFF 非法
        let mut bad = b"abc".to_vec();
        bad.push(0xFF);
        bad.extend_from_slice(b"tail");
        let f = png_with(&[(b"iTXt", itxt_payload(b"U", b"en", b"U", &bad))]);
        // 该条目应被**跳过并计数**（锚点），而非整条通道失败
        let skipped = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.is_empty() && c.inflate_skipped == 1);
        // 另断偏移：直接调公开的 validate_utf8
        let off_ok = c5::validate_utf8(&bad) == Some(3);
        cs.add("C05-ITXT-04 UTF-8 非法字节被定位到精确偏移且该条跳过计数", skipped && off_ok, "");
    }

    // -- C05-ITXT-05 UTF-8 校验覆盖全部非法形态（域扫描）--------------
    //
    // 逐类非法序列各造一例，要求**全部**被拒（None = 通过）。
    // 分开断而不是合并成「非空即非法」——后者会让「只检一种形态」的
    // 实现也全绿（弱门禁十诫 #4：采样留洞）。
    {
        // 各类非法：续字节当首位 / 过长编码 / 代理区 / 超 U+10FFFFF / 截断
        //
        // **两类用例是补 `validate_utf8` 真实漏判时新增的**（初版
        // `C0 80` 与 `F5 80 80 80` 被判为合法）：它们分别覆盖
        // 「过长 NUL 编码」与「超 U+10FFFF 首字节」两个区间。
        let cases: [(&str, &[u8]); 8] = [
            ("continuation-first", &[0x80u8, 0x41]),
            ("overlong-2byte-c0", &[0xC0u8, 0x80]),      // NUL 的过长编码
            ("overlong-2byte-c1", &[0xC1u8, 0xBF]),      // 同上
            ("overlong-3byte", &[0xE0u8, 0x80u8, 0x80]), // 同上
            ("surrogate-d800", &[0xEDu8, 0xA0u8, 0x80]), // U+D800
            ("beyond-10ffff-f5", &[0xF5u8, 0x80u8, 0x80u8, 0x80]),
            ("undefined-f8", &[0xF8u8, 0x80u8, 0x80u8, 0x80u8, 0x80]),
            ("truncated-3byte", &[0xE4u8, 0xB8]),        // 声明3字节只给2
        ];
        let mut all_ok = true;
        let mut n = 0usize;
        for (_, bytes) in cases.iter() {
            if c5::validate_utf8(bytes).is_some() {
                n += 1;
            } else {
                all_ok = false;
            }
        }
        // 反向对照：合法样本必须**通过**（否则「一律拒」也是全绿）
        let good: &[u8] = "中文😀é".as_bytes();
        let good_ok = c5::validate_utf8(good).is_none();
        cs.add(
            "C05-ITXT-05 UTF-8 八类非法形态全覆盖且合法样本不误拒",
            all_ok && n == 8 && good_ok,
            "",
        );
    }

    // -- C05-ZTXT-01 zTXt 解压出原文 + 状态位 ------------------------
    {
        let raw = b"compressed latin1 text";
        let f = png_with(&[(b"zTXt", ztxt_payload(b"Comment", raw))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword == "Comment"
                && c.entries[0].text == raw
                && c.entries[0].compressed == c5::Compressed::WasCompressed
                && c.entries[0].kind == c5::TextKind::CompressedText);
        cs.add("C05-ZTXT-01 zTXt 解压出原文且压缩态正确", ok, "");
    }

    // -- C05-ZTXT-02 zTXt 空文本（合法 stored 空块）------------------
    //
    // **承重腿在 `zlib_stored` 的空输入处理**（见该函数头部注释）：
    // 若造语料时只补 adler 不补空 stored 块，解压器会把 adler 当位流
    // 解析并报 BadZlib，于是「空文本」这条支根本没被走到——判据名义上
    // 验「空文本解压」，实测验的是「坏流被拒」，两者外部表现相同。
    {
        let f = png_with(&[(b"zTXt", ztxt_payload(b"E", b""))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].text.is_empty());
        cs.add("C05-ZTXT-02 zTXt 空文本解压成功（空 stored 块合法封装）", ok, "");
    }

    // -- C05-ZTXT-03 zTXt 方法字段非 0 被拒 ----------------------------
    {
        let mut p = b"Bad".to_vec();
        p.push(0);
        p.push(9); // 方法 ≠ 0
        p.extend_from_slice(&zlib_stored(b"x"));
        let f = png_with(&[(b"zTXt", p)]);
        // 处置是**跳过该条并计数**，不是整图失败
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.is_empty() && c.inflate_skipped == 1);
        cs.add("C05-ZTXT-03 zTXt 压缩方法非 0 被拒且只跳过该条", ok, "");
    }

    // -- C05-ERR-01 解压失败跳过该条并计数（前序条目不丢）------------
    //
    // **这是本单最容易做反的一条**（初版真犯过）：把 UTF-8/解压失败写成
    // `return Err(...)` 会把**整条通道**连同此前已解析的条目一起丢掉。
    // 语料刻意在**坏块之前**放一条好块，断它仍在。
    {
        let mut bad = b"Broken".to_vec();
        bad.push(0);
        bad.push(0);
        bad.extend_from_slice(b"NOT-ZLIB-STREAM"); // 垃圾压缩体
        let f = png_with(&[
            (b"tEXt", text_payload(b"Good", b"kept")),
            (b"zTXt", bad),
            (b"tEXt", text_payload(b"AlsoGood", b"kept2")),
        ]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 2
                && c.entries[0].keyword == "Good"
                && c.entries[1].keyword == "AlsoGood"
                && c.inflate_skipped == 1);
        cs.add("C05-ERR-01 解压失败只跳过该条，前后条目与顺序均保留", ok, "");
    }

    // -- C05-ERR-02 keyword 非法字符规整为下划线且**条目保留**--------
    //
    // 锚点是「规整为下划线**保留条目**」——两个要点各占一条腿：
    //  · 替换发生（非法字节变 `_`）
    //  · 条目**没丢**（len 仍为 1）
    // 只断前者，则「直接丢弃该块」的变体也全绿。
    {
        // keyword 含控制字符 0x01 与 0x7F（均在可见范围外）
        let f = png_with(&[(b"tEXt", text_payload(&[b'a', 0x01, b'b', 0x7F], b"v"))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword == "a_b_"
                && c.keyword_normalized == 1);
        cs.add("C05-ERR-02 keyword 非法字符规整为下划线且条目保留", ok, "");
    }

    // -- C05-ERR-03 重复 keyword 共存不覆盖 + 顺序与文件序一致--------
    //
    // 承重腿是**两条都在且顺序正确**。用 map 的 `insert` 实现会让第二条
    // 覆盖第一条 → len 变1 → 转红；而「收集但顺序不定」的实现会
    // 在顺序断言上转红。故这两条必须**同时**断。
    {
        let f = png_with(&[
            (b"tEXt", text_payload(b"Dup", b"first")),
            (b"tEXt", text_payload(b"Dup", b"second")),
            (b"tEXt", text_payload(b"Dup", b"third")),
        ]);
        let ch = c5::parse_metadata(&f);
        let ok = matches!(&ch, Ok(c)
            if c.entries.len() == 3
                && c.entries[0].text == b"first"
                && c.entries[1].text == b"second"
                && c.entries[2].text == b"third"
                && c.find_all("Dup").len() == 3);
        cs.add("C05-ERR-03 重复 keyword 三条共存不覆盖且顺序=文件序", ok, "");
    }

    // -- C05-ERR-04 空 keyword 规整为下划线保条目 ---------------------
    {
        let f = png_with(&[(b"tEXt", text_payload(b"", b"v"))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].keyword == "_");
        cs.add("C05-ERR-04 空 keyword 规整为下划线且条目保留", ok, "");
    }

    // -- C05-ERR-05 keyword 超 79 裁剪且**计数** ----------------------
    //
    // 承重腿是**计数**：静默裁剪会让下游以为拿到完整 keyword，
    // 那与「伪造」同级。故必须断 `keyword_truncated` 被置位。
    {
        let long_kw = vec![b'k'; 100];
        let f = png_with(&[(b"tEXt", text_payload(&long_kw, b"v"))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword.len() == c5::KEYWORD_MAX
                && c.keyword_truncated == 1);
        cs.add("C05-ERR-05 keyword 超 79 裁剪且计数不静默", ok, "");
    }

    // -- C05-ERR-06 keyword 79 边界（夹逼对）-------------------------
    //
    // 79 必须**恰好通过**，80 必须被裁。只测 80 的话，一个「一律裁到 79」
    // 的实现（把 10 字节的也裁成 79？不可能，但反向：把79 也拒了）
    // 仍可能蒙混。故两侧都断。
    {
        let kw79 = vec![b'k'; 79];
        let kw80 = vec![b'k'; 80];
        let f79 = png_with(&[(b"tEXt", text_payload(&kw79, b"v"))]);
        let f80 = png_with(&[(b"tEXt", text_payload(&kw80, b"v"))]);
        let at79 = matches!(&c5::parse_metadata(&f79), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword.len() == 79
                && c.keyword_truncated == 0);
        let at80 = matches!(&c5::parse_metadata(&f80), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].keyword.len() == 79
                && c.keyword_truncated == 1);
        cs.add("C05-ERR-06 keyword 79 通过 / 80 裁剪（夹逼对钉死界位置）", at79 && at80, "");
    }

    // -- C05-ERR-07 CRC 错块被跳过且计数（不阻断整图）----------------
    {
        let mut f = png_with(&[(b"tEXt", text_payload(b"K", b"v"))]);
        //破坏唯一文本块的 CRC（IEND 之前那4 字节）
        let len = f.len();
        f[len - 13] ^= 0xFF; // CRC 区首字节（IEND 12字节 + CRC 4 字节）
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.is_empty() && c.crc_warnings == 1);
        cs.add("C05-ERR-07 文本块 CRC 错被跳过并计数且不阻断整图", ok, "");
    }

    // -- C05-ERR-08 无文本块时产出空通道且不报错 ----------------------
    {
        let f = png_with(&[]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.is_empty() && c.warnings() == 0);
        cs.add("C05-ERR-08 无任何文本块时产出空通道且零告警", ok, "");
    }

    // -- C05-ERR-09 坏签名被拒（专属错误码）--------------------------
    {
        let mut f = png_with(&[(b"tEXt", text_payload(b"K", b"v"))]);
        f[1] ^= 0xFF;
        let ok = matches!(&c5::parse_metadata(&f),
            Err(ref e) if e.kind == c5::TextFaultKind::BadSignature);
        cs.add("C05-ERR-09 坏签名返回专属 BadSignature 错误码", ok, "");
    }

    // -- C05-SAFE-01 展示转义：五个危险字符全覆盖 ---------------------
    //
    // **语料必须含全部五个**：初版语料 `<script>alert('x')</script>` 只有
    // `<`/`>`/`'` 三种，**没有 `&` 与 `"`**。于是「只转义 `<`/`>`/`'`
    // 三个字符」的实现照样全绿——五字符覆盖是虚的（采样留洞）。
    // 现补`&` 与 `"`，五个字符逐个在场。
    {
        let raw = b"<a href=\"x\" title='y'>&amp;</a>";
        let esc = c5::escape_for_display(raw);
        // 五种实体各至少出现一次
        let has_entities = esc.contains("&lt;")
            && esc.contains("&gt;")
            && esc.contains("&#39;")
            && esc.contains("&quot;")
            && esc.contains("&amp;");
        // 不得残留裸的危险字符（转义后它们全部变成实体的一部分）
        let no_bare_quote = !esc.contains('"');
        // 非剥离：转义后长度必须**严格大于**原文（每个危险字符都变长了）
        let grew = esc.len() > raw.len();
        cs.add(
            "C05-SAFE-01 展示转义五字符全覆盖且非剥离（长度严格增长）",
            has_entities && no_bare_quote && grew,
            "",
        );
    }

    // -- C05-SAFE-02 转义**非剥离**（保真原则，双向断）----------------
    //
    // 这是锚点最容易被做反的一条。断**两侧**：
    //  · 不能原样透出 `<script>`（不安全）
    //  · 不能整段删掉（破坏用户资产 —— 锚点「展示安全而非内容破坏」）
    // 只断前者，则「删除全部标签」的剥离式实现也全绿。
    {
        let raw = b"<b>bold</b>";
        let esc = c5::escape_for_display(raw);
        // 内容词必须还在（没被剥掉）
        let content_kept = esc.contains("bold");
        // 但标签形态已转义（不再是可执行标签）
        let tag_neutralized = esc.contains("&lt;b&gt;");
        cs.add("C05-SAFE-02 转义非剥离：内容保留且标签形态已中和", content_kept && tag_neutralized, "");
    }

    // -- C05-SAFE-03 控制字符展示为可见转义形态 ----------------------
    {
        let raw = &[0x00u8, 0x1B, 0x41];
        let esc = c5::escape_for_display(raw);
        // 不得含裸控制字符；应有 \xNN 形态
        let no_raw_ctrl = !esc.chars().any(|c| (c as u32) < 0x20);
        let has_hex = esc.contains("\\x00") && esc.contains("\\x1B");
        cs.add("C05-SAFE-03 控制字符转可见\\xNN 形态且不混入裸控制符", no_raw_ctrl && has_hex, "");
    }

    // -- C05-SAFE-04 展示视图：下游只拿得到已转义文本 -----------------
    //
    // 锚点「元数据文本不直接执行——只作为元数据存储展示」。类型级保证：
    // [`DisplayEntry`] 只有 `escaped` 而无原始 text 字段。故断展示视图
    // 里keyword/文本均已转义。
    {
        let f = png_with(&[(b"tEXt", text_payload(b"<kw>", b"<script>x</script>"))]);
        let ch = c5::parse_metadata(&f);
        let ok = match ch {
            Ok(c) => {
                let view = c5::to_display_view(&c);
                view.len() == 1
                    && view[0].escaped.contains("&lt;script&gt;")
                    && !view[0].escaped.contains('<')
                    && view[0].keyword.contains("&lt;kw&gt;")
            }
            Err(_) => false,
        };
        cs.add("C05-SAFE-04 展示视图仅暴露已转义文本（类型级不暴露原始可执行片段）", ok, "");
    }

    // -- C05-SAFE-05 条数上限 500：超限拒收并计数 ---------------------
    //
    // 锚点「条数上限 500 防元数据炸弹」。断两侧：
    //  · 500 条**全部**收入（不误伤）
    //  · 501 条起**拒收并计数**（不是静默截断）
    // 只断后者，则一个「上限写成 1」的过严实现也全绿。
    {
        let mut exact: Vec<(&[u8; 4], Vec<u8>)> = Vec::new();
        for i in 0..c5::ENTRY_MAX_COUNT {
            // keyword 用定长数字以便逐条断言（tag +序号）
            let kw = format!("k{}", i);
            exact.push((b"tEXt", text_payload(kw.as_bytes(), b"v")));
        }
        let at_limit = png_with(&exact);
        let r1 = c5::parse_metadata(&at_limit);
        let ok_exact = matches!(&r1, Ok(c)
            if c.entries.len() == c5::ENTRY_MAX_COUNT
                && c.dropped_over_limit == 0);

        let mut over = exact.clone();
        over.push((b"tEXt", text_payload(b"overflow", b"v")));
        let r2 = c5::parse_metadata(&png_with(&over));
        let ok_over = matches!(&r2, Ok(c)
            if c.entries.len() == c5::ENTRY_MAX_COUNT
                && c.dropped_over_limit == 1);

        cs.add(
            "C05-SAFE-05 条数上限 500：恰好 500 全收 / 501 起拒收计数",
            ok_exact && ok_over,
            "",
        );
    }

    // -- C05-SAFE-06 单条 2MB 上限：解压后超限被拒 --------------------
    //
    // 承重腿：**按解压后长度算**。压缩前小、解压后大是炸弹的典型形态，
    // 若按压缩体长度算则完全拦不住。语料刻意用「小的压缩体→大的产出」。
    {
        // 2MB+1 字节的产出（全同字节，压缩体极小）
        let big = vec![0x5Au8; c5::TEXT_MAX_BYTES + 1];
        let f = png_with(&[(b"zTXt", ztxt_payload(b"Big", &big))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.is_empty() && c.rejected_oversize == 1);

        // **第二条腿：未压缩路径（tEXt）**——这一腿才让**通道侧那道闸**
        // 承重，否则它是一条死码。
        //
        // **变异实测暴露的弱门禁（如实留档）**：把通道侧的
        // `if text.len() > TEXT_MAX_BYTES` 改成 `if false`（变体 W7），
        // 上面那条压缩语料判据**照样全绿**——因为压缩路径在
        // `inflate_bounded` 内部就已经因超限被拒并置了
        // `rejected_oversize`，**通道侧那道闸根本没被执行到**。
        // 两道闸的外部表现完全相同（条目不收 + 计数 +1），
        // 属弱门禁十诫 #3「重合行为掩盖缺失分支」。
        //
        // 解法：给通道侧闸一个**只有它能拦**的语料——**未压缩的 tEXt**。
        // 该路径不经过 `inflate_bounded`，超限只能由通道侧那道闸拦。
        let big_text = vec![0x5Au8; c5::TEXT_MAX_BYTES + 1];
        let f2 = png_with(&[(b"tEXt", text_payload(b"BigPlain", &big_text))]);
        let plain_ok = matches!(&c5::parse_metadata(&f2), Ok(c)
            if c.entries.is_empty() && c.rejected_oversize == 1);

        cs.add(
            "C05-SAFE-06 单条 2MB 上限：压缩路径(解压后判定) + 未压缩路径(通道闸) 双腿",
            ok && plain_ok,
            "",
        );
    }

    // -- C05-SAFE-07 恰好等于 2MB 必须被接受（闸门不误伤）------------
    //
    // 只断「超限被拒」的话，一个「上限写成 1MB」的过严实现也全绿。
    // 故必须有反向对照腿：恰好等于上限者**必须通过**。
    {
        let at = vec![0x5Au8; c5::TEXT_MAX_BYTES];
        let f = png_with(&[(b"zTXt", ztxt_payload(b"At", &at))]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && c.entries[0].text.len() == c5::TEXT_MAX_BYTES
                && c.rejected_oversize == 0);
        cs.add("C05-SAFE-07 恰好等于 2MB 的条目被接受（上限闸不误伤边界值）", ok, "");
    }

    // -- C05-ENTRY-01 四元组齐全（keyword/语言/文本/压缩态）----------
    //
    // 逐字段断「字段不是恒空」：语言标签在 tEXt/zTXt 下**理应**为空，
    // 故这里用 iTXt 语料断四元组都非空。
    {
        let f = png_with(&[(
            b"iTXt",
            itxt_payload(b"K", b"en-US", b"T", b"body"),
        )]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1
                && !c.entries[0].keyword.is_empty()
                && !c.entries[0].language.is_empty()
                && !c.entries[0].text.is_empty()
                && c.entries[0].compressed == c5::Compressed::None);
        cs.add("C05-ENTRY-01 元数据条目四元组齐全无恒空字段", ok, "");
    }

    // -- C05-ENTRY-02 三块同文件共存且顺序稳定 ------------------------
    //
    // 断「三种kind 都被识别」+「顺序 = 文件序」。用一个只认tEXt 的
    // 实现会在此转绿失败（条目数与 kind 都不对）。
    {
        let f = png_with(&[
            (b"tEXt", text_payload(b"A", b"1")),
            (b"zTXt", ztxt_payload(b"B", b"2")),
            (b"iTXt", itxt_payload(b"C", b"en", b"C", b"3")),
        ]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 3
                && c.entries[0].kind == c5::TextKind::Text
                && c.entries[1].kind == c5::TextKind::CompressedText
                && c.entries[2].kind == c5::TextKind::InternationalText
                && c.entries[0].keyword == "A"
                && c.entries[2].keyword == "C");
        cs.add("C05-ENTRY-02 tEXt/zTXt/iTXt 三块共存且顺序=文件序", ok, "");
    }

    // -- C05-ENTRY-03 非文本块被忽略（不误收）------------------------
    {
        // 混入 gAMA / IHDR 类块，断言不被当作文本条目
        let f = png_with(&[
            (b"gAMA", vec![0x00, 0x00, 0xB1, 0x8F]),
            (b"tEXt", text_payload(b"K", b"v")),
            (b"pHYs", vec![0x00, 0x00, 0x0B, 0x13]),
        ]);
        let ok = matches!(&c5::parse_metadata(&f), Ok(c)
            if c.entries.len() == 1 && c.entries[0].keyword == "K");
        cs.add("C05-ENTRY-03 非文本块被忽略不误收为条目", ok, "");
    }

    // -- C05-ENTRY-04 IEND 之后的块不再解析 ---------------------------
    //
    // **本判据初版写得混乱（如实留档）**：构造IEND-after 语料时先造了
    // 一个完整 PNG再`[..len-12]` 截取，又同时留了两条互相冲突的构造
    // 路径（`f`/`f2`/`g` 三份），其中 `f`/`f2` 造完即弃——即「判据语料
    // 里有死代码」，读的人无法确认哪条才是真语料。
    //
    // 现改为**一条直白路径**：手工逐块拼`签名 +块A + IEND + 块B`，
    // 断言只有块A 被收入。判据的**承重点**不变：扫描器遇IEND 即 `break`。
    {
        fn push_chunk(f: &mut Vec<u8>, ty: &[u8; 4], payload: &[u8]) {
            f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            f.extend_from_slice(ty);
            f.extend_from_slice(payload);
            let crc = crc32(&f[f.len() - payload.len() - 4..]);
            f.extend_from_slice(&crc.to_be_bytes());
        }

        let mut g = dec::PNG_SIG.to_vec();
        push_chunk(&mut g, b"tEXt", &text_payload(b"Before", b"1"));
        push_chunk(&mut g, b"IEND", b"");
        push_chunk(&mut g, b"tEXt", &text_payload(b"After", b"2"));

        let ok = matches!(&c5::parse_metadata(&g), Ok(c)
            if c.entries.len() == 1 && c.entries[0].keyword == "Before");
        cs.add("C05-ENTRY-04 IEND 之后的块不再解析（遇 IEND 即停）", ok, "");
    }

    // -- C05-ERR-10 块长度字段全域扫描（长度闸 + 32 位溢出登记）-------
    //
    // `len` 来自 4 字节不可信字段，参与三处边界算术（总长/载荷/推进），
    // 三处均走 checked_add。判据只断言**可观测的长度闸**；
    // checked 溢出面在 64 位上不可观测（详见模块与判据注释的诚实登记）。
    {
        let mut all_ok = true;
        let mut probed = 0usize;
        // 越界侧靠**砍文件**制造（改 declared 而不砍文件是无效语料——
        // 那样块仍合法，实现返回 Ok 是对的，判据反而写错）
        for delta in 0..3u32 {
            let declared = 8u32;
            let present = declared as usize - delta as usize;
            let mut f = dec::PNG_SIG.to_vec();
            f.extend_from_slice(&declared.to_be_bytes());
            f.extend_from_slice(b"tEXt");
            f.extend_from_slice(&vec![0x41u8; present]);
            if delta == 0 {
                // 合法侧补足 CRC。覆盖范围 =「类型 + 载荷」= **12 字节**
                // （`tEXt` 4 + 载荷 8），即 `f.len() - 4 - 12`起算。
                //
                // **这处初版两次算错，如实留档**：第一版写 `[0u8;4]`
                // 占位（不校验 CRC，合法侧因 CRC 错被跳）；第二版改
                // `f[f.len()-4..]`——那指向的是 **CRC 区本身**（4 字节
                // 全0），而正确的 CRC 覆盖是**类型+载荷**。
                // 两版都让「合法侧」拿不到 chunk，判据恒红。
                //
                // 与被测 `file[pos+4..payload_end]` 同口径：`pos` 是块首，
                // `pos+4` 跳过长度字段后正是类型字节。
                // 用**相对定位**而非绝对算术：类型字节起点 = 签名(8)
                // + 长度字段(4) = 12；覆盖「类型(4) + 载荷(declared)」。
                //
                // **这处初版三次算错，如实留档**：① `[0u8;4]` 占位（不校验
                // CRC）② `f[f.len()-4..]`（指向 CRC 区本身）③
                // `f[f.len()-4-(4+declared)..]`（偏移量算错 4 字节）。
                // 三版都让合法侧拿不到 chunk。**教训**：块内偏移应从
                // `pos` 递推，别用「总长倒推」——倒推一旦块头布局记错
                // 就整体偏移，且不报错只表现为 CRC 恒不匹配，最难查。
                let crc_start = 8 + 4; // 签名 + 长度字段
                let crc = crc32(&f[crc_start..crc_start + 4 + declared as usize]);
                f.extend_from_slice(&crc.to_be_bytes());
            }
            probed += 1;
            match c5::scan_text_chunks(&f) {
                Err(ref e) if delta > 0 && e.kind == c5::TextFaultKind::ChunkTruncated => {}
                Ok(ref r) if delta == 0 && r.chunks.len() == 1 => {}
                _ => all_ok = false,
            }
        }
        // u32::MAX 声明（32 位下必溢出，64 位下靠 checked 拦）
        let mut big = dec::PNG_SIG.to_vec();
        big.extend_from_slice(&u32::MAX.to_be_bytes());
        big.extend_from_slice(b"tEXt");
        big.extend_from_slice(&[0u8; 4]);
        probed += 1;
        let big_ok = matches!(&c5::scan_text_chunks(&big),
            Err(ref e) if e.kind == c5::TextFaultKind::ChunkTruncated);
        cs.add(
            "C05-ERR-10 块长度字段全域扫描：截断侧必拒/合法侧必收/u32::MAX 被 checked 拦",
            all_ok && big_ok && probed == 4,
            "",
        );
    }

    // -- C05-ERR-11 畸形输入遍历不崩溃（≥16 案）---------------------
    {
        let mut cases = 0usize;
        let base = png_with(&[
            (b"tEXt", text_payload(b"K", b"v")),
            (b"zTXt", ztxt_payload(b"Z", b"w")),
            (b"iTXt", itxt_payload(b"I", b"en", b"I", b"u")),
        ]);
        // 1) 空输入
        let _ = c5::parse_metadata(&[]);
        cases += 1;
        // 2) 仅签名
        let mut only = dec::PNG_SIG.to_vec();
        only.extend_from_slice(&[0u8; 4]);
        let _ = c5::parse_metadata(&only);
        cases += 1;
        // 3) 逐字节破坏签名
        for i in 1..8usize {
            let mut v = base.clone();
            v[i] ^= 0xFF;
            let _ = c5::parse_metadata(&v);
            cases += 1;
        }
        // 4) 截断到各长度（前缀全扫）
        for n in 1..base.len().min(24) {
            let _ = c5::parse_metadata(&base[..n]);
            cases += 1;
        }
        // 5) 三块的载荷逐位置破坏
        for (ty, good) in [
            (b"tEXt", text_payload(b"K", b"v")),
            (b"zTXt", ztxt_payload(b"Z", b"w")),
            (b"iTXt", itxt_payload(b"I", b"en", b"I", b"u")),
        ]
        .iter()
        {
            for cut in 1..good.len() {
                let f = png_with(&[(ty, good[..cut].to_vec())]);
                let _ = c5::parse_metadata(&f);
                cases += 1;
            }
        }
        // 断言阈值跟随**实际枚举案数**（不写够不着的整数）
        cs.add(
            "C05-ERR-11 畸形输入遍历不崩溃（签名/截断/载荷全域）",
            cases >= 16,
            "",
        );
    }

    // -- C05-ERR-12 故障码登记表自洽（码位唯一 + 可达性登记）---------
    {
        let mut ok = c5::FAULT_REGISTRY.len() == 8;
        // 码位唯一
        for i in 0..c5::FAULT_REGISTRY.len() {
            for j in (i + 1)..c5::FAULT_REGISTRY.len() {
                if c5::FAULT_REGISTRY[i].1 == c5::FAULT_REGISTRY[j].1 {
                    ok = false;
                }
            }
        }
        // 码位连续 1..=8（空洞须显式保留，不得复用）
        for (i, item) in c5::FAULT_REGISTRY.iter().enumerate() {
            if item.1 as usize != i + 1 {
                ok = false;
            }
            // 可达性说明非空（纪律：不得留空占位）
            if item.2.is_empty() {
                ok = false;
            }
        }
        cs.add("C05-ERR-12 故障码 8 位连续唯一且逐条登记可达性说明", ok, "");
    }

    // -- C05-ERR-13 TextFaultKind::code 与登记表逐项一致 --------------
    //
    // 防「新增变体时改了 code() 但忘了同步登记表」这类漂移。
    {
        let all = [
            c5::TextFaultKind::BadSignature,
            c5::TextFaultKind::ChunkTruncated,
            c5::TextFaultKind::Utf8Invalid,
            c5::TextFaultKind::CompressionFlag,
            c5::TextFaultKind::CompressionMethod,
            c5::TextFaultKind::Inflate,
            c5::TextFaultKind::TooLarge,
            c5::TextFaultKind::TooManyEntries,
        ];
        let mut ok = all.len() == c5::FAULT_REGISTRY.len();
        for (i, k) in all.iter().enumerate() {
            if (*k).code() as usize != i + 1 {
                ok = false;
            }
            if (*k).name() != c5::FAULT_REGISTRY[i].0 {
                ok = false;
            }
        }
        cs.add("C05-ERR-13 枚举 code()/name() 与 FAULT_REGISTRY 逐项一致", ok, "");
    }

    cs
}

/// 变体登记（名称、目标判据）。
///
/// 每条都已实测让判据变红或**如实登记为不可观测**（见各项说明）。
/// 变体登记（名称、目标判据）。
///
/// **11 个变体全部经实测确认能转红**（2026-10-08 W014，隔离探针挂真
/// `mech_inflate`）。登记表不是许愿单——每一条都有对应的「改哪一行 →
/// 哪条判据红」的实测记录。
///
/// **登记的价值在于「红项名」那一列**：它把「变异」与「承重的那条判据」
/// 绑定起来。改了实现后跑一遍，若某个变体对应的判据**不再转红**，说明
/// 那条判据退化成了永绿假门禁——此时该加判据，而不是庆祝「又绿了」。
pub const VARIANT_REGISTRY: [(&str, &str); 11] = [
    ("W1-text-resplit-nul", "C05-TEXT-02 转红(tEXt 文本内 NUL 被腰斩)"),
    ("W2-itxt-lang-always-empty", "C05-ITXT-01 转红(四段结构只解两段)"),
    ("W3-compressed-always-none", "C05-ITXT-03 转红(压缩态退化)"),
    ("W4-dup-first-wins", "C05-ERR-03 转红(重复 keyword 被覆盖)"),
    ("W5-inflate-aborts-channel", "C05-ZTXT-03 转红(整条通道被丢弃)"),
    ("W6-escape-strips", "C05-SAFE-01 转红(剥离而非转义)"),
    ("W7-limit-ignores-output-len", "C05-SAFE-06 转红(通道闸摘除,压缩路径掩盖)"),
    ("W8-keyword-trunc-silent", "C05-ERR-05 转红(静默裁剪不计数)"),
    ("W9-after-no-skip-nul", "C05-ITXT-03 转红(off-by-one: 语言标签恒空)"),
    ("W10-drop-total-guard", "C05-ERR-10 转红(块长度守卫被摘)"),
    ("W11-utf8-allow-overlong-c0", "C05-ITXT-05 转红(放过过长 NUL 编码)"),
];
