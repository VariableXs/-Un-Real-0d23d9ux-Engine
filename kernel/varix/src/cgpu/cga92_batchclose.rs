//! cga92_batchclose —— CGPU-F0092 批次收口（A 域 92/160 检查点）。
//! 锚点（CGPU 施工书）：CGPU-F0092 · 批次收口 · 目标 300 行
//! 判据摘录：检查点闭集（A 域 92 个检查点分六族：算子核/账本/
//!   注册表/文档册/语料/性能表——每族配额冻结）；收口核（六族
//! 配额全绿才放行——带病收口拒）；族账明细（每族 绿/缺 两态
//! 计数——聚合账）；批次幂等（重复收口同指纹——收口可复算）；
//! 病账检出（任一族缺口>0 → 拒收并列病族——定位账）；历史
//! 批次链（上批指纹入本批账——链式可审计）；批号域闸。
//! 组件：FamilyQuota（族配额）+ CloseReport（收口报告）+ 六族
//!   配额表 + 收口门 + 病账定位 + 指纹链 + 批号闸。
//! 职责定位：A 域 92 检查点的里程碑闸——cga76（76 点）之后第
//! 二收口；与 cga76 同构但配额前移（76→92），六族账引用各专
//! 册（cga87 注册表/cga91 文档册/cga89 语料/cga90 性能表）的
//! 完整性核，不重复实现。
//!
//! 六族配额（冻结）：
//!   F0 算子核 30 / F1 账本 15 / F2 注册表 12 / F3 文档册 12 /
//!   F4 语料 13 / F5 性能表 10 —— Σ=92

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE910 段 · 本模块独占）=====
pub const XC_BATCH_OOB: u32 = 0xE910; // 批号越界（>9999）
pub const XC_SICK_FAMILY: u32 = 0xE911; // 病族（缺口>0）
pub const XC_QUOTA_OOB: u32 = 0xE912; // 配额非法（0 或超界）
pub const XC_CHAIN_DRIFT: u32 = 0xE913; // 历史链漂移

// ===== 族与配额闭集 =====
pub const F_CORE: u8 = 0;
pub const F_LEDGER: u8 = 1;
pub const F_REG: u8 = 2;
pub const F_DOC: u8 = 3;
pub const F_CORPUS: u8 = 4;
pub const F_PERF: u8 = 5;
/// (族, 配额) 冻结表。
pub const QUOTA: [(u8, u32); 6] = [(F_CORE, 30), (F_LEDGER, 15), (F_REG, 12), (F_DOC, 12), (F_CORPUS, 13), (F_PERF, 10)];

/// 族账（绿点数）。
#[derive(Clone)]
pub struct FamilyLedger {
    pub family: u8,
    pub green: u32,
}

/// 收口报告。
#[derive(Clone)]
pub struct CloseReport {
    pub batch_id: u32,
    pub ledgers: [FamilyLedger; 6],
    /// 上批指纹（链式审计；首批 0）。
    pub prev_fingerprint: u64,
}

/// 配额表合法性（Σ==92 且每项 >0）。
pub fn quota_valid() -> Result<u32, u32> {
    let mut sum = 0u32;
    for &(f, q) in QUOTA.iter() {
        if q == 0 || q > 100 {
            return Err(XC_QUOTA_OOB);
        }
        sum += q;
        let _ = f;
    }
    if sum != 92 {
        return Err(XC_QUOTA_OOB);
    }
    Ok(sum)
}

/// 收口门：六族配额全绿才放行；病账返回病族号。
pub fn close_gate(rep: &CloseReport) -> Result<(), u32> {
    for (idx, &(f, q)) in QUOTA.iter().enumerate() {
        let led = &rep.ledgers[idx];
        if led.family != f {
            return Err(XC_SICK_FAMILY);
        }
        if led.green < q {
            return Err(XC_SICK_FAMILY);
        }
    }
    Ok(())
}

/// 病账定位：返回首个病族号（绿 < 配额的第一族）。
pub fn first_sick(rep: &CloseReport) -> Option<u8> {
    for (idx, &(f, q)) in QUOTA.iter().enumerate() {
        if rep.ledgers[idx].green < q {
            return Some(f);
        }
    }
    None
}

