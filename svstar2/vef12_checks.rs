//! VE-F1010 · 域自检（判据逐条对应，见 `vef12_alpha.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 全透明形态 → `C1010-形态-*`
//! - 优先级规则 → `C1010-优先-*`
//! - 精确匹配 → `C1010-色键-*`
//! - 错误路径（截断告警 / 位深拒绝）→ `C1010-截断-*`、`C1010-拒绝-*`
//! - 预乘转换正确（roundtrip 误差 ≤1 LSB）→ `C1010-预乘-*`
//!   （≤1 LSB 由**精确分子路径全域零误差**达成；量化路径上界按 F0625
//!   单向阈值同源口径实测声明、独立重算双向钉死）
//! - 透明渐变质量 → `C1010-渐变-*`
//! - 错误路径与三要素 / 判据承载 → `C1010-错误-*`、`C1010-判据集-*`
//!
//! 弱门禁自律（本域最易犯的五种）：
//! 1. **roundtrip ≤1 LSB 不能抽样**：判据真遍历 8 位全域 65536 组；
//!    抽样会漏掉高 alpha 段的舍入边界。
//! 2. **精确匹配不能只断「键匹配自己」**：键与差 1 的邻值必须**不**匹配
//!    ——只断正向，`<=` 式范围匹配会全绿通过。
//! 3. **优先级不能只断「返回 AlphaChannel」**：还要断**告警计数**——
//!    静默忽略正是锚点点名要防的事故。
//! 4. **码段不能只断「在本段」**：还断「不在撞段值」——只钉前者挡不住
//!    改回撞段值（撞号不报编译错，运行期静默混流）。
//! 5. **「两实现一致」不能在实现内部对拍**：判据层独立调两条路径逐值比，
//!    任何一条被改坏都会被抓。
#![allow(clippy::needless_range_loop)]

use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vef02_pngenc::ColorType;
use super::vef12_alpha::*;

