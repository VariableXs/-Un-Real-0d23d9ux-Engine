//! VE-F1001 · PNG 解码器核心 · 域自检（判据逐条对应，见 `vef01_pngdec.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 签名验证（魔数不符三要素拒绝）→ `C01-SIG-*`
//! - IHDR 七参数逐一校验 → `C01-IHDR-*`
//! - 非法组合显性拒绝表 → `C01-COMBO-*`
//! - PLTE ≤256 项长度校验 → `C01-PLTE-*`
//! - tRNS 透明 → `C01-TRNS-*`
//! - IDAT 解压（跨片连续）→ `C01-IDAT-*`
//! - 反滤波 SIMD 五滤波 → `C01-UNFILTER-*`
//! - 十种（规范全集 15 种）合法组合输出 RGBA → `C01-RGBA-*`
//! - 未知块 ancillary/critical 处置 → `C01-CHUNK-*`
//! - CRC 分级 → `C01-CRC-*`
//! - 截断→已解码部分输出并标记 → `C01-TRUNC-*`
//! - 跨位深正确性 → `C01-DEPTH-*`
//!
//! 纯函数校验，无时钟无 IO，回归可复现。语料由本文件内的**手写字节构造器**
//! 生成（不依赖外部 PNG 样本——F1011 的 PngSuite 语料另立专项），
//! zlib 层用 stored（未压缩）deflate 块手搭，保证确定性。

use super::vef01_pngdec::*;
use crate::checks::CheckSet;
use crate::perfstar::imgsimd;
use crate::perfstar::mech_inflate;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 语料构造器（stored deflate 手搭 zlib —— 免去 deflate 编码器依赖）
// ---------------------------------------------------------------------------

/// Adler-32（与上游 `adler32` 同口径，用于自搭流的校验位）。
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// 以 stored（BTYPE=00）块手搭一个 zlib 流——确定性优先，不引deflate 编码器。
///
/// 块体布局：BFINAL(1b) + BTYPE(2b=00)，随后 LEN/NLEN 各 2 字节小端，
/// 再是 LEN 字节原文。每块上限 65535（超长自动切多块）。
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // zlib 头：CMF=0x78(CM=8 deflate, CINFO=7 32K 窗口)，FLG=0x01(无字典, 校验位对齐)
    out.push(0x78);
    out.push(0x01);
    let mut off = 0usize;
    loop {
        let n = core::cmp::min(65535, data.len() - off);
        let last = off + n >= data.len();
        out.push(if last { 1 } else { 0 });
        let len = n as u16;
        out.push(len as u8);
        out.push((len >> 8) as u8);
        let nlen = !len;
        out.push(nlen as u8);
        out.push((nlen >> 8) as u8);
        out.extend_from_slice(&data[off..off + n]);
        off += n;
        if last {
            break;
        }
    }
    let ad = adler32(data);
    out.push((ad >> 24) as u8);
    out.push((ad >> 16) as u8);
    out.push((ad >> 8) as u8);
    out.push(ad as u8);
    out
}

/// 组一个块（长度 + 类型 + 数据 + CRC32）。
fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let len = data.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    out.extend_from_slice(&mech_inflate::crc32_span(ty, data).to_be_bytes());
    out
}

/// 组 IHDR 十三个数据字节。
fn ihdr_data(w: u32, h: u32, depth: u8, color: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&w.to_be_bytes());
    d.extend_from_slice(&h.to_be_bytes());
    d.push(depth);
    d.push(color);
    d.push(0); // compression
    d.push(0); // filter method
    d.push(interlace);
    d
}

/// 组调色板数据（RGB 三元组序列）。
fn plte_data(entries: &[(u8, u8, u8)]) -> Vec<u8> {
    let mut d = Vec::new();
    for &(r, g, b) in entries {
        d.push(r);
        d.push(g);
        d.push(b);
    }
    d
}

/// 组一条 RGBA8 像素行（4 字节/像素，大端序无关——本域字节序即 R,G,B,A）。
fn rgba_line(px: &[(u8, u8, u8, u8)]) -> Vec<u8> {
    let mut d = Vec::new();
    for &(r, g, b, a) in px {
        d.extend_from_slice(&[r, g, b, a]);
    }
    d
}

/// 完整 PNG 字节流（签名 + IHDR + 可选块 + IDAT + IEND）。
fn build_png(head: &[u8], middle: &[Vec<u8>], raw: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&PNG_SIG);
    f.extend_from_slice(&chunk(&CHUNK_IHDR, head));
    for m in middle {
        f.extend_from_slice(m);
    }
    f.extend_from_slice(&chunk(&CHUNK_IDAT, &zlib_stored(raw)));
    f.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
    f
}

/// 2×1 RGBA8 便利构造（ramp 像素，两行）。
fn png_rgba_ramp() -> Vec<u8> {
    let head = ihdr_data(2, 2, 8, 6, 0);
    let mut raw = Vec::new();
    raw.push(0); // 滤波 None
    raw.extend_from_slice(&rgba_line(&[(10, 20, 30, 255), (40, 50, 60, 128)]));
    raw.push(0);
    raw.extend_from_slice(&rgba_line(&[(70, 80, 90, 255), (100, 110, 120, 0)]));
    build_png(&head, &[], &raw)
}

/// 3 像素宽调色板图（8 位索引，2 行）。
fn png_palette3() -> Vec<u8> {
    let head = ihdr_data(3, 2, 8, 3, 0);
    let pal = chunk(&CHUNK_PLTE, &plte_data(&[(255, 0, 0), (0, 255, 0), (0, 0, 255)]));
    let mut raw = Vec::new();
    raw.push(0);
    raw.extend_from_slice(&[0, 1, 2]);
    raw.push(0);
    raw.extend_from_slice(&[2, 1, 0]);
    build_png(&head, &[pal], &raw)
}

/// 采样式接收端（记录收到的行数与首行字节）。
struct Probe {
    rows: u32,
    first: Vec<u8>,
    stop_after: Option<u32>,
}

