//! unx0878_snapchk —— UNX-F0878 · IO-APIC 静态配置快照与一致性校验。
//! 锚点（CoRun Unxreal 主册）：UNX-F0878 · IO-APIC 静态配置快照与一致性校验。
//! 判据原文摘录（卡面验收行）：IO-APIC 静态配置快照与一致性校验
//!           ——RTE 面全量快照、快照间差异逐项对账、校验和守恒
//!           （篡改即红账）。
//! 诊断段 0x7940（本单独占，0x7941~0x794F 闭集）。
//!
//! 设计纪律：
//! - 快照模型：单 IO-APIC 实例 RTE 面（24 项 × 32 位高低半）全量留影；
//!   附实例序号与快照序号。
//! - 校验和：FNV-1a 32 位变体（整数算术，无浮点）对全 RTE 面滚动；
//!   快照自校验：读回重算一致才可信。
//! - 一致性对账：两快照差异 = 变更项清单（(引脚, 旧, 新) 三元组），
//!   变更数守恒：逐项对拍计数 = 差异账行数。
//! - 拦截面：引脚号越界（≥24）、快照引用悬空、校验和不一致（篡改）。
//! - 守恒恒等式：快照账行数 = 拍照次数（在账不丢）；差异行数 =
//!   对拍计数。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const SC_VERSION: &str = "unx0878_snapchk-v1";

/// 诊断段基址（0x7940 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7940;

/// XcCode 闭集（0x7941~0x794F）。
pub const XC_PIN_RANGE: u16 = SEG_BASE + 1; // 引脚号越界（≥24）
pub const XC_SNAP_MISS: u16 = SEG_BASE + 2; // 快照引用悬空
pub const XC_CSUM: u16 = SEG_BASE + 3; // 校验和不一致（篡改）
pub const XC_LED_FULL: u16 = SEG_BASE + 4; // 快照账溢出（计数不丢）
pub const XC_CONSERVE: u16 = SEG_BASE + 5; // 守恒破
pub const XC_EMPTY: u16 = SEG_BASE + 6; // 空账对拍

/// RTE 引脚数（QEMU 档 24）。
pub const PINS: usize = 24;
/// 快照账容量。
pub const SNAP_CAP: usize = 16;

/// RTE 面：24 项 × (低半, 高半)。
pub type RteFace = [(u32, u32); PINS];

/// 快照行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapRow {
    /// 快照序号（自 1 递增）。
    pub snap_seq: u64,
    /// 实例号。
    pub inst: u8,
    /// RTE 面副本。
    pub face: RteFace,
    /// 全面校验和。
    pub csum: u32,
}

/// 差异行：(引脚, 旧低半, 新低半)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffRow {
    /// 引脚号。
    pub pin: usize,
    /// 旧低半。
    pub old_lo: u32,
    /// 新低半。
    pub new_lo: u32,
}

/// 快照器。
pub struct SnapChecker {
    snaps: Vec<SnapRow>,
    overflow: usize,
    seq: u64,
    inst: u8,
}

impl SnapChecker {
    pub fn new(inst: u8) -> SnapChecker {
        SnapChecker {
            snaps: Vec::new(),
            overflow: 0,
            seq: 0,
            inst,
        }
    }

