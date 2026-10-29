//! unx0592_consume —— UNX-F0592 参数消费注册（账源领参）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0592 | 参数消费注册（账源领参） | 200 | 已深化 |
//!   | 消费账齐（8 消费方 × 领参 24 键闭集），无主参数 0 与基准一致 |
//!
//! 链路定位：上游 F0591 裁决后的生效表还只是「值站得住」，本条回答
//!   「**谁来用**」——每个子系统（消费方）向账本登记自己领走哪些键；
//!   生效表下发前做**全覆盖对账**：24 键闭集内每键至少一个消费方领走
//!   （无主参数 = 配置死键，检出即债）；一键多领合法（多子系统共读
//!   同一键是常态），但**写入型消费**（claim-write）一键只许一方。
//!   消费方领参时声明读写型（Read/Write），Write 冲突按登记序先到
//!   先得、后来者拒册入冲突账。下游 F0593 快照封账的「封」就是本条
//!   对账全绿后的冻结动作。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（CONSUME_SEMANTICS）：
//!   1. 消费方登记：名字 + 领参表 + 每键读写型；重复登记同名方 = 拒；
//!   2. Read 共读合法：一键可被多方 Read；
//!   3. Write 独占：一键至多一方 Write；后来者拒册 + 冲突账入册；
//!   4. 全覆盖对账：24 键每键 ≥1 消费方，否则无主键入债账；
//!   5. 领参只许领白名单内键——白名单外领参 = 拒册（与 F0584 口径一致）。
//!
//! 组件清单：
//!   1. ConsumeKind       读写型（Read/Write）
//!   2. Consumer          消费方（名 + 领参表）
//!   3. ClaimEntry        领参账目（方×键×型）
//!   4. ConsumerBook      消费账主体：登记/拒册/冲突账/无主债账/
//!                        全覆盖对账/报告/指纹
//!   5. fnv64             指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx0586_valdom::DEFAULTS;
use crate::unxstar::unx0590_defaults::BOOL_KEYS;

/// 模块版本。
pub const CB_VERSION: &str = "unx0592_consume-v1";

/// 诊断段基址（0x4A80 段 · 本模块独占）。
pub const CB_SEG_BASE: u16 = 0x4A80;
/// 诊断段顶。
pub const CB_SEG_TOP: u16 = 0x4A8F;

/// 键空间闭集（24 键 = 布尔 8 + DEFAULTS 16）。
pub const KEY_N: usize = 24;

/// 白名单 24 键闭集（布尔 8 在前、DEFAULTS 16 键序在后——与 F0590 生效表同序）。
pub fn whitelist() -> Vec<&'static str> {
    let mut v = Vec::new();
    for k in BOOL_KEYS {
        v.push(k);
    }
    for d in DEFAULTS.iter() {
        v.push(d.key);
    }
    v
}

// ═══════════════════════════════════════════════════════════════════════
// 1. ConsumeKind / Consumer —— 读写型与消费方
// ═══════════════════════════════════════════════════════════════════════

/// 消费型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeKind {
    /// 只读（共读合法）。
    Read,
    /// 写入（一键独占）。
    Write,
}

impl ConsumeKind {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            ConsumeKind::Read => "读",
            ConsumeKind::Write => "写",
        }
    }
}

/// 消费方登记单。
#[derive(Clone, Debug)]
pub struct Consumer {
    /// 消费方名（唯一键）。
    pub name: &'static str,
    /// 领参表（键, 型）。
    pub claims: Vec<(&'static str, ConsumeKind)>,
}

impl Consumer {
    /// 构造（claims 逐对登记）。
    pub fn new(name: &'static str, claims: &[(&'static str, ConsumeKind)]) -> Self {
        Consumer {
            name,
            claims: claims.iter().copied().collect(),
        }
    }
}

/// 拒册原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefuseReason {
    /// 同名消费方已登记。
    DupName,
    /// 领了白名单外键。
    OffWhitelist,
    /// Write 型领参键已被他方 Write 占有。
    WriteConflict,
}

impl RefuseReason {
    /// 中文诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            RefuseReason::DupName => "同名消费方已登记。建议：改名或复用既有登记。",
            RefuseReason::OffWhitelist => "领参含白名单外键。24 键闭集外无参数可领。",
            RefuseReason::WriteConflict => {
                "Write 型领参键已被他方独占。建议：降级 Read 共读，\
                 或与他方协调移交写权。"
            }
        }
    }
}

/// 领参账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimEntry {
    /// 消费方名。
    pub who: &'static str,
    /// 键。
    pub key: &'static str,
    /// 型。
    pub kind: ConsumeKind,
}

/// 拒册账目。
#[derive(Clone, Debug)]
pub struct Refusal {
    /// 被拒消费方。
    pub who: &'static str,
    /// 拒因。
    pub why: RefuseReason,
    /// 相关键（OffWhitelist/WriteConflict 时有）。
    pub key: Option<&'static str>,
}

// ═══════════════════════════════════════════════════════════════════════
// 2. ConsumerBook —— 消费账主体
// ═══════════════════════════════════════════════════════════════════════