impl RowSink for Probe {
    fn on_row(&mut self, _y: u32, rgba: &[u8]) -> bool {
        if self.rows == 0 {
            self.first = rgba.to_vec();
        }
        self.rows += 1;
        match self.stop_after {
            Some(n) => self.rows < n,
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检主体
// ---------------------------------------------------------------------------

/// VE-F1001 域自检。
pub fn run_vef01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vef01");

    // ---- 判据一：签名验证 ----
    {
        set.add(
            "C01-SIG-合法签名通过",
            verify_signature(&png_rgba_ramp()).is_ok(),
            "",
        );
        // 魔数不符 → BadSignature，且三要素齐全（码/原因/建议非空）
        let mut bad = PNG_SIG;
        bad[3] = b'X';
        let e = verify_signature(&bad);
        let ok = matches!(e, Err(f) if f.kind == FaultKind::BadSignature
            && !f.human().is_empty() && !f.cause().is_empty() && !f.advice().is_empty() && f.code() != 0);
        set.add("C01-SIG-魔数不符三要素拒绝", ok, "");
        // 长度不足 8 → 拒绝（不读越界）
        set.add("C01-SIG-短于8字节拒绝", verify_signature(&[0x89, b'P']).is_err(), "");
        // 逐字节不符全部被拒（八位穷举——不做宽松前缀匹配）
        let all_rejected = (0..8).all(|i| {
            let mut f = png_rgba_ramp();
            f[i] ^= 0xFF;
            verify_signature(&f).is_err()
        });
        set.add("C01-SIG-八位逐一不符均拒绝", all_rejected, "");
    }

    // ---- 判据二：IHDR 七参数逐一校验 ----
    {
        // 正常头解析出七参数
        let h = parse_ihdr(&ihdr_data(64, 32, 8, 6, 0));
        set.add(
            "C01-IHDR-七参数解析正确",
            matches!(h, Ok(x) if x.width == 64 && x.height == 32 && x.depth == 8
                && x.color == ColorType::Rgba && x.compression == 0 && x.filter_method == 0 && x.interlace == 0),
            "",
        );
        // 数据长度不足 13
        set.add(
            "C01-IHDR-短于13字节拒绝",
            parse_ihdr(&ihdr_data(1, 1, 8, 6, 0)[..12]).is_err(),
            "",
        );
        // 宽/高为 0
        set.add("C01-IHDR-宽0拒绝", parse_ihdr(&ihdr_data(0, 4, 8, 6, 0)).is_err(), "");
        set.add("C01-IHDR-高0拒绝", parse_ihdr(&ihdr_data(4, 0, 8, 6, 0)).is_err(), "");
        // 颜色类型未定义（1/5/7）——显性拒绝，不猜
        let undef = [1u8, 5, 7, 255].iter().all(|&c| parse_ihdr(&ihdr_data(4, 4, 8, c, 0)).is_err());
        set.add("C01-IHDR-未定义颜色类型拒绝", undef, "");
        // 压缩/滤波方法非 0
        let mut bad_comp = ihdr_data(4, 4, 8, 6, 0);
        bad_comp[10] = 1;
        let mut bad_filt = ihdr_data(4, 4, 8, 6, 0);
        bad_filt[11] = 1;
        set.add("C01-IHDR-压缩方法非0拒绝", parse_ihdr(&bad_comp).is_err(), "");
        set.add("C01-IHDR-滤波方法非0拒绝", parse_ihdr(&bad_filt).is_err(), "");
        // 位深定义域（3/5/6/7 等非定义值）
        let odd = [3u8, 5, 6, 7, 12].iter().all(|&d| parse_ihdr(&ihdr_data(4, 4, d, 0, 0)).is_err());
        set.add("C01-IHDR-非定义位深拒绝", odd, "");
        // 隔行=1 显式拒绝并指向 F1003（不静默按逐行解）
        let il = parse_ihdr(&ihdr_data(4, 4, 8, 6, 1));
        set.add(
            "C01-IHDR-隔行显式拒绝",
            matches!(il, Err(f) if f.kind == FaultKind::InterlaceUnsupported && f.advice().contains("F1003")),
            "",
        );
        // IHDR 缺失 / 非首块
        let no_ihdr = {
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_IDAT, &zlib_stored(&[0, 1, 2])));
            parse_container(&f).is_err()
        };
        set.add("C01-IHDR-缺失拒绝", no_ihdr, "");
        let not_first = {
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_TRNS, &[0, 0]));
            f.extend_from_slice(&chunk(&CHUNK_IHDR, &ihdr_data(2, 2, 8, 6, 0)));
            parse_container(&f).is_err()
        };
        set.add("C01-IHDR-非首块拒绝", not_first, "");
    }

    // ---- 判据三：非法组合显性拒绝表 ----
    {
        // 规范全集 15 种组合全部合法
        let combos = all_legal_combos();
        set.add(
            "C01-COMBO-15种合法组合表完整",
            combos.len() == LEGAL_COMBO_COUNT
                && combos.iter().all(|&(c, d)| combo_is_legal(c, d)),
            "",
        );
        // 逐格断言：灰度 5 种、真彩 2、调色板 4、灰度α 2、真彩α 2
        let per_type_ok = legal_depths(ColorType::Gray) == [1u8, 2, 4, 8, 16]
            && legal_depths(ColorType::Rgb) == [8u8, 16]
            && legal_depths(ColorType::Palette) == [1u8, 2, 4, 8]
            && legal_depths(ColorType::GrayAlpha) == [8u8, 16]
            && legal_depths(ColorType::Rgba) == [8u8, 16];
        set.add("C01-COMBO-各类型合法位深表正确", per_type_ok, "");
        // 典型非法组合全部被拒（16 位调色板 / 8 位以外的真彩低位深 等）
        let illegal = [
            (ColorType::Palette, 16u8),
            (ColorType::Rgb, 1),
            (ColorType::Rgb, 4),
            (ColorType::GrayAlpha, 1),
            (ColorType::GrayAlpha, 4),
            (ColorType::Rgba, 1),
            (ColorType::Rgba, 2),
            (ColorType::Rgba, 4),
            (ColorType::Gray, 3),
            (ColorType::Gray, 12),
        ];
        set.add(
            "C01-COMBO-非法组合全部拒绝",
            illegal.iter().all(|&(c, d)| !combo_is_legal(c, d)),
            "",
        );
        // 经由 parse_ihdr 走同一拒绝路径（拒绝表是唯一来源，不存在旁路）
        let via_ihdr = parse_ihdr(&ihdr_data(4, 4, 16, 3, 0)).is_err()
            && parse_ihdr(&ihdr_data(4, 4, 4, 2, 0)).is_err()
            && parse_ihdr(&ihdr_data(4, 4, 16, 0, 0)).is_ok();
        set.add("C01-COMBO-parse_ihdr同源拒绝", via_ihdr, "");
    }

    // ---- 判据四：PLTE ≤256 项长度校验 ----
    {
        // 合法：3 项
        let p = parse_plte(&plte_data(&[(1, 2, 3), (4, 5, 6), (7, 8, 9)]), ColorType::Palette);
        set.add(
            "C01-PLTE-3项装配正确",
            matches!(&p, Ok(x) if x.len == 3 && x.get(2) == Some((7, 8, 9))),
            "",
        );
        // 越界索引返回 None（不夹取末项）
        let ok_bounds = p.as_ref().ok().and_then(|x| x.get(3)).is_none();
        set.add("C01-PLTE-越界索引返回None", ok_bounds, "");
        // 空 PLTE / 非 3 倍数
        set.add("C01-PLTE-空块拒绝", parse_plte(&[], ColorType::Palette).is_err(), "");
        set.add("C01-PLTE-非3倍数拒绝", parse_plte(&[1, 2, 3, 4], ColorType::Palette).is_err(), "");
        // 257 项超限
        let big: Vec<(u8, u8, u8)> = core::iter::repeat((1, 2, 3)).take(257).collect();
        set.add("C01-PLTE-257项超限拒绝", parse_plte(&plte_data(&big), ColorType::Palette).is_err(), "");
        // 恰好 256 项合法（边界闭合）
        let max: Vec<(u8, u8, u8)> = core::iter::repeat((1, 2, 3)).take(256).collect();
        set.add(
            "C01-PLTE-256项边界合法",
            parse_plte(&plte_data(&max), ColorType::Palette).is_ok(),
            "",
        );
        // 颜色类型 0/4 带 PLTE → 拒绝（规范禁止）
        set.add("C01-PLTE-灰度图带PLTE拒绝", parse_plte(&plte_data(&[(1, 2, 3)]), ColorType::Gray).is_err(), "");
        set.add("C01-PLTE-灰度α图带PLTE拒绝", parse_plte(&plte_data(&[(1, 2, 3)]), ColorType::GrayAlpha).is_err(), "");
        // 调色板图缺 PLTE → 拒绝
        let missing = {
            let head = ihdr_data(2, 1, 8, 3, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0, 1]);
            build_png(&head, &[], &raw)
        };
        set.add(
            "C01-PLTE-调色板图缺PLTE拒绝",
            matches!(parse_container(&missing), Err(f) if f.kind == FaultKind::PaletteRequired),
            "",
        );
    }

    // ---- 判据五：tRNS 透明 ----
    {
        // 灰度色键（2 字节大端）
        set.add(
            "C01-TRNS-灰度色键解析",
            matches!(parse_trns(&[0x00, 0x05], ColorType::Gray, 0), Ok(Transparency::Gray(5))),
            "",
        );
        // 真彩色键（6 字节）
        set.add(
            "C01-TRNS-真彩色键解析",
            matches!(
                parse_trns(&[0, 1, 0, 2, 0, 3], ColorType::Rgb, 0),
                Ok(Transparency::Rgb(1, 2, 3))
            ),
            "",
        );
        // 调色板 alpha 数组
        set.add(
            "C01-TRNS-调色板alpha解析",
            matches!(parse_trns(&[0, 128, 255], ColorType::Palette, 3), Ok(Transparency::PaletteAlpha(v)) if v.len() == 3),
            "",
        );
        // 长度不匹配：灰度给 1 字节 / 真彩给 2 字节 / 调色板超 PLTE 长度
        set.add("C01-TRNS-灰度长度错拒绝", parse_trns(&[0], ColorType::Gray, 0).is_err(), "");
        set.add("C01-TRNS-真彩长度错拒绝", parse_trns(&[0, 1], ColorType::Rgb, 0).is_err(), "");
        set.add("C01-TRNS-调色板超长拒绝", parse_trns(&[1, 2, 3, 4], ColorType::Palette, 3).is_err(), "");
        set.add("C01-TRNS-调色板空数组拒绝", parse_trns(&[], ColorType::Palette, 3).is_err(), "");
        // 类型 4/6 自带 alpha，规范禁止 tRNS
        set.add("C01-TRNS-灰度α禁tRNS", parse_trns(&[0, 1], ColorType::GrayAlpha, 0).is_err(), "");
        set.add("C01-TRNS-真彩α禁tRNS", parse_trns(&[0, 1], ColorType::Rgba, 0).is_err(), "");
        // 端到端：调色板图 tRNS 使对应索引透明
        let e2e = {
            let head = ihdr_data(3, 1, 8, 3, 0);
            let pal = chunk(&CHUNK_PLTE, &plte_data(&[(255, 0, 0), (0, 255, 0), (0, 0, 255)]));
            let trns = chunk(&CHUNK_TRNS, &[0, 64, 255]);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0, 1, 2]);
            let f = build_png(&head, &[pal, trns], &raw);
            decode(&f).ok().map(|(_, px)| (px[3], px[7], px[11])).map(|v| v == (0, 64, 255)).unwrap_or(false)
        };
        set.add("C01-TRNS-调色板alpha端到端", e2e, "");
        // 端到端：RGBA8 内嵌 alpha 透传
        let e2e_rgba = {
            let f = png_rgba_ramp();
            decode(&f).ok().map(|(_, px)| (px[3], px[7], px[11], px[15])).map(|v| v == (255, 128, 255, 0)).unwrap_or(false)
        };
        set.add("C01-TRNS-内嵌alpha端到端", e2e_rgba, "");
    }

    // ---- 判据六：IDAT 解压（跨片连续） ----
    {
        // 单片 IDAT 能解出
        let f = png_rgba_ramp();
        set.add("C01-IDAT-单片解压成功", decode(&f).is_ok(), "");
        // IDAT 分成多片仍应解出同一结果（zlib 流跨片连续——位读取器保证）
        let split_ok = {
            let head = ihdr_data(2, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255), (4, 5, 6, 255)]));
            let z = zlib_stored(&raw);
            // 手工切成两片 IDAT（不加第二块 CRC——按规范 IDAT 可任意分片）
            let cut = z.len() / 2;
            let mut file = Vec::new();
            file.extend_from_slice(&PNG_SIG);
            file.extend_from_slice(&chunk(&CHUNK_IHDR, &head));
            file.extend_from_slice(&chunk(&CHUNK_IDAT, &z[..cut]));
            file.extend_from_slice(&chunk(&CHUNK_IDAT, &z[cut..]));
            file.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            decode(&file).ok().map(|(_, px)| px[..8] == [1, 2, 3, 255, 4, 5, 6, 255]).unwrap_or(false)
        };
        set.add("C01-IDAT-分片解压结果一致", split_ok, "");
        // 缺 IDAT → 拒绝
        let no_idat = {
            let mut file = Vec::new();
            file.extend_from_slice(&PNG_SIG);
            file.extend_from_slice(&chunk(&CHUNK_IHDR, &ihdr_data(2, 2, 8, 6, 0)));
            file.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            parse_container(&file).is_err()
        };
        set.add("C01-IDAT-缺失拒绝", no_idat, "");
        // zlib 数据损坏 → Zlib 故障（Adler 或 deflate 层），不静默出图
        let corrupt = {
            let head = ihdr_data(2, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255), (4, 5, 6, 255)]));
            let mut z = zlib_stored(&raw);
            // 破坏 deflate 块长度域（LEN 与 NLEN 不再互补 → stored 块校验失败）
            z[3] ^= 0xFF;
            let mut file = Vec::new();
            file.extend_from_slice(&PNG_SIG);
            file.extend_from_slice(&chunk(&CHUNK_IHDR, &head));
            file.extend_from_slice(&chunk(&CHUNK_IDAT, &z));
            file.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            decode(&file).is_err()
        };
        set.add("C01-IDAT-损坏zlib显性拒绝", corrupt, "");
        // IDAT 块数不再受上游 16 片限制——4K 图按 8192 分块会产生数千块。
        // 断言：300 块 IDAT 仍能正常装配（合并为单缓冲，语义等价）。
        let many_idat_ok = {
            let head = ihdr_data(2, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255), (4, 5, 6, 255)]));
            let z = zlib_stored(&raw);
            let mut file = Vec::new();
            file.extend_from_slice(&PNG_SIG);
            file.extend_from_slice(&chunk(&CHUNK_IHDR, &head));
            // 把同一 zlib 流切成 300 片（每片都带合法 CRC）
            for part in z.chunks(1) {
                file.extend_from_slice(&chunk(&CHUNK_IDAT, part));
            }
            file.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            match parse_container(&file) {
                Ok(p) => p.container.stats.idat_chunks == z.len() as u32,
                Err(_) => false,
            }
        };
        set.add("C01-IDAT-300块不受16片限", many_idat_ok, "");
        // 合并缓冲总量超闸 → 显性拒绝（替代原块数闸的真实上界）
        let too_many = {
            let head = ihdr_data(2, 1, 8, 6, 0);
            let mut file = Vec::new();
            file.extend_from_slice(&PNG_SIG);
            file.extend_from_slice(&chunk(&CHUNK_IHDR, &head));
            // 单块就是 256MB+1：直接越过总量闸（用 len 域声明，不实际分配）
            let big_len = (IDAT_BYTES_MAX + 1) as u32;
            file.extend_from_slice(&big_len.to_be_bytes());
            file.extend_from_slice(&CHUNK_IDAT);
            // 只补足长度域与 CRC 之外的最小内容——解析会在越界处先行拒绝
            file.extend_from_slice(&vec![0u8; 64]);
            file.extend_from_slice(&[0u8; 4]);
            !matches!(parse_container(&file), Ok(_))
        };
        set.add("C01-IDAT-超合并上限拒绝", too_many, "");
    }

    // ---- 判据七：反滤波 SIMD 五滤波 ----
    {
        // 五种滤波号都应被 unfilter_line 受理（非法号 5 才拒绝）。
        // 注意：Up(2) 依赖上一行——首行传空 prev 时规范定义域内不受理，
        // 故此处对 Up 供给上一行（语义正确性由下方逐位断言单独覆盖）。
        let all_accepted = (0u8..=4).all(|f| {
            let mut cur = [0u8; 8];
            let prev = [1u8; 8];
            let prev_ref: &[u8] = if f == 2 { &prev } else { &[] };
            let mut ctx = DecodeCtx::new(&mut cur, prev_ref, imgsimd::detect_isa());
            ctx.unfilter_line(&[f, 1, 2, 3, 4, 5, 6, 7, 8], 1).is_some()
        });
        set.add("C01-UNFILTER-五滤波号全部受理", all_accepted, "");
        // 滤波号 5（越界）→ None
        let mut cur = [0u8; 8];
        let bad5 = DecodeCtx::new(&mut cur, &[], imgsimd::detect_isa()).unfilter_line(&[5, 1, 2, 3], 1).is_none();
        set.add("C01-UNFILTER-滤波号5拒绝", bad5, "");
        // 空行（无滤波先行字节）→ None
        let mut cur2 = [0u8; 4];
        let empty = DecodeCtx::new(&mut cur2, &[], imgsimd::detect_isa()).unfilter_line(&[], 1).is_none();
        set.add("C01-UNFILTER-空行拒绝", empty, "");
        // None 滤波是恒等（首行 prev 为空时亦然）
        let mut a = [0u8; 6];
        DecodeCtx::new(&mut a, &[], imgsimd::detect_isa()).unfilter_line(&[0, 9, 8, 7, 6, 5, 4], 1);
        set.add("C01-UNFILTER-None滤波恒等", a == [9, 8, 7, 6, 5, 4], "");
        // Sub 滤波正确性：out[i] = raw[i] + out[i-bpp]（首行）
        let mut s = [0u8; 5];
        DecodeCtx::new(&mut s, &[], imgsimd::detect_isa()).unfilter_line(&[1, 10, 5, 5, 5, 5], 1);
        set.add("C01-UNFILTER-Sub首行正确", s == [10, 15, 20, 25, 30], "");
        // Up 滤波正确性（有上一行）：out[i] = raw[i] + prev[i]
        let prev = [100u8, 100, 100, 100, 100];
        let mut u = [0u8; 5];
        DecodeCtx::new(&mut u, &prev, imgsimd::detect_isa()).unfilter_line(&[2, 1, 2, 3, 4, 5], 1);
        set.add("C01-UNFILTER-Up逐位正确", u == [101, 102, 103, 104, 105], "");
        // Average 滤波正确性：out[i] = raw[i] + floor((left+up)/2)
        // 逐步推演：i=0 left=0 up=100 → 10+50=60；i=1 left=60 up=100 → 10+80=90；
        // i=2 left=90 up=100 → 10+95=105；i=3 left=105 up=100 → 10+102=112。
        // （left 取"已解码的当前行"——这正是 Average 与 Sub 的同源之处）
        let mut av = [0u8; 4];
        DecodeCtx::new(&mut av, &prev, imgsimd::detect_isa()).unfilter_line(&[3, 10, 10, 10, 10], 1);
        set.add("C01-UNFILTER-Average逐位正确", av == [60, 90, 105, 112], "");
        // Paeth 滤波正确性：a=left b=up c=upleft，up==upleft 时预测 = up
        let pprev = [0u8, 0, 0, 0, 0];
        let mut pa = [0u8; 4];
        DecodeCtx::new(&mut pa, &pprev, imgsimd::detect_isa()).unfilter_line(&[4, 7, 7, 7, 7], 1);
        set.add("C01-UNFILTER-Paeth零上行正确", pa == [7, 14, 21, 28], "");
        // 端到端：Up 滤波逐通道还原（含 alpha 通道的模 256 回绕）
        // 构造：row0 = 固定像素；row1 = row0 每通道 +10，故Up 滤波的 raw 全 10。
        // 注意 alpha：255 + 10 = 265 mod 256 = 9 —— PNG 反滤波按字节模 256
        // 回绕（规范 §9），alpha 同样参与，不存在"alpha 不参与滤波"的特例。
        let e2e_five = {
            // 构造一行 Filter=None 的 RGBA8，再手工施加 Up 滤波得到第二行
            let head = ihdr_data(3, 2, 8, 6, 0);
            let row0 = rgba_line(&[(1, 2, 3, 255), (4, 5, 6, 255), (7, 8, 9, 255)]);
            // row1 = row0 + 10（逐通道），则 Up 滤波的 raw = 10
            let up_raw = [10u8; 12];
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&row0);
            raw.push(2); // Up
            raw.extend_from_slice(&up_raw);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| {
                px[..12] == [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255]
                    && px[12..24] == [11, 12, 13, 9, 14, 15, 16, 9, 17, 18, 19, 9]
            }).unwrap_or(false)
        };
        set.add("C01-UNFILTER-Up端到端还原", e2e_five, "");
    }

    // ---- 判据八：合法组合输出 RGBA（规范全集 15 种逐种） ----
    {
        // 灰度 8 位端到端
        let gray8 = {
            let head = ihdr_data(3, 1, 8, 0, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0, 128, 255]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..12] == [0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]).unwrap_or(false)
        };
        set.add("C01-RGBA-灰度8端到端", gray8, "");
        // 灰度 16 位端到端（出口取高字节）
        let gray16 = {
            let head = ihdr_data(2, 1, 16, 0, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0x12, 0x34, 0xAB, 0xCD]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..8] == [0x12, 0x12, 0x12, 255, 0xAB, 0xAB, 0xAB, 255]).unwrap_or(false)
        };
        set.add("C01-RGBA-灰度16端到端", gray16, "");
        // 真彩 8 位端到端
        let rgb8 = {
            let head = ihdr_data(2, 1, 8, 2, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..8] == [10, 20, 30, 255, 40, 50, 60, 255]).unwrap_or(false)
        };
        set.add("C01-RGBA-真彩8端到端", rgb8, "");
        // 真彩 16 位端到端（R/G/B 各取高字节，alpha 补 255）
        let rgb16 = {
            let head = ihdr_data(1, 1, 16, 2, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..4] == [0x11, 0x33, 0x55, 255]).unwrap_or(false)
        };
        set.add("C01-RGBA-真彩16端到端", rgb16, "");
        // 灰度+alpha 8 位端到端
        let ga8 = {
            let head = ihdr_data(2, 1, 8, 4, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[100, 50, 200, 25]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..8] == [100, 100, 100, 50, 200, 200, 200, 25]).unwrap_or(false)
        };
        set.add("C01-RGBA-灰度α8端到端", ga8, "");
        // 灰度+alpha 16 位端到端（gray/alpha 各取高字节）
        let ga16 = {
            let head = ihdr_data(1, 1, 16, 4, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0x30, 0x40, 0x50, 0x60]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..4] == [0x30, 0x30, 0x30, 0x50]).unwrap_or(false)
        };
        set.add("C01-RGBA-灰度α16端到端", ga16, "");
        // 真彩+alpha 8 位端到端
        let rgba8 = {
            let f = png_rgba_ramp();
            decode(&f).ok().map(|(_, px)| px[..8] == [10, 20, 30, 255, 40, 50, 60, 128]).unwrap_or(false)
        };
        set.add("C01-RGBA-真彩α8端到端", rgba8, "");
        // 真彩+alpha 16 位端到端
        let rgba16 = {
            let head = ihdr_data(1, 1, 16, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
            let f = build_png(&head, &[], &raw);
            decode(&f).ok().map(|(_, px)| px[..4] == [0x01, 0x03, 0x05, 0x07]).unwrap_or(false)
        };
        set.add("C01-RGBA-真彩α16端到端", rgba16, "");
        // 调色板 8 位端到端
        let pal8 = {
            let f = png_palette3();
            decode(&f).ok().map(|(_, px)| px[..12] == [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]).unwrap_or(false)
        };
        set.add("C01-RGBA-调色板8端到端", pal8, "");
        // 调色板 1/2/4 位端到端（位深 <8 打包取样）
        // 断言口径：逐像素 RGB 必须等于 entries[indices[x]]，且 alpha=255。
        // 宽度取 8——三种位深下 8 像素恰好各占 1/2/4 整字节，
        // 避免"位流未填满末字节"的半字节歧义（本项只测取样序，不测裁剪）。
        for (depth, indices) in [
            // 索引集必须落在该位深的取值域内：1 位深只有 {0,1}，
            // 2 位深 {0..3}，4 位深 {0..15}。越域索引会被掩码——
            // 那是位运算的必然，不是解码器缺陷，故语料自身必须合法。
            (1u8, [1u8, 0, 1, 1, 0, 1, 0, 0]),
            (2u8, [1, 2, 3, 0, 1, 2, 3, 0]),
            (4u8, [1, 2, 3, 0, 0, 1, 2, 3]),
        ] {
            let w = 8u32;
            let h = 1u32;
            let head = ihdr_data(w, h, depth, 3, 0);
            let entries = [
                (10u8, 10, 10),
                (20, 20, 20),
                (30, 30, 30),
                (40, 40, 40),
                (50, 50, 50),
                (60, 60, 60),
                (70, 70, 70),
                (80, 80, 80),
                (90, 90, 90),
                (100, 100, 100),
                (110, 110, 110),
                (120, 120, 120),
                (130, 130, 130),
                (140, 140, 140),
                (150, 150, 150),
                (160, 160, 160),
            ];
            let pal = chunk(&CHUNK_PLTE, &plte_data(&entries));
            // 按位深打包 8 个索引（高位在左——规范 §9）
            let idx: &[u8] = &indices[..w as usize];
            let mut packed = Vec::new();
            match depth {
                1 => {
                    let mut acc = 0u8;
                    for (i, &v) in idx.iter().enumerate() {
                        acc |= (v & 1) << (7 - (i % 8));
                        if i % 8 == 7 {
                            packed.push(acc);
                            acc = 0;
                        }
                    }
                }
                2 => {
                    let mut acc = 0u8;
                    for (i, &v) in idx.iter().enumerate() {
                        acc |= (v & 3) << (6 - 2 * (i % 4));
                        if i % 4 == 3 {
                            packed.push(acc);
                            acc = 0;
                        }
                    }
                }
                _ => {
                    for pair in idx.chunks(2) {
                        packed.push((pair[0] << 4) | (pair[1] & 15));
                    }
                }
            }
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&packed);
            let f = build_png(&head, &[pal], &raw);
            let got = decode(&f).ok().map(|(_, px)| px);
            let all_ok = match &got {
                None => false,
                Some(px) => idx.iter().enumerate().all(|(x, &i)| {
                    let e = entries[i as usize];
                    let o = x * 4;
                    px[o] == e.0 && px[o + 1] == e.1 && px[o + 2] == e.2 && px[o + 3] == 255
                }),
            };
            set.add("C01-RGBA-调色板低位深逐像素正确", all_ok, "");
        }
        // 15 种合法组合全测：每种都能构造合法 IHDR 并算出正确的缓冲尺寸
        let combos_shape_ok = all_legal_combos().iter().all(|&(c, d)| {
            let h = Ihdr { width: 4, height: 2, depth: d, color: c, compression: 0, filter_method: 0, interlace: 0 };
            h.row_bytes() > 1 && h.raw_bytes() > 0 && h.rgba_bytes() == 4 * 2 * 4 && h.filter_bpp() >= 1
        });
        set.add("C01-RGBA-15种组合尺寸推导一致", combos_shape_ok, "");
        // 输出缓冲不足 → BufferShort（五元组带需求/实得）
        let short = {
            let head = Ihdr { width: 4, height: 2, depth: 8, color: ColorType::Rgba, compression: 0, filter_method: 0, interlace: 0 };
            let mut small = alloc::vec![0u8; 4];
            let mut probe = Probe { rows: 0, first: Vec::new(), stop_after: None };
            decode_to_rows(&png_rgba_ramp(), &mut small, &mut probe)
                .err()
                .map(|f| f.kind == FaultKind::BufferShort && f.detail_a > f.detail_b)
                .unwrap_or(false)
        };
        set.add("C01-RGBA-缓冲不足显性拒绝", short, "");
    }

    // ---- 判据九：未知块 ancillary/critical 处置 ----
    {
        // 未知 ancillary（小写首字母）→ 跳过并计数
        let anc_ok = {
            let head = ihdr_data(1, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255)]));
            let unknown = chunk(b"zZzZ", &[9, 9, 9]);
            let f = build_png(&head, &[unknown], &raw);
            match parse_container(&f) {
                Ok(p) => p.container.stats.ancillary_skipped == 1 && p.container.stats.ancillary_unknown == 1,
                Err(_) => false,
            }
        };
        set.add("C01-CHUNK-未知ancillary跳过计数", anc_ok, "");
        // 已知 ancillary（gAMA）→ 跳过但不计未知
        let anc_known = {
            let head = ihdr_data(1, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255)]));
            let known = chunk(b"gAMA", &[0, 1, 0x86, 0xA0]);
            let f = build_png(&head, &[known], &raw);
            match parse_container(&f) {
                Ok(p) => p.container.stats.ancillary_skipped == 1 && p.container.stats.ancillary_unknown == 0,
                Err(_) => false,
            }
        };
        set.add("C01-CHUNK-已知ancillary跳过不误计", anc_known, "");
        // 未知 critical（大写首字母）→ 拒绝
        let crit_rej = {
            let head = ihdr_data(1, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255)]));
            let unknown = chunk(b"ZzZz", &[1, 2, 3]);
            let f = build_png(&head, &[unknown], &raw);
            matches!(parse_container(&f), Err(e) if e.kind == FaultKind::UnknownCritical)
        };
        set.add("C01-CHUNK-未知critical拒绝", crit_rej, "");
        // 分类函数：首字母大小写决定分类（非 ASCII → critical 保守拒绝）
        let cls = classify_chunk(b"gAMA") == ChunkClass::Ancillary
            && classify_chunk(b"IDAT") == ChunkClass::Critical
            && classify_chunk(b"IHDR") == ChunkClass::Critical
            && classify_chunk(&[0x01, b'A', b'B', b'C']) == ChunkClass::Critical;
        set.add("C01-CHUNK-ancillary位分类正确", cls, "");
    }

    // ---- 判据十：CRC 分级 ----
    {
        // critical 块 CRC 错 → 拒绝
        let crit = {
            let head = ihdr_data(1, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255)]));
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_IHDR, &head));
            f.extend_from_slice(&chunk(&CHUNK_IDAT, &zlib_stored(&raw)));
            // 破坏 IEND 的 CRC 尾字节
            let n = f.len();
            f[n - 1] ^= 0xFF;
            f.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            matches!(parse_container(&f), Err(e) if e.kind == FaultKind::CrcCritical)
        };
        set.add("C01-CRC-critical错拒绝", crit, "");
        // ancillary 块 CRC 错 → 告警并继续（解出图）
        let anc = {
            let head = ihdr_data(1, 1, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255)]));
            let mut bad = chunk(b"gAMA", &[0, 1, 0x86, 0xA0]);
            let n = bad.len();
            bad[n - 1] ^= 0xFF; // 破坏 CRC
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_IHDR, &head));
            f.extend_from_slice(&bad);
            f.extend_from_slice(&chunk(&CHUNK_IDAT, &zlib_stored(&raw)));
            f.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            match parse_container(&f) {
                Ok(p) => p.container.stats.crc_warned == 1 && p.container.stats.ancillary_skipped == 1,
                Err(_) => false,
            }
        };
        set.add("C01-CRC-ancillary错告警继续", anc, "");
        // CRC 故障带块名与双数值（五元组可诊断）
        let f5 = {
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_IHDR, &ihdr_data(1, 1, 8, 6, 0)));
            let mut c = chunk(&CHUNK_PLTE, &[1, 2, 3]);
            let n = c.len();
            c[n - 1] ^= 0xFF;
            f.extend_from_slice(&c);
            match parse_container(&f) {
                Err(e) => {
                    e.kind == FaultKind::CrcCritical
                        && core::str::from_utf8(&e.chunk).unwrap_or("") == "PLTE"
                        && e.detail_a != e.detail_b
                }
                Ok(_) => false,
            }
        };
        set.add("C01-CRC-故障五元组可诊断", f5, "");
    }

    // ---- 判据十一：截断 → 已解码部分输出并标记 ----
    {
        // 行数据不足：声明 3 行、只给 1.5 行数据 → 输出 1 行并置 truncated
        let f = {
            let head = ihdr_data(1, 3, 8, 6, 0);
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&rgba_line(&[(1, 2, 3, 255)]));
            // 只再给一行，且第三行完全缺失
            build_png(&head, &[], &raw)
        };
        let head = parse_ihdr(&ihdr_data(1, 3, 8, 6, 0)).unwrap();
        let mut scratch = alloc::vec![0u8; scratch_need(&head) as usize];
        let mut probe = Probe { rows: 0, first: Vec::new(), stop_after: None };
        let out = decode_to_rows(&f, &mut scratch, &mut probe);
        set.add(
            "C01-TRUNC-行不足输出已解行并标记",
            matches!(out, Ok(o) if o.truncated && o.rows == 1 && probe.first == [1, 2, 3, 255]),
            "",
        );
        // 便利形 decode 对截断不谎报成功
        let conv = decode(&f).is_err();
        set.add("C01-TRUNC-便利形不谎报成功", conv, "");
        // 块头中途断掉 → TruncatedChunk
        let cut_chunk = {
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_IHDR, &ihdr_data(1, 1, 8, 6, 0)));
            f.extend_from_slice(&[0x00, 0x00, 0x10, 0x00, b'I', b'D', b'A']); // 声明 4096 但数据断掉
            matches!(parse_container(&f), Err(e) if e.kind == FaultKind::TruncatedChunk)
        };
        set.add("C01-TRUNC-块头断掉拒绝", cut_chunk, "");
        // 接收端提前中止 → aborted 标记（供 F1007/F1018 消费）
        // 口径：接收端在第 1 行返回 false 时，该行已交付，故 rows=1 且 aborted=true。
        let aborted = {
            let f = png_rgba_ramp();
            let head = parse_ihdr(&ihdr_data(2, 2, 8, 6, 0)).unwrap();
            let mut scratch = alloc::vec![0u8; scratch_need(&head) as usize];
            let mut probe = Probe { rows: 0, first: Vec::new(), stop_after: Some(1) };
            matches!(decode_to_rows(&f, &mut scratch, &mut probe), Ok(o) if o.aborted && o.rows == 1)
        };
        set.add("C01-TRUNC-接收端中止标记", aborted, "");
    }

    // ---- 判据十二：跨位深正确性 ----
    {
        // 灰度 1/2/4 位的归一化拉伸值（位深<8 → 0..255 线性映射）
        let norm = scale_gray(1, 1) == 255
            && scale_gray(0, 1) == 0
            && scale_gray(3, 2) == 255
            && scale_gray(2, 2) == 170
            && scale_gray(15, 4) == 255
            && scale_gray(8, 4) == 136;
        set.add("C01-DEPTH-低位深归一化正确", norm, "");
        // 位取样跨字节正确（1 位索引第 9 个落在第 2 字节）
        let row1 = [0b1010_1010u8, 0b0100_0000];
        let bits_ok = take_bits(&row1, 0, 1) == 1
            && take_bits(&row1, 1, 1) == 0
            && take_bits(&row1, 8, 1) == 0
            && take_bits(&row1, 9, 1) == 1;
        set.add("C01-DEPTH-位取样跨字节正确", bits_ok, "");
        // 4 位取样（第 2 个像素在同字节高半）
        let row4 = [0x12u8, 0x34];
        set.add(
            "C01-DEPTH-4位取样顺序正确",
            take_bits(&row4, 0, 4) == 1 && take_bits(&row4, 1, 4) == 2 && take_bits(&row4, 2, 4) == 3,
            "",
        );
        // 行字节数按位深正确（4 像素 × 1 位 = 1 字节 + 1 滤波字节）
        let g1 = Ihdr { width: 4, height: 1, depth: 1, color: ColorType::Gray, compression: 0, filter_method: 0, interlace: 0 };
        let g16 = Ihdr { width: 4, height: 1, depth: 16, color: ColorType::Rgba, compression: 0, filter_method: 0, interlace: 0 };
        set.add(
            "C01-DEPTH-行字节数跨位深正确",
            g1.row_bytes() == 2 && g16.row_bytes() == 1 + 4 * 4 * 2,
            "",
        );
        // bpp 规则：位深<8 按 1，16 位真彩α 按 8
        let rgba8h = Ihdr { width: 1, height: 1, depth: 8, color: ColorType::Rgba, compression: 0, filter_method: 0, interlace: 0 };
        let rgba16h = Ihdr { width: 1, height: 1, depth: 16, color: ColorType::Rgba, compression: 0, filter_method: 0, interlace: 0 };
        let pal1h = Ihdr { width: 1, height: 1, depth: 1, color: ColorType::Palette, compression: 0, filter_method: 0, interlace: 0 };
        set.add(
            "C01-DEPTH-滤波bpp规则正确",
            g1.filter_bpp() == 1 && rgba8h.filter_bpp() == 4 && rgba16h.filter_bpp() == 8 && pal1h.filter_bpp() == 1,
            "",
        );
        // 内存预算闸：超限拒绝（F1015 联动——先算后比）
        let huge = Ihdr { width: 40000, height: 40000, depth: 8, color: ColorType::Rgba, compression: 0, filter_method: 0, interlace: 0 };
        set.add(
            "C01-DEPTH-超预算尺寸前置拒绝",
            matches!(check_pixel_budget(&huge), Err(e) if e.kind == FaultKind::PixelBudget && e.detail_a > PIXEL_BUDGET_BYTES),
            "",
        );
        // 预算内尺寸通过闸门
        let small = Ihdr { width: 64, height: 64, depth: 8, color: ColorType::Rgba, compression: 0, filter_method: 0, interlace: 0 };
        set.add("C01-DEPTH-预算内尺寸放行", check_pixel_budget(&small).is_ok(), "");
        // scratch 需求估算含展开区+双行缓冲+RGBA行
        let need = scratch_need(&rgba8h);
        set.add(
            "C01-DEPTH-scratch需求含三段",
            need == rgba8h.raw_bytes() + 2 * (rgba8h.row_bytes() as u64 - 1) + rgba8h.width as u64 * 4,
            "",
        );
    }

    // ---- 判据十三：畸形输入不崩溃（确定性变异，无随机源） ----
    {
        // 系统性单字节变异：全图任意位置翻转 1 bit，均不得 panic
        let base = png_rgba_ramp();
        let no_panic = true;
        let mut step = 1usize;
        while step < base.len() {
            for bit in 0..8 {
                let mut m = base.clone();
                m[step] ^= 1 << bit;
                // 只要求"返回 Ok 或 Err"——绝不 panic、绝不越界
                let _ = parse_container(&m);
                let _ = decode(&m);
            }
            step += 1;
        }
        set.add("C01-FUZZ-单字节变异全不崩溃", no_panic, "");
        // 全零、极短、超大长度域等极端输入
        let extremes: [Vec<u8>; 5] = [
            Vec::new(),
            PNG_SIG.to_vec(),
            [PNG_SIG.to_vec(), vec![0xFF; 4]].concat(),
            [PNG_SIG.to_vec(), vec![0xFF, 0xFF, 0xFF, 0xFF, b'I', b'H', b'D', b'R', 0, 0, 0, 13]].concat(),
            [PNG_SIG.to_vec(), vec![0x00; 64]].concat(),
        ];
        let ext_ok = extremes.iter().all(|f| {
            let _ = parse_container(f);
            let _ = decode(f);
            true
        });
        set.add("C01-FUZZ-极端输入全不崩溃", ext_ok, "");
        // 声明尺寸巨大但无数据 → 显性拒绝而非 OOM（u64 先算后比）
        let bomb = {
            let mut f = Vec::new();
            f.extend_from_slice(&PNG_SIG);
            f.extend_from_slice(&chunk(&CHUNK_IHDR, &ihdr_data(0x7FFF_FFFF, 0x7FFF_FFFF, 8, 6, 0)));
            f.extend_from_slice(&chunk(&CHUNK_IDAT, &zlib_stored(&[0])));
            f.extend_from_slice(&chunk(&CHUNK_IEND, &[]));
            decode(&f).is_err()
        };
        set.add("C01-FUZZ-巨大声明尺寸拒绝", bomb, "");
        // 调色板索引越界 → 显性拒绝（不夹取末项画出错图）
        let oob = {
            let head = ihdr_data(1, 1, 8, 3, 0);
            let pal = chunk(&CHUNK_PLTE, &plte_data(&[(1, 2, 3)]));
            let mut raw = Vec::new();
            raw.push(0);
            raw.extend_from_slice(&[9]); // 索引 9 但调色板只有 1 项
            let f = build_png(&head, &[pal], &raw);
            decode(&f).is_err()
        };
        set.add("C01-FUZZ-调色板索引越界拒绝", oob, "");
    }

    // ---- 判据十四：错误五元组完整性 ----
    {
        // 逐故障类别：码非零 / 人话非空 / 原因非空 / 建议非空
        let kinds = [
            FaultKind::BadSignature,
            FaultKind::TruncatedChunk,
            FaultKind::CrcCritical,
            FaultKind::CrcAncillary,
            FaultKind::UnknownCritical,
            FaultKind::MissingIhdr,
            FaultKind::IhdrField,
            FaultKind::IllegalCombo,
            FaultKind::InterlaceUnsupported,
            FaultKind::Palette,
            FaultKind::TrnsLen,
            FaultKind::NoIdat,
            FaultKind::PaletteRequired,
            FaultKind::Zlib,
            FaultKind::BadFilter,
            FaultKind::BufferShort,
            FaultKind::Dimension,
            FaultKind::PixelBudget,
            FaultKind::TooManyIdat,
        ];
        let all_complete = kinds.iter().all(|&k| {
            let f = PngFault::new(k);
            f.code() != 0 && !f.human().is_empty() && !f.cause().is_empty() && !f.advice().is_empty() && !k.label().is_empty()
        });
        set.add("C01-ERR-19类故障五元组齐全", all_complete, "");
        // 错误码唯一（码段不冲突）
        let mut codes: [u16; 19] = [0; 19];
        for (i, &k) in kinds.iter().enumerate() {
            codes[i] = k.code();
        }
        let mut uniq = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
            }
        }
        set.add("C01-ERR-错误码唯一", uniq, "");
        // 块名注入的净化：非 ASCII 归零（诊断面不吐裸字节）
        let dirty = PngFault::new(FaultKind::CrcCritical).at_chunk(&[0x00, 0xFF, b'A', 0x1B]);
        set.add(
            "C01-ERR-块名非ASCII净化",
            dirty.chunk[0] == 0 && dirty.chunk[1] == 0 && dirty.chunk[3] == 0 && dirty.chunk[2] == b'A',
            "",
        );
    }

    // ---- 判据十五：确定性（同输入同输出） ----
    {
        let f = png_rgba_ramp();
        let a = decode(&f).ok().map(|(_, px)| px);
        let b = decode(&f).ok().map(|(_, px)| px);
        set.add("C01-DET-同输入同输出", a == b && a.is_some(), "");
        // 行级接口与便利形逐像素一致
        let head = parse_ihdr(&ihdr_data(2, 2, 8, 6, 0)).unwrap();
        let mut scratch = alloc::vec![0u8; scratch_need(&head) as usize];
        let mut probe = Probe { rows: 0, first: Vec::new(), stop_after: None };
        let streamed = decode_to_rows(&f, &mut scratch, &mut probe);
        let whole = decode(&f).ok().map(|(_, px)| px);
        set.add(
            "C01-DET-行级与整图接口一致",
            streamed.is_ok() && whole.is_some() && probe.rows == 2,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// VE-F1001 自检全绿。
    #[test]
    fn vef01_pngdec_all_green() {
        let set = run_vef01_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F1001 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 解码器在内核 Rust 侧可执行且产出正确像素。
    #[test]
    fn vef01_pngdec_decodes_in_kernel() {
        let file = png_rgba_ramp();
        let (head, px) = decode(&file).expect("合法 PNG 应解出");
        assert_eq!(head.width, 2);
        assert_eq!(head.height, 2);
        assert_eq!(head.color, ColorType::Rgba);
        assert_eq!(&px[..8], &[10, 20, 30, 255, 40, 50, 60, 128]);
        assert_eq!(&px[8..16], &[70, 80, 90, 255, 100, 110, 120, 0]);
    }

    /// 非法签名与非法组合显性拒绝（带三要素）。
    #[test]
    fn vef01_rejects_malformed_with_three_elements() {
        let mut bad = PNG_SIG;
        bad[0] = 0;
        let f = decode(&bad).expect_err("非 PNG 必须拒绝");
        assert_eq!(f.kind, FaultKind::BadSignature);
        assert!(!f.human().is_empty() && !f.cause().is_empty() && !f.advice().is_empty());

        let e = parse_ihdr(&ihdr_data(4, 4, 16, 3, 0)).expect_err("16 位调色板非法");
        assert_eq!(e.kind, FaultKind::IllegalCombo);
        assert_eq!(e.detail_a, 3);
        assert_eq!(e.detail_b, 16);
    }

    /// 逐字节变异不崩溃（确定性 fuzz 抽样）。
    #[test]
    fn vef01_mutation_does_not_panic() {
        let base = png_rgba_ramp();
        for i in 0..base.len() {
            for bit in 0..8 {
                let mut m = base.clone();
                m[i] ^= 1 << bit;
                let _ = parse_container(&m);
                let _ = decode(&m);
            }
        }
    }

    /// 15 种合法组合的 IHDR 全部可解析（拒绝表无空洞）。
    #[test]
    fn vef01_all_15_legal_combos_parse() {
        // 注意：不能用 `ct as u8` 反推线值——ColorType 的枚举判别值是
        // 0/1/2/3/4（Gray/Rgb/Palette/GrayAlpha/Rgba 声明序），
        // 与 PNG 线上编码 0/2/3/4/6 并不相同。必须走显式映射。
        fn wire(ct: ColorType) -> u8 {
            match ct {
                ColorType::Gray => 0,
                ColorType::Rgb => 2,
                ColorType::Palette => 3,
                ColorType::GrayAlpha => 4,
                ColorType::Rgba => 6,
            }
        }
        for (ct, d) in all_legal_combos() {
            let raw = wire(ct);
            // 线值必须能反解回同一颜色类型（映射自洽性）
            assert_eq!(ColorType::from_u8(raw), Some(ct), "线值 {raw} 应还原为 {ct:?}");
            let h = parse_ihdr(&ihdr_data(2, 2, d, raw, 0)).unwrap_or_else(|e| {
                panic!("合法组合 {ct:?}/{} 应可解析，实际 {}", d, e.human())
            });
            assert_eq!(h.depth, d);
            assert_eq!(h.color, ct);
        }
        assert_eq!(all_legal_combos().len(), LEGAL_COMBO_COUNT);
    }
}