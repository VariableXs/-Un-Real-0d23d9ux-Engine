//! unx0591_conflict —— UNX-F0591 参数冲突检出（互斥表）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0591 | 参数冲突检出（互斥表） | 200 | 已深化 |
//!   | 互斥账齐（3 对互斥 + 2 条联动），冲突检出 3/3 与基准一致 |
//!
//! 链路定位：上游 F0587 注记「quiet 与 loglevel 联动（quiet 时 loglevel
//!   视作封顶 3）」「watchdog 与 panic.halt 处置互斥（双弃规则）」——
//!   本条把散落的注记收拢成**一张互斥/联动表**并落检出机。语义分两类：
//!   互斥（两键同时显式真 = 冲突，按裁决规则处理）、联动（A 显式真时
//!   B 被封顶/钳制，不是冲突而是改写）。裁决规则冻结为**双弃**
//!   (both-drop)：互斥对双双回落缺省并双双入冲突账——宁可保守不可
//!   偏袒（首现优先会隐藏第二个冲突源）。下游 F0593 快照封账只收
//!   本条裁决后的生效集。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（MUTEX_SEMANTICS）：
//!   1. 互斥对：两键同时显式 → 冲突 → 双弃（回落缺省）+ 冲突账入册；
//!   2. 联动条：条件键显式真 → 受控键值被钳到 bound（改写非冲突，
//!      联动账入册，原值留痕）；
//!   3. 互斥判定只看**显式**来源（缺省值之间永不冲突——缺省态是
//!      出厂口径，天然自洽）；
//!   4. 裁决后键值来源回落 Defaulted（值 = F0590 缺省）。
//!
//! 组件清单：
//!   1. MUTEX_TABLE      互斥对 3 对（键 a/b + 对立语义注记）
//!   2. LINK_TABLE       联动条 2 条（条件键/受控键/封顶值）
//!   3. ConflictEntry    冲突账目（对 + 双方原值 + 裁决）
//!   4. LinkEntry        联动账目（条 + 原值 + 钳后值）
//!   5. arbiter          裁决机：进 F0590 生效表 → 冲突检出 → 双弃/
//!                       联动钳制 → 出裁决后生效表 + 双账
//!   6. fnv64            指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx0586_valdom::DEFAULTS;
use crate::unxstar::unx0590_defaults::{EffectiveRow, EffectiveTable, SourceKind};

/// 模块版本。
pub const MX_VERSION: &str = "unx0591_conflict-v1";

/// 诊断段基址（0x4A60 段 · 本模块独占）。
pub const MX_SEG_BASE: u16 = 0x4A60;
/// 诊断段顶。
pub const MX_SEG_TOP: u16 = 0x4A6F;

// ═══════════════════════════════════════════════════════════════════════
// 1. MUTEX_TABLE / LINK_TABLE —— 互斥与联动闭集
// ═══════════════════════════════════════════════════════════════════════

/// 互斥对数。
pub const MUTEX_N: usize = 3;
/// 联动条数。
pub const LINK_N: usize = 2;

/// 互斥对（两键语义对立：同时显式 = 处置冲突）。
#[derive(Clone, Copy, Debug)]
pub struct MutexPair {
    /// 键 a。
    pub a: &'static str,
    /// 键 b。
    pub b: &'static str,
    /// 对立语义注记（人话）。
    pub why: &'static str,
}

/// 互斥表 3 对（来源注记对齐 F0587 键表 note 列）。
pub const MUTEX_TABLE: [MutexPair; MUTEX_N] = [
    MutexPair {
        a: "watchdog",
        b: "panic.halt",
        why: "NMI 看门狗要在 panic 后复位续命，panic.halt 要停机不复位——处置对立",
    },
    MutexPair {
        a: "nokaslr",
        b: "quiet",
        why: "地址布局固定化与安静模式同为取证口径单选题——双开无增量价值且日志面冲突",
    },
    MutexPair {
        a: "earlycon",
        b: "acpi.off",
        why: "早期控制台依赖 ACPI 表定位 UART，acpi.off 抽掉其依赖——依赖对立",
    },
];

