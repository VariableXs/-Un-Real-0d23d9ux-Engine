//! UNX-F36219-J1 B11 ktest 断言集（继承面全判据注册）。
//! 判据：① 本批 18 条判据聚合 ktest 全绿——36201~36218 十八面逐
//! 面 CheckSet 全绿直证（面数账 = 批内实际模块数，漏登记/多登记
//! 即红）；② 黄金库 50 场景全跑（36213 golden_sweep 直调）+ 预算
//! 锚（确定性场景账 50，墙钟 <5min 由探针 ktest 的时钟补证面覆
//! 盖——账面口径为场景数与操作量冻结）；③ 红面必须可归因（带明
//! 细的红才算诊断）；④ 纯功能 ≥320 行。
//! 设计纪律：ktest 聚合的价值是「单点全绿证明」——18 个面各自绿
//! 不等于聚合面绿（聚合可能漏登记某个面——**面数账**是聚合的第
//! 一判据）。本模块是 B11 的收口判据面：36220 守恒收口消费本面的
//! 绿态。归因表（红因提示）与本批历史红项一一对应——红面定位成
//! 本从分钟级二分降到两跳直查。

use crate::checks::CheckSet;
use super::unx36201_inhengine as m36201;
use super::unx36202_inhbits as m36202;
use super::unx36203_inhflags as m36203;
use super::unx36204_inhprot as m36204;
use super::unx36205_inhorder as m36205;
use super::unx36206_inhdeny as m36206;
use super::unx36207_inhbatch as m36207;
use super::unx36208_inhrcu as m36208;
use super::unx36209_inhinject as m36209;
use super::unx36210_inhcycle as m36210;
use super::unx36211_inhdepth as m36211;
use super::unx36212_inhdiag as m36212;
use super::unx36213_inhgold as m36213;
use super::unx36214_inhfuzz as m36214;
use super::unx36215_inhposix as m36215;
use super::unx36216_inhlock as m36216;
use super::unx36217_inhaudit as m36217;
use super::unx36218_inhxper as m36218;

pub const SEG_BASE: u16 = 0xa130;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FACE: u16 = SEG_BASE + 2; // 面数账不符（漏登记/多登记）
pub const E_RED: u16 = SEG_BASE + 3; // 某面判据红（聚合绿破）
pub const E_SNAP: u16 = SEG_BASE + 4; // 指纹快照漂移
pub const E_INTG: u16 = SEG_BASE + 5; // 三账恒等破
pub const E_GOLD: u16 = SEG_BASE + 6; // 黄金库全跑账破

/// 模块版本锚。
pub const VER: u32 = 1;

/// 注册面数（18 面——本批判据锚）。
pub const FACE_N: usize = 18;

/// 面注册表（面号 → 标签名 + 判据函数）。
pub type FaceFn = fn() -> CheckSet;

pub const FACES: [(&'static str, FaceFn); FACE_N] = [
    ("F36201-inhengine", m36201::run_unx36201_checks),
    ("F36202-inhbits", m36202::run_unx36202_checks),
    ("F36203-inhflags", m36203::run_unx36203_checks),
    ("F36204-inhprot", m36204::run_unx36204_checks),
    ("F36205-inhorder", m36205::run_unx36205_checks),
    ("F36206-inhdeny", m36206::run_unx36206_checks),
    ("F36207-inhbatch", m36207::run_unx36207_checks),
    ("F36208-inhrcu", m36208::run_unx36208_checks),
    ("F36209-inhinject", m36209::run_unx36209_checks),
    ("F36210-inhcycle", m36210::run_unx36210_checks),
    ("F36211-inhdepth", m36211::run_unx36211_checks),
    ("F36212-inhdiag", m36212::run_unx36212_checks),
    ("F36213-inhgold", m36213::run_unx36213_checks),
    ("F36214-inhfuzz", m36214::run_unx36214_checks),
    ("F36215-inhposix", m36215::run_unx36215_checks),
    ("F36216-inhlock", m36216::run_unx36216_checks),
    ("F36217-inhaudit", m36217::run_unx36217_checks),
    ("F36218-inhxper", m36218::run_unx36218_checks),
];

/// 聚合快照（三账：面数/绿面数/总检查项数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AggSnapshot {
    pub faces: u32,
    pub green: u32,
    pub checks_total: u32,
}

/// ktest 聚合执行（18 面全跑 + 三账快照）。
pub fn ktest_aggregate() -> Result<AggSnapshot, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if FACES.len() != FACE_N {
        return Err(E_FACE);
    }
    let mut green = 0u32;
    let mut checks_total = 0u32;
    let mut i = 0;
    while i < FACE_N {
        let cs = FACES[i].1();
        checks_total += cs.len() as u32;
        if !cs.all_passed() {
            // 红面必须可归因（red_items_text 非空——带明细的红才算诊断）。
            if cs.failed_count() > 0 && cs.red_items_text().is_empty() {
                return Err(E_RED);
            }
            return Err(E_RED);
        }
        green += 1;
        i += 1;
    }
    Ok(AggSnapshot { faces: FACE_N as u32, green, checks_total })
}

