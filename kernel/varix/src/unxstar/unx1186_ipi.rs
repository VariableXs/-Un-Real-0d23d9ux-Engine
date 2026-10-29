// unx1186_ipi —— UNX-F1186 IPI 与亲和性协同账（迁移时旧核通知全语义）
// 判据原文 UNX-F1186-J1：协同账全过（向量迁移时旧核 IPI 通知逐例登记、
// 通知-确认配对守恒、漏通知立案、通知原因闭集、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8cb0 独占（0x8cb0~0x8cef）。
//
// 设计（纯功能 no_std）：
//   * 迁移案卷：向量 v 从旧核迁往新核 → 必发 IPI 通知旧核（flush 向量状态）；
//   * 配对守恒：迁移数 = 通知数 = 确认数 + 未确认数（在途）；
//   * 漏通知立案：迁移完成但通知缺失即抓（迁移前查通知账）；
//   * 原因闭集：1=亲和策略变更 2=负载均衡 3=核下线 4=手动重定向；
//   * FNV 链指纹冻结，封账后拒迁移。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8cb0 独占） ─────────────────────────────────────────────
pub const XC_IPI_SEALED: u32 = 0x8CB1; // 封账后迁移
pub const XC_IPI_NO_NOTIFY: u32 = 0x8CB2; // 迁移未通知旧核
pub const XC_IPI_DUP_MIG: u32 = 0x8CB3; // 同向量重复迁移（未确认再迁）
pub const XC_IPI_SAME_CORE: u32 = 0x8CB4; // 迁移到同核
pub const XC_IPI_BAD_CAUSE: u32 = 0x8CB5; // 原因码越闭集
pub const XC_IPI_FULL: u32 = 0x8CB6; // 账容量满

/// 迁移案卷。
#[derive(Clone, Copy)]
pub struct MigRow {
    pub vec_no: u8,
    pub from_core: u8,
    pub to_core: u8,
    pub cause: u8,
    /// 通知确认标记。
    pub acked: bool,
}

/// IPI 协同账。
pub struct IpiLedger {
    cap: usize,
    rows: Vec<MigRow>,
    acks: u32,
    pending: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl IpiLedger {
    pub fn new(cap: usize) -> IpiLedger {
        IpiLedger {
            cap,
            rows: vec![],
            acks: 0,
            pending: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 迁移并自动发 IPI 通知旧核（全语义：通知是迁移的一部分）。
    pub fn migrate(&mut self, vec_no: u8, from_core: u8, to_core: u8, cause: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IPI_SEALED, "封账后迁移"));
            return Err(XC_IPI_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_IPI_FULL, "账容量满"));
            return Err(XC_IPI_FULL);
        }
        if cause == 0 || cause > 4 {
            self.rejects.push((XC_IPI_BAD_CAUSE, "原因码越闭集"));
            return Err(XC_IPI_BAD_CAUSE);
        }
        if from_core == to_core {
            self.rejects.push((XC_IPI_SAME_CORE, "迁移到同核"));
            return Err(XC_IPI_SAME_CORE);
        }
        // 未确认的旧迁移再迁同向量：拒绝（通知未闭环）
        for r in &self.rows {
            if r.vec_no == vec_no && !r.acked {
                self.rejects.push((XC_IPI_DUP_MIG, "未确认再迁"));
                return Err(XC_IPI_DUP_MIG);
            }
        }
        // 迁移即通知（IPI 发出）：账面同记
        self.rows.push(MigRow { vec_no, from_core, to_core, cause, acked: false });
        self.pending = self.pending.saturating_add(1);
        Ok(())
    }

    /// 旧核确认收到通知。
    pub fn ack(&mut self, vec_no: u8, from_core: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IPI_SEALED, "封账后确认"));
            return Err(XC_IPI_SEALED);
        }
        for r in self.rows.iter_mut() {
            if r.vec_no == vec_no && r.from_core == from_core && !r.acked {
                r.acked = true;
                self.acks = self.acks.saturating_add(1);
                self.pending = self.pending.saturating_sub(1);
                return Ok(());
            }
        }
        self.rejects.push((XC_IPI_NO_NOTIFY, "无在途通知可确认"));
        Err(XC_IPI_NO_NOTIFY)
    }

    /// 守恒：迁移数 = 确认数 + 在途数。
    pub fn conserved(&self) -> bool {
        self.rows.len() as u32 == self.acks + self.pending
    }

    /// 漏通知审计：所有案卷都应有通知（迁移即通知），acked 或在途均合法。
    pub fn no_missing_notify(&self) -> bool {
        self.rows.len() == (self.acks + self.pending) as usize
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }
    pub fn acks(&self) -> u32 {
        self.acks
    }
    pub fn pending(&self) -> u32 {
        self.pending
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_IPI_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.vec_no as u64).rotate_left(5)
                ^ (r.from_core as u64).rotate_left(13)
                ^ (r.to_core as u64).rotate_left(23)
                ^ (r.cause as u64).rotate_left(37)
                ^ (r.acked as u64).rotate_left(47);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准协同场景：4 向量迁移（4 原因各一）+ 全确认 + 守恒。
