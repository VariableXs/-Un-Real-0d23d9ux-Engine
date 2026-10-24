//! UNX-F0256 参数变更审计账（0x3DD0 段）
//!
//! 锚点：UNX-F0256-J1 覆盖/还原/冲突三类变更全落账，账条数与
//! 事件数一致。
//! 域：CoRun·UNX-A1-B13（变更审计面）。
//!
//! 结构：变更事件闭集（覆盖 / 还原 / 冲突三型）+ 审计账（事件 →
//! 账条一一映射：序号/类型/键/旧值/新值/来源）+ 账守恒核（账条数
//! = 事件数恒等）+ 账回放核（按账序重放事件终态与直接终态一致）+
//! 注入核（越集事件 / 空账写 / 回放漂移必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3DD0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3DD0;
pub const AU_EV_OOB: u32 = SEG_BASE + 0x01; // 事件类型越集
pub const AU_LEDGER: u32 = SEG_BASE + 0x02; // 账守恒破缺
pub const AU_REPLAY: u32 = SEG_BASE + 0x03; // 回放终态漂移
pub const AU_SEALED: u32 = SEG_BASE + 0x04; // 账已封账拒写

/// 变更事件闭集。
pub const EV_OVERRIDE: u8 = 0; // 覆盖
pub const EV_RESTORE: u8 = 1; // 还原
pub const EV_CONFLICT: u8 = 2; // 冲突
pub const EV_COUNT: usize = 3;

/// 事件名。
pub fn ev_name(e: u8) -> &'static str {
    match e {
        EV_OVERRIDE => "覆盖",
        EV_RESTORE => "还原",
        EV_CONFLICT => "冲突",
        _ => "?",
    }
}

/// 审计账条。
#[derive(Clone)]
pub struct AuditRec {
    pub seq: u32,
    pub ev: u8,
    pub key: String,
    pub old_value: String,
    pub new_value: String,
    pub src: u8,
}

impl AuditRec {
    /// 账条渲染。
    pub fn render(&self) -> String {
        format!(
            "#{:04} {} [{}] {} -> {} (src={})",
            self.seq,
            ev_name(self.ev),
            self.key,
            self.old_value,
            self.new_value,
            self.src
        )
    }
}

/// 参数终态表（键 → 值）。
pub struct ParamState {
    pub map: Vec<(String, String)>,
}

impl ParamState {
    pub fn new() -> Self {
        ParamState { map: Vec::new() }
    }

    pub fn get(&self, k: &str) -> Option<&String> {
        self.map.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }

    /// 写终态（返回旧值）。
    pub fn set(&mut self, k: &str, v: &str) -> Option<String> {
        if let Some(slot) = self.map.iter_mut().find(|(key, _)| key == k) {
            let old = slot.1.clone();
            slot.1 = String::from(v);
            return Some(old);
        }
        self.map.push((String::from(k), String::from(v)));
        None
    }

