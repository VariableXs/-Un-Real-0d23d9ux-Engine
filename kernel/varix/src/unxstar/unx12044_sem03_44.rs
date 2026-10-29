//! UNX-F12044 BeingDebugged/NtGlobalFlag 调试位语义与唯一写者（0x197A0 段）
//!
//! 判据原文：调试位面双槽——BeingDebugged（布尔位）与 NtGlobalFlag
//! （位掩码面：有效位子集闭集）；两者仅内核唯一写者可改（用户态改写
//! 即拒）；NtGlobalFlag 置位必须落有效位闭集；两槽写均留痕对账。
//!
//! 结构：调试位面（DebugFace：双槽 × 唯一写者）+ 写核（四码守卫）+
//! 写账（DbRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12044_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x197A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x197A0;
pub const DB_USER_WRITE: u32 = SEG_BASE + 0x01; // 用户态改调试位
pub const DB_BAD_FLAG: u32 = SEG_BASE + 0x02; // NtGlobalFlag 越有效位
pub const DB_NOCHANGE: u32 = SEG_BASE + 0x03; // 同值重写
pub const DB_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造写账
pub const DB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍写

/// 内核唯一写者令牌。
pub const DBG_WRITER: u32 = 0x4442_4757; // "DBGW"

/// NtGlobalFlag 有效位闭集。
pub const GF_FLG_HEAP_ENABLE_TAIL_CHECK: u32 = 0x0000_0001;
pub const GF_FLG_HEAP_ENABLE_FREE_CHECK: u32 = 0x0000_0020;
pub const GF_FLG_HEAP_VALIDATE_PARAMETERS: u32 = 0x0000_0040;
pub const GF_FLG_HEAP_VALIDATE_ALL: u32 = 0x0000_0080;
pub const GF_FLG_APPLICATION_VERIFIER: u32 = 0x0000_0100;
pub const GF_VALID_MASK: u32 = GF_FLG_HEAP_ENABLE_TAIL_CHECK
    | GF_FLG_HEAP_ENABLE_FREE_CHECK
    | GF_FLG_HEAP_VALIDATE_PARAMETERS
    | GF_FLG_HEAP_VALIDATE_ALL
    | GF_FLG_APPLICATION_VERIFIER;

/// 一笔调试位写账。
#[derive(Clone)]
pub struct DbRec {
    pub seq: u32,
    /// 槽：1=BeingDebugged 2=NtGlobalFlag。
    pub slot: u8,
    pub old: u32,
    pub new: u32,
    pub note: String,
}

impl DbRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.old != self.new
    }
}

