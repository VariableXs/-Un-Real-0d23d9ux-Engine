//! svstar2 — Varix STAR II · VE 图形引擎册 · 用户态服务落位。
//!
//! # 落位依据（★ 位置铁律，勿改 ★）
//!
//! 《VE Varix STAR II · 总纲与施工书》铁律 6：
//! > **内核红线**：全部功能针对 VARIX Rust 内核编写；内核态只留合成裁决与安全，
//! > VE 全部活在用户态服务；引擎崩溃零拖垮。
//!
//! 读法（勿误读为"能用前端语言写"）：
//! - 「用户态服务」指**不在内核态特权上下文运行**——崩溃不拖垮内核；
//! - 本仓内核树是 no_std Rust（`kernel/varix/Cargo.toml` 的 `[dependencies]` 为空，
//!   `[[bin]]` 带 `required-features = ["kernel-image"]`）；
//! - TypeScript 编译出 JS，**必须靠 V8/JSC 运行时才能执行**，内核态无堆、无 GC、
//!   无动态链接，装不下也不允许跑；且 GC 停顿与内核延迟硬要求直接冲突。
//! - 故 VE 全 6400 项落地为 **Rust**，本目录即其家。
//!
//! 目录名沿用仓库既有命名先例（`svstar` 服务守护域 → `svstar2` 承载 STAR II）。
//!
//! # 命名约定
//!
//! | 前缀 | 含义 |
//! | --- | --- |
//! | `vea01` … `vea10` | VE-A 域（内核图形抽象层，F0001-F0200）|
//! | `veb01` … `veb10` | VE-B 域（GPU 驱动矩阵，F0201-F0400）|
//! | 后续按域顺延 | 域字母与册内 `VE-x` 一致 |
//!
//! # 与既有模块的关系纪律（对齐 svstar/ 与 genstar2/ 的域约）
//!
//! - 本目录只放 **VE 册**功能；CoRun 册（操作系统本体/三通道兼容）走既有内核模块，
//!   CGPU 册（计算内核）另设落位——三册互不混装；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检并登记进本域聚合器，
//!   聚合器在 `robust.rs` 以单行注册（不占 domains 定长数组名额）；
//! - 每个模块带 `#[cfg(test)]` 单元测试，宿主侧 `cargo test` 直跑；
//! - 全部确定性算法、零 IO、可序列化：时间用逻辑 tick 注入，不用墙钟——
//!   保证回归可复现、对拍可重现（规格对拍红线）。
//!
//! # 域内模块地图（施工中，随进度增补）
//!
//! | 模块 | 功能 | 判据锚 |
//! | --- | --- | --- |
//! | [`vea01`] | F0001 虚拟显卡探测仲裁器 | VE 册 #VE-F0001 |
//! | [`vea02`] | F0002 图形上下文生命周期管理器 | VE 册 #VE-F0002 |
//! | [`vea03`] | F0003 围栏与同步原语集 | VE 册 #VE-F0003 |
//! | [`vea04`] | F0004 命令缓冲环形分配器 | VE 册 #VE-F0004 |
//! | [`vea18_sampler`] | F0018 采样器状态库 | VE 册 #VE-F0018 |
//! | [`vea11_hotplug`] | F0011 适配器热插拔与路径重选 | VE 册 #VE-F0011 |
//! | [`vea12_probe`] | F0012 渲染探针与时间戳基础设施 | VE 册 #VE-F0012 |
//! | [`vea13_softfall`] | F0013 软渲染回退路径 | VE 册 #VE-F0013 |
//! | [`vea14_snapshot`] | F0014 上下文快照与场景重放 | VE 册 #VE-F0014 |
//! | [`vea15_errclass`] | F0015 渲染错误分类与上抛纪律 | VE 册 #VE-F0015 |
//! | [`ved13_dirty`] | F0613 图层脏区收集（树级） | VE 册 #VE-F0613 |
//! | [`vew01_sdk_arch`] | F4601 W 域开工与插件 SDK 总架构（四层/双承诺/承接/层冻结） | VE 册 #VE-F4601 |
//! | [`ved14_traverse`] | F0614 图层渲染遍历器 | VE 册 #VE-F0614 |
//! | [`vem02_track`] | F2402 关键帧轨道系统（六类轨道/容器多轨/绑定协议/单源扩展） | VE 册 #VE-F2402 |
//! | [`vem03_interp`] | F2403 关键帧插值（四插值器/可插拔注册/确定性/贝塞尔纪律） | VE 册 #VE-F2403 |
//! | [`ver01_arch`] | F3401 令牌运行时架构（四件两律总纲） | VE 册 #VE-F3401 |
//! | [`ver02_arch`] | F3601 R 域开工与域号 ADR（跳段裁决+五板块十项映射+四域分工+收敛复述） | VE 册 #VE-F3601 |
//! | [`vep01_arch`] | F3001 P 域开工与动效库总架构（三组接口+十项映射+单源分工+三底线+第一红线） | VE 册 #VE-F3001 |
//! | [`veq01_pipeline`] | F3201 Q 域资源管线总架构（六段签名+十项映射+收敛红线） | VE 册 #VE-F3201 |
//! | [`veq02_graph`] | F3202 资源模型与引用图（五要素+四用途单源+32MB 红线） | VE 册 #VE-F3202 |
//! | [`vee01_arch`] | F0801 文字渲染域总架构（四段单向流+ 三向兑现 + 1.5ms 预算） | VE 册 #VE-F0801 |
//! | [`vee02_utf8`] | F0802 字符编码与 UTF-8 解码（四档处置 + 偏移表 + 200MB/s） | VE 册 #VE-F0802 |
//! | [`vee03_outline`] | F0803 字形轮廓与贝塞尔（二次升三次 + 围向约定 + 1/64 量化） | VE 册 #VE-F0803 |
//! | [`vet01_a11y_render_pipeline`] | F3802 无障碍渲染管线 | VE 册 #VE-F3802 |
//! | [`ves01_sdomain_arch`] | F3801 S 域开工与无障碍渲染总架构 | VE 册 #VE-F3801 |
//! | [`vei02_locale`] | F4002 语言标签与 Locale 模型（BCP47 四段+扩展/容错表/回退链/解析缓存/单源声明） | VE 册 #VE-F4002 |
//! | [`veu01_arch`] | F4201 U 域开工与一致性总架构（五层+接口冻结+承接落地+双维入约） | VE 册 #VE-F4201 |
//! | [`vec14_include`] | F0414 include 解析与循环防护（搜索序显性+ 环检测输出环 + 包含图 + 缓存裁定） | VE 册 #VE-F0414 |

