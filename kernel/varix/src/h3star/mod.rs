//! Varix STAR I · 泳道三 H 基础通用域·三分队（AI-H3 · F301-F350）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-H3 分队的施工落位：
//! 五十项功能（F301-F350）逐项一模块，共享底盘收在 [`hbase`]（注入钟、
//! 设置登记表、旋钮与即时生效面、LRU 持久账、迟滞器、环形体验日志、
//! 分位数工具）。与既有内核模块的关系纪律（对齐 star/ 与 deskstar/
//! 域既约）：
//!
//! - 旧代 F 编号（AI-01..AI-10 划分的 F001-F500）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有模块的判据一律以
//!   《Varix STAR I start.md》主册为准（G-H 区段 F301-F350）；
//! - 跨分队依赖（F151 令牌 / F216、F436 控件件 / F095 终端 / F098 截图 /
//!   F238 锁屏 / F244 键位注册表等）未落地者一律以**显式参数/闭包
//!   注入口**承接，不反向制造编译依赖；已落地者直调公开接口；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑，注入钟确定
//!   复现）；本域模块零外部依赖（只依赖 `crate::checks` 与 alloc/core），
//!   保证隔离舱可独立编译。
//!
//! ## 域内模块地图（AI-H3 · F301-F350 · 50 项 + hbase；随批次扩列）
//!
//! | 模块 | 功能 | 主册判据锚（摘） |
//! | --- | --- | --- |
//! | [`hbase`]     | H3 共享底盘        | 钟/登记表/账本/迟滞/日志/分位 |
//! | [`setsearch`] | F301 设置中心搜索  | 同义词 100%；Top3 就地操作；<100ms |
//! | [`pagehier`]  | F302 设置页层级规范 | 三级=缺陷；≤15 条；副标题 100%；死链=0 |
//! | [`instantfx`] | F303 即时生效哲学  | ≥95%；五类例外白名单；徽标五处；<100ms |
//! | [`pagedflt`]  | F304 单页还原默认  | 只碰已改项；标记点；确认框 |
//! | [`themepack`] | F305 设置导入导出  | 白名单；跨版本；校验拒载 |
//! | [`fulltext`]  | F306 文件内容全文搜索 | 高亮；空闲构建；10MB 截断；损坏自重建 |
//! | [`srchhist`]  | F307 搜索历史与无痕 | 10 条去重；无痕不落盘；重启保持 |
//! | [`runbox`]    | F308 运行框        | 三类输入；别名表；Esc/Enter 语义 |
//! | [`winkeys`]   | F309 窗口管理快捷键族 | 六键位对照；连按轮转；Win+D 精度 |
//! | [`saveask`]   | F310 关闭前保存三问 | 默认焦点；批量勾选；未保存判定 |
//! | [`sesrestore`]| F311 会话自动恢复  | 30s 周期；恢复条；光标随恢复；清理 |
//! | [`sympanel`]  | F313 符号与表情面板 | 三页签；不抢焦点；最近 24 记忆 |
//! | [`imecore`]   | F326 输入法引擎基础 | 切分 20 组；排序可复现；自学习；离线 |
//! | [`cnensw`]    | F327 中英文切换语义 | Shift/Caps 双路；标点跟随；零吞键 |
//! | [`shuangpin`] | F328 双拼输入支持  | 四方案键位表；混输；词库共享 |
//! | [`clipbig`]   | F329 剪贴板大对象  | 50MB 阈值；引用条；内存增量 <10MB |
//! | [`keyrep`]    | F347 键盘重复参数  | 四档延迟；速率滑杆；测试框；持久化 |
//! | [`caretbold`] | F348 光标与指针加粗 | 插入符 1-4px；指针三档；即时预览 |
//! | [`ocrtake`]   | F314 截图取字      | 离线；预览可改；<2s；历史 5 条 |
//! | [`draghover`] | F315 拖拽悬停前置  | 500ms±50；划过不触发；落点高亮 |
//! | [`idlelock`]  | F316 闲置锁屏 + F317 氛围模式 | 三档+电池档；媒体豁免；职责分离 |
//! | [`pwbtn`]     | F318 电源按钮行为  | 三档×双场景；长按 4s；软件路径优先 |
//! | [`wakeresume`]| F319 唤醒即回 + F320 飞行模式 | 全链 <2s；恢复 <5s；诚实降级 |
//! | [`nearshare`] | F321 就近共享      | 四步用例；断点续传；默认隐身 |
//! | [`micind`]    | F322/F323 隐私指示 + F324 权限中心 | <200ms；purpose 缺失默认拒；热撤 |
//! | [`filevers`]  | F325 文件历史版本  | <1s 快照；对比还原可撤销；30 天/500 |
//! | [`ntfgrp`]    | F330 通知分组批量清除 | 分组折叠；全清撤销；钉选上限 3 |
//! | [`animdegrade`]| F331-F333 降级链（动画/帧率/低电量） | >90% 触发；三级阈值；<20% 触发 |
//! | [`loadresp`]  | F334 高负载保响应 + F335 指针直通 | <100ms；<16ms；1000Hz 不丢 |
//! | [`copypath`]  | F336 复制地址 + F337 命令行互通 | 引号包裹；四入口；工作目录同步 |
//! | [`multibar`]  | F338 多选操作条 + F339 空格即看 + F340 预览统一 | ≥2 阈值；首帧 <300ms；渲染单点 |
//! | [`sndmode`]   | F341 静音四档 + F349 焦点模式 | 四档矩阵；轮切；三触发源；小结准确 |
//! | [`sysgov`]    | F342-F346 系统治理五项 | 人话报告；永不静默杀；三步卸载；不抢默认；默认全关 |
//! | [`haptic`]    | F350 系统触感反馈谱 | 五项参数实测；私设=0；性能降级；F124 曲线 |
//! | [`pyfault`]   | F312 拼音与容错搜索 | 三层容错各 5；精确优先；容错开关 |
//!
//! ## 域内模块次序说明
//!
//! 上表行序即施工批次序（批次一设置与搜索 → 批次二输入与恢复 → 批次三
//! 深化层并入），与 [`run_h3star_checks`] 聚合块序一致（F312/F310 等按
//! 落位先后排布，非编号序）；行数对账与判据对照一律以
//! `_attic/aih3-f301-f350/行数对账与缺陷账本.md` 为准。

