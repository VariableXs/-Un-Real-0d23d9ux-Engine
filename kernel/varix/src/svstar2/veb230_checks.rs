//! VE-F0230 判据层：Intel 固件接口只读状态（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0230`
//!
//! **锚点原文五条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | 只读红线 | `B30-RO-*` | 银行摘要逐位不变 + 判据侧独立 FNV 对拍 + 缺测不编值 + 采集幂等 |
//! | 能力降级 | `B30-DEG-*` | GuC 失败进直通 + 三要素 + F0103 契约 + HuC 不牵连（双向）+ 不刷屏 + F0224 宿主 |
//! | 缺测标记 | `B30-MISS-*` | 全 1 缺测 + 魔数缺测 + 统计缺测 + 双向正常不缺测 |
//! | 如实呈现 | `B30-ASIS-*` | 倒退原值保留 + 越界保留 + 回绕保留 + 双向单调无异常 + 截断如实 |
//! | 判据 | `B30-PERF-*`/`B30-META-*` | O(1) 步进/切换计数 + 条数对账/截断/码段独占/桥接单射/上游代际标签 |
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! - 「只读红线」若写成「调 collect 再看结果对不对」是自证式——本层用
//!   **独立实现的 FNV-1a** 重算银行摘要，与被测 `digest()` 对拍；摘要
//!   采集前后逐位不变才放行，任何写路径都会被当场抓红。
//! - 「如实呈现」若只断「有异常记录」抓不到修数——本层断**观测值
//!   逐字等于注入的异常值**（改成 0 或钳到上界立即红）。
//! - 降级判据**双向**：GuC 失败必须降级（拒「该降不降」），HuC 失败
//!   必须不降级（拒「不该降也降」）。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

use super::veb12_recovery::NOTICE_LINK_VERSION;
use super::veb21_ident::GenTier;
use super::veb230_firmstate::*;
use super::veb24_submit::SubmitPath;

// ---------------------------------------------------------------------------
// 判据侧独立参照（不向被测要答案）
// ---------------------------------------------------------------------------

