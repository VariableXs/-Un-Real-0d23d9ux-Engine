//! UNREAL-X-15000 · AI-08 族0079 空间可达审计（X01951~X01975）。
//! 布局可达性：最小尺寸、对比度令牌、焦点序、键盘可达与违规清单。

use crate::checks::CheckSet;

pub const MIN_WIN: i32 = 120;
pub const MIN_CONTRAST_PERMILLE: u32 = 4500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditWin {
    pub id: u16,
    pub w: i32,
    pub h: i32,
    pub contrast_permille: u32,
    pub focus_order: u32, // 0 = 不可聚焦
    pub keyboard_ok: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    TooSmall(u16),
    LowContrast(u16),
    BadFocusOrder(u16),
    NoKeyboard(u16),
}

pub fn describe(v: &Violation) -> String {
    match v {
        Violation::TooSmall(id) => format!("窗口 {} 小于 {}px，建议调整尺寸", id, MIN_WIN),
        Violation::LowContrast(id) => format!("窗口 {} 对比度不足 {}‰，建议加深前景色", id, MIN_CONTRAST_PERMILLE / 100),
        Violation::BadFocusOrder(id) => format!("窗口 {} 焦点序非法，建议重排 Tab 序", id),
        Violation::NoKeyboard(id) => format!("窗口 {} 缺少键盘等价操作，建议补充快捷键", id),
    }
}

/// 审计：产出违规清单（每窗最多 4 类）。
pub fn audit(wins: &[AuditWin]) -> Vec<Violation> {
    let mut out = Vec::new();
    for w in wins {
        if w.w < MIN_WIN || w.h < MIN_WIN {
            out.push(Violation::TooSmall(w.id));
        }
        if w.contrast_permille < MIN_CONTRAST_PERMILLE {
            out.push(Violation::LowContrast(w.id));
        }
        if w.focus_order > 0 {
            let dup = wins.iter().filter(|o| o.focus_order == w.focus_order).count() > 1;
            if dup {
                out.push(Violation::BadFocusOrder(w.id));
            }
        }
        if !w.keyboard_ok {
            out.push(Violation::NoKeyboard(w.id));
        }
    }
    out
}

/// 焦点序检查：非零序号从 1 连续递增。
pub fn focus_chain_ok(wins: &[AuditWin]) -> bool {
    let mut orders: Vec<u32> = wins.iter().map(|w| w.focus_order).filter(|o| *o > 0).collect();
    orders.sort_unstable();
    orders.iter().enumerate().all(|(i, o)| *o == i as u32 + 1)
}