/// 联动条（条件键显式真 → 受控键钳到 bound）。
#[derive(Clone, Copy, Debug)]
pub struct LinkRule {
    /// 条件键（显式真才触发）。
    pub cond: &'static str,
    /// 受控键。
    pub target: &'static str,
    /// 封顶值（受控键被钳到此值）。
    pub bound: &'static str,
    /// 语义注记。
    pub why: &'static str,
}

/// 联动表 2 条（quiet→loglevel 封顶 3 为 F0587 原文注记）。
pub const LINK_TABLE: [LinkRule; LINK_N] = [
    LinkRule {
        cond: "quiet",
        target: "loglevel",
        bound: "3",
        why: "安静模式封顶日志级别 3（低优日志抑制的数值化口径）",
    },
    LinkRule {
        cond: "smp.off",
        target: "sched.slice.us",
        bound: "4000",
        why: "单核引导下时间片回落缺省（多核调优参数在单核无意义）",
    },
];

// ═══════════════════════════════════════════════════════════════════════
// 2. ConflictEntry / LinkEntry —— 双账
// ═══════════════════════════════════════════════════════════════════════

/// 冲突账目（双弃裁决）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictEntry {
    /// 键 a。
    pub a: &'static str,
    /// 键 b。
    pub b: &'static str,
    /// a 的显式原值。
    pub val_a: String,
    /// b 的显式原值。
    pub val_b: String,
    /// 对立语义注记。
    pub why: &'static str,
}

/// 联动账目（钳制改写）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkEntry {
    /// 条件键。
    pub cond: &'static str,
    /// 受控键。
    pub target: &'static str,
    /// 原值（改写前留痕）。
    pub old_val: String,
    /// 钳后值。
    pub new_val: String,
    /// 语义注记。
    pub why: &'static str,
}

// ═══════════════════════════════════════════════════════════════════════
// 3. arbiter —— 裁决机
// ═══════════════════════════════════════════════════════════════════════

/// 裁决产物：裁决后生效表 + 冲突账 + 联动账。
#[derive(Clone, Debug)]
pub struct Verdict {
    /// 裁决后生效表（冲突双弃已回落缺省；联动钳制已改写）。
    pub table: EffectiveTable,
    /// 冲突账。
    pub conflicts: Vec<ConflictEntry>,
    /// 联动账。
    pub links: Vec<LinkEntry>,
}

/// 查缺省值（键 → F0590 缺省表 / 布尔假；白名单外 None）。
fn default_of_key(key: &str) -> Option<&'static str> {
    for d in DEFAULTS.iter() {
        if d.key == key {
            return Some(d.val);
        }
    }
    // 布尔键缺省假（F0590 BOOL_DEFAULT 口径）。
    const BOOL_KEYS: [&str; 8] = [
        "nokaslr", "quiet", "earlycon", "smp.off", "watchdog", "acpi.off", "efi.debug", "panic.halt",
    ];
    for k in BOOL_KEYS {
        if k == key {
            return Some("false");
        }
    }
    None
}

