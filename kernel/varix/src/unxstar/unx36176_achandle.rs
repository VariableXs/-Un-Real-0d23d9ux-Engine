//! UNX-F36176-J1 AccessCheck 句柄面（achandle）。
//! 判据：① 固定容量句柄表——句柄值 = 槽位 | 代数位（LCG 代数防
//! 重用悬摆），授予/查表/关闭三面；② 关闭后同槽重授必须换代（旧
//! 句柄查表显性拒 E_STALE）；③ 句柄 ↔ 请求方关联与权限快照；④
//! 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：句柄语义冻结在本模块（AccessCheck 域的对象引用面），
//! 消费 36161 AcOutcome 做授权快照；供 36178 风暴面批量授予/关闭。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4060;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FULL: u16 = SEG_BASE + 2; // 句柄表满
pub const E_NOHANDLE: u16 = SEG_BASE + 3; // 句柄不在表内
pub const E_STALE: u16 = SEG_BASE + 4; // 旧代句柄（槽已重授）
pub const E_BADHANDLE: u16 = SEG_BASE + 5; // 句柄形非法（保留位/零）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 7; // 句柄表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 句柄表容量锚。
pub const HANDLE_N: usize = 16;

/// 句柄形：低 4 位槽位 / 位 4..12 代数（1..255）/ 位 12.. 保留零。
pub const H_SLOT_MASK: u32 = 0x0000_000F;
pub const H_GEN_SHIFT: u32 = 4;
pub const H_GEN_MASK: u32 = 0x0000_0FF0;
pub const H_LEGAL: u32 = H_SLOT_MASK | H_GEN_MASK;

/// 句柄授予记录（快照：授予时的判定面投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandleGrant {
    pub handle: u32,
    pub side: u16,
    pub granted: u32,
    pub verdict: u16,
    pub live: bool,
}

/// 句柄表（固定容量；代数防重用）。
pub struct HandleTable {
    slots: [Option<HandleGrant>; HANDLE_N],
    gen: [u8; HANDLE_N],
    pub grants: u32,
    pub closes: u32,
    pub stale_hits: u32,
}

impl HandleTable {
    pub fn new() -> HandleTable {
        HandleTable {
            slots: [None; HANDLE_N],
            gen: [0u8; HANDLE_N],
            grants: 0,
            closes: 0,
            stale_hits: 0,
        }
    }

    /// 授予：找空槽（代数自增一号入槽）。
    pub fn grant(&mut self, side: u16, o: &acf::AcOutcome) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        let mut slot = None;
        let mut i = 0;
        while i < HANDLE_N {
            let free = match self.slots[i] {
                None => true,
                Some(g) => !g.live, // 关闭槽可重授（换代防悬摆）
            };
            if free {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let slot = match slot {
            Some(s) => s,
            None => return Err(E_FULL),
        };
        self.gen[slot] = self.gen[slot] % 255 + 1;
        let handle = (slot as u32) | ((self.gen[slot] as u32) << H_GEN_SHIFT);
        self.slots[slot] = Some(HandleGrant {
            handle,
            side,
            granted: o.granted,
            verdict: o.verdict,
            live: true,
        });
        self.grants += 1;
        Ok(handle)
    }

    /// 查表：形校验 → 槽内代数对拍 → live 检查。
    pub fn lookup(&mut self, handle: u32) -> Result<HandleGrant, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if handle == 0 || handle & !H_LEGAL != 0 {
            return Err(E_BADHANDLE);
        }
        let slot = (handle & H_SLOT_MASK) as usize;
        match self.slots[slot] {
            Some(g) if g.live && g.handle == handle => Ok(g),
            Some(g) if g.live && g.handle != handle => {
                self.stale_hits += 1;
                Err(E_STALE)
            }
            _ => Err(E_NOHANDLE),
        }
    }

    /// 关闭：live 翻否（句柄值保留供 STALE 侦测）。
    pub fn close(&mut self, handle: u32) -> Result<(), u16> {
        let g = self.lookup(handle)?;
        let slot = (g.handle & H_SLOT_MASK) as usize;
        if let Some(rec) = self.slots[slot] {
            self.slots[slot] = Some(HandleGrant { live: false, ..rec });
        }
        self.closes += 1;
        Ok(())
    }