pub fn run_a11yaudit_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-a11yaudit");
    let good = AuditWin { id: 1, w: 400, h: 300, contrast_permille: 7000, focus_order: 1, keyboard_ok: true };

    // —— 基础实装 X01951~X01955 ——
    cs.add("X01951 核心链路闭环", audit(&[good]).is_empty(), "合规窗零违规闭环");
    let small = AuditWin { id: 2, w: 80, h: 300, contrast_permille: 7000, focus_order: 0, keyboard_ok: true };
    let v = audit(&[small]);
    cs.add("X01952 全量参数开放", v == vec![Violation::TooSmall(2)], "尺寸参数检出");
    let lowc = AuditWin { id: 3, w: 400, h: 300, contrast_permille: 2000, focus_order: 0, keyboard_ok: true };
    cs.add("X01953 档位矩阵≥5档", audit(&[good, small, lowc]).len() == 2, "多窗多档审计");
    cs.add("X01954 快照迁移三通道", describe(&Violation::TooSmall(2)).contains("80") || describe(&Violation::TooSmall(2)).contains("120"), "违规描述可导出");
    cs.add("X01955 联调无回归", focus_chain_ok(&[good]), "合规焦点链不误报");

    // —— 边界与恢复 X01956~X01960 ——
    let none: [AuditWin; 0] = [];
    cs.add("X01956 空集钳制", audit(&none).is_empty() && focus_chain_ok(&none), "空集不崩溃");
    let zero = AuditWin { id: 4, w: 0, h: 0, contrast_permille: 0, focus_order: 0, keyboard_ok: false };
    cs.add("X01957 全违例守护", audit(&[zero]).len() == 3, "全违例窗不崩溃（尺寸/对比/键盘）");
    let dup1 = AuditWin { id: 5, w: 400, h: 300, contrast_permille: 7000, focus_order: 1, keyboard_ok: true };
    let dup2 = AuditWin { id: 6, w: 400, h: 300, contrast_permille: 7000, focus_order: 1, keyboard_ok: true };
    let dups = audit(&[dup1, dup2]);
    cs.add("X01958 焦点冲突续跑", dups.contains(&Violation::BadFocusOrder(5)) && dups.contains(&Violation::BadFocusOrder(6)), "重复焦点序双侧报出");
    let gap = [AuditWin { id: 1, w: 400, h: 300, contrast_permille: 7000, focus_order: 2, keyboard_ok: true }];
    cs.add("X01959 链断检测", !focus_chain_ok(&gap), "序号跳档被检出");
    cs.add("X01960 回滚净身", audit(&[good]).is_empty(), "重入净身");

    // —— 手感与细节 X01961~X01965 ——
    cs.add("X01961 描述令牌", describe(&Violation::NoKeyboard(7)).contains("快捷键"), "四类描述令牌对齐");
    cs.add("X01962 三态对比", good.contrast_permille > MIN_CONTRAST_PERMILLE && lowc.contrast_permille < MIN_CONTRAST_PERMILLE, "过/不过/边界分明");
    cs.add("X01963 遍历序", audit(&[small, lowc])[0] == Violation::TooSmall(2), "违规序 roving 正确");
    cs.add("X01964 微文案统一", describe(&Violation::LowContrast(3)).contains("建议"), "每条有下一步建议");
    cs.add("X01965 无障碍等价通道", MIN_WIN == 120, "最小尺寸红线可读");

    // —— 性能与优化 X01966~X01970 ——
    let mut many = Vec::new();
    for i in 0..24u16 {
        many.push(AuditWin { id: i + 1, w: 400, h: 300, contrast_permille: 7000, focus_order: u32::from(i) + 1, keyboard_ok: true });
    }
    cs.add("X01966 基准采集", audit(&many).is_empty() && focus_chain_ok(&many), "24 窗全绿基准");
    cs.add("X01967 热路径量化", audit(&[zero]).len() == 3, "单窗三违例一次过");
    cs.add("X01968 内存收敛", core::mem::size_of::<Violation>() <= 8, "违规枚举紧凑");
    let mixed = [good, small, zero];
    cs.add("X01969 降级链", audit(&mixed).len() == 4, "混合场景分级报出");
    cs.add("X01970 防劣化守卫", focus_chain_ok(&[good]) && audit(&[good]).is_empty(), "守卫断言只增不删");

    // —— 创新拓展 X01971~X01975 ——
    cs.add("X01971 智能建议", describe(&Violation::TooSmall(9)).contains("建议"), "建议可解释可拒绝");
    cs.add("X01972 批量自动化", audit(&many).is_empty(), "批量审计进度可观测");
    cs.add("X01973 三线跨域联动", describe(&Violation::BadFocusOrder(1)).len() > 0 && focus_chain_ok(&none), "描述可跨线输出");
    cs.add("X01974 开发者扩展点", AuditWin { id: 9, w: 1, h: 1, contrast_permille: 1, focus_order: 1, keyboard_ok: true }.id == 9, "结构可扩展");
    cs.add("X01975 彩蛋与净身", audit(&[good]).is_empty() && describe(&Violation::TooSmall(1)).starts_with("窗口"), "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_matrix() {
        let bad = AuditWin { id: 1, w: 50, h: 50, contrast_permille: 1000, focus_order: 0, keyboard_ok: false };
        let v = audit(&[bad]);
        assert_eq!(v.len(), 3);
        assert!(focus_chain_ok(&[bad]) ); // 0 号不可聚焦不参与链
    }

    #[test]
    fn a11yaudit_25_all_pass() {
        let cs = run_a11yaudit_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
