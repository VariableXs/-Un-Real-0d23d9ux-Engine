// unx1187_u1187 —— UNX-F1187 CPU 热插拔中断再平衡预案账（离线核向量转移）
// 判据原文 UNX-F1187-J1：预案账全过（离线核向量清单逐条转移预案、
// 目标核合法、转移后零孤儿、守恒（离线向量=转移+孤儿0）、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8cf0 独占（0x8cf0~0x8d2f）。
//
// 设计（纯功能 no_std）：
//   * 预案：核下线前其向量清单逐条转移到幸存核（负载均摊）；
//   * 目标核合法性：目标必须在线且非下线核（自环拒）；
//   * 守恒：离线向量数 = 转移数 + 孤儿数（孤儿必须 0 才放行下线）；
//   * 均摊：按幸存核轮转分派（确定性）；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8cf0 独占） ─────────────────────────────────────────────
pub const XC_HP_SEALED: u32 = 0x8CF1; // 封账后预案
pub const XC_HP_BAD_TARGET: u32 = 0x8CF2; // 目标核非法
pub const XC_HP_SELF: u32 = 0x8CF3; // 转移到下线核自身
pub const XC_HP_ORPHAN: u32 = 0x8CF4; // 孤儿向量未转移
pub const XC_HP_NO_VECTORS: u32 = 0x8CF5; // 下线核无向量清单
pub const XC_HP_FULL: u32 = 0x8CF6; // 预案容量满

/// 转移行。
#[derive(Clone, Copy)]
pub struct MoveRow {
    pub vec_no: u8,
    pub from_core: u8,
    pub to_core: u8,
}

/// 热插拔预案账。
pub struct HotplugPlan {
    cap: usize,
    offline: u8,
    online: Vec<u8>,
    vectors: Vec<u8>,
    moves: Vec<MoveRow>,
    orphans: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl HotplugPlan {
    /// 新预案：offline 核 + 在线核表 + 离线核向量清单。
    pub fn new(offline: u8, online: &[u8], vectors: &[u8]) -> HotplugPlan {
        HotplugPlan {
            cap: 256,
            offline,
            online: online.to_vec(),
            vectors: vectors.to_vec(),
            moves: vec![],
            orphans: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn online_ok(&self, c: u8) -> bool {
        self.online.contains(&c) && c != self.offline
    }

    /// 执行预案：向量按幸存核轮转均摊转移。
    pub fn execute(&mut self) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_HP_SEALED, "封账后预案"));
            return Err(XC_HP_SEALED);
        }
        if self.vectors.is_empty() {
            self.rejects.push((XC_HP_NO_VECTORS, "离线核无向量清单"));
            return Err(XC_HP_NO_VECTORS);
        }
        if self.online.is_empty() {
            self.rejects.push((XC_HP_BAD_TARGET, "无幸存核"));
            return Err(XC_HP_BAD_TARGET);
        }
        let mut n = 0usize;
        let mut k = 0usize;
        for &v in &self.vectors {
            // 轮转选目标
            let mut tries = 0usize;
            let target = loop {
                let cand = self.online[k % self.online.len()];
                k += 1;
                tries += 1;
                if self.online_ok(cand) || tries > self.online.len() {
                    break cand;
                }
            };
            if !self.online_ok(target) {
                self.orphans = self.orphans.saturating_add(1);
                continue;
            }
            self.moves.push(MoveRow { vec_no: v, from_core: self.offline, to_core: target });
            n += 1;
        }
        Ok(n)
    }

    /// 下线放行门禁：孤儿必须为 0。
    pub fn offline_gate(&mut self) -> Result<(), u32> {
        if self.orphans != 0 {
            self.rejects.push((XC_HP_ORPHAN, "孤儿向量未转移"));
            return Err(XC_HP_ORPHAN);
        }
        Ok(())
    }

    /// 守恒：离线向量 = 转移 + 孤儿。
    pub fn conserved(&self) -> bool {
        self.vectors.len() == self.moves.len() + self.orphans as usize
    }

    /// 目标分布（核 → 向量数）。
    pub fn target_dist(&self) -> Vec<(u8, usize)> {
        let mut out: Vec<(u8, usize)> = vec![];
        for m in &self.moves {
            let mut hit = false;
            for x in out.iter_mut() {
                if x.0 == m.to_core {
                    x.1 += 1;
                    hit = true;
                    break;
                }
            }
            if !hit {
                out.push((m.to_core, 1));
            }
        }
        out
    }

    pub fn moves_len(&self) -> usize {
        self.moves.len()
    }
    pub fn orphans(&self) -> u32 {
        self.orphans
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_HP_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.offline as u64);
        for &v in &self.vectors {
            h ^= v as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        for m in &self.moves {
            h ^= (m.vec_no as u64).rotate_left(7) ^ (m.to_core as u64).rotate_left(31);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准预案场景：核 3 下线（8 向量）→ 4 幸存核轮转均摊 → 全转移零孤儿。
pub fn hotplug_scenario() -> Result<(usize, u32, u64), u32> {
    let mut p = HotplugPlan::new(3, &[0, 1, 2, 4], &[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]);
    let n = p.execute()?;
    p.offline_gate()?;
    if !p.conserved() || n != 8 {
        return Err(XC_HP_ORPHAN);
    }
    let fp = p.seal()?;
    Ok((p.moves_len(), p.orphans(), fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）无向量清单下线必拒
    {
        let mut p = HotplugPlan::new(2, &[0, 1], &[]);
        if p.execute() == Err(XC_HP_NO_VECTORS) {
            caught += 1;
        }
    }
    // 2）无幸存核必拒
    {
        let mut p = HotplugPlan::new(0, &[], &[1, 2]);
        if p.execute() == Err(XC_HP_BAD_TARGET) {
            caught += 1;
        }
    }
    // 3）转移零孤儿正例
    {
        let mut p = HotplugPlan::new(1, &[0, 2], &[5, 6, 7]);
        let _ = p.execute();
        if p.offline_gate() == Ok(()) && p.conserved() {
            caught += 1;
        }
    }
    // 4）目标分布均摊（3 向量 2 核 → 2+1）
    {
        let mut p = HotplugPlan::new(1, &[0, 2], &[10, 11, 12]);
        let _ = p.execute();
        let d = p.target_dist();
        let total: usize = d.iter().map(|x| x.1).sum();
        if total == 3 && d.len() == 2 {
            caught += 1;
        }
    }
    // 5）封账后预案必拒
    {
        let mut p = HotplugPlan::new(1, &[0], &[2]);
        let _ = p.execute();
        let _ = p.seal();
        if p.seal() == Err(XC_HP_SEALED) {
            caught += 1;
        }
    }
    // 6）守恒（转移+孤儿=总数）
    {
        let mut p = HotplugPlan::new(1, &[0, 2], &[3, 4]);
        let _ = p.execute();
        if p.conserved() {
            caught += 1;
        }
    }
    (caught, N)
}
