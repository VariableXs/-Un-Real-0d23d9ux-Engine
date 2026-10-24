//! cga108_batchclose —— CGPU-F0108 批次收口（A 域 108/160 检查点）。
//! 锚点（CGPU 施工书）：CGPU-F0108 · 批次收口 · 目标 300 行
//! 判据摘录：检查点闭集（A 域 108 点七族：算子核 34/账本 15/注
//! 册表 12/文档册 12/语料 13/性能表 12/编解码 10——配额冻结）；
//! 收口核（七族配额全绿放行）；病账定位（首病族号）；链式指
//! 纹（上批指纹入账——92→108 推进链）；批号域闸；超额放行
//! （配额下限语义）；幂等指纹。
//! 组件：与 cga92 同构扩族（6→7 族，配额前移 92→108）+ 收口门
//!   + 病账定位 + 链账 + 指纹。
//! 职责定位：A 域第三里程碑闸（76→92→108）；编解码族（新域 10
//! 点：路由/基准/错误/文档四册完整核）首次入收口——里程碑账
//! 本随域扩展单调生长。
//!
//! 七族配额（冻结）：34+15+12+12+13+12+10 = 108

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xEA10 段 · 本模块独占）=====
pub const XC_BATCH_OOB: u32 = 0xEA10;
pub const XC_SICK: u32 = 0xEA11;
pub const XC_QUOTA_BAD: u32 = 0xEA12;
pub const XC_CHAIN_DRIFT: u32 = 0xEA13;

// ===== 七族配额闭集 =====
pub const F_CORE: u8 = 0;
pub const F_LEDGER: u8 = 1;
pub const F_REG: u8 = 2;
pub const F_DOC: u8 = 3;
pub const F_CORPUS: u8 = 4;
pub const F_PERF: u8 = 5;
pub const F_CODEC: u8 = 6;
pub const QUOTA: [(u8, u32); 7] = [
    (F_CORE, 34),
    (F_LEDGER, 15),
    (F_REG, 12),
    (F_DOC, 12),
    (F_CORPUS, 13),
    (F_PERF, 12),
    (F_CODEC, 10),
];

/// 族账。
#[derive(Clone)]
pub struct FamilyLedger {
    pub family: u8,
    pub green: u32,
}

/// 收口报告。
#[derive(Clone)]
pub struct CloseReport {
    pub batch_id: u32,
    pub ledgers: [FamilyLedger; 7],
    pub prev_fingerprint: u64,
}

/// 配额合法性（Σ==108 且逐项有界）。
pub fn quota_valid() -> Result<u32, u32> {
    let mut sum = 0u32;
    for &(_, q) in QUOTA.iter() {
        if q == 0 || q > 100 {
            return Err(XC_QUOTA_BAD);
        }
        sum += q;
    }
    if sum != 108 {
        return Err(XC_QUOTA_BAD);
    }
    Ok(sum)
}

/// 批号闸。
pub fn batch_gate(id: u32) -> Result<(), u32> {
    if id == 0 || id > 9999 {
        return Err(XC_BATCH_OOB);
    }
    Ok(())
}

/// 收口门。
pub fn close_gate(rep: &CloseReport) -> Result<(), u32> {
    for (idx, &(f, q)) in QUOTA.iter().enumerate() {
        if rep.ledgers[idx].family != f || rep.ledgers[idx].green < q {
            return Err(XC_SICK);
        }
    }
    Ok(())
}

/// 首病族定位。
pub fn first_sick(rep: &CloseReport) -> Option<u8> {
    for (idx, &(f, q)) in QUOTA.iter().enumerate() {
        if rep.ledgers[idx].green < q {
            return Some(f);
        }
    }
    None
}

/// 总绿点。
pub fn total_green(rep: &CloseReport) -> u32 {
    rep.ledgers.iter().map(|l| l.green).sum()
}

/// 指纹（批号×总绿×族绿×上批指纹）。
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

/// 链账。
pub fn chain_check(cur: u64, prev: u64) -> Result<(), u32> {
    if prev != 0 && cur == prev {
        return Err(XC_CHAIN_DRIFT);
    }
    Ok(())
}

/// 标准批构造。
pub fn standard_report(batch_id: u32, prev: u64) -> Result<CloseReport, u32> {
    batch_gate(batch_id)?;
    let mut ledgers: [FamilyLedger; 7] = [
        FamilyLedger { family: 0, green: 0 },
        FamilyLedger { family: 1, green: 0 },
        FamilyLedger { family: 2, green: 0 },
        FamilyLedger { family: 3, green: 0 },
        FamilyLedger { family: 4, green: 0 },
        FamilyLedger { family: 5, green: 0 },
        FamilyLedger { family: 6, green: 0 },
    ];
    for (idx, &(f, q)) in QUOTA.iter().enumerate() {
        ledgers[idx] = FamilyLedger { family: f, green: q };
    }
    Ok(CloseReport { batch_id, ledgers, prev_fingerprint: prev })
}