/// 判据侧独立实现的 FNV-1a 银行摘要（口径与被测 [`FwRegBank::digest`]
/// 相同但代码独立：逐字异或后乘 FNV 素数）。
///
/// 独立重算的意义：被测 `digest()` 若被改成「恒返回某常量」，只断
/// 「digest == digest」的门禁是恒真；对拍一个独立实现才抓得住。
fn alt_digest(b: &FwRegBank) -> u32 {
    let words: [u32; 10] = [
        b.guc_status,
        b.huc_status,
        b.guc_stats[0],
        b.guc_stats[1],
        b.guc_stats[2],
        b.guc_stats[3],
        b.huc_stats[0],
        b.huc_stats[1],
        b.huc_stats[2],
        b.huc_stats[3],
    ];
    let mut h: u32 = 0x811C_9DC5;
    for w in words {
        h ^= w;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 两固件全部诊断码（判据侧点名，不从被测导出清单）。
const ALL_CODES: [FwCode; 5] = [
    FwCode::GUC_STATUS_MISS,
    FwCode::HUC_STATUS_MISS,
    FwCode::STATS_MISS,
    FwCode::STATS_ANOMALY,
    FwCode::STATE_INVALID,
];

/// 干净银行：两固件已加载已认证、统计单调一致。
fn ok_bank() -> FwRegBank {
    FwRegBank {
        guc_status: FW_STATUS_MAGIC | 0b10 | (1 << 4),
        huc_status: FW_STATUS_MAGIC | 0b10 | (1 << 4),
        guc_stats: [10, 9, 1, 100],
        huc_stats: [5, 5, 0, 50],
    }
}

/// 预期判据条数（写死后与 len 对账——超限截断或漏判都会红）。
const EXPECTED_CHECKS: usize = 27;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// F0230 域自检入口。
pub fn run_veb230_checks() -> CheckSet {
    let mut s = CheckSet::new("intel-fw-state");
    let gen = GenTier::XeStandard;
    let mut reader = FirmReader::new();

    // ================= 一、只读红线（B30-RO-*） =================

    // RO-1：采集前后银行摘要逐位不变——零写路径的可运行断言。
    let bank = ok_bank();
    let before = bank.digest();
    let _ = reader.collect(&bank, gen);
    s.add("B30-RO-采集前后银行逐位不变", bank.digest() == before, "");

    // RO-2：判据侧独立 FNV 对拍——被测摘要实现被改坏/改恒值立刻红。
    let bank2 = FwRegBank {
        guc_status: FW_STATUS_MAGIC | 0b01,
        huc_status: FW_STATUS_MAGIC | 0b11,
        guc_stats: [1, 0, 0, 7],
        huc_stats: [READ_FAIL; STAT_WORDS],
    };
    s.add(
        "B30-RO-摘要独立重算对拍",
        alt_digest(&bank2) == bank2.digest() && alt_digest(&bank) == bank.digest(),
        "",
    );

    // RO-3：缺测绝不编值——缺测通道的加载态与统计必须都是 None。
    let mut miss_bank = ok_bank();
    miss_bank.huc_status = READ_FAIL;
    miss_bank.huc_stats = [READ_FAIL; STAT_WORDS];
    let mut r3 = FirmReader::new();
    let snap3 = r3.collect(&miss_bank, gen);
    let huc_ok = match snap3.huc.load {
        None => match snap3.huc.stats {
            None => snap3.huc.is_missing(),
            Some(_) => false,
        },
        Some(_) => false,
    };
    s.add("B30-RO-缺测不编值补位", huc_ok, "");

    // RO-4：采集幂等——同一银行连采两次，业务字段逐位一致（无副作用面）。
    let mut r4 = FirmReader::new();
    let a = r4.collect(&bank, gen);
    let b = r4.collect(&bank, gen);
    s.add(
        "B30-RO-同银行连采幂等",
        a.guc == b.guc && a.huc == b.huc && a.mode == b.mode,
        "",
    );

    // ================= 二、能力降级（B30-DEG-*） =================

    // DEG-1：GuC 加载失败 → 降级 + 直通模式标记（该降必降）。
    let mut deg_bank = ok_bank();
    deg_bank.guc_status = FW_STATUS_MAGIC | 0b11;
    let mut rd = FirmReader::new();
    let snap = rd.collect(&deg_bank, gen);
    s.add(
        "B30-DEG-GuC失败进直通模式",
        snap.degraded && snap.mode == DegradeMode::ExeclistsDirect,
        "",
    );

    // DEG-2：降级通知三要素齐（what/why/next 一个不空）。
    let notices = rd.take_notices();
    let triplet_ok = match notices.first() {
        Some(n) => n.has_triplet(),
        None => false,
    };
    s.add("B30-DEG-降级通知三要素齐", triplet_ok, "");

    // DEG-3：通知走 F0103 契约（版本同源 + 可关闭——无障碍面）。
    let contract_ok = match notices.first() {
        Some(n) => n.version == NOTICE_LINK_VERSION && n.dismissible,
        None => false,
    };
    s.add("B30-DEG-通知走F0103契约可关闭", contract_ok, "");

    // DEG-4：HuC 加载失败**不**牵连提交通路（不该降也降即红——双向之二）。
    let mut huc_bad = ok_bank();
    huc_bad.huc_status = FW_STATUS_MAGIC | 0b11;
    let mut r4b = FirmReader::new();
    let snap_huc = r4b.collect(&huc_bad, gen);
    s.add(
        "B30-DEG-HuC失败不牵连提交",
        !snap_huc.degraded && snap_huc.mode == DegradeMode::GuSubmit,
        "",
    );

    // DEG-5：GuC 正常不降级（双向之一：不该降不降）。
    let mut r5 = FirmReader::new();
    let snap_ok = r5.collect(&bank, gen);
    s.add(
        "B30-DEG-GuC正常不降级",
        !snap_ok.degraded && snap_ok.mode == DegradeMode::GuSubmit,
        "",
    );

    // DEG-6：通知不刷屏——连续三个失败周期只发一次通知、计一次切换。
    let mut r6 = FirmReader::new();
    let _ = r6.collect(&deg_bank, gen);
    let _ = r6.collect(&deg_bank, gen);
    let _ = r6.collect(&deg_bank, gen);
    s.add(
        "B30-DEG-边沿触发不刷屏",
        r6.take_notices().len() == 1 && r6.switches() == 1,
        "",
    );

    // DEG-7：直通模式的宿主（上游 F0224 提交通路）真实存活可用。
    let path = SubmitPath::new();
    s.add(
        "B30-DEG-F0224直通宿主可用",
        !path.status_summary().is_empty(),
        "",
    );

    // ================= 三、缺测标记（B30-MISS-*） =================

    // MISS-1：状态寄存器全 1 = 读取失败 → 缺测，不是「未加载」。
    let mut f1 = ok_bank();
    f1.guc_status = READ_FAIL;
    let mut r7 = FirmReader::new();
    let s1 = r7.collect(&f1, gen);
    let miss1 = match s1.guc.load {
        None => s1.guc.is_missing(),
        Some(_) => false,
    };
    s.add("B30-MISS-全1读取失败标缺测", miss1, "");

    // MISS-2：魔数不符 = 接口不可信 → 缺测，且通道缺测码可追溯。
    let mut f2 = ok_bank();
    f2.huc_status = 0xBAD0_0000 | 0b10 | (1 << 4);
    let mut r8 = FirmReader::new();
    let s2 = r8.collect(&f2, gen);
    let miss2 = match s2.huc.missing {
        Some(c) => c == FwCode::HUC_STATUS_MISS && s2.huc.load.is_none(),
        None => false,
    };
    s.add("B30-MISS-魔数不符标缺测带码", miss2, "");

    // MISS-3：状态正常但统计读不到 → 统计缺测同样显性（要点四）。
    let mut f3 = ok_bank();
    f3.guc_stats = [READ_FAIL; STAT_WORDS];
    let mut r9 = FirmReader::new();
    let s3 = r9.collect(&f3, gen);
    let miss3 = match s3.guc.stats {
        None => match s3.guc.missing {
            Some(c) => c == FwCode::STATS_MISS,
            None => false,
        },
        Some(_) => false,
    };
    s.add("B30-MISS-统计缺测同样显性", miss3, "");

    // MISS-4：双向——正常读不缺测，快照级缺测标记为假。
    let mut r10 = FirmReader::new();
    let s4 = r10.collect(&bank, gen);
    s.add(
        "B30-MISS-正常读不缺测",
        !s4.has_missing() && !s4.guc.is_missing() && !s4.huc.is_missing(),
        "",
    );

    // ================= 四、如实呈现不修数（B30-ASIS-*） =================

    // ASIS-1：计数倒退——观测值逐字等于注入值（钳制/清零/丢弃即红）。
    let mut r11 = FirmReader::new();
    let _ = r11.collect(&bank, gen);
    let mut reg = ok_bank();
    reg.guc_stats = [3, 2, 0, 100]; // 10 -> 3 倒退
    let s5 = r11.collect(&reg, gen);
    let asis1 = match s5.anomalies.first() {
        Some(a) => a.observed == 3 && a.kind == FwKind::Guc,
        None => false,
    };
    s.add("B30-ASIS-倒退观测值原样保留", asis1, "");

    // ASIS-2：组内越界（错误 > 提交）同样保留原值。
    let mut r12 = FirmReader::new();
    let _ = r12.collect(&bank, gen);
    let mut bad = ok_bank();
    bad.guc_stats = [10, 9, 33, 100]; // errors 33 > submissions 10
    let s6 = r12.collect(&bad, gen);
    let asis2 = match s6.anomalies.first() {
        Some(a) => a.observed == 33,
        None => false,
    };
    s.add("B30-ASIS-越界观测值原样保留", asis2, "");

    // ASIS-3：回绕形态按回绕如实记异常——不按「清零修数」吞掉。
    // 高基线周期：提交计数接近上界（末字避开 READ_FAIL 全 1 特征）。
    let mut r13b = FirmReader::new();
    let mut hi = ok_bank();
    hi.guc_stats = [0xFFFF_FF00, 0xFFFF_FE00, 0, 0x0000_1000];
    let _ = r13b.collect(&hi, gen);
    let mut wrap = ok_bank();
    wrap.guc_stats = [0x0000_0008, 0x0000_0004, 0, 0x10]; // 0xFFFF_FF00 -> 8：回绕形态
    let s7 = r13b.collect(&wrap, gen);
    let asis3 = match s7.anomalies.first() {
        Some(a) => a.observed == 8 && a.code == FwCode::STATS_ANOMALY,
        None => false,
    };
    s.add("B30-ASIS-回绕形态如实记异常", asis3, "");

    // ASIS-4：双向——单调一致的统计不产生异常（正常不报病）。
    let mut r14 = FirmReader::new();
    let _ = r14.collect(&bank, gen);
    let mut up = ok_bank();
    up.guc_stats = [12, 11, 2, 101];
    let s8 = r14.collect(&up, gen);
    s.add("B30-ASIS-单调一致无异常", s8.anomalies.is_empty(), "");

    // ASIS-5：异常容量截断如实标记（不静默丢，也不无限增长）。
    let mut r15 = FirmReader::new();
    let _ = r15.collect(&bank, gen);
    let mut storm = ok_bank();
    storm.guc_stats = [3, 2, 9, 100]; // 倒退 + 越界 = 2 条
    storm.huc_stats = [1, 0, 7, 50]; // 倒退 + 越界 = 2 条
    let s9 = r15.collect(&storm, gen);
    s.add(
        "B30-ASIS-异常截断如实标记",
        s9.anomalies.len() == MAX_ANOMALIES && s9.anomalies_truncated,
        "",
    );

    // ================= 五、性能 O(1)（B30-PERF-*） =================

    // PERF-1：采集周期步进恰为 1——每周期固定动作，无隐藏循环面。
    let mut r16 = FirmReader::new();
    let t1 = r16.collect(&bank, gen).tick;
    let t2 = r16.collect(&bank, gen).tick;
    s.add("B30-PERF-采集周期步进恰一", t1 == 1 && t2 == 2, "");

    // PERF-2：降级切换计数逐次精确——降→复→降三次边沿恰计 3（切换
    // O(1)：每边沿一次比较一次赋值一次计数，无批量结算）。
    let mut r17 = FirmReader::new();
    let _ = r17.collect(&deg_bank, gen); // 降
    let _ = r17.collect(&bank, gen); // 复
    let _ = r17.collect(&deg_bank, gen); // 再降
    s.add("B30-PERF-切换计数逐次精确", r17.switches() == 3, "");

    // ================= 六、判据自检与对接（B30-META-*） =================

    // META-2：判据容量无截断。
    s.add("B30-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x3Dxx，且 != 0x3C/0x3B/0x34（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x3D)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x3C && hi != 0x3B && hi != 0x34
        });
    s.add("B30-META-诊断码段独占", section_ok, "");

    // META-4：桥接单射 + 人话原因非空——桥接是映射不是改写。
    let mut bridge_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].bridge() == ALL_CODES[j].bridge() {
                bridge_ok = false; // 单射破坏：两码同桥即下游无法溯源
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() || c.bridge().1 != c.code() {
            bridge_ok = false;
        }
    }
    s.add("B30-META-桥接单射原因非空", bridge_ok, "");

    // META-5：上游 F0221 代际口径真实入快照（下游 F0235 按代际定位）。
    let mut r18 = FirmReader::new();
    let snap_gen = r18.collect(&bank, GenTier::Baseline);
    s.add(
        "B30-META-上游代际标签入快照",
        snap_gen.gen_label == GenTier::Baseline.label(),
        "",
    );

    // META-1：判据条数对账（放末位：此时 len 应为 26，加自身恰 27）。
    s.add(
        "B30-META-判据条数对账",
        s.len() + 1 == EXPECTED_CHECKS,
        "",
    );

    s
}
