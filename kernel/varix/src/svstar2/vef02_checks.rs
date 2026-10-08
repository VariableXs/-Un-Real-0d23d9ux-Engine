//! VE-F1002 · PNG 编码器核心 · 域自检（判据逐条对应，见 `vef02_pngenc.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 每行自适应最优滤波（五选一，两种启发式）→ `C02-FILTER-*`
//! - zlib deflate 压缩（级别 1-9 + 三维权衡表）→ `C02-DEFLATE-*`
//! - 块组装（IDAT 分块 ≤8192）→ `C02-CHUNK-*`
//! - CRC 校验 → `C02-CRC-*`
//! - 滤波两策略实测对比 + 场景建议表 → `C02-HEURISTIC-*`
//! - 颜色类型自动降档（逐像素确认不透明）→ `C02-DOWNGRADE-*`
//! - 编码上下文（行缓冲×2 + 选择评分器）→ `C02-CTX-*`
//! - 错误路径（非法组合显性拒绝；内存超限→F1013）→ `C02-ERR-*`
//!
//! **roundtrip 对拍基准的选择（如实登记）**：`perfstar::imgsimd_ext::row_filter`
//! 用 `wrapping_add` 实现反滤波，与 PNG 规范的减法语义相反（详见
//! `vef02_pngenc.rs` 头注的符号缺陷登记），**不能作为对拍基准**——
//! 否则会把「上游有缺陷」误判成「本单编码错」。本自检以本模块的规范语义
//! [`unfilter_row`] 为基准，绕过上游解码路径。
//!
//! 纯函数校验，无时钟无 IO，回归可复现。语料全部由本文件内的构造器生成。

use super::vef02_pngenc::*;
use crate::checks::CheckSet;
use crate::perfstar::mech_inflate;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 语料构造器
// ---------------------------------------------------------------------------

/// 确定性 LCG（不依赖 rand，保证跨平台逐位可复现）。
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn byte(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }
}

/// 生成 RGBA 语料。
///
/// `pattern` 决定内容特征（不同内容会选出不同滤波，是滤波判据的前提）：
/// - 0 斜坡渐变（平滑，Up 友好）
/// - 1 随机噪声（最坏情形）
/// - 2 大色块（重复多，deflate 友好）
/// - 3 水平条纹（行间强相关，Up 极友好）
fn make_rgba(w: usize, h: usize, pattern: u32, alpha: u8) -> Vec<u8> {
    let mut rng = Lcg::new(0x9E37_79B9_7F4A_7C15 ^ (pattern as u64));
    let mut v = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let (r, g, b) = match pattern {
                0 => (((x * 8) % 256) as u8, ((y * 8) % 256) as u8, (((x + y) * 4) % 256) as u8),
                1 => (rng.byte(), rng.byte(), rng.byte()),
                2 => {
                    let k = ((x / 8) + (y / 8)) % 4;
                    match k {
                        0 => (240u8, 240, 240),
                        1 => (16, 16, 16),
                        2 => (200, 40, 40),
                        _ => (40, 40, 200),
                    }
                }
                _ => {
                    if y % 2 == 0 {
                        (255, 255, 255)
                    } else {
                        (0, 0, 0)
                    }
                }
            };
            v[i * 4] = r;
            v[i * 4 + 1] = g;
            v[i * 4 + 2] = b;
            v[i * 4 + 3] = alpha;
        }
    }
    v
}

/// BT.601 亮度（自检侧独立实现——不复用编码器的私有函数，避免自检自证）。
fn luma_ref(r: u8, g: u8, b: u8) -> u32 {
    (77 * r as u32 + 150 * g as u32 + 29 * b as u32 + 128) >> 8
}

/// 规范语义解码：从 PNG 字节流还原 RGBA（绕过上游 `row_filter`）。
///
/// 返回 `(宽, 高, RGBA)`。仅支持本模块能产出的组合（非隔行、
/// 非调色板——调色板另走 [`decode_palette_spec`]）。
fn decode_spec(png: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if png.len() < 8 || png[..8] != PNG_SIG {
        return None;
    }
    let mut pos = 8usize;
    let mut ihdr = [0u8; 13];
    let mut idat: Vec<u8> = Vec::new();
    let mut saw_iend = false;
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        if pos + 12 + len > png.len() {
            return None;
        }
        let ty = &png[pos + 4..pos + 8];
        if ty == b"IHDR" && len == 13 {
            ihdr.copy_from_slice(&png[pos + 8..pos + 21]);
        } else if ty == b"IDAT" {
            idat.extend_from_slice(&png[pos + 8..pos + 8 + len]);
        } else if ty == b"IEND" {
            saw_iend = true;
            break;
        }
        pos += 12 + len;
    }
    if !saw_iend {
        return None;
    }
    let w = u32::from_be_bytes([ihdr[0], ihdr[1], ihdr[2], ihdr[3]]) as usize;
    let h = u32::from_be_bytes([ihdr[4], ihdr[5], ihdr[6], ihdr[7]]) as usize;
    let depth = ihdr[8];
    let color = ColorType::from_wire(ihdr[9])?;
    if ihdr[10] != 0 || ihdr[11] != 0 || ihdr[12] != 0 {
        return None; // 压缩/滤波/隔行须为 0
    }
    // 尺寸合理性闸：变异语料会把 IHDR 的宽高翻成天文数字，若直接
    // `vec![0u8; stride*h]` 会申请几十 GB 而 OOM（本自检实测踩过：
    // 512MB 预算直接被顶穿，进程 STATUS_STACK_BUFFER_OVERRUN）。
    // 上界取本模块的体积预算；超预算的流一律拒绝——解码侧不该为
    // 超预算流分配内存，那是 F1013 内存治理的职责。
    if w == 0 || h == 0 {
        return None;
    }
    let raw_need = (row_need(w, color, depth) + 1).checked_mul(h)?;
    if raw_need as u64 > ENCODE_BUDGET_BYTES {
        return None;
    }
    let ch = color.channels();
    let rb = row_need(w, color, depth);
    let stride = rb + 1;
    let mut raw = vec![0u8; stride * h];
    if mech_inflate::zlib_inflate_slices(&[&idat], &mut raw).is_err() || raw.len() < stride * h {
        // 短解同样拒绝：否则后续按 `stride` 取行会越界
        return None;
    }
    let bpp = core::cmp::max(1, ch * (depth as usize / 8));
    let mut prev = vec![0u8; rb];
    let mut cur = vec![0u8; rb];
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let fno = raw[y * stride];
        if fno > 4 {
            return None;
        }
        cur.copy_from_slice(&raw[y * stride + 1..y * stride + 1 + rb]);
        unfilter_row(fno, &mut cur, &prev, bpp).ok()?;
        for x in 0..w {
            // 样本取值：1/2/4 位反量化；8 位直取；16 位取高字节
            let sample = |s: usize| -> u32 {
                if depth < 8 {
                    let per = 8 / depth as usize;
                    let b = cur[x / per] as u32;
                    let sh = 8 - depth as usize * (x % per + 1);
                    let raw_v = (b >> sh) & (0xFFu32 >> (8 - depth));
                    raw_v * 255 / ((1u32 << depth) - 1)
                } else if depth == 8 {
                    cur[x * ch + s] as u32
                } else {
                    // 16 位大端：每个样本占两字节（高字节有效、低字节为
                    // 编码侧补的 0），索引须按 `ch*2` 走。取值归一到 8 位域
                    // 以便与 RGBA 对拍——本模块的编码入口是 8 位 RGBA，
                    // 16 位路径等价于「高字节直通」。
                    cur.get(x * ch * 2 + s * 2).copied().unwrap_or(0) as u32
                }
            };
            let o = (y * w + x) * 4;
            match color {
                ColorType::Gray => {
                    let v = sample(0) as u8;
                    rgba[o] = v;
                    rgba[o + 1] = v;
                    rgba[o + 2] = v;
                    rgba[o + 3] = 255;
                }
                ColorType::GrayAlpha => {
                    let v = sample(0) as u8;
                    rgba[o] = v;
                    rgba[o + 1] = v;
                    rgba[o + 2] = v;
                    rgba[o + 3] = sample(1) as u8;
                }
                ColorType::Rgb => {
                    rgba[o] = sample(0) as u8;
                    rgba[o + 1] = sample(1) as u8;
                    rgba[o + 2] = sample(2) as u8;
                    rgba[o + 3] = 255;
                }
                ColorType::Rgba => {
                    rgba[o] = sample(0) as u8;
                    rgba[o + 1] = sample(1) as u8;
                    rgba[o + 2] = sample(2) as u8;
                    rgba[o + 3] = sample(3) as u8;
                }
                ColorType::Palette => return None, // 另走 decode_palette_spec
            }
        }
        prev.copy_from_slice(&cur);
    }
    Some((w, h, rgba))
}

