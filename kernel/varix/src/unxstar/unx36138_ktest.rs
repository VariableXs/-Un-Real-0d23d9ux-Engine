//! UNX-F36138-J1 B07 ktest 断言集（类型注册面全判据注册）。
//! 判据：① B07 十六模块（36121~36136）的 run_*_checks 全数接入本账，
//! 每模块一条聚合断言（子账全绿 ⇔ 聚合绿）；② 跨模块交叉断言五条
//! （闸门×装配 / 绑定×对账 / 档位×联签 / 指纹三账同源 / 启动链×剧本）；
//! ③ 断言失败逐模块显性归因；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：ktest 真源——「编译绿 ≠ 判据绿」（坑 8）的正面落点：本模块
//! 是 B07 判据的注册表，任何 B07 模块的 run_*_checks 没进本账，它的判据
//! 就永不执行。聚合方式 = 逐模块调子账 + all_passed 折叠，交叉断言则
//! 独立构造跨模块场景（不与 36137 剧本重复——剧本证语义链，这里证
//! 判据注册完整性本身）。

use crate::checks::CheckSet;
use super::unx36121_typereg::{self as tr, GOLDEN_N, TID_BASE};
use super::unx36122_typedeny::{access_gate, GateRoute};
use super::unx36123_genbind::{assemble_golden_paired, face_audit};
use super::unx36125_deftier::{synthesize_verified, tier_signature};
use super::unx36126_obcreate::Engine;
use super::unx36129_persist::name_fp;

pub const SEG_BASE: u16 = 0x8200;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_KTEST: u16 = SEG_BASE + 2; // ktest 聚合红
pub const E_X: u16 = SEG_BASE + 3; // 交叉断言红
pub const E_COVER: u16 = SEG_BASE + 4; // 判据覆盖缺口（模块数/段位不符）
pub const E_INTG: u16 = SEG_BASE + 5; // 矩阵/聚合两面矛盾

/// 模块版本锚。
pub const VER: u32 = 1;

/// B07 判据注册面（16 模块短名——与文件名/卡号三对齐，坑 29 纪律）。
pub const KTEST_MODULES: [&'static str; 16] = [
    "unx36121_typereg",
    "unx36122_typedeny",
    "unx36123_genbind",
    "unx36124_enroll",
    "unx36125_deftier",
    "unx36126_obcreate",
    "unx36127_nounreg",
    "unx36128_typever",
    "unx36129_persist",
    "unx36130_typaudit",
    "unx36131_selftest",
    "unx36132_typfuzz",
    "unx36133_typperf",
    "unx36134_nsrecon",
    "unx36135_wizard",
    "unx36136_eventlog",
];

/// 子账聚合（16 模块判据全数执行，折叠为总绿 + 红项归因文本）。
pub fn ktest_aggregate() -> (bool, usize, usize) {
    let mut total = 0usize;
    let mut red = 0usize;
    // 逐模块执行子账（直接调用短名判据函数——注册表本身就是调用序）。
    let sets: [CheckSet; 16] = [
        super::unx36121_typereg::run_unx36121_checks(),
        super::unx36122_typedeny::run_unx36122_checks(),
        super::unx36123_genbind::run_unx36123_checks(),
        super::unx36124_enroll::run_unx36124_checks(),
        super::unx36125_deftier::run_unx36125_checks(),
        super::unx36126_obcreate::run_unx36126_checks(),
        super::unx36127_nounreg::run_unx36127_checks(),
        super::unx36128_typever::run_unx36128_checks(),
        super::unx36129_persist::run_unx36129_checks(),
        super::unx36130_typaudit::run_unx36130_checks(),
        super::unx36131_selftest::run_unx36131_checks(),
        super::unx36132_typfuzz::run_unx36132_checks(),
        super::unx36133_typperf::run_unx36133_checks(),
        super::unx36134_nsrecon::run_unx36134_checks(),
        super::unx36135_wizard::run_unx36135_checks(),
        super::unx36136_eventlog::run_unx36136_checks(),
    ];
    let mut i = 0;
    while i < 16 {
        total += sets[i].len();
        if !sets[i].all_passed() {
            red += 1;
        }
        i += 1;
    }
    (red == 0, total, red)
}

/// 交叉断言一：闸门 × 装配（黄金表装配成功 ⇔ 八类全 Routed）。
pub fn x_gate_vs_assemble() -> Result<(), u16> {
    let reg = tr::assemble_golden()?;
    let mut g = 0;
    while g < GOLDEN_N {
        if access_gate(&reg, tr::GOLDEN_NAMES[g]) != GateRoute::Routed {
            return Err(E_X);
        }
        g += 1;
    }
    Ok(())
}

/// 交叉断言二：绑定 × 对账（双面装配成功 ⇔ face_audit 绿）。
pub fn x_bind_vs_audit() -> Result<(), u16> {
    let (reg, maps) = assemble_golden_paired()?;
    face_audit(&reg, &maps).map_err(|_| E_X)
}

