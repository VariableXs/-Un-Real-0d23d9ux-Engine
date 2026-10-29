//! UNX-F12267 NtCancelIoFile/NtCancelIoFileEx 取消语义（0x64860 段）
//!
//! 卡面：I/O 取消双协议——旧取消（CancelIoFile）按文件句柄批量取消该句柄
//! 名下全部未决请求；Ex 取消（CancelIoFileEx）按（句柄，请求号）单点取消。
//! 仅 Pending 态请求可取消；取消即置 CANCELLED 并记取消账；已完成/已取消/
//! 未知请求均不可取消。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 仅 Pending 可取消：Completed/Cancelled 态请求被取消必拒。
//! - J2 未知请求必拒：（句柄，请求号）不在册。
//! - J3 坏句柄必拒：文件句柄 0 不在句柄域。
//! - J4 Ex 单点取消只动一个请求；旧取消按文件动全部 Pending。
//! - J5 重号提交必拒：同句柄同请求号不得重复入册。
//! - J6 完成转移合法：仅 Pending → Completed 一条路。
//! - J7 取消计数可回读：取消账与实际 CANCELLED 记录数一致。
//! - J8 印账冻结：seal 后提交/取消/完成必拒。
//! - J9 指纹幂等：同态同指纹。
//! - J10 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64860 独占 0x20）。
pub const SEG_BASE: u32 = 0x64860;
pub const CX_BAD_HANDLE: u32 = SEG_BASE + 0x01; // 文件句柄越域
pub const CX_NOT_FOUND: u32 = SEG_BASE + 0x02; // 请求不在册
pub const CX_NOT_PENDING: u32 = SEG_BASE + 0x03; // 非 Pending 态不可取消
pub const CX_DUP: u32 = SEG_BASE + 0x04; // 重号提交
pub const CX_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 请求操作码（模型闭集）。
pub const OP_READ: u8 = 0;
pub const OP_WRITE: u8 = 1;
pub const OP_IOCTL: u8 = 2;

/// 请求状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ReqState {
    Pending = 0,
    Cancelled = 1,
    Completed = 2,
}

/// 单条未决/已决 I/O 请求。
#[derive(Clone)]
pub struct CancelReq {
    pub file: u32,
    pub req: u32,
    pub op: u8,
    pub state: ReqState,
    /// 取消序号（0 = 从未取消；>0 为取消发生时的账面序）。
    pub cancel_seq: u32,
}

impl CancelReq {
    pub fn render(&self) -> String {
        format!(
            "req{}|f{}|op{}|s{}|c{}",
            self.req, self.file, self.op, self.state as u8, self.cancel_seq
        )
    }
}

/// 取消账册。
pub struct CancelBook {
    pub recs: Vec<CancelReq>,
    pub seq: u32,
    pub sealed: bool,
}

impl CancelBook {
    pub fn new() -> CancelBook {
        CancelBook { recs: Vec::new(), seq: 0, sealed: false }
    }

    fn find(&self, file: u32, req: u32) -> Option<usize> {
        self.recs.iter().position(|r| r.file == file && r.req == req)
    }

    /// 提交请求：句柄须在域内（非零），（句柄，请求号）唯一。
    pub fn submit(&mut self, file: u32, req: u32, op: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if file == 0 {
            return Err(CX_BAD_HANDLE);
        }
        if op > OP_IOCTL {
            return Err(CX_BAD_HANDLE);
        }
        if self.find(file, req).is_some() {
            return Err(CX_DUP);
        }
        self.recs.push(CancelReq {
            file,
            req,
            op,
            state: ReqState::Pending,
            cancel_seq: 0,
        });
        Ok(())
    }

    /// Ex 单点取消：仅 Pending 可取消。
    pub fn cancel_one(&mut self, file: u32, req: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        match self.find(file, req) {
            None => Err(CX_NOT_FOUND),
            Some(i) => {
                if self.recs[i].state != ReqState::Pending {
                    return Err(CX_NOT_PENDING);
                }
                self.seq += 1;
                self.recs[i].state = ReqState::Cancelled;
                self.recs[i].cancel_seq = self.seq;
                Ok(())
            }
        }
    }

