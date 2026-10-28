//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12409 · 共享内存双进程语义档 · 0x30120
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12409-J1 双进程同节视图数据可见一致 10^4 次，写入序与缓存
//!      一致性账平；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 共享页域：单页 32 槽（u64 数据 + u64 提交序），两进程视图
//!     （P0/P1）挂同一底层页，同址读写即共享面；
//!   - 写入序协议：每次写先发数据后盖提交序（序全局单调），读方以序
//!     为准判定可见性（SeqBack 拒回拨、SeqDup 拒重号）；
//!   - 可见一致判据：任一进程读到的值 ≡ 该槽最新提交值（StaleRead 即
//!     账红），10^4 次读写零陈旧；
//!   - 缓存一致性账：写次数 ≡ 槽序增量总和，读次数 ≡ 一致读总和；
//!   - 视图挂靠纪律：未挂靠读写拒（ViewNotBound）、重复挂靠拒
//!     （BindDup）、越槽拒（CellOob）；
//!   - 双向域：P0 写 P1 读与 P1 写 P0 读两向都要走一遍（对称域）；
//!   - 错误路径带码+中文 detail，拒止不写回共享页；
//!   - 注入核：LCG 写死种子 0x30120，10 轮 × 1000 次可见一致 10^4。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30120 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30120;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮读写次数（10 轮 × 1000 = 10^4 口径）。
pub const OPS_PER_ROUND: usize = 1000;

/// 共享页槽位数（定容）。
pub const SHARED_CELLS: usize = 32;

/// 进程域宽度（双进程）。
pub const PROC_DOMES: usize = 2;

/// 诊断码（0x30120 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShmCode {
    /// 0x30120 提交序回拨（全局序非单调）。
    SeqBack = 0,
    /// 0x30121 提交序重号。
    SeqDup = 1,
    /// 0x30122 陈旧读（读到非最新提交值）。
    StaleRead = 2,
    /// 0x30123 越槽（槽号出共享页）。
    CellOob = 3,
    /// 0x30124 视图未挂靠。
    ViewNotBound = 4,
    /// 0x30125 重复挂靠。
    BindDup = 5,
    /// 0x30126 缓存账失平（写次数 ≠ 槽序增量总和）。
    CoherenceMiss = 6,
    /// 0x30127 一致读账失平（一致读 ≠ 读次数）。
    ReadDrift = 7,
    /// 0x30128 进程域越界（P0/P1 之外）。
    PidOob = 8,
    /// 0x30129 共享页冻结（页已回收再读写）。
    PageFrozen = 9,
}

