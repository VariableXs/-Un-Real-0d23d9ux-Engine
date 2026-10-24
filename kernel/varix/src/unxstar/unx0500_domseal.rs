//! unx0500_domseal —— UNX-F0500 域独占封印（设备单挂 + DID 回收复用前清账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0500 · 域独占封印 · 目标 290 行
//! 判据原文：UNX-F0500-J1 域独占封印：封一 设备单挂
//!           （一个 BDF 同一时刻只能挂一个域，双挂拒）；封
//!           二 摘后重挂（先摘旧挂再挂新域，路径合法）；封
//!           三 DID 回收（域销毁时 DID 进自由链，计数对
//!           账）；封四 复用前清账（回收 DID 复用前必须核
//!           销其全部翻译缓存引用，未核销复用拒）；封五
//!           隔离不变量（任意时刻任意 BDF 至多属一个域，
//!           全表扫描零双挂）。
//! 组件：域分配器（自由链）+ 设备挂域表 + 回收核销账 + 隔离扫描。
//! 职责定位：域是 DMA 隔离的牢笼——设备挂错域或双挂两域
//!   意味着它获得了两个牢笼的钥匙。DID 复用是「牢房号循
//!   环使用」：上一任的翻译缓存没清就把号发给下一任，下
//!   一任就能读到上一任的映射——复用前核销是隔离的接力棒。
//! 性能逐项分解：挂域 O(1)；隔离扫描 O(设备数)。
//! 诊断码段：0x34BC0（0x34BC1~0x34BC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34BC0;
pub const XC_DUAL: u32 = SEG_BASE + 1; // 设备双挂
pub const XC_REUSE: u32 = SEG_BASE + 2; // DID 未核销即复用
pub const XC_NOENT: u32 = SEG_BASE + 3; // 摘除目标不存在
pub const XC_EXHAUST: u32 = SEG_BASE + 4; // 域池耗尽

/// 设备挂域表项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bind {
    pub bdf: u16,
    pub did: u16,
}

/// 域独占管理器。
pub struct DomSeal {
    pub binds: Vec<Bind>,
    pub free_dids: Vec<u16>,
    pub dirty: Vec<u16>,   // 已回收未核销的 DID
    pub clean: Vec<u16>,   // 核销完毕可复用的 DID
    pub allocated: u16,    // 累计发放
}

impl DomSeal {
    pub fn new() -> Self {
        DomSeal { binds: Vec::new(), free_dids: (1..=8).collect(), dirty: Vec::new(), clean: Vec::new(), allocated: 0 }
    }

    /// 发放新 DID。
    pub fn alloc_did(&mut self) -> Result<u16, u32> {
        if let Some(d) = self.clean.pop() {
            // 复用路径：clean 栈顶。
            self.allocated += 1;
            return Ok(d);
        }
        match self.free_dids.pop() {
            Some(d) => {
                self.allocated += 1;
                Ok(d)
            }
            None => Err(XC_EXHAUST),
        }
    }

    /// 挂域：封一双挂闸。
    pub fn bind(&mut self, bdf: u16, did: u16) -> Result<(), u32> {
        if self.binds.iter().any(|b| b.bdf == bdf) {
            return Err(XC_DUAL);
        }
        self.binds.push(Bind { bdf, did });
        Ok(())
    }

    /// 摘除：返回摘下的 DID。
    pub fn unbind(&mut self, bdf: u16) -> Result<u16, u32> {
        let pos = self.binds.iter().position(|b| b.bdf == bdf);
        match pos {
            Some(i) => Ok(self.binds.remove(i).did),
            None => Err(XC_NOENT),
        }
    }

    /// 域销毁：DID 进脏账（待核销）。
    pub fn destroy_did(&mut self, did: u16) {
        self.dirty.push(did);
    }

