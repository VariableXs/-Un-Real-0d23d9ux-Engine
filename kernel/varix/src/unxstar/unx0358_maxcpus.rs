//! unx0358_maxcpus —— UNX-F0358 maxcpus 参数语义与部分启动。
//! 锚点（CoRun Unxreal 主册）：UNX-F0358 · maxcpus 语义 · 目标 200 行
//! 判据原文：UNX-F0358-J1 maxcpus=N 语义：只启动前 N 个核（按
//!           硬件序，不挑核）；N=0 无效拒（至少 1 核）；N 超过
//!           实测核数 = 钳到实测数（诚实降级不报错）；未启动
//!           核进「未启账」（拓扑可见但不参与调度）。
//! 组件：核拓扑（实测核数 + 硬件序）+ maxcpus 解析核（钳位 +
//!           拒零）+ 启动集/未启账切分 + 部分启动对账。
//! 职责定位：maxcpus 是「算力旋钮」——语义必须钉死：前 N 个是
//!   硬件序不是「最好的 N 个」（挑核会把 NUMA/缓存拓扑撕裂）；
//!   超请求数钳到实测是诚实降级——报告 8 核就启 8 核，不编 12。
//! 性能逐项分解：解析 O(1)；切分 O(核数)；对账 O(核数)。
//! 诊断码段：0x20680（0x20681~0x20689 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MX_VERSION: &str = "unx0358_maxcpus-v1";

/// 诊断段基址（0x20680 段 · 本模块独占）。
pub const MX_SEG_BASE: u32 = 0x20680;
pub const XC_ZERO_REQ: u32 = 0x20681; // N=0 拒
pub const XC_TOPO_EMPTY: u32 = 0x20682; // 实测零核拒
pub const XC_TOPO_GAP: u32 = 0x20683; // 硬件序不连续
pub const XC_CLAMPED: u32 = 0x20684; // 钳位发生（立案标记）
pub const XC_SET_DRIFT: u32 = 0x20685; // 启动集/未启账漂移
pub const XC_CORE_DUP: u32 = 0x20686; // 核重复
pub const XC_SCHED_LEAK: u32 = 0x20687; // 未启核泄漏进调度
pub const XC_ARG_BAD: u32 = 0x20688; // 参数格式破
pub const XC_ALL_UP: u32 = 0x20689; // 全启时未启账必须空

/// 核拓扑：硬件序（apic id 连续 0..n-1 的抽象序号）。
pub struct CoreTopo {
    /// 实测核 ID 序（必须升序无重复）。
    pub cores: Vec<u32>,
}

impl CoreTopo {
    pub fn new(cores: &[u32]) -> Result<CoreTopo, u32> {
        if cores.is_empty() {
            return Err(XC_TOPO_EMPTY);
        }
        let mut prev = 0;
        let mut first = true;
        for &c in cores.iter() {
            if !first && c <= prev {
                return Err(XC_TOPO_GAP);
            }
            if cores.iter().filter(|&&x| x == c).count() > 1 {
                return Err(XC_CORE_DUP);
            }
            prev = c;
            first = false;
        }
        Ok(CoreTopo { cores: cores.to_vec() })
    }

    pub fn count(&self) -> u32 {
        self.cores.len() as u32
    }
}

