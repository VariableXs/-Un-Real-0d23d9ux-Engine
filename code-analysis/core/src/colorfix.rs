//! AI-07 · 颜色标记修复（#481~#490）。
//!
//! 修复前：红色同时表示 bug / 异常 / 性能差 / 经常执行 / 红石连线，用户无法区分。
//! 修复原则：**一色一义** —— 每种颜色只表示一种含义，动态效果与形状辅助区分。
//! 零 AI：静态色表 + 确定性距离计算。

/// 语义状态（一色一义的主键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sem {
    /// 普通 bug：红 #FF3B30 脉冲 2s。
    Bug,
    /// 严重 bug：深红 #8B0000 剧烈抖动 + 烟雾。
    SevereBug,
    /// 异常传播：品红 #FF00FF 波纹扩散。
    Exception,
    /// 性能慢：橙 #FF9500 热力渐变呼吸。
    SlowPerf,
    /// 高频执行：暖黄 #FFD700 微弱发光（热 ≠ 坏）。
    Hot,
    /// 警告：黄 #FFD60A 三角角标。
    Warn,
    /// 正常/通过：绿 #34C759 勾号角标。
    Ok,
    /// 控制流：绿 #34C759 光点沿线移动。
    ControlFlow,
    /// 数据流：蓝 #007AFF 粒子流动。
    DataFlow,
    /// 循环：紫 #AF52DE 旋转光环。
    Loop,
    /// 输入输出：青 #5AC8FA 平行四边形。
    Io,
    /// 判断：琥珀 #FFAB00 菱形。
    Branch,
    /// 选中：白 #FFFFFF 发光呼吸。
    Selected,
    /// 死代码：灰 #8E8E93 半透明。
    DeadCode,
    /// 操作完成：绿 #34C759 勾号淡出 / 经验球。
    Done,
    /// 框选：蓝 #007AFF 半透明虚线矩形。
    Marquee,
    /// 红石连线：暗红 #AA0000 稳定流动（不闪），像素风专属。
    Redstone,
}

/// 语义性质（用于冲突判定：故障类颜色不得与其它性质复用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nature {
    /// 故障（bug/异常）。
    Fault,
    /// 性能（慢/热）。
    Perf,
    /// 结构（控制流/数据流/循环/IO/判断）。
    Structure,
    /// 状态（选中/死代码）。
    State,
    /// 正向（通过/完成/警告）。
    Positive,
}

/// 一条颜色规范。
#[derive(Debug, Clone, Copy)]
pub struct SemStyle {
    pub sem: Sem,
    pub name: &'static str,
    pub hex: &'static str,
    pub effect: &'static str,
    pub shape: &'static str,
    pub scene: &'static str,
    /// 是否有闪烁/抖动等动态强调。
    pub animated: bool,
}

/// 修复后完整颜色规范（主规格「二十九、颜色标记修复」17 行）。
pub const TABLE: &[SemStyle] = &[
    SemStyle { sem: Sem::Bug, name: "普通bug", hex: "#FF3B30", effect: "脉冲闪烁2s", shape: "圆形光晕", scene: "静态分析发现的一般问题", animated: true },
    SemStyle { sem: Sem::SevereBug, name: "严重bug", hex: "#8B0000", effect: "剧烈抖动+烟雾", shape: "方块", scene: "崩溃级问题", animated: true },
    SemStyle { sem: Sem::Exception, name: "异常传播", hex: "#FF00FF", effect: "波纹向外扩散", shape: "环形", scene: "throw→catch 路径(F7)", animated: true },
    SemStyle { sem: Sem::SlowPerf, name: "性能慢", hex: "#FF9500", effect: "热力渐变呼吸", shape: "背景填充", scene: "热点(F4)", animated: true },
    SemStyle { sem: Sem::Hot, name: "高频执行", hex: "#FFD700", effect: "微弱发光", shape: "圆形光点", scene: "经常执行（非负面）", animated: false },
    SemStyle { sem: Sem::Warn, name: "警告", hex: "#FFD60A", effect: "无", shape: "三角角标", scene: "需要注意", animated: false },
    SemStyle { sem: Sem::Ok, name: "正常/通过", hex: "#34C759", effect: "无", shape: "勾号角标", scene: "检测通过", animated: false },
    SemStyle { sem: Sem::ControlFlow, name: "控制流", hex: "#34C759", effect: "光点沿线移动", shape: "小圆点", scene: "执行顺序(F2)", animated: true },
    SemStyle { sem: Sem::DataFlow, name: "数据流", hex: "#007AFF", effect: "粒子流动", shape: "粒子串", scene: "变量传递(F3)", animated: true },
    SemStyle { sem: Sem::Loop, name: "循环", hex: "#AF52DE", effect: "旋转光环", shape: "环形", scene: "for/while", animated: true },
    SemStyle { sem: Sem::Io, name: "输入输出", hex: "#5AC8FA", effect: "无", shape: "平行四边形", scene: "IO 节点", animated: false },
    SemStyle { sem: Sem::Branch, name: "判断", hex: "#FFAB00", effect: "无", shape: "菱形", scene: "if/switch", animated: false },
    SemStyle { sem: Sem::Selected, name: "选中", hex: "#FFFFFF", effect: "发光呼吸", shape: "光圈", scene: "当前选中", animated: true },
    SemStyle { sem: Sem::DeadCode, name: "死代码", hex: "#8E8E93", effect: "无", shape: "半透明", scene: "不可达", animated: false },
    SemStyle { sem: Sem::Done, name: "操作完成", hex: "#34C759", effect: "勾号淡出/经验球", shape: "角标", scene: "一键改进完成", animated: true },
    SemStyle { sem: Sem::Marquee, name: "框选", hex: "#007AFF", effect: "半透明虚线", shape: "矩形", scene: "Shift+拖拽", animated: false },
    SemStyle { sem: Sem::Redstone, name: "红石连线", hex: "#AA0000", effect: "稳定流动(不闪)", shape: "线段", scene: "像素风专属", animated: false },
];

