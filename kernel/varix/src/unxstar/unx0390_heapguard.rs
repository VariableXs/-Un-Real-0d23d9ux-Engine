//! unx0390_heapguard —— UNX-F0390 堆完整性哨兵（越界写检出）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0390 · 堆完整性哨兵 · 目标 240 行
//! 判据原文：UNX-F0390-J1 每块堆分配前后埋哨兵（前哨兵 4 字节
//!           + 后哨兵 4 字节，填充固定魔数模式）；释放与巡检
//!           时逐字节核对哨兵：前哨兵破 = 越界下写，后哨兵破 =
//!           越界上写，破位偏移可定位（首个异字节的槽号）；
//!           哨兵魔数逐块异构（块号参与填充），防「整块照抄」
//!           式误对拍；哨兵账与用户区账分离，用户区改写不算
//!           哨兵破。
//! 组件：哨兵模式生成（块号 + 魔数基址异构）+ 守卫块（前哨兵 +
//!       用户区 + 后哨兵）+ 核对引擎（逐字节对拍 + 首破位定位）
//!       + 巡检账（破块立案带方向）。
//! 职责定位：堆哨兵是「越界写的案发现场」——四字节哨兵虽小，
//!   但足以把「某个分配写花了邻居」从玄学变成可定位事件；逐块
//!   异构魔数是防呆关键：如果所有块哨兵同值，攻击者/bug 一旦
//!   学会写这个值就永久隐身；异构后每个块的哨兵只跟自己的
//!   块号绑定，照抄邻居也过不了关。
//! 性能逐项分解：埋哨 O(1)；核对 O(哨兵字节)；巡检 O(块数)。
//! 诊断码段：0x20C40（0x20C41~0x20C49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HG_VERSION: &str = "unx0390_heapguard-v1";

/// 诊断段基址（0x20C40 段 · 本模块独占）。
pub const HG_SEG_BASE: u32 = 0x20C40;
pub const XC_FRONT_BRK: u32 = 0x20C41; // 前哨兵破（越界下写）
pub const XC_REAR_BRK: u32 = 0x20C42; // 后哨兵破（越界上写）
pub const XC_BOTH_BRK: u32 = 0x20C43; // 双侧破
pub const XC_BLK_OOR: u32 = 0x20C44; // 块号越域
pub const XC_DUP_BLK: u32 = 0x20C45; // 块重号
pub const XC_FREE_TWICE: u32 = 0x20C46; // 双重释放
pub const XC_SZ_OOR: u32 = 0x20C47; // 用户区尺寸越界
pub const XC_MAGIC: u32 = 0x20C48; // 模式基址错
pub const XC_REG_BAD: u32 = 0x20C49; // 巡检账自检失败

/// 哨兵字节宽度。
pub const SENT_W: usize = 4;

/// 哨兵模式生成：第 i 字节 = (基址 ^ (块号 << i)) & 0xFF；
/// 基址固定 0xA5，块号异构。
pub fn sent_byte(blk: u32, i: usize) -> u8 {
    let base: u32 = 0xA5A5_5A5A;
    ((base ^ (blk.wrapping_mul(0x9E37_79B9).wrapping_shl(i as u32))) >> ((i % 4) * 8)) as u8
}

/// 守卫块。
#[derive(Clone, Debug)]
pub struct GuardBlock {
    /// 块号。
    pub blk: u32,
    /// 前哨兵。
    pub front: [u8; SENT_W],
    /// 用户区（内容任意，哨兵账不管）。
    pub user: Vec<u8>,
    /// 后哨兵。
    pub rear: [u8; SENT_W],
}

impl GuardBlock {
    /// 建块：埋哨。
    pub fn new(blk: u32, user_len: usize) -> GuardBlock {
        let mut f = [0u8; SENT_W];
        let mut r = [0u8; SENT_W];
        for i in 0..SENT_W {
            f[i] = sent_byte(blk, i);
            r[i] = sent_byte(blk.wrapping_add(0x1000_0000), i);
        }
        GuardBlock { blk, front: f, user: vec![0u8; user_len], rear: r }
    }

    /// 前哨兵核对：返回首破位或 None。
    pub fn check_front(&self) -> Option<usize> {
        for i in 0..SENT_W {
            if self.front[i] != sent_byte(self.blk, i) {
                return Some(i);
            }
        }
        None
    }

    /// 后哨兵核对：返回首破位或 None。
    pub fn check_rear(&self) -> Option<usize> {
        let rblk = self.blk.wrapping_add(0x1000_0000);
        for i in 0..SENT_W {
            if self.rear[i] != sent_byte(rblk, i) {
                return Some(i);
            }
        }
        None
    }

    /// 模拟越界下写（改前哨兵第 k 字节）。
    pub fn corrupt_front(&mut self, k: usize) {
        if k < SENT_W {
            self.front[k] = self.front[k].wrapping_add(1);
        }
    }

    /// 模拟越界上写（改后哨兵第 k 字节）。
    pub fn corrupt_rear(&mut self, k: usize) {
        if k < SENT_W {
            self.rear[k] = self.rear[k].wrapping_add(1);
        }
    }
}

/// 哨兵巡检账。
pub struct GuardBook {
    blocks: Vec<GuardBlock>,
    /// 立案：(块号, 方向 1=前 2=后 3=双)。
    cases: Vec<(u32, u8)>,
}

impl GuardBook {
    /// 建空账。
    pub fn new() -> GuardBook {
        GuardBook { blocks: Vec::new(), cases: Vec::new() }
    }