    /// 终态指纹（键序折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for (k, v) in &self.map {
            for b in k.as_bytes().iter().chain(v.as_bytes()) {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

/// 变更审计器：事件流 + 账 + 终态镜像三轨同步。
pub struct ChangeAuditor {
    pub events: u32,
    pub ledger: Vec<AuditRec>,
    pub state: ParamState,
    pub sealed: bool,
}

impl ChangeAuditor {
    pub fn new() -> Self {
        ChangeAuditor {
            events: 0,
            ledger: Vec::new(),
            state: ParamState::new(),
            sealed: false,
        }
    }

    /// 记录变更事件（事件类型闭集守卫；旧值自动快照）。
    pub fn record(&mut self, ev: u8, key: &str, new_value: &str, src: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(AU_SEALED);
        }
        if (ev as usize) >= EV_COUNT {
            return Err(AU_EV_OOB);
        }
        let old_value = self.state.set(key, new_value).unwrap_or_default();
        self.ledger.push(AuditRec {
            seq: self.events,
            ev,
            key: String::from(key),
            old_value,
            new_value: String::from(new_value),
            src,
        });
        self.events += 1;
        Ok(())
    }

    /// 账守恒核：账条数 = 事件数；账序号严格连续。
    pub fn ledger_consistent(&self) -> bool {
        if self.ledger.len() as u32 != self.events {
            return false;
        }
        let mut i = 0u32;
        while (i as usize) < self.ledger.len() {
            if self.ledger[i as usize].seq != i {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 回放核：从空态按账序重放 → 终态指纹与现态一致。
    pub fn replay_matches(&self) -> bool {
        let mut st = ParamState::new();
        for r in &self.ledger {
            st.set(&r.key, &r.new_value);
        }
        st.fingerprint() == self.state.fingerprint()
    }

    /// 三类齐备核。
    pub fn all_kinds_present(&self) -> bool {
        let mut seen = [false; EV_COUNT];
        for r in &self.ledger {
            seen[r.ev as usize] = true;
        }
        seen.iter().all(|&b| b)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账渲染。
    pub fn render(&self) -> String {
        let mut out = format!("变更审计 事件数={} 账条数={}\n", self.events, self.ledger.len());
        for r in &self.ledger {
            let line = format!("  {}\n", r.render());
            out.push_str(&line);
        }
        out
    }
}

/// 标准夹具：三类事件各至少一次（覆盖→覆盖→还原→冲突）。
pub fn standard_auditor() -> ChangeAuditor {
    let mut a = ChangeAuditor::new();
    let _ = a.record(EV_OVERRIDE, "console", "tty0", 0);
    let _ = a.record(EV_OVERRIDE, "console", "ttyS0", 2);
    let _ = a.record(EV_RESTORE, "console", "tty0", 1);
    let _ = a.record(EV_CONFLICT, "loglevel", "6", 2);
    a
}

/// 注入核：6 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut a = ChangeAuditor::new();
    // 1) 事件越集拒
    if a.record(9, "k", "v", 0) == Err(AU_EV_OOB) {
        caught += 1;
    }
    // 2) 封账拒写
    a.seal();
    if a.record(EV_OVERRIDE, "k", "v", 0) == Err(AU_SEALED) {
        caught += 1;
    }
    // 3) 账守恒（事件数 = 账条数，序号连续）
    let mut b = standard_auditor();
    if b.ledger_consistent() && b.events == 4 {
        caught += 1;
    }
    // 4) 回放一致（重放终态 = 直接终态）
    if b.replay_matches() {
        caught += 1;
    }
    // 5) 回放漂移必抓：篡改账条新值 → 回放指纹偏离
    b.ledger[2].new_value = String::from("tampered");
    if !b.replay_matches() {
        caught += 1;
    }
    // 6) 三类齐备 + 渲染非空
    let c = standard_auditor();
    if c.all_kinds_present() && !c.render().is_empty() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx256_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0256");
    // J1-1 三类变更全落账
    let a = standard_auditor();
    let ok = a.all_kinds_present() && !a.render().is_empty();
    s.add("B13-48-三类全落账", ok, if ok { "" } else { "三类缺一" });
    // J1-2 账条数 = 事件数
    let a = standard_auditor();
    let ok = a.ledger_consistent() && a.ledger.len() == a.events as usize;
    s.add("B13-49-账条守恒", ok, if ok { "" } else { "守恒破缺" });
    // J1-3 回放终态一致
    let a = standard_auditor();
    let ok = a.replay_matches();
    s.add("B13-50-回放终态一致", ok, if ok { "" } else { "回放漂移" });
    // J1-4 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-51-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx256_all_checks_green() {
        let set = run_unx256_checks();
        assert!(set.all_passed(), "UNX-F0256 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx256_old_value_snapshot() {
        let mut a = ChangeAuditor::new();
        let _ = a.record(EV_OVERRIDE, "k", "v1", 0);
        let _ = a.record(EV_OVERRIDE, "k", "v2", 2);
        assert_eq!(a.ledger[1].old_value, "v1");
        assert_eq!(a.ledger[0].old_value, "");
    }
}