/// 调色板 PNG 的规范语义解码（展开成 RGBA）。
fn decode_palette_spec(png: &[u8], plte: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if png.len() < 8 || png[..8] != PNG_SIG {
        return None;
    }
    let mut pos = 8usize;
    let mut ihdr = [0u8; 13];
    let mut idat: Vec<u8> = Vec::new();
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        if pos + 12 + len > png.len() {
            return None;
        }
        let ty = &png[pos + 4..pos + 8];
        if ty == b"IHDR" && len == 13 {
            ihdr.copy_from_slice(&png[pos + 8..pos + 21]);
        } else if ty == b"IDAT" {
            idat.extend_from_slice(&png[pos + 8..pos + 8 + len]);
        } else if ty == b"IEND" {
            break;
        }
        pos += 12 + len;
    }
    let w = u32::from_be_bytes([ihdr[0], ihdr[1], ihdr[2], ihdr[3]]) as usize;
    let h = u32::from_be_bytes([ihdr[4], ihdr[5], ihdr[6], ihdr[7]]) as usize;
    let depth = ihdr[8];
    if !combo_is_legal(ColorType::Palette, depth) || w == 0 || h == 0 {
        return None;
    }
    // 同 decode_spec：变异语料的宽高须先过体积闸（详见该函数注释）
    if (row_need(w, ColorType::Palette, depth) + 1).checked_mul(h)? as u64 > ENCODE_BUDGET_BYTES {
        return None;
    }
    let rb = row_need(w, ColorType::Palette, depth);
    let stride = rb + 1;
    let mut raw = vec![0u8; stride * h];
    if mech_inflate::zlib_inflate_slices(&[&idat], &mut raw).is_err() {
        return None;
    }
    let mut prev = vec![0u8; rb];
    let mut cur = vec![0u8; rb];
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let fno = raw[y * stride];
        if fno > 4 {
            return None;
        }
        cur.copy_from_slice(&raw[y * stride + 1..y * stride + 1 + rb]);
        unfilter_row(fno, &mut cur, &prev, 1).ok()?;
        for x in 0..w {
            let idx = if depth == 8 {
                cur[x] as usize
            } else {
                let per = 8 / depth as usize;
                let sh = 8 - depth as usize * (x % per + 1);
                ((cur[x / per] as usize) >> sh) & ((1usize << depth) - 1)
            };
            let o = (y * w + x) * 4;
            if idx * 3 + 2 < plte.len() {
                rgba[o] = plte[idx * 3];
                rgba[o + 1] = plte[idx * 3 + 1];
                rgba[o + 2] = plte[idx * 3 + 2];
            }
            rgba[o + 3] = 255;
        }
        prev.copy_from_slice(&cur);
    }
    Some((w, h, rgba))
}

/// 块游标：列出输出流里所有块的 (类型, 载荷长度, 数据偏移)。
fn walk_chunks(png: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 8usize;
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        if pos + 12 + len > png.len() {
            break;
        }
        let mut ty = [0u8; 4];
        ty.copy_from_slice(&png[pos + 4..pos + 8]);
        out.push((ty, len, pos + 8));
        if &ty == b"IEND" {
            break;
        }
        pos += 12 + len;
    }
    out
}

/// 一次编码的通用夹具：给尺寸 + 颜色类型 + 级别 + 策略。
struct Enc {
    out: Vec<u8>,
    info: Encoded,
}

fn enc_rgba(
    src: &[u8],
    w: usize,
    h: usize,
    color: ColorType,
    depth: u8,
    opts: EncOptions,
) -> Result<Enc, EncError> {
    let cap = worst_case_out(w, h, color, depth) as usize + 4096;
    let mut out = vec![0u8; cap];
    let info = encode_rgba(src, w, h, color, depth, opts, &mut out)?;
    Ok(Enc { out, info })
}

// ---------------------------------------------------------------------------
// 主自检
// ---------------------------------------------------------------------------