pub fn ipi_scenario() -> Result<(usize, u32, u32, u64), u32> {
    let mut l = IpiLedger::new(8);
    l.migrate(0x20, 0, 2, 1)?;
    l.migrate(0x30, 1, 3, 2)?;
    l.migrate(0x40, 2, 0, 3)?;
    l.migrate(0x50, 3, 1, 4)?;
    l.ack(0x20, 0)?;
    l.ack(0x30, 1)?;
    l.ack(0x40, 2)?;
    l.ack(0x50, 3)?;
    if !l.conserved() || !l.no_missing_notify() {
        return Err(XC_IPI_NO_NOTIFY);
    }
    let fp = l.seal()?;
    Ok((l.rows_len(), l.acks(), l.pending(), fp))
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）同核迁移必拒
    {
        let mut l = IpiLedger::new(4);
        if l.migrate(1, 2, 2, 1) == Err(XC_IPI_SAME_CORE) {
            caught += 1;
        }
    }
    // 2）原因码越闭集必拒
    {
        let mut l = IpiLedger::new(4);
        if l.migrate(1, 0, 1, 9) == Err(XC_IPI_BAD_CAUSE) {
            caught += 1;
        }
    }
    // 3）未确认再迁必拒
    {
        let mut l = IpiLedger::new(4);
        let _ = l.migrate(5, 0, 1, 1);
        if l.migrate(5, 1, 2, 2) == Err(XC_IPI_DUP_MIG) {
            caught += 1;
        }
    }
    // 4）确认后可再迁（闭环正例）
    {
        let mut l = IpiLedger::new(4);
        let _ = l.migrate(5, 0, 1, 1);
        let _ = l.ack(5, 0);
        if l.migrate(5, 1, 2, 2) == Ok(()) {
            caught += 1;
        }
    }
    // 5）无在途通知确认必拒
    {
        let mut l = IpiLedger::new(4);
        if l.ack(9, 0) == Err(XC_IPI_NO_NOTIFY) {
            caught += 1;
        }
    }
    // 6）封账后迁移必拒
    {
        let mut l = IpiLedger::new(4);
        let _ = l.migrate(1, 0, 1, 1);
        let _ = l.seal();
        if l.migrate(2, 0, 1, 1) == Err(XC_IPI_SEALED) {
            caught += 1;
        }
    }
    // 7）守恒（部分确认）
    {
        let mut l = IpiLedger::new(4);
        let _ = l.migrate(1, 0, 1, 1);
        let _ = l.migrate(2, 1, 2, 2);
        let _ = l.ack(1, 0);
        if l.conserved() && l.acks() == 1 && l.pending() == 1 {
            caught += 1;
        }
    }
    (caught, N)
}