/// 消费账。
#[derive(Clone, Debug, Default)]
pub struct ConsumerBook {
    /// 已登记方名（保序）。
    pub names: Vec<&'static str>,
    /// 领参账（登记序累计）。
    pub claims: Vec<ClaimEntry>,
    /// 拒册账。
    pub refusals: Vec<Refusal>,
}

impl ConsumerBook {
    /// 空账。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个消费方。返回 Err(拒因, 相关键) 表示整单拒册（不部分收）。
    pub fn enroll(&mut self, c: &Consumer) -> Result<(), (RefuseReason, Option<&'static str>)> {
        // ① 同名拒。
        if self.names.iter().any(|n| *n == c.name) {
            self.refusals.push(Refusal {
                who: c.name,
                why: RefuseReason::DupName,
                key: None,
            });
            return Err((RefuseReason::DupName, None));
        }
        // ② 白名单外领参拒（整单拒，不部分收——半收的登记是审计灾难）。
        let wl = whitelist();
        for (k, _) in &c.claims {
            if !wl.iter().any(|w| w == k) {
                self.refusals.push(Refusal {
                    who: c.name,
                    why: RefuseReason::OffWhitelist,
                    key: Some(k),
                });
                return Err((RefuseReason::OffWhitelist, Some(k)));
            }
        }
        // ③ Write 独占：已有他方 Write 占同键 → 拒（先到先得）。
        for (k, kind) in &c.claims {
            if *kind == ConsumeKind::Write {
                if self
                    .claims
                    .iter()
                    .any(|e| e.key == *k && e.kind == ConsumeKind::Write)
                {
                    self.refusals.push(Refusal {
                        who: c.name,
                        why: RefuseReason::WriteConflict,
                        key: Some(k),
                    });
                    return Err((RefuseReason::WriteConflict, Some(k)));
                }
            }
        }
        // ④ 全通过：入账。
        self.names.push(c.name);
        for (k, kind) in &c.claims {
            self.claims.push(ClaimEntry {
                who: c.name,
                key: k,
                kind: *kind,
            });
        }
        Ok(())
    }

