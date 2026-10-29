//! UNX-F10182 pty 回环正反双判据（主从互通达成与断端误用受控）（0x109e0 段）
//!
//! 锚点：UNX-F10182-J1 回环达成（主写→从读字节一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：回环管道（m→s 单向流 × 分片写读 × 校验和比对）+ 流量账本
//! （字节守恒双计数）+ 校验核（折叠和逐字节比对）+ 注入核（翻转
//! 字节注入/空读/断端读/空留痕/印后跑环）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x109e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x109e0;
pub const LP_EMPTY: u32 = SEG_BASE + 0x01; // 空读（EAGAIN 面）
pub const LP_SHUT: u32 = SEG_BASE + 0x02; // 断端读写
pub const LP_CKSUM: u32 = SEG_BASE + 0x03; // 校验和不一致
pub const LP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const LP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍跑环

/// 一次回环运行的账本留痕。
#[derive(Clone)]
pub struct LoopRec {
    pub seq: u32,
    pub bytes: u32,
    /// 发端折叠和（16 位折叠累加）。
    pub ck_tx: u32,
    /// 收端折叠和。
    pub ck_rx: u32,
    pub note: String,
}

impl LoopRec {
    /// 账目合法性：留痕非空且两端校验和一致。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.ck_tx == self.ck_rx
    }

    pub fn render_line(&self) -> String {
        format!("L{}|n{}|tx{}|rx{}|{}", self.seq, self.bytes, self.ck_tx, self.ck_rx, self.note)
    }
}

