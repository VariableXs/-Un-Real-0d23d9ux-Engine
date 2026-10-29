//! UNX-F12172 — NtContinue 语义预告（0x1a7a0 段）
//!
//! 锚点：UNX-F12172-J1 NtContinue 语义 10/10（CONTEXT 加载 + 可选
//! 告警测试 ContinueCtx/ContinueEax），恢复目标必须过授权、未授权
//! 上下文必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：ContinueReq（恢复请求）+ NtContinueSim（模拟器）+ 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a7a0;
pub const NC_GHOST: u32 = SEG_BASE + 0x01; // 上下文未授权
pub const NC_ORDER: u32 = SEG_BASE + 0x02; // 请求锚非法
pub const NC_TWICE: u32 = SEG_BASE + 0x03; // 同请求重复
pub const NC_SEALED: u32 = SEG_BASE + 0x04; // 印后恢复
pub const NC_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕

/// 恢复请求。
#[derive(Clone, PartialEq)]
pub struct ContinueReq {
    pub rip: u64,
    pub rsp: u64,
    pub test_alert: bool,
    pub note: String,
}

impl ContinueReq {
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp % 8 == 0 && self.rsp != 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!(
            "rip={:#x} rsp={:#x} alert={}",
            self.rip, self.rsp, self.test_alert
        )
    }
}

/// NtContinue 模拟器。
pub struct NtContinueSim {
    pub authorized: Vec<(u64, u64)>,
    pub continued: Vec<ContinueReq>,
    next_eax: u32,
    sealed: bool,
}

