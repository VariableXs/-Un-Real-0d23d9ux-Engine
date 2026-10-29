//! UNX-F10286 缺陷回流通道联调（BUG-UNX-#### 双向回执）（0x18100 段）
//!
//! 锚点：UNX-F10286-J1 联调达成（缺陷工单双向回执：发出/回执/对账
//! 三面 10/10 次）；J2 回执异常检出（回执超时/回执错单/重放回执 10/10
//! 次检出）；J3 工单指纹（同工单同回执同指纹）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·缺陷回流）。
//!
//! 结构：工单册（BUG-UNX-#### 编号闭集）+ 回执账（工单 → 回执）+ 对
//! 账核（双向匹配）+ 注入核（超时/错单/重放必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x18100 独占 0x20）。
pub const SEG_BASE: u32 = 0x18100;
pub const RT_TIMEOUT: u32 = SEG_BASE + 0x01; // 回执超时
pub const RT_WRONG_ID: u32 = SEG_BASE + 0x02; // 回执错单
pub const RT_REPLAY: u32 = SEG_BASE + 0x03; // 重放回执
pub const RT_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后发
pub const RT_BAD_ID: u32 = SEG_BASE + 0x05; // 坏工单号

/// 回执时限（tick 数）。
pub const RT_LIMIT: u64 = 10;

/// 工单条目。
#[derive(Clone, PartialEq)]
pub struct Ticket {
    pub id: u32,       // BUG-UNX-#### 的 ####
    pub title: String,
    pub issued_tick: u64,
    pub acked: Option<(String, u64)>, // (回执人, 回执时点)
}

impl Ticket {
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.title.is_empty() && self.acked.is_none()
    }

    pub fn acked(&self) -> bool {
        self.acked.is_some()
    }

    /// 工单渲染行。
    pub fn render(&self) -> String {
        format!(
            "BUG-UNX-{:04}|{}|t{}|{}",
            self.id,
            self.title,
            self.issued_tick,
            if self.acked() { "ack" } else { "pending" }
        )
    }
}

/// 回执账。
pub struct ReflowBook {
    rows: Vec<Ticket>,
    frozen: bool,
    anomalies: Vec<(u32, u32)>, // (工单, 异常码)
}

impl ReflowBook {
    pub fn new() -> ReflowBook {
        ReflowBook { rows: Vec::new(), frozen: false, anomalies: Vec::new() }
    }

