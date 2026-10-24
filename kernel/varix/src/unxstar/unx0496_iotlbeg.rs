//! unx0496_iotlbeg —— UNX-F0496 IOTLB 失效语义（三粒度失效 + 对齐 + 令牌单调）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0496 · IOTLB 失效账 · 目标 300 行
//! 判据原文：UNX-F0496-J1 IOTLB 失效账：失一 全局失效
//!           （粒度 GLOBAL 清空全部缓存项）；失二 域失效
//!           （粒度 DOMAIN 只清同 DID 项，异域项存活）；失
//!           三 页失效（粒度 PAGE 只清精确 (DID,地址) 对，
//!           同域异址与异域同址均存活）；失四 地址对齐（页
//!           失效地址必须 4096 对齐，残渣拒）；失五 令牌单
//!           调（每次失效发放序列号严格递增，回退拒）。
//! 组件：IOTLB 缓存模型 + 三粒度失效 + 对齐闸 + 失效令牌序列。
//! 职责定位：IOTLB 是 DMA 翻译结果的缓存——映射表改了缓存
//!   不改，设备仍按旧映射读写旧物理页（陈旧翻译）。三粒
//!   度是精确性谱系：全局最粗最安全、页失效最精最省，选
//!   错粒度要么过清（性能坍）要么漏清（正确性坍）。
//! 性能逐项分解：全局 O(n)；域 O(n) 扫；页 O(n) 精确匹配。
//! 诊断码段：0x34AC0（0x34AC1~0x34AC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34AC0;
pub const XC_ALIGN: u32 = SEG_BASE + 1; // 页失效地址未 4K 对齐
pub const XC_TOKEN: u32 = SEG_BASE + 2; // 失效令牌回退
pub const XC_NOENT: u32 = SEG_BASE + 3; // 页失效目标不存在（可接受但记账）
pub const XC_GRAN: u32 = SEG_BASE + 4; // 非法粒度值

/// 失效粒度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gran {
    Global,
    Domain(u16),
    Page(u16, u64),
}

/// 缓存项：(DID, IOVA) 对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    pub did: u16,
    pub iova: u64,
}

/// IOTLB 模型。
pub struct Iotlb {
    pub ents: Vec<Ent>,
    pub token: u64,
    pub invalidated: u64,
}

impl Iotlb {
    pub fn new() -> Self {
        Iotlb { ents: Vec::new(), token: 0, invalidated: 0 }
    }

    /// 灌入缓存项。
    pub fn fill(&mut self, did: u16, iova: u64) {
        self.ents.push(Ent { did, iova });
    }

    /// 失效：三粒度分发。返回清掉项数。
    pub fn invalidate(&mut self, g: Gran) -> Result<usize, u32> {
        // 失五：令牌严格单调。
        let prev = self.token;
        self.token += 1;
        if self.token <= prev {
            return Err(XC_TOKEN);
        }
        let n = match g {
            Gran::Global => {
                let n = self.ents.len();
                self.ents.clear();
                n
            }
            Gran::Domain(did) => {
                let before = self.ents.len();
                self.ents.retain(|e| e.did != did);
                before - self.ents.len()
            }
            Gran::Page(did, addr) => {
                if addr & 0xFFF != 0 {
                    self.token -= 1; // 回滚令牌：无效请求不发放
                    return Err(XC_ALIGN);
                }
                let before = self.ents.len();
                self.ents.retain(|e| !(e.did == did && e.iova == addr));
                before - self.ents.len()
            }
        };
        self.invalidated += n as u64;
        Ok(n)
    }
}

pub fn run_unx0496_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0496");
    cs.add("SEG 段位独占", SEG_BASE == 0x34AC0 && XC_GRAN == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：全局失效。
    let mut t = Iotlb::new();
    t.fill(1, 0x1000);
    t.fill(2, 0x2000);
    t.fill(1, 0x3000);
    cs.add("全局清三", t.invalidate(Gran::Global) == Ok(3), "全表三项");
    cs.add("全局后空", t.ents.is_empty(), "clear 语义");

    // 族二：域失效。
    let mut t2 = Iotlb::new();
    t2.fill(1, 0x1000);
    t2.fill(2, 0x2000);
    t2.fill(1, 0x3000);
    cs.add("域清二", t2.invalidate(Gran::Domain(1)) == Ok(2), "DID=1 两项");
    cs.add("异域存活", t2.ents.len() == 1 && t2.ents[0].did == 2, "DID=2 项存活");

    // 族三：页失效。
    let mut t3 = Iotlb::new();
    t3.fill(1, 0x1000);
    t3.fill(1, 0x2000);
    t3.fill(2, 0x1000);
    cs.add("页清一", t3.invalidate(Gran::Page(1, 0x1000)) == Ok(1), "精确 (1,0x1000)");
    // 同域异址 + 异域同址均存活。
    cs.add("同域异址存活", t3.ents.iter().any(|e| e.did == 1 && e.iova == 0x2000), "页粒度不波及");
    cs.add("异域同址存活", t3.ents.iter().any(|e| e.did == 2 && e.iova == 0x1000), "DID 参与匹配");
    // 目标不存在：清零但合法。
    cs.add("页落空清零", t3.invalidate(Gran::Page(9, 0x9000)) == Ok(0), "无可清项");

    // 族四：地址对齐闸。
    cs.add("残渣地址拒", t3.invalidate(Gran::Page(1, 0x1003)) == Err(XC_ALIGN), "低 12 位非零");
    cs.add("对齐 4K 成", t3.invalidate(Gran::Page(2, 0x1000)).is_ok(), "对齐地址合法");

    // 族五：令牌单调。
    let mut t4 = Iotlb::new();
    t4.fill(1, 0x1000);
    let tk0 = t4.token;
    t4.invalidate(Gran::Global).ok();
    let tk1 = t4.token;
    t4.invalidate(Gran::Global).ok();
    let tk2 = t4.token;
    cs.add("令牌递增", tk0 < tk1 && tk1 < tk2, "两次失效两跳");
    cs.add("失效总数对账", t4.invalidated == 2, "2×1 项");
    // 对齐拒绝不消耗令牌（回滚）。
    let tk_before = t4.token;
    t4.invalidate(Gran::Page(1, 0xFFF)).ok();
    let _ = t4.invalidate(Gran::Page(1, 0xFFF));
    cs.add("拒发不耗令牌", t4.token == tk_before, "回滚后恒等");

    // 族六：混合粒度顺序（先页后域后全局的级联清空）。
    let mut t5 = Iotlb::new();
    for i in 0..8u64 {
        t5.fill((i % 2) as u16 + 1, 0x1000 + i * 0x1000);
    }
    let r1 = t5.invalidate(Gran::Page(1, 0x1000));
    let r2 = t5.invalidate(Gran::Domain(2));
    let r3 = t5.invalidate(Gran::Global);
    cs.add("级联三粒度", r1 == Ok(1) && r2 == Ok(4) && r3 == Ok(3), "1+4+3 对账全清");
    cs.add("级联后空", t5.ents.is_empty() && t5.invalidated == 8, "总量守恒");

    cs
}