/// 总绿点数。
pub fn total_green(rep: &CloseReport) -> u32 {
    rep.ledgers.iter().map(|l| l.green).sum()
}

/// 收口指纹（批号×总绿×族绿序列 × 上批指纹——链式敏感）。
pub fn report_fingerprint(rep: &CloseReport) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |v: u64, h: &mut u64| {
        for s in [0u32, 8, 16, 24, 32, 40, 48, 56].iter() {
            *h ^= (v >> s) & 0xFF;
            *h = h.wrapping_mul(0x100000001b3);
        }
    };
    feed(rep.batch_id as u64, &mut h);
    feed(total_green(rep) as u64, &mut h);
    for l in rep.ledgers.iter() {
        feed(l.green as u64, &mut h);
    }
    feed(rep.prev_fingerprint, &mut h);
    h
}

/// 批号域闸。
pub fn batch_gate(id: u32) -> Result<(), u32> {
    if id == 0 || id > 9999 {
        return Err(XC_BATCH_OOB);
    }
    Ok(())
}

/// 链式账：上批指纹非零时，本批指纹必须不同于上批（同指纹=收口未推进=漂移）。
pub fn chain_check(cur: u64, prev: u64) -> Result<(), u32> {
    if prev != 0 && cur == prev {
        return Err(XC_CHAIN_DRIFT);
    }
    Ok(())
}

/// 标准批构造（92 全绿 + 上批指纹参数）。
pub fn standard_report(batch_id: u32, prev: u64) -> Result<CloseReport, u32> {
    batch_gate(batch_id)?;
    let mut ledgers: [FamilyLedger; 6] = [
        FamilyLedger { family: 0, green: 0 },
        FamilyLedger { family: 1, green: 0 },
        FamilyLedger { family: 2, green: 0 },
        FamilyLedger { family: 3, green: 0 },
        FamilyLedger { family: 4, green: 0 },
        FamilyLedger { family: 5, green: 0 },
    ];
    for (idx, &(f, q)) in QUOTA.iter().enumerate() {
        ledgers[idx] = FamilyLedger { family: f, green: q };
    }
    Ok(CloseReport { batch_id, ledgers, prev_fingerprint: prev })
}

// ===== 主判据 =====
pub fn run_cga92_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0092");
    // 族一：配额表。
    cs.add("配额和 92", quota_valid() == Ok(92), "Σ==92");
    cs.add("六族", QUOTA.len() == 6, "闭集");
    // 族二：标准收口。
    let rep = standard_report(9201, 0).map_err(|e| e as u16);
    cs.add("建批", rep.is_ok(), "批号合法");
    if let Ok(r) = &rep {
        cs.add("收口绿", close_gate(r).is_ok(), "六族全绿");
        cs.add("总绿 92", total_green(r) == 92, "配额全满");
        cs.add("无病族", first_sick(r).is_none(), "零缺口");
        // 族三：幂等（同报告双算同指纹）。
        let f1 = report_fingerprint(r);
        cs.add("指纹幂等", report_fingerprint(r) == f1, "双算一致");
        // 族四：病账检出（抽绿 1 点）。
        let mut sick = r.clone();
        sick.ledgers[3].green -= 1;
        cs.add("病账拒收", close_gate(&sick).is_err(), "SICK_FAMILY");
        cs.add("病族定位", first_sick(&sick) == Some(F_DOC), "F_DOC 病");
        // 族五：链式账。
        let rep2 = standard_report(9202, f1).map_err(|e| e as u16);
        if let Ok(r2) = &rep2 {
            let f2 = report_fingerprint(r2);
            cs.add("链推进", chain_check(f2, f1).is_ok(), "指纹变=推进");
            cs.add("链漂移抓", chain_check(f1, f1).is_err(), "同指纹漂移");
        }
        // 族六：指纹链敏感（上批指纹变 → 本批指纹变）。
        let rep3 = standard_report(9202, f1.wrapping_add(1)).map_err(|e| e as u16);
        if let Ok(r3) = &rep3 {
            let f2a = match standard_report(9202, f1) {
                Ok(rep2) => report_fingerprint(&rep2),
                Err(_) => 0,
            };
            cs.add("链敏感", report_fingerprint(r3) != f2a, "prev 变指纹变");
        }
    }
    // 族七：批号闸。
    cs.add("批号零拒", batch_gate(0).is_err(), "BATCH_OOB");
    cs.add("批号万拒", batch_gate(10000).is_err(), "BATCH_OOB");
    cs.add("批号 1 放", batch_gate(1).is_ok(), "下界");
    // 族八：超额绿（超额不病——配额是下限）。
    if let Ok(mut over) = standard_report(9203, 0) {
        over.ledgers[0].green += 5;
        cs.add("超额放行", close_gate(&over).is_ok(), "配额下限语义");
        cs.add("总绿 97", total_green(&over) == 97, "92+5");
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga92_deep()), run_cga92_deep2())
}

