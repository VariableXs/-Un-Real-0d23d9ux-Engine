//! cgp09_privacy_checks — CGPU-F2409 遥测隐私 · 域自检判据。
//!
//! 判据原文（施工书锚点 CGPU-F2409）：隐私（遥测隐私（三级分类复用——隐私
//! 复用——隐私复用；测试（隐私一组）。判据：隐私复用、一组、判据。
//!
//! 判据纪律：零 panic 面（无 unwrap/expect，裁决用 == 于 PartialEq 闭集
//! 枚举）；预期常量独立给定；审计红线（账不含值）以结构形态断言。
//! 手算锚：四计数对账 sum == 总导出条数；脱敏 kept+stripped == 原维度数。

use crate::checks::CheckSet;
use crate::cgpu::cgp02_unifiedmodel::{Metric, MetricValue, PrivacyLevel};
use crate::cgpu::cgp09_privacy::{
    ExportVerdict, PrivacyGuard, Redactor, E_PV_GATE_CLOSED, E_PV_LOCAL, E_PV_UNREG,
    PRIVACY_VERSION,
};

/// 构造遥测样本（六元组 F2402 形状——维度集/档位/值逐参数显式）。
fn metric(id: &str, dims: &[u32], value: MetricValue, privacy: PrivacyLevel) -> Metric {
    Metric {
        id: alloc::format!("{}", id),
        caliber: alloc::format!("口径：{}", id),
        dimensions: {
            let mut v = alloc::vec::Vec::new();
            let mut i = 0usize;
            while i < dims.len() {
                v.push(dims[i]);
                i += 1;
            }
            v
        },
        value,
        timestamp_ns: 42,
        privacy,
    }
}

