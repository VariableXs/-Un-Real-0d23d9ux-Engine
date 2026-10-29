// unx1056_u1056 —— UNX-F1056 仲裁与中断面联动对账（F0971 需求增量对账）
// 判据原文 UNX-F1056-J1：增量对账完整（B13 交付项对 F0971 需求核对——满足率 100%），口径一致断言，对账报告增量归档。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2AC0 独占（0x2AC1~0x2ACF）。
//
// 设计（纯功能 no_std）：
//   * 需求增量：F0971 需求条目与 B13 交付条目逐条对账（需求号→交付号映射）；
//   * 口径一致：每条对账带口径码（metric 语义），口径漂移即拒；
//   * 满足率 100%：全需求有交付覆盖才可封账；
//   * 增量归档：对账报告逐条归档（只增不改），指纹冻结；
//   * saturating 防溢出 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2AC0 独占） ─────────────────────────────────────────────
pub const XC_RC_OOB: u32 = 0x2AC1; // 需求号/口径越界
pub const XC_RC_DUP: u32 = 0x2AC2; // 需求号重复
pub const XC_RC_SEALED: u32 = 0x2AC3; // 归档后改账
pub const XC_RC_CALIPER: u32 = 0x2AC4; // 口径不一致
pub const XC_RC_UNCOVERED: u32 = 0x2AC5; // 需求无交付覆盖

pub const F0971_REQS: usize = 8;
pub const REQ_IDS: [u32; 8] = [0x0971, 0x0972, 0x0973, 0x0974, 0x0975, 0x0976, 0x0977, 0x0978];
/// 口径码：0=计数 1=时延 2=丢率 3=覆盖率（全账统一）。
pub const CALIPERS: [u8; 4] = [0, 1, 2, 3];

#[derive(Clone, Copy)]
pub struct ReqItem {
    pub req_id: u32,     // F0971 需求号
    pub deliver_id: u32, // B13 交付号
    pub caliper: u8,     // 口径码
}

#[derive(Clone, Copy)]
pub struct ArchiveRow {
    pub req_id: u32,
    pub deliver_id: u32,
    pub caliper: u8,
    pub satisfied: bool,
}

