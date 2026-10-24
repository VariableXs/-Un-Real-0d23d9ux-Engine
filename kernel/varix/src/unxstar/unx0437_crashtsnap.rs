//! unx0437_crashtsnap —— UNX-F0437 异常现场快照账（崩溃帧记录）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0437 · 异常现场快照账 · 目标 240 行
//! 判据原文：UNX-F0437-J1 崩溃帧快照 = 致命异常发生时抓拍九
//!           元组（向量/错误码/RIP/CS/RFLAGS/RSP/SS/CR2/时间
//!           戳），环形存档（容量 16，满覆最旧）；快照完整性
//!           核：九元全字段非哨兵（0xFFFF… 哨兵 = 未抓到）；
//!           快照序列核：时间戳单调不减（同戳允许，回退立
//!           案）；CR2 语义核：仅 #PF 快照 CR2 有效，非 #PF
//!           快照 CR2 必须哨兵（乱填 CR2 立案）；导出重放：
//!           快照柜逐份导出可重建（同序同柜）。
//! 组件：九元快照结构 + 16 环形柜 + 哨兵完整性核 + 时间戳
//!       单调账 + CR2 语义闸 + 导出重放核。
//! 职责定位：崩溃快照是「黑匣子」——致命异常后内核只能死一
//!   次，黑匣子没抓全就永远不知道怎么死的；CR2 乱填是最典
//!   型的「假线索」（非 #PF 也填 CR2 误导调查方向）；环形覆
//!   盖保证黑匣子永远记录最近 16 次而不是第一次。
//! 性能逐项分解：抓拍 O(1)；完整性 O(1)；单调核 O(柜深)；
//!   重放 O(柜深)。
//! 诊断码段：0x304C0（0x304C1~0x304C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const CT_VERSION: &str = "unx0437_crashtsnap-v1";

/// 诊断段基址（0x304C0 段 · 本模块独占）。
pub const CT_SEG_BASE: u32 = 0x304C0;
pub const XC_SNAPSHOT_HOLE: u32 = 0x304C1; // 九元有哨兵孔
pub const XC_TS_REGRESS: u32 = 0x304C2; // 时间戳回退
pub const XC_CR2_ABUSE: u32 = 0x304C3; // CR2 乱填
pub const XC_CR2_MISS: u32 = 0x304C4; // #PF 快照 CR2 缺
pub const XC_REPLAY: u32 = 0x304C5; // 重放不一致
pub const XC_VEC_OOR: u32 = 0x304C6; // 向量越域
pub const XC_MAGIC: u32 = 0x304C9; // 账魔数破

/// 环形柜容量。
pub const CAB_CAP: usize = 16;
/// 未抓到哨兵。
pub const CR2_NONE: u64 = u64::MAX;
/// #PF 向量。
pub const VEC_PF: u8 = 14;

/// 九元快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrashSnap {
    pub vector: u8,
    pub err: u32,
    pub rip: u64,
    pub cs: u16,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u16,
    pub cr2: u64,
    pub tsc: u64,
}

impl CrashSnap {
    /// 哨兵完整性核：全字段非哨兵孔（RIP/CS/RSP 不得为零帧，
    /// CR2 语义另查）。
    pub fn intact(&self) -> Result<(), u32> {
        if self.vector == 0xFF || self.rip == 0 || self.cs == 0 || self.rsp == 0 || self.ss == 0 {
            return Err(XC_SNAPSHOT_HOLE);
        }
        if self.vector > 31 {
            return Err(XC_VEC_OOR);
        }
        Ok(())
    }

    /// CR2 语义核：#PF 必须带 CR2（非哨兵），非 #PF 必须哨兵。
    pub fn cr2_semantic(&self) -> Result<(), u32> {
        if self.vector == VEC_PF {
            if self.cr2 == CR2_NONE {
                return Err(XC_CR2_MISS);
            }
        } else if self.cr2 != CR2_NONE {
            return Err(XC_CR2_ABUSE);
        }
        Ok(())
    }
}

/// 16 环形黑匣子。
pub struct CrashCabinet {
    /// 环槽。
    pub ring: [Option<CrashSnap>; CAB_CAP],
    /// 写游标。
    pub cursor: usize,
    /// 总抓拍数（含被覆盖的）。
    pub total: u64,
}

impl CrashCabinet {
    pub fn new() -> Self {
        CrashRing::build()
    }

    /// 抓拍：入环（满覆最旧），游标推进。
    pub fn capture(&mut self, s: CrashSnap) -> Result<(), u32> {
        s.intact()?;
        s.cr2_semantic()?;
        // 时间戳单调核：与柜内最近份（游标前一格）比。
        let prev_idx = (self.cursor + CAB_CAP - 1) % CAB_CAP;
        if let Some(prev) = self.ring[prev_idx].as_ref() {
            if s.tsc < prev.tsc {
                return Err(XC_TS_REGRESS);
            }
        }
        self.ring[self.cursor] = Some(s);
        self.cursor = (self.cursor + 1) % CAB_CAP;
        self.total += 1;
        Ok(())
    }

    /// 柜内份数。
    pub fn count(&self) -> usize {
        self.ring.iter().filter(|x| x.is_some()).count()
    }

    /// 时间戳单调巡检（柜内按写入序）。
    pub fn monotonic(&self) -> Result<(), u32> {
        let mut last = 0u64;
        let mut first = true;
        // 按写入序遍历：从游标开始。
        for i in 0..self.count() {
            let idx = (self.cursor + CAB_CAP - self.count() + i) % CAB_CAP;
            if let Some(s) = self.ring[idx].as_ref() {
                if !first && s.tsc < last {
                    return Err(XC_TS_REGRESS);
                }
                last = s.tsc;
                first = false;
            }
        }
        Ok(())
    }

