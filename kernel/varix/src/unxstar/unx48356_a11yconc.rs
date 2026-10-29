//! unx48356_a11yconc — UNX-F48356 B18 批并发导出纪律（多读方并发零竞态）（诊断段 0xCD60）。
//!
//! 判据原文：
//!   J1 并发 ×10^3 零竞态；判据锚定 M5 母版。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 并发模型：无障碍导出面多读方（读屏器/放大镜/开关控制）并
//!      发取数——写方独占写（版本化快照整存整取），读方各自持快
//!      照（值拷贝），读写不共享可变位（结构上无竞态，不靠纪律）。
//!   2. 零竞态口径（J1 ×10^3）：10^3 步确定性交错调度（写方更新
//!      与 N 读方取快照轮转交错），每步判据——①读方快照必是某完
//!      整版本（版本号 + 内容指纹对得上，不许撕页）；②版本严格
//!      单调（读方所见版本不回退）；③写方账与版本账守恒。
//!   3. 快照完整性：快照自带（版本，指纹）双签——指纹与版本由写
//!      方同时落，读方验签：版本对得上而指纹对不上即竞态撕页，
//!      立即红。
//!   4. 读方独立：各读方进度互不牵连（慢读方持旧快照不受新写影
//!      响——读写隔离，不是 eventual 一致性话术）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性
//!      交错（无真线程，竞态判定确定可复跑），drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCD60;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SNAP_TORN: u16 = SEG_BASE + 1; // 快照撕页（版本/指纹不对）
const E_VER_REGRESS: u16 = SEG_BASE + 2;// 版本回退
const E_LEDGER_BAD: u16 = SEG_BASE + 3;// 写账与版本账不平
const E_READER_BAD: u16 = SEG_BASE + 4;// 读方进度非法
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 读方闭集（读屏器/放大镜/开关控制/高对比）。
pub const READER_N: usize = 4;
pub const RD_SCREEN: u8 = 0;
pub const RD_ZOOM: u8 = 1;
pub const RD_SWITCH: u8 = 2;
pub const RD_CONTRAST: u8 = 3;
/// 交错步数（J1 ×10^3）。
pub const STEP_N: u32 = 1000;

/// 导出载荷（无障碍树摘要，版本化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportSnap {
    pub version: u64,
    pub node_n: u32,   // 节点数
    pub focus_bit: u32,// 焦点位
    pub fp: u64,       // 内容指纹（与 version 同落）
}

/// 并发导出面：写方独占 + 读方快照隔离。
pub struct A11yConc {
    cur: ExportSnap,
    snaps: [Option<ExportSnap>; READER_N], // 各读方最近快照
    pub write_total: u32,
    pub read_total: u32,
}

impl A11yConc {
    pub fn new() -> A11yConc {
        A11yConc {
            cur: ExportSnap { version: 0, node_n: 0, focus_bit: 0, fp: 0xCD60_CD61 },
            snaps: [None; READER_N],
            write_total: 0,
            read_total: 0,
        }
    }

    /// 写方：整存整取新快照（版本 +1，指纹与新内容同落）。
    pub fn write_snap(&mut self, node_n: u32, focus_bit: u32) -> Result<u64, u16> {
        let v = self.cur.version + 1;
        let fp = Self::snap_fp(v, node_n, focus_bit);
        self.cur = ExportSnap { version: v, node_n, focus_bit, fp };
        self.write_total += 1;
        Ok(v)
    }

    /// 读方取快照：值拷贝 + 验签（版本/指纹双签对得上才算完整页）。
    pub fn read_snap(&mut self, reader: u8) -> Result<ExportSnap, u16> {
        if reader as usize >= READER_N {
            return Err(E_READER_BAD);
        }
        let s = self.cur;
        if s.fp != Self::snap_fp(s.version, s.node_n, s.focus_bit) {
            return Err(E_SNAP_TORN);
        }
        // 版本不回退：新快照版本 ≥ 该读方旧快照版本。
        if let Some(old) = self.snaps[reader as usize] {
            if s.version < old.version {
                return Err(E_VER_REGRESS);
            }
        }
        self.snaps[reader as usize] = Some(s);
        self.read_total += 1;
        Ok(s)
    }

    /// 读方最近快照版本（进度账）。
    pub fn reader_ver(&self, reader: u8) -> Result<u64, u16> {
        if reader as usize >= READER_N {
            return Err(E_READER_BAD);
        }
        Ok(self.snaps[reader as usize].map_or(0, |s| s.version))
    }

    /// 快照指纹（写方同款）。
    pub fn snap_fp(version: u64, node_n: u32, focus_bit: u32) -> u64 {
        let mut fp: u64 = 0xCD60_CD62 ^ version;
        fp = fp.rotate_left(9) ^ (node_n as u64);
        fp = fp.rotate_left(9) ^ (focus_bit as u64);
        fp.rotate_left(9)
    }

    /// 守恒审计：版本 == 写次数；读账 ≤ 步账（每读恰一账）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.cur.version != self.write_total as u64 {
            return Err(E_LEDGER_BAD);
        }
        if self.read_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.write_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_READER_BAD);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// 交错调度一轮（J1 主体）：10^3 步，写方更新与读方取快照交错，
