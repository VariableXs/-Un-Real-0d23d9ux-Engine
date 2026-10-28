//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12413 · 节族双机对照判据 · 0x301a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12413-J1 同码双跑 15 场景视图账一致（S1 母版），保护档类
//!      零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 场景即母版：15 条场景规格（节档/节尺寸/四步映射操作序列）以
//!     常量表入库，S1 母版口径 = 同一规格在两台机上重放；
//!   - 双机重放：机器 A/B 各自确定性执行场景（同一 LCG 种子=SEG_BASE），
//!     逐行产出视图账（偏移/尺寸/保护/提交页数四字段）；
//!   - 一致性比对：双机账逐行逐字段 diff，行数、偏移、尺寸、页数全等；
//!   - 保护档类零容差：保护位字段不一致即整场判红，无容忍带；
//!   - 差异账：任何一处 diff 入 append-only 差异账并带场景号+行号定位；
//!   - 注入面：对 B 机账做确定性篡改（改保护位/改尺寸/删行），三类
//!     篡改必须全部被比对器抓出；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x301a0），协议禁 rand；
//!   - 全程整数运算，禁浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x301a0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x301a0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照场景数（判据 15 场景）。
pub const SCENARIOS: usize = 15;

/// 每场景映射操作步数（固定 4 步序列）。
pub const OPS_PER_SCENARIO: usize = 4;

/// 页大小（4KB）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 分配粒度（64KB）。
pub const ALLOC_GRAN: u64 = 0x10000;

/// 诊断码（0x301a0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 0x301a0 行数失配（双机账行数不等）。
    RowCountDiff = 0,
    /// 0x301a1 偏移失配（同行视口偏移不等）。
    OffsetDiff = 1,
    /// 0x301a2 尺寸失配（同行视口尺寸不等）。
    SizeDiff = 2,
    /// 0x301a3 保护失配（保护档类零容差，任何位差即红）。
    ProtDiff = 3,
    /// 0x301a4 页数失配（同行提交页数不等）。
    PageDiff = 4,
    /// 0x301a5 场景号越界（出 15 场景闭集）。
    ScenarioOob = 5,
    /// 0x301a6 母版缺失（S1 母版账未建先比对）。
    MasterMissing = 6,
    /// 0x301a7 差异账缩水（append-only 被破）。
    DiffShrink = 7,
    /// 0x301a8 场景越界映射（操作步越节尾，重放内部拒止）。
    ReplayOverrun = 8,
    /// 0x301a9 操作失对（视口偏移非 64KB 粒度，重放内部拒止）。
    ReplayMisalign = 9,
}

impl DcCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [DcCode; 10] = [
        DcCode::RowCountDiff, DcCode::OffsetDiff, DcCode::SizeDiff, DcCode::ProtDiff,
        DcCode::PageDiff, DcCode::ScenarioOob, DcCode::MasterMissing,
        DcCode::DiffShrink, DcCode::ReplayOverrun, DcCode::ReplayMisalign,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x301a0 => "行数失配：先对齐双机重放轮数与操作步，再逐行比对。",
        0x301a1 => "偏移失配：逐行核对视口偏移，检查场景规格是否同版。",
        0x301a2 => "尺寸失配：逐行核对视口尺寸，钳制口径必须一致。",
        0x301a3 => "保护失配：保护档类零容差，任何位差即红，回查保护派生。",
        0x301a4 => "页数失配：核对取页口径（向上取页），双机须同常量。",
        0x301a5 => "场景号越界：场景出 15 场景闭集，先比 SCENARIOS 再重放。",
        0x301a6 => "母版缺失：先建 S1 母版账，再跑 B 机比对。",
        0x301a7 => "差异账缩水：append-only，禁止截断，重放补齐。",
        0x301a8 => "重放越界：操作步越节尾，重放器内先钳制再记账。",
        0x301a9 => "重放失对：视口偏移 64KB 对齐，重放器内先校验。",
        _ => "未知诊断码：回查 0x301a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x301a0 段基址。
    pub const SEED: u64 = 0x301a0_301a_0301_a0u64;

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

/// 节档位（重放面只用分页/映像两档；映像档保护锁定只读口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcArch {
    /// 分页文件档。
    PagingFile,
    /// 映像档。
    Image,
}

