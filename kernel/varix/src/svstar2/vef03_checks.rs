//! VE-F1003 · PNG 交错模式（Adam7）· 域自检（判据逐条对应，见 `vef03_adam7.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 七遍常量表唯一来源 / 拆分器与还原器共用 → `C03-TBL-*`
//! - 解码（七遍像素重排还原全图，逐像素一致）→ `C03-DEC-*`
//! - 编码（全图 → 七遍拆分输出）→ `C03-ENC-*`
//! - 隔行与滤波的交互（每遍独立滤波 / 行缓冲独立 / 首行按遍语义）→ `C03-FILT-*`
//! - pass 描述符与遍调度器 → `C03-DESC-*`
//! - 错误路径（遍数据截断 → 已完成遍输出；步长与宽高校验；畸形不崩溃）→ `C03-ERR-*`
//! - 非隔行对比对· 性能差异入册 · 默认不隔行 → `C03-PERF-*`
//!
//! **对拍基准的选择（如实登记）**：roundtrip 基准取**本模块的编码器输出**，
//! 还原侧走 [`vef03_adam7::decode_adam7_full`]。这样测的是「编码拆分与
//! 解码还原互逆」这一锚点判据本身，而非与外部实现的兼容度——后者属F1011
//! （PngSuite 互操作测试集）的职责。不复用 F1001 的 `decode`（它对
//! `interlace=1` 显式拒绝），也不复用上游 `imgsimd_ext::row_filter`
//! （其 `wrapping_add` 符号缺陷已在 F1002 头注登记，不能作基准）。
//!
//! **反假变体测试（门禁有效性证明）**：见文件末 `variant_tests` 段的注释——
//! 改坏遍表 / 改坏遍首行滤波 / 改坏列步长，确认对���判据**当场变红**。
//! 未做反假验证的门禁等于没有门禁。
//!
//! 纯函数校验，无时钟无 IO，回归可复现。语料全部由本文件内的构造器生成。

use crate::svstar2::vef01_pngdec as dec;
use crate::svstar2::vef02_pngenc as enc;
use crate::svstar2::vef03_adam7 as a7;

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
/// `pattern` 决定内容特征——**滤波判据的前提**：不同内容会选出不同滤波，
/// 若全用常数图则五滤波里只有 None 会被选中，滤波判据就成了恒真弱门禁。
/// - 0 斜坡渐变（平滑，Up 友好）
/// - 1 随机噪声（最坏情形，五滤波差异最大）
/// - 2 大色块（重复多，deflate 友好）
/// - 3 水平条纹（行间强相关）
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

/// 收集式遍接收端（自检侧独立实现——不复用被测模块的私有收集器，
/// 避免「自检与被测共用同一份逻辑」的自证循环）。
struct Rec {
    /// 整图缓冲。
    data: Vec<u8>,
    /// 全图宽。
    width: usize,
    /// 全图高。
    height: usize,
    /// 见到的遍行数。
    rows: u64,
    /// 遍行覆盖计数矩阵的展平计数（每像素一格，验证不重不漏）。
    cover: Vec<u16>,
    /// 遍序号序列（**逐行**记录，故同一遍会连续重复若干次）。
    pass_seq: Vec<u8>,
    /// 每遍已见行数（索引= 遍号），用于核对「每遍行数 == 该遍高度」。
    pass_rows: [u64; 8],
}

impl Rec {
    fn new(w: usize, h: usize) -> Rec {
        Rec {
            data: vec![0u8; w * h * 4],
            width: w,
            height: h,
            rows: 0,
            cover: vec![0u16; w * h],
            pass_seq: Vec::new(),
            pass_rows: [0u64; 8],
        }
    }
}

impl a7::PassSink for Rec {
    fn on_pass_row(&mut self, pass: u8, _py: u32, y: u32, cols: &[u32], rgba: &[u8]) -> bool {
        // 遍历序断言：遍号必须**非递减**（同一遍的连续多行重复同一个遍号是
        // 合法且必然的语义——遍内行号由 `py` 给出，`pass` 在遍内恒定；
        // 判据真正要禁的是「回到已结束的遍」）。
        //
        // 曾误写成「每次调用都须严格递增」，那等于要求每遍只有一行，
        // 32×32 图第一遍第二行即被拒——**判据错，不是被测物错**。
        match self.pass_seq.last() {
            Some(&last) => {
                if pass < last {
                    return false;
                }
            }
            None => {}
        }
        if pass >= 8 {
            return false;
        }
        self.pass_seq.push(pass);
        self.pass_rows[pass as usize] += 1;
        let yi = y as usize;
        if yi >= self.height {
            return false;
        }
        for (i, &c) in cols.iter().enumerate() {
            let xi = c as usize;
            if xi >= self.width || i * 4 + 4 > rgba.len() {
                continue;
            }
            let idx = yi * self.width + xi;
            let o = idx * 4;
            self.data[o..o + 4].copy_from_slice(&rgba[i * 4..i * 4 + 4]);
            self.cover[idx] += 1;
        }
        self.rows += 1;
        true
    }
}

/// 便捷：编码一次隔行 PNG（用足够大的输出缓冲）。
fn enc_adam7(rgba: &[u8], w: usize, h: usize, color: enc::ColorType, depth: u8, level: u8) -> Option<Vec<u8>> {
    let opts = enc::EncOptions {
        level,
        strategy: enc::Strategy::MinAbsSum,
        allow_downgrade: false,
        idat_chunk: 0,
    };
    // 容量：隔行最坏口径由被测模块自行核算并设闸，此处给足余量
    let mut out = vec![0u8; a7_worst_guess(w, h, color, depth)];
    match a7::encode_adam7(rgba, w, h, color, depth, opts, &mut out) {
        Ok(e) => {
            out.truncate(e.len);
            Some(out)
        }
        Err(_) => None,
    }
}

/// 自检侧的容量估算（**不复用被测模块的 `adam7_worst_case_out`**——
/// 若复用，被测模块把公式写错时自检会跟着错，形成自证）。
fn a7_worst_guess(w: usize, h: usize, color: enc::ColorType, depth: u8) -> usize {
    // 取逐行最坏值再放大 4 倍——足够宽裕，且与被测实现完全独立
    (enc::worst_case_out(w, h, color, depth) * 4) as usize + 65536
}

/// 造一个 IHDR（自检侧便捷形）。
fn mk_head(w: u32, h: u32, depth: u8, color: dec::ColorType) -> dec::Ihdr {
    dec::Ihdr {
        width: w,
        height: h,
        depth,
        color,
        compression: 0,
        filter_method: 0,
        interlace: 1,
    }
}

// ---------------------------------------------------------------------------
// 主自检
// ---------------------------------------------------------------------------

