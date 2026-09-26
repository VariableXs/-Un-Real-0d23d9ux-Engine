//! Varix STAR I · 泳道三 H 基础通用域·二分队（AI-H2 · F251-F300）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-H2 分队的施工落位：
//! 五十项功能（F251-F300）逐项一模块，共享底盘收在 [`h2base`]（文件名
//! 语义、命名碰撞、天窗边界、网格落位）。与既有内核模块的关系纪律：
//!
//! - 旧代 F 编号（TRINITY/M600 等波次的 F251~F475）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有模块的判据一律以
//!   《Varix STAR I start.md》主册为准，引用格式 `F2xx`/`F300` 均指
//!   STAR I 语义；
//! - 依赖其他分队的接缝（F064 低延迟链、F068 资产管线、F072 最近使用
//!   引擎、F084 网格、F124 动画谱、F151 令牌、F268 预警等）一律以
//!   **显式参数/闭包注入口**承接，不反向制造对未落地模块的编译依赖；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 判据锚 |
//! | --- | --- | --- |
//! | [`h2base`]     | 共享底盘（文件名/天窗/网格） | 五十项通用件 |
//! | [`xlog`]       | 域体验日志框架（十三章） | 挫败指纹；隐私红线 |
//! | [`h2knob`]     | 域旋钮登记表 | 零魔法数；主册依据 |
//! | [`h2persist`]  | 原子写底盘（红线④） | 三段式；断电注入 |
//! | [`h2diag`]     | 域诊断汇总 | 三色分级；最丑角落 |
//! | [`h2geo`]      | 几何布局引擎（深化一） | F276 四款；F286 拼接；F298 流式 |
//! | [`h2rank`]     | 排序评级引擎（深化一） | F257/F274/F291/F299 |
//! | [`h2cache`]    | 三层缓存引擎（深化一） | O(1) LRU；失效扇出；预算收口 |
//! | [`h2ledger`]   | 账目引擎（深化一） | 哈希链；B-2902；断点账 |
//! | [`h2snap`]     | 快照序列化层（深化一） | 版本封包；损坏容错 |
//! | [`h2curve`]    | 动效参数引擎（深化二） | F124 四档曲线；惯性采样；打断对称 |
//! | [`h2edit`]     | 编辑态几何引擎（深化二） | F260 区间；F265 三段；F272 菜单 |
//! | [`h2taskbook`] | 传输任务簿（深化二） | F269 状态机；F270 三选一 |
//! | [`h2launch`]   | 启动编排引擎（深化二） | F282 路由；F283 三拍子 |
//! | [`h2screen`]   | 显示编排引擎（深化二） | F277 随迁；F278 指纹记忆 |
//! | [`mediarbit`]   | F251 媒体会话仲裁 | 六态仲裁；OSD 归属；<50ms |
//! | [`tbgroup`]     | F252 任务栏按钮合并与分组 | 三档策略；3/7/10 角标 |
//! | [`quickpin`]    | F253 快速访问固定 | 固定/推荐共存；5+5 配额 |
//! | [`translucent`] | F254 窗口透明材质规范 | 三处白名单；降级恢复；4.5:1 |
//! | [`textdrop`]    | F255 文本拖放 | 四落点；Ctrl 语义；终端停一停 |
//! | [`hscroll`]     | F256 横向滚动语义 | Shift 映射；双轴独立；冻结列 |
//! | [`openwith`]    | F257 打开方式选择器 | 评级排序；不勾不写；Edge 兜底 |
//! | [`deskmenu`]    | F258 桌面右键菜单全集 | 六项顺序；注入拦截审计 |
//! | [`newmenu`]     | F259 新建菜单与命名初态 | 三件清单；初态三判据 |
//! | [`rename`]      | F260 行内重命名 | 三入口；扩展名隔离；8 字符拒绝 |
//! | [`delkeys`]     | F261 删除与 Shift+Delete | 两路对照；焦点取消；占用定位 |
//! | [`dragsense`]   | F262 拖拽复制/移动语义 | 六组合；盘符边界 |
//! | [`sendto`]      | F263 「发送到」菜单 | 四项默认；蓝牙动态；防重名 |
//! | [`propdlg`]     | F264 属性对话框 | 同源双形制；勾选即生效 |
//! | [`addredit`]    | F265 地址栏可编辑与补全 | 真实枚举补全；三入口 |
//! | [`navstack`]    | F266 后退/前进与 Alt+方向键 | 栈语义；跳步；上限 100 |
//! | [`storagesense`] | F267 存储感知自动清理 | 29/30/31 边界；空闲闸 |
//! | [`diskwarn`]    | F268 磁盘空间预警 | 三阈值；每日节流；目录聚合 |
//! | [`copyresume`]  | F269 长复制暂停与恢复 | 块边界暂停；断点校验 |
//! | [`opretry`]     | F270 文件操作错误重试 | 不中断；三选一；人话原因 |
//! | [`extabs`]      | F271 资源管理器多标签页 | 五组键位；拖出拖回 |
//! | [`textmenu`]    | F272 文本框右键菜单 | 六项清单；置灰稳定 |
//! | [`docpos`]      | F273 文档上次位置记忆 | 三恢复精度；损坏容错 |
//! | [`allapps`]     | F274 「所有应用」列表 | 混排二序列；索引跳段 |
//! | [`powermenu`]   | F275 开始菜单电源菜单 | 三+一项；两路同账 |
//! | [`snapgroup`]   | F276 贴靠布局组 | 四款几何；8px 缝；记忆三款 |
//! | [`multitb`]     | F277 多显示器任务栏策略 | 三策略归属；随迁 |
//! | [`projmode`]    | F278 投影/显示模式切换 | 四模式；指纹记忆 |
//! | [`padgest`]     | F279 触控板手势集 | 七手势；零误触；改禁持久化 |
//! | [`longpress`]   | F280 触屏长按右键 | 500ms；48px 防遮挡；44px 触区 |
//! | [`notifrule`]   | F281 通知交互细则 | 两按钮；5s 横幅；30s 合并 |
//! | [`singleton`]   | F282 应用单例策略 | 三策略；两入口同路由 |
//! | [`bootskel`]    | F283 应用启动骨架与首窗就绪 | 三拍子；2s 门槛 |
//! | [`notresp`]     | F284 无响应判定与恢复 | 5s 判定；抢救快照 |
//! | [`iconcache`]   | F285 图标缓存与刷新 | 三层缓存；损坏自愈；LRU |
//! | [`wallmulti`]   | F286 壁纸多屏设置 | 三模式；轮换默认关 |
//! | [`extraclk`]    | F287 附加时钟 | 上限 2；昼夜图标；离线正确 |
//! | [`fontmgr`]     | F288 字体管理 | 损坏拒绝；系统锁定；用户级装 |
//! | [`printq`]      | F289 打印队列中心 | 状态机；60s 卡住；人话映射 |
//! | [`devpage`]     | F290 外设状态页 | 离线灰显；诊断人话 |
//! | [`powerank`]    | F291 耗电排行 | 3 倍异常；只展示不越权 |
//! | [`lnkhealth`]   | F292 快捷方式健康 | 三类病；循环检测；两出路 |
//! | [`removask`]    | F293 可移动介质接入询问 | 10s 收起；Autorun 零执行 |
//! | [`safeeject`]   | F294 安全弹出与拔出保护 | 写入中拦截；强拔标注 |
//! | [`timesync`]    | F295 时间同步与准确性 | NTP 2s 阈值；48h 漂移补偿 |
//! | [`regionfmt`]   | F296 区域显示格式 | 单点服务；六格式项；KB=1024B |
//! | [`walldim`]     | F297 壁纸暗色压暗 | 30%/15%；浅色直通；0-50% |
//! | [`tileedit`]    | F298 快速设置磁贴编辑 | 默认 8；候选 16；三动作 |
//! | [`recommends`]  | F299 开始菜单推荐区 | 三来源；7 天徽标；广告位=0 |
//! | [`iconlang`]    | F300 系统图标语汇总表 | 24px 栅格；三态一致；孤例=0 |

