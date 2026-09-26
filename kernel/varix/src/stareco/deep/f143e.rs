//! 深化层二 · F143 星徽与品牌资产包（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】vxtheme 兼容容器 +【设计细节】用法图解几何
//! 规范（主册 G-D-18）：资产包清单序列化 round-trip、条款裁决引擎
//! （用途→判定→结论）、图解几何规范（间距/底色最小值）、打包器、
//! 版本映射（品牌随系统大版本）、滥用工单先礼后兵三步深化。

use crate::checks::CheckSet;
use crate::stareco::brandkit::{AbuseStage, Terms, DEFAULT_TERMS};

// ---------------------------------------------------------------------------
// 资产包清单序列化（vxtheme 容器行式承载，round-trip 无损）
// ---------------------------------------------------------------------------

/// 清单行：`kind|spec|version`（kind: logo/mono/wallpaper/vector）。
pub fn manifest_row(kind: &str, spec: &str, brand_ver: u32) -> alloc::string::String {
    alloc::format!("{}|{}|{}", kind, spec, brand_ver)
}

/// 清单行解析：三段制、规格非空、品牌版本非零。
pub fn parse_manifest_row(line: &str) -> Result<(&str, &str, u32), &'static str> {
    let p: alloc::vec::Vec<&str> = line.split('|').collect();
    if p.len() != 3 {
        return Err("清单行必须三段：kind|spec|ver");
    }
    if p[0].is_empty() || p[1].is_empty() {
        return Err("类别与规格必填");
    }
    let ver: u32 = p[2].parse().map_err(|_| "版本非数字")?;
    if ver == 0 {
        return Err("版本零值无效");
    }
    Ok((p[0], p[1], ver))
}

/// 必备 logo 规格（静态表——"logo-16".."logo-512"）。
pub const LOGO_SPECS: [&str; 5] = ["logo-16", "logo-32", "logo-48", "logo-128", "logo-512"];
pub const MONO_SPECS: [&str; 3] = ["white", "black", "accent"];

/// 清单齐套判定：五尺寸 logo + 三单色 + 矢量源 + ≥10 壁纸（资产全分辨率齐）。
pub fn manifest_complete(rows: &[(&str, &str, u32)]) -> bool {
    let has = |kind: &str, spec: &str| rows.iter().any(|(k, s, _)| *k == kind && *s == spec);
    LOGO_SPECS.iter().all(|s| has("logo", s))
        && MONO_SPECS.iter().all(|s| has("mono", s))
        && rows.iter().any(|(k, _, _)| *k == "vector")
        && rows.iter().filter(|(k, _, _)| *k == "wallpaper").count() >= 10
}

// ---------------------------------------------------------------------------
// 条款裁决引擎：用途 → 条款匹配 → 结论（清单式条款：未列入即禁止）
// ---------------------------------------------------------------------------

/// 用途清单（条款页列出的可裁决用途——枚举固定，防仲裁面膨胀）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Usage {
    NonCommercialShow,
    ReviewQuote,
    VideoProduction,
    RecolorLogo,
    OfficialEndorsementClaim,
    CommercialResale,
}

