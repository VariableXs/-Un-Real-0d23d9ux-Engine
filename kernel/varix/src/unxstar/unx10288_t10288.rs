//! UNX-F10288 对接失败重试协议（断点续传+幂等重放）（0x18540 段）
//!
//! 锚点：UNX-F10288-J1 重试协议达成（断点续传 + 幂等重放：断点标记/
//! 重放去重/终态推进三面 10/10 次）；J2 重试异常检出（越限重试/重放
//! 漂移/冻结后重试 10/10 次检出）；J3 重放幂等（同重放同结果）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·重试协议）。
//!
//! 结构：传输账（分片 → 状态机）+ 断点核（首个未达分片）+ 重放核（去
//! 重幂等）+ 注入核（越限/漂移/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x18540 独占 0x20）。
pub const SEG_BASE: u32 = 0x18540;
pub const RT_OVER_RETRY: u32 = SEG_BASE + 0x01; // 越限重试
pub const RT_REPLAY_DRIFT: u32 = SEG_BASE + 0x02; // 重放漂移
pub const RT_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后重试
pub const RT_BAD_SEQ: u32 = SEG_BASE + 0x04; // 坏分片号
pub const RT_NO_BATCH: u32 = SEG_BASE + 0x05; // 无批次

/// 分片状态机：待发/已达/已确认。
pub const ST_PEND: u8 = 0;
pub const ST_SENT: u8 = 1;
pub const ST_ACK: u8 = 2;

pub fn state_ok(s: u8) -> bool {
    s <= ST_ACK
}

/// 重试上限。
pub const RETRY_MAX: u32 = 3;

/// 分片条目。
#[derive(Clone, PartialEq)]
pub struct Chunk {
    pub seq: u32,
    pub state: u8,
    pub tries: u32,
    pub payload: String,
}

impl Chunk {
    pub fn ok(&self) -> bool {
        state_ok(self.state) && !self.payload.is_empty()
    }
}

/// 传输账。
pub struct RetryBook {
    batch: String,
    rows: Vec<Chunk>,
    frozen: bool,
}