/// 交叉断言三：档位 × 联签（档位特征面 ⇔ 引擎联签控制位一致）。
pub fn x_tier_vs_obcreate() -> Result<(), u16> {
    let mut eng = Engine::assemble()?;
    let out = eng.ob_create("Token", 0x0001).map_err(|_| E_X)?;
    let (must, must_not) = tier_signature(out.policy);
    if out.ctrl & must != must || out.ctrl & must_not != 0 {
        return Err(E_X);
    }
    // 与独立合成面复核一致性。
    let (sd, _) = synthesize_verified(out.policy).map_err(|_| E_X)?;
    if sd.control != out.ctrl {
        return Err(E_X);
    }
    Ok(())
}

/// 交叉断言四：指纹三账同源（持久化/审计/遥测三面的 FNV-16 出口同值）。
pub fn x_fp_tripbook() -> Result<(), u16> {
    let a = name_fp("File");
    let b = super::unx36130_typaudit::name_fp_file_anchor();
    let c = super::unx36136_eventlog::name_fp_log_anchor();
    if a != b || b != c {
        return Err(E_X);
    }
    if a == 0 {
        return Err(E_X); // 非退化
    }
    Ok(())
}

/// 交叉断言五：tid 编号三面一致（类型表 / 联签账逐位同源）。
pub fn x_tid_triface() -> Result<(), u16> {
    let mut eng = Engine::assemble()?;
    // 黄金装配 + 逐类联签：tid 必须逐位一致。
    let mut g = 0;
    while g < GOLDEN_N {
        let out = eng.ob_create(tr::GOLDEN_NAMES[g], 0x00A0 + g as u16).map_err(|_| E_X)?;
        if out.tid != TID_BASE + g as u16 {
            return Err(E_X);
        }
        let e = eng.reg.at(TID_BASE + g as u16).map_err(|_| E_X)?;
        if e.name != tr::GOLDEN_NAMES[g] {
            return Err(E_X);
        }
        g += 1;
    }
    Ok(())
}

/// 逐模块判定矩阵（ktest 的可观察输出——收口账 36140 与任务板写回消费）。
/// 每行 = 模块序号 + 判据数 + 通过数 + 全绿位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KtestRow {
    pub mod_idx: u16,   // 36121..36136
    pub checks: usize,  // 子账判据数
    pub passed: usize,  // 通过数
    pub green: bool,    // 全绿位
}

/// 全矩阵（16 行；红项模块显性定位——不再靠二分重跑，坑 4 的正面落点）。
pub fn ktest_matrix() -> [KtestRow; 16] {
    let sets: [CheckSet; 16] = [
        super::unx36121_typereg::run_unx36121_checks(),
        super::unx36122_typedeny::run_unx36122_checks(),
        super::unx36123_genbind::run_unx36123_checks(),
        super::unx36124_enroll::run_unx36124_checks(),
        super::unx36125_deftier::run_unx36125_checks(),
        super::unx36126_obcreate::run_unx36126_checks(),
        super::unx36127_nounreg::run_unx36127_checks(),
        super::unx36128_typever::run_unx36128_checks(),
        super::unx36129_persist::run_unx36129_checks(),
        super::unx36130_typaudit::run_unx36130_checks(),
        super::unx36131_selftest::run_unx36131_checks(),
        super::unx36132_typfuzz::run_unx36132_checks(),
        super::unx36133_typperf::run_unx36133_checks(),
        super::unx36134_nsrecon::run_unx36134_checks(),
        super::unx36135_wizard::run_unx36135_checks(),
        super::unx36136_eventlog::run_unx36136_checks(),
    ];
    let mut rows = [KtestRow { mod_idx: 0, checks: 0, passed: 0, green: false }; 16];
    let mut i = 0;
    while i < 16 {
        let cs = &sets[i];
        rows[i] = KtestRow {
            mod_idx: (36121 + i) as u16,
            checks: cs.len(),
            passed: cs.passed_count(),
            green: cs.all_passed(),
        };
        i += 1;
    }
    rows
}

/// 红项归因文本（失败模块的 red_items_text 串接——诊断面单源）。
pub fn red_attribution() -> alloc::string::String {
    let rows = ktest_matrix();
    let mut out = alloc::string::String::new();
    let mut i = 0;
    while i < 16 {
        if !rows[i].green {
            out.push_str(KTEST_MODULES[i]);
            out.push_str(" | ");
        }
        i += 1;
    }
    out
}