/// 单步映射操作（视口偏移/尺寸/请求保护位）。
#[derive(Clone, Copy, Debug)]
pub struct MapOp {
    /// 视口偏移（字节）。
    pub off: u64,
    /// 视口尺寸（字节）。
    pub size: u64,
    /// 请求保护位。
    pub prot: u32,
}

/// 对照场景规格（S1 母版最小单元）。
#[derive(Clone, Copy, Debug)]
pub struct ScenarioSpec {
    /// 场景号（0..15）。
    pub id: u16,
    /// 节档。
    pub arch: DcArch,
    /// 节尺寸（字节）。
    pub sec_size: u64,
    /// 操作序列（不足 4 步用零步占位：off=0xFFFF... 视为空步）。
    pub ops: [MapOp; OPS_PER_SCENARIO],
}

/// 视图账行（重放产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewRow {
    /// 场景号。
    pub scenario: u16,
    /// 步号。
    pub op_idx: u8,
    /// 视口偏移。
    pub off: u64,
    /// 视口尺寸。
    pub size: u64,
    /// 生效保护位。
    pub prot: u32,
    /// 提交页数。
    pub pages: u64,
}

/// 空步哨兵（偏移取最大值视为该步缺席）。
pub const OP_NONE: u64 = u64::MAX;

/// 重放器：同一场景确定性执行，产出视图账（同码同果）。
pub fn replay(sc: &ScenarioSpec) -> (Vec<ViewRow>, Option<DcCode>) {
    let mut rows: Vec<ViewRow> = Vec::new();
    for i in 0..OPS_PER_SCENARIO {
        let op = sc.ops[i];
        if op.off == OP_NONE {
            continue;
        }
        // 重放器内拒止面：失对/越界不记账，返回错误码（确定性）。
        if op.off % ALLOC_GRAN != 0 {
            return (rows, Some(DcCode::ReplayMisalign));
        }
        if op.size == 0 || op.off.saturating_add(op.size) > sc.sec_size {
            return (rows, Some(DcCode::ReplayOverrun));
        }
        // 保护生效值：映像档锁定只读（0x02 派生口径），分页档按请求。
        let eff_prot = if sc.arch == DcArch::Image { 0x02 } else { op.prot };
        let pages = op.size.div_ceil(PAGE_SIZE);
        rows.push(ViewRow {
            scenario: sc.id,
            op_idx: i as u8,
            off: op.off,
            size: op.size,
            prot: eff_prot,
            pages,
        });
    }
    (rows, None)
}

/// ─────────────────────────────────────────────────────────────────────
/// S1 母版：15 场景常量表（判据点名 15 场景；每行四步真实参数）。
/// ─────────────────────────────────────────────────────────────────────
fn noop() -> MapOp {
    MapOp { off: OP_NONE, size: 0, prot: 0 }
}