pub mod vea01_arbitrate;
pub mod vea01_engine;
pub mod vea01_index;
pub mod vea01_probe;
pub mod vea01_virtfeat;
pub mod vea02_ctx;
pub mod vea03_checks;
pub mod vea03_sync;
pub mod vea04_checks;
pub mod vea04_ring;
pub mod vea05_budget;
pub mod vea05_checks;
pub mod vea06_checks;
pub mod vea06_qsched;
pub mod vea07_caps;
pub mod vea07_checks;
pub mod vea08_checks;
pub mod vea08_handle;
pub mod vea09_checks;
pub mod vea09_recovery;
pub mod vea10_bus;
pub mod vea10_checks;
pub mod vea11_checks;
pub mod vea11_hotplug;
pub mod vea12_checks;
pub mod vea12_probe;
pub mod vea13_checks;
pub mod vea13_softfall;
pub mod vea14_checks;
pub mod vea14_snapshot;
pub mod vea15_checks;
pub mod vea15_errclass;
pub mod vea18_checks;
pub mod vea18_sampler;
pub mod veb01_checks;
pub mod veb01_device;
pub mod veb01_init;
pub mod veb01_report;
pub mod veb02_checks;
pub mod veb02_proto;
pub mod veb02_queue;
pub mod veb03_checks;
pub mod veb03_resource;
pub mod veb04_2dupdate;
pub mod veb04_checks;
pub mod veb05_checks;
pub mod veb05_virgl;
pub mod veb06_checks;
pub mod veb06_stream;
pub mod veb10_checks;
pub mod veb10_cursor;
pub mod vec01_checks;
pub mod vec01_constitution;
pub mod vec02_checks;
pub mod vec02_spec;
pub mod vec03_checks;
pub mod vec03_lexer;
pub mod vec04_checks;
pub mod vec04_keywords;
pub mod vec05_checks;
pub mod vec05_ident;
pub mod vec06_checks;
pub mod vec06_lit;
pub mod vec07_checks;
pub mod vec07_string;
pub mod vec08_checks;
pub mod vec08_comment;
pub mod vec09_checks;
pub mod vec09_operator;
pub mod vec10_brace;
pub mod vec10_checks;
pub mod vec11_checks;
pub mod vec11_prepro;
pub mod vec12_checks;
pub mod vec12_macro;
pub mod vec13_checks;
pub mod vec13_cond;
pub mod vec14_checks;
pub mod vec14_include;
pub mod ved01_checks;
pub mod ved01_tree;
pub mod ved02_checks;
pub mod ved02_xform;
pub mod ved03_alpha;
pub mod ved03_checks;
pub mod ved04_checks;
pub mod ved04_clip;
pub mod ved05_checks;
pub mod ved05_isolation;
pub mod ved06_checks;
pub mod ved06_zorder;
pub mod ved07_checks;
pub mod ved07_visibility;
pub mod ved13_dirty;
pub mod ved14_traverse;
pub mod vee01_arch;
pub mod vee01_checks;
pub mod vee02_checks;
pub mod vee02_utf8;
pub mod vee03_checks;
pub mod vee03_outline;
pub mod vef01_checks;
pub mod vef01_pngdec;
pub mod veh01_boundary;
pub mod veh01_checks;
pub mod veh02_audioarch;
pub mod veh02_checks;
pub mod veh02_service;
pub mod vej04_checks;
pub mod vej04_pointlight;
pub mod vek04_bloom;
pub mod vek04_checks;
pub mod vem02_checks;
pub mod vem02_track;
pub mod vem03_checks;
pub mod vem03_interp;
pub mod vel03_checks;
pub mod vel03_emitter;
pub mod vel04_checks;
pub mod vel04_mode;
pub mod veo01_arch;
pub mod veo01_checks;
pub mod vep01_arch;
pub mod vep01_checks;
pub mod veq01_checks;
pub mod veq01_pipeline;
pub mod veq02_checks;
pub mod veq02_graph;
pub mod ver01_arch;
pub mod ver01_checks;
pub mod ver02_arch;
pub mod ver02_checks;
pub mod ves01_sdomain_arch;
pub mod vet01_a11y_render_pipeline;
pub mod vev01_arch;
pub mod vev01_checks;
pub mod vew01_sdk_arch;
pub mod vew02_manifest;