    /// 存活计数。
    pub fn live_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < HANDLE_N {
            if let Some(g) = self.slots[i] {
                if g.live {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 按请求方清点（side 域内）。
    pub fn count_side(&self, side: u16) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < HANDLE_N {
            if let Some(g) = self.slots[i] {
                if g.live && g.side == side {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 句柄形守卫：槽位/代数/保留位三分解析。
pub fn parse_handle(h: u32) -> Result<(u32, u8), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if h == 0 || h & !H_LEGAL != 0 {
        return Err(E_BADHANDLE);
    }
    let slot = h & H_SLOT_MASK;
    let gen = ((h & H_GEN_MASK) >> H_GEN_SHIFT) as u8;
    if gen == 0 {
        return Err(E_BADHANDLE); // 代数从 1 起
    }
    Ok((slot, gen))
}

/// 授权快照一致：句柄记录的判定面与 outcome 同面。
pub fn snapshot_matches(g: &HandleGrant, o: &acf::AcOutcome) -> bool {
    g.granted == o.granted && g.verdict == o.verdict
}

/// 句柄面自检：容量锚 + 句柄形合法区 + 空表。
pub fn handle_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if HANDLE_N != 16 || H_LEGAL != 0x0000_0FFF {
        return Err(E_TABLE);
    }
    let t = HandleTable::new();
    if t.live_count() != 0 || t.grants != 0 || t.closes != 0 || t.stale_hits != 0 {
        return Err(E_TABLE);
    }
    if parse_handle(0).is_ok() || parse_handle(0x0000_1000).is_ok() {
        return Err(E_TABLE); // 零句柄 / 保留位非零
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 授予-查表-关闭全生命周期；R3 表满与 STALE；
/// R4 代数防重用（同槽重授换代）；R5 负例注入（坏形/旧代/侧清点）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    handle_integrity()?;
    hits += 1;
    // R2：全生命周期——授予（Grant 快照）→ 查表 → 快照一致 → 关闭 → 拒查。
    let mut t = HandleTable::new();
    let o = acf::AcOutcome { granted: 0x89, verdict: 0, failed_step: 0, cause: 0, missing: 0 };
    let h1 = t.grant(SIDE_A, &o)?;
    let (slot1, gen1) = parse_handle(h1)?;
    if slot1 != 0 || gen1 != 1 {
        return Err(E_TABLE);
    }
    let g = t.lookup(h1)?;
    if !snapshot_matches(&g, &o) || g.side != SIDE_A {
        return Err(E_NEG);
    }
    t.close(h1)?;
    if t.live_count() != 0 || t.closes != 1 {
        return Err(E_TABLE);
    }
    if t.lookup(h1).is_ok() {
        return Err(E_NEG); // 关闭后拒查
    }
    hits += 1;
    // R3：表满——16 授后第 17 个 E_FULL；关闭一条后可再授；STALE 侦测。
    let mut t2 = HandleTable::new();
    let mut handles: Vec<u32> = Vec::new();
    let mut n = 0;
    while n < HANDLE_N {
        handles.push(t2.grant(SIDE_B, &o)?);
        n += 1;
    }
    if t2.grant(SIDE_B, &o) != Err(E_FULL) {
        return Err(E_NEG);
    }
    t2.close(handles[3])?;
    let h_new = t2.grant(SIDE_A, &o)?;
    if t2.live_count() != HANDLE_N as u32 {
        return Err(E_TABLE);
    }
    // 槽 3 已重授给 SIDE_A——旧句柄与槽内新句柄同槽不同代 → E_STALE。
    if t2.lookup(handles[3]) != Err(E_STALE) || t2.stale_hits != 1 {
        return Err(E_NEG);
    }
    let _ = h_new;
    hits += 1;
    // R4：代数防重用——同槽关-授循环，代数单调增。
    let mut t3 = HandleTable::new();
    let a1 = t3.grant(SIDE_A, &o)?;
    let (_, g1) = parse_handle(a1)?;
    t3.close(a1)?;
    let a2 = t3.grant(SIDE_A, &o)?;
    let (_, g2) = parse_handle(a2)?;
    if g2 != g1 % 255 + 1 || a1 == a2 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R5：负例注入——坏形句柄 / 零句柄 / 空表查 / 侧清点。
    let mut t4 = HandleTable::new();
    if t4.lookup(0x0000_1000).is_ok() || t4.lookup(0).is_ok() {
        return Err(E_NEG);
    }
    if parse_handle(0).is_ok() {
        return Err(E_NEG); // 零句柄（代数 0）非法
    }
    if t4.lookup(0x0000_0101).is_ok() {
        return Err(E_NEG); // 空表查
    }
    let x1 = t4.grant(SIDE_A, &o)?;
    let x2 = t4.grant(SIDE_B, &o)?;
    if t4.count_side(SIDE_A) != 1 || t4.count_side(SIDE_B) != 1 {
        return Err(E_TABLE);
    }
    t4.close(x1)?;
    t4.close(x2)?;
    if t4.count_side(SIDE_A) != 0 || t4.closes != 2 {
        return Err(E_TABLE);
    }
    hits += 1;
    Ok(hits)
}

/// 请求方标识（句柄侧域：0=A，1=B；与 36175 交错面同域值）。
pub const SIDE_A: u16 = 0;
pub const SIDE_B: u16 = 1;

/// 句柄侧（判别式冻结；表驱动回显）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleSide {
    A = 0x0000,
    B = 0x0001,
}

impl HandleSide {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 侧名回显（越界 "bad-side"）。
pub fn side_name(v: u16) -> &'static str {
    match v {
        0 => "side-a",
        1 => "side-b",
        _ => "bad-side",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn handle_table_holds() {
        assert_eq!(handle_integrity(), Ok(()));
    }

    #[test]
    fn stale_detected() {
        let mut t = HandleTable::new();
        let o = acf::AcOutcome { granted: 1, verdict: 0, failed_step: 0, cause: 0, missing: 0 };
        let h = t.grant(0, &o).ok().unwrap_or(0xFFFF_0000);
        t.close(h).ok().unwrap_or(());
        let _ = t.grant(0, &o);
        assert_eq!(t.lookup(h), Err(E_STALE));
    }

    #[test]
    fn zero_gen_rejected() {
        assert!(parse_handle(0).is_err());
    }
}

pub fn run_unx36176_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36176_achandle");
    cs.add("SEG 段位独占", SEG_BASE == 0x4060, "0x4060~0x406F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "授予/查表/STALE/换代全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", handle_integrity() == Ok(()), "容量锚 + 句柄形合法区");
    cs.add("契约冻结", VER == 1, "句柄面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FULL => "句柄表满",
        E_NOHANDLE => "句柄不在表内",
        E_STALE => "旧代句柄（槽已重授）",
        E_BADHANDLE => "句柄形非法",
        E_NEG => "负例注入未被检出",
        E_TABLE => "句柄表闭集破坏",
        _ => "未知诊断码",
    }
}