/// 裁决：与 Terms 允许清单逐条匹配——禁止清单优先（安全侧判错成本更低）。
pub fn usage_verdict(terms: &Terms, u: Usage) -> Result<&'static str, &'static str> {
    match u {
        Usage::RecolorLogo => Err("篡改主色：禁止清单第一类（品牌不可变体化）"),
        Usage::OfficialEndorsementClaim => Err("暗示官方背书：禁止清单第二类"),
        Usage::CommercialResale => Err("商业转售：资产包不含转售授权"),
        Usage::NonCommercialShow => {
            if terms.verdict("非商业展示") {
                Ok("允许：非商业展示（条款页可查）")
            } else {
                Err("该条款包未授权非商业展示")
            }
        }
        Usage::ReviewQuote => {
            if terms.verdict("评测引用") {
                Ok("允许：评测引用（注明出处）")
            } else {
                Err("该条款包未授权评测引用")
            }
        }
        Usage::VideoProduction => {
            if terms.verdict("非商业展示") {
                Ok("允许：非商业视频制作（同展示条款）")
            } else {
                Err("该条款包未授权视频使用")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 图解几何规范：间距与底色最小值（「图标正确用法」的机器可验面）
// ---------------------------------------------------------------------------

/// LOGO 展示最小尺寸与四周净空（品牌一致性 = 几何纪律）。
pub const MIN_DISPLAY_SIZE: u16 = 16;
pub const CLEAR_SPACE_RATIO_NUM: u32 = 1; // 净空 = LOGO 高度的 1/4
pub const CLEAR_SPACE_RATIO_DEN: u32 = 4;

/// 净空计算：logo 高 h 的四边净空像素。
pub fn clear_space_px(h: u16) -> u16 {
    ((h as u32) * CLEAR_SPACE_RATIO_NUM / CLEAR_SPACE_RATIO_DEN) as u16
}

/// 摆放校验：尺寸达标 + 四边净空 ≥ 计算值。
pub fn placement_ok(display_size: u16, margin_px: u16) -> bool {
    display_size >= MIN_DISPLAY_SIZE && margin_px >= clear_space_px(display_size)
}

/// 底色合规：LOGO 主色在底色上必须过对比度（复用 a11y 对比度公式——一处一事实）。
pub fn backdrop_ok(logo_rgb: (u8, u8, u8), bg_rgb: (u8, u8, u8)) -> bool {
    crate::stareco::a11yopen::contrast_gate(logo_rgb, bg_rgb, true) // 品牌资产按大字线 3:1
}

// ---------------------------------------------------------------------------
// 版本映射：品牌资产随系统大版本
// ---------------------------------------------------------------------------

/// 资产版本对系统版本的适配判定：品牌版本 ≥ 系统大版本 → 可用；
/// 落后一个以上大版本 → 提示更新（不拦截——旧 LOGO 不违法）。
pub fn brand_version_advice(brand_major: u32, system_major: u32) -> &'static str {
    if brand_major >= system_major {
        "品牌资产与系统同步"
    } else if brand_major + 1 == system_major {
        "品牌资产落后一个版本：可继续用，建议随下季更新"
    } else {
        "品牌资产严重过期：请重新下载资产包"
    }
}

// ---------------------------------------------------------------------------
// 滥用工单：先礼后兵三步深化（每步有时限与升级条件）
// ---------------------------------------------------------------------------

pub struct AbuseTicket {
    pub target: &'static str,
    pub stage: AbuseStage,
    /// 当前阶段起始日。
    pub stage_since: u32,
}

/// 每阶段等待期（天）：沟通 14 天无响应 → 下架要求。
pub const STAGE_GRACE_DAYS: u32 = 14;

impl AbuseTicket {
    pub fn new(target: &'static str, day: u32) -> Result<AbuseTicket, &'static str> {
        if target.is_empty() {
            return Err("对象必填：工单必须有可指认的滥用位置");
        }
        Ok(AbuseTicket { target, stage: AbuseStage::Contacted, stage_since: day })
    }

    /// 升级：宽限期满才许进入下一阶段（先礼后兵顺序红线——不可跳步）。
    pub fn escalate(&mut self, today: u32) -> Result<(), &'static str> {
        if today.saturating_sub(self.stage_since) < STAGE_GRACE_DAYS {
            return Err("宽限期未满：先礼后兵，不跳步");
        }
        self.stage = match self.stage {
            AbuseStage::Contacted => AbuseStage::TakedownDemand,
            // 下架要求后分歧即入 F148 仲裁（工单冻结，无自动第三步）。
            other => return Err(alloc::format!("{:?}", other).leak() as &'static str),
        };
        self.stage_since = today;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F143E_TAG: &str = "stareco-F143-deep2";

pub fn run_f143_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F143E_TAG);

    // 清单序列化
    let row = manifest_row("logo", "logo-512", 3);
    let parsed = parse_manifest_row(&row);
    set.add("f143e row parse", parsed == Ok(("logo", "logo-512", 3)), "三段行还原");
    set.add("f143e row short", parse_manifest_row("logo|logo-512").is_err(), "段数不符拒绝");
    set.add("f143e row zero ver", parse_manifest_row("logo|x|0").is_err(), "零版本拒绝");
    set.add("f143e row bad ver", parse_manifest_row("logo|x|abc").is_err(), "非数字拒绝");

    // 清单齐套
    let mut rows: alloc::vec::Vec<(&str, &str, u32)> = alloc::vec![("vector", "logo.svg", 3)];
    for s in LOGO_SPECS {
        rows.push(("logo", s, 3));
    }
    for name in MONO_SPECS {
        rows.push(("mono", name, 3));
    }
    for i in 0..10 {
        let spec: &'static str = alloc::format!("wp-{}", i).leak();
        rows.push(("wallpaper", spec, 3));
    }
    set.add("f143e manifest complete", manifest_complete(&rows), "五尺寸+三单色+矢量+10壁纸齐");
    set.add(
        "f143e manifest missing",
        !manifest_complete(&rows[..rows.len() - 1]),
        "壁纸缺一张即不齐",
    );

    // 条款裁决
    let terms = DEFAULT_TERMS;
    set.add(
        "f143e verdict show",
        usage_verdict(&terms, Usage::NonCommercialShow).is_ok(),
        "非商业展示允许",
    );
    set.add(
        "f143e verdict recolor",
        usage_verdict(&terms, Usage::RecolorLogo).is_err(),
        "改色禁止",
    );
    set.add(
        "f143e verdict endorse",
        usage_verdict(&terms, Usage::OfficialEndorsementClaim).is_err(),
        "背书禁止",
    );
    set.add(
        "f143e verdict resale",
        usage_verdict(&terms, Usage::CommercialResale).is_err(),
        "转售禁止",
    );

    // 几何规范
    set.add("f143e clear 512", clear_space_px(512) == 128, "512 高 → 128 净空");
    set.add("f143e clear 16", clear_space_px(16) == 4, "16 高 → 4 净空");
    set.add("f143e placement ok", placement_ok(128, 32), "达标摆放");
    set.add("f143e placement tight", !placement_ok(128, 31), "净空差 1px 不合格");
    set.add("f143e placement tiny", !placement_ok(12, 8), "小于最小尺寸不合格");
    set.add("f143e backdrop dark", backdrop_ok((255, 255, 255), (20, 20, 20)), "白标深底过");
    set.add("f143e backdrop wash", !backdrop_ok((200, 200, 200), (230, 230, 230)), "灰标灰底对比不足");

    // 版本映射
    set.add("f143e ver sync", brand_version_advice(2, 2).starts_with("同步"), "同版可用");
    set.add("f143e ver behind1", brand_version_advice(1, 2).starts_with("落后一个"), "落后一版可继续");
    set.add("f143e ver stale", brand_version_advice(1, 4).starts_with("严重过期"), "跨两版提醒重下");

    // 滥用工单
    let mut t = AbuseTicket::new("forum.example/steal", 100).expect("ticket");
    set.add("f143e abuse first", matches!(t.stage, AbuseStage::Contacted), "第一步先沟通");
    set.add("f143e abuse grace", t.escalate(110).is_err(), "宽限期不升级");
    let _ = t.escalate(115);
    set.add("f143e abuse second", matches!(t.stage, AbuseStage::TakedownDemand), "期满升下架要求");
    let _ = t.escalate(200);
    set.add("f143e abuse no third", t.escalate(300).is_err(), "下架后无自动第三步");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn clear_space_matrix() {
        assert_eq!(clear_space_px(0), 0);
        assert_eq!(clear_space_px(100), 25);
        assert_eq!(clear_space_px(48), 12);
        assert!(placement_ok(16, 4)); // 恰好达标
    }

    #[test]
    fn manifest_minimal_failures() {
        assert!(!manifest_complete(&[]));
        let only_wp: alloc::vec::Vec<(&str, &str, u32)> = (0..10)
            .map(|i| {
                let spec: &'static str = alloc::format!("wp-{}", i).leak();
                ("wallpaper", spec, 1)
            })
            .collect();
        assert!(!manifest_complete(&only_wp)); // 只有壁纸 = 不齐
    }

    #[test]
    fn abuse_empty_target() {
        assert!(AbuseTicket::new("", 1).is_err());
    }
}
