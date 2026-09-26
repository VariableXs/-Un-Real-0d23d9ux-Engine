//! h1star — Varix STAR I start · H 基础通用域·一分队（F201~F250 · AI-H1 分工包）。
//!
//! 本目录是《Varix STAR I start.md》主册 H-1 深化设计报告（F201-F250）
//! 的判据实装层。五十项各占一个子模块，一项一事实：
//!
//! | 项 | 子模块 | 项 | 子模块 |
//! | --- | --- | --- | --- |
//! | F201 文本选择与光标规范 | [`textsel`] | F226 窗口层级与阴影体系 | [`winlayer`] |
//! | F202 全局撤销重做框架 | [`undoframe`] | F227 窗口开合动画统一 | [`winanim`] |
//! | F203 拖拽选择框（橡皮筋） | [`rubbersel`] | F228 列表虚拟化 | [`vlist`] |
//! | F204 滚动行为统一 | [`scrolluni`] | F229 输入框占位符与清除按钮 | [`fieldui`] |
//! | F205 Tooltip 悬停提示系统 | [`tipsys`] | F230 密码输入显隐切换 | [`pwdeye`] |
//! | F206 焦点可见性与键盘导航 | [`focusnav`] | F231 表单校验时机 | [`formval`] |
//! | F207 对话框 Esc/Enter 语义统一 | [`dialksem`] | F232 日期时间选择器 | [`datepick`] |
//! | F208 进度反馈规范 | [`progfeed`] | F233 原生文件打开/保存对话框 | [`filedlg`] |
//! | F209 错误提示文案三要素 | [`errthree`] | F234 颜色选择器 | [`colorpick`] |
//! | F210 空状态设计规范 | [`emptystate`] | F235 虚拟桌面 | [`vdesk`] |
//! | F211 文本编辑通用手势 | [`edkeys`] | F236 窗口排列命令 | [`winsnap`] |
//! | F212 文件拖放落点高亮 | [`drophl`] | F237 窗口位置与尺寸记忆 | [`winmem`] |
//! | F213 双击标题栏最大化与拖离还原 | [`titlebar`] | F238 锁屏界面 | [`lockui`] |
//! | F214 窗口最小尺寸与内容自适应 | [`winsize`] | F239 亮度调节与记忆 | [`brightosd`] |
//! | F215 右键菜单层级规范 | [`menulev`] | F240 全局音量 OSD 与静音热键 | [`volosd`] |
//! | F216 复选/单选/开关三控件规范 | [`triwidget`] | F241 音频输出设备切换 | [`audioroute`] |
//! | F217 数值输入步进器与拖拽改值 | [`numspin`] | F242 网络状态指示与诊断 | [`netstate`] |
//! | F218 列表多选修饰键语义 | [`listsel`] | F243 隐藏文件与受保护文件显示 | [`hidfiles`] |
//! | F219 排序与视图记忆 | [`viewmem`] | F244 全局快捷键注册表与冲突审计 | [`hotkeyreg`] |
//! | F220 纯文本粘贴（Ctrl+Shift+V） | [`pasteplain`] | F245 减少动效开关 | [`lessmotion`] |
//! | F221 窗口内查找（Ctrl+F） | [`findbar`] | F246 字号独立调节（无障碍） | [`textscale`] |
//! | F222 字体渲染子系统 | [`fontrnd`] | F247 长文本截断规范 | [`texttrunc`] |
//! | F223 插入符闪烁与输入节奏 | [`caret`] | F248 窗口置顶（钉住） | [`winpin`] |
//! | F224 显示缩放用户档位 | [`dpiscale`] | F249 热角触发 | [`hotcorner`] |
//! | F225 主题切换免重启生效 | [`themeswap`] | F250 指针精度与双击速度基线 | [`pointerprec`] |
//!
//! 共同纪律（与主册铁律对齐，与 perfstar/star 同源）：
//! - **零堆热路径**：交互判定路径全定长结构，无 Vec/String/format!；
//!   持久化面（账本、记忆表）走 alloc 且容量有上限。
//! - **一处一事实**：每条常量在注释里写明主册依据（F 编号 + 数值原文）。
//! - **时间注入**：一切时间由调用方以参数注入（毫秒戳/分钟戳），模块不持
//!   真实时钟——宿主测试确定复现，内核侧由上层供给真值。
//! - **判据唯一源**：验收标准第一句摘自主册判据，通用十二查叠加执行。
//! - **v2 深化批（2026-09-26）**：每模块尾部追加「UI 壳接线 / 持久化 I/O /
//!   判定面扩展」段（`run_*_v2_checks` + `tests_v2`），聚合器以 Fxxxv2
//!   并联登记；对账映射见 `docs/AI-H1-检查项对账表.md`。
//! - **依赖锚点只进不横**：对 F151 令牌、F124 总谱、F081/F084 等接缝一律以
//!   显式参数/枚举注入口承接（本目录 [`h1base`] 提供 F124 曲线与令牌色
//!   整数工具），不反向制造对未落地分队的编译依赖。

