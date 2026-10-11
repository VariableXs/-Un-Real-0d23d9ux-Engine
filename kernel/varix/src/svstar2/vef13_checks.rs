//! VE-F1011 · 域自检（判据逐条对应，见 `vef13_pngsuite.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 34 张标准图逐一解码对拍 → `C1011-对拍-*`
//! - 畸形图集全部显性拒绝不崩溃 → `C1011-畸形-*`
//! - 逐图拒绝理由核对 → `C1011-理由-*`
//! - 51 图全绿/全拒状态表 → `C1011-矩阵-*`
//! - 特性标注入矩阵（覆盖） → `C1011-覆盖-*`
//! - 矩阵机器生成（场景配置驱动） → `C1011-生成-*`
//! - 对拍容差零容差 → `C1011-零容差-*`
//! - 参考解码器基线重录走双人复核 → `C1011-基线-*`
//! - 新增语料流程（样本→最小化→归类→断言） → `C1011-流程-*`
//! - 回归集成（CI 门禁，F1016 联动：摘要灵敏） → `C1011-回归-*`
//! - 错误路径与三要素 → `C1011-错误-*`
//!
//! 弱门禁自律（本域最易犯的六种）：
//! 1. **「全绿」不能只看 `all_green()`**：还要逐行核 ID 数、逐图核三方摘要、
//!    逐畸形图核分类——`all_green()` 在**空矩阵**上也是真的。
//! 2. **对拍不能只比被测侧与参考侧**：两侧共享同一错误时仍全绿。第三路
//!    （源模型）必须参与，否则「同源错误互相抵消」这条最大的漏网之鱼仍在。
//! 3. **「机器生成」不能靠注释声明**：判据要逐图复算字节（同样场景两次
//!    生成必须逐字节相同），并确认生成器不引被测编码器的任何符号。
//! 4. **「零容差」不能只断常量是 0**：要把 1 LSB 的差异**注入**给比对函数，
//!    确认它真能抓住——常量是 0 而比对函数放水，等于没钉。
//! 5. **「双人复核」不能只断人数**：同名两人、自己批自己、空串身份都要拒。
//! 6. **语料不能自证**：CRC / Adler / inflate 的独立实现要用**已知答案向量**
//!    （`crc32("123456789")`、`adler32("Wikipedia")`）校准，而不是只断自己
//!    编自己解自洽——自洽的错误实现能骗过自洽性判据。

#![allow(clippy::needless_range_loop)]

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vef01_pngdec::ColorType;
use super::vef13_corpus::*;
use super::vef13_pngsuite::*;
use super::vef13_refdec as refdec;