    /// 旧取消：按文件句柄取消名下全部 Pending，返回取消条数。
    pub fn cancel_file(&mut self, file: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if file == 0 {
            return Err(CX_BAD_HANDLE);
        }
        let mut n = 0usize;
        for r in self.recs.iter_mut() {
            if r.file == file && r.state == ReqState::Pending {
                self.seq += 1;
                r.state = ReqState::Cancelled;
                r.cancel_seq = self.seq;
                n += 1;
            }
        }
        if n == 0 {
            Err(CX_NOT_FOUND)
        } else {
            Ok(n)
        }
    }

    /// 完成转移：仅 Pending → Completed。
    pub fn complete(&mut self, file: u32, req: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        match self.find(file, req) {
            None => Err(CX_NOT_FOUND),
            Some(i) => {
                if self.recs[i].state != ReqState::Pending {
                    return Err(CX_NOT_PENDING);
                }
                self.recs[i].state = ReqState::Completed;
                Ok(())
            }
        }
    }

    pub fn count_in_state(&self, st: ReqState) -> usize {
        self.recs.iter().filter(|r| r.state == st).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.file);
            h = h.wrapping_mul(211).wrapping_add(r.req);
            h = h.wrapping_mul(211).wrapping_add(r.op as u32);
            h = h.wrapping_mul(211).wrapping_add(r.state as u32);
            h = h.wrapping_mul(211).wrapping_add(r.cancel_seq);
        }
        h = h.wrapping_mul(211).wrapping_add(self.seq);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：账面摘要。
    pub fn summary(&self) -> String {
        format!(
            "n={} p={} c={} x={}",
            self.recs.len(),
            self.count_in_state(ReqState::Pending),
            self.count_in_state(ReqState::Completed),
            self.count_in_state(ReqState::Cancelled)
        )
    }

    pub fn line_of(&self, file: u32, req: u32) -> String {
        match self.find(file, req) {
            Some(i) => self.recs[i].render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CancelBook::new();
    // 1) 坏句柄（0）提交必拒。
    got[0] = b.submit(0, 1, OP_READ).err() == Some(CX_BAD_HANDLE);
    // 2) 取消未知请求必拒。
    got[1] = b.cancel_one(3, 9).err() == Some(CX_NOT_FOUND);
    // 3) 非 Pending 态取消必拒（先完成再取消）。
    let _ = b.submit(3, 1, OP_WRITE);
    let _ = b.complete(3, 1);
    got[2] = b.cancel_one(3, 1).err() == Some(CX_NOT_PENDING);
    // 4) 重号提交必拒。
    let _ = b.submit(3, 2, OP_READ);
    got[3] = b.submit(3, 2, OP_READ).err() == Some(CX_DUP);
    // 5) 印账后取消必抓。
    b.seal();
    got[4] = b.cancel_one(3, 2).err() == Some(CX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12267_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12267");

    // J1 仅 Pending 可取消。
    let j1 = {
        let mut b = CancelBook::new();
        let _ = b.submit(1, 1, OP_READ);
        let _ = b.complete(1, 1);
        b.cancel_one(1, 1).err() == Some(CX_NOT_PENDING)
    };
    s.add("J1-仅Pending可取消", j1, "已完成再取消必拒");

    // J2 未知请求必拒。
    let j2 = {
        let mut b = CancelBook::new();
        let _ = b.submit(1, 1, OP_READ);
        b.cancel_one(1, 2).err() == Some(CX_NOT_FOUND)
    };
    s.add("J2-未知请求拒", j2, "请求号不在册");

    // J3 坏句柄必拒。
    let j3 = {
        let mut b = CancelBook::new();
        b.submit(0, 1, OP_READ).err() == Some(CX_BAD_HANDLE)
            && b.cancel_file(0).err() == Some(CX_BAD_HANDLE)
    };
    s.add("J3-坏句柄拒", j3, "句柄 0 越域");

    // J4 单点取消 vs 按文件批量。
    let j4 = {
        let mut b = CancelBook::new();
        let _ = b.submit(2, 1, OP_READ);
        let _ = b.submit(2, 2, OP_WRITE);
        let _ = b.submit(2, 3, OP_IOCTL);
        let _ = b.cancel_one(2, 2);
        let one = b.count_in_state(ReqState::Cancelled) == 1
            && b.count_in_state(ReqState::Pending) == 2;
        let bulk = b.cancel_file(2).ok() == Some(2);
        one && bulk
    };
    s.add("J4-单点/批量", j4, "Ex 单点、旧协议批量");

    // J5 重号提交必拒。
    let j5 = {
        let mut b = CancelBook::new();
        let _ = b.submit(1, 5, OP_READ);
        b.submit(1, 5, OP_READ).err() == Some(CX_DUP)
    };
    s.add("J5-重号提交拒", j5, "句柄×请求号唯一");

    // J6 完成转移合法。
    let j6 = {
        let mut b = CancelBook::new();
        let _ = b.submit(1, 6, OP_READ);
        let ok = b.complete(1, 6).is_ok();
        let _ = b.submit(1, 7, OP_READ);
        let _ = b.cancel_one(1, 7);
        let bad = b.complete(1, 7).err() == Some(CX_NOT_PENDING);
        ok && bad
    };
    s.add("J6-完成转移", j6, "仅 Pending→Completed");

    // J7 取消计数回读。
    let j7 = {
        let mut b = CancelBook::new();
        let _ = b.submit(4, 1, OP_READ);
        let _ = b.submit(4, 2, OP_WRITE);
        let _ = b.cancel_file(4);
        b.count_in_state(ReqState::Cancelled) == 2
            && b.summary().contains("x=2")
    };
    s.add("J7-取消计数", j7, "账面与实况一致");

    // J8 印账冻结。
    let j8 = {
        let mut b = CancelBook::new();
        b.seal();
        b.submit(1, 1, OP_READ).err() == Some(CX_FROZEN)
            && b.cancel_file(1).err() == Some(CX_FROZEN)
            && b.complete(1, 1).err() == Some(CX_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后三操作皆拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = CancelBook::new();
        let _ = b.submit(1, 1, OP_READ);
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 回读渲染。
    let j10 = {
        let mut b = CancelBook::new();
        let _ = b.submit(8, 3, OP_IOCTL);
        let _ = b.cancel_one(8, 3);
        b.line_of(8, 3).contains("s1") && b.line_of(8, 4) == "missing"
    };
    s.add("J10-回读渲染", j10, "取消态行可渲染");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CancelBook::new();
    let _ = b.submit(1, 1, OP_READ);
    let _ = b.submit(1, 2, OP_WRITE);
    let _ = b.cancel_one(1, 1);
    let _ = b.complete(1, 2);
    s.add("摘要回读", b.summary().contains("n=2"), "双请求在册");
    s.add("取消序记账", b.seq == 1, "一次取消一序");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12267_cancel_flow() {
        let mut b = CancelBook::new();
        assert!(b.submit(1, 1, OP_READ).is_ok());
        assert!(b.cancel_one(1, 1).is_ok());
        assert_eq!(b.recs[0].state, ReqState::Cancelled);
        assert_eq!(b.cancel_one(1, 1), Err(CX_NOT_PENDING));
        assert_eq!(b.submit(0, 2, OP_READ), Err(CX_BAD_HANDLE));
    }

    #[test]
    fn unx12267_checks_green() {
        let s = run_unx12267_checks();
        assert!(s.all_passed(), "UNX-F12267 判据未全绿");
    }

    #[test]
    fn unx12267_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