pub mod h1base;
pub mod audioroute;
pub mod brightosd;
pub mod caret;
pub mod colorpick;
pub mod datepick;
pub mod dialksem;
pub mod dpiscale;
pub mod drophl;
pub mod edkeys;
pub mod emptystate;
pub mod errthree;
pub mod fieldui;
pub mod filedlg;
pub mod findbar;
pub mod focusnav;
pub mod fontrnd;
pub mod formval;
pub mod hidfiles;
pub mod hotcorner;
pub mod hotkeyreg;
pub mod lessmotion;
pub mod listsel;
pub mod lockui;
pub mod menulev;
pub mod netstate;
pub mod numspin;
pub mod pasteplain;
pub mod pointerprec;
pub mod progfeed;
pub mod pwdeye;
pub mod rubbersel;
pub mod scrolluni;
pub mod textscale;
pub mod textsel;
pub mod texttrunc;
pub mod themeswap;
pub mod titlebar;
pub mod tipsys;
pub mod triwidget;
pub mod undoframe;
pub mod vdesk;
pub mod viewmem;
pub mod vlist;
pub mod volosd;
pub mod winanim;
pub mod winlayer;
pub mod winmem;
pub mod winpin;
pub mod winsize;
pub mod winsnap;

/// 域标识（CheckSet 聚合用）。
pub const H1_DOMAIN: &str = "h1star";

/// 全域自检登记名（robust.rs KernelCheckup 用，每项一个独立域集，
/// 50 项逐项注册——判据唯一源与对账口径一致）。
pub const DOMAIN_NAMES: [&str; 50] = [
    "F201-textsel",
    "F202-undoframe",
    "F203-rubbersel",
    "F204-scrolluni",
    "F205-tipsys",
    "F206-focusnav",
    "F207-dialksem",
    "F208-progfeed",
    "F209-errthree",
    "F210-emptystate",
    "F211-edkeys",
    "F212-drophl",
    "F213-titlebar",
    "F214-winsize",
    "F215-menulev",
    "F216-triwidget",
    "F217-numspin",
    "F218-listsel",
    "F219-viewmem",
    "F220-pasteplain",
    "F221-findbar",
    "F222-fontrnd",
    "F223-caret",
    "F224-dpiscale",
    "F225-themeswap",
    "F226-winlayer",
    "F227-winanim",
    "F228-vlist",
    "F229-fieldui",
    "F230-pwdeye",
    "F231-formval",
    "F232-datepick",
    "F233-filedlg",
    "F234-colorpick",
    "F235-vdesk",
    "F236-winsnap",
    "F237-winmem",
    "F238-lockui",
    "F239-brightosd",
    "F240-volosd",
    "F241-audioroute",
    "F242-netstate",
    "F243-hidfiles",
    "F244-hotkeyreg",
    "F245-lessmotion",
    "F246-textscale",
    "F247-texttrunc",
    "F248-winpin",
    "F249-hotcorner",
    "F250-pointerprec",
];

// ---------------------------------------------------------------------------
// 域聚合器（robust.rs 单聚合注册入口——同 U2/J2/I4 容量纪律，不占
// domains 定长数组名额；域内 50 项逐项红绿在子行展开）
// ---------------------------------------------------------------------------

