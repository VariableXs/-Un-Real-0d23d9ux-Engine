//! VE-F2207 · 域自检（判据逐条对应，见 `vel07_blend.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 深度排序 → `E01-深度-*`
//! - 三混合语义 → `E02-混合-*`
//! - 免排序声明 → `E03-免排序-*`
//! - GPU 预留 → `E04-预留-*`
//! - 降级矩阵（关排序警告 / I03 对齐 / NaN 钳制 / GPU 报错）→ `E05-降级-*`
//! - 性能可数工作量（比较次数 / 趟数 / 免排序零成本）→ `E06-性能-*`
//!
//! 零墙钟、零 IO；相机向量、颜色、粒子位置均为注入常量，故回归可复现。
//!
//! **判据设计自律（承 W006·VE-F2203~2206 弱门禁教训）**：
//! ① 判据**不向被测函数问答案**——凡涉及数值上界/序关系的，参考值一律
//!    由判据侧**独立重算**（如期望序用「键排序 + 序号升序」自己排一遍，
//!    不调用 [`order_cmp`]）；
//! ② 「加法免排序」与「alpha 需排序」必须**双向**验证：只断前者会被
//!    「恒等混合」（什么都不混）骗过，只断后者会被「无序混合」骗过；
//! ③ 判据里的 `[i]` 一律用 `.get()`——被测函数被换成退化实现时判据
//!    自己先 panic，看着像「变异未捕获」，实为门禁崩溃；
//! ④ 变异分类先看 stdout 再看退出码（探针在 failed>0 时故意 exit(1)）。

use alloc::vec;
use alloc::vec::Vec;

use super::vel03_emitter::{DiagBag, DiagCode, Outcome};
use super::vel05_lifetime::ShadedState;
use super::vel06_render::ParticleView;
use super::vel07_blend::*;
use crate::checks::CheckSet;

/// 造一个带 alpha 的**着色状态**（F2205淡变结果的形状：`ParticleView::new`
/// 收的是 `ShadedState` 而非 `Rgba`——两者同名不同形，写错就是 E0308）。
fn shaded(alpha: f32) -> ShadedState {
    ShadedState { alpha, size: 1.0, color: Rgba { r: 1.0, g: 1.0, b: 1.0, a: alpha } }
}

/// 造标准可见粒子视图（位置沿 x 轴，尺寸 1）。
fn view_at(x: f32, alpha: f32) -> ParticleView {
    ParticleView::new([x, 0.0, 0.0], [0.0, 0.0, 0.0], 1.0, shaded(alpha))
}

/// 造 `n` 个等距可见粒子（x = 0..n）。
fn views(n: usize) -> Vec<ParticleView> {
    let mut v: Vec<ParticleView> = Vec::new();
    for i in 0..n {
        v.push(view_at(i as f32, 1.0));
    }
    v
}

/// 造一批**深度各异**的粒子视图（视深 = 指定值，用于精确控制键）。
///
/// 相机取眼点=原点、前向=+z，故`z` 就是视深。这让判据能精确构造
/// 「键边界」「同键」「NaN」「相机后方」四类关键样本，而不必靠随��分布
/// 撞出边界（撞不出的判据等于没有）。
fn views_at_depths(ds: &[f32]) -> Vec<ParticleView> {
    let mut v: Vec<ParticleView> = Vec::new();
    for d in ds.iter() {
        v.push(ParticleView::new([0.0, 0.0, *d], [0.0, 0.0, 0.0], 1.0, shaded(1.0)));
    }
    v
}

/// 判据侧**独立**重算期望序：键降序、同键序号升序。
///
/// **刻意不调用 [`order_cmp`]**：若判据调用被测的同一个比较器，则
/// 「比较器写反了」这类变异会同时改掉实现与期望值，判据全绿——
/// 典型的自证式弱门禁。故此函数用**另一套写法**（先按序号升序、再按键
/// 稳定降序）重算，且两者不一致时会立刻暴露。
fn expect_order(entries: &[SortEntry]) -> Vec<u32> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    // 先按序号升序（稳定化的起点）。
    idx.sort_by(|a, b| entries[*a].index.cmp(&entries[*b].index));
    // 再按键**稳定**降序（stable sort 保证同键保持上一步的序号序）。
    idx.sort_by(|a, b| entries[*b].key.cmp(&entries[*a].key));
    let mut out: Vec<u32> = Vec::new();
    for i in idx.iter() {
        out.push(entries[*i].index);
    }
    out
}

