//! UNX-F10239 长尾失绿升级（超期未决升级裁决）（0x11D20 段）
//!
//! 锚点：UNX-F10239-J1 升级达成（超期未决项升级登记留痕 10/10 次）；
//! J2 滞留检出（超期但未升级/未超期误升级/已升级仍滞留 10/10 次检出）；
//! J3 升级裁决留痕（升级裁决带四要素：项号/超期龄/裁决人/去向）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因周期收口组）。
//!
//! 结构：长尾册（未决项 × 龄期 × 状态）+ 升级核（阈值判定 + 裁决登记）
//! + 留痕账（四要素全备）+ 注入核（未升级滞留/误升级/冻结后写必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x11D20 独占 0x20）。
pub const SEG_BASE: u32 = 0x11D20;
pub const LS_STALE_NOESC: u32 = SEG_BASE + 0x01; // 超期未升级
pub const LS_ESC_EARLY: u32 = SEG_BASE + 0x02; // 未超期误升级
pub const LS_ESC_STUCK: u32 = SEG_BASE + 0x03; // 已升级仍滞留
pub const LS_BAD_AGE: u32 = SEG_BASE + 0x04; // 坏龄期
pub const LS_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写

/// 长尾状态：在管/已升级/已结。
pub const ST_OPEN: u8 = 0;
pub const ST_ESCALATED: u8 = 1;
pub const ST_CLOSED: u8 = 2;

pub fn state_ok(v: u8) -> bool {
    v <= ST_CLOSED
}

/// 升级阈值：超期龄（周期数）。
pub const ESC_AGE: u32 = 3;

/// 长尾项。
#[derive(Clone, PartialEq)]
pub struct LongItem {
    pub id: u32,
    pub age: u32,
    pub state: u8,
    pub note: String,
}

impl LongItem {
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.note.is_empty() && state_ok(self.state)
    }

    /// 是否超期未决。
    pub fn stale_open(&self) -> bool {
        self.age >= ESC_AGE && self.state == ST_OPEN
    }

    /// 升级四要素渲染：项号/超期龄/裁决人/去向。
    pub fn esc_record(&self, who: &str) -> String {
        format!(
            "ESC|i{}|age{}|who{}|to{}",
            self.id, self.age, who, "B40 收口组"
        )
    }
}

/// 长尾册。
pub struct LongBook {
    rows: Vec<LongItem>,
    esc_log: Vec<String>,
    frozen: bool,
}

impl LongBook {
    pub fn new() -> LongBook {
        LongBook { rows: Vec::new(), esc_log: Vec::new(), frozen: false }
    }

    /// 登项：零 id/空注/坏态/坏龄/冻结后写拒收。
    pub fn add(&mut self, id: u32, age: u32, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if id == 0 {
            return Err("zero_id");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if age > 99_999 {
            return Err("bad_age");
        }
        if self.rows.iter().any(|r| r.id == id) {
            return Err("dup_id");
        }
        self.rows.push(LongItem {
            id,
            age,
            state: ST_OPEN,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 升级裁决：超期在管项 → 升级并留痕；未超期误升级拒收。
    pub fn escalate(&mut self, id: u32, who: &str) -> Result<String, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let idx = match self.rows.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => return Err("no_item"),
        };
        let it = &mut self.rows[idx];
        if it.state == ST_CLOSED {
            return Err("closed");
        }
        if it.state == ST_ESCALATED {
            return Err("already");
        }
        if it.age < ESC_AGE {
            return Err("early");
        }
        it.state = ST_ESCALATED;
        let rec = it.esc_record(who);
        self.esc_log.push(rec.clone());
        Ok(rec)
    }

    /// 滞留扫描：超期仍在管（未升级）清单。
    pub fn stale_scan(&self) -> Vec<u32> {
        self.rows.iter().filter(|r| r.stale_open()).map(|r| r.id).collect()
    }

    /// 已升级仍滞留：升级后龄期继续涨且未结（判据注入场景）。
    pub fn esc_stuck(&self) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|r| r.state == ST_ESCALATED && r.age >= ESC_AGE * 2)
            .map(|r| r.id)
            .collect()
    }

    /// 留痕账条数。
    pub fn log_len(&self) -> usize {
        self.esc_log.len()
    }

    /// 留痕四要素完备性：每条 log 都含四段。
    pub fn log_complete(&self) -> bool {
        self.esc_log
            .iter()
            .all(|r| r.contains("ESC|i") && r.contains("|age") && r.contains("|who") && r.contains("|to"))
    }

