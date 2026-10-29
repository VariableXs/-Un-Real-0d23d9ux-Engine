//! unx0590_defaults —— UNX-F0590 缺省值账（无参缺省表）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0590 | 缺省值账（无参缺省表） | 200 | 已深化 |
//!   | 缺省账齐（24 键 16 缺省 + 8 布尔假），无参注入 24/24 与缺省表一致 |
//!
//! 链路定位：上游 F0586 持有 16 行缺省表（整数 8 + 字符串 8，值冻结且
//!   逐行过自身域检），F0587 首现口径持有 24 键白名单（8 布尔自设计 +
//!   F0586 的 16 键逐字对齐）。本条冻结「键**从未出现**时生效集长什么样」：
//!   整数/字符串 16 键从 F0586 缺省表逐字取值（单位注记同步），布尔 8 键
//!   一律缺省假（开关语义：不提 = 不开，无隐式真）。本条产出**生效表
//!   v0**（全缺省态）与**生效表 v1**（缺省 + 显式覆盖合并），显式值
//!   永远压缺省值（同一键两源同现，来源账记 Explicit），下游 F0593
//!   快照封账消费的正是 v1。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（DEF_SEMANTICS）：
//!   1. 覆盖律：显式值永远压缺省值；来源三态 Explicit / Defaulted /
//!      Unknown（白名单外键不入生效表——F0584 弃置语义承接）；
//!   2. 布尔缺省假：8 布尔键缺省值统一 "false"（词形规范化归 F0587
//!      BOOL 词形表——本条只存 canonical 词形）；
//!   3. 对齐律：缺省值、单位注记与 F0586 DEFAULTS 逐字全等（编译期
//!      对拍 + 运行期 reconcile 双保险）；键序与 F0587 KEY_TABLE 一致；
//!   4. 幂等律：同一输入两次填充产出的生效表逐行全等（F0594 复扫的
//!      确定性锚点在本条先行自证）；
//!   5. 白名单外显式值：不入生效表、入弃置账（与 F0584 口径一致）。
//!
//! 组件清单：
//!   1. BOOL_KEYS          布尔 8 键（与 F0587 KEY_TABLE 布尔段逐字对齐）
//!   2. BOOL_DEFAULT       布尔缺省词形 "false"
//!   3. SourceKind         来源三态（Explicit/Defaulted/Unknown）
//!   4. EffectiveRow       生效表行（键 + 值 + 来源 + 单位注记）
//!   5. EffectiveTable     生效表 24 槽主体：填充/合并/弃置账/幂等
//!                        自证/报告/FNV 指纹
//!   6. reconcile_v0       全缺省态对拍（24 键逐行核缺省值）
//!   7. fnv64              FNV 指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）；
//!   与 F0586/F0587 的对齐用 crate 绝对路径，无相对路径脆弱性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx0586_valdom::DEFAULTS;

/// 模块版本。
pub const DF_VERSION: &str = "unx0590_defaults-v1";

/// 诊断段基址（0x4A40 段 · 本模块独占 · 择取实录：0x4A00/0x4A20 归 F0588/F0589）。
pub const DF_SEG_BASE: u16 = 0x4A40;
/// 诊断段顶。
pub const DF_SEG_TOP: u16 = 0x4A4F;

/// 键空间总键数（F0583 白名单口径）。
pub const KEY_N: usize = 24;
/// 缺省表行数（F0586 口径：整数 8 + 字符串 8）。
pub const DEF_N: usize = 16;
/// 布尔键数。
pub const BOOL_N: usize = 8;

/// 布尔缺省词形（canonical）。
pub const BOOL_DEFAULT: &str = "false";

// ═══════════════════════════════════════════════════════════════════════
// 1. BOOL_KEYS —— 布尔 8 键（与 F0587 KEY_TABLE 布尔段逐字对齐）
// ═══════════════════════════════════════════════════════════════════════

/// 布尔键闭集（顺序 = F0587 KEY_TABLE 布尔段顺序）。
pub const BOOL_KEYS: [&str; BOOL_N] = [
    "nokaslr",
    "quiet",
    "earlycon",
    "smp.off",
    "watchdog",
    "acpi.off",
    "efi.debug",
    "panic.halt",
];

// ═══════════════════════════════════════════════════════════════════════
// 2. SourceKind / EffectiveRow —— 来源三态与生效行
// ═══════════════════════════════════════════════════════════════════════

/// 值来源三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// 显式出现（首现值，F0587 口径）。
    Explicit,
    /// 缺省填充（键未出现）。
    Defaulted,
    /// 白名单外（不入生效表，只入弃置账）。
    Unknown,
}

impl SourceKind {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            SourceKind::Explicit => "显式",
            SourceKind::Defaulted => "缺省",
            SourceKind::Unknown => "弃置",
        }
    }
}

/// 生效表行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveRow {
    /// 键名。
    pub key: &'static str,
    /// 生效值（canonical 词形）。
    pub val: String,
    /// 来源。
    pub src: SourceKind,
    /// 单位注记（布尔键空串）。
    pub unit: &'static str,
}