/// 每步验三判据（完整页/不回退/账守恒）。返回终态版本。
pub fn interleave(seed: u64, steps: u32) -> Result<u64, u16> {
    let mut lcg = Lcg::new(seed)?;
    let mut ac = A11yConc::new();
    let mut last_seen = [0u64; READER_N];
    let mut s = 0u32;
    while s < steps {
        let act = lcg.next_below(READER_N as u64 + 1);
        if act == 0 {
            // 写步：LCG 驱动新内容。
            let node_n = (lcg.next_below(64) + 1) as u32;
            let focus_bit = lcg.next_below(64) as u32;
            ac.write_snap(node_n, focus_bit)?;
        } else {
            // 读步：读方 act-1 取快照并验签。
            let rd = (act - 1) as u8;
            let snap = ac.read_snap(rd)?;
            if snap.fp != A11yConc::snap_fp(snap.version, snap.node_n, snap.focus_bit) {
                return Err(E_SNAP_TORN);
            }
            if snap.version < last_seen[rd as usize] {
                return Err(E_VER_REGRESS);
            }
            last_seen[rd as usize] = snap.version;
        }
        s += 1;
    }
    ac.audit()?;
    Ok(ac.cur.version)
}

/// 自例程判据：逐条落实 J1（并发 ×10^3 零竞态），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 交错 ×10^3 零竞态：写/读交错 10^3 步，撕页/回退/账烂三红
    //    全程未触，终态版本 == 写次数。
    let v = interleave(0xCD61, STEP_N)?;
    if v == 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 读写隔离：读方持旧快照后写方连写，旧快照逐位不变（慢读
    //    方不被新写波及）；再读版本单调不回退。
    let mut ac = A11yConc::new();
    ac.write_snap(10, 3)?;
    let old = ac.read_snap(RD_SCREEN)?;
    ac.write_snap(20, 7)?;
    ac.write_snap(30, 11)?;
    if old.version != 1 || old.node_n != 10 {
        return Err(E_SNAP_TORN);
    }
    let fresh = ac.read_snap(RD_SCREEN)?;
    if fresh.version != 3 || fresh.version < old.version {
        return Err(E_VER_REGRESS);
    }
    if ac.reader_ver(RD_SCREEN)? != 3 {
        return Err(E_READER_BAD);
    }
    hits += 1;

    // R3 多读方独立：四读方各自进度分立（读屏器读了 3 版不代表放
    //    大镜读过）——各 reader_ver 与其读取史一致。
    let mut ac = A11yConc::new();
    ac.write_snap(5, 1)?;
    ac.read_snap(RD_SCREEN)?;
    ac.write_snap(6, 2)?;
    ac.read_snap(RD_ZOOM)?;
    ac.write_snap(7, 3)?;
    if ac.reader_ver(RD_SCREEN)? != 1 || ac.reader_ver(RD_ZOOM)? != 2 {
        return Err(E_READER_BAD);
    }
    if ac.reader_ver(RD_SWITCH)? != 0 || ac.reader_ver(RD_CONTRAST)? != 0 {
        return Err(E_READER_BAD); // 未读者进度为零
    }
    hits += 1;

    // R4 撕页可红：伪造快照指纹与版本不符 → read_snap 必红（判据
    //    非恒绿——竞态可检出）。
    let mut ac = A11yConc::new();
    ac.write_snap(8, 4)?;
    ac.cur.fp ^= 0xFF; // 私改指纹（模拟撕页）
    if ac.read_snap(RD_ZOOM) != Err(E_SNAP_TORN) {
        return Err(E_SNAP_TORN);
    }
    hits += 1;

    // R5 读方越闭集：read_snap/reader_ver 越界一致拒；拒绝不落账。
    let mut ac = A11yConc::new();
    if ac.read_snap(4) != Err(E_READER_BAD) {
        return Err(E_READER_BAD);
    }
    if ac.reader_ver(9) != Err(E_READER_BAD) {
        return Err(E_READER_BAD);
    }
    if ac.read_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 版本账守恒可红：伪写账（write_total 与版本失衡）audit 必
    //    红；零读账 audit 必红。
    let mut ac = A11yConc::new();
    ac.write_snap(3, 1)?;
    ac.read_snap(RD_SCREEN)?;
    ac.write_total = 5; // 伪造写账
    if ac.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    let ac0 = A11yConc::new();
    if ac0.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 写方独占口径：无写时连读不升版本（读不改状态——并发面
    //    只有一个写入口）。
    let mut ac = A11yConc::new();
    ac.write_snap(9, 2)?;
    let v0 = ac.cur.version;
    let mut n = 0u32;
    while n < 10 {
        ac.read_snap(RD_CONTRAST)?;
        n += 1;
    }
    if ac.cur.version != v0 || ac.write_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    if ac.read_total != 10 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：同种子第二轮 ×10^3 交错终态
    //    版本与首轮逐位一致（确定性）；写回过。
    let v1 = interleave(0xCD61, STEP_N)?;
    let v2 = interleave(0xCD61, STEP_N)?;
    if v1 != v2 {
        return Err(E_REPLAY_DIFF);
    }
    let mut ac = A11yConc::new();
    ac.write_snap(1, 1)?;
    ac.read_snap(RD_SCREEN)?;
    if ac.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ac.writeback(true, 0x1E)?;
    hits += 1;

    Ok(hits)
}
