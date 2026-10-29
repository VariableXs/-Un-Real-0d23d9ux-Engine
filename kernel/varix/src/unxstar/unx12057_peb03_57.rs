//! UNX-F12057 PEB 越界写检测
//! 分段: 0x19940 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：环境块相邻字段间的越界写是经典破坏源。
//! 本卡以"哨兵带"建模：
//!   - 每个保护区 { 起点, 长度, 哨兵值 0xA5 }；
//!   - 写请求 { 区间起点, 长度 } 须完全落在某保护区内；
//!   - 触碰哨兵带（区间跨界）→ OOB 检出（V57_OOB）；
//!   - 写入未注册区间 → V57_NOSUCH；
//!   - 零长度写 → V57_LEN；
//!   - 写后哨兵完整性扫描可检出漏网写（哨兵被改）。

use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19940;
pub const SEG_V57_OOB: u32 = SEG_BASE + 0x01;
pub const SEG_V57_NOSUCH: u32 = SEG_BASE + 0x02;
pub const SEG_V57_LEN: u32 = SEG_BASE + 0x03;
pub const SEG_V57_SEALED: u32 = SEG_BASE + 0x04;
pub const SEG_V57_CORRUPT: u32 = SEG_BASE + 0x05;

pub const SENTINEL: u8 = 0xA5;

/// 保护区：数据区 + 两侧哨兵带各 4 字节
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardZone {
    pub base: u32,
    pub data_len: u32,
    pub sealed: bool,
}

impl GuardZone {
    pub const BAND: u32 = 4;

    /// 数据区范围
    pub fn data_range(&self) -> (u32, u32) {
        (self.base, self.base + self.data_len)
    }

    /// 左哨兵带范围
    pub fn left_band(&self) -> (u32, u32) {
        (self.base - Self::BAND, self.base)
    }

    /// 右哨兵带范围
    pub fn right_band(&self) -> (u32, u32) {
        (self.base + self.data_len, self.base + self.data_len + Self::BAND)
    }

    /// 完整范围（含哨兵）
    pub fn total_range(&self) -> (u32, u32) {
        (self.base - Self::BAND, self.base + self.data_len + Self::BAND)
    }
}

/// 哨兵内存
#[derive(Debug, Clone)]
pub struct GuardMem {
    pub zones: Vec<GuardZone>,
    pub mem: Vec<u8>,
    pub sealed: bool,
}

impl GuardMem {
    /// 建立内存：各保护区两侧刷哨兵
    pub fn new(zones: &[GuardZone]) -> GuardMem {
        let mut mem = Vec::new();
        let mut i = 0;
        while i < 0x400 {
            mem.push(0);
            i += 1;
        }
        let mut g = GuardMem { zones: Vec::new(), mem, sealed: false };
        for z in zones {
            let _ = g.add_zone(z.clone());
        }
        g
    }

    /// 注册区并刷哨兵：重叠 → NOSUCH；封存 → SEALED
    pub fn add_zone(&mut self, z: GuardZone) -> Result<(), u32> {
        if self.sealed {
            return Err(SEG_V57_SEALED);
        }
        let (l, r) = z.left_band();
        if r == 0 || l >= r {
            return Err(SEG_V57_LEN);
        }
        for e in &self.zones {
            let (el, er) = e.total_range();
            let (nl, nr) = z.total_range();
            if nl < er && el < nr {
                return Err(SEG_V57_NOSUCH);
            }
        }
        // 刷哨兵
        let mut i = l;
        while i < r {
            self.mem[i as usize] = SENTINEL;
            i += 1;
        }
        let (rl, rr) = z.right_band();
        let mut j = rl;
        while j < rr {
            self.mem[j as usize] = SENTINEL;
            j += 1;
        }
        self.zones.push(z);
        Ok(())
    }

