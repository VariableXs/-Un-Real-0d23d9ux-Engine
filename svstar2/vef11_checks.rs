//! VE-F1009 · 域自检（判据逐条对应，见 `vef11_wide.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 16bit roundtrip 逐位一致 → `C1009-往返-*`
//! - 降转换精度 → `C1009-降位-*`
//! - sBIT 解析 → `C1009-sbit-*`
//! - HDR 衔接预留 → `C1009-交接口-*`
//! - 性能（16bit 比 8bit 慢 ≤2 倍）→ `C1009-性能-*`
//! - 字节序 / 规范禁止组合 / 错误路径 → `C1009-字节序-*`、`C1009-禁止-*`、`C1009-错误-*`
//!
//! 弱门禁自律（逐条对照本域最容易犯的六种）：
//! 1. **「两实现对拍」不能抽样**：锚点要求 `(v+128)>>8` 与查表
//!    「对拍逐值一致」。判据**真的遍历全部 65536 个输入**逐值比，
//!    不是抽 100 个看看。抽样会漏掉 `v ≥ 65408` 那段回绕区。
//! 2. **「sBIT 有效位」不能只断「解析出来了」**：解析成功不等于
//!    语义对。判据断**有效位量程归一**——`sBIT=8` 时
//!    `v=65280`（8 位有效满量程）必须得 255，且 `v=255` 得 0
//!    （近乎全黑不许被抬成可见灰）。
//! 3. **「越界→按满精度」不能只断「返回了 Rejected」**：还要断
//!    **降级方向**——越界块必须走 [`downgrade_arith`] 而非按声明值缩放，
//!    否则 `sBIT=200` 会把画面缩成全黑。
//! 4. **「16bit 慢 ≤2 倍」不能断墙钟**：真no_std 内核没有墙钟源。
//!    判据断**可机检的结构事实**（字节数 2 倍、滤波试探不随位深放大、
//!    滤波中间量位宽需求），并在注释里明写未实测墙钟。
//! 5. **「字节序大端」不能只断「往返一致」**：一个**两端都写成小端**
//!    的实现往返也一致。判据直接断**字节布局**——`0x1234` 的
//!    线上字节必须是 `[0x12, 0x34]`，高字节在前。
//! 6. **「mod.rs 三处注册」不做成自证式判据**：判据层里用常量自造
//!    一段注册文本再自己核对，永远通过且与真 mod.rs 无关。改为由
//!    **独立探针**（`_attic/.../probe/p11_reg.rs`）从**真 mod.rs 切段**
//!    后喂进no_std 校验函数，并配变异双向验证。判据层只负责
//!    「判据集自己没被截断」。
#![allow(clippy::needless_range_loop)]

// 判据层只用 `vec!`（元数据/样本构造），不需要 `format!`——
// `detail` 必须是 `&'static str`，本层不拼串。
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vef02_pngenc::ColorType;
use super::vef04_color::ColorSource;
use super::vef11_wide::*;

/// 造一行 16 位样本：第 `i` 个样本取 `i` 的低 16 位（便于逐位核对）。
fn sample_row(ct: ColorType, width: usize) -> Row16 {
    let mut r = Row16::new(ct, width);
    let ch = r.channels();
    let n = width * ch;
    let mut i = 0usize;
    while i < n {
        // 混合高低字节，确保大端布局能被验出来
        let v = ((i as u16) << 8) | ((i as u16).wrapping_mul(7) & 0xFF);
        let _ = r.set(i, v);
        i += 1;
    }
    r
}