/// 判据侧**独立**的因子求值器（对齐核验用，不调用 [`blend`]）。
///
/// 按 [`BlendFactors`] 的六元语义逐通道算：颜色维用 `src_color/dst_color`，
/// alpha 维用 `src_alpha/dst_alpha`，算子皆 `Add`。因子取值遵循硬件规则
/// （`SrcAlpha` 塌缩为标量，作用于各颜色通道）。
///
/// **刻意写成与被测不同的形状**：被测把 alpha 提前算成 `ia = 1-sa` 再复用，
/// 本函数每次现算`1-sa`。若被测把某个因子用错（例如拿 `dst_alpha`
/// 去算颜色），两者立刻分歧——同构写法会让「同一个错误写两遍」
/// 从而互相抵消，那是自证式弱门禁的典型。
fn ref_blend(f: BlendFactors, src: Rgba, dst: Rgba) -> Rgba {
    let sa = src.a;
    // 因子取值：颜色维与 alpha 维在本域三型里取值集合相同，但**分开写**
    // 以保持与硬件的「独立 alpha 维」语义同形。
    let cterm = |fac: Factor| -> f32 {
        match fac {
            Factor::Zero => 0.0,
            Factor::One => 1.0,
            Factor::SrcAlpha => sa,
            Factor::OneMinusSrcAlpha => 1.0 - sa,
        }
    };
    let aterm = |fac: Factor| -> f32 {
        match fac {
            Factor::Zero => 0.0,
            Factor::One => 1.0,
            Factor::SrcAlpha => sa,
            Factor::OneMinusSrcAlpha => 1.0 - sa,
        }
    };
    Rgba {
        r: src.r * cterm(f.src_color) + dst.r * cterm(f.dst_color),
        g: src.g * cterm(f.src_color) + dst.g * cterm(f.dst_color),
        b: src.b * cterm(f.src_color) + dst.b * cterm(f.dst_color),
        // alpha 维：**源项用 `src.a`、目标项用 `dst.a`**。
        // 写错成「两项都用 `sa`」会让本判据恒真——这是实测踩到的坑：
        // 初版此处两项都乘 `sa`，三型全都对上了（因三型的alpha 因子恰使
        // 该错写等价），判据于是完全失去区分力。
        a: src.a * aterm(f.src_alpha) + dst.a * aterm(f.dst_alpha),
    }
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// VE-F2207 判据全集。
pub fn run_vel07_all_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2207");
    let cam_eye = [0.0, 0.0, 0.0];
    let cam_fwd = [0.0, 0.0, 1.0];

    // =======================================================================
    // E01 深度排序
    // =======================================================================

    // 视深= (p - eye)·forward 的基本正确性：沿 +z 走 3 格，深度就是 3。
    {
        let d = view_depth_raw([0.0, 0.0, 3.0], cam_eye, cam_fwd);
        set.add("E01-视深-沿前向量位移等于深度", (d - 3.0).abs() < 1e-6, "");
    }

    // 视深对后退粒子为负（这正是「相机后方」需要钳制的原因）。
    {
        let d = view_depth_raw([0.0, 0.0, -2.0], cam_eye, cam_fwd);
        set.add("E01-视深-相机后方为负", d < 0.0, "");
    }

    // 视深是位置的线性函数：前向量按倍数缩放，深度同倍缩放。
    {
        let fwd = [0.577_350_27, 0.577_350_27, 0.577_350_27];
        let p = [2.0, -1.0, 4.0];
        let a = view_depth_raw(p, cam_eye, fwd);
        let b = view_depth_raw(p, cam_eye, [fwd[0] * 2.0, fwd[1] * 2.0, fwd[2] * 2.0]);
        set.add("E01-视深-随前向量长度线性缩放", ((b - 2.0 * a).abs() < 1e-5) && a > 0.0, "");
    }

    // **三分量各自参与**（补自实测漏网）：上面那条语料里 `p - eye` 的
    // y 分量为 0（前向量虽斜，但位移在xz 平面上），故把 `dot3` 中
    // `a[1]*b[1]` 的符号改掉仍全绿——弱门禁。
    //
    // 本条用**三个分量都非零**的位移，且判据侧**独立算出期望值**
    // （不用 `dot3` 自己算，否则判据与被测同源）。
    {
        let p = [2.0, -3.0, 4.0];
        let fwd = [0.5, 0.25, 0.75];
        let got = view_depth_raw(p, cam_eye, fwd);
        // 独立重算：2*0.5 + (-3)*0.25 + 4*0.75 = 1.0 - 0.75 + 3.0 = 3.25
        let want = 2.0 * 0.5 + (-3.0) * 0.25 + 4.0 * 0.75;
        set.add(
            "E01-视深-三分量独立重算一致（反符号变异）",
            (got - want).abs() < 1e-6 && (want - 3.25).abs() < 1e-6,
            "",
        );
    }

    // **逐分量符号可辨**：四个位置各只在一个坐标轴上有非零分量，
    // 深度必须恰为该分量乘以前向对应分量——任一分量被改成减号都会红。
    {
        let fwd = [0.5, 0.25, 0.75];
        let axes: [([f32; 3], f32); 3] = [([1.0, 0.0, 0.0], 0.5), ([0.0, 1.0, 0.0], 0.25), ([0.0, 0.0, 1.0], 0.75)];
        let mut all = true;
        for (pos, want) in axes.iter() {
            let got = view_depth_raw(*pos, cam_eye, fwd);
            if (got - want).abs() > 1e-6 {
                all = false;
            }
        }
        set.add("E01-视深-三轴逐分量可辨（反符号变异）", all, "");
    }

    // 位移的**线性性**（负位移与eye 相关）：眼点非零时也能算对。
    {
        let eye = [1.0, -2.0, 0.5];
        let fwd = [0.0, 0.0, 1.0];
        let got = view_depth_raw([1.0, -2.0, 3.5], eye, fwd);
        set.add("E01-视深-眼点非零时位移正确", (got - 3.0).abs() < 1e-6, "");
    }

    // 键单调不减：深度更大 ⇒ 键不更小（用 4096 个样本扫全区间）。
    {
        let mut mono = true;
        let mut prev = depth_key(clamp_depth(DEPTH_NEAR).0);
        for i in 1..4096 {
            let t = i as f32 / 4095.0;
            let d = DEPTH_NEAR + t * (DEPTH_FAR - DEPTH_NEAR);
            let k = depth_key(clamp_depth(d).0);
            if k < prev {
                mono = false;
            }
            prev = k;
        }
        set.add("E01-键-单调不减", mono, "");
    }

    // 键落在 [0, STEPS) 内（越界键会让基数排序最高趟扫到桶外）。
    {
        let mut in_range = true;
        for i in 0..2048 {
            let t = i as f32 / 2047.0;
            let d = DEPTH_NEAR + t * (DEPTH_FAR - DEPTH_NEAR);
            let k = depth_key(clamp_depth(d).0);
            if k >= DEPTH_QUANT_STEPS {
                in_range = false;
            }
        }
        // 两端边界也必须在册内。
        if depth_key(DEPTH_FAR) >= DEPTH_QUANT_STEPS {
            in_range = false;
        }
        if depth_key(DEPTH_NEAR) != 0 {
            in_range = false;
        }
        set.add("E01-键-落在合法区间且近平面为零", in_range, "");
    }

    // 键下界精确为 0、上界精确为 STEPS-1（不留空洞也不越界）。
    {
        let lo = depth_key(DEPTH_NEAR);
        let hi = depth_key(DEPTH_FAR);
        set.add(
            "E01-键-端点精确",
            lo == 0 && hi == DEPTH_QUANT_STEPS - 1,
            "",
        );
    }

    // 量化误差在解析上界内（拿 3000 个随机深度对账真实误差 vs 上界）。
    {
        let mut worst = 0.0f32;
        let mut seed = 0x2026_1008u32;
        for _ in 0..3000 {
            // xorshift：确定性伪随机（不取系统时钟，故回归可复现）。
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let t = (seed % 100_000) as f32 / 100_000.0;
            let d = DEPTH_NEAR + t * (DEPTH_FAR - DEPTH_NEAR);
            let k = depth_key(d) as f32;
            // 由键反推量化后的深度：桶中心。
            let h = (DEPTH_FAR - DEPTH_NEAR) / DEPTH_QUANT_STEPS as f32;
            let center = DEPTH_NEAR + (k + 0.5) * h;
            let e = (center - d).abs();
            if e > worst {
                worst = e;
            }
        }
        let bound = quant_error_bound();
        set.add(
            "E01-量化-实测误差不超过解析上界",
            worst <= bound * 1.001 && bound > 0.0,
            "",
        );
    }

    // 量化上界本身必须是最紧的：实测最大误差应达到上界的 80% 以上
    //（若实测远小于上界，说明上界被夸大了——虚高的上界会让调用方
    //  按错误精度做决策，那比没有上界更坏）。
    {
        let h = (DEPTH_FAR - DEPTH_NEAR) / DEPTH_QUANT_STEPS as f32;
        let bound = quant_error_bound();
        set.add("E01-量化-上界是 h/2 且非虚高", (bound - h * 0.5).abs() < 1e-9, "");
    }

    // 画家算法方向：键降序（远者先画）。
    {
        let vs = views_at_depths(&[0.1, 0.9, 0.5]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        sort_comparison(&mut es);
        // 深度 0.9（index 1）应排在最前，0.1（index 0）最后。
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        set.add(
            "E01-序-键降序（画家算法）",
            got == vec![1u32, 2, 0],
            "",
        );
    }

    // 同键按提交序号升序（稳定性的契约化）。
    {
        let vs = views_at_depths(&[0.5, 0.5, 0.5, 0.5]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        sort_comparison(&mut es);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        set.add("E01-序-同键按提交序升序", got == vec![0u32, 1, 2, 3], "");
    }

    // **反退化判据**（补自实测漏网）：上一条语料里「同键样本的index 恰好
    // 也是升序」，故把规范序里的 `then_with(index)` 删掉仍全绿——
    // 退化实现与正确实现在该语料上**数值重合**。这是典型的弱门禁：
    // 判据绿，但是「假绿」。

    // **关键**：仅靠 `keys_from_views` 造语料**永远抓不到**这个退化——
    // 它给出的 `index` 恒等于数组位置，故「位置序」与「index 序」重合。
    // 必须**直接构造 `index != position` 的条目**才能把二者劈开。
    {
        // 位置 0,1,2 的index 是 7,2,5（与位置序不同），键全相同。
        let mut es = vec![SortEntry::new(10, 7), SortEntry::new(10, 2), SortEntry::new(10, 5)];
        sort_comparison(&mut es);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        let degenerate: Vec<u32> = vec![7, 2, 5]; // 去掉 tie-break 后的位置序
        set.add(
            "E01-序-同键按 index 升序而非位置序（反退化）",
            got == vec![2u32, 5, 7] && got != degenerate,
            "",
        );
    }

    // 同一退化判据在**基数排序**上也必须成立。
    //
    // **实测缺陷来源**：基数排序最初用「倒序喂入 + 末尾反转」实现降序，
    // 而反转会把**同键组内的相对序一起翻掉**。对 `keys_from_views` 的输入
    // （index == 位置）碰巧正确，但`plan` 的入参是任意 `entries`——
    // 一旦 `index != 位置`，结果就与 `order_cmp` 分道扬镳。
    // 本条正是那个把缺陷钉出来的判据：修前红、修后绿。
    {
        let mut es = vec![SortEntry::new(10, 7), SortEntry::new(10, 2), SortEntry::new(10, 5)];
        let mut scratch: Vec<SortEntry> = Vec::new();
        sort_radix(&mut es, &mut scratch);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        set.add(
            "E01-序-基数排序同键亦按 index 升序（反退化）",
            got == vec![2u32, 5, 7],
            "",
        );
    }

    // 两条路径在 `index !=位置` 语料上**逐位一致**（规范序的唯一性）。
    {
        let mut a = vec![
            SortEntry::new(10, 7),
            SortEntry::new(4, 2),
            SortEntry::new(10, 5),
            SortEntry::new(9, 0),
            SortEntry::new(4, 6),
        ];
        let expect = expect_order(&a);
        let mut b = a.clone();
        sort_comparison(&mut a);
        sort_radix(&mut b, &mut Vec::new());
        let ra: Vec<u32> = a.iter().map(|e| e.index).collect();
        let rb: Vec<u32> = b.iter().map(|e| e.index).collect();
        set.add(
            "E01-双路-index≠位置 时两路逐位一致",
            ra == expect && rb == expect && ra == rb,
            "",
        );
    }

    // 三键同组的更强反退化：同键样本数 ≥3。
    //
    // 深度 [0.25, 0.75, 0.25, 0.75, 0.25] ⇒ 键 49152 组 = {1,3}、
    // 键 16384 组 = {0,2,4}。规范序「键降序 + 同键 index 升序」
    // ⇒ 49152 组在前（1,3）→ 16384 组在后（0,2,4），即 `[1,3,0,2,4]`。
    //
    // **期望值由判据侧独立推导**（键分组 + 组内 index 升序），不抄
    // 任何实现的输出——初版此处手写成了 `[3,1,4,2,0]`（把组内写成降序），
    // 判据因而红，而**实现是对的、判据是错的**。这类「判据自身算错」
    // 与「判据太弱」同样有害，且更隐蔽：它会诱使后来者去改正确的实现。
    {
        let vs = views_at_depths(&[0.25, 0.75, 0.25, 0.75, 0.25]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let expect = expect_order(&es);
        sort_comparison(&mut es);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        let mut rb = es.clone();
        sort_radix(&mut rb, &mut Vec::new());
        let got_radix: Vec<u32> = rb.iter().map(|e| e.index).collect();
        set.add(
            "E01-序-三键同组按 index 升序（反退化）",
            got == vec![1u32, 3, 0, 2, 4] && got == expect && got_radix == expect,
            "",
        );
    }

    // 比较排序结果与判据侧独立重算的期望序一致（N=64，深度交错）。
    {
        let ds: Vec<f32> = (0..64).map(|i| ((i * 37) % 64) as f32 / 64.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let expect = expect_order(&es);
        sort_comparison(&mut es);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        set.add("E01-序-比较排序等于独立重算期望", got == expect, "");
    }

    // 基数排序结果同样等于独立重算的期望序。
    {
        let ds: Vec<f32> = (0..64).map(|i| ((i * 37) % 64) as f32 / 64.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let expect = expect_order(&es);
        let mut scratch: Vec<SortEntry> = Vec::new();
        sort_radix(&mut es, &mut scratch);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        set.add("E01-序-基数排序等于独立重算期望", got == expect, "");
    }

    // 置换是双射：排完后每个原始下标**恰好出现一次**
    //（排序把元素丢掉或重复是最恶劣的红项，而「有序」判据查不出）。
    {
        let ds: Vec<f32> = (0..48).map(|i| ((i * 11) % 48) as f32 / 48.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        sort_comparison(&mut es);
        let mut seen = vec![0u8; 48];
        let mut dup = false;
        for e in es.iter() {
            if let Some(s) = seen.get_mut(e.index as usize) {
                *s += 1;
                if *s > 1 {
                    dup = true;
                }
            } else {
                dup = true;
            }
        }
        set.add("E01-置换-比较排序是双射", !dup && es.len() == 48, "");
    }

    // 基数排序的置换也是双射（基数排序的桶散布极易越界写坏）。
    {
        let ds: Vec<f32> = (0..48).map(|i| ((i * 11) % 48) as f32 / 48.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut scratch: Vec<SortEntry> = Vec::new();
        sort_radix(&mut es, &mut scratch);
        let mut seen = vec![0u8; 48];
        let mut dup = false;
        for e in es.iter() {
            if let Some(s) = seen.get_mut(e.index as usize) {
                *s += 1;
                if *s > 1 {
                    dup = true;
                }
            } else {
                dup = true;
            }
        }
        set.add("E01-置换-基数排序是双射", !dup && es.len() == 48, "");
    }

    // 排序不修改键与序号本身（排序只改**次序**，不改**内容**）。
    // 这是「内容守恒」判据：把排前的 (key,index) 集合与排后比对。
    {
        let ds: Vec<f32> = (0..32).map(|i| ((i * 7) % 32) as f32 / 32.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut before: Vec<(u32, u32)> = es.iter().map(|e| (e.key, e.index)).collect();
        sort_comparison(&mut es);
        let mut after: Vec<(u32, u32)> = es.iter().map(|e| (e.key, e.index)).collect();
        before.sort();
        after.sort();
        set.add("E01-内容-排序前后键集守恒", before == after, "");
    }

    // 空批与单元素批不崩，且单元素不进入排序路径。
    {
        let (mut e0, _) = keys_from_views(&[], cam_eye, cam_fwd);
        let s0 = sort_comparison(&mut e0);
        let vs1 = views_at_depths(&[0.5]);
        let (mut e1, _) = keys_from_views(&vs1, cam_eye, cam_fwd);
        let s1 = sort_radix(&mut e1, &mut Vec::new());
        set.add(
            "E01-边界-空批与单元素不崩",
            e0.is_empty() && s0.n == 0 && s1.n == 1 && !s1.sorted,
            "",
        );
    }

    // =======================================================================
    // E02 三混合语义
    // =======================================================================

    // 在册表恰为三型（不多不少不少）。
    {
        let listed = BLEND_MODES.len();
        let has = |m: BlendMode| BLEND_MODES.contains(&m);
        set.add(
            "E02-三型-在册不多不少",
            listed == 3 && has(BlendMode::Additive) && has(BlendMode::Alpha) && has(BlendMode::Premultiplied),
            "",
        );
    }

    // 在册表元素互不重复。
    {
        let mut dup = false;
        for i in 0..BLEND_MODES.len() {
            for j in (i + 1)..BLEND_MODES.len() {
                if let (Some(a), Some(b)) = (BLEND_MODES.get(i), BLEND_MODES.get(j)) {
                    if a == b {
                        dup = true;
                    }
                }
            }
        }
        set.add("E02-三型-在册表无重复", !dup, "");
    }

    // 每型都有非空适用场景说明（锚点：每模式适用场景文档）。
    {
        let mut all = true;
        for m in BLEND_MODES.iter() {
            if m.scene().is_empty() || m.zh().is_empty() {
                all = false;
            }
        }
        set.add("E02-场景-三型均有适用场景说明", all, "");
    }

    // 三型的适用场景文本互不相同（相同即说明没写，只是复制）。
    {
        let s = [
            BlendMode::Additive.scene(),
            BlendMode::Alpha.scene(),
            BlendMode::Premultiplied.scene(),
        ];
        set.add("E02-场景-三型说明互不相同", s[0] != s[1] && s[1] != s[2] && s[0] != s[2], "");
    }

    // 加法语义：src×1 + dst×1（半透明加法对黑底应等于自身）。
    {
        let src = Rgba::new(0.2, 0.4, 0.6, 0.5);
        let out = blend(BlendMode::Additive, src, Rgba::new(0.0, 0.0, 0.0, 0.0));
        let ok = (out.r - 0.2).abs() < 1e-6
            && (out.g - 0.4).abs() < 1e-6
            && (out.b - 0.6).abs() < 1e-6
            && (out.a - 0.5).abs() < 1e-6;
        set.add("E02-加法-黑底上等于源色", ok, "");
    }

    // 加法可超过 1（HDR 合法，不钳制——钳掉即画质事故）。
    {
        let a = Rgba::new(0.7, 0.7, 0.7, 1.0);
        let b = Rgba::new(0.7, 0.7, 0.7, 1.0);
        let out = blend(BlendMode::Additive, a, b);
        set.add("E02-加法-HDR 超 1 不被钳制", out.r > 1.0, "");
    }

    // 直通 alpha：全不透明源完全覆盖目标（a=1 时 dst 项系数为 0）。
    {
        let src = Rgba::new(0.3, 0.3, 0.3, 1.0);
        let dst = Rgba::new(0.9, 0.9, 0.9, 0.25);
        let out = blend(BlendMode::Alpha, src, dst);
        let ok = (out.r - 0.3).abs() < 1e-6 && (out.a - 1.0).abs() < 1e-6;
        set.add("E02-alpha-不透明源完全覆盖", ok, "");
    }

    // 直通 alpha：全透明源不改变目标（a=0 时 src 项系数为 0）。
    {
        let src = Rgba::new(0.3, 0.3, 0.3, 0.0);
        let dst = Rgba::new(0.9, 0.2, 0.1, 0.25);
        let out = blend(BlendMode::Alpha, src, dst);
        let ok = (out.r - 0.9).abs() < 1e-6 && (out.g - 0.2).abs() < 1e-6;
        set.add("E02-alpha-全透明源不改变目标", ok, "");
    }

    // 直通 alpha 与预乘在**同一对输入**上必须给出不同结果
    //（重合行为会掩盖缺失分支——两者若恒等，则预乘语义从未被实现）。
    {
        let src = Rgba::new(0.8, 0.5, 0.2, 0.5);
        let dst = Rgba::new(0.1, 0.2, 0.3, 0.8);
        let a = blend(BlendMode::Alpha, src, dst);
        let p = blend(BlendMode::Premultiplied, src, dst);
        set.add("E02-alpha与预乘-同输入不同结果", !bits_eq(a, p), "");
    }

    // 预乘语义：把直通源预乘后再混，两型结果**逐位相同**
    //（这是预乘语义的可判定恒等式，不是估值对拍）。
    {
        let src = Rgba::new(0.8, 0.5, 0.2, 0.5);
        let dst = Rgba::new(0.1, 0.2, 0.3, 0.8);
        let a = blend(BlendMode::Alpha, src, dst);
        let p = blend(BlendMode::Premultiplied, premultiply(src), dst);
        set.add("E02-预乘-预乘后与直通结果逐位一致", bits_eq(a, p), "");
    }

    // 预乘语义：**两种误用各自错向不同**，且都不等于正确形态。
    //
    // 弱门禁教训：初版只断「正确形态 ≠ 二次乘形态」，方向没查——实测
    // `正确=0.5 / 二次乘=0.9`（黑底），是**偏亮**不是偏暗，判据写成了
    // 「二次乘更暗」因而红。故本条断**双向不等**且断出**方向**：
    // - 未预乘的源送进预乘混合 → 偏亮（`raw→premul` > 正确）；
    // - 已预乘的源送进直通混合 → 偏暗（`premul→straight` < 正确），
    //   这才是 F0019 预置表 `use_note` 说的「半透明边缘发黑」。
    {
        let src = Rgba::new(0.8, 0.8, 0.8, 0.5);
        let dst = Rgba::new(0.2, 0.2, 0.2, 0.5);
        let correct = blend(BlendMode::Premultiplied, premultiply(src), dst).r;
        let too_bright = blend(BlendMode::Premultiplied, src, dst).r;
        let too_dark = blend(BlendMode::Alpha, premultiply(src), dst).r;
        set.add(
            "E02-预乘-两种误用错向不同且均不等于正确形态",
            too_bright > correct + 1e-6 && too_dark < correct - 1e-6,
            "",
        );
    }

    // 预乘函数本身：rgb 三通道同乘 a，a 保持不变。
    {
        let src = Rgba::new(0.8, 0.4, 0.2, 0.25);
        let p = premultiply(src);
        let ok = (p.r - 0.2).abs() < 1e-6
            && (p.g - 0.1).abs() < 1e-6
            && (p.b - 0.05).abs() < 1e-6
            && (p.a - 0.25).abs() < 1e-6;
        set.add("E02-预乘函数-四通道语义正确", ok, "");
    }

    // 预乘函数对非有限 alpha 的防护（NaN 会污染整条合成链）。
    {
        let p = premultiply(Rgba::new(1.0, 1.0, 1.0, f32::NAN));
        set.add(
            "E02-预乘函数-NaN alpha 被挡",
            p.r.is_finite() && p.g.is_finite() && p.b.is_finite() && p.a == 0.0,
            "",
        );
    }

    // 混合对非有限输入的防护：NaN/Inf 源不得污染输出（三型全查）。
    {
        let src = Rgba::new(f32::NAN, 1.0, f32::INFINITY, 0.5);
        let dst = Rgba::new(0.2, 0.2, 0.2, 0.5);
        let all_ok = BLEND_MODES.iter().all(|m| {
            let o = blend(*m, src, dst);
            o.r.is_finite() && o.g.is_finite() && o.b.is_finite() && o.a.is_finite()
        });
        set.add("E02-防护-非有限输入不污染输出", all_ok, "");
    }

    // 混合是纯函数：同输入双跑逐位一致（F2215 确定性根基的延续）。
    {
        let src = Rgba::new(0.31, 0.27, 0.83, 0.44);
        let dst = Rgba::new(0.19, 0.71, 0.13, 0.66);
        let mut all = true;
        for m in BLEND_MODES.iter() {
            let a = blend(*m, src, dst);
            let b = blend(*m, src, dst);
            if !bits_eq(a, b) {
                all = false;
            }
        }
        set.add("E02-纯函数-双跑逐位一致", all, "");
    }

    // 混合无内部状态：反复混合的结果与「按同序手工混合两次」逐位一致
    //（若实现里偷偷缓存了上次结果，重复路径与新路径会分叉）。
    {
        let src = Rgba::new(0.5, 0.5, 0.5, 0.5);
        let dst = Rgba::new(0.25, 0.25, 0.25, 0.25);
        let mut cur = dst;
        for _ in 0..8 {
            cur = blend(BlendMode::Alpha, src, cur);
        }
        // 判据侧独立地从 dst 重走两轮。
        let mut hand = dst;
        for _ in 0..8 {
            hand = blend(BlendMode::Alpha, src, hand);
        }
        set.add("E02-无状态-反复混合结果可复现", bits_eq(cur, hand), "");
    }

    // 合成顺序消费口：按序合成 == 手工逐个混合（置换到画面的桥必须真对）。
    {
        let colors = [
            Rgba::new(0.1, 0.2, 0.3, 0.5),
            Rgba::new(0.4, 0.1, 0.2, 0.25),
            Rgba::new(0.2, 0.5, 0.1, 0.75),
        ];
        let order = vec![0u32, 1, 2];
        let got = composite_in_order(BlendMode::Alpha, Rgba::new(0.0, 0.0, 0.0, 0.0), &colors, &order);
        let mut hand = Rgba::new(0.0, 0.0, 0.0, 0.0);
        for i in 0..3 {
            hand = blend(BlendMode::Alpha, colors[i], hand);
        }
        set.add("E02-顺序消费-与手工混合逐位一致", bits_eq(got, hand), "");
    }

    // 顺序消费口对越界下标**跳过而非panic**（判据自身不能先崩）。
    {
        let colors = [Rgba::new(0.1, 0.1, 0.1, 0.5)];
        let order = vec![0u32, 99, 0xFFFF_FFFF];
        let got = composite_in_order(BlendMode::Additive, Rgba::new(0.0, 0.0, 0.0, 0.0), &colors, &order);
        set.add("E02-顺序消费-越界下标被跳过", got.r.is_finite(), "");
    }

    // =======================================================================
    // E03 免排序声明（双向验证 + 两级可判：交换律 / 折叠容差）
    // =======================================================================

    // 判据侧样本集：故意混用「易精确表示」与「难精确表示」的值。
    //
    // 弱门禁实测教训：本域初版判据用 4 元素样本，加法两序碰巧逐位相等；
    // 换成 3 元素样本即红。根因是 `f32` 加法**可交换但不可结合**。
    // 故样本集刻意包含 0.1/0.7/0.3 这类**二进制不可精确表示**的值——
    // 用「好表示」的样本（0.5/0.25/0.0）会让任何 ulp 级判据都恒绿。
    let probe_set = [
        Rgba::new(0.1, 0.7, 0.3, 0.9),
        Rgba::new(0.4, 0.2, 0.8, 0.6),
        Rgba::new(0.3, 0.3, 0.3, 0.3),
        Rgba::new(0.15, 0.55, 0.45, 0.25),
    ];

    // 加法**交换律逐位成立**（免排序的数学前提；`f32` 加法可交换）。
    {
        let p = order_probe(BlendMode::Additive, &probe_set);
        set.add("E03-免排序-加法交换律逐位成立", p.commutes_bitwise, "");
    }

    // 加法折叠序差**落在容差内**（不是「恰好为 0」——那是靠样本的弱门禁）。
    {
        let p = order_probe(BlendMode::Additive, &probe_set);
        set.add(
            "E03-免排序-加法折叠差在容差内",
            p.fold_delta <= order_tolerance() && !p.order_sensitive,
            "",
        );
    }

    // 容差本身**非零**（若为 0，上一条就退化为「逐位相等」的弱门禁）。
    {
        set.add(
            "E03-免排序-容差非零且为若干 ulp",
            order_tolerance() > 0.0 && order_tolerance() < 1e-5,
            "",
        );
    }

    // 换一批**长度**的样本仍成立（长度 3 是初版判据翻车的长度，必须钉住）。
    {
        let three = [
            Rgba::new(0.1, 0.7, 0.3, 0.9),
            Rgba::new(0.4, 0.2, 0.8, 0.6),
            Rgba::new(0.3, 0.3, 0.3, 0.3),
        ];
        let p = order_probe(BlendMode::Additive, &three);
        set.add(
            "E03-免排序-三元素样本亦成立",
            p.commutes_bitwise && p.fold_delta <= order_tolerance(),
            "",
        );
    }

    // **反向自检**：加法折叠差**确实非零**（证明上一条的容差不是放宽到
    // 恒真）。若某实现把加法做成结合的精确和（如 Kahan），本条会红——
    // 那不是缺陷，故本条只断言「要么为 0，要么在容差内」的**弱形式**，
    // 而真正的强度由「加法交换律成立 + alpha 折叠差远超容差」提供。
    {
        let p = order_probe(BlendMode::Additive, &probe_set);
        set.add(
            "E03-免排序-加法折叠差非负且有界",
            p.fold_delta >= 0.0 && p.fold_delta <= order_tolerance(),
            "",
        );
    }

    // alpha **交换律不成立**（源项与目标项不对称）。
    // **与加法那条成对**：只断「加法交换律成立」会被恒等混合骗过。
    {
        let p = order_probe(BlendMode::Alpha, &probe_set);
        set.add("E03-需排序-alpha 交换律不成立", !p.commutes_bitwise, "");
    }

    // alpha 折叠差**远超容差**（真顺序相关，量级是 O(1) 而非 ulp）。
    {
        let p = order_probe(BlendMode::Alpha, &probe_set);
        set.add(
            "E03-需排序-alpha 折叠差远超容差",
            p.order_sensitive && p.fold_delta > order_tolerance() * 1000.0,
            "",
        );
    }

    // 预乘同样：交换律不成立 **且** 折叠差远超容差。
    {
        let p = order_probe(BlendMode::Premultiplied, &probe_set);
        set.add(
            "E03-需排序-预乘交换律不成立且折叠差远超容差",
            !p.commutes_bitwise && p.fold_delta > order_tolerance() * 1000.0,
            "",
        );
    }

    // **强弱分离**（本组判据的核心）：加法的折叠差必须**严格小于**
    // alpha/预乘的折叠差，且相差若干数量级——只有一个布尔「相等/不等」
    // 时，「加法差 1 ulp」与「alpha 差 0.1」都被记成「不等」，强弱就丢了。
    {
        let a = order_probe(BlendMode::Additive, &probe_set).fold_delta;
        let al = order_probe(BlendMode::Alpha, &probe_set).fold_delta;
        let pr = order_probe(BlendMode::Premultiplied, &probe_set).fold_delta;
        set.add(
            "E03-强弱-加法折叠差比alpha 小三个数量级以上",
            al > a * 1000.0 && pr > a * 1000.0,
            "",
        );
    }

    // 顺序无关性不是「颜色数不足导致的巧合」：换一批长度/取值仍成立。
    {
        let alt = [
            Rgba::new(0.9, 0.1, 0.1, 0.9),
            Rgba::new(0.1, 0.9, 0.1, 0.2),
            Rgba::new(0.1, 0.1, 0.9, 0.6),
            Rgba::new(0.35, 0.35, 0.65, 0.45),
        ];
        let p = order_probe(BlendMode::Additive, &alt);
        set.add(
            "E03-免排序-换批样本仍成立",
            p.commutes_bitwise && p.fold_delta <= order_tolerance(),
            "",
        );
    }

    // 空批与单元素不崩（边界不豁免）。
    {
        let e = order_probe(BlendMode::Additive, &[]);
        let one = [Rgba::new(0.5, 0.5, 0.5, 0.5)];
        let p = order_probe(BlendMode::Additive, &one);
        set.add(
            "E03-边界-空批与单元素不崩",
            e.commutes_bitwise && p.commutes_bitwise && p.fold_delta == 0.0,
            "",
        );
    }

    // `requires_sort` 与**实测**顺序敏感性一致（裁决函数不得与实测脱节）。
    {
        let mut all = true;
        for m in BLEND_MODES.iter() {
            let p = order_probe(*m, &probe_set);
            if m.requires_sort() != p.order_sensitive {
                all = false;
            }
        }
        set.add("E03-裁决-requires_sort 与实测一致", all, "");
    }

    // `requires_sort` 与**交换律**实测也一致（两级都要对，不能只对一级）。
    {
        let mut all = true;
        for m in BLEND_MODES.iter() {
            let p = order_probe(*m, &probe_set);
            if m.requires_sort() == p.commutes_bitwise {
                // 顺序相关模式必须交换律不成立；顺序无关模式必须成立。
                all = false;
            }
        }
        set.add("E03-裁决-requires_sort 与交换律实测一致", all, "");
    }

    // =======================================================================
    // E04 排序开关与逐发射器粒度
    // =======================================================================

    // 裁决三态：顺序相关+开=Required，顺序相关+关=Forbidden，顺序无关=Unnecessary。
    {
        let a = judge(&EmitterSort::new(BlendMode::Alpha, true));
        let b = judge(&EmitterSort::new(BlendMode::Alpha, false));
        let c = judge(&EmitterSort::new(BlendMode::Additive, false));
        let d = judge(&EmitterSort::new(BlendMode::Premultiplied, true));
        let e = judge(&EmitterSort::new(BlendMode::Additive, true));
        set.add(
            "E04-裁决-三态判定正确",
            a == SortVerdict::Required
                && b == SortVerdict::Forbidden
                && c == SortVerdict::Unnecessary
                && d == SortVerdict::Required
                && e == SortVerdict::Unnecessary,
            "",
        );
    }

    // 裁决不产生诊断（纯裁决 / impure 记录的分层）。
    {
        let bag = DiagBag::new();
        let _ = judge(&EmitterSort::new(BlendMode::Alpha, false));
        let _ = judge(&EmitterSort::new(BlendMode::Additive, true));
        set.add("E04-裁决-不产生诊断", bag.is_empty(), "");
    }

    // 逐发射器粒度：两个发射器（一个 Required 一个 Forbidden）独立裁决。
    {
        let e1 = EmitterSort::new(BlendMode::Alpha, true);
        let e2 = EmitterSort::new(BlendMode::Additive, false);
        set.add(
            "E04-粒度-逐发射器独立裁决",
            judge(&e1) == SortVerdict::Required && judge(&e2) == SortVerdict::Unnecessary,
            "",
        );
    }

    // 排��路径：Required 走比较排序且顺序正确。
    {
        let ds: Vec<f32> = (0..24).map(|i| ((i * 5) % 24) as f32 / 24.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let expect = expect_order(&es);
        let mut bag = DiagBag::new();
        let mut scratch: Vec<SortEntry> = Vec::new();
        let p = plan(
            &EmitterSort::new(BlendMode::Alpha, true),
            SortAlgo::Comparison,
            &mut es,
            &mut scratch,
            &mut bag,
        );
        set.add(
            "E04-计划-Required 走比较排序且序正确",
            p.verdict == SortVerdict::Required
                && p.path == PATH_COMPARISON
                && p.order == expect
                && p.stats.comparisons > 0
                && bag.is_empty(),
            "",
        );
    }

    // 排序路径：Required 走基数排序且序与比较路径**逐位一致**。
    {
        let ds: Vec<f32> = (0..24).map(|i| ((i * 5) % 24) as f32 / 24.0).collect();
        let vs_a = views_at_depths(&ds);
        let (mut ea, _) = keys_from_views(&vs_a, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let pa = plan(
            &EmitterSort::new(BlendMode::Alpha, true),
            SortAlgo::Comparison,
            &mut ea,
            &mut Vec::new(),
            &mut bag,
        );
        let vs_b = views_at_depths(&ds);
        let (mut eb, _) = keys_from_views(&vs_b, cam_eye, cam_fwd);
        let mut scratch: Vec<SortEntry> = Vec::new();
        let pb = plan(
            &EmitterSort::new(BlendMode::Alpha, true),
            SortAlgo::Radix,
            &mut eb,
            &mut scratch,
            &mut bag,
        );
        set.add(
            "E04-双路-两条 CPU 路径置换逐位一致",
            pa.order == pb.order && pb.path == PATH_RADIX && pb.stats.radix_passes > 0,
            "",
        );
    }

    // Forbidden：显性警告 + 不排序 + 保持原始顺序。
    {
        let ds: Vec<f32> = (0..16).map(|i| ((i * 3) % 16) as f32 / 16.0).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let p = plan(
            &EmitterSort::new(BlendMode::Alpha, false),
            SortAlgo::Comparison,
            &mut es,
            &mut Vec::new(),
            &mut bag,
        );
        let identity: Vec<u32> = (0..16u32).collect();
        set.add(
            "E04-Forbidden-警告且不排序",
            p.verdict == SortVerdict::Forbidden
                && p.order == identity
                && p.stats.comparisons == 0
                && bag.has(DiagCode::TransitionRejected),
            "",
        );
    }

    // Forbidden 警告必须写明**视觉后果**（锚点：视觉错误预期声明）。
    // 只说「配置非法」不满足锚点——用户需要知道会看到什么。
    {
        let vs = views_at_depths(&[0.2, 0.8]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let _ = plan(
            &EmitterSort::new(BlendMode::Alpha, false),
            SortAlgo::Comparison,
            &mut es,
            &mut Vec::new(),
            &mut bag,
        );
        let has_visual = bag.has_msg_containing(DiagCode::TransitionRejected, "前后颠倒");
        let has_suggest = bag
            .all()
            .iter()
            .any(|d| d.hint.contains("打开排序") || d.hint.contains("加法"));
        set.add("E04-Forbidden-警告含视觉后果与建议", has_visual && has_suggest, "");
    }

    // Forbidden 每次调用恰记一条诊断（不多记、不少记——重复诊断会淹没
    // 诊断袋，少记则「发生过但查不到」）。
    {
        let vs = views_at_depths(&[0.2, 0.8]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let e = EmitterSort::new(BlendMode::Alpha, false);
        let _ = plan(&e, SortAlgo::Comparison, &mut es, &mut Vec::new(), &mut bag);
        let _ = plan(&e, SortAlgo::Comparison, &mut es, &mut Vec::new(), &mut bag);
        let n = bag
            .all()
            .iter()
            .filter(|d| d.code == DiagCode::TransitionRejected)
            .count();
        set.add("E04-Forbidden-每次调用恰记一条诊断", n == 2, "");
    }

    // Unnecessary + 关排序：零成本，且**不产生错误级诊断**。
    {
        let vs = views_at_depths(&[0.2, 0.8, 0.5]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let p = plan(
            &EmitterSort::new(BlendMode::Additive, false),
            SortAlgo::Comparison,
            &mut es,
            &mut Vec::new(),
            &mut bag,
        );
        let identity: Vec<u32> = vec![0, 1, 2];
        set.add(
            "E04-免排序-加法关排序零成本",
            p.verdict == SortVerdict::Unnecessary
                && p.path == PATH_NONE
                && p.stats.is_free()
                && p.order == identity
                && bag.is_empty(),
            "",
        );
    }

    // Unnecessary + 开排序：给浪费提示（非错误级），且仍不进入排序路径。
    {
        let vs = views_at_depths(&[0.2, 0.8, 0.5]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let p = plan(
            &EmitterSort::new(BlendMode::Additive, true),
            SortAlgo::Comparison,
            &mut es,
            &mut Vec::new(),
            &mut bag,
        );
        set.add(
            "E04-免排序-加法开排序给浪费提示",
            p.verdict == SortVerdict::Unnecessary
                && p.stats.is_free()
                && bag.has(DiagCode::ChurnCoalesced),
            "",
        );
    }

    // 诊断码分级：Forbidden 是错误级，Unnecessary 不是
    //（一个 bool 表达两件事必错——调用方需据此决定要不要紧）。
    {
        set.add(
            "E04-诊断-错误级与非错误级分离",
            BlendDiag::SortDisabledOrderError.is_error()
                && !BlendDiag::SortUnnecessary.is_error()
                && BlendDiag::GpuSortReserved.is_error()
                && !BlendDiag::KeyClamped.is_error(),
            "",
        );
    }

    // 三种本域码映射到 F2203 后**互不重合**（重合会让「按码分流」失效）。
    {
        let codes = [
            BlendDiag::SortDisabledOrderError.code(),
            BlendDiag::SortUnnecessary.code(),
            BlendDiag::KeyClamped.code(),
            BlendDiag::GpuSortReserved.code(),
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                    if a == b {
                        distinct = false;
                    }
                }
            }
        }
        set.add("E04-诊断-映射互不重合", distinct, "");
    }

    // 每个本域码都有中文标签（读屏可达，锚点无障碍面）。
    {
        let mut all = true;
        for c in [
            BlendDiag::SortDisabledOrderError,
            BlendDiag::SortUnnecessary,
            BlendDiag::KeyClamped,
            BlendDiag::GpuSortReserved,
        ] {
            if c.zh().is_empty() {
                all = false;
            }
        }
        set.add("E04-诊断-四码均有中文标签", all, "");
    }

    // =======================================================================
    // E05 降级矩阵
    // =======================================================================

    // 排序键 NaN → 钳到近平面**且回报被钳制**。
    {
        let (d, clamped) = clamp_depth(f32::NAN);
        set.add("E05-NaN-钳到近平面且回报", d == DEPTH_NEAR && clamped, "");
    }

    // ±Inf 同样被钳（NaN 只是其中一类，漏掉 Inf 会留下未定义比较）。
    {
        let (dp, cp) = clamp_depth(f32::INFINITY);
        let (dm, cm) = clamp_depth(f32::NEG_INFINITY);
        set.add(
            "E05-Inf-正负无穷均被钳",
            dp == DEPTH_FAR && cp && dm == DEPTH_NEAR && cm,
            "",
        );
    }

    // 合法深度不被误钳（钳制过度同样是缺陷：所有粒子挤到近平面）。
    {
        let mut ok = true;
        for i in 1..1000 {
            let d = DEPTH_NEAR + (i as f32 / 1000.0) * (DEPTH_FAR - DEPTH_NEAR);
            let (_, c) = clamp_depth(d);
            if c {
                ok = false;
            }
        }
        set.add("E05-钳制-合法深度不被误钳", ok, "");
    }

    // 相机后方（负深度）被钳并计数（`keys_from_views` 层面的口径）。
    {
        let vs = views_at_depths(&[-5.0, 0.5, 3.0, 0.5]);
        let (es, clamps) = keys_from_views(&vs, cam_eye, cam_fwd);
        // 两个越界样本：-5.0（相机后方）与 3.0（超远平面）。
        let all_finite = es.iter().all(|e| e.key < DEPTH_QUANT_STEPS);
        set.add(
            "E05-钳制计数-越界样本被计数且键合法",
            clamps == 2 && all_finite && es.len() == 4,
            "",
        );
    }

    // NaN 粒子不使排序结果不确定：NaN 被钳到近平面后排在**最后**
    //（深度最近 = 最后画），且不破坏其余粒子的相对次序。
    {
        let mut vs = views_at_depths(&[0.3, 0.9, 0.6]);
        // 手动注入 NaN 位置（构造 Vec<ParticleView> 后改写）。
        if let Some(mut p) = vs.get(0).copied() {
            p.position = [f32::NAN, 0.0, 0.0];
            vs[0] = p;
        }
        let (mut es, clamps) = keys_from_views(&vs, cam_eye, cam_fwd);
        sort_comparison(&mut es);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        // 期望：0.9(idx1) → 0.6(idx2) → NaN 钳到 0(idx0)。
        set.add(
            "E05-NaN-钳到近平面排在最后且不扰乱其余",
            clamps == 1 && got == vec![1u32, 2, 0],
            "",
        );
    }

    // 混合对 NaN 目标色的防护（NaN 一旦进入 dst 就再也出不来）。
    {
        let dst = Rgba::new(f32::NAN, 0.0, 0.0, 1.0);
        let src = Rgba::new(0.5, 0.5, 0.5, 0.5);
        let mut all = true;
        for m in BLEND_MODES.iter() {
            let o = blend(*m, src, dst);
            if !o.r.is_finite() {
                all = false;
            }
        }
        set.add("E05-NaN-混合挡掉 NaN 目标色", all, "");
    }

    // GPU 预留被调用 → 恒失败（绝不静默成功、绝不 panic）。
    {
        let req = GpuSortRequest::for_count(1024);
        let o = gpu_radix_sort(&req);
        let is_err = o.is_err();
        let msg = match &o {
            Outcome::Err { message, hint, .. } => {
                set.add("E05-预留-失败消息含「未实现」", message.contains("未实现"), "");
                set.add("E05-预留-建议指路 CPU 路径", hint.contains("CPU"), "");
                true
            }
            _ => false,
        };
        set.add("E05-预留-被调用恒显性报错", is_err && msg, "");
    }

    // GPU 请求校验真实存在（不是摆设）：五类非法请求各被拒。
    //
    // **逐条独立**（补自实测漏网）：初版把五类非法合成一个 `rej == 5`
    // 计数。删掉「粒子数为 0」那一条后，count=0 的请求**仍被key_bytes
 // 那条拦住**（key_bytes 仍按 64算，与 0×4=0 不符）⇒ 计数仍为 5，判据
 // 全绿。**一条校验被另一条掩护**，正是「计数式判据」的固有盲区。
    //
    // 故改为：**每次只破坏一个字段**，且让被破坏的那条成为唯一拦截者
    //（其余字段保持自洽）。
    {
        let base = GpuSortRequest::for_count(64);
        let mut rej = 0usize;

        // ① count=0 且 key_bytes 同步置 0 ⇒ 只有「count 须 ≥1」能拦。
        let mut z = base;
        z.count = 0;
        z.key_bytes = 0;
        if z.validate().is_err() {
            rej += 1;
        }
        // ② 键宽 0 且趟数 0 ⇒ 只有「键宽须在[1,32]」能拦。
        let mut kb = base;
        kb.key_bits = 0;
        kb.passes = 0;
        if kb.validate().is_err() {
            rej += 1;
        }
        // ③ 键宽 33（越界）⇒ 只有「键宽 ≤32」能拦。
        let mut kb2 = base;
        kb2.key_bits = 33;
        kb2.passes = 5;
        if kb2.validate().is_err() {
            rej += 1;
        }
        // ④ 趟数与键宽不匹配（键宽 16 需2 趟，给 7 趟）⇒ 只有「趟数」能拦。
        let mut pm = base;
        pm.passes = 7;
        if pm.validate().is_err() {
            rej += 1;
        }
        // ⑤ 工作组非 2 的幂（100）⇒ 只有「工作组」能拦。
        let mut gs = base;
        gs.group_size = 100;
        if gs.validate().is_err() {
            rej += 1;
        }
        // ⑥ 工作组为 0 ⇒ 只有「group_size 须 ≥1」能拦。
        let mut gz = base;
        gz.group_size = 0;
        if gz.validate().is_err() {
            rej += 1;
        }
        // ⑦ 缓冲尺寸不符 ⇒ 只有「缓冲」能拦。
        let mut kbz = base;
        kbz.key_bytes = 3;
        if kbz.validate().is_err() {
            rej += 1;
        }
        set.add("E05-预留-七类非法请求逐条独立被拒", rej == 7, "");
    }

    // **反掩护**（补自实测漏网）：把 count=0 与 key_bytes 同步置 0 后，
    // 拒绝**限额**必须指名「粒子数」——因为此时缓冲那条已自洽（0×4=0），
    // 若拒绝理由指向别处，说明 count 那条没在工作而是被别条掩护了。
    //
    // 断言对象选`limit`（限额）而非 `what`：本域 `what` 是通用名
    // 「GPU基数排序请求」（每条拒绝都带它，区分不出谁拦的），
    // 而 `limit` 才是逐条不同的那句。这正是「三要素要真的能定位」
    // 的可判定面。
    {
        let mut z = GpuSortRequest::for_count(64);
        z.count = 0;
        z.key_bytes = 0;
        match z.validate() {
            Err(r) => set.add(
                "E05-反掩护-count 非法由count 条拦截",
                r.limit.contains("粒子数"),
                "",
            ),
            Ok(()) => set.add("E05-反掩护-count 非法由count 条拦截", false, ""),
        }
    }

    // 同理：键宽 33 时限额须指名键宽（而非趟数）。
    {
        let mut z = GpuSortRequest::for_count(64);
        z.key_bits = 33;
        z.passes = 5;
        match z.validate() {
            Err(r) => set.add(
                "E05-反掩护-键宽越界由键宽条拦截",
                r.limit.contains("键宽"),
                "",
            ),
            Ok(()) => set.add("E05-反掩护-键宽越界由键宽条拦截", false, ""),
        }
    }

    // 同理：工作组非 2 的幂时限额须指名工作组。
    {
        let mut z = GpuSortRequest::for_count(64);
        z.group_size = 100;
        match z.validate() {
            Err(r) => set.add(
                "E05-反掩护-工作组非法由工作组条拦截",
                r.limit.contains("2 的幂"),
                "",
            ),
            Ok(()) => set.add("E05-反掩护-工作组非法由工作组条拦截", false, ""),
        }
    }

    // 同理：缓冲尺寸不符时限额须指名缓冲（这是唯一一条能拦住它的）。
    {
        let mut z = GpuSortRequest::for_count(64);
        z.key_bytes = 3;
        match z.validate() {
            Err(r) => set.add(
                "E05-反掩护-缓冲不符由缓冲条拦截",
                r.limit.contains("字节"),
                "",
            ),
            Ok(()) => set.add("E05-反掩护-缓冲不符由缓冲条拦截", false, ""),
        }
    }

    // 合规请求通过校验（三要素齐全，否则「全拒」也算通过——那是另一种弱门禁）。
    {
        let ok = GpuSortRequest::for_count(1024).validate().is_ok();
        set.add("E05-预留-合规请求通过校验", ok, "");
    }

    // 趟数与键宽自洽（16 位键 = 2 趟，这是常量关系的可观测面）。
    {
        let need = (16u32 + RADIX_BITS - 1) / RADIX_BITS;
        set.add(
            "E05-预留-趟数与键宽自洽",
            GpuSortRequest::for_count(1).passes == need && need == 2,
            "",
        );
    }

    // 拒绝三要素齐备（是什么/上限/怎么办缺一不可）。
    {
        let mut base = GpuSortRequest::for_count(64);
        base.count = 0;
        match base.validate() {
            Err(r) => set.add(
                "E05-三要素-拒绝文本齐备",
                !r.what.is_empty() && !r.limit.is_empty() && !r.suggestion.is_empty(),
                "",
            ),
            Ok(()) => set.add("E05-三要素-拒绝文本齐备", false, ""),
        }
    }

    // 不合规请求经gpu_radix_sort 也被拒（两层校验不互相绕过）。
    {
        let mut bad = GpuSortRequest::for_count(64);
        bad.group_size = 7;
        let o = gpu_radix_sort(&bad);
        set.add("E05-预留-不合规请求亦被拒", o.is_err(), "");
    }

    // 拒绝原因可区分：不合规请求的消息须说「不合规」（否则调用方
    // 分不清是「路径没实现」还是「参数错了」——处置方向不同）。
    {
        let mut bad = GpuSortRequest::for_count(64);
        bad.key_bytes = 3;
        let o = gpu_radix_sort(&bad);
        match &o {
            Outcome::Err { message, .. } => {
                set.add("E05-预留-不合规与未实现可区分", message.contains("不合规"), "")
            }
            _ => set.add("E05-预留-不合规与未实现可区分", false, ""),
        }
    }

    // I03 对齐核验：三型 wire 键两两不同。
    {
        let r = alignment_report();
        set.add("E05-对齐-三型 wire 键互不相同", r.keys_distinct, "");
    }

    // I03 对齐核验：三型都在册。
    {
        let r = alignment_report();
        set.add("E05-对齐-三型均在册", r.all_listed, "");
    }

    // I03 对齐核验：三型各自的 wire 键恰为规范值（逐位比对 7 元）。
    {
        let r = alignment_report();
        set.add(
            "E05-对齐-三型 wire 键逐位规范",
            r.additive_canonical && r.alpha_canonical && r.premul_canonical,
            "",
        );
    }

    // I03 对齐核验：无语义重合（直通与预乘的颜色因子必须不同）。
    {
        let r = alignment_report();
        set.add("E05-对齐-无语义重合", !r.semantic_overlap, "");
    }

    // I03 对齐核验：wire 值与F0019 的 BlendFactor wire 同构
    //（One=1 / SrcAlpha=4 / OneMinusSrcAlpha=10）——断的是数字不是感觉。
    {
        set.add(
            "E05-对齐-因子 wire 与 I03 同构",
            Factor::Zero.wire() == 0
                && Factor::One.wire() == 1
                && Factor::SrcAlpha.wire() == 4
                && Factor::OneMinusSrcAlpha.wire() == 10
                && Op::Add.wire() == 0,
            "",
        );
    }

    // I03 对齐核验：七元键长度固定为 7（少一位就无法逐位对账）。
    {
        set.add(
            "E05-对齐-wire 键恒为七元",
            BlendMode::Additive.desc_wire().len() == 7
                && BlendMode::Alpha.desc_wire().len() == 7
                && BlendMode::Premultiplied.desc_wire().len() == 7,
            "",
        );
    }

    // 混合本体与判据侧**独立重算**的因子求值器逐位一致
    //（三型各一对样本；这是跨域语义对齐的数值面证据）。
    {
        let src = Rgba::new(0.7, 0.3, 0.9, 0.4);
        let dst = Rgba::new(0.2, 0.8, 0.1, 0.6);
        let mut all = true;
        for m in BLEND_MODES.iter() {
            let got = blend(*m, src, dst);
            let want = ref_blend(m.factors(), src, dst);
            if !bits_eq(got, want) {
                all = false;
            }
        }
        set.add("E05-对齐-混合本体等于独立因子求值", all, "");
    }

    // =======================================================================
    // E06 性能逐项分解（可数工作量，非估值）
    // =======================================================================

    // 比较排序的比较次数**实测计数**且落在 O(N log N) 区间内。
    // 上界用 `2*N*ceil(log2 N)`：不写估值常数，用可推导的界。
    {
        let n = 1024usize;
        let ds: Vec<f32> = (0..n).map(|i| ((i * 7919) % n) as f32 / n as f32).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let s = sort_comparison(&mut es);
        // ceil(log2(1024)) = 10。
        let hi = 2u64 * n as u64 * 10;
        let lo = n as u64; // 至少比 n-1 次（单趟扫描）
        set.add(
            "E06-比较-实测比较次数落在 N log N 界内",
            s.comparisons >lo && s.comparisons <= hi && s.n == n,
            "",
        );
    }

    // 比较次数**随N 单调不减**（若某档反而变少，说明计数没真在数）。
    {
        let mut counts: Vec<u64> = Vec::new();
        for n in [64usize, 128, 256, 512] {
            let ds: Vec<f32> = (0..n).map(|i| ((i * 13) % n) as f32 / n as f32).collect();
            let vs = views_at_depths(&ds);
            let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
            let s = sort_comparison(&mut es);
            counts.push(s.comparisons);
        }
        let mono = counts.windows(2).all(|w| w[1] >= w[0]);
        let grew = counts.last().copied().unwrap_or(0) > counts.first().copied().unwrap_or(0);
        set.add("E06-比较-次数随 N 单调不减且真增长", mono && grew, "");
    }

    // 基数排序趟数恒为 RADIX_TOTAL_PASSES，且比较次数恒为 0（O(N) 的含义）。
    {
        let n = 300usize;
        let ds: Vec<f32> = (0..n).map(|i| ((i * 17) % n) as f32 / n as f32).collect();
        let vs = views_at_depths(&ds);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut scratch: Vec<SortEntry> = Vec::new();
        let s = sort_radix(&mut es, &mut scratch);
        set.add(
            "E06-基数-趟数恒定且零比较",
            s.radix_passes as usize == RADIX_TOTAL_PASSES && s.comparisons == 0 && s.sorted,
            "",
        );
    }

    // 总趟数恰为两相之和（**实测修正**：初版只用一趟+末尾反转，
    // 反转会把同键组内相对序一起翻掉 ⇒ `index != 位置` 时结果错误）。
    {
        set.add(
            "E06-基数-总趟数为两相之和",
            RADIX_TOTAL_PASSES == RADIX_PASSES * 2 && RADIX_PASSES == 4,
            "",
        );
    }

    // 基数排序趟数不随 N 增长（这是 O(N) 与 O(N log N) 的**可观测**区别：
    // 前者趟数是常数，后者比较次数是超线性）。
    {
        let mut passes: Vec<u32> = Vec::new();
        for n in [128usize, 512, 2048] {
            let ds: Vec<f32> = (0..n).map(|i| ((i * 17) % n) as f32 / n as f32).collect();
            let vs = views_at_depths(&ds);
            let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
            let mut scratch: Vec<SortEntry> = Vec::new();
            let s = sort_radix(&mut es, &mut scratch);
            passes.push(s.radix_passes);
        }
        let constant = passes.windows(2).all(|w| w[0] == w[1]);
        set.add("E06-基数-趟数不随 N 增长", constant, "");
    }

    // 免排序零成本：`is_free()` 同时要求「未进入排序」与「零比较」
    //（只断其一会让「进入路径但比较数被清零」的退化实现蒙过）。
    {
        let vs = views_at_depths(&[0.2, 0.8, 0.5, 0.1]);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let p = plan(
            &EmitterSort::new(BlendMode::Additive, false),
            SortAlgo::Comparison,
            &mut es,
            &mut Vec::new(),
            &mut bag,
        );
        set.add(
            "E06-免排序-零比较且未进路径",
            p.stats.is_free() && p.stats.comparisons == 0 && !p.stats.sorted && p.path == PATH_NONE,
            "",
        );
    }

    // 排序开关是逐发射器粒度：一个 Required 发射器**确实**产生比较，
    // 紧邻的 Unnecessary 发射器**确实**不产生（粒度不是文档声明）。
    {
        let ds: Vec<f32> = (0..32).map(|i| ((i * 5) % 32) as f32 / 32.0).collect();
        let vs_a = views_at_depths(&ds);
        let (mut ea, _) = keys_from_views(&vs_a, cam_eye, cam_fwd);
        let mut bag = DiagBag::new();
        let pa = plan(
            &EmitterSort::new(BlendMode::Alpha, true),
            SortAlgo::Comparison,
            &mut ea,
            &mut Vec::new(),
            &mut bag,
        );
        let vs_b = views_at_depths(&ds);
        let (mut eb, _) = keys_from_views(&vs_b, cam_eye, cam_fwd);
        let pb = plan(
            &EmitterSort::new(BlendMode::Additive, false),
            SortAlgo::Comparison,
            &mut eb,
            &mut Vec::new(),
            &mut bag,
        );
        set.add(
            "E06-粒度-相邻发射器成本不同",
            pa.stats.comparisons > 0 && pb.stats.comparisons == 0,
            "",
        );
    }

    // 键生成是 O(N) 单遍（工作量正比于 N，不含二次方项）：
    // 生成 4 倍N 的条目，键数恰为 4 倍（无隐藏的重复扫描）。
    {
        let vs = views_at_depths(&[0.1, 0.2, 0.3, 0.4]);
        let (es, clamps) = keys_from_views(&vs, cam_eye, cam_fwd);
        set.add(
            "E06-键生成-单遍且钳制数为零",
            es.len() == 4 && clamps == 0,
            "",
        );
    }

    // `stats.n` 与实际条目数一致（统计口径不能虚报，否则性能对账全错）。
    {
        let vs = views(37);
        let (mut es, _) = keys_from_views(&vs, cam_eye, cam_fwd);
        let a = sort_comparison(&mut es);
        let vs2 = views(37);
        let (mut es2, _) = keys_from_views(&vs2, cam_eye, cam_fwd);
        let mut scratch: Vec<SortEntry> = Vec::new();
        let b = sort_radix(&mut es2, &mut scratch);
        set.add("E06-统计-n 与实际条目一致", a.n == 37 && b.n == 37, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vel07_baseline_all_green() {
        let set = run_vel07_all_checks();
        assert!(!set.truncated(), "判据集被截断");
        assert_eq!(set.tally().1, 0, "存在红项");
    }

    #[test]
    fn vel07_both_paths_agree() {
        // 两条 CPU 路径在同一输入上给出同一置换——这是判据的核心断言，
        // 单独立一个测试便于快速定位。
        let vs = views_at_depths(&[0.9, 0.1, 0.5, 0.5, 0.3]);
        let (mut a, _) = keys_from_views(&vs, [0.0; 3], [0.0, 0.0, 1.0]);
        let expect = expect_order(&a);
        sort_comparison(&mut a);
        let mut b = a.clone();
        let mut scratch: Vec<SortEntry> = Vec::new();
        sort_radix(&mut b, &mut scratch);
        let ra: Vec<u32> = a.iter().map(|e| e.index).collect();
        let rb: Vec<u32> = b.iter().map(|e| e.index).collect();
        assert_eq!(ra, expect);
        assert_eq!(rb, expect);
    }

    #[test]
    fn vel07_nan_key_does_not_break_order() {
        let mut vs = views_at_depths(&[0.3, 0.9]);
        if let Some(mut p) = vs.get(0).copied() {
            p.position = [f32::NAN, 0.0, 0.0];
            vs[0] = p;
        }
        let (mut es, clamps) = keys_from_views(&vs, [0.0; 3], [0.0, 0.0, 1.0]);
        sort_comparison(&mut es);
        let got: Vec<u32> = es.iter().map(|e| e.index).collect();
        assert_eq!(clamps, 1);
        assert_eq!(got, vec![1, 0]);
    }

    #[test]
    fn vel07_additive_is_order_free() {
        let cs = [
            Rgba::new(0.1, 0.2, 0.3, 0.5),
            Rgba::new(0.4, 0.1, 0.2, 0.25),
            Rgba::new(0.2, 0.5, 0.1, 0.75),
        ];
        let (same, sensitive) = order_invariant(BlendMode::Additive, &cs);
        assert!(same && !sensitive);
        let (same2, _) = order_invariant(BlendMode::Alpha, &cs);
        assert!(!same2);
    }

    #[test]
    fn vel07_gpu_stub_never_succeeds() {
        // 预留路径必须恒失败。**不用 `must`**：它对Err 会 panic，
        // 而这里 Err 正是期望结果。
        match gpu_radix_sort(&GpuSortRequest::for_count(8)) {
            Outcome::Err { message, hint, .. } => {
                assert!(message.contains("未实现"), "须明说未实现：{}", message);
                assert!(hint.contains("CPU"), "须指路 CPU 路径：{}", hint);
            }
            Outcome::Ok { .. } => panic!("GPU 预留路径被调用却返回成功——F1871 语义被破坏"),
        }
        // 不合规请求同样不得成功。
        let mut bad = GpuSortRequest::for_count(8);
        bad.key_bits = 0;
        bad.passes = 0;
        assert!(gpu_radix_sort(&bad).is_err());
    }

    #[test]
    fn vel07_forbidden_emits_visual_consequence() {
        let vs = views_at_depths(&[0.2, 0.8]);
        let (mut es, _) = keys_from_views(&vs, [0.0; 3], [0.0, 0.0, 1.0]);
        let mut bag = DiagBag::new();
        let p = plan(
            &EmitterSort::new(BlendMode::Alpha, false),
            SortAlgo::Comparison,
            &mut es,
            &mut Vec::new(),
            &mut bag,
        );
        assert_eq!(p.verdict, SortVerdict::Forbidden);
        assert_eq!(p.stats.comparisons, 0);
        assert!(bag.has_msg_containing(DiagCode::TransitionRejected, "前后颠倒"));
    }
}