    /// 导出：按写入序取柜内全部快照。
    pub fn export(&self) -> [Option<CrashSnap>; CAB_CAP] {
        let mut out: [Option<CrashSnap>; CAB_CAP] = [None; CAB_CAP];
        let n = self.count();
        for i in 0..n {
            let idx = (self.cursor + CAB_CAP - n + i) % CAB_CAP;
            out[i] = self.ring[idx];
        }
        out
    }

    /// 重放核：空柜按导出序重抓，柜同。
    pub fn replay_ok(&self) -> Result<(), u32> {
        let mut fresh = CrashCabinet::new();
        for slot in self.export().iter() {
            if let Some(s) = slot {
                if fresh.capture(*s).is_err() {
                    return Err(XC_REPLAY);
                }
            }
        }
        if fresh.export() != self.export() {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

/// 构造辅助（避免数组初始化样板）。
struct CrashRing;
impl CrashRing {
    fn build() -> CrashCabinet {
        CrashCabinet { ring: [None; CAB_CAP], cursor: 0, total: 0 }
    }
}

/// 造快照（夹具）。
pub fn mk_snap(vector: u8, cr2: u64, tsc: u64) -> CrashSnap {
    CrashSnap { vector, err: 0, rip: 0x402000, cs: 0x08, rflags: 0x2, rsp: 0x8000, ss: 0x10, cr2, tsc }
}

/// 判据入口：UNX-F0437。
pub fn run_unx0437_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0437_crashtsnap");
    // 族一：抓拍与柜账。
    let mut c = CrashCabinet::new();
    cs.add("空柜", c.count() == 0, "初态");
    let _ = c.capture(mk_snap(13, CR2_NONE, 100));
    let _ = c.capture(mk_snap(6, CR2_NONE, 200));
    cs.add("两份入柜", c.count() == 2, "#GP+#UD");
    cs.add("总账 2", c.total == 2, "含未覆盖");
    cs.add("单调过", c.monotonic().is_ok(), "时间戳升");
    // 族二：CR2 语义。
    let mut c2 = CrashCabinet::new();
    cs.add("#PF 带 CR2 过", c2.capture(mk_snap(VEC_PF, 0x1234, 100)).is_ok(), "缺页地址");
    let no_cr2 = CrashSnap { cr2: CR2_NONE, ..mk_snap(VEC_PF, 0, 200) };
    cs.add("#PF 无 CR2 拒", c2.capture(no_cr2) == Err(XC_CR2_MISS), "黑匣缺线索");
    let abuse = CrashSnap { cr2: 0x999, ..mk_snap(6, CR2_NONE, 300) };
    cs.add("非 #PF 填 CR2 拒", c2.capture(abuse) == Err(XC_CR2_ABUSE), "假线索立案");
    cs.add("拒收不入柜", c2.count() == 1, "只收合规");
    // 族三：时间戳回退。
    let mut c3 = CrashCabinet::new();
    let _ = c3.capture(mk_snap(13, CR2_NONE, 500));
    cs.add("回退拒", c3.capture(mk_snap(6, CR2_NONE, 400)) == Err(XC_TS_REGRESS), "倒带");
    cs.add("同戳放行", c3.capture(mk_snap(6, CR2_NONE, 500)).is_ok(), "等号允许");
    // 族四：完整性核。
    let hole = CrashSnap { rip: 0, ..mk_snap(13, CR2_NONE, 600) };
    let mut c4 = CrashCabinet::new();
    cs.add("零 RIP 孔拒", c4.capture(hole) == Err(XC_SNAPSHOT_HOLE), "帧残");
    let vec_bad = CrashSnap { vector: 0xFF, ..mk_snap(13, CR2_NONE, 600) };
    cs.add("哨兵向量拒", c4.capture(vec_bad) == Err(XC_SNAPSHOT_HOLE), "0xFF 未抓");
    let vec_oob = CrashSnap { vector: 32, ..mk_snap(13, CR2_NONE, 600) };
    cs.add("越域向量拒", c4.capture(vec_oob) == Err(XC_VEC_OOR), ">31");
    // 族五：环形覆盖。
    let mut c5 = CrashCabinet::new();
    for i in 0..20u64 {
        let _ = c5.capture(mk_snap(13, CR2_NONE, 1000 + i * 10));
    }
    cs.add("满覆后仍 16", c5.count() == CAB_CAP, "环槽");
    cs.add("总账 20", c5.total == 20, "含覆盖 4");
    let exp = c5.export();
    cs.add("最旧是第 5 份", exp[0].map(|s| s.tsc == 1040).unwrap_or(false), "前 4 份被覆");
    cs.add("最新是第 20 份", exp[15].map(|s| s.tsc == 1190).unwrap_or(false), "游标前一格");
    // 族六：重放。
    cs.add("重放一致", c5.replay_ok().is_ok(), "同序同柜");
    // 族七：混合柜重放。
    let mut c6 = CrashCabinet::new();
    let _ = c6.capture(mk_snap(VEC_PF, 0xABC0, 10));
    let _ = c6.capture(mk_snap(8, CR2_NONE, 20));
    let _ = c6.capture(mk_snap(13, CR2_NONE, 30));
    cs.add("混合柜重放", c6.replay_ok().is_ok(), "PF/DF/GP");
    cs.add("混合单调", c6.monotonic().is_ok(), "10<20<30");
    // 族八：空柜重放。
    cs.add("空柜重放", CrashCabinet::new().replay_ok().is_ok(), "恒等");
    cs
}