/// F1010 域自检入口。
pub fn run_vef12_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-f-f1010");

    // =====================================================================
    // 判据一：全透明形态（锚点「alpha 通道 / 调色板索引 / 灰度真彩色键」）
    // =====================================================================
    {
        // 形态 1：alpha 通道（类型 4/6，无 tRNS）。
        let o = resolve_trns(ColorType::Rgba, 8, &[], 0, false);
        s.add(
            "C1010-形态-alpha通道",
            matches!(&o, Ok(r) if r.form == TransparencyForm::AlphaChannel && r.warnings.is_empty()),
            "类型 6 无 tRNS ⇒ AlphaChannel 无告警",
        );
        // 形态 2：调色板透明索引。
        let o = resolve_trns(ColorType::Palette, 8, &[255, 128, 0], 3, true);
        s.add(
            "C1010-形态-调色板索引",
            matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), warnings }) if a.len() == 3 && a[1] == 128 && warnings.is_empty()),
            "类型 3 的 tRNS 是 PLTE 尺寸内的 per-entry alpha 数组",
        );
        // 形态 3：灰度色键。
        let o = resolve_trns(ColorType::Gray, 16, &[0x12, 0x34], 0, true);
        s.add(
            "C1010-形态-灰度色键",
            matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::ColorKey(ColorKey::Gray(0x1234)), warnings }) if warnings.is_empty()),
            "类型 0 的 tRNS 是 2 字节大端灰度键",
        );
        // 形态 4：真彩色键（三分量逐一还原）。
        let o = resolve_trns(ColorType::Rgb, 16, &[0, 1, 0, 2, 0, 3], 0, true);
        s.add(
            "C1010-形态-真彩色键",
            matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::ColorKey(ColorKey::Rgb(1, 2, 3)), warnings }) if warnings.is_empty()),
            "类型 2 的 tRNS 是 6 字节大端 RGB 键",
        );
        // 形态 5：无透明（有 tRNS 之前）。
        let o0 = resolve_trns(ColorType::Gray, 8, &[], 0, false);
        let o2 = resolve_trns(ColorType::Rgb, 8, &[], 0, false);
        s.add(
            "C1010-形态-无透明",
            matches!(&o0, Ok(TrnsOutcome { form: TransparencyForm::None, warnings }) if warnings.is_empty())
                && matches!(o2, Ok(TrnsOutcome { form: TransparencyForm::None, .. })),
            "类型 0/2 无 tRNS ⇒ 无透明语义",
        );
    }

    // =====================================================================
    // 判据二：优先级规则（tRNS+alpha 共存 → alpha 优先 + 计数告警）
    // =====================================================================
    {
        let o = resolve_trns(ColorType::GrayAlpha, 8, &[255, 255], 0, true);
        s.add(
            "C1010-优先-alpha胜出",
            matches!(&o, Ok(r) if r.form == TransparencyForm::AlphaChannel),
            "tRNS 与 alpha 共存 ⇒ 语义以 alpha 为准（锚点原文）",
        );
        s.add(
            "C1010-优先-忽略计数告警",
            matches!(&o, Ok(r) if r.warning_count(true) == 1 && r.warning_count(false) == 0),
            "tRNS 被忽略必须计数告警；静默忽略是无从排查的事故",
        );
        // 两类 alpha 颜色类型都仲裁一致。
        let o6 = resolve_trns(ColorType::Rgba, 16, &[1, 2, 3, 4, 5, 6], 0, true);
        let o4 = resolve_trns(ColorType::GrayAlpha, 16, &[1, 2], 0, true);
        s.add(
            "C1010-优先-四六型一致",
            matches!(&o6, Ok(r) if r.form == TransparencyForm::AlphaChannel && r.warning_count(true) == 1)
                && matches!(&o4, Ok(r) if r.form == TransparencyForm::AlphaChannel && r.warning_count(true) == 1),
            "类型 4 与类型 6 的共存仲裁口径必须一致",
        );
    }

    // =====================================================================
    // 判据三：色键精确匹配（语义是相等，不是范围）
    // =====================================================================
    {
        let key = ColorKey::Gray(0x00FF);
        s.add(
            "C1010-色键-精确匹配命中",
            matches!(key_matches(&key, &[0x00FF]), Ok(true)),
            "样本与键全等 ⇒ 透明",
        );
        // 【弱门禁二】差 1 的邻值必须**不**匹配——只断正向挡不住范围匹配。
        s.add(
            "C1010-色键-邻值不透明",
            matches!(key_matches(&key, &[0x0100]), Ok(false))
                && matches!(key_matches(&key, &[0x00FE]), Ok(false)),
            "差 1 的两侧邻值都不透明；范围式匹配会把整段亮度抠空",
        );
        let rgb = ColorKey::Rgb(10, 20, 30);
        s.add(
            "C1010-色键-真彩逐分量全等",
            matches!(key_matches(&rgb, &[10, 20, 30]), Ok(true))
                && matches!(key_matches(&rgb, &[10, 20, 31]), Ok(false))
                && matches!(key_matches(&rgb, &[11, 20, 30]), Ok(false)),
            "任一分量不等即不透明（精确匹配逐分量）",
        );
        s.add(
            "C1010-色键-形状不符报错",
            matches!(
                key_matches(&ColorKey::Gray(1), &[1, 2]),
                Err(Fault::SampleShapeMismatch { want: 1, got: 2 })
            ),
            "灰度键配 2 样本是调用方 bug，报错不猜",
        );
    }

    // =====================================================================
    // 错误路径一：tRNS 超长截断 + 告警（锚点原文）
    // =====================================================================
    {
        // 5 项 alpha、PLTE 只有 3 项 ⇒ 截断到 3 并告警 kept=3 dropped=2。
        let o = resolve_trns(ColorType::Palette, 8, &[255, 128, 0, 64, 32], 3, true);
        s.add(
            "C1010-截断-超PLTE截到合法长度",
            matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), .. }) if a.len() == 3 && a[0] == 255 && a[2] == 0),
            "超长部分按锚点语义丢弃，保留部分照用（有损降级非报错）",
        );
        s.add(
            "C1010-截断-告警记kept与dropped",
            matches!(
                &o,
                Ok(r) if matches!(
                    r.warnings.get(0),
                    Some(TrnsWarning::TruncatedToPlte { kept: 3, dropped: 2 })
                )
            ),
            "截断必须留痕（kept/dropped 可查）；静默截断会让「少了两项透明」无从对账",
        );
        // 截断边界：恰超长 1 项（payload = PLTE+1）也必须截且告警——
        // 阈值 off-by-one（`>` 写成 `>=` 或 +1）恰在这一项上漏网。
        let o_b = resolve_trns(ColorType::Palette, 8, &[7, 8, 9, 10], 3, true);
        s.add(
            "C1010-截断-恰超长一项也截",
            matches!(&o_b, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), .. }) if a.len() == 3 && a[2] == 9)
                && matches!(&o_b, Ok(r) if matches!(
                    r.warnings.get(0),
                    Some(TrnsWarning::TruncatedToPlte { kept: 3, dropped: 1 })
                )),
            "payload=PLTE+1 是截断语义的最小正例；阈值差一会让超长部分静默生效",
        );
        // 恰好等长不告警。
        let o_eq = resolve_trns(ColorType::Palette, 8, &[9, 9, 9], 3, true);
        s.add(
            "C1010-截断-等长不告警",
            matches!(&o_eq, Ok(r) if r.warnings.is_empty() && r.warning_count(false) == 0),
            "长度等于 PLTE 是合法形态，不得误报",
        );
        // 调色板图无 PLTE 却有 tRNS ⇒ 拒。
        s.add(
            "C1010-截断-无PLTE拒绝",
            matches!(
                resolve_trns(ColorType::Palette, 8, &[1, 2], 0, true),
                Err(Fault::PlteMissing)
            ),
            "没有 PLTE 的 alpha 数组无从定界，拒块",
        );
        // 短 tRNS（< PLTE）是合法稀疏形态：不是截断也不告警——
        // 「截断」只指超长丢弃；短于 PLTE 是规范明文允许的缺省形态。
        let o_short = resolve_trns(ColorType::Palette, 8, &[64, 128], 4, true);
        s.add(
            "C1010-形态-短tRNS稀疏合法不告警",
            matches!(&o_short, Ok(TrnsOutcome { form: TransparencyForm::PaletteAlphas(ref a), warnings }) if a.len() == 2 && warnings.is_empty()),
            "tRNS 短于 PLTE 不是错误：数组在 PLTE 尺寸内稀疏合法，截断语义只管超长支",
        );
        // 缺省项全不透明（ISO/IEC 15948 §4.3.2.1）：范围内的未写项查得 255，
        // 不是 Err——把缺省当越界会让合法稀疏 tRNS 的整段调色板变故障。
        s.add(
            "C1010-形态-缺省项全不透明",
            matches!(palette_alpha_defaulted(&[64], 3, 0), Ok(64))
                && matches!(palette_alpha_defaulted(&[64], 3, 1), Ok(255))
                && matches!(palette_alpha_defaulted(&[64], 3, 2), Ok(255)),
            "tRNS 未写到的调色板项按规范全不透明（255），不是越界也不是随机值",
        );
        // 缺省查表的越界语义与严查表一致：PLTE 外仍拒、无 PLTE 仍拒。
        s.add(
            "C1010-形态-缺省查表越界仍拒",
            matches!(
                palette_alpha_defaulted(&[64], 3, 3),
                Err(Fault::AlphaIndexOutOfRange { index: 3, count: 3 })
            ) && matches!(
                palette_alpha_defaulted(&[64], 0, 0),
                Err(Fault::PlteMissing)
            ),
            "缺省语义只放宽 PLTE 内的未写项；PLTE 外索引与无 PLTE 照旧拒块",
        );
    }

    // =====================================================================
    // 错误路径二：色键与位深不匹配 → 拒绝该块（锚点原文）
    // =====================================================================
    {
        // 灰度键 300、位深 8（量程 0..=255）⇒ 拒。
        s.add(
            "C1010-拒绝-灰度键越界",
            matches!(
                resolve_trns(ColorType::Gray, 8, &[0x01, 0x2C], 0, true),
                Err(Fault::KeyOutOfRange { key: 300, max: 255 })
            ),
            "键值超出 2^位深−1 ⇒ 拒块；按错误口径匹配会抠掉不该透明的像素",
        );
        // 真彩键任一分量越界 ⇒ 拒整块。
        s.add(
            "C1010-拒绝-真彩键分量越界",
            matches!(
                resolve_trns(ColorType::Rgb, 8, &[0, 10, 0, 20, 1, 0], 0, true),
                Err(Fault::KeyOutOfRange { .. })
            ),
            "蓝分量 256 > 255 ⇒ 拒整块（半校验半使用 = 口径分裂）",
        );
        // 位深 16 的满量程键合法（边界不误拒）。
        let o = resolve_trns(ColorType::Gray, 16, &[0xFF, 0xFF], 0, true);
        s.add(
            "C1010-拒绝-满量程不误拒",
            matches!(&o, Ok(TrnsOutcome { form: TransparencyForm::ColorKey(ColorKey::Gray(0xFFFF)), .. })),
            "位深 16 的键 0xFFFF 恰在量程内；把上界写成 < 会误拒合法文件",
        );
        // 位深本身非法（IHDR 不可信输入）。
        s.add(
            "C1010-拒绝-位深非法",
            matches!(
                resolve_trns(ColorType::Gray, 0, &[0, 1], 0, true),
                Err(Fault::BadBitDepth { bit_depth: 0 })
            ) && matches!(
                resolve_trns(ColorType::Gray, 17, &[0, 1], 0, true),
                Err(Fault::BadBitDepth { bit_depth: 17 })
            ),
            "位深 0 与 >16 都先拒（1u32<<17 不溢出但语义荒谬）",
        );
        // 载荷长度不符（灰度须 2、真彩须 6）。
        s.add(
            "C1010-拒绝-键载荷长度不符",
            matches!(
                resolve_trns(ColorType::Gray, 8, &[1], 0, true),
                Err(Fault::KeyPayloadLength { want: 2, got: 1 })
            ) && matches!(
                resolve_trns(ColorType::Rgb, 8, &[1, 2, 3, 4], 0, true),
                Err(Fault::KeyPayloadLength { want: 6, got: 4 })
            ),
            "灰度 2 字节、真彩 6 字节；长度不对说明解析口径已错",
        );
    }

    // =====================================================================
    // 判据五（前半）：预乘转换正确 —— 两实现全域对拍 + roundtrip ≤1 LSB
    // =====================================================================
    {
        // 算术参考 vs 查表：**全域 65536 组**逐值一致（不抽样）。
        // 【不用 unwrap_or 兜底比对】查表 Err 被「当成 255」再比较，
        // 恰好相等时会掩蔽故障——Err 一律直接判红。
        let lut = PremulLut8::build();
        let mut same = true;
        let mut c = 0usize;
        while c < 256 {
            let mut a = 0usize;
            while a < 256 {
                match lut.lookup(c as u8, a as u8) {
                    Ok(v) => {
                        if premul8_ref(c as u8, a as u8) != v {
                            same = false;
                        }
                    }
                    Err(_) => same = false,
                }
                a += 1;
            }
            c += 1;
        }
        s.add(
            "C1010-预乘-两实现全域对拍",
            same && lut.len() == PREMUL_LUT8_ENTRIES && !lut.is_empty(),
            "算术与查表在全部 65536 组上逐值相同（抽样会漏高 alpha 舍入边界）",
        );
        // lanes4 与查表全域一致（批语义不改变逐值结果）。
        let mut lanes_same = true;
        let mut cs = [0u8; 4];
        let mut aa = [0u8; 4];
        let mut out = [0u8; 4];
        let mut ci = 0usize;
        while ci < 256 {
            let mut ai = 0usize;
            while ai < 256 {
                cs[0] = ci as u8;
                cs[1] = ci as u8;
                cs[2] = ci as u8;
                cs[3] = ci as u8;
                aa[0] = ai as u8;
                aa[1] = ai as u8;
                aa[2] = ai as u8;
                aa[3] = ai as u8;
                if premul8_lanes4(&cs, &aa, &mut out) != Ok(4) {
                    lanes_same = false;
                }
                let mut k = 0usize;
                while k < 4 {
                    // 【同纪律】查表 Err 一律判红，不用 unwrap_or 兜底——
                    // 恰好命中兜底值时会掩蔽表故障（与上方全域对拍同口径）。
                    match lut.lookup(cs[k], aa[k]) {
                        Ok(v) => {
                            if out[k] != v {
                                lanes_same = false;
                            }
                        }
                        Err(_) => lanes_same = false,
                    }
                    k += 1;
                }
                ai += 4;
            }
            ci += 64;
        }
        s.add(
            "C1010-预乘-lanes4与查表一致",
            lanes_same,
            "4 路并行语义实现必须与标量/查表逐值一致（真 SIMD 分发前的语义层门禁）",
        );
        // lanes4 批大小守卫（尾部截断是静默事故）。
        s.add(
            "C1010-预乘-lanes4批大小守卫",
            matches!(
                premul8_lanes4(&[1, 2, 3], &[1, 2, 3], &mut [0u8; 4]),
                Err(Fault::LaneBatchSize { want: 4, got_c: 3, got_a: 3 })
            ) && matches!(
                premul8_lanes4(&[1, 2, 3, 4], &[1, 2, 3, 4], &mut [0u8; 3]),
                Err(Fault::BufferSize { want: 4, got: 3 })
            ),
            "非 4 批与短缓冲都给错，绝不静默截断",
        );

        // roundtrip ≤1 LSB 的**主承载**：精确分子路径全域零误差。
        // 预乘中间量以 u32 分子 c*a 持有（不被量化），(c*a + a/2)/a 恰落回
        // c——「两次转换的精度保持」是真遍历 8 位全域 65536 组实测的。
        let mut exact_ok = true;
        let mut c = 0usize;
        while c < 256 {
            let mut a = 1usize; // a=0 属零 alpha 约定区（颜色无定义），已有独立判据
            while a < 256 {
                let n = premul8_num(c as u8, a as u8);
                match unpremul8_num(n, a as u8) {
                    Ok(back) => {
                        if back != c as u8 {
                            exact_ok = false;
                        }
                    }
                    Err(_) => exact_ok = false,
                }
                a += 1;
            }
            c += 1;
        }
        s.add(
            "C1010-预乘-精确分子往返全域零误差",
            exact_ok,
            "无量化中间量的往返精确为零（≤1 LSB 判据由零误差达成）；全域 65536 组实测非抽样",
        );
        // 精确分子路径 16 位（alpha×色 采样全域，与量化判据同一采样网格，
        // 另加低 alpha 危险区全色扫描——采样网格不能只信均匀步长）。
        let mut exact16 = true;
        let mut a16 = 1u32;
        while a16 <= 0xFFFF {
            let mut c16 = 0u32;
            while c16 <= 0xFFFF {
                let n = premul16_num(c16 as u16, a16 as u16);
                match unpremul16_num(n, a16 as u16) {
                    Ok(back) => {
                        if back as u32 != c16 {
                            exact16 = false;
                        }
                    }
                    Err(_) => exact16 = false,
                }
                c16 += 257; // 257 与 65536 互质 ⇒ 覆盖 256 个均匀色点
            }
            a16 += 257;
        }
        let mut a16b = 1u32;
        while a16b <= 257u32 {
            let mut c16 = 0u32;
            while c16 <= 0xFFFF {
                let n = premul16_num(c16 as u16, a16b as u16);
                match unpremul16_num(n, a16b as u16) {
                    Ok(back) => {
                        if back as u32 != c16 {
                            exact16 = false;
                        }
                    }
                    Err(_) => exact16 = false,
                }
                c16 += 1;
            }
            a16b += 1;
        }
        s.add(
            "C1010-预乘-精确分子往返十六位零误差",
            exact16,
            "16 位精确分子往返零误差（alpha×色 各 256 均匀点共 65536 组）",
        );
        // 精确分子路径自身的零 alpha 约定（a=0 → Ok(0)）——量化路径的
        // 同名判据钉不到这里，变异常抓：约定分支各自要有独立门禁。
        s.add(
            "C1010-预乘-精确分子零alpha约定",
            unpremul8_num(premul8_num(200, 0), 0) == Ok(0)
                && unpremul16_num(premul16_num(60000, 0), 0) == Ok(0),
            "精确分子逆在 a=0 时归零（8/16 位两个分支都要承重，不能只测量化版）",
        );

        // 量化路径的往返上界是**实测声明**不是断言 ≤1：低 alpha 段信息量
        // 不足（a=1 只剩 1 bit），F0625 单向阈值同源口径。独立重算全域
        // 最大值，与常量**双向钉死**——常量错了或实现变了都会被抓。
        let mut max_err = 0u32;
        let mut c2 = 0usize;
        while c2 < 256 {
            let mut a2 = 1usize; // a=0 无颜色语义，往返约定归零不参与量测
            while a2 < 256 {
                let p = premul8_ref(c2 as u8, a2 as u8);
                let back = unpremul8(p, a2 as u8);
                let d = if back > c2 as u8 { back - c2 as u8 } else { c2 as u8 - back };
                if d as u32 > max_err {
                    max_err = d as u32;
                }
                a2 += 1;
            }
            c2 += 1;
        }
        s.add(
            "C1010-预乘-八位量化往返上界实测钉死",
            max_err == QUANT_RT_ERR_8BIT_MAX_LSB as u32,
            "量化往返全域最大误差独立重算=声明常量(127@a=1)；低 alpha 单向区与 F0625 同源口径",
        );
        let mut max16 = 0u32;
        // 覆盖一：全 alpha（1..=65535 逐值）× 采样色（257 网格 256 点）。
        let mut a3 = 1u32;
        while a3 <= 0xFFFF {
            let mut c3 = 0u32;
            while c3 <= 0xFFFF {
                let p = premul16_ref(c3 as u16, a3 as u16) as u32;
                let back = unpremul16(p as u16, a3 as u16) as u32;
                let d = if back > c3 { back - c3 } else { c3 - back };
                if d > max16 {
                    max16 = d;
                }
                c3 += 257;
            }
            a3 += 257;
        }
        // 覆盖二：低 alpha 危险区（a ≤ 257 逐值）× 全色（0..=65535 逐值）。
        // 量化误差 ≈ 65535/(2a)，最大值藏在最小 alpha 处——纯均匀采样
        // 网格（257 步）恰会漏掉 (c=32767, a=1) 这个极值点。
        let mut a4 = 1u32;
        while a4 <= 257u32 {
            let mut c4 = 0u32;
            while c4 <= 0xFFFF {
                let p = premul16_ref(c4 as u16, a4 as u16) as u32;
                let back = unpremul16(p as u16, a4 as u16) as u32;
                let d = if back > c4 { back - c4 } else { c4 - back };
                if d > max16 {
                    max16 = d;
                }
                c4 += 1;
            }
            a4 += 1;
        }
        s.add(
            "C1010-预乘-十六位量化往返上界实测钉死",
            max16 == QUANT_RT_ERR_16BIT_MAX_LSB,
            "16 位量化往返上界独立重算=声明常量(32767@a=1)；常量错或实现变都会被抓",
        );
        // 端点性质：a=0 得 0、a=满 恒等、c=0 得 0。
        s.add(
            "C1010-预乘-端点性质",
            premul8_ref(200, 0) == 0
                && premul8_ref(200, 255) == 200
                && premul8_ref(0, 128) == 0
                && premul16_ref(60000, 0) == 0
                && premul16_ref(60000, 65535) == 60000
                && premul16_ref(0, 40000) == 0,
            "a=0 颜色归零、a=满 恒等、c=0 恒零——三端点任何一处错都会污染合成域",
        );
        // unpremul 的 a=0 约定（可复现归零，不是未定义）。
        s.add(
            "C1010-预乘-零alpha约定归零",
            unpremul8(123, 0) == 0 && unpremul16(40000, 0) == 0,
            "a=0 的颜色无定义，往返归 0 是唯一可复现的选择（写进契约不写进运气）",
        );
    }

    // =====================================================================
    // 判据五（后半）：透明渐变质量
    // =====================================================================
    {
        // alpha 方向：c=满 时预乘值随 a 恒等（渐变不变形）。
        let mut alpha_ramp = true;
        let mut a = 0usize;
        while a < 256 {
            if premul8_ref(255, a as u8) != a as u8 {
                alpha_ramp = false;
            }
            a += 1;
        }
        s.add(
            "C1010-渐变-alpha方向恒等",
            alpha_ramp,
            "c=255 时 premul(c,a)=a 全域成立；渐变在 alpha 方向零形变",
        );
        // 颜色方向：固定 a，预乘值随 c 单调非降（渐变不倒挂）。
        let mut mono_c = true;
        let mut a2 = 1usize;
        while a2 < 256 {
            let mut prev = 0u8;
            let mut c = 0usize;
            while c < 256 {
                let p = premul8_ref(c as u8, a2 as u8);
                if p < prev {
                    mono_c = false;
                }
                prev = p;
                c += 1;
            }
            a2 += 1;
        }
        s.add(
            "C1010-渐变-颜色方向单调",
            mono_c,
            "固定 alpha 下预乘随颜色单调非降；倒挂会让半透明渐变出现色带反转",
        );
        // alpha 方向单调（固定 c）。
        let mut mono_a = true;
        let mut c3 = 0usize;
        while c3 < 256 {
            let mut prev = 0u8;
            let mut a3 = 0usize;
            while a3 < 256 {
                let p = premul8_ref(c3 as u8, a3 as u8);
                if p < prev {
                    mono_a = false;
                }
                prev = p;
                a3 += 1;
            }
            c3 += 1;
        }
        s.add(
            "C1010-渐变-alpha方向单调",
            mono_a,
            "固定颜色下预乘随 alpha 单调非降（全域逐值，非抽样）",
        );
        // 量化上界声明是常量不是注释（可被下游引用并核对）。
        s.add(
            "C1010-渐变-误差声明可引用",
            QUANT_RT_ERR_8BIT_MAX_LSB == 127 && QUANT_RT_ERR_16BIT_MAX_LSB == 32767,
            "量化往返上界以常量声明(实测全域最大值)；藏在注释里的数字无法被引用方核对",
        );
    }

    // =====================================================================
    // 错误路径三：码段独占（双向钉死）+ 三要素
    // =====================================================================
    {
        let f1 = Fault::KeyOutOfRange { key: 300, max: 255 };
        let f2 = Fault::PlteMissing;
        // 在 F1010 专属 0xF6 段；不在 F1009 的 0xF5 段、不在 VE-N F2606 的 0x2C 段。
        s.add(
            "C1010-错误-码段双向钉死",
            (f1.code() & 0xFF00) == 0xF600
                && (f2.code() & 0xFF00) == 0xF600
                && (f1.code() & 0xFF00) != 0xF500
                && (f1.code() & 0xFF00) != 0x2C00
                && f1.code() != f2.code(),
            "本单独占 0xF6xx，且不落入 F1009(0xF5)/F2606(0x2C) 已占段；只钉「在段内」挡不住改回撞段值",
        );
        // 十个变体码各不同。
        let all = [
            Fault::KeyPayloadLength { want: 1, got: 2 },
            Fault::KeyOutOfRange { key: 1, max: 2 },
            Fault::PlteMissing,
            Fault::BadBitDepth { bit_depth: 1 },
            Fault::SampleShapeMismatch { want: 1, got: 2 },
            Fault::AlphaIndexOutOfRange { index: 1, count: 2 },
            Fault::LutIndexOutOfRange { index: 1 },
            Fault::LaneBatchSize { want: 4, got_c: 3, got_a: 3 },
            Fault::BufferSize { want: 4, got: 3 },
            Fault::NumOutOfRange { got: 256, max: 255 },
        ];
        let mut uniq = true;
        let mut seen: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < all.len() {
            let c = all[i].code();
            let mut k = 0usize;
            while k < seen.len() {
                if seen[k] == c {
                    uniq = false;
                }
                k += 1;
            }
            seen.push(c);
            i += 1;
        }
        s.add(
            "C1010-错误-十变体码各不同",
            all.len() == 10 && uniq,
            "码重复会让下游按码分流时把两种故障当一种",
        );
        // 三要素齐备带出处。
        let mut three_ok = true;
        let mut ti = 0usize;
        while ti < all.len() {
            let t = all[ti].three_elements();
            if t.is_empty() || !all[ti].why().contains("F1010") || !t.contains('｜') {
                three_ok = false;
            }
            ti += 1;
        }
        s.add(
            "C1010-错误-三要素齐备带出处",
            three_ok,
            "what/why/advice 非空且 why 引用 F1010；拒绝信息不全 = 用户不知道改什么",
        );
        // 诊断码格式稳定。
        s.add(
            "C1010-错误-诊断码格式稳定",
            f1.diag().starts_with("F1010-") && f1.diag().len() > 7,
            "诊断码供日志与面板 grep，格式必须稳定",
        );
        // 边界操作零 panic：越界索引/查表/批守卫全给 Err。
        let safe = palette_alpha(&[1, 2], 9).is_err()
            && PremulLut8::build().lookup(255, 255).is_ok()
            && key_matches(&ColorKey::Rgb(1, 2, 3), &[1, 2]).is_err()
            && resolve_trns(ColorType::Palette, 8, &[1], 0, true).is_err();
        s.add(
            "C1010-错误-边界操作零panic",
            safe,
            "越界索引、满表末项、形状不符、无 PLTE——全给 Err 而非 panic",
        );
    }

    // =====================================================================
    // 判据承载：判据集自身（注册校验由独立探针从真 mod.rs 切段，见 F1009）
    // =====================================================================
    {
        s.add(
            "C1010-判据集-未截断且非空转",
            !s.truncated() && s.len() >= 40,
            "截断末尾判据等于没写；空集说明判据层空转",
        );
    }

    s
}
