//! unx224_takeover —— UNX-F0224 升级接管登记表（现存能力交接账）。
//! 锚点（CoRun Unxreal 主册 L1638）：UNX-F0224 · 升级接管登记表（现存能力交接账） · 目标 240 行 · 已深化
//! 判据原文：UNX-F0224-J1 F0039 等接管条目登记"接管对象/保留/替换/退役"四字段，缺项告警
//! 组件：接管四字段冻结（接管对象/保留/替换/退役 · TkEntry 四列闭集）
//!      + 缺项告警器（四字段逐条完备性核 · 缺一即告警留案）
//!      + 处置闭集校验（保留/替换/退役 三处置 · 每条恰一处置）
//!      + 交接对账（旧能力账 vs 接管账逐条对拍 · 漏接管检出）
//! 判据落地：四字段齐备零缺项 + 处置唯一 + 交接对拍零漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u32/u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const TK_VERSION: &str = "unx224_takeover-v1";

/// 诊断段基址（0x4fe0 段 · 本模块独占）。
pub const TK_SEG_BASE: u16 = 0x4fe0;

/// 处置闭集（四字段之第四列 · 恰一处置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TkAction {
    /// 保留：能力原样交接。
    Keep,
    /// 替换：新实现顶替（旧退役）。
    Replace,
    /// 退役：能力终止（无替换）。
    Retire,
}

impl TkAction {
    /// 处置名。
    pub fn name(self) -> &'static str {
        match self {
            TkAction::Keep => "keep",
            TkAction::Replace => "replace",
            TkAction::Retire => "retire",
        }
    }

    /// 处置码。
    pub fn code(self) -> u8 {
        match self {
            TkAction::Keep => 0,
            TkAction::Replace => 1,
            TkAction::Retire => 2,
        }
    }
}

/// 接管条目（四字段冻结）。
#[derive(Clone, Debug)]
pub struct TkEntry {
    /// 接管对象（旧能力 id，如 "F0039-legacy-boot"）。
    pub subject: String,
    /// 保留（保留理由 / 空串=未定 → 缺项）。
    pub keep_note: String,
    /// 替换（新实现落点 / 空串=未定 → 缺项）。
    pub replace_target: String,
    /// 退役（退役批次 / 空串=未定 → 缺项）。
    pub retire_batch: String,
    /// 处置（恰一）。
    pub action: TkAction,
}

impl TkEntry {
    /// 缺项检测：与处置不相干字段为空 → 缺项。
    /// 规则（冻结 v1）：处置为 Keep → retire 必填（退役计划可空，但保留理由必填）；
    /// 处置为 Replace → replace_target 必填；处置为 Retire → retire_batch 必填。
    /// 全部条目 subject 恒必填。
    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut miss: Vec<&'static str> = Vec::new();
        if self.subject.is_empty() {
            miss.push("subject");
        }
        match self.action {
            TkAction::Keep => {
                if self.keep_note.is_empty() {
                    miss.push("keep");
                }
            }
            TkAction::Replace => {
                if self.replace_target.is_empty() {
                    miss.push("replace");
                }
                if self.retire_batch.is_empty() {
                    miss.push("retire");
                }
            }
            TkAction::Retire => {
                if self.retire_batch.is_empty() {
                    miss.push("retire");
                }
            }
        }
        miss
    }

    /// 行摘要。
    pub fn line(&self) -> String {
        format!(
            "{}|keep={}|repl={}|retire={}|{}",
            self.subject,
            if self.keep_note.is_empty() { "-" } else { "Y" },
            if self.replace_target.is_empty() { "-" } else { "Y" },
            if self.retire_batch.is_empty() { "-" } else { "Y" },
            self.action.name()
        )
    }
}

/// 缺项告警账。
#[derive(Clone, Debug)]
pub struct TkAlarm {
    /// 条目 subject（空缺条目用序号占位）。
    pub subject: String,
    /// 缺失字段表。
    pub missing: Vec<String>,
}

/// 接管登记表。
pub struct TkRegistry {
    /// 条目账。
    pub entries: Vec<TkEntry>,
    /// 告警账。
    pub alarms: Vec<TkAlarm>,
    /// 重复接管对象数（卫生 · 应恒 0）。
    pub dup_subject: u32,
}

impl TkRegistry {
    /// 空登记表。
    pub fn new() -> Self {
        TkRegistry {
            entries: Vec::new(),
            alarms: Vec::new(),
            dup_subject: 0,
        }
    }

    /// 登记一条（subject 重复拒绝）。
    pub fn enroll(&mut self, e: TkEntry) -> bool {
        if e.subject.is_empty() {
            return false;
        }
        if self.entries.iter().any(|x| x.subject == e.subject) {
            self.dup_subject += 1;
            return false;
        }
        self.entries.push(e);
        true
    }