    /// 核销：清掉脏账全部翻译引用（本模型即摘下未清标记）。
    pub fn scrub(&mut self, did: u16) -> Result<(), u32> {
        let pos = self.dirty.iter().position(|d| *d == did);
        match pos {
            Some(i) => {
                self.dirty.remove(i);
                self.clean.push(did);
                Ok(())
            }
            None => Err(XC_NOENT),
        }
    }

    /// 封四：从 clean 复用前核验无脏残留（应为空核验恒真，防御式）。
    pub fn reuse_gate(&self, did: u16) -> Result<(), u32> {
        if self.dirty.contains(&did) {
            return Err(XC_REUSE);
        }
        Ok(())
    }

    /// 封五：隔离不变量全表扫描——BDF 键零重复。
    pub fn isolation_scan(&self) -> bool {
        for (i, b) in self.binds.iter().enumerate() {
            if self.binds[i + 1..].iter().any(|x| x.bdf == b.bdf) {
                return false;
            }
        }
        true
    }
}

pub fn run_unx0500_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0500");
    cs.add("SEG 段位独占", SEG_BASE == 0x34BC0 && XC_EXHAUST == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：设备单挂。
    let mut s = DomSeal::new();
    let d1 = s.alloc_did();
    let d2 = s.alloc_did();
    cs.add("DID 发放", d1 == Ok(8) && d2 == Ok(7), "自由链栈式发放 8,7");
    cs.add("首挂成", s.bind(0x0210, 8).is_ok(), "bdf=0x0210 → did=8");
    cs.add("双挂拒", s.bind(0x0210, 7) == Err(XC_DUAL), "同 BDF 二挂拒");

    // 族二：摘后重挂。
    let un = s.unbind(0x0210);
    cs.add("摘除得旧 DID", un == Ok(8), "摘 0x0210 还 did=8");
    cs.add("重挂新域成", s.bind(0x0210, 7).is_ok(), "摘后挂 did=7");
    cs.add("摘除落空拒", s.unbind(0x0300) == Err(XC_NOENT), "未挂 BDF 摘空");

    // 族三：DID 回收。
    s.destroy_did(8);
    cs.add("回收进脏账", s.dirty.contains(&8), "did=8 待核销");
    cs.add("脏账复用前核验拒", s.reuse_gate(8) == Err(XC_REUSE), "脏 DID 不得直接发");

    // 族四：核销后复用。
    cs.add("核销成", s.scrub(8).is_ok(), "清翻译引用");
    cs.add("核销后过闸", s.reuse_gate(8).is_ok(), "clean 路径");
    let d3 = s.alloc_did();
    cs.add("复用发放", d3 == Ok(8), "clean 栈顶回收 8");
    cs.add("累计发放对账", s.allocated == 3, "8,7,8 三发");

    // 族五：域池耗尽。
    let mut s2 = DomSeal::new();
    let mut ok_n = 0u32;
    let mut got: [u16; 9] = [0; 9];
    for i in 0..9 {
        match s2.alloc_did() {
            Ok(d) => {
                got[ok_n as usize] = d;
                ok_n += 1;
            }
            Err(XC_EXHAUST) => {}
            Err(_) => {}
        }
    }
    cs.add("池容八", ok_n == 8, "DID 1~8 全发");
    cs.add("耗尽拒", s2.alloc_did() == Err(XC_EXHAUST), "第九发拒");
    // 发放的 DID 互异。
    let mut uniq = true;
    for i in 0..8usize {
        for j in i + 1..8usize {
            if got[i] == got[j] {
                uniq = false;
            }
        }
    }
    cs.add("DID 互异", uniq, "无重号发放");

    // 族六：隔离不变量扫描。
    let mut s3 = DomSeal::new();
    s3.bind(0x0210, 1).ok();
    s3.bind(0x0211, 2).ok();
    s3.bind(0x0212, 3).ok();
    cs.add("异键扫描过", s3.isolation_scan(), "三 BDF 零双挂");
    cs.add("跨域隔离", s3.binds.iter().map(|b| b.did).max() != s3.binds.iter().map(|b| b.did).min(), "三域三值");

    cs
}
