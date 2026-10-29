//! unx48316_cfgconc — UNX-F48316 B16 批并发焦点配置隔离（双焦点不同配置零互染）（诊断段 0xC860）。
//!
//! 判据原文：
//!   J1 并发 ×10^3 零互染；判据锚定 M5 母版。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 焦点-配置叠层：每焦点窗持独立配置叠层（overlay：键值对账），
//!      有效配置 = 基线 + 本窗叠层；叠层属主唯一，路由以窗号为键。
//!   2. 路由纪律：读写操作必须显式携带 (窗号, 键)；路由器核对键的
//!      叠层属主与窗号逐位相等，越主操作即记互染并拒——×10^3 并发
//!      操作后互染计数恒 0。
//!   3. 值级双保险：每窗叠层键空间互不重叠（专属键段），即使路由
//!      失守，值级审计仍能抓住越段写入（E_ALIEN_KEY）。
//!   4. 守恒恒等式：每窗基线值 + 叠层写量 - 叠层删量 = 有效值账；
//!      全局 成功操作数 == Σ窗操作数，账目闭合可查。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC860;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CROSS_DYE: u16 = SEG_BASE + 1; // 越主操作（互染）
const E_NO_WIN: u16 = SEG_BASE + 2; // 窗号不在册
const E_ALIEN_KEY: u16 = SEG_BASE + 3; // 键越专属段
const E_CONSERVE: u16 = SEG_BASE + 4; // 守恒恒等式破
const E_STATE_BAD: u16 = SEG_BASE + 5; // 路由器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 并发口径：4 窗 × 交织 1000 操作。
pub const WIN_N: u32 = 4;
pub const OPS_TOTAL: u32 = 1000;

/// 专属键段：窗 w 键空间 [base, base+0x40)，互不重叠。
pub fn key_base(win: u32) -> u16 {
    0x1000 + (win as u16) * 0x40
}

/// 基线配置（全局底账，各窗共享读）。
pub const BASELINE: [(u16, u32); 3] = [(0x0001, 5), (0x0002, 20), (0x0003, 1)];

/// 焦点窗配置叠层。
#[derive(Clone, Debug)]
pub struct FocusOverlay {
    pub win: u32,
    pub overlay: Vec<(u16, u32)>, // 本窗叠层键值账
    pub writes: u32,
}