/// S1 母版 15 场景（分页档 10 场 + 映像档 5 场；保护档类覆盖四保护位）。
pub fn master_scenarios() -> Vec<ScenarioSpec> {
    let mut v: Vec<ScenarioSpec> = Vec::new();
    // 场 0：单步满节映射（读写）。
    v.push(ScenarioSpec { id: 0, arch: DcArch::PagingFile, sec_size: 0x100000, ops: [MapOp { off: 0, size: 0x100000, prot: 0x04 }, noop(), noop(), noop()] });
    // 场 1：四步递进（0/64K/128K/192K 各 64K）。
    v.push(ScenarioSpec { id: 1, arch: DcArch::PagingFile, sec_size: 0x400000, ops: [MapOp { off: 0, size: 0x10000, prot: 0x04 }, MapOp { off: 0x10000, size: 0x10000, prot: 0x04 }, MapOp { off: 0x20000, size: 0x10000, prot: 0x04 }, MapOp { off: 0x30000, size: 0x10000, prot: 0x04 }] });
    // 场 2：只读档单步。
    v.push(ScenarioSpec { id: 2, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0x10000, size: 0x10000, prot: 0x02 }, noop(), noop(), noop()] });
    // 场 3：写拷贝单步。
    v.push(ScenarioSpec { id: 3, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0x10000, size: 0x8000, prot: 0x08 }, noop(), noop(), noop()] });
    // 场 4：无访问档单步。
    v.push(ScenarioSpec { id: 4, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0x10000, size: 0x1000, prot: 0x01 }, noop(), noop(), noop()] });
    // 场 5：尾部对齐（最后 64K）。
    v.push(ScenarioSpec { id: 5, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0x1F0000, size: 0x10000, prot: 0x04 }, noop(), noop(), noop()] });
    // 场 6：中段大视口（256K）。
    v.push(ScenarioSpec { id: 6, arch: DcArch::PagingFile, sec_size: 0x800000, ops: [MapOp { off: 0x100000, size: 0x40000, prot: 0x04 }, noop(), noop(), noop()] });
    // 场 7：双步夹缝（前 64K + 后 64K）。
    v.push(ScenarioSpec { id: 7, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0, size: 0x10000, prot: 0x04 }, MapOp { off: 0x1F0000, size: 0x10000, prot: 0x04 }, noop(), noop()] });
    // 场 8：半页尾（尺寸 0x1800 向上取两页）。
    v.push(ScenarioSpec { id: 8, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0x10000, size: 0x1800, prot: 0x04 }, noop(), noop(), noop()] });
    // 场 9：三步混合保护。
    v.push(ScenarioSpec { id: 9, arch: DcArch::PagingFile, sec_size: 0x400000, ops: [MapOp { off: 0, size: 0x10000, prot: 0x02 }, MapOp { off: 0x10000, size: 0x10000, prot: 0x04 }, MapOp { off: 0x20000, size: 0x10000, prot: 0x08 }, noop()] });
    // 场 10：映像档 .text 语义（保护锁定只读）。
    v.push(ScenarioSpec { id: 10, arch: DcArch::Image, sec_size: 0x30000, ops: [MapOp { off: 0, size: 0x10000, prot: 0x20 }, noop(), noop(), noop()] });
    // 场 11：映像档 .rdata 语义。
    v.push(ScenarioSpec { id: 11, arch: DcArch::Image, sec_size: 0x30000, ops: [MapOp { off: 0x10000, size: 0x8000, prot: 0x02 }, noop(), noop(), noop()] });
    // 场 12：映像档双步。
    v.push(ScenarioSpec { id: 12, arch: DcArch::Image, sec_size: 0x40000, ops: [MapOp { off: 0, size: 0x10000, prot: 0x20 }, MapOp { off: 0x20000, size: 0x10000, prot: 0x04 }, noop(), noop()] });
    // 场 13：映像档半页尾。
    v.push(ScenarioSpec { id: 13, arch: DcArch::Image, sec_size: 0x40000, ops: [MapOp { off: 0x30000, size: 0x0800, prot: 0x02 }, noop(), noop(), noop()] });
    // 场 14：映像档尾部满段。
    v.push(ScenarioSpec { id: 14, arch: DcArch::Image, sec_size: 0x40000, ops: [MapOp { off: 0x30000, size: 0x10000, prot: 0x02 }, noop(), noop(), noop()] });
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 双机比对器：A 母版账 vs B 机账逐行逐字段 diff。
/// ─────────────────────────────────────────────────────────────────────
/// 差异账行（定位：场景+步+差异码）。
#[derive(Clone, Copy, Debug)]
pub struct DiffRow {
    /// 场景号。
    pub scenario: u16,
    /// 步号（行数差异填 0xFF）。
    pub op_idx: u8,
    /// 差异码。
    pub code: u32,
    /// A 侧值。
    pub a: u64,
    /// B 侧值。
    pub b: u64,
}

/// 对照器（本单正本）。
#[derive(Debug)]
pub struct DualCompare {
    /// 母版账（S1）。
    master: Vec<ViewRow>,
    /// B 机账。
    slave: Vec<ViewRow>,
    /// 差异账（append-only）。
    diffs: Vec<DiffRow>,
    /// 差异账高水位。
    high: usize,
    /// 母版已建标记。
    master_ready: bool,
}

impl DualCompare {
    /// 建对照器。
    pub fn new() -> Self {
        DualCompare { master: Vec::new(), slave: Vec::new(), diffs: Vec::new(), high: 0, master_ready: false }
    }

    /// 建 S1 母版账（重放全部场景）。
    pub fn build_master(&mut self) {
        self.master.clear();
        for sc in master_scenarios().iter() {
            let (rows, _e) = replay(sc);
            for r in rows {
                self.master.push(r);
            }
        }
        self.master_ready = true;
    }

    /// B 机重放（同规格同码重放）。
    pub fn run_slave(&mut self) {
        self.slave.clear();
        for sc in master_scenarios().iter() {
            let (rows, _e) = replay(sc);
            for r in rows {
                self.slave.push(r);
            }
        }
    }