/// 判据主入口（注册于 cgpu/mod.rs blocks 数组，tag CGPU-F2409）。
pub fn run_cgp09_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-f2409");

    // --- 复用（判据一）---
    // 复-01：三级分类复用对拍——F2402 PrivacyLevel 三档 label 互异
    // （本地/匿名/公开）+ 档位序可辨（三档互异相等性）+ 版本锚定。
    // 编译期 `use crate::cgpu::cgp02_unifiedmodel::PrivacyLevel` 即类型面
    // 单源证明——运行期 label 对拍防漂移。
    let labels = [PrivacyLevel::Local.label(), PrivacyLevel::Anonymized.label(), PrivacyLevel::Public.label()];
    let labels_ok = labels[0] == "本地"
        && labels[1] == "匿名"
        && labels[2] == "公开"
        && labels[0] != labels[1]
        && labels[0] != labels[2]
        && labels[1] != labels[2];
    let tri_closed = PrivacyLevel::Local != PrivacyLevel::Anonymized
        && PrivacyLevel::Anonymized != PrivacyLevel::Public
        && PrivacyLevel::Local != PrivacyLevel::Public;
    let ver_ok = PRIVACY_VERSION == "P09-privacy-v1";
    if labels_ok && tri_closed && ver_ok {
        s.ok("F2409-复-01-三级分类类型面复用");
    } else {
        s.fail("F2409-复-01-三级分类类型面复用", "复用失效");
    }

    // --- 复用（判据二）---
    // 复-02：值形状卫生——Ppm 越界饱和口径（F2402 raw 构造面钳制）+
    // 导出侧复核 value_sane：合法 Ppm 真 / 越界 Ppm 假 / Count 恒真。
    let m_ok = metric("p01.fx.samples", &[1], MetricValue::Ppm(999_999), PrivacyLevel::Public);
    let m_bad = metric("p01.fx.samples", &[1], MetricValue::Ppm(1_000_001), PrivacyLevel::Public);
    let m_cnt = metric("p01.fx.count", &[1], MetricValue::Count(7), PrivacyLevel::Public);
    let sane_ok = crate::cgpu::cgp09_privacy::value_sane(&m_ok)
        && !crate::cgpu::cgp09_privacy::value_sane(&m_bad)
        && crate::cgpu::cgp09_privacy::value_sane(&m_cnt);
    if sane_ok {
        s.ok("F2409-复-02-值形状导出侧卫生");
    } else {
        s.fail("F2409-复-02-值形状导出侧卫生", "值卫生失效");
    }

    // --- 默认关（判据三）---
    // 闸-01：无显式同意全拦——三档样本（Local/Anonymized/Public）全部
    // GateClosed（vez10 纪律：档位再公开也出不去）；账四计数 closed=3。
    let mut g0 = PrivacyGuard::new();
    let m_local = metric("p01.a.x", &[1, 2], MetricValue::Count(1), PrivacyLevel::Local);
    let m_anon = metric("p01.b.y", &[1, 2], MetricValue::Count(2), PrivacyLevel::Anonymized);
    let m_pub = metric("p01.c.z", &[1, 2], MetricValue::Count(3), PrivacyLevel::Public);
    let v1 = g0.export(&m_local);
    let v2 = g0.export(&m_anon);
    let v3 = g0.export(&m_pub);
    let all_closed = v1 == ExportVerdict::GateClosed
        && v2 == ExportVerdict::GateClosed
        && v3 == ExportVerdict::GateClosed
        && !v1.released()
        && !v2.released()
        && !v3.released();
    let led0 = g0.ledger();
    let tally_ok = led0.closed == 3 && led0.total() == 3;
    if all_closed && tally_ok {
        s.ok("F2409-闸-01-默认关全拦显性");
    } else {
        s.fail("F2409-闸-01-默认关全拦显性", "默认关失效");
    }

    // --- 分流（判据四）---
    // 闸-02：同意后档位分流——Local 拦（BlockedLocal）/Public 放行
    // （Released）/Anonymized 脱敏放行（Redacted(保留数)）；
    // 白名单 {1,2} 注册 → 两维全保留 kept=2。
    let mut g1 = PrivacyGuard::new();
    g1.grant_consent();
    g1.allow_dimension(1);
    g1.allow_dimension(2);
    let r1 = g1.export(&m_local);
    let r2 = g1.export(&m_anon);
    let r3 = g1.export(&m_pub);
    let split_ok = r1 == ExportVerdict::BlockedLocal
        && r2 == ExportVerdict::Redacted(2)
        && r3 == ExportVerdict::Released
        && !r1.released()
        && r2.released()
        && r3.released();
    // 白名单幂等：重复注册不扩容。
    g1.allow_dimension(1);
    let idem_ok = g1.redactor().allow_len() == 2;
    if split_ok && idem_ok {
        s.ok("F2409-闸-02-同意后三档分流");
    } else {
        s.fail("F2409-闸-02-同意后三档分流", "档位分流失效");
    }

    // --- 脱敏（判据五）---
    // 脱-01：白名单注册制瘦身——五维 [1,2,3,4,5] 白名单 {1,3} →
    // kept=2 stripped=3 且 kept+stripped==原维度数（对账闭合）；
    // 空白名单全剥（kept=0 stripped=5）。
    let red = {
        let mut r = Redactor::new();
        r.allow_dimension(1);
        r.allow_dimension(3);
        r
    };
    let m5 = metric("p01.d.w", &[1, 2, 3, 4, 5], MetricValue::Count(9), PrivacyLevel::Anonymized);
    let (kept, stripped) = red.redact(&m5);
    let thin_ok = kept == 2 && stripped == 3 && kept + stripped == 5;
    let empty_red = Redactor::new();
    let (kept0, stripped0) = empty_red.redact(&m5);
    let all_strip = kept0 == 0 && stripped0 == 5;
    if thin_ok && all_strip {
        s.ok("F2409-脱-01-白名单维度瘦身");
    } else {
        s.fail("F2409-脱-01-白名单维度瘦身", "脱敏器失效");
    }

    // --- 审计账（判据六）---
    // 账-01：审计红线——账行结构**不含值**（AuditRow 只有 verdict_code +
    // level 两字段——账本身不成为隐私面，结构形态断言）+ 档位入账
    // （Local 样本的审计行 level==Local）+ 裁决码对拍。
    let audit = {
        let mut g = PrivacyGuard::new();
        g.grant_consent();
        g.export(&m_local); // BlockedLocal
        g.ledger().rows[0]
    };
    let audit_ok = audit.verdict_code == E_PV_LOCAL
        && audit.level == PrivacyLevel::Local
        && g0.ledger().rows[0].verdict_code == E_PV_GATE_CLOSED;
    if audit_ok {
        s.ok("F2409-账-01-审计不含值红线");
    } else {
        s.fail("F2409-账-01-审计不含值红线", "审计红线失效");
    }

    // --- 有界账（判据七）---
    // 账-02：洪峰丢最旧计数不静默——导出 130 条（>128 容量）→ 账行恰 128
    // + dropped=2 + 四计数总和 130（计数不受有界影响——账瘦了数没瘦）。
    let mut g2 = PrivacyGuard::new();
    g2.grant_consent();
    let mut i = 0u32;
    while i < 130 {
        let _ = g2.export(&m_pub);
        i += 1;
    }
    let led2 = g2.ledger();
    let bound_ok = led2.rows.len() == 128
        && led2.dropped() == 2
        && led2.total() == 130
        && led2.released == 130;
    if bound_ok {
        s.ok("F2409-账-02-有界洪峰丢最旧");
    } else {
        s.fail("F2409-账-02-有界洪峰丢最旧", "有界账失效");
    }

    // --- 编排（判据八）---
    // 编-01：全链路对账——同意+白名单 {1} 下混合流 5 条：
    // L(Local) / A(dims 1,2 匿名 kept=1) / P / A / L →
    // closed=0 blocked=2 redacted=2 released=1 total=5；
    // 摘要含版本与四计数锚。
    let mut g3 = PrivacyGuard::new();
    g3.grant_consent();
    g3.allow_dimension(1);
    let m_anon1 = metric("p01.e.a", &[1, 2], MetricValue::Count(1), PrivacyLevel::Anonymized);
    let _ = g3.export(&m_local);
    let _ = g3.export(&m_anon1);
    let _ = g3.export(&m_pub);
    let _ = g3.export(&m_anon1);
    let _ = g3.export(&m_local);
    let led3 = g3.ledger();
    let chain_ok = led3.closed == 0
        && led3.blocked_local == 2
        && led3.redacted == 2
        && led3.released == 1
        && led3.total() == 5;
    let sum3 = g3.summary();
    let sum_ok = sum3.contains("P09-privacy-v1")
        && sum3.contains("本地拦2")
        && sum3.contains("脱敏2")
        && sum3.contains("放行1");
    if chain_ok && sum_ok {
        s.ok("F2409-编-01-全链路四态对账");
    } else {
        s.fail("F2409-编-01-全链路四态对账", "全链路失效");
    }

    // --- 判据元（判据九）---
    // 元-01：诊断码四码互异 + 文案非空互异 + 未知码走缺省分支不 panic。
    let codes = [E_PV_GATE_CLOSED, E_PV_LOCAL, E_PV_UNREG, crate::cgpu::cgp09_privacy::E_PV_NOLEDGER];
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let mut det_uniq = true;
    let mut det_nonempty = true;
    for i in 0..4 {
        let d = crate::cgpu::cgp09_privacy::reason_of(codes[i]);
        if d.is_empty() {
            det_nonempty = false;
        }
        for j in (i + 1)..4 {
            if crate::cgpu::cgp09_privacy::reason_of(codes[i]) == crate::cgpu::cgp09_privacy::reason_of(codes[j]) {
                det_uniq = false;
            }
        }
    }
    let unknown_ok = crate::cgpu::cgp09_privacy::reason_of("E_UNKNOWN") == "未知隐私诊断码";
    if uniq && det_uniq && det_nonempty && unknown_ok {
        s.ok("F2409-元-01-四码闭集与缺省分支");
    } else {
        s.fail("F2409-元-01-四码闭集与缺省分支", "诊断码失效");
    }

    // --- 判据元（判据十）---
    // 元-02：零 panic 面——空账查询/空导出/零样本路径平凡；
    // 空白名单+Local 空维度集 Metric 走全链不炸（kept=0 路径）。
    let m_empty_dims = metric("p01.f.v", &[], MetricValue::Count(0), PrivacyLevel::Local);
    let mut g4 = PrivacyGuard::new();
    g4.grant_consent();
    let v_empty = g4.export(&m_empty_dims);
    let no_panic = v_empty == ExportVerdict::BlockedLocal
        && g4.ledger().total() == 1
        && g4.ledger().rows.len() == 1
        && g4.redactor().allow_len() == 0;
    if no_panic {
        s.ok("F2409-元-02-零panic空态平凡");
    } else {
        s.fail("F2409-元-02-零panic空态平凡", "panic 面失效");
    }

    s
}