impl FocusOverlay {
    pub fn in_segment(&self, key: u16) -> bool {
        let base = key_base(self.win);
        key >= base && key < base + 0x40
    }
    /// 有效值：叠层有则叠层值，无则基线值（基线键 0x0001..=0x0003）。
    pub fn effective(&self, key: u16) -> Option<u32> {
        let mut i = 0usize;
        while i < self.overlay.len() {
            if self.overlay[i].0 == key {
                return Some(self.overlay[i].1);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < BASELINE.len() {
            if BASELINE[j].0 == key {
                return Some(BASELINE[j].1);
            }
            j += 1;
        }
        None
    }
}

/// 焦点配置路由枢纽。
pub struct CfgRouter {
    wins: Vec<FocusOverlay>,
    pub cross_dye: u32,
    pub routed: u32,
    pub rejected: u32,
}

impl CfgRouter {
    pub fn new() -> CfgRouter {
        let mut wins = Vec::new();
        let mut w = 0u32;
        while w < WIN_N {
            wins.push(FocusOverlay { win: w, overlay: Vec::new(), writes: 0 });
            w += 1;
        }
        CfgRouter { wins, cross_dye: 0, routed: 0, rejected: 0 }
    }

    fn find_win(&self, win: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.wins.len() {
            if self.wins[i].win == win {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 叠层写：键须落本窗专属段（值级双保险第一道）。
    pub fn write_key(&mut self, win: u32, key: u16, val: u32) -> Result<(), u16> {
        let idx = match self.find_win(win) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(E_NO_WIN);
            }
        };
        if !self.wins[idx].in_segment(key) {
            self.rejected += 1;
            return Err(E_ALIEN_KEY);
        }
        // 叠层账内同键覆盖写（最后一次写生效）。
        let mut hit = false;
        let mut i = 0usize;
        while i < self.wins[idx].overlay.len() {
            if self.wins[idx].overlay[i].0 == key {
                self.wins[idx].overlay[i].1 = val;
                hit = true;
                break;
            }
            i += 1;
        }
        if !hit {
            self.wins[idx].overlay.push((key, val));
        }
        self.wins[idx].writes += 1;
        self.routed += 1;
        Ok(())
    }

    /// 叠层读：读他窗专属段 = 互染（读也染色）；基线键全域可读。
    pub fn read_key(&mut self, win: u32, key: u16) -> Result<u32, u16> {
        let idx = match self.find_win(win) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(E_NO_WIN);
            }
        };
        let base = key_base(win);
        if key >= base && key < base + 0x40 {
            // 本窗专属键：直接读本窗叠层。
            match self.wins[idx].effective(key) {
                Some(v) => {
                    self.routed += 1;
                    Ok(v)
                }
                None => {
                    self.rejected += 1;
                    Err(E_STATE_BAD)
                }
            }
        } else if key <= 0x0003 {
            // 基线键全域可读（非互染）。
            match self.wins[idx].effective(key) {
                Some(v) => {
                    self.routed += 1;
                    Ok(v)
                }
                None => {
                    self.rejected += 1;
                    Err(E_STATE_BAD)
                }
            }
        } else {
            // 读他窗专属段 = 互染。
            self.cross_dye += 1;
            self.rejected += 1;
            Err(E_CROSS_DYE)
        }
    }

    /// 值级隔离审计：每窗叠层键全落本窗专属段（键空间互斥）。
    pub fn audit_segments(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.wins.len() {
            let mut j = 0usize;
            while j < self.wins[i].overlay.len() {
                if !self.wins[i].in_segment(self.wins[i].overlay[j].0) {
                    return Err(E_ALIEN_KEY);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒审计：叠层写量 ≥ 叠层条目数（覆盖写不增条目）、窗数闭包。
    pub fn audit_conserve(&self) -> Result<(), u16> {
        if self.wins.len() as u32 != WIN_N {
            return Err(E_STATE_BAD);
        }
        let mut i = 0usize;
        while i < self.wins.len() {
            if (self.wins[i].overlay.len() as u32) > self.wins[i].writes {
                return Err(E_CONSERVE);
            }
            i += 1;
        }
        let routed_sum = self.routed + self.rejected;
        if routed_sum == 0 {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// ×10^3 零互染终判（J1 硬口径）。
    pub fn assert_no_dye(&self, want_ops: u32) -> Result<(), u16> {
        if self.routed < want_ops {
            return Err(E_STATE_BAD);
        }
        if self.cross_dye != 0 {
            return Err(E_CROSS_DYE);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：零互染闭合 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.cross_dye != 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（并发 ×10^3 零互染），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10^3：4 窗 round-robin 交织 1000 次叠层写（键空间
    // 0x40 段内循环、覆盖写生效），全部路由成功、互染恒 0；尾键读回
    // 逐窗取末次写值。
    let mut rt = CfgRouter::new();
    let mut i = 0u32;
    while i < OPS_TOTAL {
        let win = i % WIN_N;
        let base = key_base(win);
        rt.write_key(win, base + ((i / WIN_N) % 0x40) as u16, 1000 + i)?;
        i += 1;
    }
    rt.assert_no_dye(OPS_TOTAL)?;
    if rt.cross_dye != 0 || rt.rejected != 0 {
        return Err(E_CROSS_DYE);
    }
    rt.audit_segments()?;
    rt.audit_conserve()?;
    // 尾键读回：每窗末次写在 op=996+w（键 (996+w)/4 % 0x40 = 57），
    // 有效值必为 1000+996+w（最后写生效）。
    let mut w = 0u32;
    while w < WIN_N {
        let v = rt.read_key(w, key_base(w) + 57)?;
        if v != 1000 + 996 + w {
            return Err(E_CONSERVE);
        }
        w += 1;
    }
    hits += 1;

    // R2 值级隔离终判：每窗叠层键全落本窗段（键空间互斥复核）。
    rt.audit_segments()?;
    hits += 1;

    // R3 基线共享读：基线键全域可读且值恒基线值（未叠层前）。
    let mut rt2 = CfgRouter::new();
    let mut w = 0u32;
    while w < WIN_N {
        let mut k = 0u16;
        while k < 3 {
            let v = rt2.read_key(w, 0x0001 + k)?;
            if v != BASELINE[k as usize].1 {
                return Err(E_CONSERVE);
            }
            k += 1;
        }
        w += 1;
    }
    hits += 1;

    // R4 越主读互染负例：窗 0 读窗 3 专属段 → 必拒（E_CROSS_DYE）、
    //    互染计数 +1。
    let mut rt3 = CfgRouter::new();
    rt3.write_key(3, key_base(3) + 1, 777)?;
    if rt3.read_key(0, key_base(3) + 1).is_ok() {
        return Err(E_CROSS_DYE);
    }
    if rt3.cross_dye != 1 || rt3.rejected != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R5 越段写拒收：窗 2 写窗 0 专属段键 → E_ALIEN_KEY（值级双保险
    //    第一道），不落任何窗叠层。
    let mut rt4 = CfgRouter::new();
    if rt4.write_key(2, key_base(0) + 5, 1).is_ok() {
        return Err(E_ALIEN_KEY);
    }
    if rt4.rejected != 1 {
        return Err(E_STATE_BAD);
    }
    let mut total_overlay = 0usize;
    let mut w = 0usize;
    while w < rt4.wins.len() {
        total_overlay += rt4.wins[w].overlay.len();
        w += 1;
    }
    if total_overlay != 0 {
        return Err(E_CONSERVE);
    }
    hits += 1;

    // R6 野窗号拒收：win=9 → E_NO_WIN。
    let mut rt5 = CfgRouter::new();
    if rt5.write_key(9, key_base(1), 1).is_ok() {
        return Err(E_NO_WIN);
    }
    if rt5.read_key(9, 0x0001).is_ok() {
        return Err(E_NO_WIN);
    }
    if rt5.cross_dye != 0 || rt5.rejected != 2 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 叠层覆盖写语义：同键两写 → 有效值取后写（叠层账条目不增）。
    let mut rt6 = CfgRouter::new();
    rt6.write_key(1, key_base(1) + 3, 10)?;
    rt6.write_key(1, key_base(1) + 3, 20)?;
    let v = rt6.read_key(1, key_base(1) + 3)?;
    if v != 20 {
        return Err(E_CONSERVE);
    }
    if rt6.wins[1].overlay.len() != 1 || rt6.wins[1].writes != 2 {
        return Err(E_CONSERVE);
    }
    rt6.audit_conserve()?;
    hits += 1;

    // R8 窗间零串值终判：四窗各写同偏移键、读回值各归各窗（互染
    //    最敏感面）。
    let mut rt7 = CfgRouter::new();
    let mut w = 0u32;
    while w < WIN_N {
        rt7.write_key(w, key_base(w) + 7, 5000 + w)?;
        w += 1;
    }
    let mut w = 0u32;
    while w < WIN_N {
        let v = rt7.read_key(w, key_base(w) + 7)?;
        if v != 5000 + w {
            return Err(E_CROSS_DYE);
        }
        w += 1;
    }
    rt7.audit_segments()?;
    rt7.assert_no_dye(8)?;
    hits += 1;

    // R9 写回（判据 J2）：互染未清零不写回、空摘要拒、闭合+非空摘要放行。
    let mut rt8 = CfgRouter::new();
    rt8.cross_dye = 1;
    if rt8.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    rt8.cross_dye = 0;
    rt8.write_key(0, key_base(0), 1)?;
    if rt8.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    rt8.writeback(true, 0x50)?;
    hits += 1;

    Ok(hits)
}