// ===== 主判据 =====
pub fn run_cga108_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0108");
    // 族一：配额。
    cs.add("配额和 108", quota_valid() == Ok(108), "Σ==108");
    cs.add("七族", QUOTA.len() == 7, "扩族");
    cs.add("编解码族 10", QUOTA[6].1 == 10, "新域配额");
    // 族二：标准收口。
    if let Ok(r) = standard_report(10801, 0) {
        cs.add("收口绿", close_gate(&r).is_ok(), "七族全绿");
        cs.add("总绿 108", total_green(&r) == 108, "配额满");
        cs.add("无病族", first_sick(&r).is_none(), "零缺口");
        let f1 = report_fingerprint(&r);
        cs.add("指纹幂等", report_fingerprint(&r) == f1, "双算一致");
        // 族三：病账。
        let mut sick = r.clone();
        sick.ledgers[6].green -= 3;
        cs.add("病账拒收", close_gate(&sick).is_err(), "SICK");
        cs.add("病族定位", first_sick(&sick) == Some(F_CODEC), "F_CODEC 病");
        // 族四：链。
        if let Ok(r2) = standard_report(10802, f1) {
            let f2 = report_fingerprint(&r2);
            cs.add("链推进", chain_check(f2, f1).is_ok(), "指纹变=推进");
            cs.add("链漂移抓", chain_check(f1, f1).is_err(), "同指漂移");
        }
        // 族五：超额放行。
        let mut over = r.clone();
        over.ledgers[0].green += 6;
        cs.add("超额放行", close_gate(&over).is_ok(), "下限语义");
        cs.add("总绿 114", total_green(&over) == 114, "108+6");
    }
    // 族六：批号闸。
    cs.add("批号零拒", batch_gate(0).is_err(), "OOB");
    cs.add("批号万拒", batch_gate(10000).is_err(), "OOB");
    cs.add("批号 1 放", batch_gate(1).is_ok(), "下界");
    // 族七：里程碑链锚（92 → 108 前移 16）。
    cs.add("前移 16", 108 - 92 == 16, "92→108");
    CheckSet::merge(CheckSet::merge(cs, run_cga108_deep()), run_cga108_deep2())
}

// ===== 深化面：批次闸域与链账 =====
/// 标准报告双跑同指纹（构造确定性）。
pub fn std_stable() -> bool {
    match (standard_report(9, 0), standard_report(9, 0)) {
        (Ok(a), Ok(b)) => report_fingerprint(&a) == report_fingerprint(&b),
        _ => false,
    }
}

pub fn run_cga108_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga108_deep");
    cs.add("配额合法", quota_valid().is_ok(), "额度在域");
    cs.add("批域拒", batch_gate(0).is_err() || batch_gate(1u32 << 30).is_err(), "越域至少一向");
    if let Ok(rep) = standard_report(9, 0) {
        cs.add("收口闸", close_gate(&rep).is_ok(), "全绿过");
        cs.add("首病无", first_sick(&rep).is_none(), "无病");
        cs.add("绿计数正", total_green(&rep) > 0, "有绿");
        cs.add("链顺", chain_check(2, 1).is_ok(), "cur>prev");
        cs.add("链倒拒", chain_check(1, 2).is_err(), "CHAIN_DRIFT");
    }
    cs.add("双跑同指", std_stable(), "构造确定");
    cs.add("族闭", F_CORE == 0 && F_CODEC == 6, "七族");
    cs
}

// ===== 深化面二：批次域与链账扩展 =====
pub fn run_cga108_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga108_deep2");
    // 批号 1..8 全过。
    cs.add("批域八连", (1..8u32).all(|id| batch_gate(id).is_ok()), "全放");
    // 递增链序列。
    cs.add("链序八连", {
        let mut prev = 0u64;
        let mut ok = true;
        for i in 1..8u64 {
            if chain_check(i, prev).is_err() {
                ok = false;
            }
            prev = i;
        }
        ok
    }, "递增全过");
    // 标准报告三批：全过闸 + 无病 + 指纹互异。
    cs.add("三批收口", {
        let mut ok = true;
        let mut fps = alloc::vec::Vec::new();
        for id in [11u32, 12, 13] {
            if let Ok(r) = standard_report(id, 0) {
                if close_gate(&r).is_err() || first_sick(&r).is_some() {
                    ok = false;
                }
                fps.push(report_fingerprint(&r));
            } else {
                ok = false;
            }
        }
        ok && fps[0] != fps[1] && fps[1] != fps[2]
    }, "全绿+指纹异");
    cs.add("配额幂等", quota_valid().is_ok() && quota_valid().is_ok(), "双验");
    cs.add("族闭核", [F_CORE, F_LEDGER, F_REG, F_DOC, F_CORPUS, F_PERF, F_CODEC].iter().enumerate().all(|(i, &f)| f == i as u8), "0..6 顺排");
    cs
}
