//! VE-F3444 · 字阶变体运行时 —— 三档字阶、行高派生、可读硬线、系统缩放优先
//!
//! ## 职责拆解
//! 字阶不是「一排字号」，而是三条互相咬合的约束：
//! 1. **阶梯**（`TypeScale::ladder`）：七级（caption…display），每档一张定值表。
//!    用定值表而不用浮点比例，是因为比例生成会出现 13.7px 这种不可排版的值，
//!    再四舍五入又破坏比例——不如设计侧直接把表定死，运行时只查表。
//! 2. **行高派生**（`line_height`）：行高由字号决定，不是独立令牌。
//!    三档比例按字号分桶（小字 1.5、中字 1.4、大字 1.25）：字越大，行距相对越紧，
//!    这是排版常识而不是审美偏好。
//! 3. **可读硬线**（`MIN_READABLE_PX = 12`）：任何档、任何缩放、任何步骤都不得
//!    低于 12px。12 是「中文 100% 缩放仍可辨认」的经验下限，低于它等于把内容藏起来。
//! 4. **系统缩放优先**（`resolve`）：用户把系统字体调到 150%，**优先于**字阶变体。
//!    优先级不能反：变体是审美选择，缩放是无障碍需求。
//! 5. **双密度指标**（`density_pair`）：阅读密度（行高/字号）与信息密度
//!    （单位面积的字符数）是两个反向指标。把两个数一起报，才不会被「字大了更清楚」
//!    这种单边论证带偏——字大了确实更清楚，但一屏能看的字也少了。
//!
//! ## 为什么这么拆
//! 字阶最容易出的错是「比例很漂亮但最小那级看不清」。所以本册把可读硬线放在
//! `resolve` 内部（而不是审计里）——**硬线必须是不可绕过的**，能被绕过的硬线
//! 迟早会被绕过。审计函数只负责证明这条硬线真的在生效。
//!
//! ## 复杂度
//! - 查表 O(1)；行高 O(1)；缩放 O(1)。
//! - 全档审计 O(档 × 步) = 21 项。
//!
//! ## 零 panic 面
//! - 查表用 `get()`，越界返回末级（而不是 0——0 会让下游除零）。
//! - 乘法走 u64；除法分母是字面常量。
//! - 无 `unwrap` / `expect`。

use alloc::format;
use alloc::string::ToString;
use alloc::string::String;
use alloc::vec::Vec;

/// 可读硬线（px）。任何路径都不得产出低于此值的字号。
pub const MIN_READABLE_PX: u32 = 12;

/// 阶梯级数（caption, body-s, body, body-l, title, headline, display）。
pub const STEPS: usize = 7;

/// 系统缩放的可接受区间（千分比）：100%~225%。上限取 225% 是因为
/// 再大的缩放应该走「整页放大」而不是「字号继续膨胀」。
pub const USER_SCALE_MIN: u32 = 1000;
pub const USER_SCALE_MAX: u32 = 2250;

/// 诊断码（E18 段 · 5617~5626）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TypeCode {
    /// 档位越界。
    LevelOutOfRange = 5617,
    /// 低于可读硬线。
    BelowReadable = 5618,
    /// 行高不大于字号。
    LineHeightTooSmall = 5619,
    /// 阶梯比例断裂（相邻级涨幅异常）。
    RatioBroken = 5620,
    /// 步骤越界。
    StepOutOfRange = 5621,
    /// 用户缩放被变体覆盖（优先级反了）。
    UserScaleOverridden = 5622,
    /// 字重越界。
    WeightOutOfRange = 5623,
    /// 计算溢出。
    Overflow = 5624,
    /// 阶梯为空。
    EmptyLadder = 5625,
    /// 阶梯非单调。
    NonMonotonic = 5626,
}

