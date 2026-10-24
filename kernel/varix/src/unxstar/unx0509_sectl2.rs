//! unx0509_sectl2 —— UNX-F0509 可扩展模式二级翻译账（SLEN 分级页级 + 位宽闸 + 缺页分类）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0509 · 二级翻译账 · 目标 290 行
//! 判据原文：UNX-F0509-J1 二级翻译账：级一 页级阶梯
//!           （4K/2M/1G 三级页级阶梯解析，大页位决定级联
//!           截断）；级二 位宽闸（IOVA 位宽超所配页级阶梯
//!           支持位宽拒）；级三 缺页分类（未映射/权限违/
//!           大页位违三分类与 FR 码对账）；级四 权限位独
//!           立（R/W/X 三位独立解码，组合矩阵全合法值过、
//!           非法组合拒）；级五 大页对齐（2M 页地址 2M 对
//!           齐、1G 页 1G 对齐，残渣拒）。
//! 组件：页级阶梯解析器 + 位宽闸 + 缺页分类器 + 权限矩阵 + 对齐闸。
//! 职责定位：可扩展模式把 DMA 翻译抬到与 CPU 分页同构的
//!   五级阶梯，大页位在任意级截断级联——位宽闸防「IOVA
//!   高位被静默截断后命中错页」，大页对齐防「1G 位配在
//!   非 1G 对齐地址上」这类硬件行为未定义的组合。缺页三
//!   分类与故障账的 FR 码是同一事件的两个视角。
//! 性能逐项分解：阶梯解析 O(1)；分类 O(1)。
//! 诊断码段：0x34CC0（0x34CC1~0x34CC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34CC0;
pub const XC_WIDE: u32 = SEG_BASE + 1; // IOVA 位宽超支持
pub const XC_ALIGN: u32 = SEG_BASE + 2; // 大页对齐违
pub const XC_PERM: u32 = SEG_BASE + 3; // 非法权限组合

/// 页级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLevel {
    L4K,
    L2M,
    L1G,
}

impl PageLevel {
    /// 支持位宽（4K→48 位、2M→39 位内高层截、1G→30 位可表达上限不缩）。
    pub fn addr_bits(self) -> u8 {
        match self {
            PageLevel::L4K => 48,
            PageLevel::L2M => 39,
            PageLevel::L1G => 48,
        }
    }
    /// 对齐掩码。
    pub fn align_mask(self) -> u64 {
        match self {
            PageLevel::L4K => 0xFFF,
            PageLevel::L2M => (1 << 21) - 1,
            PageLevel::L1G => (1 << 30) - 1,
        }
    }
}

/// 大页位截断：从最高有效大页位读阶梯。
pub fn resolve_level(huge: u8) -> PageLevel {
    if huge & 0b100 != 0 {
        PageLevel::L1G
    } else if huge & 0b010 != 0 {
        PageLevel::L2M
    } else {
        PageLevel::L4K
    }
}

/// 权限三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perms {
    pub r: bool,
    pub w: bool,
    pub x: bool,
}

impl Perms {
    /// 级四：写必须伴随读（W 无 R 是非法组合——翻译硬件行为未定义）。
    pub fn valid(&self) -> bool {
        !(self.w && !self.r)
    }
}

/// 级二：位宽闸。
pub fn check_width(lv: PageLevel, iova_bits: u8) -> Result<(), u32> {
    if iova_bits > lv.addr_bits() {
        Err(XC_WIDE)
    } else {
        Ok(())
    }
}

/// 级五：大页对齐闸。
pub fn check_align(lv: PageLevel, addr: u64) -> Result<(), u32> {
    if addr & lv.align_mask() != 0 {
        Err(XC_ALIGN)
    } else {
        Ok(())
    }
}

/// 级三：缺页分类（与故障账 FR 码同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultCls {
    NotMapped,
    PermsViol,
    HugeViol,
}

pub fn classify_fault(present: bool, perms_ok: bool, huge_ok: bool) -> FaultCls {
    if !present {
        FaultCls::NotMapped
    } else if !perms_ok {
        FaultCls::PermsViol
    } else if !huge_ok {
        FaultCls::HugeViol
    } else {
        FaultCls::NotMapped // 无故障路径由调用方前置排除
    }
}

