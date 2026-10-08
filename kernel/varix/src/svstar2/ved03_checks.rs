//! VE-F0603 · 域自检（判据逐条对应，见 `ved03_alpha.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 分组语义（子树独立组合成再整体乘 alpha；嵌套 = 相乘）→ `D03-分组-*`
//! - 隐式隔离（alpha < 1 即隐式隔离——F0605 条件清单成员）→ `D03-隔离-*`
//! - 顺序契约（组内混合→组 alpha→外部混合；F0625 预乘纪律）→ `D03-顺序-*`
//! - 边界行为（0 跳绘制保布局命中豁免；1 快速路径）→ `D03-边界-*`
//! - 值域钳制（0-1 外拒绝；NaN/Inf 拒绝）→ `D03-值域-*`
//! - 淡入淡出（F0608 联动；打断→当前值为起点续走）→ `D03-渐变-*`
//! - 分组深度爆炸→隔离组复用合并 → `D03-合并-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::ved03_alpha::*;
use crate::checks::CheckSet;

/// 近似相等。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

/// VE-F0603 域自检。
pub fn run_ved03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved03");

    // ---- 值域纪律（错误路径第一道）----

    // 判据：值域钳制（0-1 外拒绝，不静默钳制）+ 非有限拒绝。
    {
        let neg = Opacity::new(-0.1);
        let big = Opacity::new(1.5);
        let nan = Opacity::new(f32::NAN);
        let inf = Opacity::new(f32::INFINITY);
        let ok = matches!(neg, Err(AlphaError::OutOfRange(_)))
            && matches!(big, Err(AlphaError::OutOfRange(_)))
            && matches!(nan, Err(AlphaError::NonFinite(_)))
            && matches!(inf, Err(AlphaError::NonFinite(_)))
            && neg.unwrap_err().code() == "E_OUT_OF_RANGE"
            && nan.unwrap_err().next_hint().contains("NaN");
        set.add("D03-值域-越界与非有限拒绝", ok, "");
    }

    // ---- 边界行为 ----

    // 判据：alpha=0 跳过绘制但保留布局与命中豁免；alpha=1 快速路径；
    // 中间值分组合成（O(1) 判定）。
    {
        let zero = Opacity::new(0.0).expect("合法");
        let one = Opacity::new(1.0).expect("合法");
        let half = Opacity::new(0.5).expect("合法");
        set.add(
            "D03-边界-三态路径与命中豁免",
            zero.path() == CompositingPath::SkipDraw
                && one.path() == CompositingPath::OpaqueFastPath
                && half.path() == CompositingPath::GroupComposite
                && !zero.hit_allowed()   // 命中豁免：全透明不拦截点击
                && one.hit_allowed()
                && half.hit_allowed()
                && !one.requires_isolation(), // 1 无需隔离（快速路径跳过分组合成）
            "",
        );
    }

    // ---- 隐式隔离 ----

    // 判据：不透明度 < 1 即隐式隔离（F0605 条件清单成员，文档挂点在册）。
    {
        let half = Opacity::new(0.5).expect("合法");
        let one = Opacity::new(1.0).expect("合法");
        let doc_ok = IMPLICIT_ISOLATION_DOC.contains("不透明度 < 1") && IMPLICIT_ISOLATION_DOC.contains("F0605");
        set.add(
            "D03-隔离-条件判定与挂点在册",
            half.requires_isolation() && !one.requires_isolation() && doc_ok,
            "",
        );
    }

    // ---- 分组语义 ----

    // 判据：嵌套组有效 alpha 相乘（整组再乘在嵌套下的唯一自洽解）。
    {
        let outer = Opacity::new(0.8).expect("合法");
        let inner = Opacity::new(0.5).expect("合法");
        let eff = inner.nested(outer);
        set.add("D03-分组-嵌套相乘", close(eff.value(), 0.4), "");
    }

    // 判据：顺序契约在册（S1-S3 三段顺序 + S4 顺序不可换的归因）。
    {
        let d = ORDER_CONTRACT_DOC;
        set.add(
            "D03-顺序-契约在册",
            d.contains("S1") && d.contains("S2") && d.contains("S3")
                && d.contains("F0625") && d.contains("顺序不可换"),
            "",
        );
    }

    // 判据：组 alpha 的预乘应用（rgb 与 a 等比缩放——F0625 纪律的 S2 实现）。
    {
        let c = PremulColor { rgb: [0.8, 0.4, 0.2], a: 0.5 };
        let g = Opacity::new(0.5).expect("合法");
        let out = apply_group_alpha(&c, g).expect("合法系数");
        set.add(
            "D03-顺序-预乘等比缩放",
            close(out.rgb[0], 0.4) && close(out.rgb[1], 0.2) && close(out.rgb[2], 0.1)
                && close(out.a, 0.25),
            "",
        );
    }

    // ---- 隔离组复用合并 ----

    // 判据：相邻纯 alpha 组合并（深度爆炸处置：alpha 相乘，语义等价）。
    {
        let a = Opacity::new(0.5).expect("合法");
        let b = Opacity::new(0.8).expect("合法");
        let merged = a.merge(b).expect("乘积在值域内");
        // 等价性：合并后与嵌套相乘一致。
        set.add("D03-合并-alpha相乘等价", close(merged.value(), 0.4), "");
    }

    // ---- 淡入淡出 ----

    // 判据：渐变步进（每 tick 步进 step，到达后 Reached——F0608 联动形态）。
    {
        let mut f = Fade::new(
            Opacity::new(0.0).expect("合法"),
            Opacity::new(0.4).expect("合法"),
            0.1,
        )
        .expect("合法");
        let mut values = [0.0f32; 5];
        for i in 0..5 {
            values[i] = match f.tick() {
                FadeStep::Stepping(o) | FadeStep::Reached(o) => o.value(),
            };
        }
        // 0.1→0.2→0.3→0.4(Reached)→0.4（到达后稳定）。
        set.add(
            "D03-渐变-线性步进到点",
            close(values[0], 0.1) && close(values[1], 0.2) && close(values[2], 0.3)
                && close(values[3], 0.4) && close(values[4], 0.4)
                && matches!(f.tick(), FadeStep::Reached(_)),
            "",
        );
    }

    // 判据：动画打断→当前值为起点续走（retarget 不跳变不重放）。
    {
        let mut f = Fade::new(
            Opacity::new(0.0).expect("合法"),
            Opacity::new(1.0).expect("合法"),
            0.1,
        )
        .expect("合法");
        // 走三步到 0.3，打断换目标 0.5：续走应从 0.3 → 0.4 → 0.5。
        for _ in 0..3 {
            f.tick();
        }
        f.retarget(Opacity::new(0.5).expect("合法"));
        f.tick();
        let cur = f.current.value();
        let _ = f.tick();
        let done = matches!(f.tick(), FadeStep::Reached(_)) && close(f.current.value(), 0.5);
        set.add(
            "D03-渐变-打断以当前值续走",
            close(cur, 0.4) && done && !f.animating(),
            "",
        );
    }

    // 判据：零步进/负步进/非有限步进拒绝（零步进渐变是死循环）。
    {
        let bad = Fade::new(Opacity::OPAQUE, Opacity::TRANSPARENT, 0.0);
        let neg = Fade::new(Opacity::OPAQUE, Opacity::TRANSPARENT, -0.1);
        let nan = Fade::new(Opacity::OPAQUE, Opacity::TRANSPARENT, f32::NAN);
        set.add(
            "D03-渐变-非法步进拒绝",
            matches!(bad, Err(AlphaError::NonFinite(_)))
                && matches!(neg, Err(AlphaError::NonFinite(_)))
                && matches!(nan, Err(AlphaError::NonFinite(_))),
            "",
        );
    }

    // ---- 动画标记查询挂点（无障碍预留）----

    {
        let m = AnimationMarker { animating: true, flicker_class: true };
        let s = m.screen_text();
        set.add(
            "D03-读屏-动画标记可播",
            s.contains("闪烁类") && s.contains("减弱动效"),
            "",
        );
    }

    // ---- 读屏 ----

    {
        let s = screen_text(Opacity::new(0.25).expect("合法"));
        set.add("D03-读屏-数值可播", s.contains("0.250"), "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ved03_all_judgements_green() {
        let set = run_ved03_checks();
        let (passed, failed) = set.tally();
        let (reds, n) = set.red_items();
        let names: Vec<&'static str> = (0..n)
            .filter_map(|i| reds[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect();
        assert!(
            set.all_passed(),
            "VE-F0603 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            names
        );
    }

    #[test]
    fn ved03_merge_product_in_range() {
        // 合并的乘积恒在 [0,1]——乘法封闭性（两合法值之积不会越界）。
        for i in 0..=10u32 {
            let a = Opacity::new(i as f32 / 10.0).expect("合法");
            for j in 0..=10u32 {
                let b = Opacity::new(j as f32 / 10.0).expect("合法");
                assert!(a.merge(b).is_ok(), "乘积 {}×{} 越界", i, j);
            }
        }
    }
}