pub use vea01_index::{ProbeReport, effective_renderer, run_a01};

use crate::checks::CheckSet;

/// 域标识（CheckSet 聚合用）。
pub const VEA_DOMAIN: &str = "svstar2-ve";

/// 本域自检聚合：逐项 `run_*_checks` 汇总。
///
/// CheckSet 容量上限见 `crate::checks::MAX_CHECKS`；单模块超限由该模块
/// 自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_svstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(VEA_DOMAIN);
    // (标签, 子集) —— 逐项加行，施工一项加一项
    let blocks: [(&'static str, CheckSet); 57] = [
        ("VE-F0001", vea01_index::run_vea01_checks()),
        ("VE-F0002", vea02_ctx::run_vea02_checks()),
        ("VE-F0003", vea03_checks::run_vea03_checks()),
        ("VE-F0004", vea04_checks::run_vea04_checks()),
        ("VE-F0005", vea05_checks::run_vea05_checks()),
        ("VE-F0006", vea06_checks::run_vea06_checks()),
        ("VE-F0007", vea07_checks::run_vea07_checks()),
        ("VE-F0008", vea08_checks::run_vea08_checks()),
        ("VE-F0009", vea09_recovery::run_vea09_checks()),
        ("VE-F0010", vea10_bus::run_vea10_checks()),
        ("VE-F0011", vea11_hotplug::run_vea11_checks()),
        ("VE-F0012", vea12_probe::run_vea12_checks()),
        ("VE-F0013", vea13_softfall::run_vea13_checks()),
        ("VE-F0014", vea14_snapshot::run_vea14_checks()),
        ("VE-F0015", vea15_errclass::run_vea15_checks()),
        ("VE-F0018", vea18_checks::run_vea18_checks()),
        ("VE-F0201", veb01_checks::run_veb01_checks()),
        ("VE-F0202", veb02_checks::run_veb02_checks()),
        ("VE-F0203", veb03_checks::run_veb03_checks()),
        ("VE-F0204", veb04_checks::run_veb04_checks()),
        ("VE-F0210", veb10_cursor::run_veb10_checks()),
        ("VE-F0401", vec01_checks::run_vec01_checks()),
        ("VE-F0402", vec02_spec::run_vec02_checks()),
        ("VE-F0403", vec03_lexer::run_vec03_checks()),
        ("VE-F0404", vec04_keywords::run_vec04_checks()),
        ("VE-F0405", vec05_checks::run_vec05_checks()),
        ("VE-F0406", vec06_checks::run_vec06_checks()),
        ("VE-F0407", vec07_string::run_vec07_checks()),
        ("VE-F0408", vec08_comment::run_vec08_checks()),
        ("VE-F0409", vec09_operator::run_vec09_checks()),
        ("VE-F0410", vec10_brace::run_vec10_checks()),
        ("VE-F0411", vec11_prepro::run_vec11_checks()),
        ("VE-F0412", vec12_macro::run_vec12_checks()),
        ("VE-F0414", vec14_include::run_vec14_checks()),
        ("VE-F0601", ved01_checks::run_ved01_checks()),
        ("VE-F0602", ved02_checks::run_ved02_checks()),
        ("VE-F0603", ved03_checks::run_ved03_checks()),
        ("VE-F0613", ved13_dirty::run_ved13_checks()),
        ("VE-F4601", vew01_sdk_arch::run_vew01_checks()),
        ("VE-F4602", vew02_manifest::run_vew02_checks()),
        ("VE-F0614", ved14_traverse::run_ved14_checks()),
        ("VE-F0801", vee01_checks::run_vee01_checks()),
        ("VE-F0802", vee02_checks::run_vee02_checks()),
        ("VE-F0803", vee03_checks::run_vee03_checks()),
        ("VE-F2004", vek04_checks::run_vek04_checks()),
        ("VE-F2402", vem02_checks::run_vem02_checks()),
        ("VE-F2403", vem03_checks::run_vem03_checks()),
        ("VE-F2203", vel03_checks::run_vel03_all_checks()),
        ("VE-F2204", vel04_checks::run_vel04_all_checks()),
        ("VE-F3401", ver01_arch::run_ver01_checks()),
        ("VE-F3601", ver02_arch::run_ver02_checks()),
        ("VE-F3801", ves01_sdomain_arch::run_f3801_checks()),
        ("VE-F3802", vet01_a11y_render_pipeline::run_f3802_checks()),
        ("VE-F4401", vev01_checks::run_vev01_checks()),
        ("VE-F3201", veq01_pipeline::run_veq01_checks()),
        ("VE-F3202", veq02_graph::run_veq02_checks()),
        ("VE-F3001", vep01_checks::run_vep01_checks()),
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            tag,
            passed,
            if passed { "" } else { "sub-checks red" },
        );
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svstar2_domain_aggregate_all_green() {
        let set = run_svstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "svstar2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 落位纪律自检：VE 册功能必须在 Rust 侧，不许混进前端 TS。
    /// 这条断言是"位置写错"的最后一道闸——防止后人又把它写回 `src/`。
    #[test]
    fn vea_lives_in_kernel_not_frontend() {
        // VE-F0001 的类型定义在本模块树内可见，且由内核 crate 编译
        let probe = vea01_probe::AdapterProbeInput::physical(
            "0000:01:00.0",
            "NVIDIA GeForce RTX 4060",
            "551.23",
        );
        let r = vea01_engine::probe_all(&[probe]);
        assert_eq!(r.len(), 1, "VE-F0001 由内核 Rust 侧实现并可执行");
    }
}