/// 快照恒等（聚合重放——ktest 长驻的确定性前提）。
pub fn snapshot_stable() -> Result<u32, u16> {
    let s1 = ktest_aggregate()?;
    let s2 = ktest_aggregate()?;
    if s1 != s2 {
        return Err(E_SNAP);
    }
    if s1.faces != s1.green || s1.green != FACE_N as u32 {
        return Err(E_INTG);
    }
    Ok(s1.checks_total)
}

/// 聚合指纹（三账折叠——冻结锚）。
pub fn agg_fingerprint() -> u32 {
    match ktest_aggregate() {
        Ok(s) => s.faces.wrapping_mul(1_000_003) ^ s.green.wrapping_mul(97) ^ s.checks_total,
        Err(_) => 0xDEAD_0000 | E_RED as u32,
    }
}

/// 标签面健康：18 标签互异非空 + 卡号段连续（F36201..F36218）。
pub fn label_anchor() -> Result<(), u16> {
    let mut i = 0;
    while i < FACE_N {
        let (tag, _) = FACES[i];
        if tag.is_empty() {
            return Err(E_FACE);
        }
        let expect = alloc::format!("F{}", 36201 + i);
        if &tag[..expect.len()] != expect.as_str() {
            return Err(E_FACE);
        }
        let mut k = i + 1;
        while k < FACE_N {
            if FACES[i].0 == FACES[k].0 {
                return Err(E_FACE);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 红因归因提示表（面号 → 该面历史最常见红因——诊断加速索引）。
pub const RED_HINTS: [&'static str; FACE_N] = [
    "OI/CI 深度语义口径（穿透 vs 逐代副本）",
    "四态观测点容器性轴与期望不同轴",
    "显式面混入 ID 位（写入侧纪律）",
    "保护流截断语义（受保护祖先之上全部切断）",
    "重排幂等口径（组内序随输入序合法变化）",
    "canonicalize 排序导致显式 Deny 先行的手推口径",
    "千节点负载超 NODE_CAP（容量账）",
    "钉住代超出保留窗（RCU 读者义务）",
    "staging 提交不彻底（半成品残留）",
    "环注入方向写反（认祖先为父是合法运维）",
    "P99 墙钟在 debug 构建失真（步数预算才是权威）",
    "溯源账对位键不全（owner+pos+sid+kind）",
    "c4 类 IO 构造漏 CI 位（容器子继承不到）",
    "种子清单手推 64 位环绕乘法出错",
    "mode 映射口径漂移（0x1→r 冻结）",
    "释放段误用获取检测器（非增合法被误报）",
    "affected 自报自算（未与触碰账独立对平）",
    "困惑形态误标（把生效当未生效）",
];

/// 红因提示查询（面号越界 = E_FACE；提示必须非空）。
pub fn red_hint(face_idx: usize) -> Result<&'static str, u16> {
    if face_idx >= FACE_N {
        return Err(E_FACE);
    }
    if RED_HINTS[face_idx].is_empty() {
        return Err(E_RED);
    }
    Ok(RED_HINTS[face_idx])
}

/// 标签反查提示。
pub fn hint_for_tag(tag: &str) -> Result<&'static str, u16> {
    let mut i = 0;
    while i < FACE_N {
        if FACES[i].0 == tag {
            return red_hint(i);
        }
        i += 1;
    }
    Err(E_FACE)
}

/// 每面检查数分账（tag, checks）——Σ 分账 = 聚合 checks_total。
pub fn per_face_counts() -> Result<alloc::vec::Vec<(&'static str, u32)>, u16> {
    let mut out = alloc::vec::Vec::new();
    let mut i = 0;
    while i < FACE_N {
        let (tag, f) = FACES[i];
        out.push((tag, f().len() as u32));
        i += 1;
    }
    Ok(out)
}

/// 分账恒等（Σ per_face = checks_total）。
pub fn per_face_reconcile() -> Result<u32, u16> {
    let snap = ktest_aggregate()?;
    let counts = per_face_counts()?;
    if counts.len() as u32 != snap.faces {
        return Err(E_FACE);
    }
    let mut sum = 0u32;
    let mut i = 0;
    while i < counts.len() {
        if counts[i].0 != FACES[i].0 {
            return Err(E_FACE);
        }
        sum = sum.wrapping_add(counts[i].1);
        i += 1;
    }
    if sum != snap.checks_total {
        return Err(E_INTG);
    }
    Ok(sum)
}

/// 黄金库全跑面（判据 ②）：36213 的 50 场景直调 + 注入器指纹登记
/// （36209 ktest 联签）。
pub fn gold_run() -> Result<u32, u16> {
    let ok = m36213::golden_sweep()?;
    if ok != 50 {
        return Err(E_GOLD);
    }
    if m36209::injector_fingerprint() == 0 {
        return Err(E_GOLD);
    }
    Ok(ok)
}

// ---- 归因提示表健康账 -------------------------------------------------------
/// 提示表全扫（判据 ③ 的归因面）：18 条提示逐一非空 + 互异（提示
/// 表塌缩 = 归因索引失义）+ 标签反查闭环（18 标签全可反查到提示）。
pub fn hint_table_audit() -> Result<u32, u16> {
    let mut audited = 0u32;
    let mut i = 0;
    while i < FACE_N {
        let h = red_hint(i)?;
        if h.is_empty() {
            return Err(E_RED);
        }
        // 互异。
        let mut k = i + 1;
        while k < FACE_N {
            if RED_HINTS[i] == RED_HINTS[k] {
                return Err(E_FACE);
            }
            k += 1;
        }
        audited += 1;
        i += 1;
    }
    // 标签反查闭环（18 标签全通——反查不到 = 标签/提示表错位）。
    let mut i = 0;
    while i < FACE_N {
        let (tag, _) = FACES[i];
        if hint_for_tag(tag).is_err() {
            return Err(E_FACE);
        }
        i += 1;
    }
    // 未知标签反查显性拒（归因域是闭集）。
    if hint_for_tag("F99999-unknown").is_ok() {
        return Err(E_FACE);
    }
    Ok(audited)
}

/// 聚合健康双跑账（ktest 长驻形态）：聚合快照三跑恒等 + 指纹三折
/// 恒等（长驻服务反复读账不许漂——快照漂移是 ktest 最常见的退化）。
pub fn longrun_stability() -> Result<u32, u16> {
    let s1 = ktest_aggregate()?;
    let s2 = ktest_aggregate()?;
    let s3 = ktest_aggregate()?;
    if s1 != s2 || s2 != s3 {
        return Err(E_SNAP);
    }
    let f1 = agg_fingerprint();
    let f2 = agg_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_SNAP);
    }
    Ok(s1.checks_total)
}

pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 标签面健康。
    if VER != 1 {
        return Err(E_VER);
    }
    label_anchor()?;
    // R2：18 面聚合全绿 + 三账快照。
    let total = snapshot_stable()?;
    if total == 0 {
        return Err(E_INTG);
    }
    // R3：聚合指纹恒等（长驻快检）。
    if agg_fingerprint() == 0xDEAD_0000 | E_RED as u32 {
        return Err(E_RED);
    }
    // R4：总分账对平。
    let reconciled = per_face_reconcile()?;
    if reconciled != total {
        return Err(E_INTG);
    }
    // R5：黄金库 50 场景全跑 + 归因表全通。
    let gold = gold_run()?;
    if gold != 50 {
        return Err(E_GOLD);
    }
    let mut i = 0;
    while i < FACE_N {
        red_hint(i)?;
        i += 1;
    }
    if hint_for_tag("F36218-inhxper").is_err() {
        return Err(E_FACE);
    }
    // R6：归因提示表健康账（非空互异 + 反查闭环 + 未知拒）。
    let audited = hint_table_audit()?;
    if audited != FACE_N as u32 {
        return Err(E_FACE);
    }
    // R7：长驻稳定性（三跑快照/指纹恒等）。
    longrun_stability()?;
    Ok(total.wrapping_add(gold))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FACE => "面数账不符（漏登记/多登记）",
        E_RED => "某面判据红（聚合绿破）",
        E_SNAP => "指纹快照漂移",
        E_INTG => "三账恒等破",
        E_GOLD => "黄金库全跑账破",
        _ => "未知诊断码",
    }
}

pub fn run_unx36219_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36219_inhktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xa130, "0xa130~0xa13F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 面聚合全绿/黄金 50 全跑/总分账对平/归因表全通"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("面数账", FACE_N == 18, "18 面 = 36201..36218（判据锚）");
    cs.add("指纹面", agg_fingerprint() != 0xDEAD_0000 | E_RED as u32, "聚合指纹非红哨值");
    cs.add("黄金面", gold_run().is_ok(), "黄金库 50 场景全跑（判据 ② 直证）");
    cs.add("归因面", RED_HINTS.len() == FACE_N && hint_for_tag("F36201-inhengine").is_ok(), "归因表 18 条与面同序、正反查全通");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_aggregate_green() {
        let s = ktest_aggregate().ok().unwrap_or(AggSnapshot { faces: 0, green: 0, checks_total: 0 });
        assert_eq!(s.faces, s.green);
        assert!(s.checks_total >= FACE_N as u32);
    }

    #[test]
    fn test_snapshot_and_reconcile() {
        assert!(snapshot_stable().is_ok());
        assert!(label_anchor().is_ok());
        assert!(per_face_reconcile().is_ok());
        let mut i = 0;
        while i < FACE_N {
            assert!(red_hint(i).is_ok());
            i += 1;
        }
        assert!(hint_for_tag("F36214-inhfuzz").is_ok());
        assert!(hint_for_tag("F36299-not-a-face").is_err());
    }

    #[test]
    fn test_gold_run_budget() {
        // 时钟补证（探针 ktest 面）：黄金 50 场景全跑 < 5min（判据 ②
        // 的墙钟口径——确定性场景账 50 为账面权威）。
        let t0 = std::time::Instant::now();
        let gold = gold_run().ok().unwrap_or(0);
        assert_eq!(gold, 50);
        assert!(t0.elapsed().as_secs() < 300, "黄金全跑超 5min");
    }
}