/// 矩阵守恒：16 行齐 + 判据总数与聚合面一致 + 全绿 ⇔ 聚合绿。
pub fn matrix_conserves(rows: &[KtestRow; 16]) -> Result<(), u16> {
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut all_green = true;
    let mut i = 0;
    while i < 16 {
        if rows[i].mod_idx != (36121 + i) as u16 {
            return Err(E_COVER); // 行序漂移
        }
        if rows[i].passed > rows[i].checks {
            return Err(E_INTG); // 通过数 > 判据数
        }
        total += rows[i].checks;
        passed += rows[i].passed;
        if !rows[i].green {
            all_green = false;
        }
        i += 1;
    }
    let (green, agg_total, _) = ktest_aggregate();
    if total != agg_total {
        return Err(E_INTG); // 两面判据总数不一致
    }
    if all_green != green {
        return Err(E_INTG); // 矩阵与聚合结论矛盾
    }
    if passed != total && green {
        return Err(E_INTG); // 绿但有不通过——矛盾
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：聚合全绿 + 覆盖数核对（16 模块 × 每模块判据数 ≥ 5）。
    let (green, total, red) = ktest_aggregate();
    if !green || red != 0 {
        return Err(E_KTEST);
    }
    if total < 16 * 5 {
        return Err(E_COVER);
    }
    // R2：注册面名单对齐（16 短名与卡号段位对齐——36121..36136 连续）。
    let mut i = 0;
    while i < 16 {
        let expect = 36121 + i;
        let mut buf = [0u8; 8];
        buf[0] = b'u';
        buf[1] = b'n';
        buf[2] = b'x';
        // 短名前缀 unx36NNN 与 KTEST_MODULES 逐位核（防手抄漂移）。
        let m = KTEST_MODULES[i];
        let mb = m.as_bytes();
        if mb.len() < 10 {
            return Err(E_COVER);
        }
        // unx 后 5 位 = 卡号。
        let card = (mb[3] - b'0') as u32 * 10000
            + (mb[4] - b'0') as u32 * 1000
            + (mb[5] - b'0') as u32 * 100
            + (mb[6] - b'0') as u32 * 10
            + (mb[7] - b'0') as u32;
        if card != expect as u32 {
            return Err(E_COVER);
        }
        let _ = &mut buf;
        i += 1;
    }
    // R3：五条交叉断言全绿。
    x_gate_vs_assemble()?;
    x_bind_vs_audit()?;
    x_tier_vs_obcreate()?;
    x_fp_tripbook()?;
    x_tid_triface()?;
    // R4：判定矩阵守恒（行序/两面总数/结论一致性）。
    let rows = ktest_matrix();
    matrix_conserves(&rows)?;
    // 矩阵全绿且每行判据数 ≥ 5（覆盖下限的矩阵形态复核）。
    let mut i2 = 0;
    while i2 < 16 {
        if !rows[i2].green || rows[i2].checks < 5 {
            return Err(E_COVER);
        }
        i2 += 1;
    }
    // 红项归因空串 ⇔ 全绿（诊断面单源自洽）。
    if red_attribution().is_empty() != rows.iter().all(|r| r.green) {
        return Err(E_INTG);
    }
    // 校验和：判据总数 × 模块数。
    Ok((total as u32).wrapping_mul(16).wrapping_add(red as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_KTEST => "ktest 聚合红（某模块子账未全绿）",
        E_X => "交叉断言红（跨模块语义串线）",
        E_COVER => "判据覆盖缺口（模块名单/卡号对齐破）",
        E_INTG => "矩阵/聚合两面矛盾（行序/总数/结论不一致）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36138_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36138_ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0x8200, "0x8200~0x820F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "16 模块聚合/五交叉断言/名单对齐全绿"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("注册面名单", KTEST_MODULES.len() == 16 && KTEST_MODULES[0] == "unx36121_typereg" && KTEST_MODULES[15] == "unx36136_eventlog", "16 模块短名闭集");
    cs.add("交叉断言闭集", x_gate_vs_assemble().is_ok() && x_bind_vs_audit().is_ok(), "闸门×装配 / 绑定×对账 两轴锚");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_probes_green() {
        // 交叉断言五探针全绿：闸门x装配 / 绑定x对账 / 档位x创建 /
        // 指纹三账 / tid 三面。
        assert!(x_gate_vs_assemble().is_ok());
        assert!(x_bind_vs_audit().is_ok());
        assert!(x_tier_vs_obcreate().is_ok());
        assert!(x_fp_tripbook().is_ok());
        assert!(x_tid_triface().is_ok());
    }

    #[test]
    fn matrix_row_conserves() {
        // ktest 矩阵守恒：16 模块全覆盖，行账守恒核全绿。
        let rows = ktest_matrix();
        assert_eq!(rows.len(), 16);
        assert!(matrix_conserves(&rows).is_ok());
        let (ok, total, red) = ktest_aggregate();
        assert!(ok);
        assert!(total >= KTEST_MODULES.len()); // 16 模块判据全数执行
        assert_eq!(red, 0); // 零红面
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(1808));
    }
}
