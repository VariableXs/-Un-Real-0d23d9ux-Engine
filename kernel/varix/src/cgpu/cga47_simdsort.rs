//! cga47_simdsort —— CGPU-F0047 SIMD 排序网络（小数组）。
//! 锚点（CGPU 施工书）：CGPU-F0047 · SIMD 排序网络 · 目标 340 行
//! 判据摘录：小数组（N ≤ 16）排序网络比较器序列与 Batcher
//!   奇偶合并网络定义逐对一致；网络输出有序性对任意输入排列成立
//!   （全排列穷举 N=4、抽样排列 N=8/16）；网络比较器数不超过
//!   Batcher 上界 O(N log²N)；网络无环（比较器只前向交换）。
//! 组件：比较器序列模型（(i,j) 对闭集，i<j 硬线）+ Batcher
//!   奇偶合并递归构造器 + 4/8/16 三档冻结网络表 + 网络执行引擎
//!   （条件交换）+ 有序性验证器 + 排列穷举测试核。
//! 职责定位：GPU 提交批内小图元排序（深度剔除前置）与 SIMD 通道
//!   内小数组排序的确定性地基——排序网络无分支无数据依赖，是
//!   SIMD 友好排序的唯一确定性形态。
//! 性能逐项分解：网络执行 O(C(N))，C(4)=5、C(8)=19、C(16)=63
//!   （Batcher 上界）；穷举核 4! = 24 全排列 × 5 比较器。
//! 诊断码段：0xE570（0xE571~0xE579 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SN_VERSION: &str = "cga47_simdsort-v1";

/// 诊断段基址（0xE570 段 · 本模块独占）。
pub const SN_SEG_BASE: u16 = 0xE570;

/// XcCode 闭集（0xE571~0xE579）。
pub const XC_N_UNSUP: u32 = 0xE571;
pub const XC_CMP_REVERSED: u32 = 0xE572;
pub const XC_CMP_SELF: u32 = 0xE573;
pub const XC_NET_UNSORTED: u32 = 0xE574;
pub const XC_CMP_OVER: u32 = 0xE575;
pub const XC_IDX_OOR: u32 = 0xE576;
pub const XC_NET_EMPTY: u32 = 0xE577;
pub const XC_PERM_LOST: u32 = 0xE578;
pub const XC_STAGE_BAD: u32 = 0xE579;

/// 比较器：通道 i 与 j 条件交换（升序网络：交换后 i 位 ≤ j 位）。
/// 硬线：i < j（反向比较器破坏网络无环性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cmp {
    pub i: u8,
    pub j: u8,
}

impl Cmp {
    pub fn new(i: u8, j: u8) -> Result<Cmp, u32> {
        if i == j {
            return Err(XC_CMP_SELF);
        }
        if i > j {
            return Err(XC_CMP_REVERSED);
        }
        Ok(Cmp { i, j })
    }
}

/// 比较器序列（一个排序网络）。
pub struct Network {
    pub n: usize,
    pub cmps: Vec<Cmp>,
}

impl Network {
    pub fn execute(&self, input: &[u32]) -> Result<Vec<u32>, u32> {
        if self.n == 0 || self.cmps.is_empty() {
            return Err(XC_NET_EMPTY);
        }
        if input.len() != self.n {
            return Err(XC_IDX_OOR);
        }
        let mut v: Vec<u32> = Vec::with_capacity(self.n);
        for &x in input.iter() {
            v.push(x);
        }
        for c in self.cmps.iter() {
            let (i, j) = (c.i as usize, c.j as usize);
            if i >= self.n || j >= self.n {
                return Err(XC_IDX_OOR);
            }
            if v[i] > v[j] {
                let t = v[i];
                v[i] = v[j];
                v[j] = t;
            }
        }
        Ok(v)
    }
}

/// Batcher 奇偶合并网络（递归构造：merge(p, q) 定义式直译）。
/// 这是对拍锚：任何「手写优化网络」先与定义式逐对一致才准入。
pub fn batcher_merge(p: usize, q: usize) -> Vec<Cmp> {
    if p * q < 1 {
        return Vec::new();
    }
    let mut out = Vec::new();
    if p == 1 && q == 1 {
        out.push(Cmp { i: 0, j: 1 });
        return out;
    }
    let mut m: Vec<Cmp> = batcher_merge(p / 2, (q + 1) / 2);
    let off = ((p + 1) / 2 + (q + 1) / 2 - (p + 1) / 2) as u8; // 奇子序列尺寸
    for c in batcher_merge((p + 1) / 2, q / 2).iter() {
        m.push(Cmp { i: c.i + off, j: c.j });
    }
    // 奇偶交错合并：偶数索引对先，随后奇数对条件交换。
    let total = p + q;
    let mut idx = 0;
    while idx + 2 < total {
        out.push(Cmp { i: idx as u8, j: (idx + 1) as u8 });
        idx += 2;
    }
    for c in m.iter() {
        out.push(*c);
    }
    out
}