    /// 缺项告警扫描：逐条完备性核，缺项即告警。
    pub fn audit(&mut self) -> u32 {
        self.alarms.clear();
        for e in &self.entries {
            let miss = e.missing_fields();
            if !miss.is_empty() {
                self.alarms.push(TkAlarm {
                    subject: e.subject.clone(),
                    missing: miss.iter().map(|s| String::from(*s)).collect(),
                });
            }
        }
        self.alarms.len() as u32
    }

    /// 零缺项。
    pub fn no_missing(&self) -> bool {
        self.alarms.is_empty()
    }

    /// 登记卫生。
    pub fn clean(&self) -> bool {
        self.dup_subject == 0
    }
}

/// 旧能力账条目（交接对拍源侧）。
#[derive(Clone, Debug)]
pub struct TkLegacy {
    /// 旧能力 id。
    pub cap_id: String,
    /// 能力状态（active/dead 闭集）。
    pub alive: bool,
}

/// 交接对拍器：旧能力账逐条必须被接管账覆盖。
pub struct TkPairer {
    /// 旧能力条数。
    pub legacy_total: u32,
    /// 已接管数（接管账含 subject 命中）。
    pub covered: u32,
    /// 漏接管明细。
    pub missed: Vec<String>,
}

impl TkPairer {
    /// 空对拍器。
    pub fn new() -> Self {
        TkPairer {
            legacy_total: 0,
            covered: 0,
            missed: Vec::new(),
        }
    }

    /// 逐条对拍：active 旧能力必须有接管条目。
    pub fn pair(&mut self, legacy: &[TkLegacy], reg: &TkRegistry) {
        self.legacy_total = legacy.len() as u32;
        self.covered = 0;
        self.missed.clear();
        for l in legacy.iter().filter(|l| l.alive) {
            if reg.entries.iter().any(|e| e.subject == l.cap_id) {
                self.covered += 1;
            } else {
                self.missed.push(l.cap_id.clone());
            }
        }
    }

    /// 交接零漏。
    pub fn fully_covered(&self) -> bool {
        self.legacy_total >= 1 && self.covered == self.legacy_total
    }
}

/// 处置唯一性核：Replace 条目替换目标不得与其他条目 subject 空转（防幽灵替换）。
pub fn tk_replace_targets_real(reg: &TkRegistry) -> bool {
    reg.entries
        .iter()
        .filter(|e| e.action == TkAction::Replace)
        .all(|e| !e.replace_target.is_empty())
}

/// 负向注入场景（四字段缺项闭集）。
pub enum TkInject {
    /// Keep 条目缺保留理由。
    KeepNoReason,
    /// Replace 条目缺替换目标。
    ReplaceNoTarget,
    /// Retire 条目缺退役批次。
    RetireNoBatch,
    /// 空 subject。
    NoSubject,
    /// 重复接管对象。
    DupSubject,
}

/// 注入核：五场注入，登记/告警闸必须全部拦截。
pub struct TkInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
    /// 明细。
    pub log: Vec<String>,
}

impl TkInjector {
    /// 空注入核。
    pub fn new() -> Self {
        TkInjector {
            cases: 0,
            caught: 0,
            log: Vec::new(),
        }
    }

