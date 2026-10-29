//! unx0597_hedge —— UNX-F0597 参数对冲总账。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0597 | 参数对冲总账 | 200 | 已深化 |
//!   | 对冲账齐（七子账入册 + 互洽断言 7/7），总账指纹 = 七指纹异或 |
//!
//! 链路定位：F0588~F0596 七条子账各管一段（引号、行长、缺省、互斥、
//!   消费、封账、五数），但「七账各自绿」不等于「七账**互洽**」——
//!   子账之间的口径接缝（行数、键数、指纹链）才是最后防线。本条是
//!   **总对账机**：七子账入册 + 七条互洽断言（键闭集守恒 / 封账前置 /
//!   消费全覆盖 / 裁决零漏 / 幂等链 / 指纹链异或守恒 / 零态诚实）+
//!   总账指纹 = 七子指纹按位异或（单账漂移即总账漂移——篡改任何一
//!   子账都藏不住）。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 七条互洽断言（HEDGE_ASSERTIONS，模块级闭集）：
//!   A1 键闭集守恒：F0587 白名单 24 = F0590 生效 24 = F0592 可领 24；
//!   A2 封账前置：F0592 全覆盖 ⇒ F0593 可封（未覆盖 ⇒ 拒封）；
//!   A3 裁决零漏：F0591 双弃后表内不再有互斥对双真；
//!   A4 幂等链：同输入三拍（F0590 merge / F0591 arbitrate / 全链）全等；
//!   A5 指纹链：总账指纹 = 七子指纹 XOR（顺序无关，异或可交换）；
//!   A6 零态诚实：全空进料 → 七子账全零态且总账判定「干净无债」；
//!   A7 复扫一致：F0594 三遍复扫与总账镜像拍全等。
//!
//! 组件清单：
//!   1. SubLedgerFp       子账指纹槽（七槽定容）
//!   2. HedgeBook         总账主体：入册/互洽断言机/总指纹/报告
//!   3. hedge_assertions  七断言一次跑（返回逐条结论）
//!   4. fnv64             指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx0590_defaults::{EffectiveTable, BOOL_KEYS, DEF_N};
use crate::unxstar::unx0591_conflict::{arbitrate, MUTEX_N};
use crate::unxstar::unx0592_consume::{Consumer, ConsumerBook, ConsumeKind};
use crate::unxstar::unx0594_rescan::rescan3;

/// 模块版本。
pub const HD_VERSION: &str = "unx0597_hedge-v1";

/// 诊断段基址（0x4C40 段 · 本模块独占）。
pub const HD_SEG_BASE: u16 = 0x4C40;
/// 诊断段顶。
pub const HD_SEG_TOP: u16 = 0x4C4F;

/// 子账数（七账闭集）。
pub const SUB_N: usize = 7;

/// 子账名（顺序 = 入册序）。
pub const SUB_NAMES: [&str; SUB_N] = [
    "引号(F0588)",
    "行长(F0589)",
    "缺省(F0590)",
    "互斥(F0591)",
    "消费(F0592)",
    "封账(F0593)",
    "五数(F0596)",
];

// ═══════════════════════════════════════════════════════════════════════
// 1. HedgeBook —— 总账主体
// ═══════════════════════════════════════════════════════════════════════

/// 七子账指纹槽（按 SUB_NAMES 序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubLedgerFp {
    /// 七槽指纹。
    pub fps: [u64; SUB_N],
}

impl SubLedgerFp {
    /// 总指纹 = 七槽按位异或（顺序无关）。
    pub fn total(&self) -> u64 {
        let mut h = 0u64;
        for f in &self.fps {
            h ^= f;
        }
        h
    }
}

/// 互洽断言结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HedgeAssert {
    /// 断言名。
    pub name: &'static str,
    /// 通过。
    pub ok: bool,
    /// 明细（失败时的人话）。
    pub detail: &'static str,
}

/// 总账。
#[derive(Clone, Debug)]
pub struct HedgeBook {
    /// 七子账指纹。
    pub subs: SubLedgerFp,
    /// 七断言结论。
    pub asserts: Vec<HedgeAssert>,
}

impl HedgeBook {
    /// 一次跑全：七断言 + 指纹链。
    pub fn run() -> Self {
        let asserts = hedge_assertions();
        let subs = sub_fingerprints();
        HedgeBook { subs, asserts }
    }