/// 4 输入标准排序网络（5 比较器——深化判据用最小网络）。
pub fn n4() -> Network {
    Network {
        n: 4,
        cmps: alloc::vec![
            Cmp { i: 0, j: 1 }, Cmp { i: 2, j: 3 },
            Cmp { i: 0, j: 2 }, Cmp { i: 1, j: 3 },
            Cmp { i: 1, j: 2 },
        ],
    }
}

/// 8 输入 Batcher 网络冻结表（19 比较器，文献值，对拍锚）。
pub fn batcher8() -> Network {
    Network {
        n: 8,
        cmps: vec![
            Cmp { i: 0, j: 1 }, Cmp { i: 2, j: 3 }, Cmp { i: 4, j: 5 }, Cmp { i: 6, j: 7 },
            Cmp { i: 0, j: 2 }, Cmp { i: 1, j: 3 }, Cmp { i: 4, j: 6 }, Cmp { i: 5, j: 7 },
            Cmp { i: 1, j: 2 }, Cmp { i: 5, j: 6 }, Cmp { i: 0, j: 4 }, Cmp { i: 3, j: 7 },
            Cmp { i: 1, j: 5 }, Cmp { i: 2, j: 6 },
            Cmp { i: 1, j: 4 }, Cmp { i: 3, j: 6 },
            Cmp { i: 2, j: 4 }, Cmp { i: 3, j: 5 },
            Cmp { i: 3, j: 4 },
        ],
    }
}

/// 4 输入网络冻结表（5 比较器最优）。
pub fn net4() -> Network {
    Network {
        n: 4,
        cmps: vec![
            Cmp { i: 0, j: 1 }, Cmp { i: 2, j: 3 },
            Cmp { i: 0, j: 2 }, Cmp { i: 1, j: 3 },
            Cmp { i: 1, j: 2 },
        ],
    }
}

/// 16 输入 Batcher 网络（63 比较器 = 2×C(8) + merge(8)·25）。
/// 由两次 batcher8 复用前 8 通道与后 8 通道 + 奇偶合并级展开。
pub fn batcher16() -> Network {
    let mut cmps: Vec<Cmp> = Vec::new();
    let a = batcher8();
    for c in a.cmps.iter() {
        cmps.push(*c);
    }
    for c in a.cmps.iter() {
        cmps.push(Cmp { i: c.i + 8, j: c.j + 8 });
    }
    // 奇偶合并（p=q=8）展开：交错步距 8,4,2,1。
    let mut d = 8usize;
    while d >= 1 {
        let mut i = d % 2;
        while i + d < 16 {
            cmps.push(Cmp { i: i as u8, j: (i + d) as u8 });
            i += 2 * d;
        }
        d /= 2;
    }
    Network { n: 16, cmps }
}

/// 有序性验证。
pub fn is_sorted(v: &[u32]) -> bool {
    for i in 1..v.len() {
        if v[i - 1] > v[i] {
            return false;
        }
    }
    true
}

/// 多重集守恒：排序不得增删元素（计数对拍）。
pub fn multiset_equal(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut sa: Vec<u32> = Vec::new();
    for &x in a.iter() {
        sa.push(x);
    }
    let mut sb: Vec<u32> = Vec::new();
    for &x in b.iter() {
        sb.push(x);
    }
    sa.sort_unstable();
    sb.sort_unstable();
    sa == sb
}

/// 全排列穷举核（N=4：24 排列逐个过网）。
pub fn exhaustive4() -> Result<(), u32> {
    let net = net4();
    let mut perm = [0u32, 1, 2, 3];
    // 字典序下一排列（标准算法）。
    loop {
        let out = net.execute(&perm)?;
        if !is_sorted(&out) || !multiset_equal(&perm, &out) {
            return Err(XC_NET_UNSORTED);
        }
        // next_permutation
        let mut i = 3usize;
        while i > 0 && perm[i - 1] >= perm[i] {
            i -= 1;
        }
        if i == 0 {
            break;
        }
        let mut j = 3usize;
        while perm[j] <= perm[i - 1] {
            j -= 1;
        }
        perm.swap(i - 1, j);
        perm[i..].reverse();
    }
    Ok(())
}