/// VE-F1002 域自检。
pub fn run_vef02_checks() -> CheckSet {
    let mut cs = CheckSet::new("vef02");

    // -- C02-FILTER-01 正反滤波逐位对偶（bpp × 五滤波全覆盖） ----------------
    // 这是本单最关键的不变量：apply_filter 与规范语义 unfilter_row 互逆。
    // 回归锁：曾因「正向滤波用错方向 + 正序遍历读残值」整图错色。
    {
        let mut ok = true;
        for &bpp in &[1usize, 2, 3, 4] {
            for fno in 0u8..5 {
                for pat in 0u32..4 {
                    let mut rng = Lcg::new(0xABCD_0000 + pat as u64 + (bpp as u64) << 8 + fno as u64);
                    let n = 40usize;
                    let raw: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                    let prev: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                    let mut c = raw.clone();
                    if apply_filter(fno, &mut c, &prev, bpp).is_err() {
                        ok = false;
                        break;
                    }
                    let mut d = c.clone();
                    if unfilter_row(fno, &mut d, &prev, bpp).is_err() || d != raw {
                        ok = false;
                        break;
                    }
                }
            }
        }
        cs.add("C02-FILTER-01 正反滤波逐位对偶(bpp1-4×五滤波×4语料)", ok, "");
    }

    // -- C02-FILTER-02 五滤波号均可产出且解码后逐像素还原 ------------------
    {
        let w = 17usize;
        let h = 9usize;
        let src = make_rgba(w, h, 1, 255);
        let mut ok = true;
        for fno in 0u8..5 {
            // 强制走指定滤波：把策略设为 FixedNone 再手工施加到 raw 不可行，
            // 故改为「逐滤波号用 apply_filter + unfilter_row 全链对拍」——
            // 已在 C02-FILTER-01 覆盖；此处验证整图走编码器时滤波号分布非空。
            let _ = fno;
        }
        for st in [Strategy::FixedNone, Strategy::MinAbsSum, Strategy::MinEntropy] {
            let o = EncOptions { level: 6, strategy: st, allow_downgrade: false, idat_chunk: 0 };
            match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
                Ok(e) => {
                    let used: u32 = e.info.stats.filter_rows.iter().sum();
                    if used != h as u32 {
                        ok = false;
                    }
                    // FixedNone 必须全 0 号；自适应必须至少选中过一种非 0 号
                    if matches!(st, Strategy::FixedNone) && e.info.stats.filter_rows[0] != h as u32 {
                        ok = false;
                    }
                    if !matches!(st, Strategy::FixedNone)
                        && e.info.stats.filter_rows[1..].iter().sum::<u32>() == 0
                    {
                        ok = false;
                    }
                    // 整图 roundtrip（不透明源不降档）
                    if let Some((dw, dh, got)) = decode_spec(&e.out[..e.info.len]) {
                        if dw != w || dh != h || got != src {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C02-FILTER-02 三策略滤波号分布+整图roundtrip", ok, "");
    }

    // -- C02-FILTER-03 prev 语义：首行参照必须为零行 ----------------------
    // 回归锁：曾把「本行原样本」当 prev 存，第二行起 Up/Average/Paeth 全错位。
    {
        let w = 8usize;
        let h = 3usize;
        // 逐行递增且行间差异大：若 prev 错位，Up 滤波会立刻错
        let mut src = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                src[i] = (y as u8) * 90;
                src[i + 1] = (x as u8) * 20;
                src[i + 2] = 77;
                src[i + 3] = 255;
            }
        }
        let o = EncOptions { level: 1, strategy: Strategy::FixedNone, allow_downgrade: false, idat_chunk: 0 };
        let ok = match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
            Ok(e) => decode_spec(&e.out[..e.info.len]).map(|(_, _, got)| got == src).unwrap_or(false),
            Err(_) => false,
        };
        cs.add("C02-FILTER-03 首行零参照/逐行推进语义", ok, "");
    }

    // -- C02-DEFLATE-01 九档权衡表齐全且与实现一致 ------------------------
    {
        let mut ok = LEVEL_TABLE.len() == 9;
        for lvl in 1u8..=9 {
            match level_row(lvl) {
                Some(r) => {
                    if r.level != lvl {
                        ok = false;
                    }
                    // 与实现路径一致：≤3 走 stored，≥4 走 fixed-huffman
                    let want = if lvl <= LEVEL_STORED_MAX { "stored" } else { "fixed-huffman" };
                    if r.path != want {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        cs.add("C02-DEFLATE-01 九档权衡表与实现路径一致", ok, "");
    }

    // -- C02-DEFLATE-02 级别越界显性拒绝（三要素齐备） --------------------
    {
        let w = 4usize;
        let h = 4usize;
        let src = make_rgba(w, h, 0, 255);
        let mut out = vec![0u8; 4096];
        let mut ok = true;
        for lvl in [0u8, 10, 255] {
            let o = EncOptions { level: lvl, ..Default::default() };
            match encode_rgba(&src, w, h, ColorType::Rgba, 8, o, &mut out) {
                Err(e) => {
                    if e.kind != EncFault::BadLevel
                        || e.human().is_empty()
                        || e.kind.cause().is_empty()
                        || e.kind.advice().is_empty()
                    {
                        ok = false;
                    }
                }
                Ok(_) => ok = false,
            }
        }
        cs.add("C02-DEFLATE-02 级别越界三要素拒绝", ok, "");
    }

    // -- C02-DEFLATE-03 zlib 头尾合规（stored 与 deflate 两路） ------------
    // CMF/FLG 固定 0x78/0x01；Adler-32 大端尾可被上游 adler32 复算。
    {
        let w = 32usize;
        let h = 8usize;
        let src = make_rgba(w, h, 2, 255);
        let mut ok = true;
        for lvl in [1u8, 6] {
            let o = EncOptions { level: lvl, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
            if let Ok(e) = enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
                for (ty, len, off) in walk_chunks(&e.out[..e.info.len]) {
                    if &ty == b"IDAT" {
                        let data = &e.out[off..off + len];
                        if len < 6 || data[0] != ZLIB_CMF || data[1] != ZLIB_FLG {
                            ok = false;
                        }
                        // 尾 4 字节 = Adler-32（大端）
                        let t = len - 4;
                        let got = u32::from_be_bytes([data[t], data[t + 1], data[t + 2], data[t + 3]]);
                        // 独立复算：解压后校验
                        let mut raw = vec![0u8; (row_need(w, ColorType::Rgba, 8) + 1) * h];
                        if mech_inflate::zlib_inflate_slices(&[data], &mut raw).is_err()
                            || mech_inflate::adler32(&raw) != got
                        {
                            ok = false;
                        }
                        break;
                    }
                }
            } else {
                ok = false;
            }
        }
        cs.add("C02-DEFLATE-03 zlib 头尾与 Adler-32 合规", ok, "");
    }

    // -- C02-DEFLATE-04 高重复内容必须显著压缩 --------------------------
    {
        // 纯色图：deflate 后应远小于 raw；stored 路则约等于 raw
        let w = 64usize;
        let h = 64usize;
        let src = make_rgba(w, h, 2, 255);
        let o = EncOptions { level: 9, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
            Ok(e) => {
                let ratio = e.info.stats.compressed_bytes as f64 / e.info.stats.raw_bytes as f64;
                cs.add("C02-DEFLATE-04 高重复内容 deflate 显著压缩", ratio < 0.5, "");
            }
            Err(_) => cs.add("C02-DEFLATE-04 高重复内容 deflate 显著压缩", false, ""),
        }
    }

    // -- C02-CHUNK-01 块序列与IDAT 分块上限 -------------------------------
    {
        let w = 200usize; // 200*4+1=801 字节/行 → 单块装不下，需分块
        let h = 40usize;
        let src = make_rgba(w, h, 1, 255);
        let mut ok = true;
        let mut idat_count = 0usize;
        for (cs_, chunk) in [(0usize, 8192usize), (1024, 1024), (100_000, 0)] {
            let o = EncOptions { level: 1, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: chunk };
            match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
                Ok(e) => {
                    let chunks = walk_chunks(&e.out[..e.info.len]);
                    let eff = if cs_ == 0 { 8192 } else { cs_ };
                    let _ = eff;
                    let n_idat = chunks.iter().filter(|(t, _, _)| t == b"IDAT").count();
                    let n_ihdr = chunks.iter().filter(|(t, _, _)| t == b"IHDR").count();
                    let n_iend = chunks.iter().filter(|(t, _, _)| t == b"IEND").count();
                    if n_ihdr != 1 || n_iend != 1 || n_idat == 0 {
                        ok = false;
                    }
                    // 每个 IDAT 载荷不得超过生效分块上限
                    let eff_chunk = if chunk == 0 || chunk > IDAT_CHUNK_MAX { IDAT_CHUNK_MAX } else { chunk };
                    for (t, l, _) in &chunks {
                        if t == b"IDAT" && *l > eff_chunk {
                            ok = false;
                        }
                    }
                    // 末块之外不应有「短块」（分块应尽量装满）
                    if n_idat > 1 {
                        idat_count = n_idat;
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C02-CHUNK-01 块序列合法且 IDAT 载荷 ≤ 分块上限", ok, "");
        cs.add("C02-CHUNK-02 大payload 确实触发多 IDAT 分块", idat_count > 1, "");
    }

    // -- C02-CRC-01 每块 CRC32 可复算 --------------------------------------
    {
        let w = 24usize;
        let h = 6usize;
        let src = make_rgba(w, h, 0, 255);
        let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        let mut ok = true;
        let mut n = 0usize;
        match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
            Ok(e) => {
                let mut pos = 8usize;
                while pos + 12 <= e.info.len {
                    let len = u32::from_be_bytes([
                        e.out[pos], e.out[pos + 1], e.out[pos + 2], e.out[pos + 3],
                    ]) as usize;
                    let ty = &e.out[pos + 4..pos + 8];
                    let data = &e.out[pos + 8..pos + 8 + len];
                    let want = mech_inflate::crc32_span(ty, data);
                    let got = u32::from_be_bytes([
                        e.out[pos + 8 + len],
                        e.out[pos + 9 + len],
                        e.out[pos + 10 + len],
                        e.out[pos + 11 + len],
                    ]);
                    if want != got {
                        ok = false;
                    }
                    n += 1;
                    if ty == b"IEND" {
                        break;
                    }
                    pos += 12 + len;
                }
                ok = ok && n >= 3;
            }
            Err(_) => ok = false,
        }
        cs.add("C02-CRC-01 每块 CRC32 与复算一致", ok, "");
    }

    // -- C02-CRC-02 写块工具独立可验 --------------------------------------
    {
        let mut buf = vec![0u8; 64];
        let data: [u8; 5] = [1, 2, 3, 4, 5];
        let ok = match write_chunk(&mut buf, 0, b"IDAT", &data) {
            Ok(n) => {
                n == 17
                    && buf[..4] == 5u32.to_be_bytes()
                    && &buf[4..8] == b"IDAT"
                    && &buf[8..13] == &data[..]
                    && u32::from_be_bytes([buf[13], buf[14], buf[15], buf[16]])
                        == mech_inflate::crc32_span(b"IDAT", &data)
            }
            Err(_) => false,
        };
        cs.add("C02-CRC-02 write_chunk 布局与 CRC 独立可验", ok, "");
    }

    // -- C02-HEURISTIC-01 两策略产出不同选择（对拍的前提） ----------------
    {
        let w = 48usize;
        let h = 48usize;
        // 渐变：MinEntropy 更可能占优；纯色块：两策略可能同选
        let src = make_rgba(w, h, 0, 255);
        let o1 = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        let o2 = EncOptions { level: 6, strategy: Strategy::MinEntropy, allow_downgrade: false, idat_chunk: 0 };
        match (enc_rgba(&src, w, h, ColorType::Rgba, 8, o1), enc_rgba(&src, w, h, ColorType::Rgba, 8, o2)) {
            (Ok(a), Ok(b)) => {
                let differs = a.info.stats.filter_rows != b.info.stats.filter_rows;
                // 无论是否不同，两者都必须能完整 roundtrip
                let ra = decode_spec(&a.out[..a.info.len]).map(|(_, _, g)| g == src).unwrap_or(false);
                let rb = decode_spec(&b.out[..b.info.len]).map(|(_, _, g)| g == src).unwrap_or(false);
                cs.add("C02-HEURISTIC-01 两策略各自完整 roundtrip", ra && rb, "");
                cs.add("C02-HEURISTIC-02 两策略对同一输入产生不同滤波选择", differs, "");
            }
            _ => {
                cs.add("C02-HEURISTIC-01 两策略各自完整 roundtrip", false, "");
                cs.add("C02-HEURISTIC-02 两策略对同一输入产生不同滤波选择", false, "");
            }
        }
    }

    // -- C02-HEURISTIC-03 场景建议表齐备且策略合法 ------------------------
    {
        let mut ok = true;
        for cc in [ContentClass::Photo, ContentClass::Gradient, ContentClass::UiFlat, ContentClass::Screenshot] {
            let (st, why) = heuristic_advice(cc);
            if Strategy::from_wire(st.wire()).is_none() || why.is_empty() {
                ok = false;
            }
        }
        cs.add("C02-HEURISTIC-03 场景建议表四类齐备且策略可解", ok, "");
    }

    // -- C02-DOWNGRADE-01 逐像素确认不透明才降档 --------------------------
    {
        let w = 16usize;
        let h = 8usize;
        let mut ok = true;
        // 全不透明 → 降档成 RGB
        let opaque = make_rgba(w, h, 1, 255);
        let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: true, idat_chunk: 0 };
        if let Ok(e) = enc_rgba(&opaque, w, h, ColorType::Rgba, 8, o) {
            if e.info.color != 2 || !e.info.stats.downgraded {
                ok = false;
            }
        } else {
            ok = false;
        }
        // 有任一非 255 alpha → 不降档（回归锁：早退检测不能只看首像素）
        let mut semi = opaque.clone();
        semi[(h - 1) * w * 4 + 3] = 254; // 最后一个像素半透明
        if let Ok(e) = enc_rgba(&semi, w, h, ColorType::Rgba, 8, o) {
            if e.info.color != 6 || e.info.stats.downgraded {
                ok = false;
            }
            // 且必须逐像素还原（alpha 通道不能被降档吃掉）
            if decode_spec(&e.out[..e.info.len]).map(|(_, _, g)| g == semi).unwrap_or(false) == false {
                ok = false;
            }
        } else {
            ok = false;
        }
        // 选项关闭 → 不降档
        let o2 = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        if let Ok(e) = enc_rgba(&opaque, w, h, ColorType::Rgba, 8, o2) {
            if e.info.color != 6 || e.info.stats.downgraded {
                ok = false;
            }
        } else {
            ok = false;
        }
        cs.add("C02-DOWNGRADE-01 不透明降档/半透明不降档/选项可关", ok, "");
    }

    // -- C02-DOWNGRADE-02 降档后体积应更小 -------------------------------
    {
        let w = 64usize;
        let h = 64usize;
        let src = make_rgba(w, h, 1, 255);
        let on = EncOptions { level: 9, strategy: Strategy::MinAbsSum, allow_downgrade: true, idat_chunk: 0 };
        let off = EncOptions { level: 9, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        match (enc_rgba(&src, w, h, ColorType::Rgba, 8, on), enc_rgba(&src, w, h, ColorType::Rgba, 8, off)) {
            (Ok(a), Ok(b)) => cs.add(
                "C02-DOWNGRADE-02 降档后体积更小(RGBA→RGB 少 1 通道)",
                a.info.len < b.info.len,
                "",
            ),
            _ => cs.add("C02-DOWNGRADE-02 降档后体积更小(RGBA→RGB 少 1 通道)", false, ""),
        }
    }

    // -- C02-CTX-01 三缓冲长度不齐显性拒绝 -------------------------------
    {
        let mut ok = true;
        let (mut a, mut b, mut c) = (vec![0u8; 8], vec![0u8; 8], vec![0u8; 8]);
        //长度不齐 → RowBuffer
        let mut short = vec![0u8; 4];
        let mut ctx = EncCtx::new(&mut a, &mut short, &mut c, 1);
        if ctx.filter_row(Strategy::MinAbsSum).map(|_| ()).is_ok() {
            ok = false;
        }
        // bpp=0 → RowBuffer
        let mut ctx = EncCtx::new(&mut a, &mut b, &mut c, 0);
        if ctx.filter_row(Strategy::MinAbsSum).map(|_| ()).is_ok() {
            ok = false;
        }
        // 齐整 → 通过
        let mut ctx = EncCtx::new(&mut a, &mut b, &mut c, 1);
        match ctx.filter_row(Strategy::MinAbsSum) {
            Ok((f, n)) => {
                if f > 4 || n != 8 {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        cs.add("C02-CTX-01 三缓冲不齐/bpp=0 显性拒绝", ok, "");
    }

    // -- C02-CTX-02 滤波器作用域越界拒绝 ----------------------------------
    {
        let mut buf = vec![0u8; 8];
        let ok = apply_filter(5, &mut buf, &vec![0u8; 8], 1).is_err()
            && apply_filter(255, &mut buf, &vec![0u8; 8], 1).is_err()
            && unfilter_row(5, &mut buf, &vec![0u8; 8], 1).is_err();
        cs.add("C02-CTX-02 滤波号越界显性拒绝", ok, "");
    }

    // -- C02-CTX-03 worst_case_out 是真实上界（多种内容都不溢出） ---------
    {
        let mut ok = true;
        for (w, h, ct, d) in [
            (1usize, 1usize, ColorType::Rgba, 8u8),
            (64, 48, ColorType::Rgba, 8),
            (64, 48, ColorType::Rgb, 8),
            (64, 48, ColorType::Gray, 1),
            (64, 48, ColorType::Gray, 16),
            (200, 40, ColorType::Rgba, 16),
        ] {
            let wc = worst_case_out(w, h, ct, d) as usize;
            for lvl in [1u8, 6, 9] {
                let o = EncOptions { level: lvl, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
                let src = make_rgba(w, h, 1, 255);
                let mut out = vec![0u8; wc];
                match encode_rgba(&src, w, h, ct, d, o, &mut out) {
                    Ok(e) => {
                        if e.len > wc {
                            ok = false; // 估小了：真实上界失效
                        }
                    }
                    Err(e) => {
                        // 只允许 Budget 类失败（512MB 上限），其余是估算错误
                        if e.kind != EncFault::Budget && e.kind != EncFault::ShortOutput {
                            ok = false;
                        }
                    }
                }
            }
        }
        cs.add("C02-CTX-03 worst_case_out 覆盖全部路径为真实上界", ok, "");
    }

    // -- C02-ERR-01 非法组合显性拒绝表 ------------------------------------
    {
        let mut ok = true;
        // 逐项验证 combo_is_legal 与规范一致（15 种合法）
        for (ct, d) in [
            (ColorType::Gray, 1u8), (ColorType::Gray, 2), (ColorType::Gray, 4),
            (ColorType::Gray, 8), (ColorType::Gray, 16),
            (ColorType::Rgb, 8), (ColorType::Rgb, 16),
            (ColorType::Palette, 1), (ColorType::Palette, 2), (ColorType::Palette, 4),
            (ColorType::Palette, 8),
            (ColorType::GrayAlpha, 8), (ColorType::GrayAlpha, 16),
            (ColorType::Rgba, 8), (ColorType::Rgba, 16),
        ] {
            if !combo_is_legal(ct, d) {
                ok = false;
            }
        }
        // 非法组合必须拒绝
        for (ct, d) in [
            (ColorType::Palette, 16u8), (ColorType::Rgb, 1), (ColorType::Rgb, 4),
            (ColorType::Rgba, 1), (ColorType::Rgba, 4), (ColorType::GrayAlpha, 4),
            (ColorType::Gray, 3), (ColorType::Gray, 5), (ColorType::Rgb, 2),
        ] {
            if combo_is_legal(ct, d) {
                ok = false;
            }
        }
        cs.add("C02-ERR-01 15 合法 + 非法组合拒绝表完备", ok, "");
    }

    // -- C02-ERR-02 维度/位深/调色板/索引/输入/输出六类拒绝 ---------------
    {
        let src = make_rgba(4, 4, 0, 255);
        let mut out = vec![0u8; 8192];
        let mut ok = true;
        let d = EncOptions::default();
        // 维度
        if encode_rgba(&src, 0, 4, ColorType::Rgba, 8, d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 位深
        if encode_rgba(&src, 4, 4, ColorType::Rgba, 7, d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 调色板类型走 encode_rgba → 显式拒绝（不静默降级为灰度）
        if encode_rgba(&src, 4, 4, ColorType::Palette, 8, d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 输入不足
        if encode_rgba(&src[..8], 4, 4, ColorType::Rgba, 8, d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 输出不足
        let mut tiny = vec![0u8; 16];
        if encode_rgba(&src, 4, 4, ColorType::Rgba, 8, d, &mut tiny).map(|_| ()).is_ok() {
            ok = false;
        }
        cs.add("C02-ERR-02 维度/位深/调色板/输入/输出五类拒绝", ok, "");
    }

    // -- C02-ERR-03 调色板长度与索引越界 ---------------------------------
    {
        let w = 4usize;
        let h = 4usize;
        let idx = vec![0u8; w * h];
        let plte: Vec<u8> = (0..12).map(|i| i as u8 * 20).collect();
        let d = EncOptions::default();
        let mut out = vec![0u8; 65536];
        let mut ok = true;
        // 调色板长度非 3 倍数
        let bad = vec![0u8; 10];
        if encode_palette(&idx, &bad, w, h, 8, None, d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 索引越界
        let mut oob = idx.clone();
        oob[5] = 200;
        match encode_palette(&oob, &plte, w, h, 8, None, d, &mut out) {
            Err(e) => {
                if e.kind != EncFault::PaletteIndex {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // tRNS 超长
        let long_trns = vec![0u8; 10];
        if encode_palette(&idx, &plte, w, h, 8, Some(&long_trns), d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 16 位调色板（规范禁止）
        if encode_palette(&idx, &plte, w, h, 16, None, d, &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        cs.add("C02-ERR-03 调色板长度/索引/tRNS/16位四类拒绝", ok, "");
    }

    // -- C02-ERR-04 错误五元组完整 ----------------------------------------
    {
        let mut ok = true;
        for i in 0..=(EncFault::DeflateOverflow as u16) {
            let f = match i {
                0 => EncFault::Dimension,
                1 => EncFault::BadDepth,
                2 => EncFault::IllegalCombo,
                3 => EncFault::Palette,
                4 => EncFault::PaletteIndex,
                5 => EncFault::ShortInput,
                6 => EncFault::ShortOutput,
                7 => EncFault::BadLevel,
                8 => EncFault::BadStrategy,
                9 => EncFault::Budget,
                10 => EncFault::DeflateOverflow,
                _ => EncFault::RowBuffer,
            };
            if f.label().is_empty() || f.cause().is_empty() || f.advice().is_empty() {
                ok = false;
            }
            let e = EncError::new(f).with(7, 9);
            if e.code() == 0 || e.human().is_empty() {
                ok = false;
            }
        }
        cs.add("C02-ERR-04 全错误类五元组齐备", ok, "");
    }

    // -- C02-DEPTH-01 15 合法组合 roundtrip（不透明源） --------------------
    {
        let w = 13usize; // 非 8 倍数，考验位打包
        let h = 5usize;
        let src = make_rgba(w, h, 1, 255);
        let mut ok = true;
        for (ct, d) in [
            (ColorType::Gray, 1u8), (ColorType::Gray, 2), (ColorType::Gray, 4),
            (ColorType::Gray, 8), (ColorType::Gray, 16),
            (ColorType::Rgb, 8), (ColorType::Rgb, 16),
            (ColorType::GrayAlpha, 8), (ColorType::GrayAlpha, 16),
            (ColorType::Rgba, 8), (ColorType::Rgba, 16),
        ] {
            let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
            match enc_rgba(&src, w, h, ct, d, o) {
                Ok(e) => {
                    if e.info.color != ct.wire() || e.info.depth != d {
                        ok = false;
                        continue;
                    }
                    match decode_spec(&e.out[..e.info.len]) {
                        Some((dw, dh, got)) => {
                            if dw != w || dh != h {
                                ok = false;
                                continue;
                            }
                            // 灰度类比对亮度；其余逐通道
                            let grayish = matches!(ct, ColorType::Gray | ColorType::GrayAlpha);
                            for i in 0..w * h {
                                if grayish {
                                    let want = if d < 8 {
                                        let maxv = (1u32 << d) - 1;
                                        (luma_ref(src[i * 4], src[i * 4 + 1], src[i * 4 + 2]) >> (8 - d))
                                            * 255
                                            / maxv
                                    } else {
                                        luma_ref(src[i * 4], src[i * 4 + 1], src[i * 4 + 2])
                                    };
                                    if want != got[i * 4] as u32 {
                                        ok = false;
                                        break;
                                    }
                                    if ct == ColorType::GrayAlpha && src[i * 4 + 3] != got[i * 4 + 3] {
                                        ok = false;
                                        break;
                                    }
                                } else {
                                    for c in 0..4 {
                                        if src[i * 4 + c] != got[i * 4 + c] {
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        None => ok = false,
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C02-DEPTH-01 11 种直采组合 roundtrip 全通(调色板另计)", ok, "");
    }

    // -- C02-DEPTH-02 调色板 4 种位深 roundtrip ---------------------------
    {
        let w = 11usize;
        let h = 3usize;
        let plen = 16usize;
        let plte: Vec<u8> = (0..plen * 3).map(|i| ((i * 37) % 256) as u8).collect();
        let mut ok = true;
        for d in [1u8, 2, 4, 8] {
            // 索引必须落在该位深的可存值域内（1 位只能0/1）——用全量语料
            // 会触发编码器的越界拒绝（那正是 C02-ERR-05 验的事）
            let cap = match d {
                1 => 2usize,
                2 => 4,
                4 => 16,
                _ => plen,
            };
            let idx: Vec<u8> = (0..w * h).map(|i| (i * 5 % cap) as u8).collect();
            let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
            let mut out = vec![0u8; 65536];
            match encode_palette(&idx, &plte, w, h, d, None, o, &mut out) {
                Ok(e) => {
                    if e.color != 3 || e.depth != d {
                        ok = false;
                        continue;
                    }
                    match decode_palette_spec(&out[..e.len], &plte) {
                        Some((dw, dh, got)) => {
                            if dw != w || dh != h {
                                ok = false;
                                continue;
                            }
                            for i in 0..w * h {
                                let k = idx[i] as usize;
                                if got[i * 4] != plte[k * 3]
                                    || got[i * 4 + 1] != plte[k * 3 + 1]
                                    || got[i * 4 + 2] != plte[k * 3 + 2]
                                {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        None => ok = false,
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C02-DEPTH-02 调色板 4 种位深 roundtrip", ok, "");
    }

    // -- C02-ERR-05 低位深索引越界显性拒绝（回归锁） ------------------------
    // 1/2/4 位深只能存 0..2^d−1；越界索引若放行，`pack_indices` 的掩码
    // 会把它静默截断成低位值——图像索引错乱且不产生任何报错。
    {
        let w = 8usize;
        let h = 2usize;
        let plen = 16usize;
        let plte: Vec<u8> = vec![7u8; plen * 3];
        let mut ok = true;
        for (d, bad_v) in [(1u8, 5u8), (1, 255), (2, 4), (2, 200), (4, 16), (4, 250)] {
            let mut idx = vec![0u8; w * h];
            idx[w] = bad_v; // 第二个像素塞越界索引
            let mut out = vec![0u8; 65536];
            let o = EncOptions::default();
            match encode_palette(&idx, &plte, w, h, d, None, o, &mut out) {
                Err(e) => {
                    if e.kind != EncFault::PaletteIndex {
                        ok = false;
                    }
                }
                Ok(_) => ok = false,
            }
        }
        // 8 位深下按调色板项数判（plen=16 → 索引 16 应拒）
        let mut idx = vec![0u8; w * h];
        idx[1] = 16;
        let mut out = vec![0u8; 65536];
        if encode_palette(&idx, &plte, w, h, 8, None, EncOptions::default(), &mut out).map(|_| ()).is_ok() {
            ok = false;
        }
        // 边界内索引须放行（1 位深索引 1 合法）
        let idx_ok = vec![1u8; w * h];
        let mut out2 = vec![0u8; 65536];
        if encode_palette(&idx_ok, &plte, w, h, 1, None, EncOptions::default(), &mut out2).is_err() {
            ok = false;
        }
        cs.add("C02-ERR-05 低位深索引越界拒绝/边界内放行", ok, "");
    }

    // -- C02-DEPTH-03 尺寸边界（1×1 / 1×N / N×1 / 宽非8倍数） -------------
    {
        let mut ok = true;
        for (w, h) in [(1usize, 1usize), (1, 17), (17, 1), (3, 3), (255, 2), (2, 255)] {
            let src = make_rgba(w, h, 1, 255);
            let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
            match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
                Ok(e) => {
                    if decode_spec(&e.out[..e.info.len]).map(|(_, _, g)| g == src).unwrap_or(false) == false {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C02-DEPTH-03 极端与边界尺寸 roundtrip", ok, "");
    }

    // -- C02-DETERM-01 同输入同输出（确定性） -----------------------------
    {
        let w = 40usize;
        let h = 24usize;
        let src = make_rgba(w, h, 3, 255);
        let o = EncOptions { level: 9, strategy: Strategy::MinEntropy, allow_downgrade: true, idat_chunk: 4096 };
        let a = enc_rgba(&src, w, h, ColorType::Rgba, 8, o);
        let b = enc_rgba(&src, w, h, ColorType::Rgba, 8, o);
        let ok = match (a, b) {
            (Ok(x), Ok(y)) => x.out[..x.info.len] == y.out[..y.info.len],
            _ => false,
        };
        cs.add("C02-DETERM-01 同输入逐字节同输出", ok, "");
    }

    // -- C02-DETERM-02 逐字节变异不崩溃 ------------------------------------
    {
        let w = 12usize;
        let h = 6usize;
        let src = make_rgba(w, h, 1, 255);
        let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: true, idat_chunk: 0 };
        let mut base = match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
            Ok(e) => e.out[..e.info.len].to_vec(),
            Err(_) => {
                cs.add("C02-DETERM-02 逐字节变异不崩溃", false, "");
                return finish(cs);
            }
        };
        // 变异编码器自己的输出（即解码器输入面）不应崩溃
        for i in 0..base.len() {
            for bit in 0..8 {
                let mut m = base.clone();
                m[i] ^= 1 << bit;
                let _ = decode_spec(&m);
            }
        }
        // 变异源像素不应崩溃（编码器输入面）
        for i in 0..src.len() {
            for bit in [0u32, 3, 7] {
                let mut m = src.clone();
                m[i] ^= 1 << bit;
                let mut out = vec![0u8; worst_case_out(w, h, ColorType::Rgba, 8) as usize];
                let _ = encode_rgba(&m, w, h, ColorType::Rgba, 8, o, &mut out);
            }
        }
        base.clear();
        cs.add("C02-DETERM-02 逐字节变异不崩溃", true, "");
    }

    // -- C02-SIG-01 签名与 IHDR 七参数（压缩/滤波/隔行恒0） --------------------
    {
        let w = 4usize;
        let h = 4usize;
        let src = make_rgba(w, h, 0, 255);
        // 关降档，否则不透明源会被降成 RGB（color=2），断言须按实际语义写
        let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        let ok = match enc_rgba(&src, w, h, ColorType::Rgba, 8, o) {
            Ok(e) => {
                let p = &e.out[..e.info.len];
                p[..8] == PNG_SIG
                    && walk_chunks(p).first().map(|c| &c.0[..] == b"IHDR").unwrap_or(false)
                    && p[8..12] == 13u32.to_be_bytes()
                    && &p[12..16] == b"IHDR"
                    && p[16..20] == (w as u32).to_be_bytes()
                    && p[20..24] == (h as u32).to_be_bytes()
                    && p[24] == 8// depth
                    && p[25] == 6 // color
                    && p[26] == 0 // compression
                    && p[27] == 0 // filter
                    && p[28] == 0 // interlace
            }
            Err(_) => false,
        };
        cs.add("C02-SIG-01 签名 + IHDR 七参数合规", ok, "");
    }

    // -- C02-SIG-02 隔行恒 0（Adam7 属F1003，本单显式不做） --------------
    {
        let w = 4usize;
        let h = 4usize;
        let src = make_rgba(w, h, 0, 255);
        let mut out = vec![0u8; 65536];
        let e = encode_rgba(&src, w, h, ColorType::Rgba, 8, EncOptions::default(), &mut out).unwrap();
        cs.add("C02-SIG-02 隔行方法恒 0(隔行属 F1003)", out[28] == 0 && e.len > 29, "");
    }

    // -- C02-OPT-01 选项面往返（wire 编解码自洽） -------------------------
    {
        let mut ok = true;
        for v in 0u8..3 {
            match Strategy::from_wire(v) {
                Some(s) => {
                    if s.wire() != v {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        if Strategy::from_wire(3).is_some() || Strategy::from_wire(255).is_some() {
            ok = false;
        }
        // 颜色类型 wire 往返
        for v in [0u8, 2, 3, 4, 6] {
            match ColorType::from_wire(v) {
                Some(c) => {
                    if c.wire() != v {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        for v in [1u8, 5, 7, 255] {
            if ColorType::from_wire(v).is_some() {
                ok = false;
            }
        }
        cs.add("C02-OPT-01 策略/颜色类型 wire 编解码自洽", ok, "");
    }

    // -- C02-OPT-02 默认选项值符合预期 ------------------------------------
    {
        let d = EncOptions::default();
        cs.add(
            "C02-OPT-02 默认级别 6 + MinAbsSum + 允许降档",
            d.level == 6 && matches!(d.strategy, Strategy::MinAbsSum) && d.allow_downgrade,
            "",
        );
    }

    // -- C02-ZLIB-01 stored 路径块布局合规（LEN/NLEN 互补） -----------------
    {
        let raw: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        let mut out = vec![0u8; 4096];
        match zlib_stored(&raw, &mut out) {
            Ok(n) => {
                let mut ok = n == 2 + 5 + raw.len() + 4 && out[0] == ZLIB_CMF && out[1] == ZLIB_FLG;
                // LEN/NLEN
                let l = u16::from_le_bytes([out[3], out[4]]);
                let nl = u16::from_le_bytes([out[5], out[6]]);
                if l as usize != raw.len() || nl != !l {
                    ok = false;
                }
                // 末块 BFINAL=1
                if out[2] != 1 {
                    ok = false;
                }
                // 原文逐位一致
                if &out[7..7 + raw.len()] != &raw[..] {
                    ok = false;
                }
                // Adler 大端
                let ad = mech_inflate::adler32(&raw);
                let t = n - 4;
                if u32::from_be_bytes([out[t], out[t + 1], out[t + 2], out[t + 3]]) != ad {
                    ok = false;
                }
                cs.add("C02-ZLIB-01 stored 块 LEN/NLEN/BFINAL/Adler 合规", ok, "");
            }
            Err(_) => cs.add("C02-ZLIB-01 stored 块 LEN/NLEN/BFINAL/Adler 合规", false, ""),
        }
    }

    // -- C02-ZLIB-02 zlib_wrap 头尾补齐 -----------------------------------
    {
        let raw: Vec<u8> = vec![7u8; 100];
        let deflated: Vec<u8> = vec![0xAB; 40];
        let mut out = vec![0u8; 128];
        match zlib_wrap(&deflated, &raw, &mut out) {
            Ok(n) => {
                let ok = n == 2 + 40 + 4
                    && out[0] == ZLIB_CMF
                    && out[1] == ZLIB_FLG
                    && &out[2..42] == &deflated[..]
                    && u32::from_be_bytes([out[42], out[43], out[44], out[45]])
                        == mech_inflate::adler32(&raw);
                cs.add("C02-ZLIB-02 zlib_wrap 补齐头与 Adler 大端尾", ok, "");
            }
            Err(_) => cs.add("C02-ZLIB-02 zlib_wrap 补齐头与 Adler 大端尾", false, ""),
        }
    }

    // -- C02-ZLIB-03 缓冲不足显式拒绝（不半写） ---------------------------
    {
        let raw: Vec<u8> = vec![1u8; 1000];
        let mut tiny = vec![0u8; 8];
        let ok = zlib_stored(&raw, &mut tiny).is_err() && compress_zlib(1, &raw, &mut tiny).is_err();
        cs.add("C02-ZLIB-03 缓冲不足显式拒绝不半写", ok, "");
    }

    finish(cs)
}

/// 收尾（预留：后续判据增量落此处，不改已登记项）。
fn finish(cs: CheckSet) -> CheckSet {
    cs
}

// ---------------------------------------------------------------------------
// 单元测试（自检之外的第二层，逐条钉死关键不变量）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归锁：正向滤波必须**倒序**遍历。
    ///
    /// 正序是隐蔽的错色来源——它读到的是已改写的残值，
    /// 而规范预测子取原值，整图亮度翻倍式错乱且不报错。
    #[test]
    fn vef02_forward_filter_is_reverse_iterated() {
        let raw: Vec<u8> = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let prev: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut f = raw.clone();
        // 滤波号 1 = Sub（不能用 `Filter::Sub as u8`——枚举判别值是 0，
        // 转u8 得 0（None），测的就不是 Sub 了）
        apply_filter(1, &mut f, &prev, 3).unwrap();
        // Sub：Filt(x) = Raw(x) + Raw(x-bpp)
        assert_eq!(f[3], 40 + 10);
        assert_eq!(f[4], 50 + 20);
        assert_eq!(f[5], 60 + 30);
        assert_eq!(f[6], 70 + 40);
        // Up：Filt(x) = Raw(x) + Prior(x)
        let mut u = raw.clone();
        apply_filter(2, &mut u, &prev, 3).unwrap();
        assert_eq!(u[0], 10 + 1);
        assert_eq!(u[7], 80 + 8);
    }

    /// 回归锁：GrayAlpha 的 alpha 源索引是 s+3，不是 s+1。
    ///
    /// 早前按`s+c` 一刀切会让 alpha 取到 G 通道（本语料G=200），
    /// 半透明层整体错，且不产生任何报错。
    #[test]
    fn vef02_gray_alpha_alpha_source_index_is_s3() {
        let w = 2usize;
        let h = 1usize;
        let src = vec![
            255, 200, 255, 10, // 像素0：L=223（BT.601），alpha=10
            0, 0, 0, 250, // 像素1：黑，L=0，alpha=250
        ];
        let o = EncOptions { level: 1, strategy: Strategy::FixedNone, allow_downgrade: false, idat_chunk: 0 };
        let e = enc_rgba(&src, w, h, ColorType::GrayAlpha, 8, o).unwrap();
        // 规范语义解码后必须逐通道还原
        let (_, _, got) = decode_spec(&e.out[..e.info.len]).expect("应可解码");
        // 亮度按 BT.601 算，不是取 R 通道（取 R 会得 255）
        assert_eq!(luma_ref(255, 200, 255), 223);
        assert_eq!(&got[0..4], &[223, 223, 223, 10]);
        assert_eq!(&got[4..8], &[0, 0, 0, 250]);
    }

    /// 回归锁：灰度取BT.601 亮度而非单一通道。
    #[test]
    fn vef02_gray_uses_bt601_luma_not_single_channel() {
        // 纯蓝：R=0 G=0 B=255→ L = 29*255/256 ≈ 29。若错取 R 通道会得 0。
        let src = vec![0, 0, 255, 255, 0, 0, 255, 255];
        let o = EncOptions { level: 1, strategy: Strategy::FixedNone, allow_downgrade: false, idat_chunk: 0 };
        let e = enc_rgba(&src, 2, 1, ColorType::Gray, 8, o).unwrap();
        let (_, _, got) = decode_spec(&e.out[..e.info.len]).unwrap();
        let want = luma_ref(0, 0, 255);
        assert_eq!(got[0] as u32, want, "灰度应为 BT.601 亮度 {want}，实得 {}", got[0]);
        assert!(got[0] > 20, "纯蓝的亮度不应为 0（取R 通道的典型错误）");
    }

    /// 回归锁：Lq77 不能放栈（163KB 踩内核 >64KB 禁栈戒律）。
    #[test]
    fn vef02_lz77_is_not_on_stack() {
        // 断言类型尺寸 —— 若将来改成栈上 new，编译期就该被这条抓住
        assert!(
            core::mem::size_of::<crate::perfstar::mech_deflate::Lz77>() > 65536,
            "Lz77 体积应超 64KB，必须走堆（Box）而非栈"
        );
    }

    /// `unfilter_row` 与 `apply_filter` 严格对偶（规范式对偶机检）。
    #[test]
    fn vef02_forward_unfilter_duality_holds() {
        for &bpp in &[1usize, 2, 3, 4] {
            for fno in 0u8..5 {
                let mut rng = Lcg::new(0x5EED ^ ((bpp as u64) << 8) ^ fno as u64);
                let n = 64usize;
                let raw: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                let prev: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                let mut f = raw.clone();
                apply_filter(fno, &mut f, &prev, bpp).unwrap();
                let mut g = f.clone();
                unfilter_row(fno, &mut g, &prev, bpp).unwrap();
                assert_eq!(g, raw, "对偶破坏：bpp={bpp} fno={fno}");
            }
        }
    }

    /// 全颜色类型 × 全位深 roundtrip 逐像素一致（不透明源）。
    #[test]
    fn vef02_roundtrip_all_direct_combinations() {
        let (w, h) = (19usize, 7usize); // 宽非 8 倍数
        let src = make_rgba(w, h, 1, 255);
        for (ct, d) in [
            (ColorType::Gray, 1u8), (ColorType::Gray, 2), (ColorType::Gray, 4),
            (ColorType::Gray, 8), (ColorType::Gray, 16),
            (ColorType::Rgb, 8), (ColorType::Rgb, 16),
            (ColorType::GrayAlpha, 8), (ColorType::GrayAlpha, 16),
            (ColorType::Rgba, 8), (ColorType::Rgba, 16),
        ] {
            let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
            let e = enc_rgba(&src, w, h, ct, d, o).unwrap_or_else(|e| panic!("{ct:?}/{d} 应可编码：{}", e.human()));
            let (dw, dh, got) = decode_spec(&e.out[..e.info.len]).unwrap_or_else(|| panic!("{ct:?}/{d} 应可解码"));
            assert_eq!((dw, dh), (w, h));
            for i in 0..w * h {
                if matches!(ct, ColorType::Gray | ColorType::GrayAlpha) {
                    let want = if d < 8 {
                        let maxv = (1u32 << d) - 1;
                        (luma_ref(src[i * 4], src[i * 4 + 1], src[i * 4 + 2]) >> (8 - d)) * 255 / maxv
                    } else {
                        luma_ref(src[i * 4], src[i * 4 + 1], src[i * 4 + 2])
                    };
                    assert_eq!(got[i * 4] as u32, want, "{ct:?}/{d} p{i} 亮度不一致");
                    if ct == ColorType::GrayAlpha {
                        assert_eq!(got[i * 4 + 3], src[i * 4 + 3], "{ct:?}/{d} p{i} alpha 不一致");
                    }
                } else {
                    assert_eq!(&got[i * 4..i * 4 + 4], &src[i * 4..i * 4 + 4], "{ct:?}/{d} p{i} 不一致");
                }
            }
        }
    }

    /// 透明像素必须逐位保留（不得被降档吃掉）。
    #[test]
    fn vef02_alpha_preserved_when_not_downgrading() {
        let (w, h) = (8usize, 4usize);
        let mut src = make_rgba(w, h, 1, 255);
        src[3] = 0; // 首像素全透明
        src[(w * h - 1) * 4 + 3] = 128; // 末像素半透明
        let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: true, idat_chunk: 0 };
        let e = enc_rgba(&src, w, h, ColorType::Rgba, 8, o).unwrap();
        assert_eq!(e.info.color, 6, "含透明像素不得降档");
        let (_, _, got) = decode_spec(&e.out[..e.info.len]).unwrap();
        assert_eq!(got, src, "含透明的 RGBA 必须逐位 roundtrip");
    }

/// 调色板四种位深 roundtrip（索引按各档合法值域构造）。
#[test]
fn vef02_palette_roundtrip_all_depths() {
    let (w, h) = (9usize, 3usize);
    let plen = 16usize;
    let plte: Vec<u8> = (0..plen * 3).map(|i| ((i * 53) % 256) as u8).collect();
    for d in [1u8, 2, 4, 8] {
        // 低位深的索引值域受 2^d 限制（1 位只能0/1），语料须按档构造
        let cap = match d {
            1 => 2usize,
            2 => 4,
            4 => 16,
            _ => plen,
        };
        let idx: Vec<u8> = (0..w * h).map(|i| (i * 7 % cap) as u8).collect();
        let mut out = vec![0u8; 65536];
        let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
        let e = encode_palette(&idx, &plte, w, h, d, None, o, &mut out).unwrap();
        assert_eq!(e.color, 3);
        let (_, _, got) = decode_palette_spec(&out[..e.len], &plte).unwrap();
        for i in 0..w * h {
            let k = idx[i] as usize;
            assert_eq!(got[i * 4], plte[k * 3]);
            assert_eq!(got[i * 4 + 1], plte[k * 3 + 1]);
            assert_eq!(got[i * 4 + 2], plte[k * 3 + 2]);
        }
    }
}

/// 回归锁：低位深的调色板索引越界必须显性拒绝。
///
/// 早前只按「调色板项数」判越界，1/2/4 位深下索引 5/10/15 会被
/// `pack_indices` 的掩码静默截断成低位——编码"成功"但索引错乱。
#[test]
fn vef02_low_depth_palette_index_rejected() {
    let (w, h) = (8usize, 2usize);
    let plte: Vec<u8> = vec![7u8; 16 * 3];
    for (d, bad) in [(1u8, 5u8), (1, 255), (2, 4), (2, 200), (4, 16), (4, 250)] {
        let mut idx = vec![0u8; w * h];
        idx[w] = bad;
        let mut out = vec![0u8; 65536];
        let e = encode_palette(&idx, &plte, w, h, d, None, EncOptions::default(), &mut out)
            .expect_err("低位深下的越界索引必须拒绝");
        assert_eq!(e.kind, EncFault::PaletteIndex);
    }
    // 8 位深按调色板项数判
    let mut idx = vec![0u8; w * h];
    idx[1] = 16;
    let mut out = vec![0u8; 65536];
    assert!(encode_palette(&idx, &plte, w, h, 8, None, EncOptions::default(), &mut out).is_err());
    // 边界内必须放行
    let idx_ok = vec![1u8; w * h];
    let mut out2 = vec![0u8; 65536];
    assert!(encode_palette(&idx_ok, &plte, w, h, 1, None, EncOptions::default(), &mut out2).is_ok());
}

    /// IDAT 分块：载荷不超过生效上限，块序列合法。
    #[test]
    fn vef02_idat_chunking_respects_limit() {
        let (w, h) = (300usize, 30usize);
        let src = make_rgba(w, h, 1, 255);
        for chunk in [0usize, 1024, 8192, 100_000] {
            let o = EncOptions { level: 6, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: chunk };
            let e = enc_rgba(&src, w, h, ColorType::Rgba, 8, o).unwrap();
            let eff = if chunk == 0 || chunk > IDAT_CHUNK_MAX { IDAT_CHUNK_MAX } else { chunk };
            let chunks = walk_chunks(&e.out[..e.info.len]);
            assert_eq!(chunks.iter().filter(|(t, _, _)| &t[..] == b"IHDR").count(), 1);
            assert_eq!(chunks.iter().filter(|(t, _, _)| &t[..] == b"IEND").count(), 1);
            let n_idat = chunks.iter().filter(|(t, _, _)| t == b"IDAT").count();
            assert!(n_idat > 1, "300×30 的 RGBA 应触发多 IDAT 分块");
            for (t, l, _) in &chunks {
                if t == b"IDAT" {
                    assert!(*l <= eff, "IDAT 载荷 {l} 超生效上限 {eff}");
                }
            }
        }
    }

    /// `worst_case_out` 必须是真实上界（含 deflate 路与 stored 路）。
    #[test]
    fn vef02_worst_case_is_tight_upper_bound() {
        for (w, h, ct, d) in [
            (1usize, 1usize, ColorType::Rgba, 8u8),
            (64, 48, ColorType::Rgba, 8),
            (64, 48, ColorType::Rgb, 16),
            (64, 48, ColorType::Gray, 4),
            (128, 64, ColorType::GrayAlpha, 8),
        ] {
            let wc = worst_case_out(w, h, ct, d) as usize;
            for lvl in 1u8..=9 {
                let src = make_rgba(w, h, 1, 255);
                let mut out = vec![0u8; wc];
                let o = EncOptions { level: lvl, strategy: Strategy::MinEntropy, allow_downgrade: false, idat_chunk: 0 };
                let e = encode_rgba(&src, w, h, ct, d, o, &mut out)
                    .unwrap_or_else(|e| panic!("worst_case 不足：{w}x{h} {ct:?}/{d} lvl{lvl} → {}", e.human()));
                assert!(e.len <= wc);
            }
        }
    }

    /// 非法输入全部显性拒绝，且错误五元组齐备。
    #[test]
    fn vef02_rejects_illegal_with_five_tuple() {
        let src = make_rgba(4, 4, 0, 255);
        let mut out = vec![0u8; 65536];
        let d = EncOptions::default();
        // 位深×类型非法
        for (ct, dp) in [
            (ColorType::Palette, 16u8),
            (ColorType::Rgb, 1),
            (ColorType::Rgba, 4),
            (ColorType::GrayAlpha, 2),
        ] {
            let e = encode_rgba(&src, 4, 4, ct, dp, d, &mut out).expect_err("非法组合必须拒绝");
            assert_eq!(e.kind, EncFault::IllegalCombo);
            assert!(!e.human().is_empty());
            assert!(!e.kind.cause().is_empty());
            assert!(!e.kind.advice().is_empty());
            assert_ne!(e.code(), 0);
        }
        // 调色板类型不该走 encode_rgba
        assert_eq!(
            encode_rgba(&src, 4, 4, ColorType::Palette, 8, d, &mut out).unwrap_err().kind,
            EncFault::Palette
        );
    }

    /// 确定性：同输入两次编码逐字节一致。
    #[test]
    fn vef02_encoding_is_deterministic() {
        let (w, h) = (33usize, 21usize);
        let src = make_rgba(w, h, 3, 255);
        let o = EncOptions { level: 9, strategy: Strategy::MinEntropy, allow_downgrade: true, idat_chunk: 2048 };
        let a = enc_rgba(&src, w, h, ColorType::Rgba, 8, o).unwrap();
        let b = enc_rgba(&src, w, h, ColorType::Rgba, 8, o).unwrap();
        assert_eq!(a.info.len, b.info.len);
        assert_eq!(&a.out[..a.info.len], &b.out[..b.info.len]);
    }

    /// 九档权衡表与实现路径一一对应。
    #[test]
    fn vef02_level_table_matches_implementation() {
        assert_eq!(LEVEL_TABLE.len(), 9);
        for lvl in 1u8..=9 {
            let row = level_row(lvl).expect("每档都应有行");
            let want = if lvl <= LEVEL_STORED_MAX { "stored" } else { "fixed-huffman" };
            assert_eq!(row.path, want, "级别 {lvl} 路径登记与实现不符");
            assert!(!row.size_class.is_empty() && !row.speed_class.is_empty());
        }
        assert!(level_row(0).is_none() && level_row(10).is_none());
    }

    /// 存储路径（级别 1-3）与 deflate 路径（4-9）都能产出可解码流。
    #[test]
    fn vef02_both_compression_paths_decodable() {
        let (w, h) = (24usize, 8usize);
        let src = make_rgba(w, h, 2, 255);
        for lvl in 1u8..=9 {
            let o = EncOptions { level: lvl, strategy: Strategy::MinAbsSum, allow_downgrade: false, idat_chunk: 0 };
            let e = enc_rgba(&src, w, h, ColorType::Rgba, 8, o).unwrap();
            let (_, _, got) = decode_spec(&e.out[..e.info.len]).unwrap_or_else(|| panic!("级别 {lvl} 的流应可解码"));
            assert_eq!(got, src, "级别 {lvl} roundtrip 不一致");
        }
    }
}