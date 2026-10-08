//! 深化层 · F574 系统更新摘要卡（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F574 节）：
//! ①「3-5 条人话要点」的 **条数约束引擎**——超 5 条按 安全>功能>修复
//!   优先级裁到上限（组内保原序）；不足 3 条经基础件 compose 门拒卡；
//! ②「人话转译（F132 差异表同源）」的 **转译分级器**——changelog 技术条
//!   目按关键词归三档，各有固定人话模板（模板即文案，不拼天书）；
//! ③「重启后首登展示（不是更新前吓你）」的 **展示时机状态机**——重启前
//!   禁展 → 首登展示窗 → 已读关闭终态，事件序乱跳即拒；
//! ④「要点挂差异表条目号」的 **同源对账账本**——挂号不重复、卡上要点
//!   全挂号且全在差异表内（一处一事实，漂移即红）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::upsummary::{SUMMARY_MAX, SUMMARY_MIN, Bullet, SummaryCard};

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 人话转译分级器
// ---------------------------------------------------------------------------

/// 要点三档（转译分级唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
    /// 安全（优先级最高——裁剪时最后动它）。
    Sec,
    /// 新功能。
    Feat,
    /// 问题修复。
    Fix,
}

/// changelog 技术条目 → 三档归类（关键词规则：安全词面最高优先）。
pub fn classify(raw: &str) -> Cat {
    let sec_marks = ["CVE", "漏洞", "安全"];
    let feat_marks = ["新增", "支持", "引入"];
    if sec_marks.iter().any(|m| raw.contains(m)) {
        Cat::Sec
    } else if feat_marks.iter().any(|m| raw.contains(m)) {
        Cat::Feat
    } else {
        Cat::Fix
    }
}

/// 三档人话模板（文案唯一源——用户读到的是人话，不是 changelog 天书）。
pub fn plain_wording(cat: Cat) -> &'static str {
    match cat {
        Cat::Sec => "本次修复了一个安全问题，建议尽快更新",
        Cat::Feat => "本次带来了一项新功能",
        Cat::Fix => "本次修好了一些已知问题",
    }
}

/// 技术条目 → 人话要点（挂 F132 差异表追踪号——同源对账锚）。
pub fn to_bullet(cat: Cat, trace_id: u32) -> Bullet {
    Bullet {
        text: String::from(plain_wording(cat)),
        trace_id,
    }
}

// ---------------------------------------------------------------------------
// 条数约束引擎
// ---------------------------------------------------------------------------

/// 成卡入口：超 SUMMARY_MAX 条按 安全>功能>修复 优先级裁到上限（各组
/// 内保持原顺序）；条数不足下限交基础件 compose 门拒绝（少条不成卡）。
pub fn trim_to_card(version: &str, entries: &[(Cat, u32)]) -> Option<SummaryCard> {
    let working: Vec<(Cat, u32)> = if entries.len() > SUMMARY_MAX {
        let mut kept: Vec<(Cat, u32)> = Vec::new();
        for cat in [Cat::Sec, Cat::Feat, Cat::Fix].iter() {
            for e in entries {
                if e.0 == *cat && kept.len() < SUMMARY_MAX {
                    kept.push(*e);
                }
            }
        }
        kept
    } else {
        entries.to_vec()
    };
    let bullets: Vec<Bullet> = working.iter().map(|(c, id)| to_bullet(*c, *id)).collect();
    SummaryCard::compose(version, bullets)
}

// ---------------------------------------------------------------------------
// 展示时机状态机
// ---------------------------------------------------------------------------

/// 展示时机三态：重启前禁展 → 首登展示窗口 → 已读关闭（终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    AwaitReboot,  // 更新尚未重启——禁展。
    FirstLogin,   // 重启后首次登录——展示窗口开。
    Closed,       // 已读关闭——终态不再出。
}

/// 展示门状态机（事件序乱跳即拒：未重启读卡、重启事件重放都收不下）。
pub struct ShowGate {
    state: GateState,
}

impl ShowGate {
    pub fn new() -> ShowGate {
        ShowGate { state: GateState::AwaitReboot }
    }

    pub fn state(&self) -> GateState {
        self.state
    }

    /// 展示窗口判定（唯一可展示态：FirstLogin）。
    pub fn can_show(&self) -> bool {
        self.state == GateState::FirstLogin
    }

    /// 重启完成事件（AwaitReboot → FirstLogin；重放拒）。
    pub fn reboot_done(&mut self) -> bool {
        if self.state == GateState::AwaitReboot {
            self.state = GateState::FirstLogin;
            true
        } else {
            false
        }
    }

