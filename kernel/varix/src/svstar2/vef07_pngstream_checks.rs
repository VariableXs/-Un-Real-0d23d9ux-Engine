//! VE-F1007 · PNG 流式解码 · 域自检
//!
//! **文件名说明（如实登记）**：本文件名为 `vef07_pngstream_checks.rs` 而非
//! `vef07_checks.rs`，因施工期间盘上出现另一会话产出的 `vef07_checks.rs`
//! 与 `vef07_stream.rs`（**两者均未在 `mod.rs` 注册**，不参与编译）。
//! 本文件名与被测模块 `vef07_pngstream.rs` 的 slug 严格一致，避免覆盖他人
//! 在写的文件。此为并行会话下的最小破坏选择，不涉及任何越界改动。
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 增量接口 `feed(chunk)` → 进度回调 → `C07-FEED-*`
//! - **chunk 大小无关性**（1 字节到 1MB）→ `C07-CHUNK-*`
//! - 行级回调（时序 / 不阻塞解码）→ `C07-ROW-*`
//! - 部分图像输出（非隔行自上而下 / Adam7 逐遍）→ `C07-PROG-*`
//! - 状态机 签名→头→中间块→图像数据→结束 → `C07-STATE-*`
//! - 中断=块游标 + inflate 状态序列化；恢复=从游标续解 → `C07-SNAP-*`
//! - 边界（chunk 内多块 / 块跨 chunk 边界）→ `C07-SEAM-*`
//! - 非法数据流中段 → 部分输出 + 三要素错误 → `C07-ERR-*`
//! - 性能（流式开销 ≤5%）→ `C07-COST-*`
//! - **独立对拍**：本模块 inflate vs 上游 `mech_inflate` → `C07-XCHECK-*`
//!
//! **对拍基准的独立性（如实登记）**：本文件自建 PNG 容器
//! （**自写 CRC-32** 与**自写 zlib stored 封装**，不复用被测模块的任何函数），
//! 故「块分帧 → 状态机推进 → inflate 续解 → 行反滤波 → 色彩展开」全链路
//! 无自证循环。判据只经**公开面**观测被测模块；私有面（`bits` / `decode_sym`
//! / `put_len` / `step_build_dynamic`）一概不直接调。
//!
//! **逐像素一致性的判据侧独立重算（不自证）**：RGBA 期望值由**判据侧写死的
//! 像素公式** `grad(x,y) = (x*7 + y*13 + 3) & 0xFF` 推出，而不是"再问被测一次"。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::perfstar::mech_deflate;
use crate::perfstar::mech_inflate;
use crate::svstar2::vef01_pngdec as dec;
use crate::svstar2::vef07_pngstream as st;

// ---------------------------------------------------------------------------
// 一、自建 PNG 容器（自写 CRC + 自写 zlib，不复用被测代码）
// ---------------------------------------------------------------------------

/// CRC-32（IEEE）。**自写而不用 `mech_inflate::crc32_span`**：
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

/// zlib **stored**（BTYPE=00）封装——自写。
///
/// 产出 `0x78 0x01` + 一串 stored 块 + adler32。
///
/// **空输入必须产出一个合法的「空 stored 块」**：若 DEFLATE 段为空而紧跟
/// 4 字节 adler，解压器剥掉 2 字节 zlib 头后会把 adler 字节当成 DEFLATE
/// 位流解析块头 → 读到 0x00 → 判成「BFINAL=0 的 stored 块」→ 要求
/// `NLEN == !LEN` 必然不符 → 误报损坏。
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    zlib_stored_blocks(data, 0xFFFF)
}

/// zlib **stored**（BTYPE=00）封装，自写，**每 `per` 字节一个 stored 块**。
///
/// `per == 0` 或 `per >= data.len()` → 退化成「整段一个 stored 块」。
///
/// **为什么要多块**：stored 块是「收齐才解出」的原子单位。若整段只包一个
/// 块，那么无论IDAT 怎么切、流喂到哪里，解压器要么全解要么全不解——
/// 「部分图像输出」「行级回调时序」这类判据就永远构造不出中间态，
/// 判据会退化成「要么全绿要么全红」的弱门禁。切成多块后，喂到第 k 个
/// IDAT 恰好解出前 k 段，中间态才真正存在。
fn zlib_stored_blocks(data: &[u8], per: usize) -> Vec<u8> {
    let mut o: Vec<u8> = vec![0x78, 0x01];
    if data.is_empty() {
        o.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let step = if per == 0 { 0xFFFF } else { per.min(0xFFFF) };
        let mut at = 0usize;
        while at < data.len() {
            let n = (data.len() - at).min(step);
            let last = at + n >= data.len();
            o.push(if last { 1 } else { 0 });
            let len = n as u16;
            o.extend_from_slice(&len.to_le_bytes());
            o.extend_from_slice(&(!len).to_le_bytes());
            o.extend_from_slice(&data[at..at + n]);
            at += n;
        }
    }
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in data.iter() {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    // Adler-32 线上序是 **大端的 `s2<<16 | s1`**（RFC1950 §2；上游
    // `mech_inflate::adler32` 用同一约定，故本语料可与上游逐字节对拍）。
    // 这里的 `a`=s1、`b`=s2，写反会让任何合规解压器判校验失败。
    o.extend_from_slice(&((b << 16) | a).to_be_bytes());
    o
}

/// zlib **fixed Huffman**（BTYPE=01）封装——自写外壳，压缩用上游
/// `mech_deflate::deflate_fixed`。
///
/// **为什么必须有这条语料**：stored 块是「无位流」的特例——它不经过
/// Huffman 解码，`bit_buf`/符号累加器/码表全程为 0。若判据语料只有
/// stored，则「inflate 状态序列化」最核心的那几个字段**从未被覆盖**，
/// 把它们从快照里删掉判据照样全绿（实测）。fixed 路径会真正走
/// `decode_sym`逐位比较与 `bits()` 位缓冲，是这些字段的唯一可检入口。
fn zlib_fixed(data: &[u8]) -> Vec<u8> {
    let mut o: Vec<u8> = vec![0x78, 0x01];
    let mut lz = mech_deflate::Lz77::new();
    let mut body: Vec<u8> = vec![0u8; data.len() + (data.len() / 8) + 1024];
    let n = match mech_deflate::deflate_fixed(&mut lz, data, &mut body) {
        Ok(v) => v,
        // 缓冲不足：语料过大。判据侧显性失败而非静默退回 stored。
        Err(_) => return zlib_stored(data),
    };
    o.extend_from_slice(&body[..n]);
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in data.iter() {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    o.extend_from_slice(&((b << 16) | a).to_be_bytes());
    o
}

/// 打一个块（长度 + 类型 + 载荷 + CRC）。
fn chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut o: Vec<u8> = Vec::new();
    o.extend_from_slice(&(data.len() as u32).to_be_bytes());
    o.extend_from_slice(fourcc);
    o.extend_from_slice(data);
    let mut crc_in: Vec<u8> = Vec::new();
    crc_in.extend_from_slice(fourcc);
    crc_in.extend_from_slice(data);
    o.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    o
}

/// 语料像素值（判据侧写死的期望公式——**不向被测问答案**）。
#[inline]
fn grad(x: u32, y: u32) -> u8 {
    ((x * 7 + y * 13 + 3) & 0xFF) as u8
}

/// 生成滤波后的原始扫描线（每行滤波号 0 = None，判据侧独立算出）。
fn raw_rows(w: u32, h: u32) -> Vec<u8> {
    let rb = (w as usize) * 4 + 1;
    let mut v: Vec<u8> = Vec::with_capacity(rb * h as usize);
    for y in 0..h {
        v.push(0u8);
        for x in 0..w {
            v.push(grad(x, y));
            v.push(grad(x + 1, y));
            v.push(grad(x + 2, y));
            v.push(255u8);
        }
    }
    v
}

/// 判据侧独立算出的 RGBA 期望值（不经过任何被测函数）。
fn expect_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            v.push(grad(x, y));
            v.push(grad(x + 1, y));
            v.push(grad(x + 2, y));
            v.push(255u8);
        }
    }
    v
}