use crate::checks::CheckSet;

pub mod hbase;

/// 深化层并入基线（合并工具的域内简写）。
use hbase::merge_sets;

/// 域标识（CheckSet 聚合用）。
pub const H3_DOMAIN: &str = "h3star-h3";

/// 域块清单（hbase + 五十项，深化层经 `merge_sets` 并入）——聚合器与
/// 对账/探针工具共用的唯一构造点（一处一事实）。
pub fn h3star_blocks() -> Vec<(&'static str, CheckSet)> {
    vec![
        ("hbase", hbase::run_hbase_checks()),
        ("F301", merge_sets(merge_sets(setsearch::run_setsearch_checks(), setsearch::run_setsearch_deep_checks()), setsearch::run_setsearch_deep3_checks())),
        ("F302", pagehier::run_pagehier_checks()),
        ("F303", merge_sets(merge_sets(instantfx::run_instantfx_checks(), instantfx::run_instantfx_deep2_checks()), instantfx::run_instantfx_deep3_checks())),
        ("F304", merge_sets(pagedflt::run_pagedflt_checks(), pagedflt::run_pagedflt_deep2_checks())),
        ("F305", merge_sets(themepack::run_themepack_checks(), themepack::run_themepack_deep2_checks())),
        ("F306", merge_sets(merge_sets(fulltext::run_fulltext_checks(), fulltext::run_fulltext_deep_checks()), fulltext::run_fulltext_deep3_checks())),
        ("F307", merge_sets(merge_sets(srchhist::run_srchhist_checks(), srchhist::run_srchhist_deep2_checks()), srchhist::run_srchhist_deep3_checks())),
        ("F308", merge_sets(runbox::run_runbox_checks(), runbox::run_runbox_deep2_checks())),
        ("F309", merge_sets(winkeys::run_winkeys_checks(), winkeys::run_winkeys_deep2_checks())),
        ("F312", merge_sets(merge_sets(pyfault::run_pyfault_checks(), pyfault::run_pyfault_deep_checks()), pyfault::run_pyfault_deep2_checks())),
        ("F310", merge_sets(merge_sets(saveask::run_saveask_checks(), saveask::run_saveask_deep2_checks()), saveask::run_saveask_deep3_checks())),
        ("F311", merge_sets(merge_sets(sesrestore::run_sesrestore_checks(), sesrestore::run_sesrestore_deep2_checks()), sesrestore::run_sesrestore_deep3_checks())),
        ("F313", merge_sets(sympanel::run_sympanel_checks(), sympanel::run_sympanel_deep2_checks())),
        ("F326", merge_sets(merge_sets(merge_sets(imecore::run_imecore_checks(), imecore::run_imecore_deep_checks()), imecore::run_imecore_deep2_checks()), imecore::run_imecore_deep3_checks())),
        ("F327", merge_sets(merge_sets(cnensw::run_cnensw_checks(), cnensw::run_cnensw_deep2_checks()), cnensw::run_cnensw_deep3_checks())),
        ("F328", merge_sets(shuangpin::run_shuangpin_checks(), shuangpin::run_shuangpin_deep2_checks())),
        ("F329", merge_sets(clipbig::run_clipbig_checks(), clipbig::run_clipbig_deep2_checks())),
        ("F347", merge_sets(keyrep::run_keyrep_checks(), keyrep::run_keyrep_deep3_checks())),
        ("F348", merge_sets(caretbold::run_caretbold_checks(), caretbold::run_caretbold_deep2_checks())),
        ("F314", merge_sets(merge_sets(merge_sets(ocrtake::run_ocrtake_checks(), ocrtake::run_ocrtake_deep2_checks()), ocrtake::run_ocrtake_deep3_checks()), ocrtake::run_ocrtake_deep4_checks())),
        ("F315", merge_sets(draghover::run_draghover_checks(), draghover::run_draghover_deep2_checks())),
        ("F316", merge_sets(merge_sets(idlelock::run_idlelock_checks(), idlelock::run_idlelock_deep2_checks()), idlelock::run_idlelock_deep3_checks())),
        ("F317", idlelock::run_ambience_checks()),
        ("F318", merge_sets(merge_sets(pwbtn::run_pwbtn_checks(), pwbtn::run_pwbtn_deep2_checks()), pwbtn::run_pwbtn_deep3_checks())),
        ("F319", merge_sets(merge_sets(wakeresume::run_wakeresume_checks(), wakeresume::run_wakeresume_deep_checks()), wakeresume::run_wakeresume_deep2_checks())),
        ("F320", wakeresume::run_airlane_checks()),
        ("F321", merge_sets(merge_sets(merge_sets(nearshare::run_nearshare_checks(), nearshare::run_nearshare_deep_checks()), nearshare::run_nearshare_deep2_checks()), nearshare::run_nearshare_deep3_checks())),
        ("F322", merge_sets(micind::run_micind_checks(), micind::run_micind_deep2_checks())),
        ("F323", micind::run_micind_checks()),
        ("F324", micind::run_permctr_checks()),
        ("F325", merge_sets(merge_sets(filevers::run_filevers_checks(), filevers::run_filevers_deep2_checks()), filevers::run_filevers_deep3_checks())),
        ("F330", merge_sets(merge_sets(ntfgrp::run_ntfgrp_checks(), ntfgrp::run_ntfgrp_deep2_checks()), ntfgrp::run_ntfgrp_deep3_checks())),
        ("F331", merge_sets(merge_sets(animdegrade::run_animdegrade_checks(), animdegrade::run_animdegrade_deep_checks()), animdegrade::run_animdegrade_deep2_checks())),
        ("F332", merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(animdegrade::run_fpsadapt_checks(), animdegrade::run_animdegrade_deep3_checks()), animdegrade::run_animdegrade_deep4_checks()), animdegrade::run_animdegrade_deep5_checks()), animdegrade::run_animdegrade_deep6_checks()), animdegrade::run_animdegrade_deep7_checks()), animdegrade::run_animdegrade_deep8_checks()), animdegrade::run_animdegrade_deep9_checks()), animdegrade::run_animdegrade_deep10_checks())),
        ("F333", animdegrade::run_lowbatt_checks()),
        ("F334", merge_sets(merge_sets(loadresp::run_loadresp_checks(), loadresp::run_loadresp_deep2_checks()), loadresp::run_loadresp_deep3_checks())),
        ("F335", loadresp::run_ptrplane_checks()),
        ("F336", merge_sets(copypath::run_copypath_checks(), copypath::run_copypath_deep_checks())),
        ("F337", merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(copypath::run_cmdbg_checks(), copypath::run_cmdbg_deep2_checks()), copypath::run_copypath_deep3_checks()), copypath::run_copypath_deep4_checks()), copypath::run_copypath_deep5_checks()), copypath::run_copypath_deep6_checks()), copypath::run_copypath_deep7_checks()), copypath::run_copypath_deep8_checks()), copypath::run_copypath_deep9_checks()), copypath::run_copypath_deep10_checks()), copypath::run_copypath_deep11_checks())),
        ("F338", merge_sets(merge_sets(multibar::run_multibar_checks(), multibar::run_multibar_deep_checks()), multibar::run_multibar_deep2_checks())),
        ("F339", multibar::run_quicklook_checks()),
        ("F340", multibar::run_prevunify_checks()),
        ("F341", merge_sets(sndmode::run_sndmode_checks(), sndmode::run_sndmode_deep_checks())),
        ("F342", merge_sets(merge_sets(sysgov::run_diskchk_checks(), sysgov::run_sysgov_deep_checks()), sysgov::run_sysgov_deep2_checks())),
        ("F343", sysgov::run_memsosc_checks()),
        ("F344", merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(merge_sets(sysgov::run_appuninst_checks(), sysgov::run_sysgov_deep3_checks()), sysgov::run_sysgov_deep4_checks()), sysgov::run_sysgov_deep5_checks()), sysgov::run_sysgov_deep6_checks()), sysgov::run_sysgov_deep7_checks()), sysgov::run_sysgov_deep8_checks()), sysgov::run_sysgov_deep9_checks()), sysgov::run_sysgov_deep10_checks()), sysgov::run_sysgov_deep11_checks())),
        ("F345", sysgov::run_defapp_checks()),
        ("F346", sysgov::run_startup_checks()),
        ("F349", sndmode::run_focusmode_checks()),
        ("F350", merge_sets(merge_sets(haptic::run_haptic_checks(), haptic::run_haptic_deep2_checks()), haptic::run_haptic_deep3_checks())),
        ("F312b", merge_sets(pyfault::run_pyfault_deep3_checks(), pyfault::run_pyfault_deep4_checks())),
    ]
}

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（施工期随模块落地扩列，
/// 全量 hbase + 五十项）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），单模块超限
/// 由该模块自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_h3star_checks() -> CheckSet {
    let mut set = CheckSet::new(H3_DOMAIN);
    for (tag, sub) in h3star_blocks() {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h3_domain_aggregate_all_green() {
        let set = run_h3star_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "H3STAR-H3 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}

// ---------------------------------------------------------------------------
// 批次一：设置与搜索（F301-F309、F312）
// ---------------------------------------------------------------------------

pub mod setsearch;
pub mod pagehier;
pub mod instantfx;
pub mod pagedflt;
pub mod themepack;
pub mod fulltext;
pub mod srchhist;
pub mod runbox;
pub mod winkeys;
pub mod pyfault;
pub mod saveask;
pub mod sesrestore;
pub mod sympanel;
pub mod imecore;
pub mod cnensw;
pub mod shuangpin;
pub mod clipbig;
pub mod keyrep;
pub mod caretbold;
pub mod ocrtake;
pub mod draghover;
pub mod idlelock;
pub mod pwbtn;
pub mod wakeresume;
pub mod nearshare;
pub mod micind;
pub mod filevers;
pub mod ntfgrp;
pub mod animdegrade;
pub mod loadresp;
pub mod copypath;
pub mod multibar;
pub mod sndmode;
pub mod sysgov;
pub mod haptic;