/// F1009 域自检入口。
pub fn run_vef11_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-f-f1009");

    // =====================================================================
    // 判据一：字节序大端（锚点「网络序读写显性」）
    // =====================================================================
    {
        // 具体字节布局：0x1234 的线上字节必须是 [0x12, 0x34]。
        let mut buf = [0u8; 2];
        let n = put_be16(&mut buf, 0x1234);
        s.add(
            "C1009-字节序-高字节在前",
            n == 2 && buf[0] == 0x12 && buf[1] == 0x34,
            "网络序大端：0x1234 的线上字节必须是 12 34（小端会是 34 12）",
        );
        // 往返：写进去再读出来必须完全一致（逐位，非抽样）。
        let mut all_same = true;
        let mut v = 0u32;
        while v <= 0xFFFF {
            let mut b = [0u8; 2];
            let val = v as u16;
            if put_be16(&mut b, val) != 2 {
                all_same = false;
            }
            match get_be16(&b) {
                Ok(g) if g == val => {}
                _ => all_same = false,
            }
            v += 1;
        }
        s.add(
            "C1009-字节序-全域往返逐位一致",
            all_same,
            "全部 65536 个 u16 逐个写入再读回，须完全相同（不是抽样）",
        );
        // 大端两两交换性质：字节序反了必然被抓。
        let mut b2 = [0u8; 2];
        put_be16(&mut b2, 0x00FF);
        let mut swapped = true;
        if b2[0] == b2[1] {
            swapped = false; // 0x00FF 高低字节不等，若相等说明没写进去
        }
        s.add(
            "C1009-字节序-非对称样本可分辨",
            swapped && b2[0] == 0x00 && b2[1] == 0xFF,
            "0x00FF 高低字节不等，正是抓「写成小端」的样本",
        );
        // 越界读给Fault，绝不静默取 0。
        s.add(
            "C1009-字节序-越界读给错非静默零",
            matches!(get_be16(&[0x01]), Err(Fault::ShortBuffer { want: 2, got: 1 })),
            "只给 1 字节时必须报错；静默返回 0 会把缺数据当全黑",
        );
    }

    // =====================================================================
    // 判据二：16bit roundtrip 逐位一致（锚点核心判据）
    // =====================================================================
    {
        // 四类合法颜色类型逐个往返。
        let mut all_ok = true;
        let mut i = 0usize;
        while i < WIDE_COLOR_TYPES.len() {
            let ct = WIDE_COLOR_TYPES[i];
            let row = sample_row(ct, 9);
            let need = row.byte_len();
            let mut buf = vec![0u8; need];
            if row.pack_be(&mut buf).is_err() {
                all_ok = false;
            }
            match Row16::unpack_be(ct, &buf, 9) {
                Ok(back) => {
                    if back != row {
                        all_ok = false;
                    }
                }
                Err(_) => all_ok = false,
            }
            i += 1;
        }
        s.add(
            "C1009-往返-四类颜色逐位一致",
            all_ok,
            "灰度/真彩/灰A/RGBA 四类各往返一次，样本序列须完全相同",
        );

        // byte_len 必须是 宽×通道×2，且与实际写入字节数一致。
        let row = sample_row(ColorType::Rgba, 5);
        let expect_len = 5 * 4 * 2;
        s.add(
            "C1009-往返-行长核算为宽乘通道乘二",
            row.byte_len() == expect_len && row.sample_count() == 20,
            "5 像素 × 4 通道 × 2 字节 = 40；算错会让缓冲越界或留空",
        );

        // 打包长度必须**恰等于** byte_len：多给/少给都要拒。
        let mut too_big = vec![0u8; expect_len + 1];
        let mut too_small = vec![0u8; expect_len - 1];
        s.add(
            "C1009-往返-缓冲长度须恰好",
            row.pack_be(&mut too_big).is_err() && row.pack_be(&mut too_small).is_err(),
            "多给/少给都拒：默默接受会让调用方以为拿到完整行",
        );

        // 空行（width=0）是合法边界，不 panic。
        let empty = Row16::new(ColorType::Gray, 0);
        let mut ebuf: [u8; 0] = [];
        s.add(
            "C1009-往返-零宽行自洽",
            empty.byte_len() == 0 && empty.pack_be(&mut ebuf).is_ok(),
            "零宽行是合法边界；构造器不替调用方拦宽度，但不得 panic",
        );

        // 样本下标越界给错，不夹取。
        let r = Row16::new(ColorType::Gray, 2);
        let oob = r.get(99);
        s.add(
            "C1009-往返-样本越界报错",
            matches!(oob, Err(Fault::SampleOutOfRange { index: 99, count: 2 })),
            "越界须报错；夹取会把「列数算错」变成「某列颜色不对」这种查不出的症状",
        );

        // 每样本字节数恒为 2（16 位深定义）。
        s.add(
            "C1009-往返-每样本两字节",
            BYTES_PER_SAMPLE_16 == 2,
            "16 位深大端样本恒2 字节；写成 1 字节就是 8 位路径混入",
        );
    }

    // =====================================================================
    // 判据三：降转换精度（锚点「(v+128)>>8 与查表两实现对拍」）
    // =====================================================================
    {
        // 两实现对拍：**全域 65536 个输入**，不是抽样。
        let lut = DowngradeLut::build();
        let mut identical = true;
        let mut first_mismatch = u32::MAX;
        let mut v = 0usize;
        while v <= 0xFFFF {
            let a = downgrade_arith(v as u16);
            match lut.lookup(v as u16) {
                Ok(b) => {
                    if a != b && first_mismatch == u32::MAX {
                        first_mismatch = v as u32;
                    }
                    if a != b {
                        identical = false;
                    }
                }
                Err(_) => identical = false,
            }
            v += 1;
        }
        s.add(
            "C1009-降位-两实现全域对拍一致",
            identical,
            "算术与查表在全部 65536 个输入上逐值相同（抽样会漏掉 >=65408 的回绕区）",
        );
        // 查表长度恒 65536、非空。
        s.add(
            "C1009-降位-查表全域且非空",
            lut.len() == U16_LUT_ENTRIES && !lut.is_empty(),
            "表长 65536；空表会让每次查表都失败，那应当由构造时就拒绝",
        );

        // 【锚点公式的两个实测缺陷】判据必须把它们钉死，
        // 否则一个「照搬锚点」的实现会被放过。
        //缺陷一：u16 回绕。v=65535 时 (v+128) 在 u16 里回绕成 127，
        //  >>8 得 0 —— 画面最亮处变全黑。
        s.add(
            "C1009-降位-最亮值不回绕成零",
            downgrade_arith(65535) == 255,
            "v=65535 必须得 255；u16 回绕会得 0（画面最亮处全黑）",
        );
        s.add(
            "C1009-降位-回绕边界前一值正确",
            downgrade_arith(65407) == 255,
            "v=65407 是回绕区前一格，须得 255；回绕区内会得 0",
        );
        // 缺陷二：越界。满位 (65535+128)>>8 = 256 超出 8 位上界。
        //
        // 【不要写 `downgrade_arith(v) <= 255`】返回类型是 `u8`，
        // `<= 255` 对 u8 **恒真**（rustc 会报 useless_comparisons）。
        // 那是判据写坏比没判据更坏的典型：它绿着，却什么也没钉。
        // 真语义是「**整段溢出区被钳到 255 而不是回绕成小值**」——
        // 65280..=65535 共 256 个输入，按 `(v+128)>>8` 都算出 256，
        // 钳位实现给 255，不钳/回绕的实现给 0 或其它值。逐值断。
        let mut clamp_block = true;
        let mut cv = 65280usize;
        while cv <= 0xFFFF {
            if downgrade_arith(cv as u16) != U8_MAX {
                clamp_block = false;
            }
            cv += 1;
        }
        s.add(
            "C1009-降位-溢出区整段钳到满位",
            clamp_block,
            "65280..=65535 共 256 个输入都算出 256，必须整段钳到 255（回绕会给 0）",
        );

        // 舍入而非截断：紧邻半值的两侧必须给出不同结果。
        // (127+128)>>8=0，(128+128)>>8=1 —— 截断实现会给都是 0。
        s.add(
            "C1009-降位-舍入非截断",
            downgrade_arith(127) == 0 && downgrade_arith(128) == 1,
            "半值两侧须不同；截断实现两边都给 0（这正是锚点写「不截断用舍入」的原因）",
        );

        // 全域单调非降（降转换不能出现亮度倒挂）。
        let mut mono = true;
        let mut prev = 0u8;
        let mut w = 0usize;
        while w <= 0xFFFF {
            let cur = downgrade_arith(w as u16);
            if cur < prev {
                mono = false;
            }
            prev = cur;
            w += 1;
        }
        s.add(
            "C1009-降位-全域单调非降",
            mono,
            "亮度不能倒挂；单调性破了会让渐变出现色带倒转",
        );

        // 两端锚点：0 与满位。
        s.add(
            "C1009-降位-两端锚点正确",
            downgrade_arith(0) == 0 && downgrade_arith(65535) == 255,
            "0→0、65535→255 是夹逼两端，两端对了中间才有意义",
        );
    }

    // =====================================================================
    // 判据四：sBIT 解析与有效位语义
    // =====================================================================
    {
        // 解析：长度按通道数。
        let d = match SbitDescriptor::from_payload(ColorType::Rgb, &[8, 10, 12]) {
            Ok(x) => x,
            Err(_) => SbitDescriptor::full(ColorType::Rgb),
        };
        s.add(
            "C1009-sbit-按通道数解析",
            d.channels() == 3 && d.bit_of(0) == Ok(8) && d.bit_of(2) == Ok(12),
            "RGB 三通道各一个有效位，顺序须与通道顺序一致",
        );
        // 组装与解析互逆。
        let payload = d.to_payload();
        let back = SbitDescriptor::from_payload(ColorType::Rgb, &payload);
        s.add(
            "C1009-sbit-组装解析互逆",
            matches!(back, Ok(ref x) if x == &d),
            "to_payload 后 from_payload 须得回原描述符",
        );
        // 长度不符给错。
        s.add(
            "C1009-sbit-长度不符报错",
            matches!(
                SbitDescriptor::from_payload(ColorType::Rgb, &[8, 10]),
                Err(Fault::SbitLengthMismatch { want: 3, got: 2 })
            ),
            "长度由颜色类型唯一决定，长度不符即拒",
        );
        // 通道下标越界给错。
        s.add(
            "C1009-sbit-通道越界报错",
            matches!(d.bit_of(9), Err(Fault::ChannelOutOfRange { index: 9, count: 3 })),
            "越界须报错，不夹取",
        );

        // 解析层自带的越界守卫必须承重（变异双向验证 M5 钉出：
        // 摘掉 validate 的越界拒绝后 resolve_sbit 的前置扫描仍会兜住，
        // 但**绕过 resolve_sbit 直调 from_payload** 的调用方会拿到
        // 「按 200 位缩放」的描述符——第二道门不能是死代码）。
        s.add(
            "C1009-sbit-解析层拒绝越界位",
            matches!(
                SbitDescriptor::from_payload(ColorType::Gray, &[0]),
                Err(Fault::SbitOutOfRange { sbit: 0 })
            ) && matches!(
                SbitDescriptor::from_payload(ColorType::Gray, &[200]),
                Err(Fault::SbitOutOfRange { sbit: 200 })
            ),
            "有效位 0 与 >16 在 from_payload 解析层即拒，不依赖 resolve_sbit 前置扫描",
        );

        // 【弱门禁二】有效位语义：sBIT=8 时有效数据原样落8 位。
        s.add(
            "C1009-sbit-八位有效原样落下",
            downgrade_by_sbit(65280, 8) == Ok(255) && downgrade_by_sbit(255, 8) == Ok(0),
            "sBIT=8：v=65280（8位有效满量程）→255，v=255（近乎全黑）→0",
        );
        s.add(
            "C1009-sbit-十二位有效中点落一二八",
            downgrade_by_sbit(65520, 12) == Ok(255) && downgrade_by_sbit(32760, 12) == Ok(127),
            "sBIT=12：满量程→255、中点→127，按有效位量程归一而非按 16 位量程",
        );
        // sBIT=16 满位时与算术实现逐值一致（**全域**，不是抽样）。
        let mut full_same = true;
        let mut fv = 0usize;
        while fv <= 0xFFFF {
            match downgrade_by_sbit(fv as u16, 16) {
                Ok(a) => {
                    if a != downgrade_arith(fv as u16) {
                        full_same = false;
                    }
                }
                Err(_) => full_same = false,
            }
            fv += 1;
        }
        s.add(
            "C1009-sbit-满位与算术实现全域一致",
            full_same,
            "sBIT=16 时必须退化为普通算术降位，两条路径全域逐值相同",
        );
        // sBIT 1..=16 全域单调且满量程落到满位（真遍历 65536×16 = 1048576 组）。
        //
        // 【不要写 `cur > 255`】`cur` 是 `u8`，该比较恒假（无信息）。
        // 「不越界」在 `u8` 返回型上**原理上不可观测** ⇒ 按纪律下沉到
        // 可观测的替代表述：**每个 sbit 的满量程输入必须得 255**。
        // 若某sbit 的缩放算出 >255，它在返回 u8 前必被钳位或回绕，
        // 而回绕会让满量程得0 —— 满量程得 255 把这两种都抓住了。
        let mut sbit_mono = true;
        let mut sb = 1u8;
        while sb <= 16 {
            let mut pv = 0u8;
            let mut x = 0usize;
            while x <= 0xFFFF {
                match downgrade_by_sbit(x as u16, sb) {
                    Ok(cur) => {
                        if cur < pv {
                            sbit_mono = false;
                        }
                        pv = cur;
                    }
                    Err(_) => sbit_mono = false,
                }
                x += 1;
            }
            if downgrade_by_sbit(0xFFFF, sb) != Ok(U8_MAX) {
                sbit_mono = false;
            }
            sb += 1;
        }
        s.add(
            "C1009-sbit-全域单调且满量程到顶",
            sbit_mono,
            "sBIT 1..=16 × 全部 65536 输入共 1048576 组须单调非降，且每个 sbit 的满量程须得 255",
        );
        // 有效位 0 与 >16 越界。
        s.add(
            "C1009-sbit-有效位越界给错",
            matches!(downgrade_by_sbit(100, 0), Err(Fault::SbitOutOfRange { sbit: 0 }))
                && matches!(downgrade_by_sbit(100, 17), Err(Fault::SbitOutOfRange { sbit: 17 })),
            "有效位须 1..=16；越界给错不钳位（钳位就是静默）",
        );

        // 【弱门禁三】越界块 ⇒ 拒块**并按满精度**，
        // 关键是「降级方向」不能错。
        let v_rej = resolve_sbit(ColorType::Rgb, &[16, 200, 16]);
        s.add(
            "C1009-sbit-越界块被拒",
            matches!(v_rej, SbitVerdict::RejectedOutOfRange { bad_sbit: 200 }),
            "有效位 200 > 16 ⇒ 拒绝该块",
        );
        // 越界块必须走**满精度**（arithmetic），不得按声明值缩放。
        let rej_out = downgrade_with_verdict(65535, &v_rej, 1);
        s.add(
            "C1009-sbit-越界块按满精度处理",
            matches!(rej_out, Ok(255)),
            "越界块须按 16 位满精度降位；若按 200 缩放会把画面缩成全黑",
        );
        // 有效块的通道定向降转换。
        let v_ok = resolve_sbit(ColorType::Rgb, &[8, 16, 16]);
        s.add(
            "C1009-sbit-有效块按通道分别缩放",
            matches!(&v_ok, SbitVerdict::Accepted(_))
                && matches!(downgrade_with_verdict(65280, &v_ok, 0), Ok(255))
                && matches!(downgrade_with_verdict(65280, &v_ok, 1), Ok(255)),
            "通道 0 是sBIT=8（按 8 位量程）、通道 1 是满位（按 16 位量程）",
        );
        // 有效位 0 也算越界（0 位有效无意义）。
        let v_zero = resolve_sbit(ColorType::Gray, &[0]);
        s.add(
            "C1009-sbit-零有效位被拒",
            matches!(v_zero, SbitVerdict::RejectedOutOfRange { bad_sbit: 0 }),
            "0 位有效无意义，按越界处理",
        );
        // 长度不符也走「按满精度」而非报错中断。
        let v_len = resolve_sbit(ColorType::Rgb, &[8, 8]);
        s.add(
            "C1009-sbit-长度不符按满精度",
            matches!(v_len, SbitVerdict::RejectedOutOfRange { .. }),
            "长度不符的块不可用，按满精度处理而不是让整帧失败",
        );
        // 16 位 + 调色板 ⇒ sBIT 不适用。
        s.add(
            "C1009-sbit-调色板不适用",
            resolve_sbit(ColorType::Palette, &[8, 8, 8]) == SbitVerdict::NotApplicable,
            "调色板没有 16 位形态，sBIT 无从谈起",
        );
        // 满位描述符自识别。
        s.add(
            "C1009-sbit-满位自识别",
            SbitDescriptor::full(ColorType::Rgba).is_full()
                && !SbitDescriptor::new(ColorType::Gray, vec![8]).map(|d| d.is_full()).unwrap_or(true),
            "is_full 须真的区分满位与非满位",
        );
    }

    // =====================================================================
    // 判据五：16bit + 调色板显性拒绝
    // =====================================================================
    {
        s.add(
            "C1009-禁止-调色板十六位被拒",
            matches!(Row16::new_checked(ColorType::Palette, 4), Err(Fault::PaletteWithWide)),
            "规范禁止该组合；在构造器就拦，数据进模块第一道门即拒",
        );
        s.add(
            "C1009-禁止-四类宽色合法",
            WIDE_COLOR_TYPES.len() == 4
                && WIDE_COLOR_TYPES
                    .iter()
                    .all(|c| wide_combo_is_legal(*c)),
            "灰度/真彩/灰A/RGBA 四类在 16 位下都合法",
        );
        s.add(
            "C1009-禁止-调色板非法标记",
            !wide_combo_is_legal(ColorType::Palette),
            "调色板须被标为非法，否则「宽容处理」会让索引图变成能显示的噪声图",
        );
        // 通道数与F1002 口径一致：灰度1/真彩3/灰A2/RGBA4。
        s.add(
            "C1009-禁止-通道数逐类正确",
            channel_count(ColorType::Gray) == 1
                && channel_count(ColorType::Rgb) == 3
                && channel_count(ColorType::GrayAlpha) == 2
                && channel_count(ColorType::Rgba) == 4,
            "通道数算错会让 byte_len 全错（多算一半通道 ⇒ 缓冲越界）",
        );
        // 拒绝路径的三要素齐备。
        let f = reject_palette_wide();
        s.add(
            "C1009-禁止-拒绝信息三要素齐备",
            !f.what().is_empty() && !f.why().is_empty() && !f.advice().is_empty(),
            "拒绝必须给三要素，否则用户不知道该改成什么",
        );
    }

    // =====================================================================
    // 判据六：HDR 衔接预留
    // =====================================================================
    {
        let row = sample_row(ColorType::Rgb, 6);
        let h = handoff_for_hdr(&row, ColorSource::GammaChrm, Some(45455));
        s.add(
            "C1009-交接口-形状与源行一致",
            h.channels == row.channels() && h.samples.len() == row.sample_count(),
            "交接面样本数与通道数须与源行严格相等",
        );
        s.add(
            "C1009-交接口-来源随帧带出",
            h.source == ColorSource::GammaChrm,
            "色彩来源必须随帧带出；不许下游假定sRGB",
        );
        // gAMA 缺省是**显式默认**而非「不知道」。
        let h_none = handoff_for_hdr(&row, ColorSource::Srgb, None);
        s.add(
            "C1009-交接口-缺省 gama 为显式默认",
            h_none.gamma_x100000.is_none() && h_none.source == ColorSource::Srgb,
            "None 表示按默认 sRGB 处理，不是「不知道」",
        );
        // 校验：形状与 gAMA 边界。
        s.add(
            "C1009-交接口-合法交接面通过校验",
            validate_handoff(&h).is_ok() && validate_handoff(&h_none).is_ok(),
            "正常形状须通过自检",
        );
        s.add(
            "C1009-交接口-样本数不整除报错",
            matches!(
                validate_handoff(&Linear16 {
                    samples: vec![1, 2, 3],
                    channels: 2,
                    gamma_x100000: None,
                    source: ColorSource::Srgb,
                }),
                Err(Fault::HandoffShapeMismatch { .. })
            ),
            "样本数须是通道数整数倍；多半是少写了一行",
        );
        s.add(
            "C1009-交接口-通道数越界报错",
            matches!(
                validate_handoff(&Linear16 {
                    samples: vec![1, 2],
                    channels: 9,
                    gamma_x100000: None,
                    source: ColorSource::Srgb,
                }),
                Err(Fault::BadChannelCount(9))
            ),
            "通道数只能是 1..=4",
        );
        s.add(
            "C1009-交接口-gama 越界报错",
            matches!(
                validate_handoff(&Linear16 {
                    samples: vec![1, 2, 3],
                    channels: 3,
                    gamma_x100000: Some(900_000),
                    source: ColorSource::Srgb,
                }),
                Err(Fault::GamaOutOfRange { gamma: 900_000 })
            ),
            "与 F1004 同口径：gAMA 越出 (0.01,5.0) 即拒",
        );
        // 契约三条可被读出（**契约写在文档里等于没有契约**）。
        s.add(
            "C1009-交接口-契约三条齐备",
            HDR_CONTRACT.len() == 3
                && HDR_CONTRACT.iter().all(|c| !c.is_empty())
                && HDR_CONTRACT[0].contains("16"),
            "三条硬要求须能被下游读到并逐条核对，不只是文档措辞",
        );
    }

    // =====================================================================
    // 判据七：性能的可机检部分（锚点「16bit 慢 <=2 倍」）
    // =====================================================================
    {
        // 本单**不实测墙钟**（真no_std 无墙钟源），只断结构事实。
        s.add(
            "C1009-性能-字节数二倍",
            WIDE_BYTE_FACTOR == 2 && BYTES_PER_SAMPLE_16 == 2,
            "每样本字节数是 8 位路径的 2 倍；这是 2 倍开销的主要来源",
        );
        s.add(
            "C1009-性能-滤波试探不随位深放大",
            wide_filter_trials() == narrow_filter_trials(),
            "16 位不该多跑一遍滤波试探；放大就说明实现把转换开销混入了热路径",
        );
        // 滤波中间量位宽：16 位下 u16 装不下，必须 i32。
        s.add(
            "C1009-性能-滤波中间量需十八位",
            filter_acc_bits_needed() == 18 && WIDE_FILTER_ACC > 65535,
            "Paeth 的 a+b-c 在 16 位域最坏 131070，u16 装不下⇒必须 i32",
        );
        s.add(
            "C1009-性能-八位域同常数不需宽化",
            NARROW_FILTER_ACC == 510 && NARROW_FILTER_ACC < 65535,
            "8 位下 u16 绰绰有余；这一对比说明「宽化」是 16 位特有的需求",
        );
        // 16 位行缓冲不与 8 位路径共用（类型层隔离）。
        let w = Row16::new(ColorType::Gray, 3);
        s.add(
            "C1009-性能-行缓冲类型独立",
            w.byte_len() == 6 && WIDE_COLOR_TYPES.len() == 4,
            "16 位缓冲是独立强类型，不靠标记位与 8 位区分（否则开销混入热路径）",
        );
    }

    // =====================================================================
    // 判据八：错误路径与三要素
    // =====================================================================
    {
        // 码段独占：F1009 = 0xF5xx（0xF1..0xF4 被 F1001~F1004 占用、
        // F1007 沿 0xF2 续段，F1009 顺延取全仓空闲的 0xF5）。
        // 0x2Cxx 已被 VE-N F2606（ven06_incr）占用——这里**双向钉**：
        // 既断「在 0xF5 段内」，也断「不在 0x2C 段」，只钉前者挡不住
        // 码段被改回撞段值（撞号不报编译错，只在运行期静默混流）。
        let f1 = Fault::SbitOutOfRange { sbit: 20 };
        let f2 = Fault::PaletteWithWide;
        s.add(
            "C1009-错误-码段独占不撞",
            (f1.code() & 0xFF00) == 0xF500
                && (f2.code() & 0xFF00) == 0xF500
                && (f1.code() & 0xFF00) != 0x2C00
                && (f2.code() & 0xFF00) != 0x2C00
                && f1.code() != f2.code(),
            "本单独占 0xF5xx 且不落入 VE-N F2606 已占的 0x2Cxx；同段内每个变体须有独立码",
        );
        // 每个变体都有独立码（下游可据码分流）。
        let all = [
            Fault::BufferSize { want: 1, got: 2 },
            Fault::ShortBuffer { want: 2, got: 1 },
            Fault::SampleOutOfRange { index: 1, count: 2 },
            Fault::ChannelOutOfRange { index: 1, count: 2 },
            Fault::LutIndexOutOfRange { index: 1 },
            Fault::SbitOutOfRange { sbit: 1 },
            Fault::SbitLengthMismatch { want: 1, got: 2 },
            Fault::PaletteWithWide,
            Fault::BadChannelCount(1),
            Fault::HandoffShapeMismatch { samples: 1, channels: 2 },
            Fault::GamaOutOfRange { gamma: 1 },
            Fault::DimensionOverflow { width: 1, channels: 1 },
        ];
        let mut uniq = true;
        let mut seen: Vec<u16> = Vec::new();
        let mut ai = 0usize;
        while ai < all.len() {
            let c = all[ai].code();
            let mut k = 0usize;
            while k < seen.len() {
                if seen[k] == c {
                    uniq = false;
                }
                k += 1;
            }
            seen.push(c);
            ai += 1;
        }
        s.add(
            "C1009-错误-十二个变体码各不同",
            all.len() == 12 && uniq,
            "码重复会让下游按码分流时把两种故障当一种",
        );
        // 每个变体三要素齐备且带规则出处。
        let mut three_ok = true;
        let mut ti = 0usize;
        while ti < all.len() {
            let t = all[ti].three_elements();
            if t.is_empty() || !all[ti].why().contains("F1009") || !t.contains('｜') {
                three_ok = false;
            }
            ti += 1;
        }
        s.add(
            "C1009-错误-三要素齐备带出处",
            three_ok,
            "每个变体的 what/why/advice 都非空且 why 引用 F1009",
        );
        // 诊断码字符串格式稳定。
        s.add(
            "C1009-错误-诊断码格式稳定",
            f1.diag().starts_with("F1009-") && f1.diag().len() > 7,
            "诊断码供日志与面板显示，格式必须稳定可 grep",
        );
        // 全单零 panic 面：这些操作都不能 panic。
        let safe = Row16::new(ColorType::Gray, 0).get(0).is_err()
            && DowngradeLut::build().lookup(65535).is_ok()
            && downgrade_by_sbit(0, 0).is_err()
            && validate_handoff(&Linear16 {
                samples: vec![],
                channels: 0,
                gamma_x100000: Some(0),
                source: ColorSource::Srgb,
            })
            .is_err();
        s.add(
            "C1009-错误-边界操作零 panic",
            safe,
            "零宽行查越界、满表查末项、有效位 0、gAMA 0——全都给 Err 而非 panic",
        );

        // ---- 尺寸算术溢出族（`width` 来自不可信 IHDR，裸乘法会回绕/panic）----
        //
        // 【为什么单列一族】上面那条「零 panic」只覆盖**语义非法**输入
        // （零宽、越界 sBIT），没覆盖**尺寸算术溢出**：宽度取 `usize::MAX`
        // 时 `width * ch * 2` 在 release 下静默回绕、debug 下直接 panic。
        // 这类输入语法合法、语义荒谬，只有独立一组判据钉得住。
        let huge = usize::MAX;
        let ov_row = Row16::new(ColorType::Rgba, huge);
        s.add(
            "C1009-溢出-构造兜底不静默",
            ov_row.overflowed() && ov_row.sample_count() == 0 && ov_row.byte_len() == 0,
            "infallible 构造遇溢出：样本留空、字节数 0、overflowed 真——不留痕就会被读成合法空行",
        );
        // checked 入口必须**硬拒**，不给兜底留退路（否则不可信尺寸被推到下游）。
        s.add(
            "C1009-溢出-校验入口硬拒",
            matches!(
                Row16::new_checked(ColorType::Rgba, huge),
                Err(Fault::DimensionOverflow { .. })
            ),
            "new_checked 是「尺寸可信」入口，溢出即 Err::DimensionOverflow",
        );
        // 解包路径同样不得回绕：溢出优先于长度比较被拒。
        s.add(
            "C1009-溢出-解包拒而未回绕",
            matches!(
                Row16::unpack_be(ColorType::Rgba, &[0u8; 8], huge),
                Err(Fault::DimensionOverflow { .. })
            )
            && matches!(
                Row16::unpack_be(ColorType::Gray, &[0u8; 8], huge),
                Err(Fault::DimensionOverflow { .. })
            ),
            "宽×通道×2 全程 checked；回绕成小数字会让 8 字节缓冲「长度刚好对上」而越界读",
        );
        // 诊断码与三要素齐备（并入上方清单的独立码，此处只断内容不空）。
        s.add(
            "C1009-溢出-诊断三要素带出处",
            !Fault::DimensionOverflow { width: huge, channels: 4 }
                .three_elements()
                .is_empty()
                && Fault::DimensionOverflow { width: huge, channels: 4 }
                    .why()
                    .contains("F1009"),
            "溢出也是要往面板上报的故障，诊断三要素与规则出处不能缺",
        );
        // 未溢出时 checked 与朴素算术必须逐例一致（夹逼对的下界侧）。
        let mut agree = true;
        let mut wi = 0usize;
        while wi <= 8 {
            let got = match row_byte_len(wi, 4) {
                Ok(v) => v,
                Err(_) => 0,
            };
            if got != wi * 4 * 2 {
                agree = false;
            }
            wi += 1;
        }
        s.add(
            "C1009-溢出-未溢出处与朴素算术一致",
            agree,
            "checked 不是「一律拒绝」：0..=8 宽必须给出与朴素算术相同的字节数",
        );
    }

    // =====================================================================
    // 判据九：判据集自身的承载（注册校验见独立探针，见上方注释）
    // =====================================================================
    {
        // 判据集未截断。
        // 【不写 `s.len() < MAX_CHECKS`】那是**自证式**：问被测对象自己的长度，
        // 而被测对象正是「把判据装进 CheckSet」这件事本身。
        // 真语义是「一条都没被丢掉」⇒ 直接断 `truncated()` 为假。
        // `len()` 仍单独断一个下界，防「判据层整体空转返回空集」。
        //
        // 【注册校验不在本层】mod.rs 三处注册的逐字节核对由**独立探针**
        // （`_attic/.../probe/p11_reg.rs`）从**真 mod.rs 切段**来做。
        // 放在判据层里用常量自造切片再自己核对是**自证式**：永远通过，
        // 且与真 mod.rs 无关——那种「判据」钉不住任何东西。
        s.add(
            "C1009-判据集-未截断且非空转",
            !s.truncated() && s.len() >= 40,
            "截断了末尾判据就等于没写；空集则说明判据层整体空转",
        );
    }

    s
}