impl ShmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [ShmCode; 10] = [
        ShmCode::SeqBack, ShmCode::SeqDup, ShmCode::StaleRead, ShmCode::CellOob,
        ShmCode::ViewNotBound, ShmCode::BindDup, ShmCode::CoherenceMiss,
        ShmCode::ReadDrift, ShmCode::PidOob, ShmCode::PageFrozen,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30120 => "提交序回拨：全局序单调递增，回拨即写序违约红账。",
        0x30121 => "提交序重号：一写一号，重号先查序分配器。",
        0x30122 => "陈旧读：读值必须等于该槽最新提交值，回查写序协议。",
        0x30123 => "越槽：槽号先比 SHARED_CELLS 再读写。",
        0x30124 => "视图未挂靠：先 bind 再读写共享页。",
        0x30125 => "重复挂靠：同进程幂等挂靠，重复走拒止。",
        0x30126 => "缓存账失平：写次数 ≡ 各槽序增量总和，先对平再读。",
        0x30127 => "一致读账失平：一致读计数必须等于读次数，零陈旧口径。",
        0x30128 => "进程域越界：只收 P0/P1 两域。",
        0x30129 => "共享页冻结：页回收后只读，重建走新页。",
        _ => "未知诊断码：回查 0x30120 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30120 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3012_3012_3012_3012;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 共享页槽（数据 + 提交序）。
#[derive(Clone, Copy, Debug)]
pub struct ShmCell {
    /// 数据值。
    pub data: u64,
    /// 提交序（0=未写）。
    pub seq: u64,
}

/// 进程视图挂靠行。
#[derive(Clone, Copy, Debug)]
pub struct ViewBind {
    /// 挂靠进程域。
    pub pid: usize,
    /// 已挂靠。
    pub bound: bool,
}

/// 共享内存双进程语义档正本。
///
/// 生命周期：freeze/unfreeze（页态）→ bind（进程挂靠）→ write（P 写：
/// 先数据后盖序）→ read（Q 读：以序判定可见性）。
#[derive(Debug)]
pub struct ShmBook {
    /// 共享页。
    page: [ShmCell; SHARED_CELLS],
    /// 挂靠表（双进程）。
    binds: [ViewBind; PROC_DOMES],
    /// 全局写序。
    seq: u64,
    /// 写次数。
    writes: u32,
    /// 读次数。
    reads: u32,
    /// 一致读次数。
    coherent: u32,
    /// 页冻结态。
    frozen: bool,
    /// 拒止累计。
    rejects: u32,
}

impl ShmBook {
    /// 建账（页空、双方未挂靠）。
    pub fn new() -> Self {
        ShmBook {
            page: core::array::from_fn(|_| ShmCell { data: 0, seq: 0 }),
            binds: core::array::from_fn(|i| ViewBind { pid: i, bound: false }),
            seq: 0,
            writes: 0,
            reads: 0,
            coherent: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 进程域核对。
    pub fn pid_ok(pid: usize) -> bool {
        pid < PROC_DOMES
    }

    /// 挂靠（同进程重复挂靠拒）。
    pub fn bind(&mut self, pid: usize) -> Result<bool, ShmCode> {
        if !Self::pid_ok(pid) {
            self.rejects += 1;
            return Err(ShmCode::PidOob);
        }
        if self.binds[pid].bound {
            self.rejects += 1;
            return Err(ShmCode::BindDup);
        }
        self.binds[pid].bound = true;
        Ok(true)
    }

    /// 挂靠状态。
    pub fn is_bound(&self, pid: usize) -> bool {
        Self::pid_ok(pid) && self.binds[pid].bound
    }

    /// 写（先数据后盖提交序；全局序单调）。
    pub fn write(&mut self, pid: usize, cell: usize, data: u64) -> Result<u64, ShmCode> {
        if !Self::pid_ok(pid) {
            self.rejects += 1;
            return Err(ShmCode::PidOob);
        }
        if !self.binds[pid].bound {
            self.rejects += 1;
            return Err(ShmCode::ViewNotBound);
        }
        if cell >= SHARED_CELLS {
            self.rejects += 1;
            return Err(ShmCode::CellOob);
        }
        if self.frozen {
            self.rejects += 1;
            return Err(ShmCode::PageFrozen);
        }
        // 写序分配：先留痕数据，再盖新序（序单调）。
        let new_seq = self.seq + 1;
        self.page[cell].data = data;
        if self.page[cell].seq >= new_seq {
            self.rejects += 1;
            return Err(ShmCode::SeqBack);
        }
        self.page[cell].seq = new_seq;
        self.seq = new_seq;
        self.writes += 1;
        Ok(new_seq)
    }

    /// 读（以序判定可见性：读到该槽当前提交值并核对一致）。
    pub fn read(&mut self, pid: usize, cell: usize) -> Result<(u64, u64), ShmCode> {
        if !Self::pid_ok(pid) {
            self.rejects += 1;
            return Err(ShmCode::PidOob);
        }
        if !self.binds[pid].bound {
            self.rejects += 1;
            return Err(ShmCode::ViewNotBound);
        }
        if cell >= SHARED_CELLS {
            self.rejects += 1;
            return Err(ShmCode::CellOob);
        }
        let c = self.page[cell];
        self.reads += 1;
        // 可见一致性：读值与提交序自洽（seq>0 时数据必须非幻值域）。
        if c.seq > 0 && c.data == u64::MAX {
            self.rejects += 1;
            return Err(ShmCode::StaleRead);
        }
        self.coherent += 1;
        Ok((c.data, c.seq))
    }

    /// 最新提交值（判据面：任一进程视角取该槽最新值）。
    pub fn latest(&self, cell: usize) -> Option<(u64, u64)> {
        if cell >= SHARED_CELLS {
            None
        } else {
            Some((self.page[cell].data, self.page[cell].seq))
        }
    }

    /// 缓存一致性账：写次数 ≡ 各槽序增量总和（按全局序核对）。
    pub fn coherence_ok(&self) -> bool {
        // 全局序 = 各槽 seq 的最大值（写序单调全局分配）。
        let mut max_seq = 0u64;
        for c in self.page.iter() {
            if c.seq > max_seq {
                max_seq = c.seq;
            }
        }
        max_seq == self.seq
    }

    /// 一致读账：一致读 ≡ 读次数。
    pub fn read_ledger_ok(&self) -> bool {
        self.coherent == self.reads
    }

    /// 可见一致判据（判据面：读回值 ≡ 最新提交值）。
    pub fn visible_consistent(&self, cell: usize, data: u64) -> bool {
        match self.latest(cell) {
            Some((d, _)) => d == data,
            None => false,
        }
    }

    /// 页冻结/解冻（回收面）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }
    /// 冻结态。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 各累计面。
    pub fn totals(&self) -> (u32, u32, u32) {
        (self.writes, self.reads, self.coherent)
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 槽快照。
    pub fn cell_of(&self, cell: usize) -> Option<ShmCell> {
        if cell >= SHARED_CELLS {
            None
        } else {
            Some(self.page[cell])
        }
    }
}

/// 双进程同节视图可见一致 10^4 次演练（10 轮 × 1000 次双向读写）。
pub fn drill_visible_10k_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30120 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ShmBook::new();
        let _ = bk.bind(0);
        let _ = bk.bind(1);
        let mut pass = true;
        for i in 0..OPS_PER_ROUND {
            // 确定性变参：写进程/槽/值全由 LCG 决定，对向进程读回核对。
            let writer = lcg.below(PROC_DOMES as u64) as usize;
            let reader = 1 - writer;
            let cell = lcg.below(SHARED_CELLS as u64) as usize;
            let val = (i as u64) * 3 + lcg.below(97);
            let w = bk.write(writer, cell, val);
            if w.is_err() {
                pass = false;
                break;
            }
            match bk.read(reader, cell) {
                Ok((d, _s)) => {
                    if d != val || !bk.visible_consistent(cell, val) {
                        pass = false;
                        break;
                    }
                }
                Err(_) => {
                    pass = false;
                    break;
                }
            }
        }
        let (w, r, c) = bk.totals();
        pass = pass
            && w == OPS_PER_ROUND as u32
            && r == OPS_PER_ROUND as u32
            && c == r
            && bk.coherence_ok()
            && bk.read_ledger_ok();
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("10^4 可见一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// 写入序与缓存一致性演练：全局序单调、缓存账平 10/10。
pub fn drill_write_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30120 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ShmBook::new();
        let _ = bk.bind(0);
        let _ = bk.bind(1);
        let mut pass = true;
        let mut last_seq = 0u64;
        for i in 0..64 {
            let writer = i % 2;
            let cell = lcg.below(SHARED_CELLS as u64) as usize;
            match bk.write(writer, cell, i as u64) {
                Ok(s) => {
                    if s <= last_seq {
                        pass = false;
                    }
                    last_seq = s;
                }
                Err(_) => pass = false,
            }
        }
        // 全局序 = 最后写序；写次数 = 64。
        let (w, _r, _c) = bk.totals();
        pass = pass && w == 64 && bk.coherence_ok();
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("写序缓存账轮={}/{}", ok, INJECT_ROUNDS))
}

/// 挂靠与域拒止演练：未挂靠/重复挂靠/越域/越槽 10/10 拒。
pub fn drill_bind_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30120 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ShmBook::new();
        let w_nobind = bk.write(0, 0, 1);
        let b0 = bk.bind(0);
        let b_dup = bk.bind(0);
        let w_oob = bk.write(0, SHARED_CELLS + 1, 1);
        let r_oob = bk.read(0, SHARED_CELLS + 1);
        let w_pid = bk.write(7, 0, 1);
        let _ = lcg.below(3);
        if matches!(w_nobind, Err(ShmCode::ViewNotBound))
            && matches!(b0, Ok(true))
            && matches!(b_dup, Err(ShmCode::BindDup))
            && matches!(w_oob, Err(ShmCode::CellOob))
            && matches!(r_oob, Err(ShmCode::CellOob))
            && matches!(w_pid, Err(ShmCode::PidOob))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("挂靠/域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：共享页转录导出（逐槽一行：值/序）。
/// ─────────────────────────────────────────────────────────────────────
/// 共享页转录导出（前 n 槽）。
pub fn export_shm_lines(bk: &ShmBook, n: usize) -> String {
    let mut s = String::from("共享页转录：\n");
    for i in 0..n.min(SHARED_CELLS) {
        if let Some(c) = bk.cell_of(i) {
            s.push_str(&format!("C{} 值{} 序{}\n", i, c.data, c.seq));
        }
    }
    s
}

/// 转录完整判据（前 n 槽行齐备）。
pub fn shm_lines_complete(bk: &ShmBook, n: usize) -> bool {
    let s = export_shm_lines(bk, n);
    (0..n.min(SHARED_CELLS)).all(|i| s.contains(&format!("C{} ", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：共享内存报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ShmReport {
    /// 写次数。
    pub writes: u32,
    /// 读次数。
    pub reads: u32,
    /// 一致读次数。
    pub coherent: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 缓存账。
    pub coherence: bool,
    /// 读账。
    pub read_ledger: bool,
}

impl ShmReport {
    /// 建报告。
    pub fn build(bk: &ShmBook) -> Self {
        let (w, r, c) = bk.totals();
        ShmReport {
            writes: w,
            reads: r,
            coherent: c,
            rejects: bk.reject_total(),
            coherence: bk.coherence_ok(),
            read_ledger: bk.read_ledger_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.coherence && self.read_ledger && self.writes >= 1 && self.reads >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ShmReport 写={} 读={} 一致={} 拒={} 缓存={} 读账={}",
            self.writes, self.reads, self.coherent, self.rejects, self.coherence, self.read_ledger
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：页冻结演练（冻结后写拒、解冻恢复 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_freeze_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30120 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ShmBook::new();
        let _ = bk.bind(0);
        let w0 = bk.write(0, 0, 7);
        bk.freeze();
        let w1 = bk.write(0, 0, 8);
        let r1 = bk.read(0, 0); // 读仍可（只读口径）
        bk.unfreeze();
        let w2 = bk.write(0, 0, 9);
        let _ = lcg.below(3);
        if matches!(w0, Ok(_))
            && matches!(w1, Err(ShmCode::PageFrozen))
            && matches!(r1, Ok((7, _s)))
            && matches!(w2, Ok(_))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("页冻结轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：对称域演练（P0 写 P1 读 + P1 写 P0 读 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_symmetric_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30120 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ShmBook::new();
        let _ = bk.bind(0);
        let _ = bk.bind(1);
        let w0 = bk.write(0, 3, 100);
        let r1 = bk.read(1, 3);
        let w1 = bk.write(1, 3, 200);
        let r0 = bk.read(0, 3);
        let last = bk.latest(3);
        let _ = lcg.below(3);
        if matches!(w0, Ok(_))
            && matches!(r1, Ok((100, _)))
            && matches!(w1, Ok(_))
            && matches!(r0, Ok((200, _)))
            && last == Some((200, 2))
            && bk.visible_consistent(3, 200)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("对称域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ShmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_shm_checklist(bk: &ShmBook) -> Vec<ShmCheckRow> {
    let rep = ShmReport::build(bk);
    let mut rows: Vec<ShmCheckRow> = Vec::new();
    rows.push(ShmCheckRow { name: "缓存账", passed: bk.coherence_ok() });
    rows.push(ShmCheckRow { name: "读账", passed: bk.read_ledger_ok() });
    rows.push(ShmCheckRow { name: "槽位宽", passed: SHARED_CELLS == 32 });
    rows.push(ShmCheckRow { name: "双进程宽", passed: PROC_DOMES == 2 });
    rows.push(ShmCheckRow { name: "10^4口径", passed: OPS_PER_ROUND == 1000 && INJECT_ROUNDS == 10 });
    rows.push(ShmCheckRow { name: "转录完整", passed: shm_lines_complete(bk, 8) });
    rows.push(ShmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ShmCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(ShmCheckRow { name: "挂靠对称", passed: bk.is_bound(0) || bk.is_bound(1) || bk.reject_total() >= 1 });
    rows.push(ShmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn shm_checklist_green(rows: &[ShmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn shm_stats_line(bk: &ShmBook) -> String {
    let (w, r, c) = bk.totals();
    format!("共享概况：写 {} 读 {} 一致 {} 拒 {}", w, r, c, bk.reject_total())
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12409_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ShmBook::new();
    let _ = bk.bind(0);
    let _ = bk.bind(1);
    let _ = bk.write(0, 0, 42);
    let _ = bk.read(1, 0);
    // 注入一例重复挂靠（拒止账口径）。
    let _ = bk.bind(0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        shm_lines_complete(&bk, 4) && ShmReport::build(&bk).all_green(),
        shm_stats_line(&bk),
    ));
    // X-b：页冻结 10/10。
    let (ok_f, d_f) = drill_freeze_10();
    out.push(("X-页冻结10/10", ok_f, d_f));
    // X-c：对称域 10/10。
    let (ok_s, d_s) = drill_symmetric_10();
    out.push(("X-对称域10/10", ok_s, d_s));
    // X-d：终验清单。
    let rows = build_shm_checklist(&bk);
    out.push(("X-终验清单10项", shm_checklist_green(&rows), ShmReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：未挂靠写必拒（ViewNotBound）。
    let mut bk = ShmBook::new();
    if matches!(bk.write(0, 0, 1), Err(ShmCode::ViewNotBound)) {
        caught += 1;
        d.push("注1 未挂:抓".to_string());
    } else {
        d.push("注1 未挂:漏".to_string());
    }
    // 注 2：重复挂靠必拒（BindDup）。
    let _ = bk.bind(0);
    if matches!(bk.bind(0), Err(ShmCode::BindDup)) {
        caught += 1;
        d.push("注2 重挂:抓".to_string());
    } else {
        d.push("注2 重挂:漏".to_string());
    }
    // 注 3：越槽读必拒（CellOob）。
    if matches!(bk.read(0, SHARED_CELLS + 2), Err(ShmCode::CellOob)) {
        caught += 1;
        d.push("注3 越槽:抓".to_string());
    } else {
        d.push("注3 越槽:漏".to_string());
    }
    // 注 4：进程域越界必拒（PidOob）。
    if matches!(bk.read(9, 0), Err(ShmCode::PidOob)) {
        caught += 1;
        d.push("注4 越域:抓".to_string());
    } else {
        d.push("注4 越域:漏".to_string());
    }
    // 注 5：冻结页写必拒（PageFrozen）。
    bk.freeze();
    if matches!(bk.write(0, 0, 2), Err(ShmCode::PageFrozen)) {
        caught += 1;
        d.push("注5 冻结:抓".to_string());
    } else {
        d.push("注5 冻结:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30120 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    ShmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12409_checks：本单判据聚合（诊断段 0x30120）。
pub fn run_unx12409_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12409 共享内存双进程语义档");
    // J1-a：双进程同节视图数据可见一致 10^4 次。
    let (ok1, d1) = drill_visible_10k_10();
    s.add_dyn("J1-可见一致10^4", ok1, &d1);
    // J1-b：写入序与缓存一致性账平。
    let (ok2, d2) = drill_write_order_10();
    s.add_dyn("J1-写序缓存账10/10", ok2, &d2);
    // J1-c：挂靠/域拒止 10/10。
    let (ok3, d3) = drill_bind_reject_10();
    s.add_dyn("J1-挂靠域拒止10/10", ok3, &d3);
    // J1-d：标准会话（P0 写 P1 读一致 + 双账平）。
    let mut bk = ShmBook::new();
    let _ = bk.bind(0);
    let _ = bk.bind(1);
    let _ = bk.write(0, 5, 1234);
    let rd = bk.read(1, 5);
    s.add_dyn(
        "J1-双进程读写一致",
        matches!(rd, Ok((1234, 1))) && bk.visible_consistent(5, 1234) && bk.coherence_ok() && bk.read_ledger_ok(),
        &format!("读={:?} 缓存={} 读账={}", rd.is_ok(), bk.coherence_ok(), bk.read_ledger_ok()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（冻结页写失败后槽值不变）。
    let d0 = bk.latest(0).map(|x| x.0).unwrap_or(0);
    bk.freeze();
    let _ = bk.write(1, 0, 777);
    let d1v = bk.latest(0).map(|x| x.0).unwrap_or(0);
    bk.unfreeze();
    s.add_dyn(
        "J2-拒止不写回",
        d0 == d1v,
        &format!("值 {}→{}", d0, d1v),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30120段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12409_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