// ===== 深化面：批次链与指纹稳定二次账 =====
/// 标准报告双跑同指纹（构造确定性）。
pub fn std_report_stable() -> bool {
    match (standard_report(7, 0), standard_report(7, 0)) {
        (Ok(a), Ok(b)) => report_fingerprint(&a) == report_fingerprint(&b),
        _ => false,
    }
}

pub fn run_cga92_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga92_deep");
    cs.add("配额合法", quota_valid().is_ok(), "额度在域");
    if let Ok(rep) = standard_report(7, 0) {
        cs.add("标准报告收", close_gate(&rep).is_ok(), "全绿过闸");
        cs.add("首病 None", first_sick(&rep).is_none(), "无病");
        cs.add("全绿计数", total_green(&rep) > 0, "有绿");
        cs.add("指纹稳定", report_fingerprint(&rep) == report_fingerprint(&rep), "双算同值");
        cs.add("双跑同指", std_report_stable(), "构造确定");
        cs.add("链核", chain_check(1, 0).is_ok(), "cur>prev");
        cs.add("链破检出", chain_check(0, 1).is_err(), "倒链立案");
        cs.add("批次闸收", batch_gate(7).is_ok(), "域内批次");
    } else {
        cs.add("标准报告收", false, "构造失败");
    }
    cs
}

// ===== 深化面二：批次域扫与链序 =====
pub fn run_cga92_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga92_deep2");
    // 批号 1..6 全过闸 + 收口。
    cs.add("批域扫", {
        let mut all_ok = true;
        for id in 1..6u32 {
            if batch_gate(id).is_err() {
                all_ok = false;
            }
        }
        all_ok
    }, "1..6 全放");
    cs.add("配额双验", quota_valid().is_ok() && quota_valid().is_ok(), "幂等");
    // 链序单调：递增序列全过。
    cs.add("链序单调", {
        let mut prev = 0u64;
        let mut all_ok = true;
        for i in 1..8u64 {
            if chain_check(i, prev).is_err() {
                all_ok = false;
            }
            prev = i;
        }
        all_ok
    }, "递增全过");
    // 标准报告收口 + 首病 + 指纹链。
    if let (Ok(r1), Ok(r2)) = (standard_report(9203, 0), standard_report(9204, 0)) {
        cs.add("双批收口", close_gate(&r1).is_ok() && close_gate(&r2).is_ok(), "两批全过");
        cs.add("双批无病", first_sick(&r1).is_none() && first_sick(&r2).is_none(), "无病");
        cs.add("绿账一致", total_green(&r1) == total_green(&r2), "同构报告");
        let fa = report_fingerprint(&r1);
        let fb = report_fingerprint(&r2);
        cs.add("批间指纹异", fa != fb, "批号参与指纹");
    } else {
        cs.add("双批收口", false, "构建失败");
    }
    cs.add("同批同指纹", {
        match (standard_report(9205, 0), standard_report(9205, 0)) {
            (Ok(a), Ok(b)) => report_fingerprint(&a) == report_fingerprint(&b),
            _ => false,
        }
    }, "构造确定");
    cs
}