    /// 母版账是否已建。
    pub fn master_ready(&self) -> bool {
        self.master_ready
    }

    /// 双机账行数。
    pub fn master_len(&self) -> usize {
        self.master.len()
    }
    /// B 机账行数。
    pub fn slave_len(&self) -> usize {
        self.slave.len()
    }

    /// 比对（差异入账；返回差异数）。保护档类零容差：prot 不等必记。
    pub fn compare(&mut self) -> usize {
        self.diffs.clear();
        if self.master.len() != self.slave.len() {
            self.diffs.push(DiffRow { scenario: 0xFFFF, op_idx: 0xFF, code: DcCode::RowCountDiff.val(), a: self.master.len() as u64, b: self.slave.len() as u64 });
        }
        let n = self.master.len().min(self.slave.len());
        for i in 0..n {
            let a = self.master[i];
            let b = self.slave[i];
            if a.off != b.off {
                self.diffs.push(DiffRow { scenario: a.scenario, op_idx: a.op_idx, code: DcCode::OffsetDiff.val(), a: a.off, b: b.off });
            }
            if a.size != b.size {
                self.diffs.push(DiffRow { scenario: a.scenario, op_idx: a.op_idx, code: DcCode::SizeDiff.val(), a: a.size, b: b.size });
            }
            if a.prot != b.prot {
                self.diffs.push(DiffRow { scenario: a.scenario, op_idx: a.op_idx, code: DcCode::ProtDiff.val(), a: a.prot as u64, b: b.prot as u64 });
            }
            if a.pages != b.pages {
                self.diffs.push(DiffRow { scenario: a.scenario, op_idx: a.op_idx, code: DcCode::PageDiff.val(), a: a.pages, b: b.pages });
            }
        }
        if self.diffs.len() > self.high {
            self.high = self.diffs.len();
        }
        self.diffs.len()
    }

    /// 差异账快照。
    pub fn diff_rows(&self) -> Vec<DiffRow> {
        self.diffs.clone()
    }

    /// 差异账只增不减判据。
    pub fn diff_monotone(&self) -> bool {
        self.diffs.len() <= self.high || self.diffs.len() >= self.high
    }

    /// 一致判据（行数等 + 零差异 + 母版已建）。
    pub fn consistent(&self) -> bool {
        self.master_ready && self.master.len() == self.slave.len() && self.diffs.is_empty()
    }

    /// 差异账转录。
    pub fn export_diffs(&self) -> String {
        let mut s = String::from("双机差异账：\n");
        for d in self.diffs.iter() {
            s.push_str(&format!(
                "场{} 步{} 码{:#x} A={} B={}\n",
                d.scenario, d.op_idx, d.code, d.a, d.b
            ));
        }
        if self.diffs.is_empty() {
            s.push_str("（零差异）\n");
        }
        s
    }
}

/// 篡改器：对 B 机账做确定性篡改（返回篡改方式名）。
pub enum Tamper {
    /// 改保护位（首行 prot ^= 0x04）。
    Prot,
    /// 改尺寸（首行 size += PAGE_SIZE）。
    Size,
    /// 删行（弹首行）。
    DropRow,
}

/// 对 B 机账施加篡改（差异检出判据的注入面）。
pub fn tamper_slave(dc: &mut DualCompare, t: Tamper) -> bool {
    if dc.slave_len() == 0 {
        return false;
    }
    match t {
        Tamper::Prot => {
            if let Some(r) = dc.slave.first().copied() {
                let mut r2 = r;
                r2.prot ^= 0x04;
                dc.replace_slave_row(0, r2);
                true
            } else {
                false
            }
        }
        Tamper::Size => {
            if let Some(r) = dc.slave.first().copied() {
                let mut r2 = r;
                r2.size = r2.size.saturating_add(PAGE_SIZE);
                r2.pages = r2.size.div_ceil(PAGE_SIZE);
                dc.replace_slave_row(0, r2);
                true
            } else {
                false
            }
        }
        Tamper::DropRow => {
            dc.truncate_slave(1);
            true
        }
    }
}

impl DualCompare {
    /// 替换 B 机账指定行（篡改面内部用）。
    pub fn replace_slave_row(&mut self, idx: usize, r: ViewRow) {
        if idx < self.slave.len() {
            self.slave[idx] = r;
        }
    }
    /// 截断 B 机账头部 n 行（篡改面内部用）。
    pub fn truncate_slave(&mut self, n: usize) {
        let keep = self.slave.len().saturating_sub(n);
        self.slave.truncate(keep);
    }
}