impl TypeCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            TypeCode::LevelOutOfRange => "档位越界",
            TypeCode::BelowReadable => "低于可读硬线",
            TypeCode::LineHeightTooSmall => "行高不大于字号",
            TypeCode::RatioBroken => "阶梯比例断裂",
            TypeCode::StepOutOfRange => "步骤越界",
            TypeCode::UserScaleOverridden => "用户缩放被变体覆盖",
            TypeCode::WeightOutOfRange => "字重越界",
            TypeCode::Overflow => "计算溢出",
            TypeCode::EmptyLadder => "阶梯为空",
            TypeCode::NonMonotonic => "阶梯非单调",
        }
    }
}

/// 三档字阶。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TypeScale {
    Tight,
    Standard,
    Loose,
}

impl TypeScale {
    pub const ALL: [TypeScale; 3] = [TypeScale::Tight, TypeScale::Standard, TypeScale::Loose];

    /// 阶梯表。索引即步骤（0=caption … 6=display）。
    const TIGHT: [u32; STEPS] = [12, 12, 14, 15, 17, 20, 24];
    const STANDARD: [u32; STEPS] = [12, 13, 14, 16, 20, 24, 32];
    const LOOSE: [u32; STEPS] = [13, 14, 16, 18, 22, 28, 36];

    pub fn of_level(level: u8) -> Result<Self, TypeCode> {
        match level {
            0 => Ok(TypeScale::Tight),
            1 => Ok(TypeScale::Standard),
            2 => Ok(TypeScale::Loose),
            _ => Err(TypeCode::LevelOutOfRange),
        }
    }

    pub fn level(self) -> u8 {
        match self {
            TypeScale::Tight => 0,
            TypeScale::Standard => 1,
            TypeScale::Loose => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TypeScale::Tight => "tight",
            TypeScale::Standard => "standard",
            TypeScale::Loose => "loose",
        }
    }

    pub fn ladder(self) -> &'static [u32; STEPS] {
        match self {
            TypeScale::Tight => &Self::TIGHT,
            TypeScale::Standard => &Self::STANDARD,
            TypeScale::Loose => &Self::LOOSE,
        }
    }

    /// 查表。越界返回末级：截断比返回 0 安全（0 会让下游除零或渲染出空字）。
    pub fn size_at(self, step: usize) -> u32 {
        self.ladder()
            .get(step)
            .copied()
            .unwrap_or_else(|| self.ladder().last().copied().unwrap_or(MIN_READABLE_PX))
    }

    /// 该档的字重（100..900）。字阶越松、字重越轻：松散字阶本身已占更多空间，
    /// 再用重字重会让版面「糊成一团」。
    pub fn weight(self) -> u32 {
        match self {
            TypeScale::Tight => 450,
            TypeScale::Standard => 400,
            TypeScale::Loose => 350,
        }
    }

    /// 行高（px）。按字号分桶取比例，且保证严格大于字号。
    pub fn line_height(self, size_px: u32) -> u32 {
        let permille = if size_px < 16 {
            1500
        } else if size_px < 24 {
            1400
        } else {
            1250
        };
        let raw = mul_permille(size_px, permille);
        let q = quantize_even(raw);
        if q <= size_px {
            quantize_even(size_px + 2)
        } else {
            q
        }
    }
}

fn mul_permille(value: u32, permille: u32) -> u32 {
    let v = u64::from(value) * u64::from(permille);
    let r = (v + 500) / 1000;
    if r > u64::from(u32::MAX) {
        u32::MAX
    } else {
        r as u32
    }
}

/// 取偶，**向上取**。字号宁愿大一丝也不能小一丝——向下取会在
/// 「刚好 21px」这类值上把可读性砍掉一档，而用户不会知道是舍入干的。
pub fn quantize_even(px: u32) -> u32 {
    if px < 2 {
        return 2;
    }
    let q = ((px + 1) / 2) * 2;
    if q < 2 {
        2
    } else {
        q
    }
}

/// 生效字号的来源。**必须能区分**——用户改过缩放却没生效，是设计上最容易赖掉的缺陷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontSource {
    /// 来自字阶变体。
    Gear,
    /// 来自用户系统缩放（优先级更高）。
    UserScale,
}

impl FontSource {
    pub fn label(self) -> &'static str {
        match self {
            FontSource::Gear => "变体",
            FontSource::UserScale => "用户缩放",
        }
    }
}