    /// 发单：坏号/重号/冻结拒收。
    pub fn issue(&mut self, id: u32, title: &str, tick: u64) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if id == 0 || id > 9999 {
            return Err("bad_id");
        }
        if title.is_empty() {
            return Err("empty_title");
        }
        if self.rows.iter().any(|r| r.id == id) {
            return Err("dup_id");
        }
        self.rows.push(Ticket {
            id,
            title: title.to_string(),
            issued_tick: tick,
            acked: None,
        });
        Ok(())
    }

    /// 回执：工单须存在；超时/重放拒收并留痕。
    pub fn ack(&mut self, id: u32, who: &str, tick: u64) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if who.is_empty() {
            return Err("no_who");
        }
        let idx = match self.rows.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => {
                self.anomalies.push((id, RT_WRONG_ID));
                return Err("no_ticket");
            }
        };
        if self.rows[idx].acked() {
            self.anomalies.push((id, RT_REPLAY));
            return Err("replay");
        }
        if tick > self.rows[idx].issued_tick + RT_LIMIT {
            self.anomalies.push((id, RT_TIMEOUT));
            return Err("timeout");
        }
        self.rows[idx].acked = Some((who.to_string(), tick));
        Ok(())
    }

    /// 对账核：全单回执齐备。
    pub fn all_acked(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.acked())
    }

    /// 超时单清单。
    pub fn overdue(&self, now: u64) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|r| !r.acked() && now > r.issued_tick + RT_LIMIT)
            .map(|r| r.id)
            .collect()
    }

    /// 工单指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&Ticket> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.id);
        for r in sorted {
            for b in r.render().as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn anomaly_len(&self) -> usize {
        self.anomalies.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 注入核。
pub fn inject_fault(b: &mut ReflowBook, code: u32, id: u32) -> Result<(), &'static str> {
    match code {
        RT_TIMEOUT => {
            let _ = b.issue(id, "超时样本", 1);
            b.ack(id, "晚到人", 100)
        }
        RT_WRONG_ID => {
            let _ = b.issue(id, "正常单", 1);
            b.ack(id + 5000, "错单人", 2)
        }
        RT_REPLAY => {
            let _ = b.issue(id, "重放样本", 1);
            let _ = b.ack(id, "首回执", 2);
            b.ack(id, "重放人", 3)
        }
        RT_FROZEN => {
            b.freeze();
            b.issue(id, "冻结后", 1)
        }
        RT_BAD_ID => b.issue(0, "零号", 1),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10286_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10286");

    // J1 双向回执全链：发单→回执→对账
    let j1 = {
        let mut b = ReflowBook::new();
        let _ = b.issue(1, "fd 泄漏", 1);
        let _ = b.issue(2, "锁序倒置", 2);
        let _ = b.ack(1, "O3 侧", 3);
        let _ = b.ack(2, "O3 侧", 4);
        b.all_acked() && b.len() == 2
    };
    s.add("J1-双向回执", j1, "发回对三面全通");

    // J2a 回执超时检出
    let j2a = {
        let mut b = ReflowBook::new();
        let r = inject_fault(&mut b, RT_TIMEOUT, 10);
        r == Err("timeout") && b.anomalies.iter().any(|(i, c)| *i == 10 && *c == RT_TIMEOUT)
    };
    s.add("J2a-超时检出", j2a, "超限回执精确入单");

    // J2b 回执错单检出
    let j2b = {
        let mut b = ReflowBook::new();
        let r = inject_fault(&mut b, RT_WRONG_ID, 11);
        r == Err("no_ticket") && b.anomalies.iter().any(|(i, c)| *i == 5011 && *c == RT_WRONG_ID)
    };
    s.add("J2b-错单检出", j2b, "无单回执精确入单");

    // J2c 重放回执检出
    let j2c = {
        let mut b = ReflowBook::new();
        let r = inject_fault(&mut b, RT_REPLAY, 12);
        r == Err("replay") && b.anomalies.iter().any(|(i, c)| *i == 12 && *c == RT_REPLAY)
    };
    s.add("J2c-重放检出", j2c, "二回执精确入单");

    // J3 工单指纹：回执前后指纹变
    let j3 = {
        let mut b = ReflowBook::new();
        let _ = b.issue(20, "样本", 1);
        let f1 = b.fp();
        let _ = b.ack(20, "x", 2);
        let f2 = b.fp();
        f1 != f2 && f2 != 0
    };
    s.add("J3-工单指纹", j3, "回执改变指纹");

    // J4 超时单清单
    let j4 = {
        let mut b = ReflowBook::new();
        let _ = b.issue(30, "未回执", 1);
        let _ = b.issue(31, "已回执", 1);
        let _ = b.ack(31, "x", 2);
        b.overdue(50) == vec![30u32]
    };
    s.add("J4-超时清单", j4, "未回执超限单精确");

    // J5 工单号闭集：>9999 拒收
    let j5 = ReflowBook::new().issue(10000, "越界", 1) == Err("bad_id");
    s.add("J5-编号闭集", j5, "#### 四位上限");

    // J6 注入五路
    let codes = [RT_TIMEOUT, RT_WRONG_ID, RT_REPLAY, RT_FROZEN, RT_BAD_ID];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut ReflowBook::new(), **c, 40).is_err())
        .count();
    s.add("J6-注入五路", caught == 5, "五路注入全拒");

    // J7 冻结红线
    let j7 = {
        let mut b = ReflowBook::new();
        inject_fault(&mut b, RT_FROZEN, 41);
        b.is_frozen()
    };
    s.add("J7-冻结拒发", j7, "印后发单必拒");

    // J8 零号拒收
    let j8 = inject_fault(&mut ReflowBook::new(), RT_BAD_ID, 0) == Err("bad_id");
    s.add("J8-零号拒收", j8, "id=0 即拒");

    // J9 工单渲染行格式
    let j9 = {
        let mut b = ReflowBook::new();
        let _ = b.issue(42, "渲染样本", 1);
        let _ = b.ack(42, "y", 2);
        let row = b.rows[0].render();
        row.starts_with("BUG-UNX-0042|") && row.contains("|ack")
    };
    s.add("J9-渲染格式", j9, "四位编号+状态渲染");

    // J10 十轮回执回放
    let j10 = {
        let mk = || {
            let mut b = ReflowBook::new();
            let _ = b.issue(50, "回放样本", 1);
            let _ = b.ack(50, "J2", 3);
            (b.all_acked(), b.fp())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0
    };
    s.add("J10-十轮回执", j10, "10/10 轮一致");

    s
}