/// 回环管道：m→s 单向流 + 分片接口。
pub struct LoopPipe {
    pub m2s: Vec<u8>,
    pub shut_tx: bool,
    pub shut_rx: bool,
    pub total_tx: u64,
    pub total_rx: u64,
    pub ledger: Vec<LoopRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl LoopPipe {
    pub fn new() -> LoopPipe {
        LoopPipe {
            m2s: Vec::new(),
            shut_tx: false,
            shut_rx: false,
            total_tx: 0,
            total_rx: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 主端写（可分片；断写端必拒）。
    pub fn write_master(&mut self, chunk: &[u8]) -> Result<usize, u32> {
        if self.shut_tx {
            return Err(LP_SHUT);
        }
        for b in chunk {
            self.m2s.push(*b);
        }
        self.total_tx += chunk.len() as u64;
        Ok(chunk.len())
    }

    /// 从端读至多 n 字节（空即 LP_EMPTY 受控面）。
    pub fn read_slave(&mut self, n: usize) -> Result<Vec<u8>, u32> {
        if self.shut_rx {
            return Err(LP_SHUT);
        }
        if self.m2s.is_empty() {
            return Err(LP_EMPTY);
        }
        let take = n.min(self.m2s.len());
        let mut out = Vec::new();
        let mut i = 0;
        while i < take {
            out.push(self.m2s.remove(0));
            i += 1;
        }
        self.total_rx += out.len() as u64;
        Ok(out)
    }

    /// 关断端（关后读写皆拒）。
    pub fn shutdown(&mut self, tx: bool, rx: bool) {
        self.shut_tx = self.shut_tx || tx;
        self.shut_rx = self.shut_rx || rx;
    }

    /// 16 位折叠和（RFC1071 风格，本批独立实现）。
    pub fn fold_sum(data: &[u8]) -> u32 {
        let mut s: u32 = 0;
        for b in data {
            s = (s + *b as u32) & 0xFFFF;
        }
        s ^ 0xB10C
    }

    /// 回环运行核：模式字节全量写出→分片读回→校验和比对落账。
    pub fn run_loopback(&mut self, n: u32, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(LP_FROZEN);
        }
        if note.is_empty() {
            return Err(LP_EMPTY_NOTE);
        }
        // 发端：模式字节 (i mod 251)。
        let mut tx: Vec<u8> = Vec::new();
        let mut i: u32 = 0;
        while i < n {
            tx.push((i % 251) as u8);
            i += 1;
        }
        let ck_tx = LoopPipe::fold_sum(&tx);
        let _ = self.write_master(&tx);
        // 收端：分片 7 字节读回。
        let mut rx: Vec<u8> = Vec::new();
        loop {
            match self.read_slave(7) {
                Ok(chunk) => {
                    let done = chunk.len() < 7;
                    for b in chunk {
                        rx.push(b);
                    }
                    if done {
                        break;
                    }
                }
                Err(LP_EMPTY) => break, // 流尽即止
                Err(e) => return Err(e),
            }
        }
        let ck_rx = LoopPipe::fold_sum(&rx);
        self.ledger.push(LoopRec {
            seq: self.seq_next,
            bytes: n,
            ck_tx,
            ck_rx,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(ck_tx == ck_rx && rx.len() == tx.len())
    }

    /// 注入路径：发送时翻转第 k 字节（校验和必不一致→检出）。
    pub fn run_corrupted(&mut self, n: u32, k: usize, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(LP_FROZEN);
        }
        if note.is_empty() {
            return Err(LP_EMPTY_NOTE);
        }
        let mut tx: Vec<u8> = Vec::new();
        let mut i: u32 = 0;
        while i < n {
            tx.push((i % 251) as u8);
            i += 1;
        }
        let ck_tx = LoopPipe::fold_sum(&tx);
        if k < tx.len() {
            tx[k] ^= 0xFF;
        }
        let _ = self.write_master(&tx);
        let mut rx: Vec<u8> = Vec::new();
        loop {
            match self.read_slave(64) {
                Ok(chunk) => {
                    let done = chunk.len() < 64;
                    for b in chunk {
                        rx.push(b);
                    }
                    if done {
                        break;
                    }
                }
                Err(LP_EMPTY) => break,
                Err(e) => return Err(e),
            }
        }
        let ck_rx = LoopPipe::fold_sum(&rx);
        self.ledger.push(LoopRec {
            seq: self.seq_next,
            bytes: n,
            ck_tx,
            ck_rx,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(ck_tx == ck_rx)
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（83 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x109e0;
        for r in &self.ledger {
            h = h.wrapping_mul(83).wrapping_add(r.ck_tx);
            h = h.wrapping_mul(83).wrapping_add(r.bytes);
            for by in r.note.bytes() {
                h = h.wrapping_mul(83).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(83).wrapping_add(self.total_tx as u32);
        h = h.wrapping_mul(83).wrapping_add(self.total_rx as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 字节翻转注入：单字节翻转必被校验和检出。
    let mut p = LoopPipe::new();
    got[0] = p.run_corrupted(64, 17, "inj-flip") == Ok(false);
    // 2) 空读受控（EAGAIN 面）。
    let mut p2 = LoopPipe::new();
    got[1] = p2.read_slave(4).err() == Some(LP_EMPTY);
    // 3) 断端读写必拒。
    p2.shutdown(true, true);
    got[2] = p2.write_master(b"x").err() == Some(LP_SHUT)
        && p2.read_slave(1).err() == Some(LP_SHUT);
    // 4) 伪造账：校验和不一致的留痕必判非法。
    let forged = LoopRec {
        seq: 1,
        bytes: 10,
        ck_tx: 100,
        ck_rx: 200,
        note: "inj-ck".to_string(),
    };
    got[3] = !forged.ok();
    // 5) 印账后跑环必抓。
    p.seal();
    got[4] = p.run_loopback(8, "inj-frozen").err() == Some(LP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10182");

    // J1 回环达成：256 字节模式回环校验和一致。
    let j1 = {
        let mut p = LoopPipe::new();
        p.run_loopback(256, "a") == Ok(true)
    };
    s.add("J1-回环达成", j1, "256 字节主写从读全一致");

    // J2 分片一致：7 字节分片读拼回与整发等长等和。
    let j2 = {
        let mut p = LoopPipe::new();
        let r = p.run_loopback(23, "frag");
        r == Ok(true) && p.total_tx == 23 && p.total_rx == 23
    };
    s.add("J2-分片一致", j2, "23 字节七分片拼回无损");

    // J3 空读受控。
    let j3 = {
        let mut p = LoopPipe::new();
        p.read_slave(1).err() == Some(LP_EMPTY)
    };
    s.add("J3-空读受控", j3, "空流读按 EAGAIN 面受控");

    // J4 字节守恒：tx 总数 == rx 总数。
    let j4 = {
        let mut p = LoopPipe::new();
        let _ = p.run_loopback(100, "c");
        let _ = p.run_loopback(50, "d");
        p.total_tx == p.total_rx && p.total_tx == 150
    };
    s.add("J4-字节守恒", j4, "双计数 150 对平");

    // J5 注入检出：翻转字节回环必红。
    let j5 = {
        let mut p = LoopPipe::new();
        p.run_corrupted(32, 5, "inj") == Ok(false)
    };
    s.add("J5-注入检出", j5, "单字节翻转校验必不一致");

    // J6 断端受控：关断后双方向皆拒。
    let j6 = {
        let mut p = LoopPipe::new();
        p.shutdown(true, false);
        p.write_master(b"z").err() == Some(LP_SHUT)
    };
    s.add("J6-断端受控", j6, "关写端后写必拒");

    // J7 账本完整：好账全合法（坏注入账不进 ledger_ok 面）。
    let j7 = {
        let mut p = LoopPipe::new();
        let _ = p.run_loopback(10, "a");
        let _ = p.run_loopback(20, "b");
        p.ledger.len() == 2 && p.ledger_ok() && p.seq_unique()
    };
    s.add("J7-账本完整", j7, "两账校验和一致全合法");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut p = LoopPipe::new();
        let _ = p.run_loopback(8, "a");
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2 && p.run_loopback(8, "post").err() == Some(LP_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10182_loop_flow() {
        let mut p = LoopPipe::new();
        assert_eq!(p.run_loopback(100, "t"), Ok(true));
        assert_eq!(p.total_tx, 100);
        let _ = p.read_slave(1).unwrap_or_default();
        p.shutdown(false, true);
        assert_eq!(p.read_slave(1), Err(LP_SHUT));
    }

    #[test]
    fn unx10182_checks_green() {
        let s = run_unx10182_checks();
        assert!(s.all_passed(), "UNX-F10182 判据未全绿");
    }
}