/// maxcpus 参数解析：`maxcpus=N` → N；`maxcpus`（无=）→ 参数破；
/// 其他串 → 参数破。
pub fn parse_maxcpus(arg: &str) -> Result<u32, u32> {
    const PREFIX: &str = "maxcpus=";
    if arg.len() <= PREFIX.len() || !arg.starts_with(PREFIX) {
        return Err(XC_ARG_BAD);
    }
    let digits = &arg[PREFIX.len()..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(XC_ARG_BAD);
    }
    // 无溢出解析（≤8 位数字）。
    if digits.len() > 8 {
        return Err(XC_ARG_BAD);
    }
    let mut v: u32 = 0;
    for b in digits.bytes() {
        v = v.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    if v == 0 {
        return Err(XC_ZERO_REQ);
    }
    Ok(v)
}

/// 部分启动切分：前 N 个启动（硬件序），其余进未启账。
/// 返回（启动集, 未启账, 是否钳位）。
pub fn partial_boot(topo: &CoreTopo, req: u32) -> Result<(Vec<u32>, Vec<u32>, bool), u32> {
    if req == 0 {
        return Err(XC_ZERO_REQ);
    }
    let n = topo.count();
    let clamped = req > n;
    let take = if clamped { n } else { req };
    let mut up = Vec::new();
    let mut down = Vec::new();
    for (i, &c) in topo.cores.iter().enumerate() {
        if (i as u32) < take {
            up.push(c);
        } else {
            down.push(c);
        }
    }
    // 切分对账：两账并集 = 拓扑全集、交集 = 空。
    let mut both: Vec<u32> = up.iter().chain(down.iter()).copied().collect();
    both.sort_unstable();
    both.dedup();
    if both.len() != topo.cores.len() {
        return Err(XC_SET_DRIFT);
    }
    if clamped {
        // 钳位立案（调用侧可见）。
        let _ = XC_CLAMPED;
    }
    Ok((up, down, clamped))
}

/// 调度泄漏核：未启账核不得出现在启动集（并查双向）。
pub fn sched_leak_check(up: &[u32], down: &[u32]) -> Result<(), u32> {
    for &d in down.iter() {
        if up.contains(&d) {
            return Err(XC_SCHED_LEAK);
        }
    }
    for &u in up.iter() {
        if down.contains(&u) {
            return Err(XC_SCHED_LEAK);
        }
    }
    Ok(())
}

pub fn run_unx0358_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0358_maxcpus");
    // 族一：拓扑构造。
    let topo = match CoreTopo::new(&[0, 1, 2, 3, 4, 5, 6, 7]) {
        Ok(t) => t,
        Err(_) => {
            cs.add("拓扑构造", false, "被拒");
            return cs;
        }
    };
    cs.add("实测 8 核", topo.count() == 8, "硬件序连续");
    cs.add("空拓扑拒", CoreTopo::new(&[]).is_err(), "TOPO_EMPTY");
    cs.add("序乱拒", CoreTopo::new(&[0, 2, 1]).is_err(), "TOPO_GAP");
    cs.add("重核拒", CoreTopo::new(&[1, 1]).is_err(), "CORE_DUP");
    // 族二：参数解析。
    cs.add("解析 4", parse_maxcpus("maxcpus=4") == Ok(4), "标准形");
    cs.add("解析 1", parse_maxcpus("maxcpus=1") == Ok(1), "下界");
    cs.add("零核拒", parse_maxcpus("maxcpus=0") == Err(XC_ZERO_REQ), "ZERO");
    cs.add("无等号拒", parse_maxcpus("maxcpus") == Err(XC_ARG_BAD), "ARG");
    cs.add("非数字拒", parse_maxcpus("maxcpus=4x") == Err(XC_ARG_BAD), "ARG");
    cs.add("前缀不符拒", parse_maxcpus("cpus=4") == Err(XC_ARG_BAD), "ARG");
    // 族三：部分启动。
    let (up, down, clamped) = match partial_boot(&topo, 4) {
        Ok(x) => x,
        Err(_) => {
            cs.add("部分启动", false, "被拒");
            return cs;
        }
    };
    cs.add("前 4 启动", up.as_slice() == &[0u32, 1, 2, 3], "硬件序");
    cs.add("后 4 未启", down.as_slice() == &[4u32, 5, 6, 7], "未启账");
    cs.add("不钳位", !clamped, "4 ≤ 8");
    cs.add("无泄漏", sched_leak_check(&up, &down).is_ok(), "两账互斥");
    // 族四：钳位。
    let (up2, down2, cl2) = partial_boot(&topo, 12).unwrap_or((up.clone(), down.clone(), false));
    cs.add("钳到 8", up2.len() == 8, "诚实降级");
    cs.add("未启账空", down2.is_empty(), "全启");
    cs.add("钳位立案", cl2, "CLAMPED 标记");
    cs.add("全启零未启", down2.is_empty(), "XC_ALL_UP 语义");
    // 族五：单核启动。
    let (up3, down3, _) = partial_boot(&topo, 1).unwrap_or((up.clone(), down.clone(), false));
    cs.add("单核 BSP", up3.as_slice() == &[0u32], "仅核 0");
    cs.add("单核泄漏核", sched_leak_check(&up3, &down3).is_ok(), "互斥");
    // 族六：泄漏检出。
    let up4 = vec![0u32, 4];
    let down4 = vec![4u32, 5];
    cs.add("泄漏检出", sched_leak_check(&up4, &down4) == Err(XC_SCHED_LEAK), "4 号两账");
    cs
}