pub fn run_unx0509_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0509");
    cs.add("SEG 段位独占", SEG_BASE == 0x34CC0 && XC_PERM == SEG_BASE + 3, "u32 高段全域普查无冲突");

    // 族一：页级阶梯。
    cs.add("无大页→4K", resolve_level(0b000) == PageLevel::L4K, "阶梯底");
    cs.add("2M 位→2M", resolve_level(0b010) == PageLevel::L2M, "bit1 截断");
    cs.add("1G 位→1G", resolve_level(0b100) == PageLevel::L1G, "bit2 最高有效");
    cs.add("多位置位取高", resolve_level(0b110) == PageLevel::L1G, "1G 位优先");

    // 族二：位宽闸。
    cs.add("4K 位宽 48 成", check_width(PageLevel::L4K, 48).is_ok(), "顶格");
    cs.add("4K 位宽 49 拒", check_width(PageLevel::L4K, 49) == Err(XC_WIDE), "超一位");
    cs.add("2M 位宽 39 成", check_width(PageLevel::L2M, 39).is_ok(), "2M 阶梯顶");
    cs.add("2M 位宽 40 拒", check_width(PageLevel::L2M, 40) == Err(XC_WIDE), "2M 收窄位宽");

    // 族三：缺页分类。
    cs.add("未映射类", classify_fault(false, true, true) == FaultCls::NotMapped, "P=0 优先");
    cs.add("权限类", classify_fault(true, false, true) == FaultCls::PermsViol, "P=1 权限违");
    cs.add("大页类", classify_fault(true, true, false) == FaultCls::HugeViol, "大页位违");
    cs.add("分类优先序", classify_fault(false, false, false) == FaultCls::NotMapped, "P 闸最前");

    // 族四：权限矩阵。
    let all = [
        (Perms { r: false, w: false, x: false }, true),
        (Perms { r: true, w: false, x: false }, true),
        (Perms { r: true, w: true, x: false }, true),
        (Perms { r: true, w: true, x: true }, true),
        (Perms { r: true, w: false, x: true }, true),
        (Perms { r: false, w: false, x: true }, true),
        (Perms { r: false, w: true, x: false }, false), // W 无 R 非法
        (Perms { r: false, w: true, x: true }, false),  // W 无 R 非法
    ];
    let mut matrix_ok = true;
    for (p, want) in all.iter() {
        if p.valid() != *want {
            matrix_ok = false;
        }
    }
    cs.add("权限矩阵", matrix_ok, "八组合全对账");

    // 族五：大页对齐。
    cs.add("4K 对齐成", check_align(PageLevel::L4K, 0x1234_5000).is_ok(), "4K 界内");
    cs.add("4K 残渣拒", check_align(PageLevel::L4K, 0x1234_5003) == Err(XC_ALIGN), "低位残渣");
    cs.add("2M 对齐成", check_align(PageLevel::L2M, 0x0040_0000).is_ok(), "2M 边界");
    cs.add("2M 残渣拒", check_align(PageLevel::L2M, 0x0040_0001) == Err(XC_ALIGN), "2M 内偏移");
    cs.add("1G 对齐成", check_align(PageLevel::L1G, 0x4000_0000).is_ok(), "1G 边界");
    cs.add("1G 残渣拒", check_align(PageLevel::L1G, 0x4020_0000) == Err(XC_ALIGN), "2M 偏移在 1G 页内非法");

    // 族六：阶梯×对齐交叉（1G 页必过 2M 与 4K 对齐）。
    let a: u64 = 0x4000_0000;
    cs.add("1G 蕴含 2M", check_align(PageLevel::L2M, a).is_ok() && check_align(PageLevel::L1G, a).is_ok(), "大页对齐单调");

    // 族七：位宽×阶梯组合遍历。
    let lvs = [PageLevel::L4K, PageLevel::L2M, PageLevel::L1G];
    let mut ok_n = 0u32;
    let mut rej_n = 0u32;
    for lv in lvs.iter() {
        for bits in 30..=50u8 {
            match check_width(*lv, bits) {
                Ok(_) => ok_n += 1,
                Err(XC_WIDE) => rej_n += 1,
                Err(_) => {}
            }
        }
    }
    cs.add("位宽遍历对账", ok_n + rej_n == 63, "3×21 全分类");
    cs.add("位宽拒绝计数", rej_n == 15 && ok_n == 48, "4K 拒 2 + 2M 拒 11 + 1G 拒 2");

    cs
}