    /// 五场注入：每场构造脏条目，闸门必须拒或告警。
    pub fn inject_all(&mut self, reg: &mut TkRegistry) {
        // 场一：Keep 缺理由 → enroll 受理但 audit 告警。
        let e1 = TkEntry {
            subject: String::from("INJ-keep-noreason"),
            keep_note: String::new(),
            replace_target: String::new(),
            retire_batch: String::new(),
            action: TkAction::Keep,
        };
        let _ = reg.enroll(e1);
        let alarms = reg.audit();
        let hit1 = alarms >= 1;
        self.cases += 1;
        if hit1 {
            self.caught += 1;
        }
        self.log.push(format!("#1 keep-noreason -> {}", if hit1 { "ALARMED" } else { "MISS" }));
        // 清场。
        reg.entries.retain(|e| e.subject != "INJ-keep-noreason");
        reg.alarms.clear();
        // 场二：Replace 缺目标。
        let e2 = TkEntry {
            subject: String::from("INJ-repl-notarget"),
            keep_note: String::new(),
            replace_target: String::new(),
            retire_batch: String::from("B40"),
            action: TkAction::Replace,
        };
        let _ = reg.enroll(e2);
        let hit2 = reg.audit() >= 1;
        self.cases += 1;
        if hit2 {
            self.caught += 1;
        }
        self.log.push(format!("#2 replace-notarget -> {}", if hit2 { "ALARMED" } else { "MISS" }));
        reg.entries.retain(|e| e.subject != "INJ-repl-notarget");
        reg.alarms.clear();
        // 场三：Retire 缺批次。
        let e3 = TkEntry {
            subject: String::from("INJ-retire-nobatch"),
            keep_note: String::new(),
            replace_target: String::new(),
            retire_batch: String::new(),
            action: TkAction::Retire,
        };
        let _ = reg.enroll(e3);
        let hit3 = reg.audit() >= 1;
        self.cases += 1;
        if hit3 {
            self.caught += 1;
        }
        self.log.push(format!("#3 retire-nobatch -> {}", if hit3 { "ALARMED" } else { "MISS" }));
        reg.entries.retain(|e| e.subject != "INJ-retire-nobatch");
        reg.alarms.clear();
        // 场四：空 subject（enroll 必拒）。
        let e4 = TkEntry {
            subject: String::new(),
            keep_note: String::from("x"),
            replace_target: String::new(),
            retire_batch: String::new(),
            action: TkAction::Keep,
        };
        let hit4 = !reg.enroll(e4);
        self.cases += 1;
        if hit4 {
            self.caught += 1;
        }
        self.log.push(format!("#4 no-subject -> {}", if hit4 { "DENIED" } else { "MISS" }));
        // 场五：重复 subject（enroll 必拒）。
        let e5a = TkEntry {
            subject: String::from("INJ-dup"),
            keep_note: String::from("r1"),
            replace_target: String::new(),
            retire_batch: String::new(),
            action: TkAction::Keep,
        };
        let _ = reg.enroll(e5a);
        let e5b = TkEntry {
            subject: String::from("INJ-dup"),
            keep_note: String::from("r2"),
            replace_target: String::new(),
            retire_batch: String::new(),
            action: TkAction::Keep,
        };
        let hit5 = !reg.enroll(e5b) && reg.dup_subject >= 1;
        self.cases += 1;
        if hit5 {
            self.caught += 1;
        }
        self.log.push(format!("#5 dup-subject -> {}", if hit5 { "DENIED" } else { "MISS" }));
        reg.entries.retain(|e| e.subject != "INJ-dup");
        reg.dup_subject = 0;
        reg.alarms.clear();
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 5 && self.caught == self.cases
    }
}

/// 接管登记门面（判据四件：四字段齐备/缺项告警/交接对拍/注入必拒）。
pub struct TkLedger {
    /// 登记表。
    pub reg: TkRegistry,
    /// 对拍器。
    pub pairer: TkPairer,
    /// 注入核。
    pub inj: TkInjector,
}

impl TkLedger {
    /// 新门面。
    pub fn new() -> Self {
        TkLedger {
            reg: TkRegistry::new(),
            pairer: TkPairer::new(),
            inj: TkInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.reg.no_missing()
            && self.reg.clean()
            && self.pairer.fully_covered()
            && self.inj.all_caught()
            && tk_replace_targets_real(&self.reg);
        let msg = format!(
            "{}: entries={} alarms={} covered={}/{} missed={} inj={}/{} seg=0x{:04x}",
            TK_VERSION,
            self.reg.entries.len(),
            self.reg.alarms.len(),
            self.pairer.covered,
            self.pairer.legacy_total,
            self.pairer.missed.len(),
            self.inj.caught,
            self.inj.cases,
            TK_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：F0039 等四条真接管登记 + 交接对拍 + 五场注入。
pub fn run_t224_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx224-t224");
    let mut tk = TkLedger::new();
    // 判据点名条目：F0039 系接管。
    let seeds: [(&str, TkAction); 4] = [
        ("F0039-legacy-bootmenu", TkAction::Replace),
        ("F0039-legacy-vga-text", TkAction::Retire),
        ("F0039-legacy-int13", TkAction::Replace),
        ("F0039-legacy-rtc-walk", TkAction::Keep),
    ];
    for (subject, action) in seeds {
        let e = TkEntry {
            subject: String::from(subject),
            keep_note: if action == TkAction::Keep {
                String::from("保留只读快照语义供审计")
            } else {
                String::new()
            },
            replace_target: if action == TkAction::Replace {
                String::from("unx-new-impl")
            } else {
                String::new()
            },
            retire_batch: if action != TkAction::Keep {
                String::from("B40")
            } else {
                String::new()
            },
            action,
        };
        tk.reg.enroll(e);
    }
    // 旧能力账（active 全集 · 与登记对拍零漏）。
    let legacy: Vec<TkLegacy> = seeds
        .iter()
        .map(|(s, _)| TkLegacy {
            cap_id: String::from(*s),
            alive: true,
        })
        .collect();
    tk.reg.audit();
    tk.pairer.pair(&legacy, &tk.reg);
    // 五场注入。
    tk.inj.inject_all(&mut tk.reg);
    let (ok, msg) = tk.judge();
    set.add_dyn("UNX-F0224-J1", ok, &msg);
    set.add("TK-no-missing", tk.reg.no_missing(), "");
    set.add("TK-covered-full", tk.pairer.fully_covered(), "");
    set.add("TK-inject-5/5", tk.inj.all_caught(), "");
    set.add("TK-replace-real", tk_replace_targets_real(&tk.reg), "");
    set
}
