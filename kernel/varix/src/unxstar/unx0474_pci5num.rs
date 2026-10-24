//! unx0474_pci5num —— UNX-F0474 PCI 面五数概括账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0474 · PCI 面五数概括 · 目标 210 行
//! 判据原文：UNX-F0474-J1 对 PCI 发现面做五数概括（复用
//!           0457 线性插值语义）：每设备摘要值 = vendor 低
//!           12 位 XOR class（模型面合成负载）；账二 概括
//!           自洽核：min ≤ Q1 ≤ median ≤ Q3 ≤ max；账三
//!           设备计数一致性：概括样本数 ≡ 发现清单长度
//!           （0458 对冲纪律消费）；账四 离群记名不剔除
//!           （Tukey 栅栏，复用 0457）；账五 空发现面 →
//!           None（无概括）。
//! 组件：设备摘要合成器 + 五数消费器 + 计数对冲核。
//! 职责定位：PCI 面五数是发现面的「人口普查摘要」——不关
//!   心每台设备是谁，只关心分布形态；离群的大摘要值往往是
//!   异常设备（桥后面挂着不该有的东西），记名报备给人工。
//! 性能逐项分解：合成 O(n)；概括 O(n log n)。
//! 诊断码段：0x346C0（0x346C1~0x346C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x346C0 起）。
pub const SEG_BASE: u32 = 0x346C0;
pub const XC_CNT_MISS: u32 = SEG_BASE + 1; // 计数对冲不齐

/// 设备清单条目（投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciDev {
    pub vendor: u16,
    pub class: u8,
}

/// 摘要合成：vendor 低 12 位 XOR class（零扩展）。
pub fn digest(d: &PciDev) -> u64 {
    ((d.vendor & 0x0FFF) as u64) ^ (d.class as u64)
}

/// 五数投影（复用 0457 的插值语义——此处重入轻量版：
/// 排序后取 [min, p25, p50, p75, max]）。
pub fn five(vals: &[u64]) -> Option<[u64; 5]> {
    if vals.is_empty() {
        return None;
    }
    let mut s = alloc::vec::Vec::new();
    for v in vals {
        s.push(*v);
    }
    s.sort_unstable();
    let n = s.len();
    let at = |i: usize| s[i.min(n - 1)];
    let q = |p: usize| -> u64 {
        // 位置 (n-1)*p/4，线性插值。
        let prod = (n - 1) * p;
        let lo = prod / 4;
        let rem = prod % 4;
        if rem == 0 || lo + 1 >= n {
            at(lo)
        } else {
            let a = at(lo) as u128;
            let b = at(lo + 1) as u128;
            (a + (b - a) * rem as u128 / 4) as u64
        }
    };
    Some([s[0], q(1), q(2), q(3), s[n - 1]])
}

/// 五数自洽核。
pub fn coherent(f: &[u64; 5]) -> Result<(), u32> {
    if !(f[0] <= f[1] && f[1] <= f[2] && f[2] <= f[3] && f[3] <= f[4]) {
        return Err(XC_CNT_MISS);
    }
    Ok(())
}

/// 计数对冲：概括样本数 ≡ 发现清单长度。
pub fn count_ok(vals: &[u64], devs: &[PciDev]) -> Result<(), u32> {
    if vals.len() != devs.len() {
        return Err(XC_CNT_MISS);
    }
    Ok(())
}

/// Tukey 记名（IQR 栅栏外索引）。
pub fn outliers(vals: &[u64]) -> alloc::vec::Vec<usize> {
    let f = match five(vals) {
        Some(f) => f,
        None => return alloc::vec::Vec::new(),
    };
    let iqr = f[3].saturating_sub(f[1]);
    let lo = f[1].saturating_sub(15 * iqr / 10);
    let hi = f[3].saturating_add(15 * iqr / 10);
    vals.iter().enumerate().filter(|(_, v)| **v < lo || **v > hi).map(|(i, _)| i).collect()
}

/// 判据入口：UNX-F0474 全族核。
pub fn run_unx0474_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0474");

    // 夹具：八设备。
    let devs: alloc::vec::Vec<PciDev> = alloc::vec![
        PciDev { vendor: 0x8086, class: 0x06 },
        PciDev { vendor: 0x10DE, class: 0x03 },
        PciDev { vendor: 0x1002, class: 0x03 },
        PciDev { vendor: 0x8086, class: 0x02 },
        PciDev { vendor: 0x10EC, class: 0x02 },
        PciDev { vendor: 0x1022, class: 0x06 },
        PciDev { vendor: 0x1B21, class: 0x01 },
        PciDev { vendor: 0x1234, class: 0x03 },
    ];

    // 族一：摘要合成。
    let vals: alloc::vec::Vec<u64> = devs.iter().map(digest).collect();
    cs.add("摘要八行", vals.len() == 8, "一比一");
    cs.add("摘要异或语义", digest(&PciDev { vendor: 0x1000, class: 0x00 }) == 0x1000, "低 12 位");
    cs.add("摘要类异或", digest(&PciDev { vendor: 0x0FF0, class: 0x0F }) == 0x0FFF, "XOR");

    // 族二：五数概括。
    let f = five(&vals).unwrap_or([0; 5]);
    cs.add("五数自洽", coherent(&f).is_ok(), "单调链");
    cs.add("两端锚定", f[0] == *vals.iter().min().unwrap_or(&0), "min");
    cs.add("max 锚定", f[4] == *vals.iter().max().unwrap_or(&0), "max");

    // 族三：计数对冲。
    cs.add("计数齐平", count_ok(&vals, &devs).is_ok(), "8=8");
    let short: alloc::vec::Vec<u64> = vals[..3].to_vec();
    cs.add("计数缺立案", count_ok(&short, &devs) == Err(XC_CNT_MISS), "3≠8");

    // 族四：离群记名。
    let mut noisy = vals.clone();
    noisy.push(0xFFFF); // 异常大摘要
    let out = outliers(&noisy);
    cs.add("离群记名", out == alloc::vec![8usize], "点行 8");
    cs.add("净面无离群", outliers(&vals).is_empty(), "正常分布");

    // 族五：空面退化。
    cs.add("空面 None", five(&[]).is_none(), "无概括");
    cs.add("空面离群空", outliers(&[]).is_empty(), "无记名");

    // 族六：单设备退化。
    let one = five(&[0x0123]).unwrap_or([0; 5]);
    cs.add("单元素五数等", one.iter().all(|v| *v == 0x0123), "全同源");
    cs.add("单元素自洽", coherent(&one).is_ok(), "退化");

    // 族七：偶数插值。
    let fe = five(&[10, 20, 30, 40]).unwrap_or([0; 5]);
    cs.add("Q2 插值", fe[2] == 25, "(20+30)/2");
    cs.add("Q1 插值", fe[1] == 17, "10+0.75*10");

    cs
}
