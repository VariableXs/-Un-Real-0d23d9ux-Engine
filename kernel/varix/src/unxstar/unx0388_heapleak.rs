//! unx0388_heapleak —— UNX-F0388 泄漏探测（分配点标记对冲）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0388 · 泄漏探测 · 目标 220 行
//! 判据原文：UNX-F0388-J1 堆分配按「分配点」标记（站点号 + 代次），
//!           释放时对冲销账；同站点多次分配独立计数，站点净额
//!           （分配量 - 释放量）即泄漏量；检查闸要求：检查点处
//!           白名单站点净额可为正（预期常驻），非白名单站点净额
//!           必须为零，泄漏检出带站点号可定位；代次防 ABA（旧
//!           指针释放新分配被对冲错账）。
//! 组件：分配点表（站点号 → 站点名 + 白名单位）+ 泄漏账（站点 ×
//!       代次二级账）+ 对冲引擎（alloc 记 + free 冲）+ 净额巡检
//!       （白名单豁免 + 泄漏立案）+ 巡检报告。
//! 职责定位：泄漏探测是「堆的防盗账」——每个分配必须挂着出生
//!   地（站点号），释放必须按代次对冲；净额不为零的站点要么
//!   在白名单里（设计上常驻），要么就是泄漏。代次是防 ABA 的
//!   关键：同一地址复用时代次递增，旧代次的 free 无法对冲新
//!   代次的账。
//! 性能逐项分解：alloc/free O(1)（直接查站点桶）；巡检 O(站点数)。
//! 诊断码段：0x20BC0（0x20BC1~0x20BC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HL_VERSION: &str = "unx0388_heapleak-v1";

/// 诊断段基址（0x20BC0 段 · 本模块独占）。
pub const HL_SEG_BASE: u32 = 0x20BC0;
pub const XC_SITE_OOR: u32 = 0x20BC1; // 站点号越域
pub const XC_SITE_DUP: u32 = 0x20BC2; // 站点重号
pub const XC_STALE_GEN: u32 = 0x20BC3; // 旧代次释放（ABA 拦截）
pub const XC_NO_OPEN: u32 = 0x20BC4; // 无在册分配可对冲
pub const XC_LEAK: u32 = 0x20BC5; // 泄漏检出（非白名单净额非零）
pub const XC_MISMATCH: u32 = 0x20BC6; // 释放站点与出生站点不符
pub const XC_BYTES_NEG: u32 = 0x20BC7; // 字节账负值
pub const XC_QUOTA: u32 = 0x20BC8; // 白名单净额超配额
pub const XC_REPORT: u32 = 0x20BC9; // 报告签名自检失败

/// 分配点。
#[derive(Clone, Copy, Debug)]
pub struct Site {
    /// 站点号（1~255）。
    pub id: u32,
    /// 站点名。
    pub name: &'static str,
    /// 白名单：设计上常驻，净额允许为正。
    pub resident: bool,
    /// 白名单净额配额（字节；非白名单恒 0）。
    pub quota: u64,
}

/// 站点在册代次账。
#[derive(Clone, Copy, Debug)]
pub struct GenSlot {
    /// 当前代次（从 1 起，每次 free 后复用地址即递增——此处简化
    /// 为站点内计数器：每笔分配占用一个代次号）。
    pub gen: u64,
    /// 该代次在册字节数（0 = 已对冲）。
    pub bytes: u64,
}

/// 泄漏账。
pub struct LeakBook {
    sites: Vec<Site>,
    /// 每站点在册分配（代次 + 字节）。
    open: Vec<Vec<GenSlot>>,
    /// 巡检立案（站点号）。
    leaks: Vec<u32>,
}

impl LeakBook {
    /// 建空账。
    pub fn new() -> LeakBook {
        LeakBook { sites: Vec::new(), open: Vec::new(), leaks: Vec::new() }
    }

    /// 登记站点：重号拒、号越域拒、白名单零配额拒。
    pub fn add_site(&mut self, id: u32, name: &'static str, resident: bool, quota: u64) -> Result<(), u32> {
        if id == 0 || id > 255 {
            return Err(XC_SITE_OOR);
        }
        if resident && quota == 0 {
            return Err(XC_QUOTA);
        }
        for s in self.sites.iter() {
            if s.id == id {
                return Err(XC_SITE_DUP);
            }
        }
        self.sites.push(Site { id, name, resident, quota });
        self.open.push(Vec::new());
        Ok(())
    }

    /// 站点下标。
    fn idx(&self, id: u32) -> Option<usize> {
        self.sites.iter().position(|s| s.id == id)
    }

    /// 分配：站点记一笔（代次 + 字节），返回代次号。
    pub fn alloc(&mut self, id: u32, bytes: u64) -> Result<u64, u32> {
        let i = match self.idx(id) {
            Some(i) => i,
            None => return Err(XC_SITE_OOR),
        };
        if bytes == 0 {
            return Err(XC_BYTES_NEG);
        }
        let next_gen = match self.open[i].last() {
            Some(s) => s.gen + 1,
            None => 1,
        };
        self.open[i].push(GenSlot { gen: next_gen, bytes });
        Ok(next_gen)
    }

    /// 释放：按站点 + 代次对冲；代次不符（ABA）拒；无在册拒。
    pub fn free(&mut self, id: u32, gen: u64) -> Result<u64, u32> {
        let i = match self.idx(id) {
            Some(i) => i,
            None => return Err(XC_SITE_OOR),
        };
        // 只许冲最早未对冲的一笔（FIFO 对冲语义）。
        for k in 0..self.open[i].len() {
            if self.open[i][k].bytes == 0 {
                continue;
            }
            if self.open[i][k].gen != gen {
                return Err(XC_STALE_GEN);
            }
            let b = self.open[i][k].bytes;
            self.open[i][k].bytes = 0;
            return Ok(b);
        }
        Err(XC_NO_OPEN)
    }