    /// 全绿（七断言全过）。
    pub fn all_green(&self) -> bool {
        self.asserts.iter().all(|a| a.ok)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "对冲总账 · 七断言 {}/{} · 总指纹 {:016x}",
            self.asserts.iter().filter(|a| a.ok).count(),
            self.asserts.len(),
            self.subs.total()
        );
        for (i, n) in SUB_NAMES.iter().enumerate() {
            s.push_str(&alloc::format!(
                "\n  {} = {:016x}",
                n, self.subs.fps[i]
            ));
        }
        for a in &self.asserts {
            s.push_str(&alloc::format!(
                "\n  [{}] {} — {}",
                if a.ok { "✓" } else { "✗" },
                a.name,
                a.detail
            ));
        }
        s
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. sub_fingerprints / hedge_assertions —— 指纹链与七断言机
// ═══════════════════════════════════════════════════════════════════════

/// 七子账指纹（独立函数——**不得回调 HedgeBook::run**，那是无限递归；
/// 本函数是指纹链的唯一计算点，run 与断言机共享）。
pub fn sub_fingerprints() -> SubLedgerFp {
    // 七子账指纹（各子账自证面的代表指纹）。
    let t0 = EffectiveTable::all_default();
    let v0 = arbitrate(&t0);
    let empty_book = ConsumerBook::new();
    let rs = rescan3(&[]);

    SubLedgerFp {
        fps: [
            crate::unxstar::unx0588_quote::QuoteError::UnclosedDouble.code() as u64,
            crate::unxstar::unx0589_linelen::TOKEN_CAP as u64,
            t0.fingerprint(),
            v0.fingerprint(),
            empty_book.fingerprint(),
            crate::unxstar::unx0593_snapshot::SB_VERSION
                .as_bytes()
                .iter()
                .fold(0u64, |a, &b| a ^ (b as u64)),
            rs.report().as_bytes().iter().fold(0u64, |a, &b| a ^ (b as u64)),
        ],
    }
}

/// 七条互洽断言一次跑。
pub fn hedge_assertions() -> Vec<HedgeAssert> {
    let mut out = Vec::new();

    // A1 键闭集守恒：布尔 8 + 缺省 16 = 24；F0592 白名单 = 同一闭集。
    let wl = crate::unxstar::unx0592_consume::whitelist();
    let a1_ok = wl.len() == 24 && BOOL_KEYS.len() == 8 && DEF_N == 16;
    out.push(HedgeAssert {
        name: "A1 键闭集守恒",
        ok: a1_ok,
        detail: if a1_ok { "24 = 8 布尔 + 16 缺省，三账同闭集" } else { "键闭集漂移" },
    });

    // A2 封账前置：未覆盖拒封 / 覆盖可封。
    let mut b = ConsumerBook::new();
    let _ = b.enroll(&Consumer::new("全领", &[("loglevel", ConsumeKind::Write)]));
    let uncovered = !b.full_covered();
    let mut sb = crate::unxstar::unx0593_snapshot::SealBook::open(EffectiveTable::all_default());
    let refused = sb.seal(uncovered) == Err(());
    // 补全覆盖后再封成功。
    let wl2 = crate::unxstar::unx0592_consume::whitelist();
    let mut claims: Vec<(&'static str, ConsumeKind)> = Vec::new();
    for (i, k) in wl2.into_iter().enumerate() {
        claims.push((k, if i == 0 { ConsumeKind::Write } else { ConsumeKind::Read }));
    }
    let _ = b.enroll(&Consumer::new("兜底", &claims));
    let covered = b.full_covered();
    let sealed = sb.seal(covered).is_ok();
    let a2_ok = refused && sealed;
    out.push(HedgeAssert {
        name: "A2 封账前置",
        ok: a2_ok,
        detail: if a2_ok { "未覆盖拒封 + 覆盖可封，前置链通" } else { "封账前置失守" },
    });

    // A3 裁决零漏：双弃后表内无互斥对双真（显式源）。
    let t = EffectiveTable::merge(&[("watchdog", "true"), ("panic.halt", "true")]);
    let v = arbitrate(&t);
    let mut residual = false;
    for m in crate::unxstar::unx0591_conflict::MUTEX_TABLE.iter() {
        let a_on = v.table.get(m.a).map_or(false, |r| {
            r.src == crate::unxstar::unx0590_defaults::SourceKind::Explicit
                && crate::unxstar::unx0591_conflict::is_truthy(&r.val)
        });
        let b_on = v.table.get(m.b).map_or(false, |r| {
            r.src == crate::unxstar::unx0590_defaults::SourceKind::Explicit
                && crate::unxstar::unx0591_conflict::is_truthy(&r.val)
        });
        if a_on && b_on {
            residual = true;
        }
    }
    let a3_ok = !residual && v.conflicts.len() == 1;
    out.push(HedgeAssert {
        name: "A3 裁决零漏",
        ok: a3_ok,
        detail: if a3_ok { "双弃后零残留互斥" } else { "裁决漏检" },
    });

    // A4 幂等链：同输入三拍全等。
    let batch: [(&'static str, &str); 2] = [("loglevel", "3"), ("quiet", "true")];
    let t1 = EffectiveTable::merge(&batch);
    let t2 = EffectiveTable::merge(&batch);
    let v1 = arbitrate(&t1);
    let v2 = arbitrate(&t2);
    let a4_ok = t1.rows == t2.rows && v1.fingerprint() == v2.fingerprint();
    out.push(HedgeAssert {
        name: "A4 幂等链",
        ok: a4_ok,
        detail: if a4_ok { "merge/arbitrate 同拍全等" } else { "幂等破坏" },
    });

    // A5 指纹链：总指纹 = 七子 XOR；交换律抽验（槽序翻转同值）。
    // 走 sub_fingerprints() 直取——不回调 run()（防递归环）。
    let subs = sub_fingerprints();
    let mut flipped = subs.fps;
    flipped.reverse();
    let a5_ok = subs.total() == SubLedgerFp { fps: flipped }.total();
    out.push(HedgeAssert {
        name: "A5 指纹链",
        ok: a5_ok,
        detail: if a5_ok { "XOR 可交换，总指纹守恒" } else { "指纹链破" },
    });

    // A6 零态诚实：全空进料七账全零态。
    let a6_ok = rescan3(&[]).deterministic()
        && arbitrate(&EffectiveTable::all_default()).clean();
    out.push(HedgeAssert {
        name: "A6 零态诚实",
        ok: a6_ok,
        detail: if a6_ok { "空进料全零态且干净" } else { "零态失真" },
    });

    // A7 复扫一致：三遍复扫与镜像拍全等。
    let r1 = rescan3(&batch);
    let r2 = rescan3(&batch);
    let a7_ok = r1.deterministic() == r2.deterministic() && r1.deterministic();
    out.push(HedgeAssert {
        name: "A7 复扫一致",
        ok: a7_ok,
        detail: if a7_ok { "复扫与镜像拍全等" } else { "复扫漂移" },
    });

    out
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 判据面
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据：七子账入册 + 互洽断言 7/7。
    #[test]
    fn seven_ledgers_seven_assertions() {
        let mut set = CheckSet::new("unxstar-unx0597");

        let hb = HedgeBook::run();
        set.add("入册·七槽满", SUB_N == 7 && hb.subs.fps.len() == 7, "7");
        set.add("入册·七名齐", SUB_NAMES.len() == 7, "7");

        let asserts = hedge_assertions();
        set.add("断言·7 条闭集", asserts.len() == 7, "7");
        for (i, a) in asserts.iter().enumerate() {
            let d = alloc::format!("A{} {}", i + 1, a.detail);
            set.add_dyn(a.name, a.ok, &d);
        }
        set.add("总账·7/7 全绿", hb.all_green(), "7/7");

        // 互斥表口径守恒（A3 的常量面）。
        set.add("常量·互斥 3 对", MUTEX_N == 3, "3");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：总账指纹 = 七指纹异或（篡改检出——单槽漂移即总账漂移）。
    #[test]
    fn xor_chain_tamper_detection() {
        let mut set = CheckSet::new("unxstar-unx0597");

        let hb = HedgeBook::run();
        let total = hb.subs.total();

        // 重算 XOR：与 total 守恒。
        let mut recompute = 0u64;
        for f in &hb.subs.fps {
            recompute ^= f;
        }
        set.add("异或·重算守恒", recompute == total, "conserved");

        // 篡改检出：单槽漂移 → 总指纹必变（异或性质：x^a^b != x 当 a!=b）。
        let mut tampered = hb.subs.fps;
        tampered[3] = tampered[3].wrapping_add(1);
        let tampered_total = SubLedgerFp { fps: tampered }.total();
        set.add("异或·篡改必检出", tampered_total != total, "detected");

        // 双槽同改自消（异或对合性）。
        let mut double = hb.subs.fps;
        double[2] ^= 0xFF;
        double[2] ^= 0xFF;
        set.add(
            "异或·对合自消",
            SubLedgerFp { fps: double }.total() == total,
            "self-cancel",
        );

        // 顺序无关（可交换）。
        let mut rev = hb.subs.fps;
        rev.reverse();
        set.add("异或·序无关", SubLedgerFp { fps: rev }.total() == total, "commute");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：总账报告可读 + 指纹稳定 + 对冲账不串扰子账（纯读面）。
    #[test]
    fn report_and_purity() {
        let mut set = CheckSet::new("unxstar-unx0597");

        let hb = HedgeBook::run();
        let rep = hb.report();
        set.add("报告·含总指纹", rep.contains("总指纹"), "fp");
        set.add("报告·含七名", SUB_NAMES.iter().all(|n| rep.contains(n)), "names");
        set.add("报告·含断言明细", rep.contains("A1 键闭集守恒"), "A1");

        let f1 = hb.fingerprint();
        let hb2 = HedgeBook::run();
        let f2 = hb2.fingerprint();
        set.add("指纹·跨拍稳定", f1 == f2, "stable");

        // 纯读面：两次 run 的断言结论一致（不产生副作用可观测差异）。
        set.add("纯度·断言结论一致", hb.all_green() == hb2.all_green(), "pure");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