    /// 审批写：零长 → LEN；未注册 → NOSUCH；跨界 → OOB
    pub fn approve_write(&self, start: u32, len: u32) -> Result<u32, u32> {
        if len == 0 {
            return Err(SEG_V57_LEN);
        }
        let wend = start + len;
        for z in &self.zones {
            let (dl, dr) = z.data_range();
            if start >= dl && wend <= dr {
                return Ok(self.zone_id(z));
            }
            // 部分落在数据区但跨界 → OOB
            if start < dr && wend > dl {
                return Err(SEG_V57_OOB);
            }
        }
        Err(SEG_V57_NOSUCH)
    }

    /// 执行写（仅写入，不做审批——审批由 approve_write 先行）
    pub fn do_write(&mut self, start: u32, data: &[u8]) -> Result<(), u32> {
        if self.sealed {
            return Err(SEG_V57_SEALED);
        }
        if data.is_empty() {
            return Err(SEG_V57_LEN);
        }
        if self.approve_write(start, data.len() as u32).is_err() {
            // 直写也要求审批通过
            return Err(SEG_V57_OOB);
        }
        let mut i = 0;
        while i < data.len() {
            self.mem[(start as usize) + i] = data[i];
            i += 1;
        }
        Ok(())
    }

    /// 非法直写（绕过审批）用于注入验证
    pub fn raw_poke(&mut self, off: u32, v: u8) -> Result<(), u32> {
        if (off as usize) >= self.mem.len() {
            return Err(SEG_V57_NOSUCH);
        }
        self.mem[off as usize] = v;
        Ok(())
    }

    /// 哨兵完整性扫描：返回被破坏的区号列表
    pub fn scan(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        let mut idx = 0;
        while idx < self.zones.len() {
            let z = &self.zones[idx];
            let mut corrupt = false;
            let (l, r) = z.left_band();
            let mut i = l;
            while i < r {
                if self.mem[i as usize] != SENTINEL {
                    corrupt = true;
                }
                i += 1;
            }
            let (rl, rr) = z.right_band();
            let mut j = rl;
            while j < rr {
                if self.mem[j as usize] != SENTINEL {
                    corrupt = true;
                }
                j += 1;
            }
            if corrupt {
                bad.push(idx as u32);
            }
            idx += 1;
        }
        bad
    }