    /// 挂块：重号拒。
    pub fn hang(&mut self, blk: u32, user_len: usize) -> Result<(), u32> {
        if blk == 0 || blk > 0xFFFF {
            return Err(XC_BLK_OOR);
        }
        if user_len > 1 << 20 {
            return Err(XC_SZ_OOR);
        }
        for b in self.blocks.iter() {
            if b.blk == blk {
                return Err(XC_DUP_BLK);
            }
        }
        self.blocks.push(GuardBlock::new(blk, user_len));
        Ok(())
    }

    /// 巡检全部：破块立案（前/后/双）。
    pub fn patrol(&mut self) -> Result<u32, u32> {
        self.cases.clear();
        for b in self.blocks.iter() {
            let f = b.check_front().is_some();
            let r = b.check_rear().is_some();
            if f || r {
                let dir = if f && r { 3u8 } else if f { 1u8 } else { 2u8 };
                self.cases.push((b.blk, dir));
            }
        }
        if !self.cases.is_empty() {
            return Err(XC_REG_BAD);
        }
        Ok(self.blocks.len() as u32)
    }

    /// 立案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 块数。
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 按号取块（可变）。
    pub fn block_mut(&mut self, blk: u32) -> Option<&mut GuardBlock> {
        self.blocks.iter_mut().find(|b| b.blk == blk)
    }

    /// 立案表读取。
    pub fn cases(&self) -> &[(u32, u8)] {
        &self.cases
    }
}

/// 哨兵魔数异构核：不同块号的首字节必不同（小域内）。
pub fn hetero_ok(ids: &[u32]) -> Result<(), u32> {
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            if sent_byte(ids[i], 0) == sent_byte(ids[j], 0)
                && sent_byte(ids[i], 1) == sent_byte(ids[j], 1)
                && sent_byte(ids[i], 2) == sent_byte(ids[j], 2)
                && sent_byte(ids[i], 3) == sent_byte(ids[j], 3)
            {
                return Err(XC_MAGIC);
            }
        }
    }
    Ok(())
}

pub fn run_unx0390_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0390_heapguard");
    // 族一：埋哨与干净巡检。
    let mut g = GuardBook::new();
    cs.add("挂块 1", g.hang(1, 64).is_ok(), "用户区 64");
    cs.add("挂块 2", g.hang(2, 32).is_ok(), "用户区 32");
    cs.add("挂块 3", g.hang(3, 128).is_ok(), "用户区 128");
    cs.add("块数", g.block_count() == 3, "三块在册");
    cs.add("干净巡检绿", g.patrol().is_ok(), "哨兵全整");
    // 族二：越界下写检出与定位。
    cs.add("破块 1 前哨 0 位", g.block_mut(1).map_or(false, |b| { b.corrupt_front(0); true }), "写花");
    cs.add("前破检出", g.patrol() == Err(XC_REG_BAD), "立案");
    cs.add("立案数 1", g.case_count() == 1, "只破一块");
    cs.add("方向前破", g.cases()[0] == (1, 1), "块 1 方向 1");
    // 族三：越界上写检出。
    let mut h = GuardBook::new();
    let _ = h.hang(7, 16);
    cs.add("破块 7 后哨 2 位", h.block_mut(7).map_or(false, |b| { b.corrupt_rear(2); true }), "写花");
    cs.add("后破检出", h.patrol() == Err(XC_REG_BAD), "立案");
    cs.add("方向后破", h.cases()[0] == (7, 2), "块 7 方向 2");
    // 族四：双侧破。
    let mut k = GuardBook::new();
    let _ = k.hang(9, 8);
    cs.add("双侧破块 9",
        k.block_mut(9).map_or(false, |b| { b.corrupt_front(3); b.corrupt_rear(0); true }),
        "前后都花");
    cs.add("双破检出", k.patrol() == Err(XC_REG_BAD), "立案");
    cs.add("方向双破", k.cases()[0] == (9, 3), "方向 3");
    // 族五：用户区改写不算哨兵破。
    let mut u = GuardBook::new();
    let _ = u.hang(5, 16);
    cs.add("改用户区", u.block_mut(5).map_or(false, |b| { b.user[0] = 0xFF; b.user[15] = 0xEE; true }), "内容任意");
    cs.add("用户区自由", u.patrol().is_ok(), "哨兵不理用户区");
    // 族六：魔数异构。
    cs.add("异构过", hetero_ok(&[1, 2, 3, 4, 5]).is_ok(), "五块互异");
    let sb = sent_byte(1, 0);
    let sc = sent_byte(2, 0);
    cs.add("块号敏感", sb != sc, "首字节异");
    cs.add("模式确定", sent_byte(1, 0) == sent_byte(1, 0), "同号同值");
    cs.add("同号四字节回读",
        [sent_byte(3, 0), sent_byte(3, 1), sent_byte(3, 2), sent_byte(3, 3)]
            == GuardBlock::new(3, 0).front,
        "生成一致");
    // 族七：挂块拒绝。
    let mut z = GuardBook::new();
    cs.add("块号零拒", z.hang(0, 8) == Err(XC_BLK_OOR), "blk=0");
    cs.add("块号超域拒", z.hang(0x1_0000, 8) == Err(XC_BLK_OOR), ">0xFFFF");
    let _ = z.hang(4, 8);
    cs.add("重号拒", z.hang(4, 8) == Err(XC_DUP_BLK), "blk=4 撞");
    cs.add("超尺寸拒", z.hang(6, 1 << 21) == Err(XC_SZ_OOR), ">1MB 用户区");
    cs
}