/// 对账账本。
pub struct ReconcileBook {
    reqs: Vec<ReqItem>,
    seen_req: Vec<u32>,
    archive: Vec<ArchiveRow>,
    global_caliper: u8,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ReconcileBook {
    pub fn new(global_caliper: u8) -> ReconcileBook {
        ReconcileBook {
            reqs: Vec::new(),
            seen_req: Vec::new(),
            archive: Vec::new(),
            global_caliper,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn caliper_legal(c: u8) -> bool {
        CALIPERS.iter().any(|&x| x == c)
    }

    /// 登记一条需求-交付映射（口径必须与全账一致）。
    pub fn map(&mut self, r: ReqItem) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RC_SEALED, "归档后改账"));
            return Err(XC_RC_SEALED);
        }
        if !Self::caliper_legal(r.caliper) {
            self.rejects.push((XC_RC_OOB, "口径码非法"));
            return Err(XC_RC_OOB);
        }
        if r.caliper != self.global_caliper {
            self.rejects.push((XC_RC_CALIPER, "口径漂移"));
            return Err(XC_RC_CALIPER);
        }
        if r.req_id == 0 || r.deliver_id == 0 {
            self.rejects.push((XC_RC_OOB, "编号为零"));
            return Err(XC_RC_OOB);
        }
        for &q in &self.seen_req {
            if q == r.req_id {
                self.rejects.push((XC_RC_DUP, "需求号重复"));
                return Err(XC_RC_DUP);
            }
        }
        self.seen_req.push(r.req_id);
        self.reqs.push(r);
        Ok(())
    }

    /// 归档一条对账结果（只增不改）。
    pub fn archive_row(&mut self, row: ArchiveRow) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RC_SEALED, "归档后追加"));
            return Err(XC_RC_SEALED);
        }
        if row.caliper != self.global_caliper {
            self.rejects.push((XC_RC_CALIPER, "归档口径漂移"));
            return Err(XC_RC_CALIPER);
        }
        self.archive.push(row);
        Ok(())
    }

    /// 增量覆盖核验：F0971 全部需求有交付映射且已归档。
    pub fn full_coverage(&self) -> bool {
        for want in REQ_IDS.iter() {
            let mapped = self.reqs.iter().any(|r| r.req_id == *want);
            let archived = self.archive.iter().any(|a| a.req_id == *want && a.satisfied);
            if !mapped || !archived {
                return false;
            }
        }
        true
    }

    /// 未覆盖需求清单。
    pub fn uncovered(&self) -> Vec<u32> {
        let mut miss = Vec::new();
        for want in REQ_IDS.iter() {
            let covered = self.reqs.iter().any(|r| r.req_id == *want);
            if !covered {
                miss.push(*want);
            }
        }
        miss
    }

    /// 满足率（万分比）：已归档且 satisfied 的占比（对 F0971 全需求）。
    pub fn satisfy_rate(&self) -> u32 {
        let mut sat = 0u32;
        for want in REQ_IDS.iter() {
            if self.archive.iter().any(|a| a.req_id == *want && a.satisfied) {
                sat = sat.saturating_add(1);
            }
        }
        sat.saturating_mul(10_000).saturating_div(F0971_REQS as u32)
    }

    /// 守恒：映射数 = 归档数 = 需求总数。
    pub fn conserved(&self) -> bool {
        self.reqs.len() == self.seen_req.len()
            && self.reqs.len() == self.archive.len()
            && self.reqs.len() == F0971_REQS
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_RC_SEALED);
        }
        if !self.full_coverage() {
            self.rejects.push((XC_RC_UNCOVERED, "需求缺覆盖禁封"));
            return Err(XC_RC_UNCOVERED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.global_caliper as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in &self.reqs {
            h ^= r.req_id ^ r.deliver_id.wrapping_mul(0x9E37) ^ r.caliper as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        for a in &self.archive {
            h ^= a.req_id ^ (a.satisfied as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h as u64
    }

    pub fn archive_len(&self) -> usize {
        self.archive.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）口径漂移必拒（10/10）
    {
        let mut b = ReconcileBook::new(0);
        let mut hits = 0;
        for k in 1..11u8 {
            if b.map(ReqItem { req_id: 0x1000 + k as u32, deliver_id: 1, caliper: k % 4 + 4 }) == Err(XC_RC_OOB)
                || b.map(ReqItem { req_id: 0x2000 + k as u32, deliver_id: 1, caliper: (k % 3) + 1 }) == Err(XC_RC_CALIPER)
            {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）需求号重复必拒
    {
        let mut b = ReconcileBook::new(0);
        let _ = b.map(ReqItem { req_id: 0x0971, deliver_id: 1, caliper: 0 });
        if b.map(ReqItem { req_id: 0x0971, deliver_id: 2, caliper: 0 }) == Err(XC_RC_DUP) {
            caught += 1;
        }
    }
    // 3）编号为零必拒
    {
        let mut b = ReconcileBook::new(0);
        if b.map(ReqItem { req_id: 0, deliver_id: 1, caliper: 0 }) == Err(XC_RC_OOB) {
            caught += 1;
        }
    }
    // 4）需求缺覆盖禁封
    {
        let mut b = ReconcileBook::new(0);
        // 只登记一条
        let _ = b.map(ReqItem { req_id: REQ_IDS[0], deliver_id: 1, caliper: 0 });
        let _ = b.archive_row(ArchiveRow { req_id: REQ_IDS[0], deliver_id: 1, caliper: 0, satisfied: true });
        if b.seal() == Err(XC_RC_UNCOVERED) {
            caught += 1;
        }
    }
    // 5）归档后改账必拒
    {
        let mut b = ReconcileBook::new(0);
        for (i, want) in REQ_IDS.iter().enumerate() {
            let _ = b.map(ReqItem { req_id: *want, deliver_id: 100 + i as u32, caliper: 0 });
            let _ = b.archive_row(ArchiveRow { req_id: *want, deliver_id: 100 + i as u32, caliper: 0, satisfied: true });
        }
        let _ = b.seal();
        if b.map(ReqItem { req_id: 0x0999, deliver_id: 1, caliper: 0 }) == Err(XC_RC_SEALED) {
            caught += 1;
        }
    }
    // 6）未满足归档不算覆盖
    {
        let mut b = ReconcileBook::new(0);
        for (i, want) in REQ_IDS.iter().enumerate() {
            let _ = b.map(ReqItem { req_id: *want, deliver_id: 100 + i as u32, caliper: 0 });
            let sat = i != 0; // 第一条不满足
            let _ = b.archive_row(ArchiveRow { req_id: *want, deliver_id: 100 + i as u32, caliper: 0, satisfied: sat });
        }
        if !b.full_coverage() && b.satisfy_rate() == 8750 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1056-J1。
pub fn run_unx1056_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1056");

    // C56-01 F0971 全需求映射
    {
        let mut b = ReconcileBook::new(0);
        let mut all_ok = true;
        for (i, want) in REQ_IDS.iter().enumerate() {
            if b.map(ReqItem { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0 }).is_err() {
                all_ok = false;
            }
        }
        s.add("C56-01-全需求映射", all_ok && b.reqs_len() == F0971_REQS, if all_ok { "" } else { "映射缺" });
    }
    // C56-02 增量对账归档
    {
        let mut b = ReconcileBook::new(0);
        for (i, want) in REQ_IDS.iter().enumerate() {
            let _ = b.map(ReqItem { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0 });
            let _ = b.archive_row(ArchiveRow { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0, satisfied: true });
        }
        s.add("C56-02-增量归档", b.archive_len() == F0971_REQS && b.conserved(), "");
    }
    // C56-03 满足率 100%
    {
        let mut b = ReconcileBook::new(0);
        for (i, want) in REQ_IDS.iter().enumerate() {
            let _ = b.map(ReqItem { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0 });
            let _ = b.archive_row(ArchiveRow { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0, satisfied: true });
        }
        s.add("C56-03-满足率100%", b.satisfy_rate() == 10_000 && b.full_coverage(), "");
    }
    // C56-04 口径一致断言
    {
        let mut b = ReconcileBook::new(2);
        let _ = b.map(ReqItem { req_id: 0x0971, deliver_id: 1, caliper: 2 });
        let ok = b.map(ReqItem { req_id: 0x0972, deliver_id: 2, caliper: 1 }) == Err(XC_RC_CALIPER);
        s.add("C56-04-口径一致", ok, if ok { "" } else { "口径漏抓" });
    }
    // C56-05 口径码非法拒
    {
        let mut b = ReconcileBook::new(0);
        let ok = b.map(ReqItem { req_id: 0x0971, deliver_id: 1, caliper: 9 }) == Err(XC_RC_OOB);
        s.add("C56-05-口径非法拒", ok, if ok { "" } else { "漏抓" });
    }
    // C56-06 未覆盖清单
    {
        let mut b = ReconcileBook::new(0);
        for want in REQ_IDS.iter().take(5) {
            let _ = b.map(ReqItem { req_id: *want, deliver_id: 1, caliper: 0 });
        }
        let miss = b.uncovered();
        let ok = miss.len() == 3 && miss[0] == REQ_IDS[5];
        s.add("C56-06-未覆盖清单", ok, if ok { "" } else { "清单漂移" });
    }
    // C56-07 封账产指纹
    {
        let mut b = ReconcileBook::new(0);
        for (i, want) in REQ_IDS.iter().enumerate() {
            let _ = b.map(ReqItem { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0 });
            let _ = b.archive_row(ArchiveRow { req_id: *want, deliver_id: 0xB100 + i as u32, caliper: 0, satisfied: true });
        }
        s.add("C56-07-封账指纹", b.seal().is_ok(), "");
    }
    // C56-08 指纹一致
    {
        let f = || {
            let mut b = ReconcileBook::new(1);
            let _ = b.map(ReqItem { req_id: 0x0971, deliver_id: 7, caliper: 1 });
            b.fingerprint()
        };
        s.add("C56-08-指纹一致", f() == f(), "");
    }
    // C56-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C56-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C56-10 拒收计数
    {
        let mut b = ReconcileBook::new(0);
        let _ = b.map(ReqItem { req_id: 0x0971, deliver_id: 1, caliper: 0 });
        let _ = b.map(ReqItem { req_id: 0x0971, deliver_id: 1, caliper: 0 });
        s.add("C56-10-拒收计数", b.reject_count() == 1, if b.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}

impl ReconcileBook {
    pub fn reqs_len(&self) -> usize {
        self.reqs.len()
    }
}