/// 生效字号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Effective {
    pub px: u32,
    pub source: FontSource,
    pub line_height: u32,
}

/// 解析生效字号。
///
/// 优先级：**用户缩放 > 字阶变体**。用户缩放为 1000（未设置）时走变体；
/// 否则一律以缩放结果为准，并夹到可读硬线之上。
pub fn resolve(gear: TypeScale, step: usize, user_permille: u32) -> Effective {
    let base = gear.size_at(step);
    if user_permille == USER_SCALE_MIN || user_permille == 0 {
        return Effective {
            px: base,
            source: FontSource::Gear,
            line_height: gear.line_height(base),
        };
    }
    let clamped_scale = user_permille.clamp(USER_SCALE_MIN, USER_SCALE_MAX);
    let scaled = quantize_even(mul_permille(base, clamped_scale)).max(MIN_READABLE_PX);
    Effective {
        px: scaled,
        source: FontSource::UserScale,
        line_height: gear.line_height(scaled),
    }
}

/// 双密度指标：阅读密度（行高/字号，千分比）与信息密度（千字符当量/屏，粗算）。
///
/// 信息密度用 `1_000_000 / (字号² × 行高比例)` 的近似式：字号翻倍，一屏字符数
/// 降到四分之一，这与真实排版的对数量级一致，够用来做「两个方向都别走极端」的判断。
pub fn density_pair(eff: &Effective) -> (u32, u32) {
    if eff.px == 0 {
        return (0, 0);
    }
    let reading = ((u64::from(eff.line_height) * 1000) / u64::from(eff.px)) as u32;
    let area = u64::from(eff.px) * u64::from(eff.line_height);
    let info = if area == 0 {
        0
    } else {
        (1_000_000u64 / area) as u32
    };
    (reading, info)
}

/// 单档自检。
pub fn audit_one(gear: TypeScale) -> Vec<TypeCode> {
    let mut out = Vec::new();
    let ladder = gear.ladder();
    if ladder.is_empty() {
        out.push(TypeCode::EmptyLadder);
        return out;
    }
    let mut prev = 0u32;
    for (i, size) in ladder.iter().enumerate() {
        if *size < MIN_READABLE_PX {
            out.push(TypeCode::BelowReadable);
        }
        if i > 0 && *size < prev {
            out.push(TypeCode::NonMonotonic);
        }
        // 相邻级涨幅上限 1.5 倍：超过说明阶梯断了（缺一级）。
        if i > 0 && prev > 0 && *size > prev + prev / 2 {
            out.push(TypeCode::RatioBroken);
        }
        if gear.line_height(*size) <= *size {
            out.push(TypeCode::LineHeightTooSmall);
        }
        prev = *size;
    }
    if gear.weight() < 100 || gear.weight() > 900 || gear.weight() % 50 != 0 {
        out.push(TypeCode::WeightOutOfRange);
    }
    out
}