use crate::checks::CheckSet;

pub mod addredit;
pub mod allapps;
pub mod bootskel;
pub mod copyresume;
pub mod delkeys;
pub mod deskmenu;
pub mod devpage;
pub mod diskwarn;
pub mod docpos;
pub mod dragsense;
pub mod extabs;
pub mod extraclk;
pub mod fontmgr;
pub mod h2base;
pub mod h2cache;
pub mod h2curve;
pub mod h2diag;
pub mod h2edit;
pub mod h2geo;
pub mod h2knob;
pub mod h2launch;
pub mod h2ledger;
pub mod h2persist;
pub mod h2rank;
pub mod h2screen;
pub mod h2snap;
pub mod h2taskbook;
pub mod hscroll;
pub mod iconcache;
pub mod iconlang;
pub mod lnkhealth;
pub mod longpress;
pub mod mediarbit;
pub mod multitb;
pub mod navstack;
pub mod newmenu;
pub mod notifrule;
pub mod notresp;
pub mod openwith;
pub mod opretry;
pub mod padgest;
pub mod powermenu;
pub mod powerank;
pub mod printq;
pub mod projmode;
pub mod propdlg;
pub mod quickpin;
pub mod recommends;
pub mod regionfmt;
pub mod removask;
pub mod rename;
pub mod safeeject;
pub mod sendto;
pub mod singleton;
pub mod snapgroup;
pub mod storagesense;
pub mod tbgroup;
pub mod textdrop;
pub mod textmenu;
pub mod tileedit;
pub mod timesync;
pub mod translucent;
pub mod wallmulti;
pub mod walldim;
pub mod xlog;

/// 域标识（CheckSet 聚合用）。
pub const H2_DOMAIN: &str = "h2star-h2";

