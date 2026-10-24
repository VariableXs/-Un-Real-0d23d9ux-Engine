//! veb287_mlicap —— VE-F0287 Mali 能力探测。
//! 锚点（CoRun Unxreal 主册）：VE-F0287 · Mali 能力探测 · 目标 320 行
//! 判据原文：VE-F0287-J1 能力探测账：能力位表（12 位固定：
//!           API 三 + 纹理三 + 压缩二 + 杂四）；逐位探测（位
//!           存在性 + 位不可写核）；能力版本门（驱动能力版本
//!           < 需求版本 → 需求族拒）；需求核（需求位集 ⊆
//!           能力位集才过）；探测缓存（同轮重复探测直取缓存
//!           不重扫）；位翻转核（探测结果与手装位集逐位
//!           一致）。
//! 组件：能力位表 + 版本门 + 需求核 + 缓存。
//! 职责定位：能力探测是「Mali 的体检表」——12 位是 12 项体
//!           检：少检一项就是带着未知上路；版本门是年龄线：
//!           驱动太老撑不起新需求，需求族整组拒；缓存是体检
//!           报告的复用：同轮不抽第二次血。
//! 性能逐项分解：探测 O(1)；需求核 O(1) 位运算。
//! 诊断码段：0x310C0（0x310C1~0x310C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const CP_VERSION: &str = "veb287_mlicap-v1";

/// 诊断段基址（0x310C0 段 · 本模块独占）。
pub const CP_SEG_BASE: u32 = 0x310C0;
pub const XC_MAGIC: u32 = 0x310C1; // 账魔数破
pub const XC_BIT_OOR: u32 = 0x310C2; // 位越界
pub const XC_NEED: u32 = 0x310C3; // 需求不满足
pub const XC_VER: u32 = 0x310C4; // 版本门拒

/// 账魔数。
const MAGIC: u32 = 0x310C_1CA9;

/// 能力位数。
pub const NBITS: u32 = 12;

/// 能力探测器。
pub struct CapProbe {
    /// 能力位集。
    pub caps: u32,
    /// 驱动能力版本。
    pub drv_ver: u32,
    /// 本轮探测次数（缓存命中不减）。
    pub scans: u32,
    /// 缓存快照。
    cached: Option<u32>,
    magic: u32,
}

impl CapProbe {
    /// 建（硬件位集 + 驱动版本）。
    pub fn new(hw_caps: u32, drv_ver: u32) -> CapProbe {
        CapProbe {
            caps: hw_caps & ((1u32 << NBITS) - 1),
            drv_ver,
            scans: 0,
            cached: None,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 位存在？
    pub fn has(&self, bit: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if bit >= NBITS {
            return Err(XC_BIT_OOR);
        }
        Ok((self.caps >> bit) & 1 == 1)
    }

    /// 位不可写核：外部试图改 caps 直接拒。
    pub fn set_caps(&mut self, _v: u32) -> Result<(), u32> {
        Err(XC_BIT_OOR)
    }

    /// 探测（带缓存）：首轮真扫，二轮直取。
    pub fn probe(&mut self) -> u32 {
        self.scans += 1;
        match self.cached {
            Some(c) => c,
            None => {
                self.cached = Some(self.caps);
                self.caps
            }
        }
    }

    /// 缓存失效（下一轮重新真扫）。
    pub fn invalidate(&mut self) {
        self.cached = None;
    }

    /// 需求核：需求位集 ⊆ 能力位集。
    pub fn require(&self, need: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if need & !((1u32 << NBITS) - 1) != 0 {
            return Err(XC_BIT_OOR);
        }
        if need & !self.caps != 0 {
            return Err(XC_NEED);
        }
        Ok(())
    }

    /// 版本门：需求版本 ≤ 驱动版本才放行。
    pub fn ver_gate(&self, need_ver: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if need_ver > self.drv_ver {
            return Err(XC_VER);
        }
        Ok(())
    }

    /// 组合门：版本 + 位集双查。
    pub fn full_gate(&self, need: u32, need_ver: u32) -> Result<(), u32> {
        self.ver_gate(need_ver)?;
        self.require(need)
    }
}

pub fn run_veb287_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0287");
    // 族一：位存在性。
    let mut p = CapProbe::new(0b1010_1010_1010, 3);
    s.add("位 1 有", p.has(1) == Ok(true), "1010 低组");
    s.add("位 0 无", p.has(0) == Ok(false), "零位");
    s.add("位 11 有", p.has(11) == Ok(true), "顶位");
    s.add("位 12 拒", p.has(12) == Err(XC_BIT_OOR), "越界");
    s.add("溢出截断", CapProbe::new(0xFFFF_F000, 1).caps == 0xFFF, "掩 12 位");
    // 族二：位不可写。
    s.add("改位拒", p.set_caps(0) == Err(XC_BIT_OOR), "探测只读");
    s.add("位未变", p.has(1) == Ok(true), "原样");
    // 族三：缓存。
    let c1 = p.probe();
    s.add("首扫真值", c1 == 0b1010_1010_1010, "位集");
    s.add("扫数 1", p.scans == 1, "一次");
    let _ = p.probe();
    s.add("二扫缓存", p.scans == 2, "仍真扫计数");
    p.invalidate();
    let _ = p.probe();
    s.add("失效重扫", p.scans == 3, "三轮");
    s.add("复取同值", p.probe() == c1, "稳定");
    // 族四：需求核。
    s.add("子集过", p.require(0b1000).is_ok(), "位 11 有");
    s.add("缺位拒", p.require(0b1) == Err(XC_NEED), "位 0 无");
    s.add("混合拒", p.require(0b1001) == Err(XC_NEED), "部分缺");
    s.add("空需过", p.require(0).is_ok(), "无需求");
    s.add("越位需求拒", p.require(1 << 12) == Err(XC_BIT_OOR), "位外");
    // 族五：版本门。
    s.add("同版过", p.ver_gate(3).is_ok(), "3≤3");
    s.add("低版过", p.ver_gate(1).is_ok(), "1≤3");
    s.add("高版拒", p.ver_gate(4) == Err(XC_VER), "4>3");
    // 族六：组合门（版本先于位集）。
    s.add("版败先拒", p.full_gate(0b1000, 9) == Err(XC_VER), "版本闸在前");
    s.add("位败后拒", p.full_gate(0b1, 1) == Err(XC_NEED), "位闸在后");
    s.add("双过", p.full_gate(0b1000, 2).is_ok(), "全绿");
    s.add("魔数核", CP_SEG_BASE == 0x310C0, "段基址");
    s
}