/// 裁决：进 F0590 生效表，出裁决后表 + 双账。
///
/// 流程：互斥检出（仅显式来源）→ 双弃回落 → 联动钳制（条件键
/// 裁决后仍显式真才触发——被双弃的条件键不再引发联动）。
pub fn arbitrate(input: &EffectiveTable) -> Verdict {
    let mut t = input.clone();
    let mut conflicts = Vec::new();

    // ① 互斥检出 + 双弃。
    for m in MUTEX_TABLE.iter() {
        let a_on = t.get(m.a).map_or(false, |r| {
            r.src == SourceKind::Explicit && is_truthy(&r.val)
        });
        let b_on = t.get(m.b).map_or(false, |r| {
            r.src == SourceKind::Explicit && is_truthy(&r.val)
        });
        if a_on && b_on {
            let va = t.get(m.a).map(|r| r.val.clone()).unwrap_or_default();
            let vb = t.get(m.b).map(|r| r.val.clone()).unwrap_or_default();
            conflicts.push(ConflictEntry {
                a: m.a,
                b: m.b,
                val_a: va,
                val_b: vb,
                why: m.why,
            });
            // 双弃：双双回落缺省、来源回落 Defaulted。
            for k in [m.a, m.b] {
                if let Some(r) = t.rows.iter_mut().find(|r| r.key == k) {
                    if let Some(d) = default_of_key(k) {
                        r.val = d.to_string();
                    }
                    r.src = SourceKind::Defaulted;
                }
            }
        }
    }

    // ② 联动钳制（条件键裁决后仍显式真才触发）。
    let mut links = Vec::new();
    for l in LINK_TABLE.iter() {
        let cond_on = t.get(l.cond).map_or(false, |r| {
            r.src == SourceKind::Explicit && is_truthy(&r.val)
        });
        if !cond_on {
            continue;
        }
        if let Some(r) = t.rows.iter_mut().find(|r| r.key == l.target) {
            if r.val != l.bound {
                let old = r.val.clone();
                r.val = l.bound.to_string();
                links.push(LinkEntry {
                    cond: l.cond,
                    target: l.target,
                    old_val: old,
                    new_val: l.bound.to_string(),
                    why: l.why,
                });
            }
        }
    }

    Verdict { table: t, conflicts, links }
}

/// 真值判定（布尔词形：true/on/1/yes → 真；其余假——宽容收口）。
pub fn is_truthy(v: &str) -> bool {
    matches!(v, "true" | "on" | "1" | "yes")
}