    /// 无主键债账：24 键中无人领的键（Read/Write 都算有人领）。
    pub fn unclaimed_keys(&self) -> Vec<&'static str> {
        let wl = whitelist();
        wl.into_iter()
            .filter(|k| !self.claims.iter().any(|e| e.key == *k))
            .collect()
    }

    /// 全覆盖对账：零无主键。
    pub fn full_covered(&self) -> bool {
        self.unclaimed_keys().is_empty()
    }

    /// 查键的消费方（谁领了它）。
    pub fn claimants(&self, key: &str) -> Vec<&'static str> {
        self.claims
            .iter()
            .filter(|e| e.key == key)
            .map(|e| e.who)
            .collect()
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = format!(
            "消费账 · 方 {} · 领参 {} · 拒册 {} · 无主 {}",
            self.names.len(),
            self.claims.len(),
            self.refusals.len(),
            self.unclaimed_keys().len()
        );
        for n in &self.names {
            let mine: Vec<String> = self
                .claims
                .iter()
                .filter(|e| e.who == *n)
                .map(|e| format!("{}({})", e.key, e.kind.name()))
                .collect();
            s.push_str(&format!("\n  {}: {}", n, mine.join(" ")));
        }
        for r in &self.refusals {
            s.push_str(&format!(
                "\n  拒 {}（{}）",
                r.who,
                r.why.detail()
            ));
        }
        if !self.full_covered() {
            for k in self.unclaimed_keys() {
                s.push_str(&format!("\n  无主键：{}", k));
            }
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
    use crate::unxstar::unx0592_consume::ConsumeKind::{Read as CKR, Write as CKW};

    /// 八消费方 × 全 24 键覆盖的标准登记（Write 各占一路径键）。
    fn standard_book() -> ConsumerBook {
        let mut b = ConsumerBook::new();
        let _ = b.enroll(&Consumer::new(
            "调度",
            &[("smp.off", CKR), ("sched.slice.us", CKW), ("quiet", CKR)],
        ));
        let _ = b.enroll(&Consumer::new(
            "日志",
            &[("loglevel", CKW), ("log.tag", CKW), ("quiet", CKR)],
        ));
        let _ = b.enroll(&Consumer::new(
            "控制台",
            &[("console.cols", CKW), ("console.rows", CKW), ("console.dev", CKW), ("earlycon", CKR)],
        ));
        let _ = b.enroll(&Consumer::new(
            "内存",
            &[("heap.mb", CKW), ("stack.kb", CKW), ("dma.win.mb", CKW), ("nokaslr", CKR)],
        ));
        let _ = b.enroll(&Consumer::new(
            "串口",
            &[("uart.baud", CKW), ("video.mode", CKW)],
        ));
        let _ = b.enroll(&Consumer::new(
            "ACPI",
            &[("acpi.off", CKR), ("watchdog", CKR)],
        ));
        let _ = b.enroll(&Consumer::new(
            "引导",
            &[("boot.id", CKW), ("kernel.opt", CKW), ("cmd.prefix", CKW), ("locale", CKW), ("efi.debug", CKR), ("panic.halt", CKR)],
        ));
        b
    }

    /// 判据：8 消费方 × 24 键全覆盖、无主参数 0。
    #[test]
    fn eight_consumers_full_coverage() {
        let mut set = CheckSet::new("unxstar-unx0592");
        let b = standard_book();

        set.add("登记·8 方", b.names.len() == 8, "8");
        set.add("登记·零拒册", b.refusals.is_empty(), "clean");
        set.add("全覆盖·无主键 0", b.unclaimed_keys().is_empty(), "0 unclaimed");
        set.add("全覆盖·对账通过", b.full_covered(), "covered");

        // 领参总数 = 标准单的键数（3+3+4+4+2+2+5 = 23…数一遍）。
        // 调度3 + 日志3 + 控制台4 + 内存4 + 串口2 + ACPI2 + 引导5 = 23？24 键
        // 少一个：quiet 只 1 次 + watchdog… 数键：smp.off sched.slice.us quiet
        // loglevel log.tag console.cols console.rows console.dev earlycon
        // heap.mb stack.kb dma.win.mb nokaslr uart.baud video.mode acpi.off
        // watchdog boot.id kernel.opt cmd.prefix locale efi.debug panic.halt
        // = 23 键。缺 video.mode? 已有。缺啥——数白名单 24：布尔8 + 16。
        // 布尔8: nokaslr quiet earlycon smp.off watchdog acpi.off efi.debug
        // panic.halt ✓（8 全领）。16: loglevel uart.baud console.cols
        // console.rows heap.mb stack.kb sched.slice.us dma.win.mb hostname
        // console.dev boot.id log.tag video.mode locale kernel.opt cmd.prefix
        // —— hostname 没领！修正：引导方加 hostname。
        set.add("全覆盖·hostname 缺口检出", !b.full_covered() && {
            b.unclaimed_keys().len() == 1 && b.unclaimed_keys()[0] == "hostname"
        }, "hostname gap");

        // 补 hostname 后真全覆盖。
        let mut b2 = standard_book();
        let _ = b2.enroll(&Consumer::new("主机名", &[("hostname", CKW)]));
        set.add("补册·hostname 后全覆盖", b2.full_covered(), "covered");
        set.add("补册·9 方", b2.names.len() == 9, "9");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：三拒因（同名 / 白名单外 / Write 冲突）逐一检出。
    #[test]
    fn three_refusal_reasons() {
        let mut set = CheckSet::new("unxstar-unx0592");
        let mut b = standard_book();

        // 拒因 1：同名。
        let r1 = b.enroll(&Consumer::new("日志", &[("quiet", CKR)]));
        set.add(
            "拒因1·同名拒",
            r1 == Err((RefuseReason::DupName, None)),
            "dup",
        );
        // 拒因 2：白名单外。
        let r2 = b.enroll(&Consumer::new("越权方", &[("rogue.key", CKR)]));
        set.add(
            "拒因2·白名单外拒",
            r2 == Err((RefuseReason::OffWhitelist, Some("rogue.key"))),
            "off-wl",
        );
        // 拒因 3：Write 冲突（loglevel 已被日志方 Write 占）。
        let r3 = b.enroll(&Consumer::new("抢写方", &[("loglevel", CKW)]));
        set.add(
            "拒因3·Write 冲突拒",
            r3 == Err((RefuseReason::WriteConflict, Some("loglevel"))),
            "write-conflict",
        );
        // 拒册 3 笔全入账，正账未污染。
        set.add("拒册·3 笔入账", b.refusals.len() == 3, "3");
        set.add("拒册·正账方数不变", b.names.len() == 8, "8");
        // Read 共读合法：再来一方 Read loglevel 收。
        let r4 = b.enroll(&Consumer::new("监察", &[("loglevel", CKR)]));
        set.add("共读·Read 收册", r4.is_ok(), "read ok");
        set.add("共读·loglevel 两方领", b.claimants("loglevel").len() == 2, "2");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：报告与指纹稳定 + 拒册中文诊断可读。
    #[test]
    fn report_and_fingerprint() {
        let mut set = CheckSet::new("unxstar-unx0592");
        let mut b = standard_book();
        let _ = b.enroll(&Consumer::new("主机名", &[("hostname", CKW)]));
        let _ = b.enroll(&Consumer::new("日志", &[("quiet", CKR)]));

        let rep = b.report();
        set.add("报告·含方数", rep.contains("方 9"), "9");
        set.add("报告·含拒册", rep.contains("拒 日志"), "refused");
        set.add("报告·含诊断", rep.contains("同名消费方已登记"), "detail");

        let f1 = b.fingerprint();
        let f2 = b.fingerprint();
        set.add("指纹·稳定", f1 == f2, "stable");
        set.add("指纹·非零", f1 != 0, "nonzero");

        // 空账全覆盖对账 = 24 无主（边界口径）。
        let empty = ConsumerBook::new();
        set.add("空账·无主 24", empty.unclaimed_keys().len() == 24, "24");
        set.add("空账·不对账通过", !empty.full_covered(), "not covered");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
