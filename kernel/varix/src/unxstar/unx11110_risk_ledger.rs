//! UNX-F11110 风险台账总核（R-C4-001/002/003 终态）（0x1f590 段）
//!
//! 锚点：UNX-F11110-J1 风险项逐条终核对平（10/10），终态处置零在途（10/10）。
//! 域：CoRun·UNX-C4-B36（风险台账）。
//!
//! 结构：风险台账（RiskLedger：立账 → 缓解 → 关闭 → 零在途断言）+ 单项
//! （RiskItem：编号 + 严重度 + 状态机 + 责任人）+ 冻结根 + 注入核（越级关闭/
//! 重复关闭/非法严重度/非 R-C4 号四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f590 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f590;
pub const RK_ID: u32 = SEG_BASE + 0x01; // 风险号非法
pub const RK_DUP: u32 = SEG_BASE + 0x02; // 风险号重号
pub const RK_SEV: u32 = SEG_BASE + 0x03; // 严重度越界
pub const RK_OWNER: u32 = SEG_BASE + 0x04; // 责任人空
pub const RK_STATE: u32 = SEG_BASE + 0x05; // 状态机越级
pub const RK_INFLIGHT: u32 = SEG_BASE + 0x06; // 终态仍有在途
pub const RK_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const RK_OVER: u32 = SEG_BASE + 0x08; // 超定容

/// 风险项数（R-C4-001/002/003 三条）。
pub const NEEDED: usize = 3;
/// 台账定容。
pub const CAP: usize = 8;
/// 风险号前缀。
pub const RISK_PREFIX: &str = "R-C4-";
/// 状态：在途 / 已缓解 / 已关闭。
pub const ST_OPEN: u8 = 0;
pub const ST_MITIGATED: u8 = 1;
pub const ST_CLOSED: u8 = 2;
/// 台账标（fold 种子）。
pub const RK_TAG: u64 = 0x1f5_9090;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单项风险：编号 + 严重度 + 状态 + 责任人。
#[derive(Clone, Debug, PartialEq)]
pub struct RiskItem {
    pub id: String,
    pub severity: u32,
    pub state: u8,
    pub owner: String,
}

impl RiskItem {
    pub fn state_name(&self) -> &'static str {
        match self.state {
            ST_OPEN => "OPEN",
            ST_MITIGATED => "MIT",
            ST_CLOSED => "CLOSED",
            _ => "?",
        }
    }

    pub fn render(&self) -> String {
        format!("{}|S{}|{}|{}", self.id, self.severity, self.state_name(), self.owner)
    }
}

/// 风险台账。
#[derive(Clone, Debug)]
pub struct RiskLedger {
    pub items: Vec<RiskItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RiskLedger {
    pub fn new() -> RiskLedger {
        RiskLedger { items: Vec::new(), frozen: false, root: None }
    }

    /// 立账：前缀不符/重号/严重度越界/责任人空/超容皆拒。
    pub fn file(&mut self, id: &str, severity: u32, owner: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(RK_FROZEN);
        }
        if !id.starts_with(RISK_PREFIX) || id.len() <= RISK_PREFIX.len() {
            return Err(RK_ID);
        }
        if self.items.iter().any(|r| r.id == id) {
            return Err(RK_DUP);
        }
        if severity == 0 || severity > 3 {
            return Err(RK_SEV);
        }
        if owner.is_empty() {
            return Err(RK_OWNER);
        }
        if self.items.len() >= CAP {
            return Err(RK_OVER);
        }
        self.items.push(RiskItem {
            id: id.to_string(),
            severity,
            state: ST_OPEN,
            owner: owner.to_string(),
        });
        Ok(())
    }