    /// 已读关闭事件（FirstLogin → Closed 终态；premature 拒）。
    pub fn mark_read(&mut self) -> bool {
        if self.state == GateState::FirstLogin {
            self.state = GateState::Closed;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 同源对账账本
// ---------------------------------------------------------------------------

/// 同源对账账本：一处一事实——差异表条目号至多挂一条要点（重复挂号
/// 即红）；对账 = 卡上要点全部挂号、全部在差异表内、卡内无重号。
pub struct TraceLedger {
    claimed: Vec<u32>,
}

impl TraceLedger {
    pub fn new() -> TraceLedger {
        TraceLedger { claimed: Vec::new() }
    }

    /// 挂号（同一追踪号二次挂号拒）。
    pub fn claim(&mut self, trace_id: u32) -> bool {
        if self.claimed.contains(&trace_id) {
            return false;
        }
        self.claimed.push(trace_id);
        true
    }

    pub fn claimed(&self) -> &[u32] {
        &self.claimed
    }

    /// 卡 ↔ 账本 ↔ 差异表三方对账。
    pub fn reconcile(&self, card: &SummaryCard, diff_table: &[u32]) -> bool {
        if !card.same_source_with(diff_table) {
            return false; // 漂移号：要点不在差异表内。
        }
        for b in card.bullets.iter() {
            if !self.claimed.contains(&b.trace_id) {
                return false; // 账外物：要点没挂过号。
            }
        }
        for i in 0..card.bullets.len() {
            for j in (i + 1)..card.bullets.len() {
                if card.bullets[i].trace_id == card.bullets[j].trace_id {
                    return false; // 一号两挂：违一处一事实。
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f574_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 人话转译分级：安全词面 → Sec、新增/支持 → Feat、其余 → Fix。
    let c1 = classify("修复 CVE-2026-1234 远程漏洞");
    let c2 = classify("新增任务栏中键关闭窗口");
    let c3 = classify("修复蓝牙重连失败的问题");
    cs.add(
        "classify three tiers",
        c1 == Cat::Sec && c2 == Cat::Feat && c3 == Cat::Fix,
        "",
    );

    // 2) 三档人话模板齐备且互异（转译缺位 = 天书直达，拒）。
    let w = [plain_wording(Cat::Sec), plain_wording(Cat::Feat), plain_wording(Cat::Fix)];
    cs.add(
        "wording templates distinct",
        w[0] != w[1] && w[1] != w[2] && w.iter().all(|t| !t.is_empty()),
        "",
    );

    // 3) 条数约束：8 条（1 安全殿后 + 3 功能 + 4 修复）裁到 5 条——
    //    安全必留、末位修复先出局。
    let entries: Vec<(Cat, u32)> = alloc::vec![
        (Cat::Fix, 1),
        (Cat::Feat, 2),
        (Cat::Fix, 3),
        (Cat::Feat, 4),
        (Cat::Fix, 5),
        (Cat::Feat, 6),
        (Cat::Fix, 7),
        (Cat::Sec, 8),
    ];
    let card = trim_to_card("2.0", &entries);
    let trimmed_ok = match &card {
        Some(c) => {
            c.bullets.len() == SUMMARY_MAX
                && c.bullets.iter().any(|b| b.trace_id == 8)
                && !c.bullets.iter().any(|b| b.trace_id == 7)
        }
        None => false,
    };
    cs.add("trim to five sec kept", trimmed_ok, "");

    // 4) 少条不成卡：2 条拒；恰好 3 条收（3-5 红线两端）。
    let few = trim_to_card("2.0", &[(Cat::Fix, 1), (Cat::Fix, 2)]);
    let min_ok = trim_to_card("2.0", &[(Cat::Fix, 1), (Cat::Fix, 2), (Cat::Sec, 3)]).is_some();
    cs.add(
        "under min rejected min kept",
        few.is_none() && min_ok && SUMMARY_MIN == 3,
        "",
    );

    // 5) 展示时机状态机：重启前禁展 → 重启后首登开窗 → 已读关闭终态。
    let mut gate = ShowGate::new();
    let early = !gate.can_show() && gate.state() == GateState::AwaitReboot;
    let rebooted = gate.reboot_done();
    let window = gate.can_show() && gate.state() == GateState::FirstLogin;
    let read = gate.mark_read();
    cs.add(
        "gate three states",
        early && rebooted && window && read && !gate.can_show() && gate.state() == GateState::Closed,
        "",
    );

    // 6) 事件序诚实：未重启先读卡拒；重启事件重放拒。
    let mut g2 = ShowGate::new();
    let premature_read = !g2.mark_read();
    let _ = g2.reboot_done();
    let replay = !g2.reboot_done();
    cs.add("gate event order honest", premature_read && replay, "");

    // 7) 同源对账：挂号不重复；要点全挂号且全在差异表内。
    let mut led = TraceLedger::new();
    let d1 = led.claim(11);
    let d2 = led.claim(12);
    let d3 = led.claim(13);
    let dup = led.claim(11);
    let ok_card = trim_to_card("2.0", &[(Cat::Sec, 11), (Cat::Feat, 12), (Cat::Fix, 13)]);
    let table = [11u32, 12, 13];
    let recon_ok = matches!(&ok_card, Some(c) if led.reconcile(c, &table));
    cs.add("trace ledger reconcile", d1 && d2 && d3 && !dup && recon_ok, "");

    // 8) 对账诚实两面：漂移号（不在差异表）即红；一号两挂即红。
    let drift_card = trim_to_card("2.0", &[(Cat::Sec, 11), (Cat::Feat, 12), (Cat::Fix, 99)]);
    let drift_rejected = match &drift_card {
        Some(c) => !led.reconcile(c, &table),
        None => false,
    };
    let dup_card = trim_to_card("2.0", &[(Cat::Sec, 11), (Cat::Feat, 11), (Cat::Fix, 12)]);
    let dup_rejected = matches!(&dup_card, Some(c) if !led.reconcile(c, &table));
    cs.add("drift and duplicate rejected", drift_rejected && dup_rejected, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_empty_is_fix() {
        assert_eq!(classify(""), Cat::Fix);
    }

    #[test]
    fn no_trim_at_exact_max() {
        let e: Vec<(Cat, u32)> = (1..=5u32).map(|i| (Cat::Fix, i)).collect();
        let c = trim_to_card("v", &e);
        assert!(c.is_some());
        assert_eq!(c.unwrap().bullets.len(), 5);
    }

    #[test]
    fn ledger_claim_account() {
        let mut l = TraceLedger::new();
        assert!(l.claim(1));
        assert!(l.claim(2));
        assert!(!l.claim(1));
        assert_eq!(l.claimed().len(), 2);
    }
}