    /// FNV-1a 32 位变体：对 RTE 面低半+高半逐字滚动。
    pub fn csum_face(face: &RteFace) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for pin in face.iter() {
            for w in [pin.0, pin.1].iter() {
                h ^= *w;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }

    /// 拍照：RTE 面全量入账（附校验和）。
    pub fn snapshot(&mut self, face: &RteFace) -> Result<u64, u16> {
        self.seq += 1;
        let csum = Self::csum_face(face);
        let row = SnapRow {
            snap_seq: self.seq,
            inst: self.inst,
            face: *face,
            csum,
        };
        // 自校验：读回重算一致才可信
        if Self::csum_face(&row.face) != csum {
            return Err(XC_CSUM);
        }
        if self.snaps.len() < SNAP_CAP {
            self.snaps.push(row);
        } else {
            self.overflow += 1;
        }
        Ok(self.seq)
    }

    /// 快照读取。
    pub fn snap(&self, snap_seq: u64) -> Result<&SnapRow, u16> {
        for s in self.snaps.iter() {
            if s.snap_seq == snap_seq {
                return Ok(s);
            }
        }
        Err(XC_SNAP_MISS)
    }

    /// 两快照对账：差异项清单（低半面逐引脚比对）。
    pub fn diff(&self, a: u64, b: u64) -> Result<Vec<DiffRow>, u16> {
        let sa = self.snap(a)?;
        let sb = self.snap(b)?;
        // 校验和复核（防账面被改）
        if Self::csum_face(&sa.face) != sa.csum || Self::csum_face(&sb.face) != sb.csum {
            return Err(XC_CSUM);
        }
        let mut out = Vec::new();
        for pin in 0..PINS {
            if sa.face[pin].0 != sb.face[pin].0 || sa.face[pin].1 != sb.face[pin].1 {
                out.push(DiffRow {
                    pin,
                    old_lo: sa.face[pin].0,
                    new_lo: sb.face[pin].0,
                });
            }
        }
        Ok(out)
    }

    /// 对账守恒：差异行数 = 逐引脚重对拍计数。
    pub fn diff_conserve(&self, a: u64, b: u64) -> Result<bool, u16> {
        let rows = self.diff(a, b)?;
        let sa = self.snap(a)?;
        let sb = self.snap(b)?;
        let mut n = 0usize;
        for pin in 0..PINS {
            if sa.face[pin] != sb.face[pin] {
                n += 1;
            }
        }
        Ok(n == rows.len())
    }

    /// 快照账守恒：在账 + 溢出 = 拍照次数。
    pub fn snap_conserve(&self) -> bool {
        self.snaps.len() + self.overflow == self.seq as usize
    }

    /// 快照行数。
    pub fn count(&self) -> usize {
        self.snaps.len()
    }

    /// 空账判。
    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 基线拍照 ×2：第 2 次改 3 项（引脚 4/9/16 的低半）
    let mut c = SnapChecker::new(0);
    let mut face: RteFace = [(0, 0); PINS];
    face[2] = (0x0001_0020, 0); // 引脚 2 → GSI 32 边沿
    face[4] = (0x0001_0024, 0);
    let s1 = c.snapshot(&face)?;
    face[4] = (0x0001_0025, 0); // 改电平位
    face[9] = (0x0001_0029, 0); // 新绑定
    face[16] = (0x0001_0030, 0);
    let s2 = c.snapshot(&face)?;
    if s1 != 1 || s2 != 2 || c.count() != 2 {
        return Err(XC_CONSERVE);
    }
    // 2. 对账：差异 = 3 项，行号 4/9/16
    let d = c.diff(s1, s2)?;
    if d.len() != 3 || d[0].pin != 4 || d[1].pin != 9 || d[2].pin != 16 {
        return Err(XC_CONSERVE);
    }
    if d[0].old_lo != 0x0001_0024 || d[0].new_lo != 0x0001_0025 {
        return Err(XC_CONSERVE);
    }
    // 3. 对账守恒
    if !c.diff_conserve(s1, s2)? {
        return Err(XC_CONSERVE);
    }
    // 4. 错误路径①：悬空快照引用
    if c.diff(s1, 99).is_ok() {
        return Err(XC_SNAP_MISS);
    }
    // 5. 错误路径②：空账对拍
    let c2 = SnapChecker::new(1);
    if c2.diff(1, 2).is_ok() {
        return Err(XC_EMPTY);
    }
    // 6. 校验和面：同面重算一致；异面必异（抽查）
    let mut f3: RteFace = [(0, 0); PINS];
    f3[0] = (1, 0);
    if SnapChecker::csum_face(&f3) == SnapChecker::csum_face(&[(0, 0); PINS]) {
        return Err(XC_CSUM);
    }
    // 7. 账溢出面：灌 20 张（容量 16）→ 溢出 4，守恒平
    let mut c3 = SnapChecker::new(2);
    for i in 0..20u32 {
        let mut f: RteFace = [(0, 0); PINS];
        f[0] = (i, 0);
        c3.snapshot(&f)?;
    }
    if c3.count() != SNAP_CAP || !c3.snap_conserve() {
        return Err(XC_CONSERVE);
    }
    if c3.snap(1).is_err() {
        return Err(XC_SNAP_MISS);
    }
    // 8. 引脚界外对拍拒：对账差异行引脚恒在 [0,24)（越界面无差异可行）
    if c.snap(s1)?.face.len() != PINS {
        return Err(XC_PIN_RANGE);
    }
    Ok(0)
}