    fn find(&mut self, id: &str) -> Option<&mut RiskItem> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].id == id {
                return Some(&mut self.items[k]);
            }
            k += 1;
        }
        None
    }

    /// 缓解：仅 OPEN 可缓解。
    pub fn mitigate(&mut self, id: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(RK_FROZEN);
        }
        let r = self.find(id).ok_or(RK_ID)?;
        if r.state != ST_OPEN {
            return Err(RK_STATE);
        }
        r.state = ST_MITIGATED;
        Ok(())
    }

    /// 关闭：仅 MITIGATED 可关闭，越级拒。
    pub fn close(&mut self, id: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(RK_FROZEN);
        }
        let r = self.find(id).ok_or(RK_ID)?;
        if r.state != ST_MITIGATED {
            return Err(RK_STATE);
        }
        r.state = ST_CLOSED;
        Ok(())
    }

    /// 在途枚举：未到终态的项。
    pub fn inflight(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].state != ST_CLOSED {
                v.push(self.items[k].id.clone());
            }
            k += 1;
        }
        v
    }

    pub fn closed_count(&self) -> usize {
        self.items.len().saturating_sub(self.inflight().len())
    }

    /// 终态对平：三条风险全关闭且零在途。
    pub fn closed(&self) -> bool {
        self.items.len() == NEEDED && self.inflight().is_empty() && self.closed_count() == NEEDED
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 台账指纹：编号 + 严重度 + 状态 + 责任人链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RK_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let r = &self.items[k];
            h = (h ^ r.severity as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.state as u64).wrapping_mul(FP_STEP);
            let idb = r.id.as_bytes();
            let mut b = 0usize;
            while b < idb.len() {
                h = (h ^ idb[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            let ob = r.owner.as_bytes();
            let mut b2 = 0usize;
            while b2 < ob.len() {
                h = (h ^ ob[b2] as u64).wrapping_mul(FP_STEP);
                b2 += 1;
            }
            k += 1;
        }
        h
    }
}

/// 风险号样本：R-C4-001..003。
pub fn risk_id(seq: u32) -> String {
    format!("R-C4-{:03}", seq)
}

/// 标准台账：三条风险立账 → 缓解 → 关闭。
pub fn build_ledger() -> (RiskLedger, bool) {
    let mut l = RiskLedger::new();
    let mut ok = true;
    let mut seq = 1u32;
    while seq <= NEEDED as u32 {
        if l.file(&risk_id(seq), seq, "域责任面").is_err() {
            ok = false;
        }
        if l.mitigate(&risk_id(seq)).is_err() {
            ok = false;
        }
        if l.close(&risk_id(seq)).is_err() {
            ok = false;
        }
        seq += 1;
    }
    (l, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 越级关闭必拒
    let mut a = RiskLedger::new();
    let _ = a.file(&risk_id(1), 1, "甲");
    got[0] = a.close(&risk_id(1)).err() == Some(RK_STATE) && a.inflight().len() == 1;
    // 2) 重复关闭必拒
    let mut b = RiskLedger::new();
    let _ = b.file(&risk_id(1), 2, "乙");
    let _ = b.mitigate(&risk_id(1));
    let _ = b.close(&risk_id(1));
    got[1] = b.close(&risk_id(1)).err() == Some(RK_STATE);
    // 3) 非法严重度必拒
    let mut c = RiskLedger::new();
    got[2] = c.file(&risk_id(1), 4, "丙").err() == Some(RK_SEV)
        && c.file(&risk_id(1), 0, "丙").err() == Some(RK_SEV);
    // 4) 非 R-C4 号必拒
    let mut d = RiskLedger::new();
    got[3] = d.file("R-X-001", 1, "丁").err() == Some(RK_ID)
        && d.file("R-C4-", 1, "丁").err() == Some(RK_ID);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11110_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11110");

    let (mut led, built) = build_ledger();
    // J1 风险项逐条终核对平
    s.add(
        "J1-风险逐条对平",
        built && led.items.len() == NEEDED && led.closed(),
        "三条风险逐条走到终态且对平",
    );

    // J2 终态处置零在途
    s.add(
        "J2-零在途",
        led.inflight().is_empty() && led.closed_count() == NEEDED,
        "在途清单为空且关闭计数足额",
    );

    // J3 越级关闭拒
    let j3 = {
        let mut c = RiskLedger::new();
        let _ = c.file(&risk_id(1), 1, "甲");
        c.close(&risk_id(1)).err() == Some(RK_STATE)
    };
    s.add("J3-越级关闭拒", j3, "未缓解先关闭必拒");

    // J4 重复关闭拒
    let j4 = {
        let (mut c, _) = build_ledger();
        c.close(&risk_id(2)).err() == Some(RK_STATE)
    };
    s.add("J4-重复关闭拒", j4, "已关闭项再关闭必拒");

    // J5 非法严重度拒
    let j5 = {
        let mut c = RiskLedger::new();
        c.file(&risk_id(1), 9, "甲").err() == Some(RK_SEV) && c.items.is_empty()
    };
    s.add("J5-严重度拒", j5, "严重度越出 1..3 必拒");

    // J6 非法风险号与空责任人拒
    let j6 = {
        let mut c = RiskLedger::new();
        let a = c.file("R-C4-001", 1, "").err() == Some(RK_OWNER);
        let b = c.file("BAD-001", 1, "甲").err() == Some(RK_ID);
        a && b && c.items.is_empty()
    };
    s.add("J6-号责任拒", j6, "空责任人与非法风险号两路必拒");

    // J7 重号立账拒
    let j7 = {
        let mut c = RiskLedger::new();
        let _ = c.file(&risk_id(1), 1, "甲");
        c.file(&risk_id(1), 2, "乙").err() == Some(RK_DUP)
    };
    s.add("J7-重号拒", j7, "同号二次立账必拒");

    // J8 注入检出：越级 + 重复关闭
    let inj = inject_four();
    s.add("J8-越级重关检出", inj[0] && inj[1], "越级关闭与重复关闭两路必抓");

    // J9 注入检出：严重度 + 风险号
    s.add("J9-严重度号检出", inj[2] && inj[3], "非法严重度与非法风险号两路必抓");

    // J10 在途点名
    let j10 = {
        let mut c = RiskLedger::new();
        let _ = c.file(&risk_id(1), 1, "甲");
        let _ = c.file(&risk_id(2), 2, "乙");
        let _ = c.mitigate(&risk_id(2));
        c.inflight().len() == 2 && c.inflight().contains(&risk_id(1)) && !c.closed()
    };
    s.add("J10-在途点名", j10, "缓解未关闭仍在途");

    // J11 冻结拒写
    let root = led.seal();
    let j11 = led.file(&risk_id(9), 1, "追").err() == Some(RK_FROZEN)
        && led.mitigate(&risk_id(1)).err() == Some(RK_FROZEN)
        && led.close(&risk_id(1)).err() == Some(RK_FROZEN);
    s.add("J11-冻结拒写", j11, "封存后立账/缓解/关闭三路必拒");

    // J12 冻结根稳定
    s.add(
        "J12-冻结根稳定",
        led.root == Some(root) && root == led.fold() && root != 0,
        "风险台账根可重算零漂移",
    );

    // J13 状态名与渲染
    let j13 = led.items[0].state_name() == "CLOSED"
        && led.items[0].render().contains("CLOSED")
        && led.items[0].render() != led.items[1].render();
    s.add("J13-状态渲染", j13, "终态渲染带 CLOSED 且逐条可区分");

    // J14 动态明细
    s.add_dyn(
        "J14-风险明细",
        led.closed() && led.fold() == root,
        &format!("items={} inflight={} root={:#x}", led.items.len(), led.inflight().len(), root),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11110_risk_flow() {
        let (mut l, built) = build_ledger();
        assert!(built);
        assert!(l.closed());
        assert!(l.seal() != 0);
        assert!(l.file(&risk_id(4), 1, "x").err() == Some(RK_FROZEN));
        let mut c = RiskLedger::new();
        assert!(c.file(&risk_id(1), 1, "甲").is_ok());
        assert!(c.mitigate(&risk_id(1)).is_ok());
        assert!(c.close(&risk_id(1)).is_ok());
        assert!(c.close(&risk_id(1)).err() == Some(RK_STATE));
    }

    #[test]
    fn unx11110_checks_green() {
        let s = run_unx11110_checks();
        assert!(s.all_passed(), "UNX-F11110 判据未全绿");
    }
}
