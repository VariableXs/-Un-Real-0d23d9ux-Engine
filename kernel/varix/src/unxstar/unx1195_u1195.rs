// unx1195_u1195 —— UNX-F1195 亲和面故障注入资产（路由损坏/目标掉线必抓）
// 判据原文 UNX-F1195-J1：故障注入全过（路由表损坏/目标核掉线两类逐例注入
// 必抓、检出率 100%、路由回退登记、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8ef0 独占（0x8ef0~0x8f2f）。
//
// 设计（纯功能 no_std）：
//   * 故障闭集两类：RouteCorrupt（路由表项损坏：越核号）/
//     TargetOffline（目标核掉线：路由到离线核）；
//   * 注入器：构造损坏态 → 检出器判定 → 逐例案卷（类型/证据/回退动作）；
//   * 回退动作闭集：1=回退默认核 2=隔离向量 3=拒绝路由；
//   * 守恒：注入 = 检出 + 漏网（漏网必 0）；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8ef0 独占） ─────────────────────────────────────────────
pub const XC_AF_SEALED: u32 = 0x8EF1; // 封账后注入
pub const XC_AF_BAD_CORE: u32 = 0x8EF2; // 越核号构造非法
pub const XC_AF_UNCAUGHT: u32 = 0x8EF3; // 故障漏网
pub const XC_AF_BAD_FALLBACK: u32 = 0x8EF4; // 回退动作越闭集
pub const XC_AF_FULL: u32 = 0x8EF5; // 案卷容量满

/// 故障类型。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AffFault {
    RouteCorrupt,
    TargetOffline,
}

/// 回退动作。
pub const FB_DEFAULT_CORE: u8 = 1;
pub const FB_ISOLATE_VEC: u8 = 2;
pub const FB_REJECT: u8 = 3;

/// 案卷行。
#[derive(Clone, Copy)]
pub struct AffFaultRow {
    pub kind: AffFault,
    /// 证据核号（损坏值/掉线核）。
    pub evid_core: u8,
    /// 在线核表快照大小。
    pub online_n: u8,
    pub fallback: u8,
    pub caught: bool,
}

/// 故障注入器。
pub struct AffFaultInjector {
    cap: usize,
    online: Vec<u8>,
    rows: Vec<AffFaultRow>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl AffFaultInjector {
    pub fn new(online: &[u8]) -> AffFaultInjector {
        AffFaultInjector {
            cap: 64,
            online: online.to_vec(),
            rows: vec![],
            sealed: false,
            rejects: vec![],
        }
    }

    fn online_ok(&self, c: u8) -> bool {
        self.online.contains(&c)
    }

    /// 注入路由损坏（核号越出在线表即损坏 → 必抓）。
    pub fn inject_corrupt(&mut self, bad_core: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AF_SEALED, "封账后注入"));
            return Err(XC_AF_SEALED);
        }
        if self.online_ok(bad_core) {
            self.rejects.push((XC_AF_BAD_CORE, "核号在线表内不构成损坏"));
            return Err(XC_AF_BAD_CORE);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_AF_FULL, "案卷容量满"));
            return Err(XC_AF_FULL);
        }
        let caught = !self.online_ok(bad_core);
        self.rows.push(AffFaultRow {
            kind: AffFault::RouteCorrupt,
            evid_core: bad_core,
            online_n: self.online.len() as u8,
            fallback: FB_DEFAULT_CORE,
            caught,
        });
        if !caught {
            self.rejects.push((XC_AF_UNCAUGHT, "故障漏网"));
            return Err(XC_AF_UNCAUGHT);
        }
        Ok(())
    }

    /// 注入目标掉线（核不在在线表 → 必抓 + 回退）。
    pub fn inject_offline(&mut self, dead_core: u8, fallback: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AF_SEALED, "封账后注入"));
            return Err(XC_AF_SEALED);
        }
        if fallback < FB_DEFAULT_CORE || fallback > FB_REJECT {
            self.rejects.push((XC_AF_BAD_FALLBACK, "回退动作越闭集"));
            return Err(XC_AF_BAD_FALLBACK);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_AF_FULL, "案卷容量满"));
            return Err(XC_AF_FULL);
        }
        let caught = !self.online_ok(dead_core);
        self.rows.push(AffFaultRow {
            kind: AffFault::TargetOffline,
            evid_core: dead_core,
            online_n: self.online.len() as u8,
            fallback,
            caught,
        });
        if !caught {
            self.rejects.push((XC_AF_UNCAUGHT, "故障漏网"));
            return Err(XC_AF_UNCAUGHT);
        }
        Ok(())
    }

    /// 检出率 100% 判定。
    pub fn all_caught(&self) -> bool {
        self.rows.iter().all(|r| r.caught)
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.rows.len() == self.rows.iter().filter(|r| r.caught).count()
    }

    pub fn count_kind(&self, k: AffFault) -> usize {
        self.rows.iter().filter(|r| r.kind == k).count()
    }
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_AF_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            let k: u64 = match r.kind {
                AffFault::RouteCorrupt => 0,
                AffFault::TargetOffline => 1,
            };
            h ^= k.rotate_left(3)
                ^ (r.evid_core as u64).rotate_left(17)
                ^ (r.fallback as u64).rotate_left(31)
                ^ (r.caught as u64).rotate_left(41);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准注入场景：损坏 3 例 + 掉线 3 例（三回退各一）全抓。