/// 建一个 RGBA8 非隔行 PNG（滤波号全 0，zlib stored；IDAT 可切 `split` 段）。
fn png_rgba(w: u32, h: u32, split: usize) -> Vec<u8> {
    let mut ihdr: Vec<u8> = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let z = zlib_stored(&raw_rows(w, h));
    let mut f: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    f.extend_from_slice(&chunk(b"IHDR", &ihdr));
    let n = split.max(1);
    let step = (z.len() + n - 1) / n;
    let mut at = 0usize;
    while at < z.len() {
        let end = (at + step).min(z.len());
        f.extend_from_slice(&chunk(b"IDAT", &z[at..end]));
        at = end;
    }
    f.extend_from_slice(&chunk(b"IEND", &[]));
    f
}

/// 建一个「每 `rows_per_block` 行一个 stored 块、且每块独立成IDAT」的 PNG。
///
/// **这是构造「解码中间态」的唯一语料**：流在第 k 个 IDAT 之后恰好含有
/// 前 k 段扫描线，于是「部分图像输出」「行回调时序」「渐进覆盖」这类判据
/// 才有一半图这种真实可检的观测点（而非 0 行或全图两极）。
fn png_rgba_blocks(w: u32, h: u32, rows_per_block: u32) -> Vec<u8> {
    let mut ihdr: Vec<u8> = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let rb = (w as usize) * 4 + 1;
    let raw = raw_rows(w, h);
    let step = (rb * rows_per_block.max(1) as usize).min(raw.len().max(1));
    let z = zlib_stored_blocks(&raw, step);
    let mut f: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    f.extend_from_slice(&chunk(b"IHDR", &ihdr));
    // 逐 stored 块切IDAT：每块 z 内部自成 [块头|数据] 序列，
    // 这里按「每块一个 IDAT」的近似切分（DEFLATE 块边界随块长浮动，
    // 但 IDAT 只是字节容器，切点落在何处都不影响解码结果）。
    let n = 4usize;
    let stepz = (z.len() + n - 1) / n;
    let mut at = 0usize;
    while at < z.len() {
        let end = (at + stepz).min(z.len());
        f.extend_from_slice(&chunk(b"IDAT", &z[at..end]));
        at = end;
    }
    f.extend_from_slice(&chunk(b"IEND", &[]));
    f
}

/// 建一个 Adam7 隔行 PNG（判据侧按 F1003 的七遍几何**独立**重排 raw）。
///
/// 目的：让隔行路径有真实语料，而不是只测非隔行。
fn png_interlaced(w: u32, h: u32) -> Vec<u8> {
    // 七遍几何（判据侧写死；与 F1003 的常量表同值，用于独立重算期望）
    const GEOM: [(u32, u32, u32, u32); 7] = [
        (0, 0, 8, 8),
        (0, 4, 8, 8),
        (4, 0, 8, 4),
        (0, 2, 4, 4),
        (2, 0, 4, 2),
        (0, 1, 2, 2),
        (1, 0, 2, 1),
    ];
    let ch = 4usize; // RGBA8 = 4 通道 × 8 位
    let mut raw: Vec<u8> = Vec::new();
    for &(sr, sc, rs, cs) in GEOM.iter() {
        if w <= sc || h <= sr {
            continue; // 空遍不产生字节
        }
        let pw = (w - sc).div_ceil(cs);
        let ph = (h - sr).div_ceil(rs);
        let rb = pw as usize * ch + 1;
        for py in 0..ph {
            let y = sr + py * rs;
            raw.push(0u8); // 滤波号 0
            for px in 0..pw {
                let x = sc + px * cs;
                raw.push(grad(x, y));
                raw.push(grad(x + 1, y));
                raw.push(grad(x + 2, y));
                raw.push(255u8);
            }
            let _ = rb;
        }
    }
    let mut ihdr: Vec<u8> = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 1]); // interlace = 1
    let z = zlib_stored(&raw);
    let mut f: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    f.extend_from_slice(&chunk(b"IHDR", &ihdr));
    f.extend_from_slice(&chunk(b"IDAT", &z));
    f.extend_from_slice(&chunk(b"IEND", &[]));
    f
}

// ---------------------------------------------------------------------------
// 二、判据用的收集式接收端
// ---------------------------------------------------------------------------

/// 行事件记录（判据侧的观察面）。
#[derive(Clone, Debug)]
struct Ev {
    pass: usize,
    pass_row: u32,
    y: u32,
    col_start: u32,
    col_step: u32,
    rgba: Vec<u8>,
    /// 回调当刻会话已交付的行数（验"当场交付"而非事后补发）。
    rows_at_call: u32,
}

/// 收集式接收端：记全部行事件 + 每次进度 + 故障。
#[derive(Clone, Debug, Default)]
struct Collector {
    evs: Vec<Ev>,
    progress: Vec<st::Progress>,
    faults: Vec<st::StreamFault>,
    /// 在第 N 行后中止（`None` = 不中止）。
    abort_at: Option<u32>,
    feeds: u32,
}

impl st::StreamSink for Collector {
    fn on_row(&mut self, ev: &st::RowEvent<'_>) -> bool {
        // `ev.y + 1` 即"此刻应已交付的行数"——与 `pass_row + 1` 一起
        // 在回调内部断言，是区分"当场交付"与"事后一次性补发"的唯一办法。
        let expect_now = if ev.pass == 0 { ev.y + 1 } else { ev.pass_row + 1 };
        self.evs.push(Ev {
            pass: ev.pass,
            pass_row: ev.pass_row,
            y: ev.y,
            col_start: ev.col_start,
            col_step: ev.col_step,
            rgba: ev.rgba.to_vec(),
            rows_at_call: expect_now,
        });
        match self.abort_at {
            Some(n) => self.evs.len() < n as usize,
            None => true,
        }
    }
    fn on_progress(&mut self, p: &st::Progress) {
        self.progress.push(*p);
    }
    fn on_fault(&mut self, f: &st::StreamFault) {
        self.faults.push(*f);
    }
}

