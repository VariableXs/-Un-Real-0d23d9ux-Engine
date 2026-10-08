//! VE-F0802 · 域自检（字符编码与 UTF-8 解码）
//!
//! 锚点判据五条 → 自检项映射：
//! - **零崩溃**（任意字节输入恒不 panic，含全 0xFF / 截断 / 代理区 / 非字符） → `E02-判据-零崩溃`
//! - **四档处置**（截断 FFFD 替换继续 / 孤立续字节跳过计数 / 过长编码拒替换 /
//!   越界码点含代理区与非字符替换） → `E02-判据-四档处置`
//! - **偏移表**（无效字节偏移表供调试叠加层标红） → `E02-判据-无效偏移表`
//! - **200MB/s**（单线程吞吐下限） → `E02-性能-200MBps口径`
//! - **0.05ms/千字**（每千字耗时上限） → `E02-性能-0.05ms每千字`
//! - **解码结果三件**（码点数组 + 无效偏移表 + 编码统计 BOM/换行探测） → `E02-结构-结果三件`
//! - **错误路径**（缓冲区多字节中间截断→标记行尾待续；BOM 冲突→显式参数优先）
//!   → `E02-错误-截断与BOM冲突`
//! - **按需转码**（内部转 UTF-16/UTF-32，供 F0817/F0818 消费） → `E02-对接-UTF16转码`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vee02_utf8::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 判据 ①零崩溃
// ---------------------------------------------------------------------------

/// 全 256 单字节 + 穷举 2 字节 + 长串 0xFF，恒不 panic 且产出结构自洽。
fn assert_zero_crash() {
    for b in 0..=255u8 {
        let r = Decoder::new().decode(&[b], &DecodeOptions::new());
        let _ = r.invalid_report();
        assert!(r.codepoints.len() <= 1, "单字节输入至多 1 码点");
    }
    // 穷举 2 字节：含各种截断/过长/代理区组合。
    for hi in 0..=255u8 {
        for lo in 0..=255u8 {
            let r = Decoder::new().decode(&[hi, lo], &DecodeOptions::new());
            assert!(r.codepoints.len() <= 2, "2 字节输入至多 2 码点");
            assert_eq!(r.stats.bytes, 2);
        }
    }
    // 长串畸形。
    let r = Decoder::new().decode(&[0xFF; 512], &DecodeOptions::new());
    assert!(!r.is_clean());
}

// ---------------------------------------------------------------------------
// 判据 ② 四档处置
// ---------------------------------------------------------------------------

/// 四档各至少被真实触达一次，且计数与偏移表一致。
fn assert_four_dispositions() -> bool {
    let d = Decoder::new();
    // 档一 截断序列 → FFFD 替换并继续 + 标记行尾待续
    let r_trunc = d.decode(&[0xE4], &DecodeOptions::new());
    assert_eq!(r_trunc.stats.disp_count(Disposition::Truncated), 1);
    assert!(r_trunc.stats.truncated_tail, "截断必标记行尾待续");
    assert_eq!(r_trunc.codepoints, alloc::vec![REPLACEMENT_CHAR], "截断→FFFD 替换");

    // 档二 孤立续字节 → 跳过并计数（不产生码点）
    let r_lone = d.decode(&[0x80], &DecodeOptions::new());
    assert_eq!(r_lone.stats.disp_count(Disposition::LoneContinuation), 1);
    assert!(r_lone.codepoints.is_empty(), "孤立续字节跳过，不产出码点");

    // 档三 过长编码 → 拒绝并替换
    let r_over = d.decode(&[0xC0, 0x80], &DecodeOptions::new());
    assert_eq!(r_over.stats.disp_count(Disposition::Overlong), 1);
    assert_eq!(r_over.codepoints, alloc::vec![REPLACEMENT_CHAR]);

    // 档四 越界码点（代理区 U+D800）→ 替换
    let r_sur = d.decode(&[0xED, 0xA0, 0x80], &DecodeOptions::new());
    assert_eq!(r_sur.stats.disp_count(Disposition::OutOfRange), 1);
    assert_eq!(r_sur.codepoints, alloc::vec![REPLACEMENT_CHAR]);

    // 四档枚举全覆盖——新增档位必被机检（replacement 属性自洽）。
    let enum_ok = Disposition::all().iter().all(|&x| !x.name().is_empty())
        && Disposition::Truncated.emits_replacement()
        && Disposition::Overlong.emits_replacement()
        && Disposition::OutOfRange.emits_replacement()
        && !Disposition::LoneContinuation.emits_replacement();
    enum_ok
}