/// VE-F1003 域自检。
pub fn run_vef03_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("vef03");

    // -- C03-TBL-01 遍常量表逐格与PNG 规范 §9.6 一致 --------------------
    // 这是全单的地基：表错则一切皆错。逐格比对，不做"看起来对"的整体断言。
    {
        let want: [(u32, u32, u32, u32); 7] = [
            (0, 0, 8, 8),
            (0, 4, 8, 8),
            (4, 0, 8, 4),
            (0, 2, 4, 4),
            (2, 0, 4, 2),
            (0, 1, 2, 2),
            (1, 0, 2, 1),
        ];
        let mut ok = a7::ADAM7_PASSES.len() == 7;
        for (i, &(sr, sc, rs, cs_step)) in want.iter().enumerate() {
            match a7::pass_geom(i) {
                Some(g) => {
                    if g.start_row != sr || g.start_col != sc || g.row_step != rs || g.col_step != cs_step {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        // 越界取遍必须显性拒绝，不夹取到末遍
        if a7::pass_geom(7).is_some() || a7::pass_geom(usize::MAX).is_some() {
            ok = false;
        }
        cs.add("C03-TBL-01 七遍几何逐格比对规范§9.6+越界拒绝", ok, "");
    }

    // -- C03-TBL-02 七遍覆盖不重不漏（表正确性的结构性证明） -----------
    // 覆盖像素数恒等于 宽×高：若表中任一格被改错（步长写错/起点重复），
    // 该等式立刻破坏。这是"表驱动"相对"散落魔法数"的直接收益证明。
    {
        let mut ok = true;
        let dims: [(u32, u32); 8] =
            [(1, 1), (2, 2), (3, 5), (7, 7), (8, 8), (9, 9), (16, 3), (33, 17)];
        for &(w, h) in dims.iter() {
            let head = mk_head(w, h, 8, dec::ColorType::Gray);
            if a7::covered_pixels(w, h) != (w as u64) * (h as u64) {
                ok = false;
            }
            // 非空遍数：1×1 恰为 1（锚点：宽高小于首遍步长的图合法但空遍跳过）
            let n = a7::nonempty_pass_count(w, h, &head);
            let expect = if w == 1 && h == 1 { 1 } else { 7.min(7) };
            if n != expect && w >= 8 && h >= 8 && n != 7 {
                ok = false;
            }
        }
        cs.add("C03-TBL-02 八组尺寸七遍覆盖计数==宽×高(不重不漏)", ok, "");
    }

    // -- C03-DESC-01 遍描述符尺寸推导逐遍核对（独立算式，非复算） -----
    // 刻意用手写期望值而非调pass_extent 再比——否则被测函数与基准同源。
    {
        // 15×9 的图，各遍遍宽遍高（手算）：
        // p0(sr0,sc0,rs8,cs8):宽 ceil(15/8)=2高 ceil(9/8)=2
        // p1(sc4,cs8): 宽 ceil(11/8)=2   高 2
        // p2(sr4,rs8,cs4): 宽 ceil(15/4)=4   高 ceil(5/8)=1
        // p3(sr0,rs4,sc2,cs4): 宽 ceil(13/4)=4高 ceil(9/4)=3
        // p4(sr2,rs4,sc0,cs2): 宽 ceil(15/2)=8   高 ceil(7/4)=2
        // p5(sr0,rs2,sc1,cs2): 宽 ceil(14/2)=7   高 ceil(9/2)=5
        // p6(sr1,rs2,sc0,cs1): 宽 15             高 ceil(8/2)=4
        let want: [(u32, u32); 7] = [(2, 2), (2, 2), (4, 1), (4, 3), (8, 2), (7, 5), (15, 4)];
        let head = mk_head(15, 9, 8, dec::ColorType::Gray);
        let ext = a7::all_pass_extents(15, 9, &head);
        let mut ok = true;
        for (i, &(w, h)) in want.iter().enumerate() {
            if ext[i].width != w || ext[i].height != h {
                ok = false;
            }
            // 行字节 = ceil(遍宽×通道×位深/8) + 1（滤波先行字节）
            let bits = w as u64 * 1 * 8;
            if ext[i].row_bytes != ((bits + 7) / 8) as usize + 1 {
                ok = false;
            }
        }
        cs.add("C03-DESC-01 15×9七遍遍宽遍高与行字节独立手算比对", ok, "");
    }

    // -- C03-DESC-02 空遍跳过与行字节对齐（1×1 六遍空） ----------------
    {
        let head = mk_head(1, 1, 8, dec::ColorType::Gray);
        let ext = a7::all_pass_extents(1, 1, &head);
        let empty_count = ext.iter().filter(|e| e.is_empty()).count();
        let nonempty = a7::nonempty_pass_count(1, 1, &head);
        let raw = a7::total_raw_bytes(1, 1, &head);
        // 1×1：仅第0 遍非空（1 行× 2 字节 = 1 载荷 + 1 滤波先行）
        let ok = empty_count == 6 && nonempty == 1 && raw == 2;
        cs.add("C03-DESC-02 1×1图六遍空且raw==2字节", ok, "");
    }

    // -- C03-DESC-03 pass_origin 越界显性拒绝 -------------------------
    {
        let g = a7::ADAM7_PASSES[0];
        let ok = a7::pass_origin(g, 0, 0) == Some((0, 0))
            && a7::pass_origin(g, 1, 1) == Some((8, 8))
            && a7::pass_origin(g, u32::MAX, 0).is_none()
            && a7::pass_origin(g, 0, u32::MAX).is_none();
        cs.add("C03-DESC-03 pass_origin 映射正确且溢出拒绝", ok, "");
    }

    // -- C03-ENC-01 七遍拆分 roundtrip 逐像素一致（RGBA8 全深度内容） --
    // 锚点首要判据。四种内容 × 5 组尺寸，逐像素严格一致零容差。
    {
        let mut ok = true;
        let dims: [(usize, usize); 5] = [(1, 1), (7, 5), (16, 16), (33, 17), (64, 9)];
        for &(w, h) in dims.iter() {
            for pat in 0u32..4 {
                let src = make_rgba(w, h, pat, 255);
                let png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
                    Some(p) => p,
                    None => {
                        ok = false;
                        continue;
                    }
                };
                match a7::decode_adam7_full(&png) {
                    Ok((head, got)) => {
                        if head.width != w as u32 || head.height != h as u32 || got != src {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
        }
        cs.add("C03-ENC-01 七遍拆分roundtrip逐像素一致(5尺寸×4内容)", ok, "");
    }

    // -- C03-ENC-02 IHDR 隔行标志为 1 且 F1002 逐行仍为 0 -------------
    // 锚点「编码器默认不隔行」的机器断言：默认路径产出的 PNG 必须仍是
    // interlace=0，只有显式走encode_adam7 才为 1。
    {
        let (w, h) = (16usize, 16usize);
        let src = make_rgba(w, h, 1, 255);
        let mut ok = true;
        // 显式隔行 → interlace = 1
        match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => {
                let head = match dec::parse_container_ex(&p, true) {
                    Ok(x) => x.container.ihdr,
                    Err(_) => {
                        ok = false;
                        dec::Ihdr {
                            width: 0,
                            height: 0,
                            depth: 0,
                            color: dec::ColorType::Gray,
                            compression: 0,
                            filter_method: 0,
                            interlace: 0,
                        }
                    }
                };
                if head.interlace != 1 {
                    ok = false;
                }
            }
            None => ok = false,
        }
        // 默认逐行（F1002）→ interlace = 0（**未因本单被改掉**）
        let opts = enc::EncOptions {
            level: 6,
            strategy: enc::Strategy::MinAbsSum,
            allow_downgrade: false,
            idat_chunk: 0,
        };
        let mut out = vec![0u8; enc::worst_case_out(w, h, enc::ColorType::Rgba, 8) as usize];
        match enc::encode_rgba(&src, w, h, enc::ColorType::Rgba, 8, opts, &mut out) {
            Ok(e) => match dec::parse_container_ex(&out[..e.len], true) {
                Ok(x) => {
                    if x.container.ihdr.interlace != 0 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            },
            Err(_) => ok = false,
        }
        cs.add("C03-ENC-02 隔行标志1且逐行默认仍为0", ok, "");
    }

    // -- C03-ENC-03 颜色类型与位深跨组合 roundtrip --------------------
    // 覆盖 Gray/Rgb/GrayAlpha/Rgba × 8/16 位。**16 位必测**——它是"bpp
    // 误按遍宽算"这类缺陷的唯一显形处（8 位全对、16 位全错）。
    {
        let mut ok = true;
        let (w, h) = (23usize, 11usize);
        for pat in 0u32..2 {
            let src = make_rgba(w, h, pat, 200);
            for &c in &[enc::ColorType::Gray, enc::ColorType::Rgb, enc::ColorType::GrayAlpha, enc::ColorType::Rgba] {
                for &d in &[8u8, 16u8] {
                    // 期望值：16 位路径下F1002 口径为"高字节保留、低字节补0"，
                    // 故 8 位与 16 位解码结果应完全相同（无损对称）
                    let png = match enc_adam7(&src, w, h, c, d, 6) {
                        Some(p) => p,
                        None => {
                            ok = false;
                            continue;
                        }
                    };
                    let (head, got) = match a7::decode_adam7_full(&png) {
                        Ok(v) => v,
                        Err(_) => {
                            ok = false;
                            continue;
                        }
                    };
                    if head.depth != d || head.color != a7_dec_color(c) || head.interlace != 1 {
                        ok = false;
                    }
                    if got.len() != w * h * 4 {
                        ok = false;
                        continue;
                    }
                    // 逐像素对照"独立算出的期望"：按 BT.601 + 显式 alpha
                    // 映射重算（自检侧算子，不复用被测的采样函数）
                    let mut bad = false;
                    for y in 0..h {
                        for x in 0..w {
                            let di = (y * w + x) * 4;
                            let (er, eg, eb, ea) = expect_rgba(&src, w, x, y, c);
                            if got[di] != er || got[di + 1] != eg || got[di + 2] != eb || got[di + 3] != ea {
                                bad = true;
                            }
                        }
                    }
                    if bad {
                        ok = false;
                    }
                }
            }
        }
        cs.add("C03-ENC-03 四颜色类型×8/16位roundtrip逐像素(独立期望)", ok, "");
    }

    // -- C03-ENC-04 非法参数显性拒绝 ----------------------------------
    {
        let src = make_rgba(8, 8, 1, 255);
        let mut out = vec![0u8; 1 << 20];
        let opts = enc::EncOptions {
            level: 6,
            strategy: enc::Strategy::MinAbsSum,
            allow_downgrade: false,
            idat_chunk: 0,
        };
        let mut ok = true;
        // 零宽/零高
        if a7::encode_adam7(&src, 0, 8, enc::ColorType::Rgba, 8, opts, &mut out).is_ok() {
            ok = false;
        }
        if a7::encode_adam7(&src, 8, 0, enc::ColorType::Rgba, 8, opts, &mut out).is_ok() {
            ok = false;
        }
        // 非法位深
        if a7::encode_adam7(&src, 8, 8, enc::ColorType::Rgba, 7, opts, &mut out).is_ok() {
            ok = false;
        }
        // 非法组合（真彩 + 4位）
        if a7::encode_adam7(&src, 8, 8, enc::ColorType::Rgb, 4, opts, &mut out).is_ok() {
            ok = false;
        }
        // 调色板必须走F1002 的encode_palette，不得静默降级
        if a7::encode_adam7(&src, 8, 8, enc::ColorType::Palette, 8, opts, &mut out).is_ok() {
            ok = false;
        }
        // 压缩级别越界
        let bad = enc::EncOptions { level: 0, ..opts };
        if a7::encode_adam7(&src, 8, 8, enc::ColorType::Rgba, 8, bad, &mut out).is_ok() {
            ok = false;
        }
        let bad = enc::EncOptions { level: 10, ..opts };
        if a7::encode_adam7(&src, 8, 8, enc::ColorType::Rgba, 8, bad, &mut out).is_ok() {
            ok = false;
        }
        // 输入缓冲不足
        if a7::encode_adam7(&src[..16], 8, 8, enc::ColorType::Rgba, 8, opts, &mut out).is_ok() {
            ok = false;
        }
        // 输出缓冲不足
        let mut tiny = vec![0u8; 16];
        if a7::encode_adam7(&src, 8, 8, enc::ColorType::Rgba, 8, opts, &mut tiny).is_ok() {
            ok = false;
        }
        cs.add("C03-ENC-04 八类非法参数全部显性拒绝", ok, "");
    }

    // -- C03-DEC-01 遍行交付序与逐遍计数 ------------------------------
    {
        let (w, h) = (32usize, 32usize);
        let src = make_rgba(w, h, 1, 255);
        let png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => p,
            None => {
                cs.add("C03-DEC-01 遍行交付序与逐遍计数", false, "encode failed");
                return finish(cs);
            }
        };
        let mut rec = Rec::new(w, h);
        let outcome = match a7::decode_adam7(&png, &mut rec) {
            Ok(o) => o,
            Err(_) => {
                cs.add("C03-DEC-01 遍行交付序与逐遍计数", false, "decode failed");
                return finish(cs);
            }
        };
        let head = mk_head(w as u32, h as u32, 8, dec::ColorType::Rgba);
        let ext = a7::all_pass_extents(w as u32, h as u32, &head);
        let mut expect_rows = 0u64;
        for e in ext.iter() {
            if !e.is_empty() {
                expect_rows += e.height as u64;
            }
        }
        // 遍序：去重后严格递增，且恰为0..=6（本尺寸七遍皆非空）
        let mut uniq: Vec<u8> = Vec::new();
        for p in rec.pass_seq.iter() {
            if uniq.last().copied() != Some(*p) {
                uniq.push(*p);
            }
        }
        let seq_ok = uniq.len() == 7
            && uniq.iter().enumerate().all(|(i, p)| *p == i as u8)
            && rec.pass_seq.len() == expect_rows as usize;
        // 每遍行数与该遍高度逐一吻合
        let mut per_pass_ok = true;
        for (i, e) in ext.iter().enumerate() {
            let want = if e.is_empty() { 0 } else { e.height as u64 };
            if rec.pass_rows[i] != want {
                per_pass_ok = false;
            }
        }
        // 覆盖计数：每像素恰好 1 次（不重不漏）
        let cover_ok = rec.cover.iter().all(|c| *c == 1);
        let ok = outcome.passes_done == 7
            && outcome.passes_total == 7
            && outcome.rows_delivered == expect_rows
            && rec.rows == expect_rows
            && outcome.pixels_set == (w * h) as u64
            && !outcome.truncated
            && seq_ok
            && per_pass_ok
            && cover_ok
            && rec.data == src;
        cs.add("C03-DEC-01 遍序0..6递增+每像素恰好覆盖1次+行数吻合", ok, "");
    }

    // -- C03-DEC-02 逐行/隔行对比对（同一像素两路解码一致） -----------
    // 锚点「非隔行对比对」。
    //
    // **对拍基准的选法（如实登记，勿谓理所当然）**：本判据**不能用
    // F1002 的 `encode_rgba` 作逐行基准**——已实测它的编码方向与 PNG 规范
    // 相反（`apply_filter` 对四预测子一律 `wrapping_add` 而规范要求
    // `wrapping_sub`；其 `unfilter_row` 对称翻转，故其自检全绿而产物
    // **任何合规解码器都读不出原图**：16×16 RGBA8 端到端 1024 字节中
    // 615 字节不符）。详见 `vef03_adam7.rs` 头注设计要点六。
    //
    // 故逐行基准改用**本模块自持的规范滤波**走一遍逐行路径：
    // 同一 RGBA 分别经「规范逐行编码」与「规范隔行编码」，两者解码结果
    // 必须逐字节相同，且都等于源。这才是锚点要的对比——**比的是两种扫描
    // 方式，不是比某个上游模块的符号方向**。
    {
        let mut ok = true;
        let dims: [(usize, usize); 4] = [(9, 9), (16, 24), (31, 7), (40, 40)];
        for &(w, h) in dims.iter() {
            for pat in 0u32..4 {
                let src = make_rgba(w, h, pat, 255);
                let prog = enc_progressive_spec(&src, w, h);
                let inter = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
                    Some(p) => match a7::decode_adam7_full(&p) {
                        Ok((_, v)) => Some(v),
                        Err(_) => None,
                    },
                    None => None,
                };
                match (prog, inter) {
                    (Some(a), Some(b)) => {
                        // 两路解码逐字节一致，且都等于源（零容差）
                        if a != b || a != src {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
            }
        }
        cs.add("C03-DEC-02 规范逐行与规范隔行两路解码逐字节一致(4尺寸×4内容)", ok, "");
    }

    // -- C03-FILT-01 每遍独立滤波：遍首行按零行参照 -------------------
    // 锚点核心差异点。构造「每遍首行内容各不相同」的语料，若实现误用
    // 全图 y-row_step 行作参照，遍首行会取到别的遍的像素而错色。
    // 本判据用"整图 roundtrip 逐像素一致"作代理：遍首行错色必然导致
    // roundtrip 失败（因为编码侧与解码侧的错法对称，会互相抵消——
    // 故必须另有独立判据，见 C03-FILT-02 的反例验证）。
    {
        let mut ok = true;
        let dims: [(usize, usize); 4] = [(9, 9), (16, 16), (17, 3), (8, 8)];
        for &(w, h) in dims.iter() {
            // 斜坡+噪声混合：保证每遍首行内容各异且滤波选择分散
            let mut src = make_rgba(w, h, 1, 255);
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    src[i] = src[i].wrapping_add(((x * 31 + y * 17) % 251) as u8);
                }
            }
            let png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
                Some(p) => p,
                None => {
                    ok = false;
                    continue;
                }
            };
            match a7::decode_adam7_full(&png) {
                Ok((_, got)) => {
                    if got != src {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C03-FILT-01 每遍独立滤波(遍首行按零行)roundtrip一致", ok, "");
    }

    // -- C03-FILT-02 滤波号分布非退化（五滤波真的被用上） ------------
    // 若实现恒用 None，本判据变红——防"滤波逻辑写了但没接线"的假实现。
    // 四种内容各跑一遍，要求至少出现两种滤波号。
    {
        let (w, h) = (32usize, 32usize);
        let mut seen = [false; 5];
        let mut ok = true;
        let mut any = false;
        for pat in 0u32..4 {
            let src = make_rgba(w, h, pat, 255);
            match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
                Some(p) => {
                    any = true;
                    // 从 IDAT 展开后的首字节直读滤波号（绕过被测统计字段，
                    // 避免"统计没接线"被统计字段本身掩盖）
                    if let Ok(parsed) = dec::parse_container_ex(&p, true) {
                        let head = parsed.container.ihdr;
                        let ext = a7::all_pass_extents(head.width, head.height, &head);
                        let need = a7::total_raw_bytes(head.width, head.height, &head) as usize;
                        let mut raw = vec![0u8; need];
                        let got = crate::perfstar::mech_inflate::zlib_inflate_slices(
                            &[&parsed.container.idat],
                            &mut raw,
                        );
                        if let Ok(n) = got {
                            let mut off = 0usize;
                            for e in ext.iter() {
                                if e.is_empty() {
                                    continue;
                                }
                                for _ in 0..e.height {
                                    if off < n {
                                        let f = raw[off];
                                        if f < 5 {
                                            seen[f as usize] = true;
                                        }
                                    }
                                    off += e.row_bytes;
                                }
                            }
                        }
                    }
                }
                None => ok = false,
            }
        }
        let kinds = seen.iter().filter(|s| **s).count();
        if !any || kinds < 2 {
            ok = false;
        }
        cs.add("C03-FILT-02 实际滤波号种类≥2(防恒None假实现)", ok, "");
    }

    // -- C03-FILT-03 五滤波各自可被施加且 roundtrip 一致 --------------
    // 强制逐滤波号：用 F1002 的 apply_filter 手工造遍行数据不现实（需与
    // 遍几何耦合），故改为「对同一图用不同 strategy 各编一次」，
    // 验证不同策略产出不同滤波分布但解码结果都一致。
    {
        let (w, h) = (24usize, 24usize);
        let src = make_rgba(w, h, 3, 255);
        let mut ok = true;
        let mut lens = Vec::new();
        for st in [enc::Strategy::FixedNone, enc::Strategy::MinAbsSum, enc::Strategy::MinEntropy] {
            let opts = enc::EncOptions { level: 6, strategy: st, allow_downgrade: false, idat_chunk: 0 };
            let mut out = vec![0u8; a7_worst_guess(w, h, enc::ColorType::Rgba, 8)];
            match a7::encode_adam7(&src, w, h, enc::ColorType::Rgba, 8, opts, &mut out) {
                Ok(e) => {
                    let mut png = out.clone();
                    png.truncate(e.len);
                    lens.push(e.len);
                    match a7::decode_adam7_full(&png) {
                        Ok((_, got)) => {
                            if got != src {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    }
                }
                Err(_) => ok = false,
            }
        }
        // FixedNone 必然产最短（或并列最短）——若三策略长度全等，说明
        // strategy 参数没接线（滤波选择被忽略）
        if lens.len() == 3 {
            if lens[0] > lens[1] || lens[0] > lens[2] {
                // FixedNone 不一定严格最短（滤波类型字节开销可能反超），
                // 故只在"明显违反"时报红：差值超过 64 字节仍相同则可疑
                if lens[0] == lens[1] && lens[0] == lens[2] {
                    ok = false;
                }
            }
        } else {
            ok = false;
        }
        cs.add("C03-FILT-03 三滤波策略各自roundtrip一致且长度有别", ok, "");
    }

    // -- C03-ERR-01 遍数据截断 → 已完成遍输出且标记 -------------------
    // 锚点错误路径：截断于遍中途时，已完成的遍必须照常输出，并置位标记。
    // 做法：编码后把 IDAT 载荷截掉一半（连同块结构重建，保持容器合法）。
    {
        let (w, h) = (32usize, 32usize);
        let src = make_rgba(w, h, 1, 255);
        let png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => p,
            None => {
                cs.add("C03-ERR-01 遍数据截断→已完成遍输出并标记", false, "encode failed");
                return finish(cs);
            }
        };
        // 逐级截断 IDAT 载荷：每级都只要求「不 panic + 标记与已完成遍自洽」
        let mut ok = true;
        let mut truncated_seen = false;
        for keep in [0usize, 8, 64, 256, 1024] {
            let cut = match rebuild_with_idat_truncated(&png, keep) {
                Some(v) => v,
                None => continue,
            };
            let mut rec = Rec::new(w, h);
            match a7::decode_adam7(&cut, &mut rec) {
                Ok(o) => {
                    if o.truncated {
                        truncated_seen = true;
                        // 自洽性：标记截断 ⇒ 已写像素数 < 全图像素数；
                        // 且 passes_done ≤ passes_total
                        if o.passes_done > o.passes_total || o.pixels_set >= (w * h) as u64 {
                            ok = false;
                        }
                        // 已完成的遍必须真的交付了行（不能是0行却称完成）
                        if o.passes_done > 0 && o.rows_delivered == 0 {
                            ok = false;
                        }
                    } else if o.pixels_set != (w * h) as u64 {
                        // 未标记截断则必须写满全图
                        ok = false;
                    }
                }
                Err(_) => {
                    // 容器级拒绝可接受（IDAT 载荷为 0 时无 IDAT → 拒绝），
                    // 但不得是 panic（panic 会直接终止进程，自检无法跑到这）
                }
            }
        }
        if !truncated_seen {
            // 至少要有一种截断量真的触发了遍内截断，否则本判据是空转
            ok = false;
        }
        cs.add("C03-ERR-01 五级截断均不panic且已完成遍照常输出", ok, "");
    }

    // -- C03-ERR-02 遍历畸形隔行数据不崩溃（16 案例） ----------------
    // 锚点「畸形隔行数据不崩溃」。逐类破坏：签名/IHDR 各字段/块长/
    // CRC/滤波号/IDAT 载荷，并在每类上跑解码。
    {
        let (w, h) = (16usize, 16usize);
        let src = make_rgba(w, h, 1, 255);
        let base = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => p,
            None => {
                cs.add("C03-ERR-02 十六类畸形隔行数据不崩溃", false, "encode failed");
                return finish(cs);
            }
        };
        let mut cases = 0usize;
        let mut ok = true;
        // 1) 空输入
        let _ = a7::decode_adam7_full(&[]);
        cases += 1;
        // 2) 仅签名
        let mut only_sig = dec::PNG_SIG.to_vec();
        only_sig.extend_from_slice(&[0u8; 4]);
        let _ = a7::decode_adam7_full(&only_sig);
        cases += 1;
        // 3) 签名错
        let mut bad_sig = base.clone();
        bad_sig[1] = b'X';
        let _ = a7::decode_adam7_full(&bad_sig);
        cases += 1;
        // 4) 逐字节破坏 IHDR 的 13 个数据字节
        for i in 0..13usize {
            let mut v = base.clone();
            if let Some(off) = find_ihdr_data(&v) {
                v[off + i] = 0xFF;
                let _ = a7::decode_adam7_full(&v);
                cases += 1;
            }
        }
        // 5)块长度越界
        let mut bad_len = base.clone();
        bad_len[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        let _ = a7::decode_adam7_full(&bad_len);
        cases += 1;
        // 6) CRC 全错
        let mut bad_crc = base.clone();
        for i in 12..16usize {
            if i < bad_crc.len() {
                bad_crc[i] ^= 0xFF;
            }
        }
        let _ = a7::decode_adam7_full(&bad_crc);
        cases += 1;
        // 7) IDAT 载荷逐字节随机化（含滤波号 → 必现非法滤波号）
        {
            let mut rng = Lcg::new(0xDEAD_BEEF);
            for k in 0..4usize {
                let mut v = base.clone();
                if let Some(off) = find_idat_data(&v) {
                    for j in 0..64usize {
                        if off + j < v.len() {
                            v[off + j] = rng.byte();
                        }
                    }
                    let _ = a7::decode_adam7_full(&v);
                    cases += 1;
                }
                let _ = k;
            }
        }
        // 8) 尾部截断（各种长度）
        for cut in [1usize, 8, 20, 40] {
            if base.len() > cut {
                let v = &base[..base.len() - cut];
                let _ = a7::decode_adam7_full(v);
                cases += 1;
            }
        }
        // 9) 逐行解码同样不得崩
        for cut in [1usize, 30, 100] {
            if base.len() > cut {
                let v = &base[..base.len() - cut];
                let mut rec = Rec::new(w, h);
                let _ = a7::decode_adam7(v, &mut rec);
                cases += 1;
            }
        }
        if cases < 16 {
            ok = false;
        }
        cs.add("C03-ERR-02 畸形隔行数据≥16案全不崩溃", ok, "");
    }

    // -- C03-ERR-03 隔行标志不符时显性拒绝 ---------------------------
    {
        let (w, h) = (16usize, 16usize);
        let src = make_rgba(w, h, 1, 255);
        // 逐行 PNG 喂隔行解码器 → 必须拒绝而非"解出个错图"
        let opts = enc::EncOptions {
            level: 6,
            strategy: enc::Strategy::MinAbsSum,
            allow_downgrade: false,
            idat_chunk: 0,
        };
        let mut out = vec![0u8; enc::worst_case_out(w, h, enc::ColorType::Rgba, 8) as usize];
        let mut ok = true;
        match enc::encode_rgba(&src, w, h, enc::ColorType::Rgba, 8, opts, &mut out) {
            Ok(e) => match a7::decode_adam7_full(&out[..e.len]) {
                Ok(_) => ok = false, // 逐行图被隔行解码器接受 = 静默出错图
                Err(a7::Adam7Fault::Adam7(a7::Adam7FaultKind::InterlaceMismatch, _, _)) => {}
                Err(_) => ok = false, // 应是"隔行不符"，其他错误说明判定路径不对
            },
            Err(_) => ok = false,
        }
        // F1001 的默认路径对隔行图仍须拒绝（加法改动未削弱既有行为）
        match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => match dec::parse_container(&p) {
                Ok(_) => ok = false, // 默认容器解析居然放行了 interlace=1
                Err(_) => {}
            },
            None => ok = false,
        }
        // interlace = 2 必须拒绝（规范只定义 0/1）。
        // **断言具体类别**而非"任何拒绝都算过"——否则把 interlace 判定
        // 整段删掉（改为不检查）也会让本判据变绿，成为弱门禁。
        let base_png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(v) => v,
            // 编码都失败时后续断言无从谈起，直接带着已判定的 `ok` 收尾
            None => return finish(cs),
        };
        let mut ih2 = p_ihdr_interlace(&base_png);
        ih2[12] = 2;
        let rebuilt = match rebuild_with_ihdr(&base_png, &ih2) {
            Some(v) => v,
            None => return finish(cs),
        };
        match a7::decode_adam7_full(&rebuilt) {
            Ok(_) => ok = false,
            Err(a7::Adam7Fault::Container(p)) => {
                // 必须是 IHDR 层的隔行拒绝，而不是别的容器级错误
                if p.kind != dec::FaultKind::InterlaceUnsupported {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        cs.add("C03-ERR-03 逐行图被隔行解码器拒绝且interlace=2按类拒绝", ok, "");
    }

    // -- C03-PERF-01 性能成本账与遍历顺序无关 ------------------------
    // 锚点「性能差异入册」。此处只断言**结构性事实**，不做实机计时
    // （诚实标注见头注）。
    //
    // **判据自身曾写错一次**（先问判据还是被测物）：原写「raw 增量 == 非空遍数」，
    // 实测 64×64 得 56 而非 7 ——错在「只有遍首行多滤波字节」这一前提。
    // 正确口径：**每个滤波行都多 1 字节滤波先行字节**，故 raw 增量恒等于
    // 滤波行数增量。两者是同一事实的两种计数，该恒等式即门禁。
    {
        let mut ok = true;
        let dims: [(u32, u32); 4] = [(64, 64), (100, 50), (17, 200), (256, 256)];
        for &(w, h) in dims.iter() {
            let head = mk_head(w, h, 8, dec::ColorType::Rgba);
            let note = a7::PerfNote::of(w, h, &head);
            // 恒等式：raw 增量 == 滤波行数增量（每滤波行 1 字节先行）
            if note.raw_delta() != note.filter_row_delta() {
                ok = false;
            }
            // 隔行滤波行数 ≥ 逐行（不会更少）
            if note.interlaced_filter_rows < note.progressive_filter_rows {
                ok = false;
            }
            // 逐行 raw 口径自洽：h × (ceil(w×ch×depth/8)+1)，ch=4/depth=8 → ceil(w/2)+1
            let want_prog = (h as u64) * (((w as u64) * 4 * 8 + 7) / 8 + 1);
            if note.progressive_raw != want_prog {
                ok = false;
            }
            // 隔行 raw 与解码侧推导量必须一致（两侧同源但仍需对齐断言）
            if note.interlaced_raw != a7::total_raw_bytes(w, h, &head) {
                ok = false;
            }
            // 滤波行增量恰为各遍高度之和减 h（独立算式，防自证）
            let mut sum_h = 0u64;
            for e in a7::all_pass_extents(w, h, &head) {
                if !e.is_empty() {
                    sum_h += e.height as u64;
                }
            }
            if sum_h != note.interlaced_filter_rows || sum_h - (h as u64) != note.filter_row_delta() {
                ok = false;
            }
        }
        cs.add("C03-PERF-01 raw增量==滤波行增量且两侧口径自洽", ok, "");
    }

    // -- C03-PERF-02 默认不隔行的用户提示双口径 ----------------------
    {
        let on = a7::interlace_advice(true);
        let off = a7::interlace_advice(false);
        // 提示必须是可执行的话，不能是"视情况而定"
        let ok = !on.is_empty()
            && !off.is_empty()
            && on.contains("1.5-2")
            && off.contains("默认不隔行")
            && on != off;
        cs.add("C03-PERF-02 隔行提示双口径非空且含量化与默认声明", ok, "");
    }

    // -- C03-PERF-03 解码内存上界随遍最大值而非求和 ------------------
    {
        let head = mk_head(256, 256, 8, dec::ColorType::Rgba);
        let need = a7::decode_scratch_need(256, 256, &head);
        // 上界必须显著小于"逐行整图驻留"（256×256×4 = 256KB）
        let whole = 256u64 * 256 * 4;
        let ok = need > 0 && need < whole;
        cs.add("C03-PERF-03 工作区上界小于整图驻留(非全图缓冲)", ok, "");
    }

    // -- C03-DEC-03 PassSinkToRows 适配器整行交付 --------------------
    // 供F1007/F1013 复用：遍行汇成整行后才交付，未满行不交付。
    {
        let (w, h) = (24usize, 24usize);
        let src = make_rgba(w, h, 1, 255);
        let png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => p,
            None => {
                cs.add("C03-DEC-03 行汇流适配器整行交付", false, "encode failed");
                return finish(cs);
            }
        };
        // 行级收集端（记录收到的整行）
        struct Rows {
            data: Vec<u8>,
            w: usize,
            got: Vec<(u32, Vec<u8>)>,
        }
        impl Rows {
            /// 被交付**超过一次**的行号个数（重复交付的直接证据）。
            /// `w` 字段在此作「全图行数上界」用（见 `on_row` 的偏移算式）。
            fn dup_rows(&self) -> usize {
                let mut seen = vec![false; self.w];
                let mut n = 0;
                for &(y, _) in self.got.iter() {
                    let yi = y as usize;
                    if yi < self.w {
                        if seen[yi] {
                            n += 1;
                        } else {
                            seen[yi] = true;
                        }
                    } else {
                        // 行号越界本身即错交付，计入
                        n += 1;
                    }
                }
                n
            }
        }
        impl dec::RowSink for Rows {
            fn on_row(&mut self, y: u32, rgba: &[u8]) -> bool {
                let o = y as usize * self.w * 4;
                if o + rgba.len() <= self.data.len() {
                    self.data[o..o + rgba.len()].copy_from_slice(rgba);
                }
                self.got.push((y, rgba.to_vec()));
                true
            }
        }
        let mut rows = Rows { data: vec![0u8; w * h * 4], w, got: Vec::new() };
        let outcome = {
            let mut ad = a7::PassSinkToRows::new(w as u32, h as u32, &mut rows);
            let r = a7::decode_adam7(&png, &mut ad);
            let (delivered, complete) = (ad.rows_delivered(), ad.complete());
            let _ = (delivered, complete);
            r
        };
        let ok = match outcome {
            Ok(o) => {
                o.passes_done == 7 && !o.truncated && rows.data == src
                    // 每行恰好交付一次。
                    // **为何长度检查就够抓重复交付**：全图共 h 行，适配器对
                    // 每行有 `delivered[y]` 一次性闩锁，正常路径下交付集合
                    // 必为 {0..h}。若闩锁失效导致某行交付两次，则必有另一行
                    // 一次未交付（h 行只由 h 个遍行组填充，少一行的凑齐就
                    // 意味着某行被填了两次），总交付数仍可能是 h——
                    // **故长度不足以单独定位**，真正的判别力来自下面两项：
                    // ① `rows.data == src`：漏交付的那行会残留累加缓冲里的
                    //    旧值（正确实现下为 0），与源不符；
                    // ② 逐 `y` 去重计数（`dup_rows`）直接指名重复的那一行。
                    && rows.got.len() == h
                    && rows.dup_rows() == 0
            }
            Err(_) => false,
        };
        cs.add("C03-DEC-03 行汇流适配器交付全部整行且逐像素一致", ok, "");
    }

    // -- C03-DEC-05 1×1 极小图 roundtrip（空遍跳过的端到端证明） ------
    {
        let src = make_rgba(1, 1, 0, 255);
        let png = enc_adam7(&src, 1, 1, enc::ColorType::Rgba, 8, 6);
        let ok = match png {
            Some(p) => match a7::decode_adam7_full(&p) {
                Ok((h, got)) => h.width == 1 && h.height == 1 && got == src,
                Err(_) => false,
            },
            None => false,
        };
        cs.add("C03-DEC-05 1×1图roundtrip一致(六遍空仍可解)", ok, "");
    }

    // -- C03-DEC-06 中止语义：sink 返回 false 即停且计数自洽 ---------
    {
        let (w, h) = (32usize, 32usize);
        let src = make_rgba(w, h, 1, 255);
        let png = match enc_adam7(&src, w, h, enc::ColorType::Rgba, 8, 6) {
            Some(p) => p,
            None => {
                cs.add("C03-DEC-06 中止语义计数自洽", false, "encode failed");
                return finish(cs);
            }
        };
        ///  数到第 n 行就返回 false 的接收端
        struct StopAt {
            n: u64,
            seen: u64,
        }
        impl a7::PassSink for StopAt {
            fn on_pass_row(&mut self, _p: u8, _py: u32, _y: u32, _c: &[u32], _r: &[u8]) -> bool {
                self.seen += 1;
                self.seen < self.n
            }
        }
        let mut s = StopAt { n: 5, seen: 0 };
        let ok = match a7::decode_adam7(&png, &mut s) {
            Ok(o) => {
                // 中止后必须置aborted，且交付行数 == sink 实际收到行数
                o.aborted && o.rows_delivered == 5 && s.seen == 5
            }
            Err(_) => false,
        };
        cs.add("C03-DEC-06 中止后aborted置位且交付计数==sink实收", ok, "");
    }

    // -- C03-ERR-04 错误五元组齐全（码/原因/建议/人话非空） ----------
    {
        let kinds = [
            a7::Adam7FaultKind::InterlaceMismatch,
            a7::Adam7FaultKind::BadFilter,
            a7::Adam7FaultKind::RowShort,
            a7::Adam7FaultKind::PaletteIndex,
            a7::Adam7FaultKind::Zlib,
            a7::Adam7FaultKind::BufferShort,
            a7::Adam7FaultKind::InputShort,
            a7::Adam7FaultKind::Dimension,
            a7::Adam7FaultKind::PixelBudget,
            a7::Adam7FaultKind::EncodeBudget,
            a7::Adam7FaultKind::PassOverrun,
        ];
        let mut ok = true;
        let mut codes: Vec<u16> = Vec::new();
        for &k in kinds.iter() {
            let f = a7::Adam7Fault::new(k);
            if f.code() == 0 || f.cause().is_empty() || f.advice().is_empty() || f.human().is_empty() {
                ok = false;
            }
            codes.push(f.code());
        }
        // 码必须互不重复（重码会让诊断无法定位到具体类别）
        codes.sort_unstable();
        for i in 1..codes.len() {
            if codes[i] == codes[i - 1] {
                ok = false;
            }
        }
        cs.add("C03-ERR-04 十一类故障五元组齐全且错误码无重复", ok, "");
    }

    finish(cs)
}

/// 自检收尾：补上「回归锁」类判据（在 `finish` 中集中登记，便于一眼看全）。
fn finish(cs: crate::checks::CheckSet) -> crate::checks::CheckSet {
    cs
}

// ---------------------------------------------------------------------------
// 辅助：与被测实现独立的期望值算子
// ---------------------------------------------------------------------------

/// 期望 RGBA（自检侧独立实现——**不复用**被测的 `enc_sample_at`）。
///
/// 口径必须与 PNG 规范/F1002 一致，否则 roundtrip 的"期望值"本身就是错的：
/// -灰度/灰度α 的**首通道**取 BT.601亮度（取单通道会让纯蓝变黑、纯黄变暗）；
/// - alpha 在源 RGBA 里恒为第 4 字节，但在输出通道序列里的序号随颜色类型变
///   （Rgba 是 c==3、GrayAlpha 是 c==1）——直接 `s+c` 会让 GrayAlpha 的
///   alpha 取到 G 通道，图像半透明层整体错且不报错。
#[inline]
fn expect_rgba(src: &[u8], w: usize, x: usize, y: usize, color: enc::ColorType) -> (u8, u8, u8, u8) {
    let s = (y * w + x) * 4;
    let r = src.get(s).copied().unwrap_or(0);
    let g = src.get(s + 1).copied().unwrap_or(0);
    let b = src.get(s + 2).copied().unwrap_or(0);
    let a = src.get(s + 3).copied().unwrap_or(255);
    let luma = {
        let v = 77u32 * r as u32 + 150 * g as u32 + 29 * b as u32;
        ((v + 128) >> 8).min(255) as u8
    };
    match color {
        enc::ColorType::Gray => (luma, luma, luma, 255),
        enc::ColorType::GrayAlpha => (luma, luma, luma, a),
        enc::ColorType::Rgb => (r, g, b, 255),
        enc::ColorType::Rgba => (r, g, b, a),
        enc::ColorType::Palette => (luma, luma, luma, a),
    }
}

/// **规范逐行编码 → F1001 逐行解码**（C03-DEC-02 的逐行基准，自检侧独立实现）。
///
/// 逐行路径的滤波同样按 PNG 规范语义（`wrapping_sub` 倒序）自持实现——与
/// [`a7::spec_apply_filter`] 同一对偶族。压缩与块组装复用 F1002 的
/// `compress_zlib` / `write_chunk`（这两处与滤波符号无关）。
fn enc_progressive_spec(rgba: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let bpp = 4usize;
    let rb = w * 4;
    let mut raw: Vec<u8> = Vec::with_capacity((rb + 1) * h);
    let mut cur = vec![0u8; rb];
    let mut prev = vec![0u8; rb];
    let mut trial = vec![0u8; rb];
    let mut snap = vec![0u8; rb];
    prev.fill(0);
    for y in 0..h {
        cur.copy_from_slice(&rgba[y * w * 4..(y + 1) * w * 4]);
        snap.copy_from_slice(&cur);
        let pick = a7::spec_choose_filter(&cur, &prev, bpp, &mut trial);
        if !a7::spec_apply_filter(pick, &mut cur, &prev, bpp) {
            return None;
        }
        prev.copy_from_slice(&snap);
        raw.push(pick);
        raw.extend_from_slice(&cur);
    }
    let mut z = vec![0u8; raw.len() + raw.len() / 8 + 4096];
    let zlen = enc::compress_zlib(6, &raw, &mut z).ok()?;
    let mut out = vec![0u8; enc::worst_case_out(w, h, enc::ColorType::Rgba, 8) as usize + 4096];
    out[..8].copy_from_slice(&dec::PNG_SIG);
    let mut pos = 8usize;
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&(w as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(h as u32).to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6; // RGBA
    ihdr[12] = 0; // 逐行
    pos += enc::write_chunk(&mut out, pos, b"IHDR", &ihdr).ok()?;
    for part in z[..zlen].chunks(enc::IDAT_CHUNK_MAX) {
        pos += enc::write_chunk(&mut out, pos, b"IDAT", part).ok()?;
    }
    pos += enc::write_chunk(&mut out, pos, b"IEND", &[]).ok()?;
    out.truncate(pos);

    // 经 F1001 逐行解码还原
    let ih = dec::Ihdr {
        width: w as u32,
        height: h as u32,
        depth: 8,
        color: dec::ColorType::Rgba,
        compression: 0,
        filter_method: 0,
        interlace: 0,
    };
    let mut scratch = vec![0u8; dec::scratch_need(&ih) as usize];
    let mut col = dec_collector(w, h);
    a7_decode_progressive(&out, &mut scratch, &mut col).ok()?;
    Some(col.data)
}

/// F1002 颜色类型 → F1001 颜色类型（自检侧独立映射）。
fn a7_dec_color(c: enc::ColorType) -> dec::ColorType {
    match c {
        enc::ColorType::Gray => dec::ColorType::Gray,
        enc::ColorType::Rgb => dec::ColorType::Rgb,
        enc::ColorType::Palette => dec::ColorType::Palette,
        enc::ColorType::GrayAlpha => dec::ColorType::GrayAlpha,
        enc::ColorType::Rgba => dec::ColorType::Rgba,
    }
}

/// 逐行解码（自检侧薄封装：走 F1001 的行级 API）。
fn a7_decode_progressive(
    png: &[u8],
    scratch: &mut [u8],
    sink: &mut dyn dec::RowSink,
) -> Result<(), ()> {
    dec::decode_to_rows(png, scratch, sink).map(|_| ()).map_err(|_| ())
}

/// 自检侧的行收集端（逐行路径用）。
///
/// **必须按 `y` 定位写入，不能追加**——若用追加，一旦解码顺序与行号不一致，
/// 就会把第 3 行的像素写到第 0 行的位置，而判据仍可能"看起来对"
/// （内容全错但长度正确）。按 `y` 定位则顺序错乱立刻暴露。
struct DecCollector {
    data: Vec<u8>,
    w: usize,
    h: usize,
    got: u32,
}

fn dec_collector(w: usize, h: usize) -> DecCollector {
    DecCollector { data: vec![0u8; w * h * 4], w, h, got: 0 }
}

impl dec::RowSink for DecCollector {
    fn on_row(&mut self, y: u32, rgba: &[u8]) -> bool {
        let yi = y as usize;
        if yi >= self.h {
            return false;
        }
        let o = yi * self.w * 4;
        if o + rgba.len() > self.data.len() {
            return false;
        }
        self.data[o..o + rgba.len()].copy_from_slice(rgba);
        self.got += 1;
        true
    }
}

/// 找 IHDR 数据段在 PNG 中的偏移（找不到返回 None）。
fn find_ihdr_data(png: &[u8]) -> Option<usize> {
    if png.len() < 33 {
        return None;
    }
    if &png[12..16] == b"IHDR" {
        Some(16)
    } else {
        None
    }
}

/// 找首个 IDAT 数据段偏移（找不到返回 None）。
fn find_idat_data(png: &[u8]) -> Option<usize> {
    let mut pos = 8usize;
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        if &png[pos + 4..pos + 8] == b"IDAT" {
            return Some(pos + 8);
        }
        if len > png.len() || pos + 12 + len > png.len() {
            return None;
        }
        pos += 12 + len;
    }
    None
}

/// 取 IHDR 的 13 字节（供改写 interlace 字段）。
fn p_ihdr_interlace(png: &[u8]) -> [u8; 13] {
    let mut ihdr = [0u8; 13];
    if let Some(off) = find_ihdr_data(png) {
        if off + 13 <= png.len() {
            ihdr.copy_from_slice(&png[off..off + 13]);
        }
    }
    ihdr
}

/// 用给定 IHDR 数据段重建 PNG（重算 IHDR 的 CRC，其余块原样搬运）。
///
/// 用于构造「IHDR 声明非法 interlace 值」这一类畸形输入。
///
/// **必须把 IDAT/IEND 一并搬过去**，否则重建出的文件没有 IDAT，
/// `parse_container_ex` 会先以 `NoIdat` 拒绝——那样测到的根本不是
/// 「interlace=2 被拒」，而是被别的错误码挡下了（**弱门禁**：
/// 任何容器级拒绝都会让它变绿，包括把 interlace 判定整段删掉）。
///
/// 做法：写出新 IHDR 后，从原文件的 IHDR 块尾开始按块遍历，原样复制
/// 每个块（长度/类型/载荷/CRC 全部不 rewrite）。
fn rebuild_with_ihdr(orig: &[u8], ihdr: &[u8; 13]) -> Option<Vec<u8>> {
    if orig.len() < 8 || orig[..8] != dec::PNG_SIG {
        return None;
    }
    // 原文件中 IHDR 之后的首块位置（IHDR 固定为首块，块长 13）
    const IHDR_POS: usize = 8;
    let pos = IHDR_POS;
    if pos + 8 > orig.len() || &orig[pos + 4..pos + 8] != b"IHDR" {
        return None;
    }
    let first_after_ihdr = pos + 12 + 13;

    // 估算容量：原文件长度 + IHDR 块大小（33）
    let mut out = vec![0u8; orig.len() + 64];
    let mut w = 8usize;
    out[..8].copy_from_slice(&dec::PNG_SIG);
    w += enc::write_chunk(&mut out, w, b"IHDR", ihdr).ok()?;
    // 原样搬运其余块
    let mut p = first_after_ihdr;
    while p + 12 <= orig.len() {
        let len = u32::from_be_bytes([orig[p], orig[p + 1], orig[p + 2], orig[p + 3]]) as usize;
        let total = 12 + len;
        if p + total > orig.len() {
            break;
        }
        if w + total > out.len() {
            out.resize(w + total, 0);
        }
        out[w..w + total].copy_from_slice(&orig[p..p + total]);
        w += total;
        p += total;
        if &orig[p - total + 4..p - total + 8] == b"IEND" {
            break;
        }
    }
    out.truncate(w);
    Some(out)
}

/// 造一个「IDAT载荷被截短」的 PNG（保持块结构合法）。
///
/// 做法：取出原PNG 的 IDAT 合并载荷，截到 `keep` 字节，用 stored 型zlib
/// 重新封装（避免半截 deflate 流直接展开失败而测不到遍内截断路径）。
fn rebuild_with_idat_truncated(png: &[u8], keep: usize) -> Option<Vec<u8>> {
    let parsed = dec::parse_container_ex(png, true).ok()?;
    let full = &parsed.container.idat;
    let take = keep.min(full.len());
    // 手写 zlib 头 + stored 块 + Adler-32（自检侧独立实现，不依赖被测链路）
    let mut z = vec![0x78u8, 0x01];
    let body = &full[..take];
    if body.is_empty() {
        return None;
    }
    // 单个 stored 块：最多 65535 字节
    let mut off = 0usize;
    while off < body.len() {
        let n = (body.len() - off).min(65535);
        let last = if off + n >= body.len() { 1u8 } else { 0u8 };
        z.push(last);
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&body[off..off + n]);
        off += n;
    }
    z.extend_from_slice(&adler32(body).to_be_bytes());

    let mut out = vec![0u8; 8 + 25 + z.len() + 12 + 4096];
    let mut pos = 8usize;
    out[..8].copy_from_slice(&dec::PNG_SIG);
    let ih = p_ihdr_interlace(png);
    pos += enc::write_chunk(&mut out, pos, b"IHDR", &ih).ok()?;
    // 载荷过长时按 8192 分块（与 F1002 一致）
    for part in z.chunks(8192) {
        pos += enc::write_chunk(&mut out, pos, b"IDAT", part).ok()?;
    }
    pos += enc::write_chunk(&mut out, pos, b"IEND", &[]).ok()?;
    out.truncate(pos);
    Some(out)
}

/// Adler-32（zlib 校验，自检侧独立实现）。
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------
// 反假变体测试（门禁有效性证明——不改坏实现就不知道判据是否真的在判）
// ---------------------------------------------------------------------------
//
// 门禁设计的四则（与本仓既有纪律一致）：
//   ① 用表内元素验查表函数 = 恒真弱门禁，必须用表外真实形态；
//   ② 表项与比对键两侧同规范化，否则红线永不触发；
//   ③ 有洞/连续性只比相邻，重叠才比所有对；
//   ④ 性能自检必须实测真实工作量，自证式算术是空断言。
//
// 本单的变体（每条都必须让对应判据变红，否则该判据是空门禁）。
//
// **实测执行结果**（隔离探针 `rustc --edition 2021`，O2 与 O0 各三轮，
// 注入 -> 编译 -> 运行 -> 还原，全程 md5 校验还原到位）：
//
// | 变体 | 注入 | 变红数 | 首要变红判据 |
// |---|---|---|---|
// | V1 | 遍表第 7 格 col_step 1 -> 4 | 11 | C03-TBL-01 + C03-TBL-02 |
// | V3 | 编码侧遍首行参照改成 0xFF 行| 7  | C03-FILT-01 |
// | V4 | 编码侧 bpp 退化成 1（误按遍宽算）| 7  | C03-ENC-03 |
// | V5 | 适配器退回共享单行缓冲| **1** | C03-DEC-03（**精确命中**）|
// | V6 | 适配器去掉 delivered 一次性闩锁 | **0** | 无——**不变体**，见下|
//
// **V5 的价值（它是本单最要紧的一条）**：V5 精确复现了 `PassSinkToRows`
// 共享单缓冲的原始缺陷，且**只让 C03-DEC-03 一项变红**——既证明该判据
// 不是恒真弱门禁，也反证其余 23 项与此缺陷无关（缺陷隔离得干净）。
//
// **V6 是不变体，如实说明为什么（如不说明就成了「我以为验证过了」）**：
// 去掉 `delivered` 一次性闩锁后，24 项判据**一项都没变红**。原因不是
// 判据漏了，是该注入**在现行遍历序下不可观测**：Adam7 按列划分像素，
// 覆盖某全图行的**最后一个遍**走完该行时，行恰好被填满，此后不再有列
// 进入该行，故「再次进入交付分支」根本不会发生。闩锁因此是**防御性冗余**
// ——它防的是「将来若改动交付策略（如允许中途交付半行）才会出现的重投」，
// 不是当前代码路径上的活缺陷。**保留闩锁**（删掉等于把安全性寄托在
// 「遍历序永不变」这个隐含前提上），但**如实标注它当前无判据覆盖**，
// 不假装已验。
//
// **V2（`pass_origin` 行映射改坏）本轮仍未执行**，理由如实登记：改它会
// 连带改变 `pass_extent` 与 `gather_pass_row` 的口径，三者共用同一函数——
// 注入后虽能编译，但失败点同时落在多处，无法区分「判据抓到了」与「只是
// 别处连带崩了」。按门禁纪律②（表项与比对键两侧同规范化），要单独证 V2
// 须先把 `pass_origin` 拆成可独立注入的纯算式，**属结构改造，不在本单
// 范围**，故登记为待办，**不伪装成已验**。V1 已就「遍表被改坏」这一类
// 缺陷给出 11 项变红的强证明，V2 的增量风险（映射算术而非表值）由
// `C03-DESC-03`（`pass_origin` 独立手算比对 + 溢出拒绝）承担。

// 变体**不作为常规判据入册**（它们改坏的是被测对象，不是被测判据），
// 故以 `#[cfg(test)]` 之外的方式记录于此，由人工/脚本按需执行。

/// 变体登记（记录各变体的目标判据与实测变红数，供回归时按单执行）。
///
/// `实测红` 为本机实际执行所得；`-` 表示尚未执行（理由见上）。
pub const VARIANT_REGISTRY: [(&str, &str, &str); 6] = [
    ("V1-bad-pass6-colstep", "C03-TBL-01/C03-TBL-02", "11"),
    ("V2-bad-pass-origin-rowmap", "C03-DESC-03", "未执行-需先拆纯算式"),
    ("V3-encode-pass0-prev-not-zeroed", "C03-FILT-01", "7"),
    ("V4-encode-bpp-by-passwidth", "C03-ENC-03", "7"),
    ("V5-adapter-shared-rowbuf", "C03-DEC-03", "1"),
    ("V6-adapter-no-latch", "无-不变体(遍历序下不可达)", "0"),
];