pub fn aff_fault_scenario() -> Result<(usize, u64), u32> {
    let mut f = AffFaultInjector::new(&[0, 1, 2, 3]);
    f.inject_corrupt(8)?;
    f.inject_corrupt(9)?;
    f.inject_corrupt(10)?;
    f.inject_offline(4, FB_DEFAULT_CORE)?;
    f.inject_offline(5, FB_ISOLATE_VEC)?;
    f.inject_offline(6, FB_REJECT)?;
    if !f.all_caught() || !f.conserved() {
        return Err(XC_AF_UNCAUGHT);
    }
    let fp = f.seal()?;
    Ok((f.rows_len(), fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）在线核不构成损坏必拒
    {
        let mut f = AffFaultInjector::new(&[0, 1]);
        if f.inject_corrupt(1) == Err(XC_AF_BAD_CORE) {
            caught += 1;
        }
    }
    // 2）损坏检出（越表核）
    {
        let mut f = AffFaultInjector::new(&[0, 1]);
        let _ = f.inject_corrupt(9);
        if f.all_caught() && f.count_kind(AffFault::RouteCorrupt) == 1 {
            caught += 1;
        }
    }
    // 3）回退动作越闭集必拒
    {
        let mut f = AffFaultInjector::new(&[0, 1]);
        if f.inject_offline(5, 9) == Err(XC_AF_BAD_FALLBACK) {
            caught += 1;
        }
    }
    // 4）掉线检出（离线核）
    {
        let mut f = AffFaultInjector::new(&[0, 1]);
        let _ = f.inject_offline(7, FB_REJECT);
        if f.count_kind(AffFault::TargetOffline) == 1 && f.all_caught() {
            caught += 1;
        }
    }
    // 5）封账后注入必拒
    {
        let mut f = AffFaultInjector::new(&[0, 1]);
        let _ = f.inject_corrupt(9);
        let _ = f.seal();
        if f.inject_corrupt(10) == Err(XC_AF_SEALED) {
            caught += 1;
        }
    }
    // 6）案卷容量满必拒
    {
        let mut f = AffFaultInjector::new(&[0]);
        // cap 64 → 注入 64 例后满
        for k in 20..84u8 {
            let _ = f.inject_corrupt(k);
        }
        if f.inject_corrupt(200) == Err(XC_AF_FULL) {
            caught += 1;
        }
    }
    (caught, N)
}