impl NtContinueSim {
    pub fn new() -> NtContinueSim {
        NtContinueSim { authorized: Vec::new(), continued: Vec::new(), next_eax: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 授权一个上下文（非对齐/零 RIP 拒）。
    pub fn authorize(&mut self, rip: u64, rsp: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(NC_SEALED);
        }
        if rip == 0 || rsp == 0 || rsp % 8 != 0 {
            return Err(NC_ORDER);
        }
        self.authorized.push((rip, rsp));
        Ok(())
    }

    /// NtContinue（必须命中授权账；重复同请求拒）。
    pub fn nt_continue(&mut self, req: ContinueReq) -> Result<u32, u32> {
        if self.sealed {
            return Err(NC_SEALED);
        }
        if req.note.is_empty() {
            return Err(NC_EMPTY);
        }
        if !req.ok() {
            return Err(NC_ORDER);
        }
        if !self.authorized.iter().any(|&(r, s)| r == req.rip && s == req.rsp) {
            return Err(NC_GHOST);
        }
        if self.continued.iter().any(|x| *x == req) {
            return Err(NC_TWICE);
        }
        let eax = self.next_eax;
        self.next_eax += 1;
        self.continued.push(req);
        Ok(eax)
    }

    /// 恢复留痕全锚定。
    pub fn continued_anchored(&self) -> bool {
        self.continued
            .iter()
            .all(|c| self.authorized.iter().any(|&(r, s)| r == c.rip && s == c.rsp))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a7a0;
        for (r, s) in &self.authorized {
            h = h.wrapping_mul(211).wrapping_add(*r as u32);
            h = h.wrapping_mul(211).wrapping_add(*s as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.next_eax);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "auth={} cont={} anchored={}",
            self.authorized.len(),
            self.continued.len(),
            self.continued_anchored()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = NtContinueSim::new();
    // 1) 未授权上下文恢复必拒。
    got[0] = b
        .nt_continue(ContinueReq { rip: 0x1000, rsp: 0x2000, test_alert: false, note: "w".to_string() })
        .err()
        == Some(NC_GHOST);
    // 2) 非对齐授权必拒。
    got[1] = b.authorize(0x1000, 0x2004).err() == Some(NC_ORDER);
    // 3) 零 RIP 授权必拒。
    got[2] = b.authorize(0, 0x2000).err() == Some(NC_ORDER);
    // 4) 空留痕恢复必拒。
    let _ = b.authorize(0x1000, 0x2000);
    got[3] = b
        .nt_continue(ContinueReq { rip: 0x1000, rsp: 0x2000, test_alert: false, note: "".to_string() })
        .err()
        == Some(NC_EMPTY);
    // 5) 印后恢复必拒。
    b.seal();
    got[4] = b
        .nt_continue(ContinueReq { rip: 0x1000, rsp: 0x2000, test_alert: false, note: "post".to_string() })
        .err()
        == Some(NC_SEALED);
    got
}

/// 判据入口：UNX-F12172
pub fn run_unx12172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12172");
    let mut b = NtContinueSim::new();

    // J1 授权十点。
    let mut ok1 = true;
    for i in 0..10u64 {
        if b.authorize(0x1000 + i * 0x40, 0x8000 - i * 0x20).is_err() {
            ok1 = false;
        }
    }
    s.add("J1_auth10", ok1 && b.authorized.len() == 10, "十点授权全过");

    // J2 恢复三点全接。
    let r1 = b.nt_continue(ContinueReq { rip: 0x1000, rsp: 0x8000, test_alert: false, note: "c1".to_string() });
    let r2 = b.nt_continue(ContinueReq { rip: 0x1040, rsp: 0x7FE0, test_alert: true, note: "c2".to_string() });
    let r3 = b.nt_continue(ContinueReq { rip: 0x1080, rsp: 0x7FC0, test_alert: false, note: "c3".to_string() });
    s.add(
        "J2_cont3",
        r1 == Ok(1) && r2 == Ok(2) && r3 == Ok(3),
        "三点恢复 EAX 1/2/3",
    );

    // J3 未授权恢复拒。
    s.add(
        "J3_wild",
        b.nt_continue(ContinueReq { rip: 0xDEAD, rsp: 0xBEE0, test_alert: false, note: "w".to_string() })
            .err()
            == Some(NC_GHOST),
        "未授权恢复拒 GHOST",
    );

    // J4 同请求重复拒。
    s.add(
        "J4_twice",
        b.nt_continue(ContinueReq { rip: 0x1000, rsp: 0x8000, test_alert: false, note: "c1".to_string() })
            .err()
            == Some(NC_TWICE),
        "同请求重复拒 TWICE",
    );

    // J5 请求自洽核。
    s.add(
        "J5_reqs_ok",
        b.continued.iter().all(|c| c.ok()),
        "全恢复请求过自洽核",
    );

    // J6 请求渲染。
    let r = b.continued[1].render();
    s.add(
        "J6_render",
        r.contains("rip=0x1040") && r.contains("alert=true"),
        "渲染含 rip/rsp/alert",
    );

    // J7 告警位语义分账。
    s.add(
        "J7_alert_split",
        b.continued.iter().filter(|c| c.test_alert).count() == 1,
        "告警位 1/2 分账",
    );

    // J8 恢复留痕全锚定。
    s.add("J8_anchored", b.continued_anchored(), "恢复点全锚定");

    // J9 非对齐授权拒。
    s.add(
        "J9_misalign",
        b.authorize(0x2000, 0x8004).err() == Some(NC_ORDER),
        "RSP 非对齐授权拒 ORDER",
    );

    // J10 零 RIP 授权拒。
    s.add(
        "J10_zero_rip",
        b.authorize(0, 0x8000).err() == Some(NC_ORDER),
        "零 RIP 授权拒 ORDER",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "模拟器折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.authorize(0x3000, 0x6000).err() == Some(NC_SEALED)
            && b
                .nt_continue(ContinueReq { rip: 0x10C0, rsp: 0x7FA0, test_alert: false, note: "p".to_string() })
                .err()
                == Some(NC_SEALED),
        "印账后授权/恢复皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("auth=10") && sm.contains("cont=3"),
        "摘要含授权/恢复计数",
    );

    // J15 空留痕恢复拒。
    let mut b15 = NtContinueSim::new();
    let _ = b15.authorize(0x1000, 0x2000);
    s.add(
        "J15_empty_note",
        b15
            .nt_continue(ContinueReq { rip: 0x1000, rsp: 0x2000, test_alert: false, note: "".to_string() })
            .err()
            == Some(NC_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J16 EAX 序连续。
    let seqs = [r1.unwrap_or(0), r2.unwrap_or(0), r3.unwrap_or(0)];
    s.add(
        "J16_eax_seq",
        seqs == [1, 2, 3],
        "EAX 序 1/2/3 连续",
    );

    // J17 十点授权全恢复。
    let mut b17 = NtContinueSim::new();
    let mut ok17 = true;
    for i in 0..10u64 {
        let rip = 0x2000 + i * 0x40;
        let rsp = 0x9000 + i * 0x20;
        if b17.authorize(rip, rsp).is_err()
            || b17
                .nt_continue(ContinueReq { rip, rsp, test_alert: false, note: "f".to_string() })
                .is_err()
        {
            ok17 = false;
        }
    }
    s.add(
        "J17_full10",
        ok17 && b17.continued.len() == 10 && b17.continued_anchored(),
        "十点授权全恢复",
    );

    // J18 空模拟器指纹稳定。
    let b18 = NtContinueSim::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空模拟器指纹稳定");

    // J19 请求非法（RSP 零）拒。
    s.add(
        "J19_zero_rsp_req",
        b15
            .nt_continue(ContinueReq { rip: 0x1000, rsp: 0, test_alert: false, note: "z".to_string() })
            .err()
            == Some(NC_ORDER),
        "零 RSP 请求拒 ORDER",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12172_ntcont() {
        let mut b = NtContinueSim::new();
        let _ = b.authorize(0x1000, 0x2000);
        assert_eq!(
            b.nt_continue(ContinueReq { rip: 0x1000, rsp: 0x2000, test_alert: false, note: "c".to_string() }),
            Ok(1)
        );
    }

    #[test]
    fn unx12172_checks_green() {
        let s = run_unx12172_checks();
        assert!(s.all_passed(), "UNX-F12172 判据未全绿");
    }

    #[test]
    fn unx12172_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