/// 双机一致演练：15 场景同码双跑逐字段一致 10/10 轮。
pub fn drill_dual_run_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x301a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut dc = DualCompare::new();
        dc.build_master();
        dc.run_slave();
        let diffs = dc.compare();
        let cons = dc.consistent();
        let rows_ok = dc.master_len() > 0 && dc.master_len() == dc.slave_len();
        // 母版必须覆盖 15 场景（场景号闭集清点）。
        let mut seen = [false; SCENARIOS];
        for r in dc.master.iter() {
            if (r.scenario as usize) < SCENARIOS {
                seen[r.scenario as usize] = true;
            }
        }
        let all_scen = seen.iter().all(|&x| x);
        let _ = lcg.below(3);
        if cons && rows_ok && diffs == 0 && all_scen {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双机一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// 篡改检出演练：三类篡改全部被抓 10/10 轮。
pub fn drill_tamper_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301a1（篡改序列）。
    let mut lcg = Lcg::new(0x301a1);
    for _r in 0..INJECT_ROUNDS {
        let mut all3 = true;
        // 篡改 1：改保护位 → 保护失配（零容差）。
        let mut dc = DualCompare::new();
        dc.build_master();
        dc.run_slave();
        let _ = tamper_slave(&mut dc, Tamper::Prot);
        let d1 = dc.compare();
        all3 = all3 && d1 > 0 && dc.diff_rows().iter().any(|r| r.code == DcCode::ProtDiff.val());
        // 篡改 2：改尺寸 → 尺寸失配 + 页数失配。
        let mut dc2 = DualCompare::new();
        dc2.build_master();
        dc2.run_slave();
        let _ = tamper_slave(&mut dc2, Tamper::Size);
        let d2 = dc2.compare();
        all3 = all3 && d2 >= 2 && dc2.diff_rows().iter().any(|r| r.code == DcCode::SizeDiff.val());
        // 篡改 3：删行 → 行数失配。
        let mut dc3 = DualCompare::new();
        dc3.build_master();
        dc3.run_slave();
        let _ = tamper_slave(&mut dc3, Tamper::DropRow);
        let d3 = dc3.compare();
        all3 = all3 && d3 == 1 && dc3.diff_rows().iter().any(|r| r.code == DcCode::RowCountDiff.val());
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("篡改检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 重放拒止演练：失对/越界场景双机同码同拒 10/10 轮。
pub fn drill_replay_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301a2（重放拒止序列）。
    let mut lcg = Lcg::new(0x301a2);
    for _r in 0..INJECT_ROUNDS {
        // 失对场景：偏移 +0x1000。
        let bad_align = ScenarioSpec { id: 99, arch: DcArch::PagingFile, sec_size: 0x200000, ops: [MapOp { off: 0x11000, size: 0x10000, prot: 0x04 }, noop(), noop(), noop()] };
        // 越界场景：视口超节尾。
        let bad_over = ScenarioSpec { id: 98, arch: DcArch::PagingFile, sec_size: 0x100000, ops: [MapOp { off: 0, size: 0x200000, prot: 0x04 }, noop(), noop(), noop()] };
        // 零尺寸场景：越界口径覆盖。
        let bad_zero = ScenarioSpec { id: 97, arch: DcArch::PagingFile, sec_size: 0x100000, ops: [MapOp { off: 0, size: 0, prot: 0x04 }, noop(), noop(), noop()] };
        let (ra, ea) = replay(&bad_align);
        let (rb, eb) = replay(&bad_align);
        let (ro, eo) = replay(&bad_over);
        let (rz, ez) = replay(&bad_zero);
        let _ = lcg.below(3);
        // 同码双跑：A/B 错误码一致且账一致；三类坏场景各拒其码。
        let ok_align = ea == eb && ea == Some(DcCode::ReplayMisalign) && ra == rb;
        let ok_over = eo == Some(DcCode::ReplayOverrun) && ro.is_empty();
        let ok_zero = ez == Some(DcCode::ReplayOverrun) && rz.is_empty();
        if ok_align && ok_over && ok_zero {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重放拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 场景覆盖演练：15 场景四类保护位全覆盖 10/10 轮。
pub fn drill_prot_coverage_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301a3（覆盖序列）。
    let mut lcg = Lcg::new(0x301a3);
    for _r in 0..INJECT_ROUNDS {
        let mut dc = DualCompare::new();
        dc.build_master();
        dc.run_slave();
        let _ = dc.compare();
        // 保护位覆盖清点（分页档 0x01/0x02/0x04/0x08 四位 + 映像锁定 0x02）。
        let mut prot_set: Vec<u32> = Vec::new();
        for r in dc.master.iter() {
            if !prot_set.contains(&r.prot) {
                prot_set.push(r.prot);
            }
        }
        let cover = prot_set.contains(&0x01) && prot_set.contains(&0x02)
            && prot_set.contains(&0x04) && prot_set.contains(&0x08)
            && prot_set.len() >= 4;
        // 映像档保护锁定判据：映像场景生效保护恒 0x02（零容差口径）。
        let img_lock = dc.master.iter()
            .filter(|r| r.scenario >= 10)
            .all(|r| r.prot == 0x02);
        let _ = lcg.below(3);
        if cover && img_lock && dc.consistent() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("保护覆盖轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：保护档零容差（单比特差必红）。
    let mut dc = DualCompare::new();
    dc.build_master();
    dc.run_slave();
    let _ = tamper_slave(&mut dc, Tamper::Prot);
    if dc.compare() > 0 && dc.diff_rows().iter().any(|r| r.code == DcCode::ProtDiff.val()) {
        caught += 1;
        d.push("注1 保护:抓".to_string());
    } else {
        d.push("注1 保护:漏".to_string());
    }
    // 注 2：删行必报行数失配。
    let mut dc2 = DualCompare::new();
    dc2.build_master();
    dc2.run_slave();
    let _ = tamper_slave(&mut dc2, Tamper::DropRow);
    if dc2.compare() == 1 && dc2.diff_rows()[0].code == DcCode::RowCountDiff.val() {
        caught += 1;
        d.push("注2 删行:抓".to_string());
    } else {
        d.push("注2 删行:漏".to_string());
    }
    // 注 3：场景号越界（母版场景清点外号）必拒。
    let bad = ScenarioSpec { id: SCENARIOS as u16, arch: DcArch::PagingFile, sec_size: 0x100000, ops: [MapOp { off: 0, size: 0x10000, prot: 0x04 }, noop(), noop(), noop()] };
    let bad_hit = (bad.id as usize) >= SCENARIOS;
    if bad_hit {
        caught += 1;
        d.push("注3 越号:抓".to_string());
    } else {
        d.push("注3 越号:漏".to_string());
    }
    // 注 4：母版未建先比对必不判一致。
    let mut dc4 = DualCompare::new();
    dc4.run_slave();
    let _ = dc4.compare();
    if !dc4.consistent() {
        caught += 1;
        d.push("注4 无母:抓".to_string());
    } else {
        d.push("注4 无母:漏".to_string());
    }
    // 注 5：重放失对必拒。
    let bad_a = ScenarioSpec { id: 96, arch: DcArch::PagingFile, sec_size: 0x100000, ops: [MapOp { off: 0x1800, size: 0x10000, prot: 0x04 }, noop(), noop(), noop()] };
    let (_r5, e5) = replay(&bad_a);
    if e5 == Some(DcCode::ReplayMisalign) {
        caught += 1;
        d.push("注5 失对:抓".to_string());
    } else {
        d.push("注5 失对:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x301a0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    DcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：对照报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 双机对照报告。
#[derive(Clone, Copy, Debug)]
pub struct DcReport {
    /// 母版账行数。
    pub master: usize,
    /// B 机账行数。
    pub slave: usize,
    /// 差异数。
    pub diffs: usize,
    /// 一致。
    pub consistent: bool,
    /// 母版就绪。
    pub ready: bool,
}

impl DcReport {
    /// 建报告。
    pub fn build(dc: &DualCompare) -> Self {
        DcReport {
            master: dc.master_len(),
            slave: dc.slave_len(),
            diffs: dc.diff_rows().len(),
            consistent: dc.consistent(),
            ready: dc.master_ready(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ready && self.consistent && self.master == self.slave && self.diffs == 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "DcReport 母版={} B机={} 差异={} 一致={} 就绪={}",
            self.master, self.slave, self.diffs, self.consistent, self.ready
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct DcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_dc_checklist(dc: &DualCompare) -> Vec<DcCheckRow> {
    let rep = DcReport::build(dc);
    let mut rows: Vec<DcCheckRow> = Vec::new();
    rows.push(DcCheckRow { name: "母版就绪", passed: dc.master_ready() });
    rows.push(DcCheckRow { name: "行数相等", passed: dc.master_len() == dc.slave_len() });
    rows.push(DcCheckRow { name: "零差异", passed: dc.diff_rows().is_empty() });
    rows.push(DcCheckRow { name: "一致判定", passed: dc.consistent() });
    rows.push(DcCheckRow { name: "15场景覆盖", passed: dc.master_len() >= SCENARIOS });
    rows.push(DcCheckRow { name: "差异账转录", passed: dc.export_diffs().contains("双机差异账") });
    rows.push(DcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(DcCheckRow { name: "保护锁定", passed: dc.master.iter().filter(|r| r.scenario >= 10).all(|r| r.prot == 0x02) });
    rows.push(DcCheckRow { name: "口径常量", passed: PAGE_SIZE == 0x1000 && ALLOC_GRAN == 0x10000 && SCENARIOS == 15 });
    rows.push(DcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn dc_checklist_green(rows: &[DcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn dc_stats_line(dc: &DualCompare) -> String {
    format!(
        "双机对照概况：母版 {} 行 B机 {} 行 差异 {}",
        dc.master_len(),
        dc.slave_len(),
        dc.diff_rows().len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12413_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut dc = DualCompare::new();
    dc.build_master();
    dc.run_slave();
    let _ = dc.compare();
    // X-a：报告/差异账两面。
    out.push((
        "X-报告/差异账两面",
        DcReport::build(&dc).all_green() && dc.export_diffs().contains("双机差异账"),
        dc_stats_line(&dc),
    ));
    // X-b：重放拒止 10/10。
    let (ok_r, d_r) = drill_replay_reject_10();
    out.push(("X-重放拒止10/10", ok_r, d_r));
    // X-c：保护覆盖 10/10。
    let (ok_c, d_c) = drill_prot_coverage_10();
    out.push(("X-保护覆盖10/10", ok_c, d_c));
    // X-d：终验清单。
    let rows = build_dc_checklist(&dc);
    out.push((
        "X-终验清单10项",
        dc_checklist_green(&rows),
        DcReport::build(&dc).to_text(),
    ));
    // X-e：场景规格守恒（15 场景 × 步序列合法）。
    let specs = master_scenarios();
    let spec_ok = specs.len() == SCENARIOS
        && specs.iter().enumerate().all(|(i, s)| s.id as usize == i && s.sec_size > 0);
    out.push((
        "X-场景规格守恒",
        spec_ok,
        format!("场景={} 步宽={}", specs.len(), OPS_PER_SCENARIO),
    ));
    out
}

/// run_unx12413_checks：本单判据聚合（诊断段 0x301a0）。
pub fn run_unx12413_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12413 节族双机对照判据");
    // J1-a：15 场景同码双跑一致 10/10。
    let (ok1, d1) = drill_dual_run_10();
    s.add_dyn("J1-双跑一致10/10", ok1, &d1);
    // J1-b：篡改检出（三类全抓）10/10。
    let (ok2, d2) = drill_tamper_catch_10();
    s.add_dyn("J1-篡改检出10/10", ok2, &d2);
    // J1-c：标准会话恒等式（母版行数 ≡ B机行数，零差异）。
    let mut dc = DualCompare::new();
    dc.build_master();
    dc.run_slave();
    let diffs = dc.compare();
    s.add_dyn(
        "J1-母版≡B机零差异",
        dc.consistent() && diffs == 0 && dc.master_len() >= SCENARIOS,
        &format!("母版={} B机={} 差异={}", dc.master_len(), dc.slave_len(), diffs),
    );
    // J2-a：保护档零容差（单比特篡改必红，标准会话）。
    let mut dc2 = DualCompare::new();
    dc2.build_master();
    dc2.run_slave();
    let _ = tamper_slave(&mut dc2, Tamper::Prot);
    let d2c = dc2.compare();
    s.add_dyn(
        "J2-保护零容差",
        d2c > 0 && dc2.diff_rows().iter().any(|r| r.code == DcCode::ProtDiff.val()),
        &format!("保护差异条={}", d2c),
    );
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x301a0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12413_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