    /// 推进龄期（周期折叠用）。
    pub fn tick(&mut self, id: u32) -> Result<(), &'static str> {
        match self.rows.iter_mut().find(|r| r.id == id) {
            Some(it) => {
                it.age += 1;
                Ok(())
            }
            None => Err("no_item"),
        }
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

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 注入核。
pub fn inject_fault(b: &mut LongBook, code: u32, id: u32, age: u32) -> Result<(), &'static str> {
    match code {
        LS_STALE_NOESC => b.add(id, age, "超期未升级样本"),
        LS_ESC_EARLY => {
            b.add(id, 1, "未超期样本")?;
            b.escalate(id, "early").map(|_| ())
        }
        LS_ESC_STUCK => {
            b.add(id, ESC_AGE, "升级样本")?;
            b.escalate(id, "B40")?;
            for _ in 0..(ESC_AGE * 2) {
                b.tick(id)?;
            }
            Ok(())
        }
        LS_BAD_AGE => b.add(id, 999_999, "坏龄期"),
        LS_FROZEN => {
            b.freeze();
            b.add(id, 1, "冻结后写")
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10239_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10239");

    // J1 升级达成：阈值项升级留痕四要素全备
    let j1 = {
        let mut b = LongBook::new();
        let _ = b.add(1, ESC_AGE, "临界项");
        let _ = b.add(2, 1, "新项");
        let rec = b.escalate(1, "AI-WBX1027-J2");
        let _ = b.escalate(2, "AI-WBX1027-J2");
        match rec {
            Ok(r) => b.log_len() == 1 && b.log_complete() && r.contains("|i1|"),
            Err(_) => false,
        }
    };
    s.add("J1-升级留痕四要素", j1, "临界项升级+未超期拒收");

    // J2a 超期未升级滞留检出
    let j2a = {
        let mut b = LongBook::new();
        let _ = inject_fault(&mut b, LS_STALE_NOESC, 10, 5);
        b.stale_scan() == vec![10u32]
    };
    s.add("J2a-滞留检出", j2a, "超期在管必进清单");

    // J2b 未超期误升级拒收
    let j2b = inject_fault(&mut LongBook::new(), LS_ESC_EARLY, 11, 0) == Err("early");
    s.add("J2b-误升级拒收", j2b, "未超期 escalate 即拒");

    // J2c 已升级仍滞留检出
    let j2c = {
        let mut b = LongBook::new();
        let _ = inject_fault(&mut b, LS_ESC_STUCK, 12, 0);
        b.esc_stuck() == vec![12u32]
    };
    s.add("J2c-升级后滞留", j2c, "升级后长龄必进 stuck 清单");

    // J3 升级留痕不可逆：二次升级拒收
    let j3 = {
        let mut b = LongBook::new();
        let _ = b.add(3, ESC_AGE, "项");
        let _ = b.escalate(3, "A");
        b.escalate(3, "B") == Err("already")
    };
    s.add("J3-二次升级拒", j3, "已升级再升即拒");

    // J4 注入面五路
    let codes = [LS_STALE_NOESC, LS_ESC_EARLY, LS_ESC_STUCK, LS_BAD_AGE, LS_FROZEN];
    let caught = codes
        .iter()
        .filter(|c| {
            let r = inject_fault(&mut LongBook::new(), **c, 20, ESC_AGE);
            r.is_err()
        })
        .count();
    s.add("J4-注入五路", caught >= 3, "至少三路注入直接拒收");

    // J5 坏龄期拒收
    let j5 = inject_fault(&mut LongBook::new(), LS_BAD_AGE, 21, 0) == Err("bad_age");
    s.add("J5-坏龄期拒", j5, "age 越界即拒");

    // J6 冻结红线
    let j6 = {
        let mut b = LongBook::new();
        inject_fault(&mut b, LS_FROZEN, 22, 0);
        b.is_frozen() && b.add(23, 1, "后") == Err("frozen")
    };
    s.add("J6-冻结拒收", j6, "印后写必抓");

    // J7 重号拒收
    let j7 = {
        let mut b = LongBook::new();
        let a = b.add(7, 1, "一");
        let bb = b.add(7, 2, "二");
        a == Ok(()) && bb == Err("dup_id")
    };
    s.add("J7-重号拒收", j7, "同 id 二次登记即拒");

    // J8 十轮升级回放：超期项 10/10 升级留痕（确定性）
    let j8 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = LongBook::new();
            let _ = b.add(100 + r, ESC_AGE + r, "超期项");
            match b.escalate(100 + r, "J2") {
                Ok(rec) => {
                    if rec.contains("|age") && b.log_len() == 1 {
                        ok += 1;
                    }
                }
                Err(_) => {}
            }
        }
        ok == 10
    };
    s.add("J8-十轮升级", j8, "10/10 次升级留痕");

    // J9 阈值边界：ESC_AGE-1 不升、ESC_AGE 升（边界精确）
    let j9 = {
        let mut b = LongBook::new();
        let _ = b.add(1, ESC_AGE - 1, "差一");
        let _ = b.add(2, ESC_AGE, "临界");
        let early = b.escalate(1, "J") == Err("early");
        let ontime = b.escalate(2, "J").is_ok();
        early && ontime
    };
    s.add("J9-阈值边界", j9, "差一不升临界升");

    // J10 项合法性：坏态负样本 ok 必假
    let j10 = {
        let bad = LongItem {
            id: 1,
            age: 1,
            state: 9,
            note: "x".to_string(),
        };
        !bad.ok()
    };
    s.add("J10-坏态负样本", j10, "state=9 ok 必假");

    s
}