    fn zone_id(&self, z: &GuardZone) -> u32 {
        let mut i = 0;
        while i < self.zones.len() {
            if self.zones[i] == *z {
                return i as u32;
            }
            i += 1;
        }
        0
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(g: &GuardMem) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for z in &g.zones {
        h = (h.wrapping_mul(211).wrapping_add((z.base & 0xFF) as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(z.data_len as u64)) & 0xFFFF_FFFF;
    }
    // 摘样哨兵字节
    let mut k = 0;
    while k < g.mem.len() {
        h = (h.wrapping_mul(211).wrapping_add(g.mem[k] as u64)) & 0xFFFF_FFFF;
        k += 0x40;
    }
    h = (h.wrapping_mul(211).wrapping_add(g.sealed as u64)) & 0xFFFF_FFFF;
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    let mk = |base: u32, len: u32| GuardZone { base, data_len: len, sealed: false };
    // 样1: 跨界写 → OOB
    let mut g = GuardMem::new(&[mk(0x100, 0x20)]);
    if g.approve_write(0x11E, 4) == Err(SEG_V57_OOB) {
        ok[0] = true;
    }
    // 样2: 未注册区间写 → NOSUCH
    if g.approve_write(0x300, 4) == Err(SEG_V57_NOSUCH) {
        ok[1] = true;
    }
    // 样3: 零长度 → LEN
    if g.approve_write(0x100, 0) == Err(SEG_V57_LEN) {
        ok[2] = true;
    }
    // 样4: 绕过审批写哨兵 → scan 检出 CORRUPT
    let _ = g.raw_poke(0x100 + 0x20, 0xFF);
    if !g.scan().is_empty() {
        ok[3] = true;
    }
    // 样5: 封存后写 → SEALED
    let mut g2 = GuardMem::new(&[mk(0x100, 0x20)]);
    g2.seal();
    if g2.do_write(0x100, &[1, 2]) == Err(SEG_V57_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12057_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12057");
    let mk = |base: u32, len: u32| GuardZone { base, data_len: len, sealed: false };
    // J1 区内写合法
    let g = GuardMem::new(&[mk(0x100, 0x20)]);
    cs.add("J1", g.approve_write(0x100, 0x20) == Ok(0), "full write ok");
    // J2 部分区内写合法
    cs.add("J2", g.approve_write(0x110, 0x10) == Ok(0), "partial ok");
    // J3 左跨界 OOB
    cs.add("J3", g.approve_write(0xFC, 8).err() == Some(SEG_V57_OOB), "left oob");
    // J4 右跨界 OOB
    cs.add("J4", g.approve_write(0x11E, 4).err() == Some(SEG_V57_OOB), "right oob");
    // J5 未注册区间
    cs.add("J5", g.approve_write(0x300, 2).err() == Some(SEG_V57_NOSUCH), "no zone");
    // J6 零长度
    cs.add("J6", g.approve_write(0x100, 0).err() == Some(SEG_V57_LEN), "zero len");
    // J7 哨兵初值
    let g2 = GuardMem::new(&[mk(0x100, 0x20)]);
    cs.add("J7", g2.mem[0xFC] == SENTINEL && g2.mem[0x120] == SENTINEL, "sentinel init");
    // J8 干净扫描
    cs.add("J8", g2.scan().is_empty(), "clean scan");
    // J9 破坏右带检出
    let mut g3 = GuardMem::new(&[mk(0x100, 0x20)]);
    let _ = g3.raw_poke(0x120, 0x00);
    cs.add("J9", g3.scan() == vec![0], "right corrupt");
    // J10 破坏左带检出
    let mut g4 = GuardMem::new(&[mk(0x100, 0x20)]);
    let _ = g4.raw_poke(0xFD, 0x11);
    cs.add("J10", g4.scan() == vec![0], "left corrupt");
    // J11 双区只检出被破坏者
    let mut g5 = GuardMem::new(&[mk(0x100, 0x20), mk(0x200, 0x20)]);
    let _ = g5.raw_poke(0x220, 0xFF);
    cs.add("J11", g5.scan() == vec![1], "only zone1 bad");
    // J12 区间重叠注册拒绝
    let mut g6 = GuardMem::new(&[]);
    cs.add("J12", g6.add_zone(mk(0x100, 0x20)).is_ok()
        && g6.add_zone(mk(0x110, 0x20)).err() == Some(SEG_V57_NOSUCH), "overlap reject");
    // J13 实际写 + 读回
    let mut g7 = GuardMem::new(&[mk(0x100, 0x20)]);
    let _ = g7.do_write(0x100, &[0x42, 0x43]);
    cs.add("J13", g7.mem[0x100] == 0x42 && g7.mem[0x101] == 0x43, "write readback");
    // J14 直写拦截（审批不通过的 do_write）
    cs.add("J14", g7.do_write(0x11E, &[1, 2, 3, 4]).err() == Some(SEG_V57_OOB), "do write oob");
    // J15 封存后写
    let mut g8 = GuardMem::new(&[mk(0x100, 0x20)]);
    g8.seal();
    cs.add("J15", g8.do_write(0x100, &[1]).err() == Some(SEG_V57_SEALED), "sealed write");
    // J16 注入五样
    let inj = inject_five();
    cs.add("J16", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_bounds() {
        let z = GuardZone { base: 0x100, data_len: 0x10, sealed: false };
        assert_eq!(z.left_band(), (0xFC, 0x100));
        assert_eq!(z.right_band(), (0x110, 0x114));
        assert_eq!(z.total_range(), (0xFC, 0x114));
    }

    #[test]
    fn test_scan_clean_and_corrupt() {
        let mut g = GuardMem::new(&[GuardZone { base: 0x100, data_len: 8, sealed: false }]);
        assert!(g.scan().is_empty());
        let _ = g.raw_poke(0x108, 1);
        assert_eq!(g.scan(), vec![0]);
    }

    #[test]
    fn test_fold_stable() {
        let g = GuardMem::new(&[GuardZone { base: 0x100, data_len: 0x20, sealed: false }]);
        let a = fold(&g);
        let b = fold(&g);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }
}