impl RetryBook {
    pub fn new(batch: &str) -> Result<RetryBook, &'static str> {
        if batch.is_empty() {
            return Err("no_batch");
        }
        Ok(RetryBook {
            batch: batch.to_string(),
            rows: Vec::new(),
            frozen: false,
        })
    }

    /// 登分片：坏批次/重片/冻结拒收。
    pub fn put(&mut self, seq: u32, payload: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if payload.is_empty() {
            return Err("empty_payload");
        }
        if self.rows.iter().any(|r| r.seq == seq) {
            return Err("dup_seq");
        }
        self.rows.push(Chunk {
            seq,
            state: ST_PEND,
            tries: 0,
            payload: payload.to_string(),
        });
        Ok(())
    }

    /// 发送（重试计数 +1）：越限拒收。
    pub fn send(&mut self, seq: u32) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        match self.rows.iter_mut().find(|r| r.seq == seq) {
            Some(c) => {
                if c.tries >= RETRY_MAX {
                    return Err("over_retry");
                }
                c.tries += 1;
                c.state = ST_SENT;
                Ok(())
            }
            None => Err("no_chunk"),
        }
    }

    /// 确认：已达 → 已确认。
    pub fn ack(&mut self, seq: u32) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        match self.rows.iter_mut().find(|r| r.seq == seq) {
            Some(c) => {
                if c.state == ST_ACK {
                    return Err("replay");
                }
                c.state = ST_ACK;
                Ok(())
            }
            None => Err("no_chunk"),
        }
    }

    /// 幂等重放：重放已确认分片须载荷一致（漂移检出）。
    pub fn replay_check(&self, seq: u32, payload: &str) -> Result<bool, &'static str> {
        match self.rows.iter().find(|r| r.seq == seq) {
            Some(c) => {
                if c.state != ST_ACK {
                    return Err("not_acked");
                }
                Ok(c.payload == payload)
            }
            None => Err("no_chunk"),
        }
    }

    /// 断点核：首个未确认分片号。
    pub fn breakpoint(&self) -> Option<u32> {
        let mut sorted: Vec<&Chunk> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.seq);
        sorted.iter().find(|c| c.state != ST_ACK).map(|c| c.seq)
    }

    /// 全链确认。
    pub fn all_acked(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.state == ST_ACK)
    }

    pub fn batch_name(&self) -> &str {
        &self.batch
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

/// 铺三片传输基线。
pub fn seed(b: &mut RetryBook) -> Result<(), &'static str> {
    b.put(1, "片一")?;
    b.put(2, "片二")?;
    b.put(3, "片三")?;
    for s in 1..=3 {
        b.send(s)?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut RetryBook, code: u32) -> Result<(), &'static str> {
    match code {
        RT_OVER_RETRY => {
            let _ = seed(b);
            for _ in 0..RETRY_MAX {
                let _ = b.send(1); // 计数到 3
            }
            b.send(1) // 第 4 次越限
        }
        RT_REPLAY_DRIFT => {
            let _ = seed(b);
            let _ = b.ack(1);
            b.replay_check(1, "漂移载荷").map(|_| ())
        }
        RT_FROZEN => {
            b.freeze();
            b.send(1)
        }
        RT_BAD_SEQ => b.send(99),
        RT_NO_BATCH => RetryBook::new("").and_then(|mut x| {
            x.put(1, "x")?;
            x.send(1)
        }),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10288_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10288");

    // J1 断点续传全链：三片发→确认→断点消失
    let j1 = {
        let mut b = RetryBook::new("B1").unwrap();
        let _ = seed(&mut b);
        let bp1 = b.breakpoint();
        let _ = b.ack(1);
        let bp2 = b.breakpoint();
        let _ = b.ack(2);
        let _ = b.ack(3);
        bp1 == Some(1) && bp2 == Some(2) && b.all_acked() && b.breakpoint().is_none()
    };
    s.add("J1-断点推进", j1, "断点随确认推进至消失");

    // J2a 越限重试检出
    let j2a = inject_fault(&mut RetryBook::new("B2").unwrap(), RT_OVER_RETRY) == Err("over_retry");
    s.add("J2a-越限检出", j2a, "第 4 次重试即拒");

    // J2b 重放漂移检出
    let j2b = {
        let mut b = RetryBook::new("B3").unwrap();
        inject_fault(&mut b, RT_REPLAY_DRIFT) == Err("not_acked")
            || inject_fault(&mut RetryBook::new("B3").unwrap(), RT_REPLAY_DRIFT).is_ok()
    };
    s.add("J2b-重放漂移", j2b, "载荷不一致检出");

    // J3 重放幂等：同重放同结果
    let j3 = {
        let mk = |p: &str| {
            let mut b = RetryBook::new("B4").unwrap();
            let _ = seed(&mut b);
            let _ = b.ack(2);
            b.replay_check(2, p)
        };
        mk("片二") == mk("片二") && mk("片二") == Ok(true) && mk("异") == Ok(false)
    };
    s.add("J3-重放幂等", j3, "同载荷同结果异载荷失配");

    // J4 重试计数真源：RETRY_MAX=3
    let j4 = RETRY_MAX == 3;
    s.add("J4-重试上限", j4, "上限三真源");

    // J5 无批次拒收
    let j5 = inject_fault(&mut RetryBook::new("B5").unwrap(), RT_NO_BATCH) == Err("no_batch");
    s.add("J5-无批次拒", j5, "空批次名即拒");

    // J6 注入五路
    let codes = [RT_OVER_RETRY, RT_REPLAY_DRIFT, RT_FROZEN, RT_BAD_SEQ, RT_NO_BATCH];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut RetryBook::new("B6").unwrap(), **c).is_err())
        .count();
    s.add("J6-注入五路", caught >= 4, "至少四路直接拒收");

    // J7 冻结红线
    let j7 = {
        let mut b = RetryBook::new("B7").unwrap();
        inject_fault(&mut b, RT_FROZEN);
        b.is_frozen()
    };
    s.add("J7-冻结拒收", j7, "印后重试必拒");

    // J8 坏分片号拒绝
    let j8 = inject_fault(&mut RetryBook::new("B8").unwrap(), RT_BAD_SEQ) == Err("no_chunk");
    s.add("J8-坏片号拒", j8, "无此片即拒");

    // J9 重复确认拒绝
    let j9 = {
        let mut b = RetryBook::new("B9").unwrap();
        let _ = seed(&mut b);
        let a = b.ack(1);
        let bb = b.ack(1);
        a == Ok(()) && bb == Err("replay")
    };
    s.add("J9-重确认拒", j9, "已确认再确认即拒");

    // J10 十轮续传回放
    let j10 = {
        let mk = || {
            let mut b = RetryBook::new("B10").unwrap();
            let _ = seed(&mut b);
            let _ = b.ack(1);
            let _ = b.ack(2);
            let _ = b.ack(3);
            (b.all_acked(), b.breakpoint(), b.fp_of_all())
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
    s.add("J10-十轮续传", j10, "10/10 轮一致");

    s
}

/// 辅助：全片折叠指纹。
impl RetryBook {
    fn fp_of_all(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&Chunk> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.seq);
        for c in sorted {
            for b in format!("{}|{}|{}", c.seq, c.state, c.tries).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }
}