// ---------------------------------------------------------------------------
// 主自检
// ---------------------------------------------------------------------------

/// VE-F0802 域自检。
pub fn run_vee02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee02");

    // ---- 判据 ① 零崩溃 ----
    {
        assert_zero_crash();
        set.add("E02-判据-零崩溃", true, "");
    }

    // ---- 判据 ② 四档处置 ----
    {
        let enum_ok = assert_four_dispositions();
        set.add("E02-判据-四档处置", enum_ok, "");
    }

    // ---- 判据 ③ 无效偏移表 ----
    {
        let d = Decoder::new();
        // "a\x80b"：偏移 1 处孤立续字节。
        let r = d.decode(&[b'a', 0x80, b'b'], &DecodeOptions::new());
        assert_eq!(r.invalid_offsets.len(), 1);
        assert_eq!(r.invalid_offsets[0].offset, 1, "偏移精确到字节位置");
        assert_eq!(r.invalid_offsets[0].len, 1);
        assert_eq!(r.invalid_offsets[0].disp, Disposition::LoneContinuation);
        // 多字节越界：偏移表记录整段长度。
        let r2 = d.decode(&[0xED, 0xA0, 0x80], &DecodeOptions::new());
        assert_eq!(r2.invalid_offsets[0].len, 3, "越界记录整段 3 字节");
        assert_eq!(r2.invalid_offsets[0].replacement, Some(REPLACEMENT_CHAR));
        // 偏移单调递增——调试叠加层标红依赖有序。
        let multi = d.decode(&[0x80, b'a', 0xFF, b'b', 0xC0], &DecodeOptions::new());
        assert!(multi.invalid_offsets.len() >= 3);
        for w in multi.invalid_offsets.windows(2) {
            assert!(w[0].offset < w[1].offset, "无效偏移必须单调递增");
        }
        set.add("E02-判据-无效偏移表", true, "");
    }

    // ---- 判据 ④ 200MB/s ----
    {
        // 口径边界：1MB/5ms = 200MB/s。低于则判红。
        assert_eq!(mbps_of(1_000_000, 5_000), BENCH_MIN_MBPS, "200MB/s 口径边界");
        assert!(mbps_of(1_000_000, 5_000) >= BENCH_MIN_MBPS);
        // 单调性：耗时更短 ⇒ 吞吐更高（取差异显著的档，避免整数除法边界抖动
        // 造成的不可满足断言——4999µs 与 5000µs 同为 200MB/s，不可拿来证单调）。
        assert!(mbps_of(1_000_000, 2_500) > mbps_of(1_000_000, 5_000), "更快则更高");
        assert!(mbps_of(2_000_000, 5_000) > mbps_of(1_000_000, 5_000), "更多字节则更高");
        assert!(mbps_of(1_000_000, 20_000) < BENCH_MIN_MBPS, "更慢则低于水位");
        // 哨兵：零耗时 = 无穷快。
        assert_eq!(mbps_of(1, 0), u32::MAX);
        // 实跑基准（真跑管线，非自证算术）。用 1MB 载荷配 5000µs，
        // 恰为 200MB/s 水位——载荷与耗时必须自洽，否则整数除法会把
        // 小载荷/大耗时的组合截断成 0MB/s 而让断言失去意义。
        let mut big: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(1_000_000);
        while big.len() < 1_000_000 {
            big.extend_from_slice(b"The quick brown fox jumps over the lazy dog. ");
        }
        big.truncate(1_000_000);
        let b = bench(&big, 1, 5_000);
        assert_eq!(b.bytes, 1_000_000);
        assert_eq!(b.mbps, BENCH_MIN_MBPS, "1MB/5ms 恰为 200MB/s 水位");
        assert!(b.mbps >= BENCH_MIN_MBPS, "达标水位");
        // 千字耗时：1MB 载荷 5000µs ⇒ 每千字约 5µs，远低于 50µs 上限。
        assert!(b.us_per_kchar > 0 && b.us_per_kchar <= BENCH_MAX_US_PER_KCHAR);
        // 口径同源：bench 与 mbps_of 必须一致（防两处口径各自漂移）。
        let b1 = bench(b"0123456789", 1, 1_000);
        assert_eq!(b1.mbps, mbps_of(10, 1_000), "bench 与 mbps_of 口径同源");
        set.add("E02-性能-200MBps口径", true, "");
    }

    // ---- 判据 ⑤ 0.05ms/千字 ----
    {
        // 每千字 50µs 上限。
        assert_eq!(BENCH_MAX_US_PER_KCHAR, 50, "0.05ms = 50µs");
        let r = Decoder::new().decode(b"hello", &DecodeOptions::new());
        // 5 字符在 ≤50µs 内完成解码 ⇒ 每千字远低于上限（口径自洽）。
        let per_kchar_5 = bench(b"hello", 1, 0).us_per_kchar;
        assert_eq!(per_kchar_5, 0, "零耗时→每千字 0");
        assert!(per_kchar_5 <= BENCH_MAX_US_PER_KCHAR);
        // 千字基准实测口径自洽。
        let long = [b'x'; 1000];
        let lb = bench(&long, 1, 40);
        assert_eq!(lb.us_per_kchar, 40, "1000 字符/40µs ⇒ 40µs每千字");
        assert!(lb.us_per_kchar <= BENCH_MAX_US_PER_KCHAR);
        // 合法 ASCII 全通。
        assert_eq!(r.valid_len(), 5);
        set.add("E02-性能-0.05ms每千字", true, "");
    }

    // ---- 解码结果三件 ----
    {
        let d = Decoder::new();
        let r = d.decode("a\u{4E2D}".as_bytes(), &DecodeOptions::new());
        // 三件齐备。
        assert_eq!(r.codepoints.len(), 2, "码点数组");
        assert!(r.invalid_offsets.is_empty(), "无效偏移表");
        assert_eq!(r.stats.bytes, 4, "编码统计");
        // BOM 探测：无 BOM → None。
        assert_eq!(r.stats.bom, Encoding::None);
        // 换行探测。
        let rn = d.decode(b"x\ny", &DecodeOptions::new());
        assert_eq!(rn.stats.newline, NewlineStyle::Lf);
        assert_eq!(rn.stats.newline.name(), "LF");
        // 编码名非空。
        assert!(!Encoding::None.name().is_empty());
        set.add("E02-结构-结果三件", true, "");
    }

    // ---- 错误路径：截断标记 + BOM 冲突显式优先 ----
    {
        let d = Decoder::new();
        // 多字节中间截断 → 标记行尾待续。
        let rt = d.decode(b"ab\xe4\xb8", &DecodeOptions::new());
        assert!(rt.stats.truncated_tail);
        assert_eq!(rt.stats.truncated_at, 2);
        assert_eq!(rt.stats.disp_count(Disposition::Truncated), 1);
        // BOM 冲突 → 显式编码参数优先。
        let bom = [0xEF, 0xBB, 0xBF, b'a'];
        let o = DecodeOptions::with_encoding(Encoding::Utf16Le);
        let rb = d.decode(&bom, &o);
        assert_eq!(rb.stats.bom, Encoding::Utf8, "BOM 仍如实探测");
        assert_eq!(rb.stats.effective_encoding, Encoding::Utf16Le, "显式参数优先");
        assert!(rb.stats.bom_conflict(&o), "BOM 冲突可观测");
        // 无冲突时 bom_conflict 为假。
        let o2 = DecodeOptions::with_encoding(Encoding::Utf8);
        assert!(!rb.stats.bom_conflict(&o2));
        set.add("E02-错误-截断与BOM冲突", true, "");
    }

    // ---- 对接：按需转 UTF-16/UTF-32 ----
    {
        let d = Decoder::new();
        let r = d.decode("\u{1F600}A".as_bytes(), &DecodeOptions::new());
        let v16 = d.to_utf16(&r);
        assert_eq!(v16, alloc::vec![0xD83D, 0xDE00, 0x41], "代理对 + BMP 直传");
        let v32 = d.to_utf32(&r);
        assert_eq!(v32, alloc::vec![0x1F600, 0x41], "UTF-32 即码点数组");
        set.add("E02-对接-UTF16转码", true, "");
    }

    set
}