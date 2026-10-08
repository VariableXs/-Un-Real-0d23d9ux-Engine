//! VE-F2008 · 域自检（判据逐条对应，见 `vek08_colorspace.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **双编码** → `K08-双编码-*`
//! - **标记防错配** → `K08-标记-*`
//! - **边界双端** → `K08-边界-*`
//! - **精度声明** → `K08-精度-*`
//! - 错误路径四条（方向错配/默认声明/PQ 钳制/ΔE 立案）→ `K08-错误-*`
//!
//! **门禁设计的四条自律**（本文件是它们的实践样本）：
//! 1. **参考必须外部**：ΔE 门禁对拍的是独立公布值，不是本实现自己算的值。
//!    本条开发中该门禁真的抓到过参考表自身的错误（code=160）——若拿自己当参考，
//!    那个错误会永远测不出来。
//! 2. **反假变体**：每条"防错配"判据都配一条**注入错配**的用例，确认它真的
//!    会红。恒绿的断言等于没有断言。
//! 3. **不测表内元素验表函数**：`srgb_decode` 的往返测试用**表外**的连续值，
//!    表内 9 点只用于对拍外部参考——两件事不能混成一件。
//! 4. **判定与数值分离**：方向判定（安全相关）与解码数值（功能相关）分开断言，
//!    改数值不会碰到判定。
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek08_colorspace::*;
use crate::checks::CheckSet;
// 上游依赖显式导入：`use ...::*` 只带出模块**自己定义**的公开项，不带出它
// `use` 进来的上游项——靠间接带入能编过，但本模块删掉某个 import 时调用方
// 可见性会静默变化。
use crate::svstar2::vek06_tonemap::SceneReferred;

/// 近似相等。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// 构造一个已校验的 SDR 目标（不经诊断袋，供纯数值判据使用）。
///
/// **不经 `resolve()`是刻意的**：数值判据要的是"一个确定的已校验目标"，
/// 若走 `resolve()`，每条数值判据都会往诊断袋里塞一条"目标空间未探测"，
/// 那些判据就再也无法单独断言诊断内容了。诊断类判据另起`CsDiagBag` 单独测。
fn sdr_target() -> ResolvedTarget {
    ResolvedTarget {
        encoding: Encoding::SdrGamma,
        origin: TargetSpaceOrigin::Probed,
        reference_nits: SDR_REFERENCE_NITS,
    }
}

/// 构造一个已校验的 PQ 目标。
fn pq_target(reference_nits: f32) -> ResolvedTarget {
    ResolvedTarget {
        encoding: Encoding::HdrPq,
        origin: TargetSpaceOrigin::Probed,
        reference_nits,
    }
}