// ═══════════════════════════════════════════════════════════════════════
// 3. EffectiveTable —— 生效表主体
// ═══════════════════════════════════════════════════════════════════════

/// 生效表（24 槽闭集）。
#[derive(Clone, Debug)]
pub struct EffectiveTable {
    /// 生效行（顺序 = BOOL_KEYS 后接 DEFAULTS 键序）。
    pub rows: Vec<EffectiveRow>,
    /// 白名单外弃置账（键 → 首现值截 32 字节）。
    pub discarded: Vec<(&'static str, String)>,
}

impl EffectiveTable {
    /// v0：全缺省态（无参注入——24 键全部 Defaulted）。
    pub fn all_default() -> Self {
        let mut rows = Vec::new();
        for k in BOOL_KEYS {
            rows.push(EffectiveRow {
                key: k,
                val: BOOL_DEFAULT.to_string(),
                src: SourceKind::Defaulted,
                unit: "",
            });
        }
        for d in DEFAULTS.iter() {
            rows.push(EffectiveRow {
                key: d.key,
                val: d.val.to_string(),
                src: SourceKind::Defaulted,
                unit: d.unit,
            });
        }
        EffectiveTable { rows, discarded: Vec::new() }
    }

    /// v1：缺省 + 显式覆盖合并（显式压缺省；白名单外入弃置账）。
    ///
    /// `appeared` 为 (键, 首现值) 序列（F0587 生效表直读）。
    pub fn merge(appeared: &[(&'static str, &str)]) -> Self {
        let mut t = Self::all_default();
        for (k, v) in appeared {
            let mut hit = false;
            for r in t.rows.iter_mut() {
                if r.key == *k {
                    r.val = v.to_string();
                    r.src = SourceKind::Explicit;
                    hit = true;
                    break;
                }
            }
            if !hit {
                // 白名单外：弃置账（值截 32 字节存证）。
                let head: String = v.chars().take(32).collect();
                t.discarded.push((k, head));
            }
        }
        t
    }

    /// 查行。
    pub fn get(&self, key: &str) -> Option<&EffectiveRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// 显式行数。
    pub fn explicit_n(&self) -> usize {
        self.rows.iter().filter(|r| r.src == SourceKind::Explicit).count()
    }

    /// 缺省行数。
    pub fn default_n(&self) -> usize {
        self.rows.iter().filter(|r| r.src == SourceKind::Defaulted).count()
    }

    /// 幂等自证：同输入两拍逐行全等（F0594 确定性锚点）。
    pub fn idempotent(appeared: &[(&'static str, &str)]) -> bool {
        let a = Self::merge(appeared);
        let b = Self::merge(appeared);
        a.rows == b.rows && a.discarded.len() == b.discarded.len()
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = format!(
            "缺省值账 · 生效 24 槽（显式 {} / 缺省 {}）· 弃置 {}",
            self.explicit_n(),
            self.default_n(),
            self.discarded.len()
        );
        for r in &self.rows {
            let line = format!(
                "\n  {} = {}（{}）",
                r.key,
                r.val,
                r.src.name()
            );
            s.push_str(&line);
        }
        for (k, v) in &self.discarded {
            s.push_str(&format!("\n  弃置 {} = {}（白名单外）", k, v));
        }
        s
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. reconcile_v0 —— 全缺省态对拍（24 键逐行核缺省值）
// ═══════════════════════════════════════════════════════════════════════

/// 全缺省态对拍：v0 的 24 行与（BOOL_KEYS×false + DEFAULTS×16）逐行全等。
///
/// 返回 (通过行数, 总行数, 不一致明细首条)。
pub fn reconcile_v0() -> (usize, usize, Option<String>) {
    let t = EffectiveTable::all_default();
    let mut ok = 0usize;
    let mut bad: Option<String> = None;
    for (i, r) in t.rows.iter().enumerate() {
        let expect_val: &str = if i < BOOL_N {
            BOOL_DEFAULT
        } else {
            &DEFAULTS[i - BOOL_N].val
        };
        let expect_unit: &str = if i < BOOL_N { "" } else { DEFAULTS[i - BOOL_N].unit };
        if r.val == expect_val && r.unit == expect_unit && r.src == SourceKind::Defaulted {
            ok += 1;
        } else if bad.is_none() {
            bad = Some(format!(
                "行 {} 键 {} 期望 {}/{} 实得 {}/{}",
                i, r.key, expect_val, expect_unit, r.val, r.unit
            ));
        }
    }
    (ok, t.rows.len(), bad)
}

/// 编译期对拍：DEFAULTS 行数、BOOL_KEYS 与键空间闭集守恒。
const _: () = {
    assert!(DEFAULTS.len() == DEF_N, "F0586 DEFAULTS 行数漂移");
    assert!(BOOL_KEYS.len() == BOOL_N, "BOOL_KEYS 键数漂移");
    assert!(DEF_N + BOOL_N == KEY_N, "24 键闭集守恒破坏");
};

// ═══════════════════════════════════════════════════════════════════════
// 5. fnv64
// ═══════════════════════════════════════════════════════════════════════

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

    /// 判据：缺省账齐——24 键 16 缺省 + 8 布尔假，v0 与缺省表一致。
    #[test]
    fn v0_matches_default_tables() {
        let mut set = CheckSet::new("unxstar-unx0590");

        let t = EffectiveTable::all_default();
        set.add("v0·24 槽守恒", t.rows.len() == 24, "24");
        set.add("v0·全缺省来源", t.default_n() == 24, "24 defaulted");
        set.add("v0·零显式", t.explicit_n() == 0, "0 explicit");
        set.add("v0·零弃置", t.discarded.is_empty(), "0 discarded");

        // 布尔 8 键全部缺省假。
        let mut bool_ok = true;
        for k in BOOL_KEYS {
            match t.get(k) {
                Some(r) => {
                    if r.val != BOOL_DEFAULT || r.src != SourceKind::Defaulted {
                        bool_ok = false;
                    }
                }
                None => bool_ok = false,
            }
        }
        set.add("v0·布尔 8 键全假", bool_ok, "false×8");

        // 整数样张：loglevel 缺省 6（级）。
        match t.get("loglevel") {
            Some(r) => set.add(
                "v0·loglevel=6",
                r.val == "6" && r.unit == "级",
                "6/级",
            ),
            None => set.add("v0·loglevel=6", false, "键缺"),
        }
        // 字符串样张：hostname 缺省 varix。
        match t.get("hostname") {
            Some(r) => set.add(
                "v0·hostname=varix",
                r.val == "varix" && r.unit == "",
                "varix",
            ),
            None => set.add("v0·hostname=varix", false, "键缺"),
        }

        // 逐行 reconcile。
        let (ok, total, bad) = reconcile_v0();
        set.add("v0·逐行对拍 24/24", ok == 24 && total == 24, "24/24");
        if let Some(b) = bad {
            set.add_dyn("v0·对拍零不一致", false, &b);
        } else {
            set.add("v0·对拍零不一致", true, "clean");
        }

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：无参注入 24/24——空输入 merge 后仍与 v0 全等（幂等锚点）。
    #[test]
    fn empty_inject_yields_full_defaults() {
        let mut set = CheckSet::new("unxstar-unx0590");

        let v0 = EffectiveTable::all_default();
        let v1 = EffectiveTable::merge(&[]);
        set.add("空注入·行数守恒", v1.rows.len() == 24, "24");
        set.add(
            "空注入·与 v0 逐行全等",
            v0.rows == v1.rows,
            "equal",
        );
        set.add("空注入·弃置账空", v1.discarded.is_empty(), "clean");
        set.add("空注入·幂等", EffectiveTable::idempotent(&[]), "idempotent");

        // 指纹：v0 与空注入 v1 指纹全等（结构等价的强断言）。
        set.add(
            "空注入·指纹全等",
            v0.fingerprint() == v1.fingerprint(),
            "fp equal",
        );

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：覆盖律 + 弃置账（显式压缺省；白名单外入弃置）。
    #[test]
    fn override_and_discard() {
        let mut set = CheckSet::new("unxstar-unx0590");

        let appeared: [(&'static str, &str); 4] = [
            ("loglevel", "3"),
            ("hostname", "vx-node1"),
            ("quiet", "true"),
            ("rogue.key", "junk"),
        ];
        let t = EffectiveTable::merge(&appeared);

        // 显式压缺省。
        match t.get("loglevel") {
            Some(r) => set.add(
                "覆盖·loglevel=3 显式",
                r.val == "3" && r.src == SourceKind::Explicit,
                "explicit 3",
            ),
            None => set.add("覆盖·loglevel=3 显式", false, "键缺"),
        }
        match t.get("hostname") {
            Some(r) => set.add(
                "覆盖·hostname 显式",
                r.val == "vx-node1" && r.src == SourceKind::Explicit,
                "explicit",
            ),
            None => set.add("覆盖·hostname 显式", false, "键缺"),
        }
        // 布尔显式真。
        match t.get("quiet") {
            Some(r) => set.add(
                "覆盖·quiet=true 显式",
                r.val == "true" && r.src == SourceKind::Explicit,
                "explicit true",
            ),
            None => set.add("覆盖·quiet=true 显式", false, "键缺"),
        }
        // 未出现键仍缺省。
        match t.get("uart.baud") {
            Some(r) => set.add(
                "未现·uart.baud 仍缺省",
                r.val == "115200" && r.src == SourceKind::Defaulted,
                "115200",
            ),
            None => set.add("未现·uart.baud 仍缺省", false, "键缺"),
        }
        // 白名单外弃置。
        set.add("弃置·rogue.key 入账", t.discarded.len() == 1, "1");
        set.add(
            "弃置·弃置不污染生效表",
            t.get("rogue.key").is_none(),
            "absent",
        );
        set.add("计数·显式 3", t.explicit_n() == 3, "3");
        set.add("计数·缺省 21", t.default_n() == 21, "21");

        // 幂等：同输入两拍全等。
        set.add("幂等·同输入稳定", EffectiveTable::idempotent(&appeared), "idem");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