impl Verdict {
    /// 全账健康：零冲突。
    pub fn clean(&self) -> bool {
        self.conflicts.is_empty()
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = format!(
            "互斥裁决账 · 冲突 {} · 联动 {}",
            self.conflicts.len(),
            self.links.len()
        );
        for c in &self.conflicts {
            s.push_str(&format!(
                "\n  冲突 {}×{}（{} 保留 / {} 保留）→ 双弃回落缺省：{}",
                c.a, c.b, c.val_a, c.val_b, c.why
            ));
        }
        for l in &self.links {
            s.push_str(&format!(
                "\n  联动 {} → {}：{} 钳至 {}（{}）",
                l.cond, l.target, l.old_val, l.new_val, l.why
            ));
        }
        s
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
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

    /// 判据：冲突检出 3/3 与基准一致（三对互斥逐一注入）。
    #[test]
    fn three_mutex_pairs_detected() {
        let mut set = CheckSet::new("unxstar-unx0591");

        // 对 1：watchdog × panic.halt。
        let t1 = EffectiveTable::merge(&[("watchdog", "true"), ("panic.halt", "true")]);
        let v1 = arbitrate(&t1);
        set.add("冲突1·watchdog×halt 检出", v1.conflicts.len() == 1, "1");
        set.add("冲突1·双弃回落假", {
            let a = v1.table.get("watchdog").map(|r| r.val.clone());
            let b = v1.table.get("panic.halt").map(|r| r.val.clone());
            a.as_deref() == Some("false") && b.as_deref() == Some("false")
        }, "false×2");
        set.add("冲突1·来源回落缺省", {
            v1.table.get("watchdog").map_or(false, |r| r.src == SourceKind::Defaulted)
        }, "defaulted");

        // 对 2：nokaslr × quiet。
        let t2 = EffectiveTable::merge(&[("nokaslr", "on"), ("quiet", "yes")]);
        let v2 = arbitrate(&t2);
        set.add("冲突2·nokaslr×quiet 检出", v2.conflicts.len() == 1, "1");
        // 词形宽容：on/yes 均判真（is_truthy 面）。
        set.add("词形·on 判真", is_truthy("on"), "on");
        set.add("词形·yes 判真", is_truthy("yes"), "yes");
        set.add("词形·0 判假", !is_truthy("0"), "0");

        // 对 3：earlycon × acpi.off。
        let t3 = EffectiveTable::merge(&[("earlycon", "1"), ("acpi.off", "true")]);
        let v3 = arbitrate(&t3);
        set.add("冲突3·earlycon×acpi.off 检出", v3.conflicts.len() == 1, "1");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：联动 2 条（quiet→loglevel 封顶、smp.off→slice 回落）。
    #[test]
    fn linkage_clamps_two_rules() {
        let mut set = CheckSet::new("unxstar-unx0591");

        // 联动 1：quiet 显式真 → loglevel 钳 3（原 6 留痕）。
        let t1 = EffectiveTable::merge(&[("quiet", "true"), ("loglevel", "7")]);
        let v1 = arbitrate(&t1);
        set.add("联动1·零冲突", v1.conflicts.is_empty(), "clean");
        set.add("联动1·账目 1 条", v1.links.len() == 1, "1");
        set.add("联动1·loglevel 钳 3", {
            v1.table.get("loglevel").map_or(false, |r| r.val == "3")
        }, "clamped 3");
        set.add("联动1·原值 7 留痕", {
            v1.links.first().map_or(false, |l| l.old_val == "7")
        }, "old=7");

        // 联动 2：smp.off 显式真 → sched.slice.us 回落 4000。
        let t2 = EffectiveTable::merge(&[("smp.off", "true"), ("sched.slice.us", "9000")]);
        let v2 = arbitrate(&t2);
        set.add("联动2·slice 钳 4000", {
            v2.table.get("sched.slice.us").map_or(false, |r| r.val == "4000")
        }, "clamped 4000");
        set.add("联动2·原值 9000 留痕", {
            v2.links.first().map_or(false, |l| l.old_val == "9000")
        }, "old=9000");

        // 条件键值已是 bound → 不入联动账（改写无意义）。
        let t3 = EffectiveTable::merge(&[("quiet", "true"), ("loglevel", "3")]);
        let v3 = arbitrate(&t3);
        set.add("联动·值已 bound 不入账", v3.links.is_empty(), "no-op");

        // 条件键非显式（缺省假）→ 不触发。
        let t4 = EffectiveTable::merge(&[("loglevel", "7")]);
        let v4 = arbitrate(&t4);
        set.add("联动·条件假不触发", v4.links.is_empty(), "no trigger");
        set.add("联动·loglevel 保持 7", {
            v4.table.get("loglevel").map_or(false, |r| r.val == "7")
        }, "kept 7");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：被双弃的条件键不再引发联动（裁决次序语义）。
    #[test]
    fn dropped_cond_does_not_link() {
        let mut set = CheckSet::new("unxstar-unx0591");

        // quiet 与 nokaslr 互斥 → 双弃；被弃的 quiet 不得再钳 loglevel。
        let t = EffectiveTable::merge(&[
            ("quiet", "true"),
            ("nokaslr", "true"),
            ("loglevel", "7"),
        ]);
        let v = arbitrate(&t);
        set.add("次序·冲突检出", v.conflicts.len() == 1, "1 conflict");
        set.add("次序·quiet 回落假", {
            v.table.get("quiet").map_or(false, |r| r.val == "false")
        }, "false");
        set.add("次序·被弃条件不联动", v.links.is_empty(), "no link");
        set.add("次序·loglevel 保持 7", {
            v.table.get("loglevel").map_or(false, |r| r.val == "7")
        }, "kept 7");

        // 纯缺省态：天然自洽，零冲突零联动。
        let v0 = arbitrate(&EffectiveTable::all_default());
        set.add("缺省态·零冲突", v0.clean(), "clean");
        set.add("缺省态·零联动", v0.links.is_empty(), "no link");

        // 指纹稳定。
        let f1 = v.fingerprint();
        let f2 = arbitrate(&EffectiveTable::merge(&[
            ("quiet", "true"),
            ("nokaslr", "true"),
            ("loglevel", "7"),
        ]))
        .fingerprint();
        set.add("指纹·同输入稳定", f1 == f2, "stable");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