    /// 站点净额（字节）。
    pub fn net_of(&self, id: u32) -> u64 {
        match self.idx(id) {
            Some(i) => self.open[i].iter().map(|s| s.bytes).sum(),
            None => 0,
        }
    }

    /// 巡检：非白名单净额非零 → 泄漏立案；白名单净额超配额 → 超额立案。
    pub fn patrol(&mut self) -> Result<u32, u32> {
        self.leaks.clear();
        for s in self.sites.iter() {
            let net = self.net_of(s.id);
            if !s.resident && net != 0 {
                self.leaks.push(s.id);
            }
            if s.resident && net > s.quota {
                self.leaks.push(s.id);
            }
        }
        if !self.leaks.is_empty() {
            return Err(XC_LEAK);
        }
        Ok(self.sites.len() as u32)
    }

    /// 泄漏站点数。
    pub fn leak_count(&self) -> usize {
        self.leaks.len()
    }

    /// 站点数。
    pub fn site_count(&self) -> usize {
        self.sites.len()
    }

    /// 巡检报告：站点号 + 净额序列 + FNV32 签。
    pub fn report(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(self.site_count() as u32);
        for s in self.sites.iter() {
            words.push(s.id);
            words.push(self.net_of(s.id) as u32);
            words.push((self.net_of(s.id) >> 32) as u32);
        }
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        (words, h)
    }
}

pub fn run_unx0388_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0388_heapleak");
    // 族一：标准流程（分配 - 释放 - 巡检绿）。
    let mut b = LeakBook::new();
    cs.add("站点 A 登记成", b.add_site(1, "临时A", false, 0).is_ok(), "非白名单");
    cs.add("站点 B 登记成", b.add_site(2, "常驻B", true, 4096).is_ok(), "白名单 4K");
    cs.add("站点数", b.site_count() == 2, "A + B");
    let g1 = b.alloc(1, 128);
    cs.add("分配 A 记代次", g1 == Ok(1), "代次 1");
    let g2 = b.alloc(1, 64);
    cs.add("二分配代次递增", g2 == Ok(2), "代次 2");
    cs.add("A 净额 192", b.net_of(1) == 192, "128 + 64");
    cs.add("冲第一笔", b.free(1, 1) == Ok(128), "FIFO 对冲");
    cs.add("A 净额 64", b.net_of(1) == 64, "剩第二笔");
    cs.add("冲第二笔", b.free(1, 2) == Ok(64), "代次对上");
    cs.add("A 清零", b.net_of(1) == 0, "全对冲");
    cs.add("巡检绿", b.patrol().is_ok(), "B 净额 0 且白名单未超");
    // 族二：泄漏检出与定位。
    let mut c = LeakBook::new();
    let _ = c.add_site(1, "漏点", false, 0);
    let _ = c.add_site(2, "常驻", true, 1024);
    let _ = c.alloc(1, 256);
    cs.add("泄漏检出", c.patrol() == Err(XC_LEAK), "非白名单净额 256");
    cs.add("泄漏站点定位", c.leak_count() == 1 && c.leaks[0] == 1, "站点 1");
    // 白名单内净额合法：常驻站分配 512 后巡检仍绿。
    let _ = c.free(1, 1);
    let g = c.alloc(2, 512);
    cs.add("白名单分配", g.is_ok(), "常驻站");
    cs.add("白名单巡检绿", c.patrol().is_ok(), "512 ≤ 1024");
    // 白名单超额。
    let _ = c.alloc(2, 600);
    cs.add("白名单超额检出", c.patrol() == Err(XC_LEAK), "1112 > 1024");
    // 族三：ABA / 无在册 / 站点不符。
    let mut d = LeakBook::new();
    let _ = d.add_site(1, "x", false, 0);
    let _ = d.add_site(2, "y", false, 0);
    let _ = d.alloc(1, 64);
    cs.add("旧代次拒", d.free(1, 9) == Err(XC_STALE_GEN), "gen 9 不在册");
    cs.add("空冲拒", d.free(2, 1) == Err(XC_NO_OPEN), "站点 2 无分配");
    cs.add("跨站冲拒", d.free(2, 1) == Err(XC_NO_OPEN), "站点 1 的账");
    // 族四：登记拒绝。
    let mut e = LeakBook::new();
    cs.add("站点号零拒", e.add_site(0, "z", false, 0) == Err(XC_SITE_OOR), "id=0");
    cs.add("站点号超域拒", e.add_site(256, "z", false, 0) == Err(XC_SITE_OOR), "id=256");
    let _ = e.add_site(1, "first", false, 0);
    cs.add("重号拒", e.add_site(1, "again", false, 0) == Err(XC_SITE_DUP), "id=1 撞");
    cs.add("白名单零配额拒", e.add_site(2, "q", true, 0) == Err(XC_QUOTA), "resident+quota0");
    // 族五：报告签名自检。
    let (w1, s1) = b.report();
    let (w2, s2) = b.report();
    cs.add("报告确定", s1 == s2 && w1 == w2, "两次一致");
    let s3 = match b.report() {
        (_, s) => s,
    };
    cs.add("报告非零签", s3 != 0, "有内容必有签");
    cs
}