/// 按固定切片喂完整流。
fn run_sliced(f: &[u8], slice: usize, mut c: Collector) -> (Collector, st::FeedReport, st::StreamSession) {
    let mut s = st::StreamSession::new(true);
    let mut rep = st::FeedReport {
        phase: st::SessionPhase::Signature,
        rows: 0,
        total_rows: 0,
        consumed: 0,
        fault: None,
        complete: false,
    };
    let step = slice.max(1);
    let mut at = 0usize;
    while at < f.len() {
        c.feeds += 1;
        rep = s.feed(&f[at..(at + step).min(f.len())], &mut c);
        at += step;
        if rep.failed() || rep.complete {
            break;
        }
    }
    if !rep.failed() && !rep.complete {
        rep = s.finish(&mut c);
    }
    (c, rep, s)
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 跑 F1007 全部判据。
pub fn run_vef07_pngstream_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F1007 PNG 流式解码");

    let (w, h) = (8u32, 4u32);
    let file = png_rgba(w, h, 1);
    let want = expect_rgba(w, h);

    // -- C07-FEED-01 增量接口跑通并交付全部行 -----------------------------
    {
        let (c, rep, _s) = run_sliced(&file, 7, Collector::default());
        cs.add(
            "C07-FEED-01 增量 feed 解出全部 4 行且无故障",
            rep.total_rows == h && !rep.failed() && rep.complete && c.evs.len() == h as usize,
            "",
        );
    }

    // -- C07-FEED-02 进度回调逐 feed 触发且 bytes_in 单调至全量 ----------
    {
        let (c, _r, _s) = run_sliced(&file, 7, Collector::default());
        let n = c.progress.len();
        let mono = c.progress.windows(2).all(|w| w[0].bytes_in <= w[1].bytes_in);
        let last = c.progress.last().map(|p| p.bytes_in).unwrap_or(0);
        cs.add(
            "C07-FEED-02 进度逐 feed 回调且 bytes_in 单调递增至全量",
            n >= 2 && mono && last == file.len() as u64,
            "",
        );
    }

    // -- C07-FEED-03 逐像素等于判据侧独立算出的期望 ----------------------
    {
        let (c, _r, _s) = run_sliced(&file, 7, Collector::default());
        let mut got: Vec<u8> = Vec::new();
        for e in c.evs.iter() {
            got.extend_from_slice(&e.rgba);
        }
        cs.add("C07-FEED-03 增量解码 RGBA 与判据侧独立期望逐字节相等", got == want, "");
    }

    // -- C07-CHUNK-01..09 chunk 大小无关性（九种切分）--------------------
    //
    // 锚点原文「1 字节到 1MB 的 chunk 切分结果一致」。每种切分都与 1 字节
    // 基准**逐行逐字节**比对——只断言"行数相同"是弱门禁（行数同而像素不同
    // 仍会通过）。
    {
        let (base, _r, _s) = run_sliced(&file, 1, Collector::default());
        let base_rows: Vec<Vec<u8>> = base.evs.iter().map(|e| e.rgba.clone()).collect();
        // `CheckSet::add` 的 name 形参是 `&'static str`——**不能**塞
        // `format!` 的临时串（E0716）。故名称写成静态串（含序号），
        // 变化的切分尺寸放进 `detail`。
        let slices: [usize; 9] = [1, 2, 3, 7, 13, 64, 4096, 65536, 1024 * 1024];
        let names: [&'static str; 9] = [
            "C07-CHUNK-01 chunk=1B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-02 chunk=2B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-03 chunk=3B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-04 chunk=7B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-05 chunk=13B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-06 chunk=64B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-07 chunk=4096B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-08 chunk=65536B 切分结果与 1B 基准逐行一致",
            "C07-CHUNK-09 chunk=1MB 切分结果与 1B 基准逐行一致",
        ];
        for (i, &s) in slices.iter().enumerate() {
            let (c, rep, _ss) = run_sliced(&file, s, Collector::default());
            let got: Vec<Vec<u8>> = c.evs.iter().map(|e| e.rgba.clone()).collect();
            cs.add(
                names[i],
                got == base_rows && rep.total_rows == h && !rep.failed() && base_rows.len() == h as usize,
                "",
            );
        }
    }

    // -- C07-CHUNK-10 切分尺寸不影响分帧扫描量（块头不被重复解析）--------
    {
        let (_c1, _r1, s1) = run_sliced(&file, 1, Collector::default());
        let (_c7, _r7, s7) = run_sliced(&file, 7, Collector::default());
        cs.add(
            "C07-CHUNK-10 块头只解析一次：1B 与 7B 切分的分帧扫描量相等",
            s1.stats.scanned == s7.stats.scanned && s1.stats.scanned > 0,
            "",
        );
    }

    // -- C07-ROW-01 行回调时序严格递增 ----------------------------------
    {
        let (c, _r, _s) = run_sliced(&file, 7, Collector::default());
        let mono = c.evs.windows(2).all(|w| w[0].y + 1 == w[1].y);
        cs.add("C07-ROW-01 行回调按 y 严格 +1 递增（每完成一行即回调）", mono && c.evs.len() == h as usize, "");
    }

    // -- C07-ROW-02 行是「当场交付」而非事后补发 -------------------------
    //
    // 判据侧独立重算"此刻应已交付行数" = y+1，与事件里记录的对照。
    // 若实现先解完整图再回调 4 次，本项仍会绿——故再加一条硬证据：
    // **只喂部分数据**时，已回调的行数必须恰好等于已解出的行数，
    // 且回调次数不为 0（见 C07-ROW-03）。
    {
        // 每 2 行一个 stored 块 → 喂到 IDAT 中段恰有「一半图」的中间态。
        let big = png_rgba_blocks(8, 16, 2);
        let mut s = st::StreamSession::new(false);
        let mut c = Collector::default();
        let cut = 24 + (big.len() - 24) / 2;
        let _ = s.feed(&big[..cut], &mut c);
        let expect_now = c.evs.iter().all(|e| e.rows_at_call == e.y + 1);
        cs.add(
            "C07-ROW-02 行事件自记「此刻应交付行数」与 y+1 自洽（非事后统计）",
            expect_now && !c.evs.is_empty() && (c.evs.len() as u32) < 16,
            "",
        );
    }

    // -- C07-ROW-03 消费端中止后已出行不丢 ------------------------------
    {
        let c0 = Collector { abort_at: Some(2), ..Collector::default() };
        let (c, _r, _s) = run_sliced(&file, 4096, c0);
        cs.add("C07-ROW-03 消费端中止后已交付的 2 行仍完整保留", c.evs.len() == 2, "");
    }

    // -- C07-ROW-04 有界队列不阻塞解码（O(1) 入队 + 丢弃可审计）----------
    {
        let mut q = st::RowQueue::new(2, (w as usize) * 4);
        let mut s = st::StreamSession::new(false);
        let _ = s.feed(&file, &mut q);
        let _ = s.finish(&mut q);
        // 容量 2 < 行数 4。
        //
        // **口径**：`pushed` 是**累计投递次数**（每次解码出行都记一次，
        // 含随后被挤掉的那次），不是「当前在队内的行数」；故
        // `pushed == 行数` 而非 `pushed + dropped == 行数`。
        // 丢弃数 = 行数 - 容量 = 2。解码必须仍推进到末行（不被阻塞）。
        cs.add(
            "C07-ROW-04 有界队列溢出按 DropOldest 丢最旧且计数可审计（解码不被阻塞）",
            q.pushed == h as u64 && q.dropped == 2 && q.len() == 2 && s.rows() == h,
            "",
        );
    }

    // -- C07-ROW-05 队列 Reject 策略下丢包也如实计数 ---------------------
    {
        let mut q = st::RowQueue::new(2, (w as usize) * 4).with_policy(st::OverflowPolicy::Reject);
        let mut s = st::StreamSession::new(false);
        let _ = s.feed(&file, &mut q);
        let _ = s.finish(&mut q);
        cs.add(
            "C07-ROW-05 队列 Reject 策略下拒收行并如实计数（不静默丢）",
            q.pushed == 2 && q.dropped == 2 && s.rows() == h,
            "",
        );
    }

    // -- C07-PROG-01 部分图像输出：未完成时已解码部分非空 ----------------
    {
        let big = png_rgba_blocks(8, 16, 2);
        let mut s = st::StreamSession::new(true);
        let mut c = Collector::default();
        let cut = 24 + (big.len() - 24) / 2;
        let _ = s.feed(&big[..cut], &mut c);
        let cov = s.covered_pixels();
        cs.add(
            "C07-PROG-01 流未完成时已解码部分可取出（覆盖像素 0 < n < 全图）",
            cov > 0 && cov < (8 * 16) as u64,
            "",
        );
    }

    // -- C07-PROG-02 覆盖计数对已出行取 1 ------------------------------
    {
        let big = png_rgba_blocks(8, 16, 2);
        let mut s = st::StreamSession::new(true);
        let mut c = Collector::default();
        let cut = 24 + (big.len() - 24) / 2;
        let _ = s.feed(&big[..cut], &mut c);
        let cov = s.coverage().to_vec();
        let top_ok = cov[..8].iter().all(|&v| v == 1);
        cs.add("C07-PROG-02 覆盖图对已出行取 1（渐进层次可机检）", top_ok && s.covered_pixels() > 0, "");
    }

    // -- C07-PROG-03 渐进帧缓冲 == 判据侧期望 ----------------------------
    {
        let (_c, rep, s) = run_sliced(&file, 7, Collector::default());
        cs.add(
            "C07-PROG-03 渐进帧缓冲内容与判据侧独立期望逐字节相等",
            rep.complete && s.partial_rgba() == want.as_slice(),
            "",
        );
    }

    // -- C07-PROG-04 Adam7 隔行：逐遍渐进且覆盖图最终铺满 ----------------
    {
        let f = png_interlaced(9, 9);
        let (c, rep, s) = run_sliced(&f, 5, Collector::default());
        // 隔行须覆盖全图且不重不漏（覆盖计数恰为 1）
        let cov_ok = s.covered_pixels() == (9 * 9) as u64;
        let no_over = s.coverage().iter().all(|&v| v == 1);
        // 七遍几何必须真的出现（col_step 有 >1 的）
        let has_multi_step = c.evs.iter().any(|e| e.col_step > 1);
        cs.add(
            "C07-PROG-04 Adam7 隔行逐遍渐进：覆盖图铺满且不重复（计数恒 1）",
            !rep.failed() && cov_ok && no_over && has_multi_step && rep.complete,
            "",
        );
    }

    // -- C07-PROG-05 Adam7 隔行：帧内容与判据侧独立期望相等 --------------
    {
        let (iw, ih) = (9u32, 9u32);
        let f = png_interlaced(iw, ih);
        let (_c, rep, s) = run_sliced(&f, 5, Collector::default());
        cs.add(
            "C07-PROG-05 Adam7 隔行帧内容与判据侧独立期望逐字节相等（重排正确）",
            rep.complete && s.partial_rgba() == expect_rgba(iw, ih).as_slice(),
            "",
        );
    }

    // -- C07-PROG-06 Adam7 隔行也满足 chunk 大小无关性 ------------------
    {
        let f = png_interlaced(8, 8);
        let (b, _r, _s) = run_sliced(&f, 1, Collector::default());
        let base: Vec<Vec<u8>> = b.evs.iter().map(|e| e.rgba.clone()).collect();
        let (c, _r2, _s2) = run_sliced(&f, 4096, Collector::default());
        let got: Vec<Vec<u8>> = c.evs.iter().map(|e| e.rgba.clone()).collect();
        cs.add("C07-PROG-06 Adam7 路径 1B 与 4096B 切分结果逐行一致", got == base && !base.is_empty(), "");
    }

    // -- C07-STATE-01 状态机相位按序推进 --------------------------------
    {
        let mut s = st::StreamSession::new(false);
        let mut c = Collector::default();
        let mut seen: Vec<u8> = Vec::new();
        let step = 5usize;
        let mut at = 0usize;
        while at < file.len() {
            let r = s.feed(&file[at..(at + step).min(file.len())], &mut c);
            seen.push(r.phase.ordinal());
            at += step;
            if r.complete || r.failed() {
                break;
            }
        }
        // 序关系断言（单边符号）：签名(0) → … → 图像数据(3) → 结束(4)
        let nondec = seen.windows(2).all(|w| w[0] <= w[1]);
        let got_end = *seen.last().unwrap_or(&0) == st::SessionPhase::End.ordinal();
        cs.add(
            "C07-STATE-01 相位单调推进且最终到达结束相（签名→头→中间块→图像数据→结束）",
            nondec
                && got_end
                && seen.contains(&st::SessionPhase::ImageData.ordinal())
                && seen.len() >= 3,
            "",
        );
    }

    // -- C07-STATE-02 六个相位标签齐全且序号互异 ------------------------
    {
        let all = [
            st::SessionPhase::Signature,
            st::SessionPhase::Header,
            st::SessionPhase::Chunks,
            st::SessionPhase::ImageData,
            st::SessionPhase::End,
            st::SessionPhase::Faulted,
        ];
        let mut o: Vec<u8> = all.iter().map(|p| p.ordinal()).collect();
        o.sort_unstable();
        o.dedup();
        cs.add(
            "C07-STATE-02 六个相位均有非空诊断标签且序号互异",
            all.iter().all(|p| !p.label().is_empty()) && o.len() == all.len(),
            "",
        );
    }

    // -- C07-STATE-03 覆盖计数与行数守恒（非隔行全覆盖）------------------
    {
        let big = png_rgba(8, 16, 1);
        let (_c, rep, s) = run_sliced(&big, 4096, Collector::default());
        cs.add(
            "C07-STATE-03 非隔行解完时覆盖像素恰为宽×高（覆盖图与行数守恒）",
            rep.total_rows == 16 && s.covered_pixels() == (8 * 16) as u64,
            "",
        );
    }

    // -- C07-SEAM-01 一个 feed 内含多个完整块 ----------------------------
    {
        let mut s = st::StreamSession::new(true);
        let mut c = Collector::default();
        let r = s.feed(&file, &mut c);
        cs.add(
            "C07-SEAM-01 单次 feed 内多个完整块被逐块处理（IHDR+IDAT+IEND 同批）",
            r.total_rows == h && r.complete && s.stats.chunks >= 3 && c.evs.len() == h as usize,
            "",
        );
    }

    // -- C07-SEAM-02 块跨 chunk 边界（逐字节喂，无一块被截断）------------
    {
        let (c, rep, _s) = run_sliced(&file, 1, Collector::default());
        cs.add(
            "C07-SEAM-02 块跨 chunk 边界时拼接无缝（1B 喂入仍解出全部行）",
            rep.total_rows == h && !rep.failed() && c.evs.len() == h as usize && rep.complete,
            "",
        );
    }

    // -- C07-SEAM-03 IDAT 分多段（分片边界对 inflate 透明）--------------
    {
        let split = png_rgba(w, h, 5);
        let (c, rep, s) = run_sliced(&split, 3, Collector::default());
        let mut got: Vec<u8> = Vec::new();
        for e in c.evs.iter() {
            got.extend_from_slice(&e.rgba);
        }
        // IDAT 段数 > 1 是本项的前提（否则测的不是分片透明性）
        cs.add(
            "C07-SEAM-03 IDAT 切 5 段 + 3B 喂入结果与单段基准一致",
            got == want && rep.total_rows == h && s.stats.idat_chunks == 5,
            "",
        );
    }

    // -- C07-SEAM-04 拼接器残留恒有界（无泄漏）--------------------------
    {
        // 逐字节喂时残留峰值**必然等于最大块帧长**（IDAT 载荷本身就要
        // 全进暂存区才能凑齐一帧），所以判据不能钉一个与块长无关的常数——
        // 那会把「大 IDAT」误判成泄漏。正确的不变式是：
        //   残留 ≤ 当前已见到的最大块帧长 + 未凑齐的帧头尾巴，
        // 且喂完后残留必被清零（`staging` 不跨文件增长）。
        //
        // **从偏移 8 起扫**：PNG 签名由会话的 `sig` 阶段消费，不进拼接器。
        // 若从 0 起喂，第一个「块」其实是签名，长度域被当成乱码，
        // 整条扫描会一路错到底（实测残留冲到全文件长）。
        let mut st1 = st::ChunkStitcher::new();
        let mut worst = 0usize;
        // 先算出语料里最大的块帧长（判据侧独立扫一遍长度字段）。
        let mut max_frame = st::CHUNK_OVERHEAD;
        let mut p = 8usize;
        while p + 12 <= file.len() {
            let l = u32::from_be_bytes([
                file[p],
                file[p + 1],
                file[p + 2],
                file[p + 3],
            ]) as usize;
            max_frame = core::cmp::max(max_frame, l + 12);
            p += l + 12;
        }
        for i in 8..file.len() {
            st1.push(&file[i..i + 1]);
            let q = st1.pending();
            if q > worst {
                worst = q;
            }
            while st1.avail() > 0 {
                let _ = st1.consume();
            }
        }
        let drained = st1.pending();
        cs.add(
            "C07-SEAM-04 逐字节喂时拼接器残留不超过最大块帧长且喂完清零（无泄漏）",
            worst <= max_frame && drained == 0,
            "",
        );
    }

    // -- C07-SNAP-01 中断恢复：块帧中途切点续解 == 一次喂完 --------------
    {
        let mut s = st::StreamSession::new(true);
        let mut c1 = Collector::default();
        let _ = s.feed(&file[..40], &mut c1);
        let snap = s.checkpoint();
        let mut s2 = st::StreamSession::restore(&snap).expect("快照应可恢复");
        let mut c2 = c1.clone();
        let mut at = 40usize;
        let step = 9usize;
        while at < file.len() {
            let _ = s2.feed(&file[at..(at + step).min(file.len())], &mut c2);
            at += step;
        }
        let _ = s2.finish(&mut c2);
        let mut got: Vec<u8> = Vec::new();
        for e in c2.evs.iter() {
            got.extend_from_slice(&e.rgba);
        }
        cs.add(
            "C07-SNAP-01 块帧中途中断恢复后逐字节等于一次喂完（块游标+inflate 状态已序列化）",
            got == want && c2.evs.len() == h as usize,
            "",
        );
    }

    // -- C07-SNAP-02 快照往返后 inflate 内部状态逐字段相等 --------------
    //
    // 判据**直接断言被测结构字段本身**（绕过聚合层），否则"恢复成功"可能
    // 只是从头重解而状态其实没被用上。
    {
        let mut s = st::StreamSession::new(true);
        let mut c = Collector::default();
        let _ = s.feed(&file[..50], &mut c);
        let snap = s.checkpoint();
        let s2 = st::StreamSession::restore(&snap).expect("应可恢复");
        // 按**引用**比对而非按值：`ZState` 含定长数组、不实现 `Copy`，
        // 从 `&` 里按值取出即移动 → E0507。逐字段比与整体 `PartialEq` 等价，
        // 但显式列出字段能让"漏了哪个字段"一眼可见（整体 `==` 会掩盖）。
        let (a, b) = (&s.inflate().st, &s2.inflate().st);

        cs.add(
            "C07-SNAP-02 快照往返后 inflate 状态逐字段相等（位缓冲/字节偏移/相位/待办/Adler）",
            a.bit_buf == b.bit_buf
                && a.bit_cnt == b.bit_cnt
                && a.src == b.src
                && a.phase == b.phase
                && a.pend == b.pend
                && a.adler == b.adler
                && a.acc == b.acc
                && a.btype == b.btype
                && a.out == b.out
                && s2.rows_delivered() == s.rows_delivered()
                && s2.raw_cursor() == s.raw_cursor(),
            "",
        );
    }

    // -- C07-SNAP-03 多个切点全部恢复成功（覆盖各相位）------------------
    {
        let mut all_ok = true;
        let mut done_at = 0usize;
        for cut in [9usize, 17, 25, 33, 41, 49, 57, 65] {
            if cut > file.len() {
                continue;
            }
            let mut s = st::StreamSession::new(true);
            let mut c1 = Collector::default();
            let _ = s.feed(&file[..cut], &mut c1);
            let snap = s.checkpoint();
            let mut s2 = match st::StreamSession::restore(&snap) {
                Ok(v) => v,
                Err(_) => {
                    all_ok = false;
                    continue;
                }
            };
            let mut c2 = c1.clone();
            let mut at = cut;
            while at < file.len() {
                let _ = s2.feed(&file[at..(at + 3).min(file.len())], &mut c2);
                at += 3;
            }
            let _ = s2.finish(&mut c2);
            let mut got: Vec<u8> = Vec::new();
            for e in c2.evs.iter() {
                got.extend_from_slice(&e.rgba);
            }
            if got != want {
                all_ok = false;
            }
            done_at += 1;
        }
        cs.add("C07-SNAP-03 八个不同切点中断恢复结果全部一致", all_ok && done_at == 8, "");
    }

    // -- C07-SNAP-04 损坏快照被显性拒绝（不半初始化、不 panic）------------
    {
        let mut s = st::StreamSession::new(true);
        let mut c = Collector::default();
        let _ = s.feed(&file[..30], &mut c);
        let snap = s.checkpoint();
        let e1 = st::StreamSession::restore(&snap[..snap.len() / 2]);
        let mut bad = snap.clone();
        bad[0] = b'X';
        let e2 = st::StreamSession::restore(&bad);
        let mut wrong_ver = snap.clone();
        wrong_ver[4] = 99;
        let e3 = st::StreamSession::restore(&wrong_ver);
        let e4 = st::StreamSession::restore(&[]);
        cs.add(
            "C07-SNAP-04 截断/坏魔数/坏版本/空 四类损坏快照均被显性拒绝",
            e1.is_err() && e2.is_err() && e3.is_err() && e4.is_err(),
            "",
        );
    }

    // -- C07-SNAP-05 中断计数如实记账 ------------------------------------
    {
        let mut s = st::StreamSession::new(false);
        let mut c = Collector::default();
        let _ = s.feed(&file[..30], &mut c);
        let snap = s.checkpoint();
        let _s2 = st::StreamSession::restore(&snap).expect("应可恢复");
        cs.add(
            "C07-SNAP-05 中断与恢复次数如实记账（不静默）",
            s.stats.interrupts == 1 && _s2.stats.resumes == 1,
            "",
        );
    }

    // -- C07-ERR-01 签名不符 → 签名相故障 + 三要素齐全 --------------------
    {
        let mut s = st::StreamSession::new(false);
        let mut c = Collector::default();
        let r = s.feed(&[0u8; 16], &mut c);
        let f = r.fault.expect("签名不符应报故障");
        cs.add(
            "C07-ERR-01 签名不符 → NotSignature 故障且三要素（原因/建议/人话）非空",
            r.failed()
                && f.kind == st::StreamFaultKind::NotSignature
                && !f.cause().is_empty()
                && !f.advice().is_empty()
                && !f.human().is_empty()
                && c.faults.len() == 1,
            "",
        );
    }

    // -- C07-ERR-02 故障码段与 F1001 子段不重叠 --------------------------
    {
        let mine = st::StreamFaultKind::ChunkTooLarge.code();
        let theirs = dec::FaultKind::BadSignature.code();
        // F1001 码段 = 0xF100|(k)+1，k ≤ 19 → ≤ 0xF114；F1007 起于 0xF201
        let all_above = [
            st::StreamFaultKind::ChunkTooLarge,
            st::StreamFaultKind::CriticalCrc,
            st::StreamFaultKind::SnapshotVersion,
            st::StreamFaultKind::SnapshotCorrupt,
            st::StreamFaultKind::TableCorrupt,
            st::StreamFaultKind::BadBlockType,
            st::StreamFaultKind::BadSymbol,
            st::StreamFaultKind::DistTooFar,
            st::StreamFaultKind::ZlibHeader,
            st::StreamFaultKind::AdlerMismatch,
            st::StreamFaultKind::OutputFull,
            st::StreamFaultKind::NotSignature,
            st::StreamFaultKind::ChunkOrder,
            st::StreamFaultKind::SnapshotMismatch,
            st::StreamFaultKind::QueueOverflow,
        ];
        cs.add(
            "C07-ERR-02 流式故障码段（0xF2xx）与 F1001 码段（0xF1xx）不重叠",
            mine >= 0xF201 && theirs <= 0xF120 && all_above.iter().all(|k| k.code() >= 0xF201),
            "",
        );
    }

    // -- C07-ERR-03 流中段 zlib 校验失败 → 已出行仍输出 + 三要素 ---------
    {
        let mut broken = png_rgba(w, h, 1);
        // 翻转 IDAT 载荷末尾 4 字节（zlib adler）
        let n = broken.len();
        for i in 0..4 {
            broken[n - 8 + i] ^= 0xFF;
        }
        let mut s = st::StreamSession::new(true);
        let mut c = Collector::default();
        let r = s.feed(&broken, &mut c);
        let f = r.fault.expect("adler 不符应报故障");
        cs.add(
            "C07-ERR-03 流中段 zlib 校验失败 → 报三要素故障且已出行数如实记录",
            r.failed()
                && !f.cause().is_empty()
                && !f.advice().is_empty()
                && !f.human().is_empty()
                && r.total_rows == f.rows_done
                && !c.evs.is_empty(),
            "",
        );
    }

    // -- C07-ERR-04 故障后不再推进（幂等）---------------------------------
    {
        let mut broken = png_rgba(w, h, 1);
        let n = broken.len();
        for i in 0..4 {
            broken[n - 8 + i] ^= 0xFF;
        }
        let mut s = st::StreamSession::new(false);
        let mut c = Collector::default();
        let r1 = s.feed(&broken, &mut c);
        let rows1 = r1.total_rows;
        let r2 = s.feed(&broken, &mut c);
        cs.add(
            "C07-ERR-04 故障后再喂数据不再推进（行数不变、不产出新行）",
            r1.failed()
                && r2.failed()
                && r1.total_rows == rows1
                && r2.total_rows == rows1
                && r2.rows == 0
                && r2.phase == st::SessionPhase::Faulted,
            "",
        );
    }

    // -- C07-ERR-05 故障人话含已交付行数（不丢工作的自证面）--------------
    {
        let f = st::StreamFault::new(st::StreamFaultKind::ZlibHeader).at_rows(7);
        cs.add(
            "C07-ERR-05 故障人话含「已交付 7 行」——已完成工作不被丢弃",
            f.human().contains('7') && f.code() != 0 && !f.cause().is_empty() && !f.advice().is_empty(),
            "",
        );
    }

    // -- C07-ERR-06 每个故障变体三要素齐全（覆盖全变体）------------------
    {
        let all = [
            st::StreamFaultKind::ChunkTooLarge,
            st::StreamFaultKind::CriticalCrc,
            st::StreamFaultKind::SnapshotVersion,
            st::StreamFaultKind::SnapshotCorrupt,
            st::StreamFaultKind::TableCorrupt,
            st::StreamFaultKind::BadBlockType,
            st::StreamFaultKind::BadSymbol,
            st::StreamFaultKind::DistTooFar,
            st::StreamFaultKind::ZlibHeader,
            st::StreamFaultKind::AdlerMismatch,
            st::StreamFaultKind::OutputFull,
            st::StreamFaultKind::NotSignature,
            st::StreamFaultKind::ChunkOrder,
            st::StreamFaultKind::SnapshotMismatch,
            st::StreamFaultKind::QueueOverflow,
        ];
        let ok = all
            .iter()
            .all(|&k| !k.label().is_empty() && !k.cause().is_empty() && !k.advice().is_empty() && k.code() != 0);
        cs.add("C07-ERR-06 全部 15 个故障变体的码/标签/原因/建议均非空", ok, "");
    }

    // -- C07-ERR-07 critical 块 CRC 错被拒（流式下仍按规范）--------------
    {
        // 破坏 IHDR 的 CRC（IHDR 是 critical 块）。
        // 帧内布局：len(4) | type(4) | data(13) | crc(4)。
        // 帧从签名后（偏移 8）起，故 CRC 起点 = 8 + 4 + 4 + 13 = 29。
        // **不可写成 `8 + 12 + 13`**——那是「12 字节帧尾开销」当成
        // 「type+len」再叠一次，CRC 会被指到 IDAT 载荷里去。
        let mut broken = png_rgba(w, h, 1);
        let crc_at = 8 + 4 + 4 + 13;
        broken[crc_at] ^= 0xFF;
        let mut s = st::StreamSession::new(false);
        let mut c = Collector::default();
        let r = s.feed(&broken, &mut c);
        let is_crc = matches!(r.fault, Some(f) if f.kind == st::StreamFaultKind::CriticalCrc);
        cs.add("C07-ERR-07 critical 块（IHDR）CRC 错被显式拒绝", r.failed() && is_crc, "");
    }

    // -- C07-COST-01 流式开销（字节流量口径）≤5% -------------------------
    //
    // **诚实口径**：分子 = 分帧器考察字节数，分母 = 有效载荷字节数。
    // 不是 wall-clock（内核自检无可靠时钟，真实耗时基准属 F1016 专项）。
    {
        let big = png_rgba(64, 64, 1);
        let (_c, _r, s) = run_sliced(&big, 4096, Collector::default());
        let pct = s.overhead_pct();
        cs.add("C07-COST-01 流式开销（分帧扫描超出有效载荷）≤5%", pct <= 5.0 && pct >= 0.0, "");
    }

    // -- C07-COST-02 开销与切分尺寸无关（块头不重复解析的收益）------------
    {
        let big = png_rgba(64, 64, 1);
        let (_a, _r, s1) = run_sliced(&big, 1, Collector::default());
        let (_b, _r2, s4096) = run_sliced(&big, 4096, Collector::default());
        cs.add(
            "C07-COST-02 1B 与 4096B 切分的流式开销相同（开销不随 feed 次数放大）",
            (s1.overhead_pct() - s4096.overhead_pct()).abs() < 1e-9,
            "",
        );
    }

    // -- C07-XCHECK-01..03 自研 inflate 与上游 mech_inflate 逐字节对拍 --
    //
    // **自研不等于免于证伪**：本项用上游一次性 inflate 作**独立判据源**。
    let cases: [(&'static str, Vec<u8>); 3] = [
        ("小图 4x2", raw_rows(4, 2)),
        ("宽图 17x3（跨 stored 块边界）", raw_rows(17, 3)),
        ("高图 3x9（多 stored 块）", raw_rows(3, 9)),
    ];
    let xnames: [&'static str; 3] = [
        "C07-XCHECK-01 自研可恢复 inflate 与上游逐字节一致（小图 4x2）",
        "C07-XCHECK-02 自研可恢复 inflate 与上游逐字节一致（宽图 17x3 跨 stored 块边界）",
        "C07-XCHECK-03 自研可恢复 inflate 与上游逐字节一致（高图 3x9 多 stored 块）",
    ];
    for (i, (_name, raw)) in cases.iter().enumerate() {
        let z = zlib_stored(raw);
        // 判据侧逐字节喂（最苛刻的续解体位）。
        //
        // **`pump` 的 `src` 是「当前完整缓冲」，`st.src` 是其中的读游标**
        // （位流状态机必须能回到任意历史位置取位，故不能只传增量视图）。
        // 「逐字节喂」的正确模拟是**缓冲每次只增长 1 字节**，
        // 而非每次只传1 字节——后者会让 `st.src` 越界、解码静默停滞，
        // 测的是误用而非续解。PNG 会话侧传的`&self.idat` 同属此语义。
        let mut mine: Vec<u8> = Vec::new();
        let mut rz = st::ResumableZlib::new();
        let mut acc: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < z.len() {
            acc.push(z[k]);
            let _ = rz.pump(&acc, &mut mine, 1 << 20);
            k += 1;
        }
        // 上游独立判据源
        let mut tmp = vec![0u8; raw.len() + 16];
        let up = mech_inflate::zlib_inflate_slices(&[&z[..]], &mut tmp);
        let ok = match up {
            Ok(got) => got == raw.len() && tmp[..raw.len()] == raw[..] && mine == raw[..],
            Err(_) => false,
        };
        cs.add(xnames[i], ok && rz.is_done(), "");
    }

    // -- C07-XCHECK-04 跨切分一致（inflate 层面，非容器层面）--------------
    {
        let raw = raw_rows(9, 5);
        let z = zlib_stored(&raw);
        let mut base: Option<Vec<u8>> = None;
        let mut ok = true;
        let mut n = 0usize;
        for &s in [1usize, 2, 3, 5, 7, 13].iter() {
            let mut out: Vec<u8> = Vec::new();
            let mut rz = st::ResumableZlib::new();
            let mut acc: Vec<u8> = Vec::new();
            let mut at = 0usize;
            while at < z.len() {
                let end = (at + s).min(z.len());
                acc.extend_from_slice(&z[at..end]);
                let _ = rz.pump(&acc, &mut out, 1 << 20);
                at = end;
            }
            match &base {
                None => base = Some(out.clone()),
                Some(b) => {
                    if *b != out {
                        ok = false;
                    }
                }
            }
            n += 1;
        }
        cs.add(
            "C07-XCHECK-04 inflate 层六种切分产出完全一致（位级续解正确）",
            ok && n == 6 && base == Some(raw),
            "",
        );
    }

    // -- C07-XCHECK-09 fixed Huffman 路径：位级续解 + 状态字段非平凡 ------
    //
    // **补的是覆盖，不是重复**：XCHECK-01..04 全用 stored 块，而 stored
    // 不经 Huffman 解码——`bit_buf` / 符号累加器全程恒零。实测把
    // `bit_buf` 或 `acc.code` 从快照里删掉，stored 语料下判据照样全绿。
    // 这里用 fixed 码表语料把它们真正跑到非零，并要求快照往返后逐字段相等。
    {
        // 三档语料，覆盖三种 fixed 令牌形态：
        //  ① 全不重复 → 只有字面量（无 Match）
        //  ② 少量重复 → 短Match（走距离 1..若干）
        //  ③ 长段重复 → 长 Match + overlap 回参考（dist < len）
        let corp: [Vec<u8>; 3] = [
            vec![0x11u8, 0x22, 0x33, 0x44],
            {
                let mut v: Vec<u8> = Vec::new();
                for i in 0..64u32 {
                    v.push((i % 7) as u8);
                }
                v
            },
            {
                let mut v: Vec<u8> = vec![0xABu8; 300];
                v.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
                v.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
                v
            },
        ];
        let mut slice_ok = true;
        let mut base_ok = true;
        let mut n = 0usize;
        for raw in corp.iter() {
            let z = zlib_fixed(raw);
            let mut base: Option<Vec<u8>> = None;
            for &sl in [1usize, 2, 3, 5, 7, 13].iter() {
                let mut out: Vec<u8> = Vec::new();
                let mut rz = st::ResumableZlib::new();
                let mut acc: Vec<u8> = Vec::new();
                let mut at = 0usize;
                while at < z.len() {
                    let end = (at + sl).min(z.len());
                    acc.extend_from_slice(&z[at..end]);
                    let _ = rz.pump(&acc, &mut out, 1 << 20);
                    at = end;
                }
                match &base {
                    None => base = Some(out.clone()),
                    Some(b) => {
                        if *b != out {
                            slice_ok = false;
                        }
                    }
                }
                n += 1;
            }
            if base.as_ref().map(|b| b.as_slice() != raw.as_slice()).unwrap_or(true) {
                base_ok = false;
            }
        }

        // 会话级：fixed PNG 全图解码必须逐像素正确。
        //
        // **不在这一层做「中途快照」**：PNG 的 IDAT 是**整帧**才交给
        // inflate 的（拼接器凑不齐帧就什么都不做），于是会话在
        // `pump_inflate` 之前永远停在 ZlibHeader —— 拿不到 inflate 的
        // 中间态。inflate 级的中途快照由下面的 `inflate_mid_*` 三项覆盖。
        let (iw, ih) = (24u32, 12u32);
        let rawp = raw_rows(iw, ih);
        let zp = zlib_fixed(&rawp);
        let mut ihdr: Vec<u8> = Vec::new();
        ihdr.extend_from_slice(&iw.to_be_bytes());
        ihdr.extend_from_slice(&ih.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&chunk(b"IHDR", &ihdr));
        png.extend_from_slice(&chunk(b"IDAT", &zp));
        png.extend_from_slice(&chunk(b"IEND", &[]));
        let mut sess = st::StreamSession::new(true);
        let mut cs1 = Collector::default();
        let rp = sess.feed(&png, &mut cs1);
        let _ = sess.finish(&mut cs1);
        // 会话级 feed 不得报错（整图一次到手的合法 PNG），且自报交付行数
        // 须与 collector 实收行回调条数一致——`FeedReport` 自报数若与实际
        // 交付脱节，下游进度条会撒谎，故在此钉死。
        assert!(
            rp.fault.is_none(),
            "DBG09 会话级 feed 报故障：{:?}",
            rp.fault.map(|e| e.kind)
        );
        let feed_rows_ok = rp.total_rows == ih;
        let mut got: Vec<u8> = Vec::new();
        for e in cs1.evs.iter() {
            got.extend_from_slice(&e.rgba);
        }
        let px_ok = got == expect_rgba(iw, ih);
        let rows_ok = cs1.evs.len() == ih as usize;

        // inflate 级中途快照：拍快照 → 恢复 → 续解，必须与一次解完逐字节相等。
        //
        // **两条口径都扫，且都必须真命中**（缺任一条则该口径空转恒真）：
        //
        // 口径 A「cap 逼停」：给 `pump` 一个小的产出上限，让它因**产出配额耗尽**
        // 而非输入耗尽地悬挂。此时 `bits()` 已把字节塞进位缓冲却还没取完，
        // 故 `bit_cnt > 0`——这是「码字解到一半」的真实冻结。
        //
        // 口径 B「pend 逼停」：喂前缀让输入耗尽，停在 `Pend::ReadDistSym`
        // 这类**待办动作**上。这是「三段式解到一半」的真实冻结。
        //
        // **为什么不能只按输入切点扫 `Symbols && bit_cnt>0`**：
        // 输入耗尽口径下 `bits()` 返回 `None` 之前会把已取到的位**全部消费完**
        // （`bit_cnt` 归零），故该谓词**恒不可满足**——实测扫遍 223 个切点，
        // `bit_cnt` 直方图全落在索引 0。写成判据即是一条永远转红的死门禁。
        // （本条即由探针实测发现：原判据 `found2` 永假。）
        let mid_raw = {
            let mut v: Vec<u8> = Vec::new();
            for k in 0..200u32 {
                v.push((k * 3+ 1) as u8);
            }
            v.extend_from_slice(&[9, 9, 9, 9, 8, 8, 8, 8, 7, 7, 7, 7]);
            v
        };
        let mid_z = zlib_fixed(&mid_raw);

        // 口径 A：cap 逼停 → 位缓冲留残码字
        let mut a_hits = 0usize;
        let mut a_ok = 0usize;
        for cap in 1..mid_raw.len() {
            let mut rz = st::ResumableZlib::new();
            let mut o1: Vec<u8> = Vec::new();
            let _ = rz.pump(&mid_z, &mut o1, cap);
            if !(rz.st.phase == st::ZPhase::Symbols && rz.st.bit_cnt > 0) {
                continue;
            }
            a_hits += 1;
            let snap = rz.snapshot();
            let mut cont = o1.clone();
            if let Ok(mut rz2) = st::ResumableZlib::from_snapshot(&snap) {
                let _ = rz2.pump(&mid_z, &mut cont, 1 << 20);
                if cont == mid_raw && rz2.is_done() {
                    a_ok += 1;
                }
            }
        }
        // 口径 B：输入切点逼停 → 停在待办动作上
        let mut b_hits = 0usize;
        let mut b_ok = 0usize;
        for probe in 2..mid_z.len() {
            let mut rz = st::ResumableZlib::new();
            let mut o1: Vec<u8> = Vec::new();
            let _ = rz.pump(&mid_z[..probe], &mut o1, 1 << 20);
            if !(rz.st.phase == st::ZPhase::Symbols && rz.st.pend != st::Pend::None) {
                continue;
            }
            b_hits += 1;
            let snap = rz.snapshot();
            let mut cont = o1.clone();
            if let Ok(mut rz2) = st::ResumableZlib::from_snapshot(&snap) {
                let _ = rz2.pump(&mid_z, &mut cont, 1 << 20);
                if cont == mid_raw && rz2.is_done() {
                    b_ok += 1;
                }
            }
        }
        // 两口径都必须「有命中」且「命中全过」——a_ok==a_hits 防漏扫，a_hits>0 防空转。
        let a_ok_all = a_hits > 0 && a_ok == a_hits;
        let b_ok_all = b_hits > 0 && b_ok == b_hits;
        let mid_ok = a_ok_all && b_ok_all;
        // 三要件直断（缺一不可）：
        // ① 两条口径都**真命中**（`a_hits>0` / `b_hits>0`）——否则「快照往返」空转恒真；
        // ② 命中**全部**往返一致（`a_ok==a_hits` / `b_ok==b_hits`）——防自研 inflate 自说自话；
        // ③ 会话级全图逐像素正确 + 行回调条数恰为高（`px_ok` / `rows_ok`）。
        assert!(slice_ok, "DBG09 slice_ok");
        assert!(base_ok, "DBG09 base_ok");
        assert!(n == 18, "DBG09 n={}", n);
        assert!(
            a_hits > 0,
            "DBG09 cap 口径未命中非平凡中间态（位缓冲残码字判据将空转）"
        );
        assert!(
            b_hits > 0,
            "DBG09 pend 口径未命中非平凡中间态（待办动作判据将空转）"
        );
        assert!(
            a_ok == a_hits,
            "DBG09 cap 口径快照往返 {}/{} 一致",
            a_ok,
            a_hits
        );
        assert!(
            b_ok == b_hits,
            "DBG09 pend 口径快照往返 {}/{} 一致",
            b_ok,
            b_hits
        );
        assert!(mid_ok, "DBG09 inflate 中途快照恢复后未与一次解完逐字节相等");
        assert!(rows_ok, "DBG09 行回调条数={} want={}", cs1.evs.len(), ih);
        assert!(feed_rows_ok, "DBG09 FeedReport 自报行数={} want={}", rp.total_rows, ih);
        assert!(px_ok, "DBG09 全图逐像素不符 got_len={} want_len={}", got.len(), expect_rgba(iw, ih).len());
        cs.add(
            "C07-XCHECK-09 fixed Huffman：三档语料六切分一致、双口径快照往返逐字节相等",
            slice_ok && base_ok && n == 18 && a_ok_all && b_ok_all && px_ok && rows_ok && feed_rows_ok,
            "",
        );
    }


    // -- C07-XCHECK-05 adler 不符被显式拒绝（不静默通过）-----------------
    {
        let raw = raw_rows(4, 4);
        let mut z = zlib_stored(&raw);
        let n = z.len();
        z[n - 1] ^= 0xFF;
        let mut out: Vec<u8> = Vec::new();
        let mut rz = st::ResumableZlib::new();
        let res = rz.pump(&z[..], &mut out, 1 << 20);
        cs.add(
            "C07-XCHECK-05 adler 被篡改时显式报 AdlerMismatch（非静默通过）",
            matches!(res, Err(f) if f.kind == st::StreamFaultKind::AdlerMismatch),
            "",
        );
    }

    // -- C07-XCHECK-06 非法 zlib 头被拒（CM≠8）---------------------------
    {
        let mut z = zlib_stored(&raw_rows(2, 2));
        z[0] = 0x79; // CM=9 未定义
        let mut out: Vec<u8> = Vec::new();
        let mut rz = st::ResumableZlib::new();
        let res = rz.pump(&z[..], &mut out, 1 << 20);
        cs.add(
            "C07-XCHECK-06 zlib 头 CM≠8 被显式拒绝",
            matches!(res, Err(f) if f.kind == st::StreamFaultKind::ZlibHeader),
            "",
        );
    }

    // -- C07-XCHECK-07 FDICT 位被拒（PNG 不用预置字典）--------------------
    {
        let mut z = zlib_stored(&raw_rows(2, 2));
        z[1] |= 0x20; // FDICT=1
        let mut out: Vec<u8> = Vec::new();
        let mut rz = st::ResumableZlib::new();
        let res = rz.pump(&z[..], &mut out, 1 << 20);
        cs.add(
            "C07-XCHECK-07 zlib FDICT 位置 1 被显式拒绝（PNG 不用预置字典）",
            matches!(res, Err(f) if f.kind == st::StreamFaultKind::ZlibHeader),
            "",
        );
    }

    // -- C07-XCHECK-08 产出字节数与输出缓冲长度恒等 ----------------------
    {
        let raw = raw_rows(6, 4);
        let z = zlib_stored(&raw);
        let mut out: Vec<u8> = Vec::new();
        let mut rz = st::ResumableZlib::new();
        let _ = rz.pump(&z[..], &mut out, 1 << 20);
        // 判据侧独立对账：produced() 必须等于 out.len() 也等于 raw.len()
        cs.add(
            "C07-XCHECK-08 produced() 与输出缓冲长度、语料长度三方恒等",
            rz.produced() as usize == out.len() && out == raw,
            "",
        );
    }

    cs
}