/// 全档审计：单档 + 「用户缩放必须压过变体」的优先级证明。
pub fn audit() -> Vec<TypeCode> {
    let mut out = Vec::new();
    for g in TypeScale::ALL {
        for c in audit_one(g) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    // 优先级证明：同一 (档,步) 在缩放 1500‰ 下必须比未缩放更大且来源为 UserScale。
    for g in TypeScale::ALL {
        for step in 0..STEPS {
            let plain = resolve(g, step, USER_SCALE_MIN);
            let scaled = resolve(g, step, 1500);
            if scaled.source != FontSource::UserScale {
                out.push(TypeCode::UserScaleOverridden);
            }
            if scaled.px < plain.px || scaled.px < MIN_READABLE_PX {
                out.push(TypeCode::UserScaleOverridden);
            }
        }
    }
    out
}

/// 读屏清单：三档七级一行一档。
pub fn ladder_lines() -> Vec<String> {
    let mut out = Vec::new();
    out.push("字阶档位 | 七级字号（px） | 字重".to_string());
    for g in TypeScale::ALL {
        let sizes: Vec<String> = g.ladder().iter().map(|s| format!("{}", s)).collect();
        out.push(format!(
            "{:<8} {} | {}",
            g.name(),
            sizes.join(" "),
            g.weight()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladders_are_seven_steps_and_monotonic() {
        for g in TypeScale::ALL {
            assert_eq!(g.ladder().len(), STEPS);
            for w in g.ladder().windows(2) {
                assert!(w[0] <= w[1], "{} 阶梯必须单调", g.name());
            }
            assert_eq!(g.size_at(0) >= MIN_READABLE_PX, true);
        }
    }

    #[test]
    fn step_overflow_clamps_instead_of_zero() {
        let g = TypeScale::Standard;
        assert_eq!(g.size_at(STEPS + 5), g.size_at(STEPS - 1));
        assert!(g.size_at(999) >= MIN_READABLE_PX);
    }

    #[test]
    fn line_height_always_exceeds_size_and_is_even() {
        for g in TypeScale::ALL {
            for step in 0..STEPS {
                let s = g.size_at(step);
                let lh = g.line_height(s);
                assert!(lh > s, "{} @ {}px 行高 {}px", g.name(), s, lh);
                assert_eq!(lh % 2, 0);
            }
        }
    }

    #[test]
    fn user_scale_beats_gear() {
        let plain = resolve(TypeScale::Tight, 2, USER_SCALE_MIN);
        assert_eq!(plain.source, FontSource::Gear);
        assert_eq!(plain.px, 14);
        let scaled = resolve(TypeScale::Tight, 2, 1500);
        assert_eq!(scaled.source, FontSource::UserScale);
        assert_eq!(scaled.px, 22, "14 × 1.5 = 21 → 取偶 22");
        // 换档但同样缩放：来源仍是用户缩放（优先级不看档）
        let loose = resolve(TypeScale::Loose, 2, 1500);
        assert_eq!(loose.source, FontSource::UserScale);
        assert_eq!(loose.px, 24);
    }

    #[test]
    fn user_scale_is_clamped_to_range() {
        let huge = resolve(TypeScale::Loose, 6, 9999);
        let capped = resolve(TypeScale::Loose, 6, USER_SCALE_MAX);
        assert_eq!(huge.px, capped.px, "超上限必须夹到 225%");
        assert!(huge.px > 36);
        // 0 视为未设置
        assert_eq!(resolve(TypeScale::Standard, 2, 0).source, FontSource::Gear);
    }

    #[test]
    fn readable_floor_survives_every_path() {
        for g in TypeScale::ALL {
            for step in 0..STEPS {
                for scale in [1000u32, 1001, 1200, 2250] {
                    let e = resolve(g, step, scale);
                    assert!(
                        e.px >= MIN_READABLE_PX,
                        "{} step{} ×{}% 得到 {}px",
                        g.name(),
                        step,
                        scale,
                        e.px
                    );
                }
            }
        }
    }

    #[test]
    fn density_pair_moves_in_opposite_directions() {
        let small = resolve(TypeScale::Tight, 0, 1000);
        let large = resolve(TypeScale::Loose, 6, 1000);
        let (r1, i1) = density_pair(&small);
        let (r2, i2) = density_pair(&large);
        assert!(i1 > i2, "大字的信息密度必须更低：{} vs {}", i1, i2);
        assert!(r1 >= 1400, "小字行距应更松：{}", r1);
        assert!(r2 <= r1);
    }

    #[test]
    fn full_audit_is_clean() {
        assert!(audit().is_empty(), "{:?}", audit());
    }

    #[test]
    fn ratio_break_is_detectable() {
        // 手工构造一个断档阶梯：验证判据不是恒绿。
        let broken = [12u32, 14, 16, 18, 20, 24, 40];
        let mut flagged = false;
        for w in broken.windows(2) {
            if w[1] > w[0] + w[0] / 2 {
                flagged = true;
            }
        }
        assert!(flagged, "24 → 40 应被判为比例断裂");
    }

    #[test]
    fn ladder_lines_are_readable() {
        let l = ladder_lines();
        assert_eq!(l.len(), 4);
        assert!(l[0].contains("字阶档位"));
        assert!(l[1].contains("tight"));
    }
}