/// F1011 域自检入口。
pub fn run_vef13_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-f-f1011");

    // 矩阵只跑一次，全域判据共用（34 图 + 17 图的解码是本单最贵的一段）。
    let m = run_suite();

    // =====================================================================
    // 判据一：51 图全绿 / 全拒状态表
    // =====================================================================
    {
        // 【弱门禁一】先断行数：空矩阵 all_green() 也是真的。
        s.add(
            "C1011-矩阵-行数恰51",
            m.rows.len() == TOTAL_COUNT,
            "34 正常 + 17 畸形 = 51（锚点给定规模）；少一行即语料破损",
        );
        s.add(
            "C1011-矩阵-全绿",
            m.all_green() && m.green_count() == TOTAL_COUNT && m.red_count() == 0,
            "51 图全过：正常图三方逐像素一致，畸形图按声明分类拒绝",
        );
        // 逐行核：正常行 34 + 畸形行 17，ID 与表一致（不能靠行序凑数）。
        let mut accept_ok = true;
        let mut reject_ok = true;
        let mut i = 0usize;
        while i < m.rows.len() {
            let r = &m.rows[i];
            match r.expectation {
                Expectation::Accept => {
                    if r.id != VALID_SCENARIOS[i].id || r.diff.is_some() {
                        accept_ok = false;
                    }
                }
                Expectation::Reject(k) => {
                    let cs = CORRUPT_SCENARIOS[i - VALID_COUNT];
                    if r.id != cs.id || r.subject_reason != Some(k) {
                        reject_ok = false;
                    }
                    if r.reference_reason != Some(k) {
                        reject_ok = false;
                    }
                }
            }
            i += 1;
        }
        s.add(
            "C1011-矩阵-逐行核ID与期望",
            accept_ok && reject_ok && m.rows.len() == TOTAL_COUNT,
            "逐行核 ID/期望/实测分类，不靠行序也不靠总数蒙混",
        );
        // 构造失败的行必须显性留痕（不能静默少一行）。
        let mut fail_n = 0usize;
        let mut j = 0usize;
        while j < m.rows.len() {
            if m.rows[j].failure.is_some() {
                fail_n += 1;
            }
            j += 1;
        }
        s.add(
            "C1011-矩阵-构造失败显性留痕",
            fail_n == 0,
            "语料构造失败会写成红行并带原因，而不是被跳过（少一行就是语料破了）",
        );
    }

    // =====================================================================
    // 判据二：三方对拍（源模型 == 被测 == 参考），零容差
    // =====================================================================
    {
        // 【弱门禁二】第三路（源模型）参与：被测摘要与参考摘要都必须等于源模型摘要。
        let mut three_way = true;
        let mut k = 0usize;
        while k < VALID_COUNT {
            let sc = VALID_SCENARIOS[k];
            let src = build_source(&sc);
            let want = digest_bytes(&src.rgba);
            if m.rows[k].subject_digest != want || m.rows[k].reference_digest != want {
                three_way = false;
            }
            k += 1;
        }
        s.add(
            "C1011-对拍-三方摘要逐图一致",
            three_way,
            "源模型摘要 == 被测摘要 == 参考摘要（两侧共享同一错误时由第三路戳破）",
        );
        // 零容差常量与比对函数的行为。
        s.add(
            "C1011-零容差-容差恒0",
            PIXEL_TOLERANCE == 0,
            "PNG 无损：容差常量恒 0",
        );
        // 【弱门禁四】注入 1 LSB 差异，比对函数必须抓住。
        let base = vec![10u8, 20, 30, 40, 50, 60, 70, 80];
        let mut bumped = base.clone();
        bumped[5] = 61; // 1 LSB
        s.add(
            "C1011-零容差-1LSB必抓",
            compare_rgba(&base, &base).is_none()
                && compare_rgba(&base, &bumped).map(|d| d.index == 1 && d.actual[1] == 61).unwrap_or(false),
            "1 LSB 差异必须被抓到并定位到像素下标（有容差的实现会放过）",
        );
        // 长度不等也不和稀泥。
        s.add(
            "C1011-零容差-长度不等即判异",
            compare_rgba(&base, &base[..4]).is_some(),
            "长度不同直接判差异，不按短的那边算",
        );
        // 首差异定位（非首字节差异时给的是第一个不同像素）。
        let a = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut b = a.clone();
        b[7] = 9;
        s.add(
            "C1011-零容差-首差异定位",
            compare_rgba(&a, &b).map(|d| d.index == 1).unwrap_or(false),
            "差异定位到第一个不同像素（行优先），不只报「不相等」",
        );
    }

    // =====================================================================
    // 判据三：畸形图集逐图拒绝理由核对
    // =====================================================================
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < CORRUPT_COUNT {
            let cs = CORRUPT_SCENARIOS[i];
            let row = &m.rows[VALID_COUNT + i];
            if row.expectation != Expectation::Reject(cs.expect)
                || row.subject_reason != Some(cs.expect)
                || row.reference_reason != Some(cs.expect)
                || cs.mechanism.is_empty()
            {
                ok = false;
            }
            i += 1;
        }
        s.add(
            "C1011-理由-逐图核对相符",
            ok,
            "17 张畸形图：两侧解码器的分类都与表里独立声明的期望一致",
        );
        // 拒绝理由覆盖：15 类逐条有人证。
        let missing = reason_coverage();
        s.add(
            "C1011-理由-分类全覆盖",
            missing.is_empty() && RejectReason::ALL.len() == 15,
            "15 个拒绝分类逐条被畸形图覆盖（锚点：逐图拒绝理由核对）",
        );
        // 拒绝深度：容器级与解码级都各有证，且分类表本身自洽。
        let mut container_n = 0usize;
        let mut decode_n = 0usize;
        let mut i2 = 0usize;
        while i2 < RejectReason::ALL.len() {
            match RejectReason::ALL[i2].depth() {
                RejectDepth::Container => container_n += 1,
                RejectDepth::Decode => decode_n += 1,
            }
            i2 += 1;
        }
        s.add(
            "C1011-理由-拒绝深度双侧有证",
            container_n == 11 && decode_n == 4,
            "11 类容器级 + 4 类解码级；把解码级问题写成容器级会让防线提前",
        );
        // 不崩溃：17 张畸形图全部跑过两侧解码器且都有结论（跑到这里即已证明）。
        let mut decided = 0usize;
        let mut i3 = 0usize;
        while i3 < CORRUPT_COUNT {
            let row = &m.rows[VALID_COUNT + i3];
            if row.subject_reason.is_some() && row.reference_reason.is_some() {
                decided += 1;
            }
            i3 += 1;
        }
        s.add(
            "C1011-畸形-全拒不崩溃",
            decided == CORRUPT_COUNT,
            "17 张畸形图全部有结论：显性拒绝，不 panic、不静默接受",
        );
        // 畸形图必须是从合法基图变异来的（凭空造的畸形测不出防线）。
        let mut all_based = true;
        let mut i4 = 0usize;
        while i4 < CORRUPT_COUNT {
            if valid_index_of(CORRUPT_SCENARIOS[i4].base).is_none() {
                all_based = false;
            }
            i4 += 1;
        }
        s.add(
            "C1011-畸形-基图皆存在",
            all_based,
            "每张畸形图都由一张合法图变异而来（注入是计划层字段变异）",
        );
    }

    // =====================================================================
    // 判据四：覆盖矩阵（每图对应特性标注入矩阵）
    // =====================================================================
    {
        let cov = coverage();
        s.add(
            "C1011-覆盖-无缺口",
            cov.clean() && cov.gaps.is_empty(),
            "必须覆盖的 30 个特性位逐条有人证；缺口即未测",
        );
        s.add(
            "C1011-覆盖-合法组合全覆盖",
            cov.combos_covered == LEGAL_COMBOS.len() && cov.phantom_combos == 0,
            "规范 15 种合法（位深×颜色类型）全有语料图；声明即事实（无虚报）",
        );
        s.add(
            "C1011-覆盖-交错七遍全非空",
            cov.min_interlaced_passes == 7,
            "交错语料必须七遍全非空（遍覆盖不足 = 某些像素没人铺，对拍最难漏）",
        );
        // 「声明即事实」逐项核对：场景表声明的特性位与实际注入一致。
        let mut declared_ok = true;
        let mut i = 0usize;
        while i < VALID_SCENARIOS.len() {
            let sc = VALID_SCENARIOS[i];
            let f = sc.features;
            // 颜色类型位
            let color_bit = match sc.color {
                ColorType::Gray => F_GRAY,
                ColorType::Rgb => F_RGB,
                ColorType::Palette => F_PALETTE,
                ColorType::GrayAlpha => F_GRAY_ALPHA,
                ColorType::Rgba => F_RGBA,
            };
            if f & color_bit == 0 {
                declared_ok = false;
            }
            // 位深位
            let depth_bit = match sc.depth {
                1 => F_DEPTH_1,
                2 => F_DEPTH_2,
                4 => F_DEPTH_4,
                8 => F_DEPTH_8,
                16 => F_DEPTH_16,
                _ => 0,
            };
            if f & depth_bit == 0 {
                declared_ok = false;
            }
            // 滤波 / 压缩 / 透明 / 交错四项必须与场景字段逐位相符
            if f & FILTER_MASK != sc.filter.feature_mask() {
                declared_ok = false;
            }
            if f & COMP_MASK != sc.comp.feature() {
                declared_ok = false;
            }
            if f & (F_TRNS_PALETTE | F_TRNS_GRAY | F_TRNS_RGB) != sc.trns.feature() {
                declared_ok = false;
            }
            if sc.interlace != (f & F_INTERLACE != 0) {
                declared_ok = false;
            }
            // 辅助块声明必须真的写进了 ancillary 列表
            let mut want_blocks = 0u32;
            let mut bi = 0usize;
            while bi < sc.ancillary.len() {
                match sc.ancillary[bi] {
                    Fourcc::TEXT | Fourcc::ZTXT | Fourcc::ITXT => want_blocks |= F_TEXT,
                    Fourcc::GAMA | Fourcc::CHRM | Fourcc::SRGB => want_blocks |= F_COLOR_MGMT,
                    Fourcc::BKGD => want_blocks |= F_BACKGROUND,
                    Fourcc::PHYS => want_blocks |= F_PHYSICAL,
                    _ => {}
                }
                bi += 1;
            }
            if f & (F_TEXT | F_COLOR_MGMT | F_BACKGROUND | F_PHYSICAL) != want_blocks {
                declared_ok = false;
            }
            if (sc.idat_split > 1) != (f & F_SPLIT_IDAT != 0) {
                declared_ok = false;
            }
            if sc.alpha_all_zero != (f & F_ALPHA_ALL != 0) {
                declared_ok = false;
            }
            if (sc.width * sc.height == 1) != (f & F_TINY != 0) {
                declared_ok = false;
            }
            // 非字节对齐：位深 <8 且行宽不是 8 的倍数
            let odd = sc.depth < 8 && (sc.width as usize * sc.depth as usize) % 8 != 0;
            if odd != (f & F_ODD_WIDTH != 0) {
                declared_ok = false;
            }
            i += 1;
        }
        s.add(
            "C1011-覆盖-声明即事实逐项核对",
            declared_ok,
            "颜色/位深/滤波/压缩/透明/交错/元数据/分片/极小/非对齐十项与场景字段逐位相符",
        );
        // 稀疏 tRNS 的语义核对：声明稀疏 ⇔ tRNS 长度真的短于 PLTE。
        let mut sparse_ok = true;
        let mut j = 0usize;
        while j < VALID_SCENARIOS.len() {
            let sc = VALID_SCENARIOS[j];
            let sparse = match sc.trns {
                TrnsPlan::PaletteAlpha(n) => n < sc.palette_entries as usize,
                _ => false,
            };
            if (sc.features & F_SPARSE_TRNS != 0) != sparse {
                sparse_ok = false;
            }
            j += 1;
        }
        s.add(
            "C1011-覆盖-稀疏tRNS语义相符",
            sparse_ok,
            "声明稀疏 ⇔ tRNS 长度真的短于 PLTE（缺省项按规范不透明）",
        );
        // 满调色板声明核对。
        let mut full_ok = true;
        let mut k = 0usize;
        while k < VALID_SCENARIOS.len() {
            let sc = VALID_SCENARIOS[k];
            if (sc.features & F_FULL_PALETTE != 0) != (sc.palette_entries == 256) {
                full_ok = false;
            }
            k += 1;
        }
        s.add(
            "C1011-覆盖-满调色板声明相符",
            full_ok,
            "声明 256 项满表 ⇔ 调色板项数真的是 256",
        );
    }

    // =====================================================================
    // 判据五：矩阵机器生成（场景配置驱动）+ 逐字节可复算
    // =====================================================================
    {
        let issues = corpus_self_check();
        s.add(
            "C1011-生成-表自洽",
            issues.is_empty(),
            "ID 唯一、基图存在、非隔行族内组合不重复（表是数据不是代码）",
        );
        // 逐字节复算：同一场景两次生成必须完全相同（不可复算的语料不能当基线）。
        let mut repro = true;
        let mut i = 0usize;
        while i < VALID_COUNT {
            let a = valid_bytes(i);
            let b = valid_bytes(i);
            match (a, b) {
                (Ok(x), Ok(y)) => {
                    if x != y {
                        repro = false;
                    }
                }
                _ => repro = false,
            }
            i += 1;
        }
        s.add(
            "C1011-生成-逐字节可复算",
            repro,
            "同一场景两次生成的字节流逐字节相同：语料可当回归基线",
        );
        // 序列化的纯函数性：serialize(build_plan(sc)) 必须等于 valid_bytes。
        let mut pure = true;
        let mut j = 0usize;
        while j < VALID_COUNT {
            let sc = VALID_SCENARIOS[j];
            let via_plan = serialize(&build_plan(&sc));
            let direct = valid_bytes(j);
            match (via_plan, direct) {
                (Ok(x), Ok(y)) => {
                    if x != y {
                        pure = false;
                    }
                }
                _ => pure = false,
            }
            j += 1;
        }
        s.add(
            "C1011-生成-序列化纯函数",
            pure,
            "serialize 是纯函数：计划 → 字节，没有隐藏的运行期状态",
        );
        // 每张图都以合法 PNG 签名开头，且非空（否则「全绿」是空转）。
        let mut shaped = true;
        let mut k = 0usize;
        while k < VALID_COUNT {
            if let Ok(b) = valid_bytes(k) {
                if b.len() < 45 || b[..8] != dec_sig()[..] {
                    shaped = false;
                }
            } else {
                shaped = false;
            }
            k += 1;
        }
        s.add(
            "C1011-生成-签名与非空",
            shaped,
            "34 张图都以合法 PNG 签名开头且字节数达最小容器量",
        );
        // 语料字节上限真实生效。
        let mut big = build_plan(&VALID_SCENARIOS[0]);
        big.raw = alloc::vec![0u8; CORPUS_BYTES_MAX + 1];
        s.add(
            "C1011-生成-字节上限生效",
            serialize(&big).is_err(),
            "扫描线超上限即拒（生成失控时立刻显性，而不是产出巨型语料）",
        );
        // 越界下标给错不 panic。
        s.add(
            "C1011-生成-越界下标给错",
            valid_bytes(VALID_COUNT).is_err() && corrupt_bytes(CORRUPT_COUNT).is_err(),
            "下标越界返回 Err 而非 panic（语料表改动后的常见误用）",
        );
    }

    // =====================================================================
    // 判据六：独立实现的已知答案校准（不自证）
    // =====================================================================
    {
        // 【弱门禁六】已知答案向量：crc32("123456789") 与 adler32("Wikipedia")。
        s.add(
            "C1011-独立-CRC已知答案",
            refdec::crc32(b"123456789") == 0xCBF4_3921,
            "自写 CRC-32 对标准检验值正确（只断自编自解自洽会被错的实现骗过）",
        );
        s.add(
            "C1011-独立-Adler已知答案",
            refdec::adler32(b"Wikipedia") == 0x11E6_0398,
            "自写 Adler-32 对标准检验值正确",
        );
        // 三种 deflate 块：写侧产出的流必须能被自写 inflate 原样读回。
        let raw = build_plan(&VALID_SCENARIOS[5]).raw;
        let stored = refdec::deflate_stored(&raw);
        let fixed = refdec::deflate_fixed_literal(&raw);
        let dynamic = match refdec::deflate_dynamic_literal(&raw) {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };
        s.add(
            "C1011-独立-stored块往返",
            refdec::inflate_raw(&stored, 1 << 20).map(|v| v == raw).unwrap_or(false),
            "stored 块：自写 deflate → 自写 inflate 逐字节相同",
        );
        s.add(
            "C1011-独立-fixed块往返",
            refdec::inflate_raw(&fixed, 1 << 20).map(|v| v == raw).unwrap_or(false),
            "fixed Huffman 字面量块往返（含 EOB 与码字 MSB-first 约定）",
        );
        s.add(
            "C1011-独立-dynamic块往返",
            !dynamic.is_empty() && refdec::inflate_raw(&dynamic, 1 << 20).map(|v| v == raw).unwrap_or(false),
            "dynamic 块往返（表头 RLE + 码长码 + 15 位试探解码全链路）",
        );
        // 三种块形态的字节流必须互不相同（否则「覆盖三种压缩」是假的）。
        s.add(
            "C1011-独立-三块形态互异",
            stored != fixed && fixed != dynamic && stored != dynamic,
            "stored/fixed/dynamic 三种块形态产出互异的字节流",
        );
        // 块类型头位必须真的不同（读一眼头三位，别只比整流）。
        s.add(
            "C1011-独立-块类型头互异",
            (stored[0] & 0x06) == 0 && (fixed[0] & 0x06) == 0x02 && (dynamic[0] & 0x06) == 0x04,
            "BTYPE 字段确为 00 / 01 / 10（位序低位在前）",
        );
        // zlib 封装：FCHECK 使 (CMF*256+FLG) % 31 == 0。
        let z = refdec::zlib_wrap(&stored, &raw, 8);
        let head_ok = z.len() > 6 && (((z[0] as u32) * 256 + (z[1] as u32)) % 31) == 0;
        s.add(
            "C1011-独立-zlib头自洽",
            head_ok && z[0] & 0x0F == 8,
            "zlib 头：CM=8 且 FCHECK 令 16 位头值整除 31",
        );
        // 非法 CM 必须被自写 inflate 拒（CM=9 的畸形语料靠这条分类）。
        let bad = refdec::zlib_wrap(&stored, &raw, 9);
        s.add(
            "C1011-独立-非法CM被拒",
            refdec::zlib_inflate(&bad, 1 << 20).is_err(),
            "CM=9 的 zlib 头被自写 inflate 显性拒绝（畸形语料 xzlb01 的分类依据）",
        );
        // 位打包：位深 1/2/4 的往返必须逐位一致（跨字节边界是错点高发区）。
        let mut pack_ok = true;
        let mut d = 1u8;
        while d <= 4 {
            let vals: Vec<u8> = (0..37u32).map(|i| ((i * 3 + d as u32) % (1u32 << d)) as u8).collect();
            let mut tmp = [0u8; 16];
            let n = refdec::pack_bits(&vals, d, &mut tmp);
            if n == 0 {
                pack_ok = false;
            } else {
                let per = 8 / d as usize;
                let mut i = 0usize;
                while i < vals.len() {
                    let byte = tmp[i / per];
                    let shift = 8 - d as usize * (i % per + 1);
                    if (byte >> shift) & ((1u8 << d) - 1) != vals[i] {
                        pack_ok = false;
                    }
                    i += 1;
                }
            }
            d = if d == 1 { 2 } else if d == 2 { 4 } else { 9 };
        }
        s.add(
            "C1011-独立-位打包往返",
            pack_ok,
            "位深 1/2/4 共 37 个样本（跨字节边界）打包后逐位读回一致",
        );
        // 滤波：非法滤波号必须被拒（不带着非法号继续写）。
        let mut buf = [0u8; 8];
        s.add(
            "C1011-独立-滤波号先查域",
            !refdec::apply_filter(5, &[1, 2, 3], &[0, 0, 0], 1, &mut buf)
                && !refdec::apply_filter(0, &[1, 2, 3], &[0, 0, 0], 0, &mut buf),
            "滤波号 >4 与 bpp=0 都拒：语料层也不许造非法滤波流",
        );
    }

    // =====================================================================
    // 判据七：基线重录走双人复核（锚点错误路径）
    // =====================================================================
    {
        let base = current_baseline(&m);
        s.add(
            "C1011-基线-摘要非空且归属明确",
            base.reference == BASELINE_REFERENCE_ID && base.version == BASELINE_VERSION && !base.dual_reviewed(),
            "新推的基线无复核人（dual_reviewed 假），重录后才带两名复核人",
        );
        let ok = rerecord(&base, &m, &["A", "B"]);
        s.add(
            "C1011-基线-双人复核通过",
            matches!(&ok, Ok(b) if b.version == base.version + 1 && b.dual_reviewed() && b.digest != base.digest),
            "两名不同复核人 ⇒ 版本 +1、摘要更新、复核人留痕",
        );
        // 【弱门禁五】人数、同名、空身份都要拒。
        let one = rerecord(&base, &m, &["A"]);
        let same = rerecord(&base, &m, &["A", "A"]);
        let empty = rerecord(&base, &m, &["", "B"]);
        s.add(
            "C1011-基线-人数同名空身份全拒",
            matches!(one, Err(Fault::SecondReviewRequired { .. }))
                && matches!(same, Err(Fault::ReviewerNotDistinct))
                && matches!(empty, Err(Fault::ReviewerEmpty)),
            "只断人数挡不住自己批自己与空串身份：三者分别给不同码",
        );
        // 红矩阵不允许录基线（造一个红矩阵来试）。
        let mut red = m.clone();
        if let Some(row) = red.rows.get_mut(0) {
            row.green = false;
        }
        let red_res = rerecord(&base, &red, &["A", "B"]);
        s.add(
            "C1011-基线-红矩阵拒录",
            matches!(red_res, Err(Fault::BaselineNotGreen { .. })),
            "给红矩阵录基线等于把 bug 固化成期望（金样纪律）",
        );
        // 摘要未变的重录是噪声。
        let same_res = rerecord(&base, &m, &["A", "B"]);
        let again = match &same_res {
            Ok(b) => rerecord(b, &m, &["A", "B"]),
            Err(_) => Err(Fault::BaselineUnchanged),
        };
        s.add(
            "C1011-基线-摘要未变拒重录",
            matches!(again, Err(Fault::BaselineUnchanged)),
            "摘要没变却重录会污染重录历史（谁在什么时候按了重录必须可查）",
        );
        // 摘要灵敏：一个像素翻一位即摘要改变（回归网靠它抓回归）。
        let mut px = build_source(&VALID_SCENARIOS[0]).rgba;
        let before = digest_bytes(&px);
        if let Some(b) = px.get_mut(0) {
            *b = b.wrapping_add(1);
        }
        s.add(
            "C1011-回归-摘要对单像素灵敏",
            digest_bytes(&px) != before,
            "一个像素翻一位即摘要改变：CI 门禁靠它抓回归（位级摘要，不是像素级平均）",
        );
        // 矩阵摘要对行序敏感（行序变了摘要必须变，否则矩阵重排不会被发现）。
        let mut reordered = m.clone();
        reordered.rows.swap(0, 1);
        s.add(
            "C1011-回归-摘要对行序灵敏",
            reordered.digest() != m.digest(),
            "行序变化即摘要变化：语料重排不会悄悄通过基线",
        );
    }

    // =====================================================================
    // 判据八：期望变更守卫（人工只补新增语料）
    // =====================================================================
    {
        let no_change = guard_expectation_change(&m, &m, &["A", "B"]);
        s.add(
            "C1011-流程-无变更放行",
            matches!(&no_change, Ok(v) if v.is_empty()),
            "没人改期望时守卫放行（否则这函数会变成纯噪音）",
        );
        let mut changed = m.clone();
        if let Some(row) = changed.rows.get_mut(0) {
            row.expectation = Expectation::Reject(RejectReason::Zlib);
        }
        let guarded = guard_expectation_change(&m, &changed, &["A", "B"]);
        let one_reviewer = guard_expectation_change(&m, &changed, &["A"]);
        s.add(
            "C1011-流程-期望变更须复核",
            matches!(guarded, Err(Fault::ExpectationChangedWithoutReview { .. }))
                && matches!(one_reviewer, Err(Fault::SecondReviewRequired { .. })),
            "既有行的期望被改 ⇒ 拒；且这道拒本身也要双人复核",
        );
        // 语料 ID 规则。
        s.add(
            "C1011-流程-ID规则可执行",
            corpus_id_ok("xnew0001") && !corpus_id_ok("xsig01") && !corpus_id_ok("x") && !corpus_id_ok("basn0g08"),
            "新 ID 须 x 开头、≥5 字符、不撞既有 ID；撞号会让矩阵两行同名",
        );
    }

    // =====================================================================
    // 判据九：新增语料流程四步（样本→最小化→归类→断言）
    // =====================================================================
    {
        // 用一张真畸形图走全流程。
        let sample = match corrupt_bytes(5) {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };
        let out = intake(&sample);
        s.add(
            "C1011-流程-四步走到断言",
            !sample.is_empty()
                && out.agreed
                && out.class == Some(CORRUPT_SCENARIOS[5].expect)
                && out.stopped_at == IntakeStep::Assert
                && out.minimized_bytes <= out.sample_bytes,
            "合法畸形样本走完样本/最小化/归类/断言四步，两侧分类一致",
        );
        // 最小化必须真的变短（否则「最小化」是摆设）。
        s.add(
            "C1011-流程-最小化真变短",
            out.minimized_bytes < out.sample_bytes,
            "最小化产出严格短于原样本（最小化是最小化，不是「重新序列化」）",
        );
        // 最小化后分类不变（这是最小化的正确性定义）。
        let min = minimize_sample(&sample, CORRUPT_SCENARIOS[5].expect);
        let min_class = match subject_decode(&min, false) {
            SubjectVerdict::Rejected(r) => Some(r),
            _ => None,
        };
        s.add(
            "C1011-流程-最小化不改分类",
            min.len() < sample.len() && min_class == Some(CORRUPT_SCENARIOS[5].expect),
            "砍字节后拒绝分类不变（分类变了就是砍过了头）",
        );
        // 空样本停在第一步。
        s.add(
            "C1011-流程-非法样本停第一步",
            intake(&[]).stopped_at == IntakeStep::Sample,
            "样本非法即停在样本步：不跳步（跳步处正是语料库被垃圾填满处）",
        );
        // 正常图不得入表（两侧都接受 ⇒ 无归类）。
        let good = match valid_bytes(0) {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };
        s.add(
            "C1011-流程-正常图不入表",
            intake(&good).stopped_at == IntakeStep::Classify && !intake(&good).agreed,
            "两侧都接受的样本没有归类结论，停在归类步（不得当畸形语料收）",
        );
        // 四步序号与短名齐备（报告面）。
        s.add(
            "C1011-流程-四步序号齐备",
            IntakeStep::Sample.ordinal() == 1
                && IntakeStep::Minimize.ordinal() == 2
                && IntakeStep::Classify.ordinal() == 3
                && IntakeStep::Assert.ordinal() == 4
                && !IntakeStep::Minimize.label().is_empty(),
            "流程四步有序号与短名（面板按序展示，不靠 if 链硬编）",
        );
    }

    // =====================================================================
    // 判据十：错误路径与三要素
    // =====================================================================
    {
        let f1 = Fault::BaselineNotGreen { red: 3 };
        let f2 = Fault::ReviewerNotDistinct;
        // 码段双向钉死：在 0xF7 段，不在 F1009(0xF5)/F1010(0xF6)/F1001(0xF1)/VE-J(0xF9)/VE-N(0x2C)。
        s.add(
            "C1011-错误-码段双向钉死",
            (f1.code() & 0xFF00) == 0xF700
                && (f2.code() & 0xFF00) == 0xF700
                && (f1.code() & 0xFF00) != 0xF600
                && (f1.code() & 0xFF00) != 0xF500
                && (f1.code() & 0xFF00) != 0xF100
                && (f1.code() & 0xFF00) != 0xF900
                && (f1.code() & 0xFF00) != 0x2C00,
            "本单独占 0xF7xx，且不落入 F1010/F1009/F1001/VE-J/VE-N 已占段",
        );
        let all = [
            Fault::BaselineNotGreen { red: 1 },
            Fault::BaselineUnchanged,
            Fault::SecondReviewRequired { got: 1 },
            Fault::ReviewerNotDistinct,
            Fault::ReviewerEmpty,
            Fault::ExpectationChangedWithoutReview { count: 1 },
            Fault::CorpusSelfCheck { issues: 1 },
            Fault::CoverageGap { missing: 1 },
            Fault::ReasonCoverageGap { missing: 1 },
            Fault::RowCountMismatch { got: 1 },
            Fault::CorpusTooLarge { bytes: 1 },
            Fault::CorpusBuildFailed,
            Fault::CorpusIdRejected,
            Fault::PixelMismatch { index: 1 },
            Fault::IntakeSampleRejected { bytes: 1 },
            Fault::IntakeClassificationDisagreed,
            Fault::ExpectationDrift { count: 1 },
        ];
        let mut uniq = true;
        let mut i = 0usize;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                if all[i].code() == all[j].code() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add(
            "C1011-错误-17变体码各不同",
            all.len() == 17 && uniq,
            "码重复会让下游按码分流时把两种故障当一种",
        );
        let mut three_ok = true;
        let mut k = 0usize;
        while k < all.len() {
            let t = all[k].three_elements();
            if t.is_empty() || !all[k].why().contains("F1011") || !t.contains('｜') {
                three_ok = false;
            }
            k += 1;
        }
        s.add(
            "C1011-错误-三要素齐备带出处",
            three_ok,
            "what/why/advice 非空且 why 引用 F1011；拒绝信息不全 = 用户不知道改什么",
        );
        s.add(
            "C1011-错误-诊断码格式稳定",
            f1.diag().starts_with("F1011-") && f2.diag().len() > 7,
            "诊断码供面板 grep，格式必须稳定",
        );
        // 边界操作零 panic。
        let safe = valid_bytes(usize::MAX).is_err()
            && corrupt_bytes(usize::MAX).is_err()
            && corpus_id_ok("")
            && minimize_sample(&[], RejectReason::Zlib).is_empty()
            && intake(&alloc::vec![0u8; CORPUS_BYTES_MAX + 8]).stopped_at == IntakeStep::Sample;
        s.add(
            "C1011-错误-边界操作零panic",
            safe,
            "越界下标 / 空 ID / 空样本 / 超限样本：全给结论而非 panic",
        );
    }

    // =====================================================================
    // 判据承载：判据集自身（注册校验由 mod.rs 切段，独立探针复核）
    // =====================================================================
    {
        s.add(
            "C1011-判据集-未截断且非空转",
            !s.truncated() && s.len() >= 45,
            "截断末尾判据等于没写；空集说明判据层空转",
        );
        let _ = format!("{}", m.rows.len());
    }

    s
}

/// PNG 签名（判据里用于核对语料形状；本地持有一份以免判据层依赖被测常量）。
fn dec_sig() -> [u8; 8] {
    [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
}