/// 取样式。
pub fn style(sem: Sem) -> &'static SemStyle {
    TABLE
        .iter()
        .find(|s| s.sem == sem)
        .expect("颜色规范表覆盖全部语义状态")
}

pub fn hex_of(sem: Sem) -> &'static str {
    style(sem).hex
}

/// 语义性质。
pub fn nature(sem: Sem) -> Nature {
    match sem {
        Sem::Bug | Sem::SevereBug | Sem::Exception => Nature::Fault,
        Sem::SlowPerf | Sem::Hot => Nature::Perf,
        Sem::ControlFlow | Sem::DataFlow | Sem::Loop | Sem::Io | Sem::Branch => Nature::Structure,
        Sem::Selected | Sem::DeadCode => Nature::State,
        Sem::Warn | Sem::Ok | Sem::Done | Sem::Marquee | Sem::Redstone => Nature::Positive,
    }
}

/// 十六进制 → RGB。
pub fn rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let p = |a: usize, b: usize| u8::from_str_radix(&h[a..b], 16).unwrap_or(0);
    if h.len() >= 6 {
        (p(0, 2), p(2, 4), p(4, 6))
    } else {
        (0, 0, 0)
    }
}

/// 色彩距离（欧氏，用于「琥珀 vs 警告黄」可分辨性判定 #488）。
pub fn color_distance(a: &str, b: &str) -> f64 {
    let (r1, g1, b1) = rgb(a);
    let (r2, g2, b2) = rgb(b);
    let dr = r1 as f64 - r2 as f64;
    let dg = g1 as f64 - g2 as f64;
    let db = b1 as f64 - b2 as f64;
    (dr * dr + dg * dg + db * db).sqrt()
}

/// 使用某颜色的全部语义（用于一色一义冲突审计）。
pub fn used_by(hex: &str) -> Vec<Sem> {
    TABLE.iter().filter(|s| s.hex == hex).map(|s| s.sem).collect()
}

/// Nature::Fault 颜色是否被其它性质复用（修复前红色被复用 → 返回 true 即不通过）。
pub fn fault_color_reused() -> bool {
    let faults: Vec<Sem> = TABLE.iter().filter(|s| nature(s.sem) == Nature::Fault).map(|s| s.sem).collect();
    faults.iter().any(|f| {
        let h = hex_of(*f);
        used_by(h).iter().any(|s| nature(*s) != Nature::Fault)
    })
}

/// 性能色是否与故障色混用（#484：橙色不再用红色）。
pub fn perf_fault_overlap() -> bool {
    TABLE
        .iter()
        .filter(|s| nature(s.sem) == Nature::Perf)
        .any(|s| used_by(s.hex).iter().any(|x| nature(*x) == Nature::Fault))
}

/// #490 风格互斥规则：像素风用火焰粒子时禁用热力渐变，避免同义叠加。
pub fn style_exclusive(style: &str, effect: &str) -> bool {
    match style {
        "pixel" => effect != "热力渐变呼吸",
        _ => true,
    }
}

/// 解析叠加效果：同一节点上两个同义效果取优先级高者（序号小者）。
pub fn resolve_overlap(a: &str, b: &str) -> &'static str {
    const ORDER: &[&str] = &["火焰粒子", "热力渐变呼吸", "脉冲闪烁2s", "微弱发光", "无"];
    let ra = ORDER.iter().position(|x| *x == a).unwrap_or(ORDER.len() - 1);
    let rb = ORDER.iter().position(|x| *x == b).unwrap_or(ORDER.len() - 1);
    ORDER[ra.min(rb)]
}