/// VE-F2008 域自检。
pub fn run_vek08_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek08");

    // ---- 双编码（判据一）----

    // 判据：sRGB 编码端点精确——0→0、1→1。
    //
    // 端点是最容易被"顺手加个 clamp"破坏的地方：若编码在1.0 处出 1.0000001，
    // 画面最亮处会被削掉一档。这条断言 1e-6 精度。
    {
        let ok = srgb_encode(0.0) == 0.0 && (srgb_encode(1.0) - 1.0).abs() < 1e-6;
        set.add("K08-双编码-sRGB端点精确", ok, "0->0, 1->1");
    }

    // 判据：sRGB 编码在两个折点两侧**连续**（不出现折点处的跳变台阶）。
    //
    // 连续性用"折点左右极邻近取值之差"判定：若折点处理写成分段错位
    // （例如编码侧用了 0.04045），折点两侧会出现 ~0.03 的台阶。
    {
        let e = 1e-7;
        let jump_enc = (srgb_encode(SRGB_OETF_BREAK + e) - srgb_encode(SRGB_OETF_BREAK - e)).abs();
        let jump_dec = (srgb_decode(SRGB_EOTF_BREAK + e) - srgb_decode(SRGB_EOTF_BREAK - e)).abs();
        set.add(
            "K08-双编码-sRGB折点连续无台阶",
            jump_enc < 1e-2 && jump_dec < 1e-2,
            "编码跳变 <1e-2",
        );
    }

    // 判据：PQ 编码端点精确——0nit→≈0（规范行为，见下条）、10000nit→1。
    {
        let at_max = (pq_encode(MAX_PQ_NITS) - 1.0).abs();
        set.add("K08-双编码-PQ峰值精确", at_max < 1e-6, "10000nit -> 1.0");
    }

    // 判据：PQ 零点码值为**规范非零值**而非精确 0。
    //
    // 这条断言的是"知道规范在这里不精确"。若哪天有人把 pq_encode(0) 改成
    // 硬return 0.0，这条会红——那是**行为变更**，必须走 ADR，不能悄悄改。
    {
        let z = pq_zero_code_measured();
        let ok = z > 0.0 && z < 1e-5 && (z - PQ_ZERO_CODE_VALUE).abs() < 1e-8;
        set.add("K08-双编码-PQ零点为规范非零", ok, "7.31e-7");
    }

    // 判据：PQ 双向可用（encode→decode 往返成立）。
    //
    // **这条是本条最要紧的一条**：开发中 pq_decode 的分母曾误写为 `c2`
    // 而非 `c2 − P·c3`，往返结果恒为 0，且**不报任何错**。断言只测端点
    // 是抓不到的（端点恰好也对），必须测往返。
    {
        let mut ok = true;
        for &n in &[1.0f32, 10.0, 100.0, 1000.0, 5000.0, 10000.0] {
            let back = pq_decode(pq_encode(n));
            if (back - n).abs() / n > 1e-3 {
                ok = false;
            }
        }
        set.add("K08-双编码-PQ往返成立", ok, "6 点相对误差 <1e-3");
    }

    // 判据：PQ 量纲——编码输入是**绝对亮度**，不是归一值。
    //
    // 同一线性值 0.5，在 SDR 目标下是 sRGB 码值 0.7354，在 PQ 目标下
    // （参考 100nit）应是 `pq_encode(50)` ≈ 0.4403。若实现误把归一值直接喂
    // PQ（等价于宣称峰值 1nit），会得到 `pq_encode(0.5)` ≈ 0.1175——
    // 差 3.7 倍，肉眼可见地暗。
    //
    // **不断言 `pq > sdr`**：两族编码的码值刻度不同（sRGB 幂段 vs PQ 的
    // m2=78.84 曲线），数值大小关系没有意义，拿它当判据是"看起来在比大小"
    // 的假门禁。真正要锁住的是"喂进去的量纲是 50nit 而不是 0.5"这一件事。
    {
        let mut bag = CsDiagBag::new();
        let sdr = encode_for_present(SceneReferred::new(0.5, 0.5, 0.5), &sdr_target(), &mut bag);
        let pq = encode_for_present(
            SceneReferred::new(0.5, 0.5, 0.5),
            &pq_target(SDR_REFERENCE_NITS),
            &mut bag,
        );
        let correct = pq_encode(50.0);
        let wrong = pq_encode(0.5);
        let ok = close(sdr.r, 0.735_357)
            && close(pq.r, correct)
            && (pq.r - wrong).abs() > 0.1;
        set.add("K08-双编码-PQ量纲为绝对亮度", ok, "0.5线性->0.440 PQ码");
    }

    // 判据：两种编码器对同一输入**产出不同码值**（不是同一个函数换名字）。
    //
    // 反假门禁：若两个编码器被写成同一个实现，本条会红。
    {
        let mut bag = CsDiagBag::new();
        let c = SceneReferred::new(0.18, 0.18, 0.18);
        let a = encode_for_present(c, &sdr_target(), &mut bag);
        let b = encode_for_present(c, &pq_target(SDR_REFERENCE_NITS), &mut bag);
        set.add(
            "K08-双编码-两族编码器输出相异",
            (a.r - b.r).abs() > 0.05,
            "0.18 灰阶分道",
        );
    }

    // ---- 标记防错配（判据二）----

    // 判据：编码标记三值齐全且键名互异（LINEAR/SDR_GAMMA/HDR_PQ）。
    {
        let keys: Vec<&str> = Encoding::all().iter().map(|e| e.key()).collect();
        let distinct = keys.len() == 3
            && keys[0] != keys[1]
            && keys[1] != keys[2]
            && keys[0] != keys[2];
        let roundtrip = keys.iter().all(|k| {
            Encoding::from_key(k).map(|e| e.key() == *k).unwrap_or(false)
        });
        set.add("K08-标记-三值键名互异且可反解", distinct && roundtrip, "3 值");
    }

    // 判据：只有 LINEAR 未编码，两个编码态均已编码（边界断言的第一判据）。
    {
        let ok = !Encoding::Linear.is_encoded()
            && Encoding::SdrGamma.is_encoded()
            && Encoding::HdrPq.is_encoded();
        set.add("K08-标记-已编码判定正确", ok, "LINEAR 未编码");
    }

    // 判据：仅 PQ 为绝对亮度量纲（sRGB 不是）。
    //
    // 这条守的是本条最容易犯的量纲错误：把 sRGB 缓冲也当绝对亮度处理。
    {
        let ok = Encoding::HdrPq.is_absolute_luminance()
            && !Encoding::SdrGamma.is_absolute_luminance()
            && !Encoding::Linear.is_absolute_luminance();
        set.add("K08-标记-仅PQ为绝对亮度量纲", ok, "");
    }

    // 判据：方向判定四种错法**逐条可区分**（不是笼统一个"不匹配"）。
    //
    // 判定表的每一格都在这里被点名。若四种错法被压成一种码，收到告警的人
    // 得从头查根因——诊断的价值在于指认。
    {
        let v1 = check_direction(Encoding::Linear, SampleIntent::WantEncoded);
        let v2 = check_direction(Encoding::SdrGamma, SampleIntent::WantLinear);
        let v3 = check_direction(Encoding::HdrPq, SampleIntent::WantSdrGamma);
        let v4 = check_direction(Encoding::SdrGamma, SampleIntent::WantHdrPq);
        let ok = v1 == DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchLinearAsEncoded)
            && v2 == DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchEncodedAsLinear)
            && v3 == DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchCrossFamily)
            && v4 == DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchCrossFamily);
        set.add("K08-标记-四种错法逐条可区分", ok, "4 码");
    }

    // 判据：方向判定的一致格确实为 Match（六种组合全测，不抽样）。
    {
        let mut ok = true;
        for e in Encoding::all().iter() {
            let intent = match e {
                Encoding::Linear => SampleIntent::WantLinear,
                Encoding::SdrGamma => SampleIntent::WantSdrGamma,
                Encoding::HdrPq => SampleIntent::WantHdrPq,
            };
            if check_direction(*e, intent) != DirectionVerdict::Match {
                ok = false;
            }
            // WantEncoded 对两个已编码态都应放行
            if e.is_encoded() && check_direction(*e, SampleIntent::WantEncoded) != DirectionVerdict::Match
            {
                ok = false;
            }
        }
        set.add("K08-标记-一致格全放行", ok, "6 组合");
    }

    // 判据：方向错配**必然**产生可访问性影响标记（发灰压低对比度）。
    {
        let codes = [
            CsDiagCode::DirectionMismatchLinearAsEncoded,
            CsDiagCode::DirectionMismatchEncodedAsLinear,
            CsDiagCode::DirectionMismatchCrossFamily,
            CsDiagCode::DirectionMismatchDoubleDecode,
        ];
        let ok = codes.iter().all(|c| c.is_a11y_impact() && c.is_blocking());
        set.add("K08-标记-方向错配记为可访问性影响", ok, "4 码");
    }

    // 判据：**反假变体**——注入错配后守卫必须报警。
    //
    // 没有这一条，上面所有"判定正确"都可能是恒绿的假绿：守卫若写反了
    // （该报警时沉默），一致格仍会全绿。
    {
        let desc = RtDesc::new(1, Encoding::Linear);
        let mut bag = CsDiagBag::new();
        let (_, warned) = sample_checked(&desc, SampleIntent::WantEncoded, (0.5, 0.5, 0.5), &mut bag);
        let ok = warned
            && bag.has(CsDiagCode::DirectionMismatchLinearAsEncoded)
            && bag.blocking_count() == 1
            && bag.a11y_count() == 1;
        set.add("K08-标记-注入错配必报警", ok, "LINEAR 当已编码读");
    }

    // 判据：类型携带路径与运行期路径**同语义**（同输入同输出）。
    //
    // 两条路径是同一套语义的两个载体。漂移的后果是"编译期过的代码在运行期
    // 给出不同像素"——最难查的一类不一致，故必须逐点比对。
    {
        let mut ok = true;
        for &v in &[0.0f32, 0.04045, 0.2158, 0.5, 0.7354, 1.0] {
            let typed = decode_to_linear(&SdrBuf::new(v, v, v)).r;
            let runtime = decode_linear_rt(Encoding::SdrGamma, (v, v, v)).0;
            if (typed - runtime).abs() > 1e-6 {
                ok = false;
            }
        }
        set.add("K08-标记-类型路径与运行期路径同语义", ok, "6 点");
    }

    // 判据：类型标记由类型参数决定，构造时**无法**传入错误编码。
    //
    // 这是"编译期携带"的可观测证据：`SdrBuf::new` 只有三个 f32 形参，
    // 根本没有编码参数可传——错配在语言层面不可表达。
    {
        let b = SdrBuf::new(0.1, 0.2, 0.3);
        let ok = b.encoding() == Encoding::SdrGamma
            && LinearBuf::new(0.1, 0.2, 0.3).encoding() == Encoding::Linear
            && PqBuf::new(0.1, 0.2, 0.3).encoding() == Encoding::HdrPq;
        set.add("K08-标记-类型决定编码不可传错", ok, "");
    }

    // 判据：封桶函数拒绝错配（LINEAR 结果封不进 SdrBuf，反之亦然）。
    //
    // 三个目标各封一次，三个封桶函数各问一次——9 种组合里只允许 3 种成功
    // （对角线）。逐一列出而非用循环，是为了让失败时能直接看出是哪一格错配。
    {
        let mut bag = CsDiagBag::new();
        let c = SceneReferred::new(0.5, 0.5, 0.5);
        let lin = encode_for_present(
            c,
            &ResolvedTarget {
                encoding: Encoding::Linear,
                origin: TargetSpaceOrigin::Probed,
                reference_nits: SDR_REFERENCE_NITS,
            },
            &mut bag,
        );
        let sdr = encode_for_present(c, &sdr_target(), &mut bag);
        let pq = encode_for_present(c, &pq_target(SDR_REFERENCE_NITS), &mut bag);
        let ok = seal_linear(&lin).is_some()
            && seal_srgb(&lin).is_none()
            && seal_pq(&lin).is_none()
            && seal_srgb(&sdr).is_some()
            && seal_linear(&sdr).is_none()
            && seal_pq(&sdr).is_none()
            && seal_pq(&pq).is_some()
            && seal_linear(&pq).is_none()
            && seal_srgb(&pq).is_none();
        set.add("K08-标记-封桶拒绝错配", ok, "9 格中 3 纳 6 拒");
    }

    // 判据：RT 元数据可读（含编码键与呈现面标记），供遥测与设置页定位。
    {
        let d = RtDesc::new(7, Encoding::HdrPq).as_present_surface();
        let t = d.meta_text();
        set.add(
            "K08-标记-RT元数据可读",
            t.contains("RT#7") && t.contains("HDR_PQ") && t.contains("present"),
            "",
        );
    }

    // ---- 边界双端（判据三）----

    // 判据：合规交接放行。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::SdrGamma,
            k_side_encoded: true,
            buffer_id: 1,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::SdrGamma,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: false,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::Passed && bag.is_empty();
        set.add("K08-边界-合规交接放行", ok, "");
    }

    // 判据：V 侧二次编码被拦截（双重 gamma，画面发灰）。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::SdrGamma,
            k_side_encoded: true,
            buffer_id: 1,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::SdrGamma,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: true,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::RejectedVReencode
            && bag.has(CsDiagCode::BoundaryVSideReencodeAttempt)
            && bag.blocking_count() == 1;
        set.add("K08-边界-V侧二次编码被拦截", ok, "");
    }

    // 判据：K 侧交出未编码缓冲被拦截（LINEAR 不得上屏）。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::Linear,
            k_side_encoded: true,
            buffer_id: 2,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::Linear,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: false,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::RejectedKNotEncoded
            && bag.has(CsDiagCode::BoundaryKSideNotEncoded);
        set.add("K08-边界-K侧未编码被拦截", ok, "LINEAR 上屏阻断");
    }

    // 判据：K 侧自认已编码但实际标记是 LINEAR 时**同样**被拦截。
    //
    // 上一条测的是"标记与自认一致地错"，这条测的是"自认为对但实际错"——
    // 后者更危险，因为 K 侧的确认位是绿的。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::Linear,
            k_side_encoded: true,
            buffer_id: 3,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::SdrGamma,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: false,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::RejectedKNotEncoded;
        set.add("K08-边界-标记与自认矛盾被拦截", ok, "");
    }

    // 判据：双端认知不一致被拦截（各以为对方编了 → 实际零次编码）。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::SdrGamma,
            k_side_encoded: true,
            buffer_id: 4,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::HdrPq,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: false,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::RejectedEncodingDisagreement;
        set.add("K08-边界-双端认知不一致被拦截", ok, "sRGB vs PQ");
    }

    // 判据：缺确认位被拦截（**两侧**都测，不只测 V 侧）。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::SdrGamma,
            k_side_encoded: true,
            buffer_id: 5,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::SdrGamma,
            v_side_accepted_encoded: false,
            v_side_wants_reencode: false,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::RejectedConfirmationMissing
            && bag.has(CsDiagCode::BoundaryConfirmationMissing);
        set.add("K08-边界-缺确认位被拦截", ok, "");
    }

    // 判据：**判定顺序**——主动违规（V 二次编码）优先于认知不一致。
    //
    // 顺序有实际后果：若先报"认知不一致"，收到告警的人会去查编码配置，
    // 而真正的根因（V 侧明说要再编）被排在后面。根因必须先报。
    {
        let mut bag = CsDiagBag::new();
        let k = KSideHandoff {
            encoding: Encoding::SdrGamma,
            k_side_encoded: true,
            buffer_id: 6,
        };
        let v = VSideIntake {
            observed_encoding: Encoding::HdrPq,
            v_side_accepted_encoded: false,
            v_side_wants_reencode: true,
        };
        let ok = judge_handoff(&k, &v, &mut bag) == HandoffVerdict::RejectedVReencode;
        set.add("K08-边界-主动违规优先报出", ok, "");
    }

    // 判据：每种拦截裁决都有**说清谁违约**的文本（诊断要说实话）。
    {
        let all = [
            HandoffVerdict::Passed,
            HandoffVerdict::RejectedKNotEncoded,
            HandoffVerdict::RejectedVReencode,
            HandoffVerdict::RejectedEncodingDisagreement,
            HandoffVerdict::RejectedConfirmationMissing,
        ];
        let ok = all.iter().all(|v| {
            let t = handoff_verdict_text(*v);
            t.len() > 4
        });
        set.add("K08-边界-裁决文本齐备", ok, "5 裁决");
    }

    // ---- 精度声明（判据四）----

    // 判据：参考表**非空**（空表会让ΔE 门禁"零点全过"）。
    //
    // 这是门禁自身的完整性检查。表被清空时 `check_srgb8_reference()` 返回
    // 空 Vec，逐点循环一次都不跑，ΔE 恒过——门禁静默失效。
    {
        let pts = check_srgb8_reference();
        set.add("K08-精度-参考表非空", pts.len() >= 9, "9 点跨段");
    }

    // 判据：参考表覆盖两段与两端（只有幂段会漏折点错误，只有线性段会漏指数错误）。
    {
        let pts = check_srgb8_reference();
        let has_linear_seg = pts.iter().any(|p| p.code as f32 / 255.0 <= SRGB_EOTF_BREAK);
        let has_power_seg = pts.iter().any(|p| p.code as f32 / 255.0 > SRGB_EOTF_BREAK);
        let has_both_ends = pts.iter().any(|p| p.code == 0) && pts.iter().any(|p| p.code == 255);
        set.add(
            "K08-精度-参考表覆盖两段两端",
            has_linear_seg && has_power_seg && has_both_ends,
            "",
        );
    }

    // 判据：sRGB 解码与**外部参考**逐点 ΔE 全部低于立案阈值。
    //
    // 逐点核，不取最大值糊弄——最大值达标不代表每点达标（虽然实践中等价，
    // 但逐点核能在报告里指出是哪一点差）。
    {
        let pts = check_srgb8_reference();
        let mut worst = 0.0f32;
        let mut all_below = true;
        for p in pts.iter() {
            if p.delta_e > worst {
                worst = p.delta_e;
            }
            if p.delta_e > MAX_DELTA_E {
                all_below = false;
            }
        }
        set.add(
            "K08-精度-外部参考逐点ΔE低于阈值",
            all_below && !pts.is_empty(),
            "最差 ΔE<0.5",
        );
    }

    // 判据：**反假变体**——把参考表灌入明显错误的值后，ΔE 门禁必须变红。
    //
    // 这条是"参考必须外部"这条自律的执行保障。若门禁对错误参考值仍绿，
    // 说明它比的是自己而不是参考表（恒真弱门禁）。
    {
        // 构造一个与真值差 0.005 的伪参考点，走同一 ΔE 计算路径。
        let bogus_expected = srgb_decode(0.5) + 0.005;
        let bogus_actual = srgb_decode(0.5);
        let la = linear_srgb_to_lab(bogus_actual, bogus_actual, bogus_actual);
        let le = linear_srgb_to_lab(bogus_expected, bogus_expected, bogus_expected);
        let de = delta_e76(la, le);
        set.add("K08-精度-ΔE门禁对错误参考变红", de > MAX_DELTA_E, "偏差 0.005 必立案");
    }

    // 判据：ΔE 计算本身有效（相同 Lab → ΔE=0，不同 Lab → ΔE>0）。
    //
    // 测ΔE 函数而不只是测它的结果：ΔE 若恒返回 0，上面那条门禁就是恒真。
    {
        let a = linear_srgb_to_lab(0.2, 0.2, 0.2);
        let b = linear_srgb_to_lab(0.8, 0.8, 0.8);
        let ok = delta_e76(a, a) == 0.0 && delta_e76(a, b) > 1.0;
        set.add("K08-精度-ΔE计算非恒零", ok, "");
    }

    // 判据：sRGB 往返误差在**声明的实测上界**内（表外连续值，不是表内 9 点）。
    {
        let e = srgb_roundtrip_max_error(4096);
        let ok = matches!(e, Some(v) if v > 0.0 && v < 1e-5);
        set.add("K08-精度-sRGB往返在声明界内", ok, "<1e-5 实测 2.4e-7");
    }

    // 判据：负半轴往返误差在界内**且符号正确**。
    //
    // 符号单独测：忘记乘回符号的实现，正半轴往返完全正常，只有负半轴露馅。
    {
        let e = srgb_negative_roundtrip_max_error(4096);
        let neg = srgb_encode(-0.5);
        let ok = matches!(e, Some(v) if v < 1e-5) && neg < 0.0 && srgb_decode(neg) < 0.0;
        set.add("K08-精度-负半轴往返且符号正确", ok, "符号对称");
    }

    // 判据：PQ 往返相对误差在声明界内（1000 采样，最差点≈4810nit）。
    {
        let e = pq_roundtrip_max_relative_error(1000);
        let ok = matches!(e, Some(v) if v > 0.0 && v < 1e-3);
        set.add("K08-精度-PQ往返相对误差在界内", ok, "<1e-3 实测 1.1e-4");
    }

    // 判据：PQ 100nit 码值落在公布锚点容差内（对拍**外部**公布值）。
    {
        let c = pq_encode(100.0);
        let ok = (c - PQ_100NITS_CODE).abs() <= PQ_100NITS_TOLERANCE;
        set.add("K08-精度-PQ100nit对拍公布锚点", ok, "0.50808");
    }

    // 判据：PQ 解码**不产出 NaN**（`max(·,0)` 与分母守卫在位）。
    //
    // 逐点扫全码值域：删掉 `num <= 0` 守卫后，小码值处负底数取分数次幂
    // 会产出 NaN，而 NaN 在画面上是永不消失的异色块。
    {
        let mut ok = true;
        for i in 0..=1000 {
            let c = i as f32 / 1000.0;
            let l = pq_decode(c);
            if !l.is_finite() || l < 0.0 {
                ok = false;
            }
        }
        set.add("K08-精度-PQ解码全域无NaN", ok, "1001 点");
    }

    // 判据：sRGB 编解码**全域无 NaN**（含负输入，负底数幂是主要风险）。
    {
        let mut ok = true;
        for i in 0..=1000 {
            let v = i as f32 / 1000.0;
            for &x in &[v, -v] {
                if !srgb_encode(x).is_finite() || !srgb_decode(srgb_encode(x)).is_finite() {
                    ok = false;
                }
                if !srgb_decode(x).is_finite() {
                    ok = false;
                }
            }
        }
        set.add("K08-精度-sRGB全域无NaN", ok, "含负半轴");
    }

    // 判据：ΔE 立案台账当前**零立案**（实测通过）。
    {
        let mut bag = CsDiagBag::new();
        let mut log = DeviationLog::new();
        let n = log.audit(&mut bag);
        set.add(
            "K08-精度-ΔE台账零立案",
            n == 0 && log.is_clean() && !bag.has(CsDiagCode::DeltaEExceeded),
            "9 点全过",
        );
    }

    // ---- 错误路径（锚点四条）----

    // 判据（错误 1）：编码方向错配 → 运行时告警（不中止渲染）。
    //
    // "不中止"是设计决策：中止会让画面黑掉，那比画错更难排查。
    {
        let desc = RtDesc::new(9, Encoding::SdrGamma);
        let mut bag = CsDiagBag::new();
        let (out, warned) =
            sample_checked(&desc, SampleIntent::WantLinear, (0.5, 0.5, 0.5), &mut bag);
        let ok = warned
            && bag.has(CsDiagCode::DirectionMismatchEncodedAsLinear)
            && out.0.is_finite();
        set.add("K08-错误-方向错配告警且继续", ok, "已编码当线性读");
    }

    // 判据（错误 1 续）：错配时**按标记事实解码**，不按意图解码。
    //
    // 反过来（按意图解码）会让错配静默按错误语义处理，诊断永不触发。
    {
        let desc = RtDesc::new(10, Encoding::SdrGamma);
        let mut bag = CsDiagBag::new();
        let (out, _) = sample_checked(&desc, SampleIntent::WantLinear, (0.5, 0.5, 0.5), &mut bag);
        let by_fact = decode_linear_rt(Encoding::SdrGamma, (0.5, 0.5, 0.5)).0;
        let ok = close(out.0, by_fact) && close(out.0, srgb_decode(0.5));
        set.add("K08-错误-按标记事实而非意图解码", ok, "");
    }

    // 判据（错误 2）：目标空间未知 → 默认 sRGB + **显性声明**。
    {
        let mut bag = CsDiagBag::new();
        let r = TargetSpace::unprobed_default().resolve(&mut bag);
        set.add(
            "K08-错误-目标空间未知默认sRGB且声明",
            r.encoding == Encoding::SdrGamma
                && r.origin == TargetSpaceOrigin::Defaulted
                && bag.has(CsDiagCode::TargetSpaceUnknownDefaulted),
            "默认不冒充探测",
        );
    }

    // 判据（错误 2 续）：**已探测**的目标**不**产生"默认"诊断（不误报）。
    //
    // 反假门禁：若 `resolve` 无条件落默认诊断，这条会红——那会让真探测结果
    // 也背上"未探测"的告警，久了就没人看告警了。
    {
        let mut bag = CsDiagBag::new();
        let r = TargetSpace::probed(Encoding::HdrPq, 1000.0).resolve(&mut bag);
        set.add(
            "K08-错误-已探测目标不误报默认",
            r.origin == TargetSpaceOrigin::Probed && !bag.has(CsDiagCode::TargetSpaceUnknownDefaulted),
            "",
        );
    }

    // 判据（错误 3）：PQ 越界（超 10000nit / 为负）→ 钳制 + 显性诊断。
    {
        let mut bag = CsDiagBag::new();
        let hi = pq_encode_guarded(20000.0, &mut bag);
        let lo = pq_encode_guarded(-5.0, &mut bag);
        let ok = close(hi, 1.0)
            && close(lo, pq_encode(0.0))
            && bag.has(CsDiagCode::PqOutOfRangeClamped);
        set.add("K08-错误-PQ越界钳制且显性", ok, "20000/负值");
    }

    // 判据（错误 3 续）：**范围内**输入不产生越界诊断（不误报）。
    //
    // 与上条成对：钳制诊断若对正常值也报，日志会被噪声淹没，真越界反而被忽略。
    {
        let mut bag = CsDiagBag::new();
        let _ = pq_encode_guarded(100.0, &mut bag);
        let ok = !bag.has(CsDiagCode::PqOutOfRangeClamped);
        set.add("K08-错误-范围内PQ不误报越界", ok, "");
    }

    // 判据（错误 3 续）：**反假变体**——不钳制就会漏报。
    //
    // 纯钳制不回灌诊断的门禁是"看起来在防越界、实际静默吞掉"。这条用
    // 手工构造证明诊断通道确实能承载越界事实（与上面 20000nit 那条互补：
    // 那条证钳制生效，这条证诊断不依赖钳制实现）。
    {
        let mut bag = CsDiagBag::new();
        bag.note(CsDiagCode::PqOutOfRangeClamped, 20000.0);
        let ok = bag.has(CsDiagCode::PqOutOfRangeClamped) && bag.len() == 1;
        set.add("K08-错误-越界事实可独立落账", ok, "");
    }

    // 判据（错误 4）：ΔE 超阈值 → 立案（进台账 + 落诊断），不就地凑合。
    {
        let mut bag = CsDiagBag::new();
        let mut log = DeviationLog::new();
        // 审计当前实现（应零立案），再验证台账机制本身能承载立案条目。
        let n = log.audit(&mut bag);
        let mut forced = DeviationLog::new();
        forced.record(DeviationCase {
            code: 160,
            delta_e: 0.9,
            verdict: "超 ΔE 阈值：按外部参考表逐点修正公式实现",
        });
        let ok = n == 0
            && log.is_clean()
            && forced.len() == 1
            && !forced.is_clean()
            && forced.report().contains("160");
        set.add("K08-错误-ΔE超阈值即立案", ok, "台账可承载");
    }

    // 判据：非有限输入在**编码入口**被拦下（NaN 不得穿透到显示）。
    {
        let mut bag = CsDiagBag::new();
        let dirty = SceneReferred::new(f32::NAN, f32::INFINITY, -1.0);
        let out = encode_for_present(dirty, &sdr_target(), &mut bag);
        let ok = out.sanitized
            && out.r.is_finite()
            && out.g.is_finite()
            && out.b.is_finite()
            && out.b >= 0.0
            && bag.has(CsDiagCode::InputNonFiniteSanitized);
        set.add("K08-错误-非有限输入编码前清洗", ok, "NaN/Inf/负");
    }

    // 判据：清洗后**报告与实际一致**（`sanitized` 说动了就真动了）。
    //
    // `sanitized` 标志与实际输出脱钩是"名字撒谎"型缺陷：调用点据它决定是否
    // 告警，标志恒 false 就等于告警永不发。
    {
        let mut bag = CsDiagBag::new();
        let clean = encode_for_present(SceneReferred::new(0.5, 0.5, 0.5), &sdr_target(), &mut bag);
        let dirty = encode_for_present(
            SceneReferred::new(f32::NAN, 0.5, f32::NAN),
            &sdr_target(),
            &mut bag,
        );
        let ok = !clean.sanitized && dirty.sanitized && close(dirty.r, 0.0) && close(dirty.g, clean.g);
        set.add("K08-错误-清洗标志与实际一致", ok, "");
    }

    // ---- 帧边界与状态 ----

    // 判据：同帧内重复切档被拒（一帧内双重编码 = 方向错配的又一形态）。
    {
        let mut st = ColorSpaceState::new();
        let probe = TargetSpace::probed(Encoding::SdrGamma, 100.0);
        let first = st.set_target(&probe, 10, 9);
        let same_frame = st.set_target(&probe, 10, 10);
        let next_frame = st.set_target(&probe, 11, 10);
        set.add(
            "K08-边界-切档仅帧边界生效",
            first && !same_frame && next_frame,
            "同帧拒/跨帧纳",
        );
    }

    // 判据：状态遥测计数随真实事件增长（不恒零、不恒增）。
    {
        let mut st = ColorSpaceState::new();
        let desc = RtDesc::new(11, Encoding::Linear);
        let before = st.direction_mismatches;
        let _ = st.sample(&desc, SampleIntent::WantSdrGamma, (0.5, 0.5, 0.5));
        let after = st.direction_mismatches;
        let desc_ok = RtDesc::new(12, Encoding::SdrGamma);
        let _ = st.sample(&desc_ok, SampleIntent::WantSdrGamma, (0.5, 0.5, 0.5));
        let unchanged = st.direction_mismatches;
        set.add(
            "K08-标记-错配计数随真实事件增长",
            after == before + 1 && unchanged == after,
            "1 错配 0 正常",
        );
    }

    // 判据：交接拦截计数与最近裁决一致（遥测不撒谎）。
    {
        let mut st = ColorSpaceState::new();
        let k = KSideHandoff {
            encoding: Encoding::SdrGamma,
            k_side_encoded: true,
            buffer_id: 20,
        };
        let bad_v = VSideIntake {
            observed_encoding: Encoding::SdrGamma,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: true,
        };
        let v1 = st.handoff(&k, &bad_v);
        let good_v = VSideIntake {
            observed_encoding: Encoding::SdrGamma,
            v_side_accepted_encoded: true,
            v_side_wants_reencode: false,
        };
        let v2 = st.handoff(&k, &good_v);
        set.add(
            "K08-边界-拦截计数与裁决一致",
            v1 == HandoffVerdict::RejectedVReencode
                && v2 == HandoffVerdict::Passed
                && st.handoff_rejections == 1
                && st.last_handoff == HandoffVerdict::Passed,
            "",
        );
    }

    // 判据：设置页文本含关键态（编码 / 来源 / 错配数 / 立案数）。
    //
    // `CheckSet::add` 的 detail 形参是 `&'static str`，装不下运行时 String，
    // 故此处只能判"该含的都在"这种粗粒度，全文断言在 `#[cfg(test)]` 里做。
    {
        let st = ColorSpaceState::new();
        let t = st.screen_text();
        let ok = t.contains("sRGB")
            && t.contains("默认")
            && t.contains("方向错配")
            && t.contains("ΔE");
        set.add("K08-呈现-设置页含关键态", ok, "");
    }

    // 判据：PQ 目标的设置页文本显示**参考亮度**（量纲必须对用户可见）。
    {
        let mut st = ColorSpaceState::new();
        let pq = TargetSpace::probed(Encoding::HdrPq, 1000.0);
        let _ = st.set_target(&pq, 1, 0);
        let t = st.screen_text();
        set.add(
            "K08-呈现-PQ目标显示参考亮度",
            t.contains("PQ") && t.contains("nit") && t.contains("1000"),
            "",
        );
    }

    // 判据：参考亮度越界被夹取（0 或负参考亮度会让画面纯黑或除零）。
    {
        let mut bag = CsDiagBag::new();
        let zero = TargetSpace::probed(Encoding::HdrPq, 0.0).resolve(&mut bag);
        let neg = TargetSpace::probed(Encoding::HdrPq, -100.0).resolve(&mut bag);
        let huge = TargetSpace::probed(Encoding::HdrPq, 1e9).resolve(&mut bag);
        let ok = zero.reference_nits >= MIN_REFERENCE_NITS
            && neg.reference_nits >= MIN_REFERENCE_NITS
            && huge.reference_nits <= MAX_REFERENCE_NITS;
        set.add("K08-错误-参考亮度越界夹取", ok, "0/负/1e9");
    }

    // 判据：全通道诊断报告可读（含计数与逐条文本）。
    {
        let mut bag = CsDiagBag::new();
        let _ = TargetSpace::unprobed_default().resolve(&mut bag);
        let _ = pq_encode_guarded(20000.0, &mut bag);
        let r = bag.report();
        let ok = r.contains("色彩空间输出诊断")
            && r.contains("TARGET_SPACE_DEFAULTED")
            && r.contains("PQ_OUT_OF_RANGE");
        set.add("K08-呈现-诊断报告可读", ok, "2 码");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PQ 往返在**全量程**相对误差内。
    ///
    /// 判据项只抽查 6 点（要快），这里扫全量程 1000 点——因为 PQ 的误差
    /// 随码值非线性变化，抽查很容易正好落在误差小的区域而误判为"精度达标"。
    #[test]
    fn pq_roundtrip_over_full_range() {
        let worst = pq_roundtrip_max_relative_error(1000);
        assert!(worst.is_some(), "PQ 往返产出非有限值");
        let w = match worst {
            Some(v) => v,
            None => panic!("PQ 往返产出非有限值"),
        };
        assert!(w < 1e-3, "PQ 全量程往返相对误差 {:.3e} 超 1e-3", w);
    }

    /// PQ 双向在多个数量级上都成立（跨 5 个数量级逐档验证）。
    #[test]
    fn pq_survives_five_decades() {
        let mut n = 0.01f32;
        for _ in 0..7 {
            let back = pq_decode(pq_encode(n));
            assert!(
                back.is_finite() && (back - n).abs() / n < 1e-3,
                "{:.4} nit 往返失真：得 {:.6}",
                n,
                back
            );
            n *= 10.0;
        }
    }

    /// ΔE 门禁对**真实错误参考**变红（code=160 曾被写错过的回归防护）。
    ///
    /// 这是本条最有价值的一条测试：它把开发中真实发生过的错误固化成回归。
    /// 若表里 160 再次被误写，此测试立刻变红。
    #[test]
    fn reference_table_detects_wrong_entry() {
        // 表中 160 的真值必须是 0.3515326（f64 独立核算）。
        let mut found = false;
        for (code, v) in SRGB8_REFERENCE.iter() {
            if *code == 160 {
                found = true;
                assert!(
                    (v - 0.351_532_6).abs() < 1e-6,
                    "参考表 code=160 表值 {} 已偏离真值 0.3515326（这正是开发中发生过的错）",
                    v
                );
            }
        }
        assert!(found, "参考表缺 code=160 锚点");
    }

    /// sRGB 折点处**不连续**是规范性质，本测试固化该认识。
    ///
    /// 断言的是"两个折点不相等"这个事实本身。若哪天把 `SRGB_EOTF_BREAK`
    /// 误改成等于 `SRGB_OETF_BREAK * SRGB_SLOPE`（看起来更"整除"），
    /// 本测试会红——那是一次需要 ADR 的行为变更。
    #[test]
    fn srgb_breakpoints_are_intentionally_different() {
        assert!(
            (SRGB_OETF_BREAK - 0.003_130_8).abs() < 1e-9,
            "编码折点应为 0.0031308"
        );
        assert!(
            (SRGB_EOTF_BREAK - 0.040_45).abs() < 1e-9,
            "解码折点应为 0.04045"
        );
        // 二者的乘积关系：0.0031308 × 12.92 = 0.0404499…，比 0.04045 小约 6e-8。
        let product = SRGB_OETF_BREAK * SRGB_SLOPE;
        assert!(
            product < SRGB_EOTF_BREAK && (SRGB_EOTF_BREAK - product) < 1e-6,
            "折点乘积关系变了：{} vs {}（规范关系被破坏）",
            product,
            SRGB_EOTF_BREAK
        );
    }

    /// PQ 解码分母含 `c3·P` 项——写错则往返恒为 0。
    ///
    /// 同样是开发中真实发生过的错误（分母误写为 `c2`），固化为回归。
    #[test]
    fn pq_decode_denominator_includes_c3_term() {
        let p = pq_encode(100.0).powf(1.0 / PQ_M2);
        let correct_den = PQ_C2 - PQ_C3 * p;
        let wrong_den = PQ_C2;
        //正确分母在 P→1 时趋近 c2 − c3 ≈ 0.164，比 c2 小两个数量级。
        assert!(
            correct_den < wrong_den / 10.0,
            "PQ 解码分母未含 c3·P 项：{} vs {}",
            correct_den,
            wrong_den
        );
        // 且往返确实成立（错写法下恒为 0）。
        let back = pq_decode(pq_encode(100.0));
        assert!((back - 100.0).abs() < 0.5, "PQ 往返失效：得 {}", back);
    }

    /// 类型路径与运行期路径在PQ 上也同语义（sRGB 已由判据项覆盖）。
    #[test]
    fn typed_and_runtime_paths_agree_on_pq() {
        let v = 0.50808f32;
        let typed = decode_to_linear(&PqBuf::new(v, v, v)).r;
        let runtime = decode_linear_rt(Encoding::HdrPq, (v, v, v)).0;
        assert!(
            (typed - runtime).abs() < 1e-6,
            "PQ 两路径分歧：typed {} vs runtime {}",
            typed,
            runtime
        );
    }

    /// 设置页文本含全部关键态（全文断言，判据项只做粗粒度）。
    #[test]
    fn settings_text_has_all_key_states() {
        let mut st = ColorSpaceState::new();
        let txt = st.screen_text();
        for kw in ["sRGB", "默认", "方向错配", "交接拦截", "ΔE"] {
            assert!(txt.contains(kw), "常态文本缺 {}: {}", kw, txt);
        }

        st.encoding = Encoding::HdrPq;
        st.origin = TargetSpaceOrigin::Probed;
        st.reference_nits = 1000.0;
        st.direction_mismatches = 3;
        st.handoff_rejections = 2;
        let pq_txt = st.screen_text();
        assert!(
            pq_txt.contains("PQ") && pq_txt.contains("能力探测") && pq_txt.contains("1000"),
            "PQ 态文本缺项: {}",
            pq_txt
        );
        assert!(pq_txt.contains("3") && pq_txt.contains("2"), "计数未显性: {}", pq_txt);
    }

    /// 无障碍：方向错配在报告里必须显性（不能只进遥测计数）。
    #[test]
    fn direction_mismatch_is_explicit_in_report() {
        let mut bag = CsDiagBag::new();
        let desc = RtDesc::new(1, Encoding::Linear);
        let _ = sample_checked(&desc, SampleIntent::WantSdrGamma, (0.5, 0.5, 0.5), &mut bag);
        let r = bag.report();
        assert!(
            r.contains("DIR_LINEAR_AS_ENCODED") && r.contains("可访问性影响 1"),
            "方向错配未在报告中显性: {}",
            r
        );
    }

    /// 交接五种裁决各有说清谁违约的文本。
    #[test]
    fn every_handoff_verdict_names_the_culprit() {
        assert!(handoff_verdict_text(HandoffVerdict::Passed).contains("放行"));
        assert!(handoff_verdict_text(HandoffVerdict::RejectedKNotEncoded).contains("K 侧"));
        assert!(handoff_verdict_text(HandoffVerdict::RejectedVReencode).contains("V 侧"));
        assert!(handoff_verdict_text(HandoffVerdict::RejectedEncodingDisagreement).contains("不一致"));
        assert!(handoff_verdict_text(HandoffVerdict::RejectedConfirmationMissing).contains("确认"));
    }
}
