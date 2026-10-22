//! VE-F1008 · 域自检外壳（判据逐条对应 `vef10_pngopts.rs` 头注）：只承载模块文档、
//! `for_each_cell` 共享遍历骨架与判据编排；判据实现拆至 `tables`（判据一/二/五）与
//! `options`（判据三/四与门禁）。`run_vef10_checks` 公开路径零变化；判据映射与
//! 弱门禁原为头注文本，本次整改原样迁入 `CRITERIA_MAP`/`WEAK_GATES`（零删减）。

#![allow(clippy::needless_range_loop)]

mod options;
mod tables;

use crate::checks::CheckSet;

use crate::svstar2::vef10_pngopts::{COLOR_TYPES, DEPTHS, LegalMatrix};

/// 锚点原文 → 自检项判据映射（自原头注迁入，逐条对应，不可增删）。
#[allow(dead_code)]
pub const CRITERIA_MAP: [(&str, &str); 5] = [
    ("选项全组合 roundtrip（合法性矩阵内全测）", "C1008-矩阵-*"),
    ("压缩比对比表", "C1008-权衡-*"),
    ("快速模式收益", "C1008-快速-*"),
    ("非法组合拒绝", "C1008-拒绝-*"),
    ("元数据部分成功 / 门禁", "C1008-元数据-* / C1008-门禁-*"),
];
/// 弱门禁自律（本域最易犯的五种，自原头注迁入；新增判据前先对照此表）。
#[allow(dead_code)]
pub const WEAK_GATES: [(&str, &str); 5] = [
    ("「矩阵全测」不能只测几个代表格", "只填两格的矩阵会全绿——故遍历全部 25 格逐格裁决，并断「格子总数 == 25」「合法+降档+非法 == 25」（三者相加，覆盖无空格）"),
    ("「压缩比对比表」不能只断「有 9 行」", "九行全同值的常数表能全绿——故断双向单调（级别↑ ⇒ 体积比不增 且 耗时比不减）加端点不等（首末档体积比必须不同）"),
    ("「快速模式 −40%」不能靠注释", "no_std 拿不到墙钟——断可机检结构事实：开快速 ⇒ 试探次数恒 1；关快速 ⇒ == 行数；且体积代价 > 0（「快速还更小」的实现在此变红）"),
    ("「拒绝给最近合法组合」不能靠「返回了某个建议」", "「永远返回第一合法格」能过弱判据——故断恰近：两非法格的建议曼哈顿距离逐个独立算出并相等，且为最小可达"),
    ("「部分成功」不能靠「有 dropped」", "整单失败也满足「有 dropped」——故要求 kept 与 dropped 同时非空，并断 kept + dropped == 提交数（恰等于，用 == 不用 >=）"),
];

// 判据锚点：判据一/二/五由 `tables` 承载，判据三/四与门禁由 `options` 承载；注入顺序即判据序。
/// 全表遍历骨架（消重核心）：对 5×5 矩阵按「颜色类型 × 位深」行主序逐格调用
/// `visit(ti, di)`。判据侧原有九处内联「双层 while + 逐格裁决」同构块（判重器
/// 控制流签名同构判红的主因），现统一收敛到本纯遍历骨架；访问顺序与拆分前
/// 完全一致，语义零变化；判据独立性不破——各判据只共用此 helper，互不调用。
fn for_each_cell(mut visit: impl FnMut(usize, usize)) {
    let mut ti = 0usize;
    while ti < COLOR_TYPES.len() {
        let mut di = 0usize;
        while di < DEPTHS.len() {
            visit(ti, di);
            di += 1;
        }
        ti += 1;
    }
}

/// F1008 域自检入口：各判据域提取为私有函数，按原顺序逐域注入判据（调用顺序即判据序）。
pub fn run_vef10_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-f-f1008");
    let m = LegalMatrix::new();

    tables::check_matrix(&m, &mut s);
    tables::check_level_table(&mut s);
    options::check_fast_mode(&mut s);
    options::check_rejection(&m, &mut s);
    tables::check_metadata(&m, &mut s);
    options::check_gates(&m, &mut s);

    s
}