/// 全域自检：50 项逐项 CheckSet 收进一个域集（v1 判据实装批 + v2 深化批
/// 各 50 块，共 100 块——v2 块名为 Fxxxv2，对账口径见
/// `docs/AI-H1-检查项对账表.md`）。
/// 判据唯一源：每子集的首条对账「主册验收标准第一句 + 通用十二查」。
pub fn run_h1_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new(H1_DOMAIN);
    let blocks = [
        ("F201", textsel::run_textsel_checks as fn() -> crate::checks::CheckSet),
        ("F202", undoframe::run_undoframe_checks()),
        ("F203", rubbersel::run_rubbersel_checks()),
        ("F204", scrolluni::run_scrolluni_checks()),
        ("F205", tipsys::run_tipsys_checks()),
        ("F206", focusnav::run_focusnav_checks()),
        ("F207", dialksem::run_dialksem_checks()),
        ("F208", progfeed::run_progfeed_checks()),
        ("F209", errthree::run_errthree_checks()),
        ("F210", emptystate::run_emptystate_checks()),
        ("F211", edkeys::run_edkeys_checks()),
        ("F212", drophl::run_drophl_checks()),
        ("F213", titlebar::run_titlebar_checks()),
        ("F214", winsize::run_winsize_checks()),
        ("F215", menulev::run_menulev_checks()),
        ("F216", triwidget::run_triwidget_checks()),
        ("F217", numspin::run_numspin_checks()),
        ("F218", listsel::run_listsel_checks()),
        ("F219", viewmem::run_viewmem_checks()),
        ("F220", pasteplain::run_pasteplain_checks()),
        ("F221", findbar::run_findbar_checks()),
        ("F222", fontrnd::run_fontrnd_checks()),
        ("F223", caret::run_caret_checks()),
        ("F224", dpiscale::run_dpiscale_checks()),
        ("F225", themeswap::run_themeswap_checks()),
        ("F226", winlayer::run_winlayer_checks()),
        ("F227", winanim::run_winanim_checks()),
        ("F228", vlist::run_vlist_checks()),
        ("F229", fieldui::run_fieldui_checks()),
        ("F230", pwdeye::run_pwdeye_checks()),
        ("F231", formval::run_formval_checks()),
        ("F232", datepick::run_datepick_checks()),
        ("F233", filedlg::run_filedlg_checks()),
        ("F234", colorpick::run_colorpick_checks()),
        ("F235", vdesk::run_vdesk_checks()),
        ("F236", winsnap::run_winsnap_checks()),
        ("F237", winmem::run_winmem_checks()),
        ("F238", lockui::run_lockui_checks()),
        ("F239", brightosd::run_brightosd_checks()),
        ("F240", volosd::run_volosd_checks()),
        ("F241", audioroute::run_audioroute_checks()),
        ("F242", netstate::run_netstate_checks()),
        ("F243", hidfiles::run_hidfiles_checks()),
        ("F244", hotkeyreg::run_hotkeyreg_checks()),
        ("F245", lessmotion::run_lessmotion_checks()),
        ("F246", textscale::run_textscale_checks()),
        ("F247", texttrunc::run_texttrunc_checks()),
        ("F248", winpin::run_winpin_checks()),
        ("F249", hotcorner::run_hotcorner_checks()),
        ("F250", pointerprec::run_pointerprec_checks()),
    ];
    // v2 深化批（2026-09-26）：UI 壳接线 / 持久化 I/O / 判定面扩展，逐项并联。
    let v2 = [        ("F201v2", textsel::run_textsel_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F202v2", undoframe::run_undoframe_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F203v2", rubbersel::run_rubbersel_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F204v2", scrolluni::run_scrolluni_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F205v2", tipsys::run_tipsys_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F206v2", focusnav::run_focusnav_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F207v2", dialksem::run_dialksem_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F208v2", progfeed::run_progfeed_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F209v2", errthree::run_errthree_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F210v2", emptystate::run_emptystate_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F211v2", edkeys::run_edkeys_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F212v2", drophl::run_drophl_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F213v2", titlebar::run_titlebar_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F214v2", winsize::run_winsize_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F215v2", menulev::run_menulev_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F216v2", triwidget::run_triwidget_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F217v2", numspin::run_numspin_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F218v2", listsel::run_listsel_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F219v2", viewmem::run_viewmem_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F220v2", pasteplain::run_pasteplain_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F221v2", findbar::run_findbar_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F222v2", fontrnd::run_fontrnd_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F223v2", caret::run_caret_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F224v2", dpiscale::run_dpiscale_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F225v2", themeswap::run_themeswap_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F226v2", winlayer::run_winlayer_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F227v2", winanim::run_winanim_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F228v2", vlist::run_vlist_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F229v2", fieldui::run_fieldui_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F230v2", pwdeye::run_pwdeye_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F231v2", formval::run_formval_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F232v2", datepick::run_datepick_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F233v2", filedlg::run_filedlg_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F234v2", colorpick::run_colorpick_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F235v2", vdesk::run_vdesk_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F236v2", winsnap::run_winsnap_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F237v2", winmem::run_winmem_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F238v2", lockui::run_lockui_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F239v2", brightosd::run_brightosd_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F240v2", volosd::run_volosd_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F241v2", audioroute::run_audioroute_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F242v2", netstate::run_netstate_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F243v2", hidfiles::run_hidfiles_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F244v2", hotkeyreg::run_hotkeyreg_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F245v2", lessmotion::run_lessmotion_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F246v2", textscale::run_textscale_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F247v2", texttrunc::run_texttrunc_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F248v2", winpin::run_winpin_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F249v2", hotcorner::run_hotcorner_v2_checks() as fn() -> crate::checks::CheckSet),
        ("F250v2", pointerprec::run_pointerprec_v2_checks() as fn() -> crate::checks::CheckSet),
    ];
    for (tag, sub) in v2 {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
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
    fn h1_domain_aggregate_all_green() {
        let set = run_h1_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "H1 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
        assert!(!set.truncated(), "H1 域聚合溢出（块数超 CheckSet 上限）");
        // 100 块在册（50 项 v1 判据实装 + 50 项 v2 深化批）。
        assert_eq!(set.len(), DOMAIN_NAMES.len() * 2);
    }

    #[test]
    fn h1_domain_names_sequential() {
        // DOMAIN_NAMES：50 项、F201 起连续编号、无重号无断档。
        assert_eq!(DOMAIN_NAMES.len(), 50);
        for (i, name) in DOMAIN_NAMES.iter().enumerate() {
            let expect = format!("F{}", 201 + i);
            assert!(name.starts_with(&expect), "第 {} 项登记名 {} 与序号 {} 不符", i, name, expect);
        }
        // 去重检查（一处一事实：登记名不重复）。
        for i in 0..DOMAIN_NAMES.len() {
            for j in (i + 1)..DOMAIN_NAMES.len() {
                assert_ne!(DOMAIN_NAMES[i], DOMAIN_NAMES[j], "登记名重复：{} / {}", i, j);
            }
        }
    }
}