/// 域内自足的 CheckSet 合并（走公开 add/tally/truncated 口，不依赖
/// checks.rs 的未落位扩展——多 AI 并行纪律：只用已冻结的底盘 API）。
/// 超容时 add 内部计数 dropped，合并结果不静默丢。
fn merge_sets(parts: &[CheckSet]) -> CheckSet {
    let mut out = CheckSet::new("h2-h2core");
    for p in parts {
        for i in 0..p.len() {
            if let Some(c) = p.get(i) {
                out.add(c.name, c.passed, c.detail);
            }
        }
    }
    out
}

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（深化二起基础设施五件
/// 合并为 `h2core` 一块——CheckSet 上限 64 内腾出席位给批次引擎：
/// h2core + 深化一五件 + 深化二五件 + F251-F300，共 61 块）。
pub fn run_h2_checks() -> CheckSet {
    let mut set = CheckSet::new(H2_DOMAIN);
    let core = merge_sets(&[
        h2base::run_h2base_checks(),
        h2knob::run_h2knob_checks(),
        xlog::run_xlog_checks(),
        h2persist::run_h2persist_checks(),
        h2diag::run_h2diag_checks(),
    ]);
    let blocks: [(&'static str, CheckSet); 61] = [
        ("h2core", core),
        ("h2geo", h2geo::run_h2geo_checks()),
        ("h2rank", h2rank::run_h2rank_checks()),
        ("h2cache", h2cache::run_h2cache_checks()),
        ("h2ledger", h2ledger::run_h2ledger_checks()),
        ("h2snap", h2snap::run_h2snap_checks()),
        ("h2curve", h2curve::run_h2curve_checks()),
        ("h2edit", h2edit::run_h2edit_checks()),
        ("h2taskbook", h2taskbook::run_h2taskbook_checks()),
        ("h2launch", h2launch::run_h2launch_checks()),
        ("h2screen", h2screen::run_h2screen_checks()),
        ("F251", mediarbit::run_mediarbit_checks()),
        ("F252", tbgroup::run_tbgroup_checks()),
        ("F253", quickpin::run_quickpin_checks()),
        ("F254", translucent::run_translucent_checks()),
        ("F255", textdrop::run_textdrop_checks()),
        ("F256", hscroll::run_hscroll_checks()),
        ("F257", openwith::run_openwith_checks()),
        ("F258", deskmenu::run_deskmenu_checks()),
        ("F259", newmenu::run_newmenu_checks()),
        ("F260", rename::run_rename_checks()),
        ("F261", delkeys::run_delkeys_checks()),
        ("F262", dragsense::run_dragsense_checks()),
        ("F263", sendto::run_sendto_checks()),
        ("F264", propdlg::run_propdlg_checks()),
        ("F265", addredit::run_addredit_checks()),
        ("F266", navstack::run_navstack_checks()),
        ("F267", storagesense::run_storagesense_checks()),
        ("F268", diskwarn::run_diskwarn_checks()),
        ("F269", copyresume::run_copyresume_checks()),
        ("F270", opretry::run_opretry_checks()),
        ("F271", extabs::run_extabs_checks()),
        ("F272", textmenu::run_textmenu_checks()),
        ("F273", docpos::run_docpos_checks()),
        ("F274", allapps::run_allapps_checks()),
        ("F275", powermenu::run_powermenu_checks()),
        ("F276", snapgroup::run_snapgroup_checks()),
        ("F277", multitb::run_multitb_checks()),
        ("F278", projmode::run_projmode_checks()),
        ("F279", padgest::run_padgest_checks()),
        ("F280", longpress::run_longpress_checks()),
        ("F281", notifrule::run_notifrule_checks()),
        ("F282", singleton::run_singleton_checks()),
        ("F283", bootskel::run_bootskel_checks()),
        ("F284", notresp::run_notresp_checks()),
        ("F285", iconcache::run_iconcache_checks()),
        ("F286", wallmulti::run_wallmulti_checks()),
        ("F287", extraclk::run_extraclk_checks()),
        ("F288", fontmgr::run_fontmgr_checks()),
        ("F289", printq::run_printq_checks()),
        ("F290", devpage::run_devpage_checks()),
        ("F291", powerank::run_powerank_checks()),
        ("F292", lnkhealth::run_lnkhealth_checks()),
        ("F293", removask::run_removask_checks()),
        ("F294", safeeject::run_safeeject_checks()),
        ("F295", timesync::run_timesync_checks()),
        ("F296", regionfmt::run_regionfmt_checks()),
        ("F297", walldim::run_walldim_checks()),
        ("F298", tileedit::run_tileedit_checks()),
        ("F299", recommends::run_recommends_checks()),
        ("F300", iconlang::run_iconlang_checks()),
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
    fn h2_domain_aggregate_all_green() {
        let set = run_h2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "H2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
        assert!(!set.truncated(), "H2 域聚合溢出（块数超 CheckSet 上限）");
    }
}
