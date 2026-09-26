//! Varix STAR I · 泳道三 J 鼠标域·二分队（AI-J2 · F621-F640）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-J2 分队的施工落位：
//! 二十项功能（F621-F640）逐项一模块，共享底盘收在 [`jbase`]（像素面、
//! 定点色彩数学、Lanczos3 重采样、15 标准态方案模型、.vxcur 容器、
//! 内置默认方案）。与既有内核模块的关系纪律（对齐 star/secstar2/
//! deskstar 域既约）：
//!
//! - 旧代 F 编号（AI-01..AI-10 划分的 F001-F500）与本目录的 STAR I 新
//!   编号**同号不同义**——本目录所有模块的判据一律以《Varix STAR I
//!   start.md》主册第 8 部分（J-0 域内总纲 + F621-F640 各节）为准；
//! - 跨分队依赖（F350 触感谱、F594 录屏高亮、F391 vxtheme、F147 随身
//!   同步、F244/F301 聚合、F151/F116 令牌、A4 门禁、F372 留痕、F633
//!   管线等接缝）一律以**显式参数/闭包注入口**承接，不反向制造对未
//!   落地模块的编译依赖；
//! - 与 AI-J1（F601-F620）同域不共目录：分队落位互相独立；J1 未落地
//!   的接缝（速度曲线族/滚轮档/侧键库/手势库）以 spec 结构体投影承接
//!   （F623/F624 的四件数据模型即冻结接口面）；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合
//!   器）与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`jbase`]     | J2 共享底盘        | 像素/色彩/重采样/方案模型/vxcur |
//! | [`clickanim`] | F621 指针点击动效档 | 0.94/120ms/8px/200ms；F350 对齐；F594 边界 |
//! | [`wheelsnd`]  | F622 滚轮交互音效   | 逐档同步；40%/两音色；E5 登记；勿扰联动 |
//! | [`vtheme`]    | F623 鼠标档案入 vxtheme | 四件打包往返；三例校验；同源对账；回退链 |
//! | [`dashbrd`]   | F624 鼠标设置总览仪表盘 | 四信息；四直达；同源对账；一键恢复 |
//! | [`workshop`]  | F625 指针绘制工坊   | 三笔刷；双倍率同屏；洋葱皮 30%；16 帧纪律 |
//! | [`recolor`]   | F626 指针颜色重染   | 三滑杆；强调色对拍；ΔE<1 帧同步；非破坏 |
//! | [`checker`]   | F627 指针语义检查器 | 四检注入全对；四路修复；回退链；可撤销 |
//! | [`library`]   | F628 指针方案库管理器 | 缩略墙；三视图；往返一致；50 上限；E4 对账 |
//! | [`themeclr`]  | F629 主题派生指针配色 | ≥3:1 描边；存为新方案；令牌同源；F626 边界 |
//! | [`sharing`]   | F630 指针分享链路   | 四链路；体检前置；签名黄条；peblock；F133 |
//! | [`a11ytmpl`]  | F631 无障碍创作模板 | 三模板判据；改形仍绿；入库即用；免检映射 |
//! | [`audit`]     | F632 指针渲染保真审计 | 24 组合；标红阈值；重生成；内置基线过审 |
//! | [`curimport`] | F633 .cur/.ani 全量导入 | 100 样本对拍；三编码族；帧序保真；定位报错 |
//! | [`svgimport`] | F634 SVG 指针直用   | 双层标记；双倍率栅格；热点向导；诚实报错 |
//! | [`sideload`]  | F635 指针包侧载链   | 注册链路；不静默替换；卸载确认；双入口同源 |
//! | [`dpicontract`] | F636 DPI 自适配契约 | 原生优先；矢量派生；三处标注；缓存零开销 |
//! | [`hotspotfix`] | F637 热区补偿      | 三例检测；≤2px 对拍；补偿非破坏；一键采纳 |
//! | [`winbridge`] | F638 Windows 方案迁移桥 | Win10/11 双样本；逐态保真；双入口；空态诚实 |
//! | [`gate`]      | F639 异形指针安全闸 | 三闸全拦；熔断自愈；零误拦；管理员钳制 |
//! | [`ledger`]    | F640 J 域收官登记与兼容台账 | 640 检查点；生态台账；季度审视；五处边界 |

use crate::checks::CheckSet;

pub mod a11ytmpl;
pub mod audit;
pub mod checker;
pub mod clickanim;
pub mod curimport;
pub mod dashbrd;
pub mod dpicontract;
pub mod gate;
pub mod hotspotfix;
pub mod jbase;
pub mod ledger;
pub mod library;
pub mod recolor;
pub mod sharing;
pub mod sideload;
pub mod svgimport;
pub mod themeclr;
pub mod vtheme;
pub mod wheelsnd;
pub mod winbridge;
pub mod workshop;

/// 域标识（CheckSet 聚合用）。
pub const JSTAR2_DOMAIN: &str = "jstar2-j2";

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（21 块：jbase + F621-F640）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——本聚合器按
/// 「每模块一行」登记，永不超容；单模块自身超限时由该模块负责裁剪。
pub fn run_jstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(JSTAR2_DOMAIN);
    let blocks: [(&'static str, CheckSet); 21] = [
        ("jbase", jbase::run_jbase_checks()),
        ("F621", clickanim::run_clickanim_checks()),
        ("F622", wheelsnd::run_wheelsnd_checks()),
        ("F623", vtheme::run_vtheme_checks()),
        ("F624", dashbrd::run_dashbrd_checks()),
        ("F625", workshop::run_workshop_checks()),
        ("F626", recolor::run_recolor_checks()),
        ("F627", checker::run_checker_checks()),
        ("F628", library::run_library_checks()),
        ("F629", themeclr::run_themeclr_checks()),
        ("F630", sharing::run_sharing_checks()),
        ("F631", a11ytmpl::run_a11ytmpl_checks()),
        ("F632", audit::run_audit_checks()),
        ("F633", curimport::run_curimport_checks()),
        ("F634", svgimport::run_svgimport_checks()),
        ("F635", sideload::run_sideload_checks()),
        ("F636", dpicontract::run_dpicontract_checks()),
        ("F637", hotspotfix::run_hotspotfix_checks()),
        ("F638", winbridge::run_winbridge_checks()),
        ("F639", gate::run_gate_checks()),
        ("F640", ledger::run_ledger_checks()),
    ];
    for (tag, sub) in blocks {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jstar2_domain_aggregate_all_green() {
        let set = run_jstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "JSTAR2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