/// 调试位面模型。
pub struct DebugFace {
    pub being_debugged: u8,
    pub nt_global_flag: u32,
    pub writer: u32,
    pub ledger: Vec<DbRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl DebugFace {
    pub fn new() -> DebugFace {
        DebugFace {
            being_debugged: 0,
            nt_global_flag: 0,
            writer: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn attach_writer(&mut self, token: u32) {
        self.writer = token;
    }

    fn check_writer(&self) -> Result<(), u32> {
        if self.writer != DBG_WRITER {
            return Err(DB_USER_WRITE);
        }
        Ok(())
    }

    /// BeingDebugged 写核。
    pub fn set_debugged(&mut self, v: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DB_FROZEN);
        }
        if note.is_empty() {
            return Err(DB_BAD_REC);
        }
        self.check_writer()?;
        let val = if v != 0 { 1u8 } else { 0u8 };
        if val == self.being_debugged {
            return Err(DB_NOCHANGE);
        }
        let rec = DbRec {
            seq: self.seq_next,
            slot: 1,
            old: self.being_debugged as u32,
            new: val as u32,
            note: note.to_string(),
        };
        if !rec.ok() {
            return Err(DB_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        self.being_debugged = val;
        Ok(())
    }

    /// NtGlobalFlag 写核：有效位闭集守卫。
    pub fn set_global_flag(&mut self, v: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DB_FROZEN);
        }
        if note.is_empty() {
            return Err(DB_BAD_REC);
        }
        self.check_writer()?;
        if v & !GF_VALID_MASK != 0 {
            return Err(DB_BAD_FLAG);
        }
        if v == self.nt_global_flag {
            return Err(DB_NOCHANGE);
        }
        let rec = DbRec {
            seq: self.seq_next,
            slot: 2,
            old: self.nt_global_flag,
            new: v,
            note: note.to_string(),
        };
        if !rec.ok() {
            return Err(DB_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        self.nt_global_flag = v;
        Ok(())
    }

    /// 有效位掩码复核。
    pub fn flag_in_mask(v: u32) -> bool {
        v & !GF_VALID_MASK == 0
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.being_debugged as u32);
        h = h.wrapping_mul(211).wrapping_add(self.nt_global_flag);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 用户态写（未挂令牌）→ DB_USER_WRITE。
    let mut d = DebugFace::new();
    got[0] = d.set_debugged(1, "u").err() == Some(DB_USER_WRITE);
    d.attach_writer(DBG_WRITER);
    // 2) NtGlobalFlag 越有效位（位 15）→ DB_BAD_FLAG。
    got[1] = d.set_global_flag(0x8000, "wild").err() == Some(DB_BAD_FLAG);
    // 3) 同值重写 → DB_NOCHANGE。
    let _ = d.set_debugged(1, "ok");
    got[2] = d.set_debugged(1, "dup").err() == Some(DB_NOCHANGE);
    // 4) 空留痕 → DB_BAD_REC。
    got[3] = d.set_global_flag(GF_FLG_HEAP_VALIDATE_ALL, "").err() == Some(DB_BAD_REC);
    // 5) 印账后写 → DB_FROZEN。
    d.seal();
    got[4] = d.set_debugged(0, "post").err() == Some(DB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12044_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12044");

    // J1 正路径：调试位开→关，全局旗置位。
    let j1 = {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        let _ = d.set_debugged(1, "on");
        let on = d.being_debugged == 1;
        let _ = d.set_debugged(0, "off");
        let off = d.being_debugged == 0;
        let _ = d.set_global_flag(GF_FLG_HEAP_VALIDATE_ALL, "gf");
        on && off && d.nt_global_flag == GF_FLG_HEAP_VALIDATE_ALL
    };
    s.add("J1-调试位双槽", j1, "开-关-置旗全通");

    // J2 用户态写拒（未挂令牌与伪令牌双面）。
    let j2 = {
        let mut a = DebugFace::new();
        let no = a.set_debugged(1, "u").err() == Some(DB_USER_WRITE);
        let mut b = DebugFace::new();
        b.attach_writer(DBG_WRITER + 1);
        let fake = b.set_global_flag(1, "f").err() == Some(DB_USER_WRITE);
        no && fake
    };
    s.add("J2-用户写拒", j2, "无令牌/伪令牌全拒");

    // J3 有效位闭集：全有效位过、越位拒。
    let j3 = {
        let all = DebugFace::flag_in_mask(GF_VALID_MASK);
        let wild = !DebugFace::flag_in_mask(0x8000);
        all && wild
    };
    s.add("J3-有效位闭集", j3, "全掩码过/越位拒");

    // J4 组合置位面：三旗组合可清点。
    let j4 = {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        let combo = GF_FLG_HEAP_ENABLE_TAIL_CHECK
            | GF_FLG_HEAP_ENABLE_FREE_CHECK
            | GF_FLG_HEAP_VALIDATE_PARAMETERS;
        let _ = d.set_global_flag(combo, "c");
        d.nt_global_flag.count_ones() == 3
    };
    s.add("J4-组合置位", j4, "三旗三位");

    // J5 同值重写拒（双槽）。
    let j5 = {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        let _ = d.set_debugged(1, "a");
        let _ = d.set_global_flag(GF_FLG_HEAP_VALIDATE_ALL, "b");
        let dup1 = d.set_debugged(1, "c").err() == Some(DB_NOCHANGE);
        let dup2 =
            d.set_global_flag(GF_FLG_HEAP_VALIDATE_ALL, "e").err() == Some(DB_NOCHANGE);
        dup1 && dup2
    };
    s.add("J5-同值拒", j5, "双槽空变更全拒");

    // J6 调试位归一：非零值一律记 1。
    let j6 = {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        let _ = d.set_debugged(7, "x");
        d.being_debugged == 1
    };
    s.add("J6-布尔回一", j6, "7→1 归一");

    // J7 账目留痕：槽号与旧值对账。
    let j7 = {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        let _ = d.set_debugged(1, "a");
        let _ = d.set_global_flag(GF_FLG_HEAP_VALIDATE_ALL, "b");
        d.ledger[0].slot == 1 && d.ledger[0].old == 0 && d.ledger[1].slot == 2 && d.ledger_ok()
    };
    s.add("J7-双槽留痕", j7, "slot 1/2 对账");

    // J8 指纹区分度与幂等。
    let j8 = {
        let mut a = DebugFace::new();
        a.attach_writer(DBG_WRITER);
        let _ = a.set_debugged(1, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = DebugFace::new();
        b.attach_writer(DBG_WRITER);
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J8-指纹性质", j8, "幂等且异面异纹");

    // J9 印账冻结。
    let j9 = {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        d.seal();
        d.set_debugged(1, "post").err() == Some(DB_FROZEN)
            && d.set_global_flag(1, "post").err() == Some(DB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "双面操作印后全拒");

    // J10 钉值核对：写者令牌与有效位表。
    let j10 = DBG_WRITER == 0x4442_4757 && GF_VALID_MASK == 0x1E1;
    s.add("J10-钉值核对", j10, "DBGW/0x1E1");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：被调试场景。
    let mut d = DebugFace::new();
    d.attach_writer(DBG_WRITER);
    let _ = d.set_debugged(1, "sum-dbg");
    let _ = d.set_global_flag(GF_FLG_HEAP_ENABLE_TAIL_CHECK, "sum-gf");
    s.add("样例调试面", d.being_debugged == 1 && d.nt_global_flag == 0x1, "位+尾查旗");
    s.add("样例账面合法", d.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12044_debug_flow() {
        let mut d = DebugFace::new();
        d.attach_writer(DBG_WRITER);
        assert!(d.set_debugged(1, "a").is_ok());
        assert_eq!(d.set_debugged(1, "b"), Err(DB_NOCHANGE));
        assert_eq!(d.set_global_flag(0x8000, "c"), Err(DB_BAD_FLAG));
        assert!(d.set_global_flag(GF_FLG_HEAP_VALIDATE_ALL, "d").is_ok());
        assert_eq!(d.nt_global_flag, 0x80);
    }

    #[test]
    fn unx12044_checks_green() {
        let s = run_unx12044_checks();
        assert!(s.all_passed(), "UNX-F12044 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12044_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