/// #481~#490 域自检。
pub fn run_colorfix_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("colorfix");

    // #481 红色=bug 专用
    let bug = style(Sem::Bug);
    let red_reuse = used_by("#FF3B30");
    s.add(
        "#481 红色=bug专用",
        bug.hex == "#FF3B30"
            && bug.effect.contains("脉冲")
            && bug.effect.contains("2s")
            && red_reuse == vec![Sem::Bug]
            && !TABLE.iter().any(|x| x.hex == "#FF0000"),
        "红色只表示普通bug，不再用于性能/连线/异常",
    );

    // #482 深红=严重 bug
    let sev = style(Sem::SevereBug);
    s.add(
        "#482 深红=严重bug",
        sev.hex == "#8B0000"
            && sev.effect.contains("抖动")
            && sev.effect.contains("烟雾")
            && sev.shape == "方块"
            && color_distance(sev.hex, bug.hex) > 60.0,
        "深红+剧烈抖动+烟雾，与普通bug区分",
    );

    // #483 品红=异常路径
    let exc = style(Sem::Exception);
    s.add(
        "#483 品红=异常路径",
        exc.hex == "#FF00FF" && exc.effect.contains("波纹") && exc.hex != bug.hex,
        "品红波纹扩散，替代原来的红色异常",
    );

    // #484 橙色=性能慢
    let perf = style(Sem::SlowPerf);
    let perf_reused_by_fault = perf_fault_overlap();
    s.add(
        "#484 橙色=性能慢",
        perf.hex == "#FF9500" && perf.effect.contains("热力渐变") && !perf_reused_by_fault,
        "橙色热力渐变，不再用红色",
    );

    // #485 暖黄=高频执行（热 ≠ 坏）
    let hot = style(Sem::Hot);
    s.add(
        "#485 暖黄=高频执行",
        hot.hex == "#FFD700" && hot.effect.contains("发光") && nature(Sem::Hot) != Nature::Fault,
        "暖黄微光，热≠坏",
    );

    // #486 青色=输入输出
    let io = style(Sem::Io);
    s.add(
        "#486 青色=输入输出",
        io.hex == "#5AC8FA" && io.hex != perf.hex && color_distance(io.hex, perf.hex) > 100.0,
        "青色替代原来与热点重叠的橙色",
    );

    // #487 白色=选中高亮
    let sel = style(Sem::Selected);
    s.add(
        "#487 白色=选中高亮",
        sel.hex == "#FFFFFF" && sel.effect.contains("发光") && sel.hex != style(Sem::Warn).hex,
        "白色发光，替代原来与警告重叠的黄色",
    );

    // #488 琥珀=判断节点
    let br = style(Sem::Branch);
    let warn = style(Sem::Warn);
    s.add(
        "#488 琥珀=判断节点",
        br.hex == "#FFAB00" && br.shape == "菱形" && br.hex != warn.hex && color_distance(br.hex, warn.hex) >= 30.0,
        "琥珀与警告黄可分辨",
    );

    // #489 暗红=像素风连线（稳定不闪）
    let rs = style(Sem::Redstone);
    s.add(
        "#489 暗红=像素风连线",
        rs.hex == "#AA0000"
            && rs.effect.contains("不闪")
            && !rs.animated
            && bug.animated
            && color_distance(rs.hex, bug.hex) > 40.0,
        "暗红稳定流动，与bug闪烁区分",
    );

    // #490 风格互斥规则
    let pixel_heat = style_exclusive("pixel", "热力渐变呼吸");
    let pixel_flame = style_exclusive("pixel", "火焰粒子");
    let star_heat = style_exclusive("star", "热力渐变呼吸");
    let pick = resolve_overlap("火焰粒子", "热力渐变呼吸");
    s.add(
        "#490 风格互斥规则",
        !pixel_heat && pixel_flame && star_heat && pick == "火焰粒子",
        "像素风火焰粒子时禁用热力渐变，避免同义叠加",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f481_no_fault_color_reuse() {
        assert!(!fault_color_reused());
        assert_eq!(used_by("#FF3B30"), vec![Sem::Bug]);
    }

    #[test]
    fn f484_perf_not_red() {
        assert_ne!(hex_of(Sem::SlowPerf), "#FF3B30");
        assert_ne!(hex_of(Sem::SlowPerf), "#8B0000");
    }

    #[test]
    fn f488_branch_vs_warn_resolvable() {
        assert!(color_distance("#FFAB00", "#FFD60A") >= 30.0);
    }

    #[test]
    fn f489_stable_vs_flicker() {
        assert!(!style(Sem::Redstone).animated);
        assert!(style(Sem::Bug).animated);
    }
}