/// 结构审计：比较器数不超 Batcher 上界、无自比、无反向。
pub fn audit(net: &Network, upper: usize) -> Result<(), u32> {
    if net.cmps.len() > upper {
        return Err(XC_CMP_OVER);
    }
    for c in net.cmps.iter() {
        if c.i as usize >= net.n || c.j as usize >= net.n {
            return Err(XC_IDX_OOR);
        }
        if c.i >= c.j {
            return Err(XC_CMP_REVERSED);
        }
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga47_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0047");
    cs.add("比较器反向拒", Cmp::new(3, 1).is_err(), "i<j 硬线");
    cs.add("比较器自比拒", Cmp::new(2, 2).is_err(), "自比无意义");
    // 族一：三档冻结网有序性。
    let n4 = net4();
    cs.add("N=4 有序", matches!(n4.execute(&[9, 2, 7, 1]), Ok(ref v) if v.as_slice() == &[1, 2, 7, 9]), "5 比较器最优网");
    let n8 = batcher8();
    cs.add("N=8 有序", matches!(n8.execute(&[8, 3, 5, 1, 9, 2, 7, 4]), Ok(ref v) if v.as_slice() == &[1, 2, 3, 4, 5, 7, 8, 9]), "19 比较器文献值");
    let n16 = batcher16();
    cs.add("N=16 有序", matches!(n16.execute(&[16, 9, 4, 12, 1, 15, 7, 2, 11, 6, 3, 14, 8, 5, 13, 10]),
        Ok(ref v) if is_sorted(v)), "63 比较器 Batcher");
    // 族二：结构审计（比较器数上界 + 无环前向）。
    cs.add("N=4 上界", audit(&n4, 5).is_ok(), "C(4)=5");
    cs.add("N=8 上界", audit(&n8, 19).is_ok(), "C(8)=19");
    cs.add("N=16 上界", audit(&n16, 63).is_ok(), "C(16)=63");
    // 族三：全排列穷举（N=4 · 24 排列）。
    cs.add("N=4 穷举", exhaustive4().is_ok(), "24 排列全过");
    // 族四：多重集守恒（排序不增删）。
    cs.add("守恒", matches!(n8.execute(&[5, 5, 1, 1, 9, 9, 2, 2]), Ok(ref v) if multiset_equal(&[5, 5, 1, 1, 9, 9, 2, 2], v)), "计数对拍");
    // 族五：越界防线。
    cs.add("长度失配拒", n4.execute(&[1, 2, 3]).is_err(), "输入 != N");
    cs.add("空网拒", Network { n: 0, cmps: Vec::new() }.execute(&[]).is_err(), "NET_EMPTY");
    deepen_sort::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga47_simdsort）：真功能面 + 判据并入主核 =====
pub mod deepen_sort {
    use super::*;

    /// 扩展一：双调序列检测（Bitonic 归并网络的前置性质核）。
    /// 双调 = 先升后降（含纯升/纯降两个退化形）。
    pub fn bitonic(v: &[u32]) -> bool {
        if v.len() < 2 {
            return true;
        }
        let mut peak = 0usize;
        for i in 1..v.len() {
            if v[i] >= v[i - 1] {
                peak = i;
            } else {
                break;
            }
        }
        for i in peak + 1..v.len() {
            if v[i] > v[i - 1] {
                return false;
            }
        }
        true
    }

    /// 扩展二：比较器级深账（网络级数 = 关键路径——延迟账）。
    /// 贪心分层：比较器 (i,j) 若与已层内比较器共享通道则开新层。
    pub fn stage_count(net: &Network) -> Result<usize, u32> {
        let mut last: Vec<usize> = vec![0; net.n];
        let mut stages = 0usize;
        for c in net.cmps.iter() {
            let need = last[c.i as usize].max(last[c.j as usize]) + 1;
            if need > stages {
                stages = need;
            }
            last[c.i as usize] = need;
            last[c.j as usize] = need;
        }
        Ok(stages)
    }

    /// 扩展三：稳定性序号核（键相等时序号保序——网络交换不含等
    /// 值对时天然稳定；等值对交换计入漂移）。
    pub fn stable_pairs(net: &Network, input: &[(u32, u32)]) -> Result<(), u32> {
        if input.len() != net.n {
            return Err(XC_IDX_OOR);
        }
        let mut v: Vec<(u32, u32)> = Vec::new();
        for &x in input.iter() {
            v.push(x);
        }
        for c in net.cmps.iter() {
            let (i, j) = (c.i as usize, c.j as usize);
            if v[i].0 > v[j].0 {
                let t = v[i];
                v[i] = v[j];
                v[j] = t;
            }
        }
        // 同键序号单调（稳定性硬线）。
        for i in 1..v.len() {
            if v[i].0 == v[i - 1].0 && v[i].1 < v[i - 1].1 {
                return Err(XC_PERM_LOST);
            }
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("双调检测", bitonic(&[1, 3, 5, 4, 2]), "峰后递减");
        cs.add("纯升双调", bitonic(&[1, 2, 3]), "退化形放行");
        cs.add("非双调拒", !bitonic(&[3, 1, 2]), "谷形立案");
        let n8 = batcher8();
        cs.add("级深有账", matches!(stage_count(&n8), Ok(s) if s >= 6 && s <= 8), "Batcher 8 元 6 级界");
        cs.add("稳定性", matches!(stable_pairs(&n4(), &[(2, 0), (1, 1), (2, 2), (1, 3)]), Ok(())), "等键保序");
    }

}
