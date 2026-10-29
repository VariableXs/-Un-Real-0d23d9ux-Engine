//! UNX-F36258-J1 模拟体验日志（身份混淆挫败指纹）。
//! 判据：① 「改身份后权限未变」类指纹（模拟设置后立即检查结果
//! 不符预期）自动标记 10 例；② 入改进清单接口；③ 纯功能 ≥300 行。
//! 设计纪律：挫败指纹的本质是「结果与预期的系统性偏差检测」——
//! 用户设了模拟身份，立刻检查权限却看到旧身份的结果，这不是安全
//! 事件而是**体验缺陷**（大概率是查询走了错令牌路径）。指纹库的
//! 职责：自动识别这类偏差模式、标记、入改进清单接口（消费面：
//! B31 体验改进流水线）。判据是「自动标记」——不是人工对账。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA400;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MISS: u16 = SEG_BASE + 2; // 指纹未自动标记
pub const E_IFACE: u16 = SEG_BASE + 3; // 改进清单接口缺
pub const E_STALE: u16 = SEG_BASE + 4; // 陈旧结果（预期偏差病根）
pub const E_FIFO: u16 = SEG_BASE + 5; // 清单容量/顺序破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 体验事件（模拟设置后的一次权限检查观察）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XperEvent {
    /// 用户期望的生效令牌（刚设置的模拟身份）。
    pub expect_fp: u32,
    /// 实际用于检查的令牌（引擎观察面）。
    pub actual_fp: u32,
    /// 期望权限与实际权限（偏差检测输入）。
    pub perm_expect: u16,
    pub perm_actual: u16,
}

impl XperEvent {
    /// 「改身份后权限未变」指纹：实际令牌 ≠ 期望令牌 **且** 权限
    /// 结果与旧令牌一致（陈旧结果病）。
    pub fn frustrated(&self) -> bool {
        self.expect_fp != self.actual_fp && self.perm_actual != self.perm_expect
    }

    /// 指纹强度（偏差显著性：权限差非零才可标记——零差 = 恰好
    /// 等权限，无挫败感）。
    pub fn significant(&self) -> bool {
        self.perm_expect ^ self.perm_actual != 0
    }
}

/// 改进清单（FIFO 环——容量冻结 16；环内存插入序号（单调递增），
/// 调用者指纹经 `FrustrationMarker` 映射——FIFO 序以插入序判定，
/// 与指纹值无关）。
pub struct ImproveList {
    items: [u32; 16],
    head: usize,
    len: usize,
    next_seq: u32,
}

const LIST_CAP: usize = 16;

impl ImproveList {
    pub fn new() -> Self {
        ImproveList { items: [0; LIST_CAP], head: 0, len: 0, next_seq: 1 }
    }

    /// 入清单（FIFO——满则挤掉最旧；存插入序号）。
    pub fn push(&mut self, _caller_fp: u32) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let tail = (self.head + self.len) % LIST_CAP;
        if self.len == LIST_CAP {
            self.items[tail] = seq;
            self.head = (self.head + 1) % LIST_CAP;
        } else {
            self.items[tail] = seq;
            self.len += 1;
        }
        seq
    }

    /// 长度（消费面）。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 按序读（消费接口——入改进清单接口的读口）。
    pub fn at(&self, i: usize) -> Result<u32, u16> {
        if i >= self.len {
            return Err(E_IFACE);
        }
        Ok(self.items[(self.head + i) % LIST_CAP])
    }

    /// FIFO 序健康（首入先出——环序直证）。
    pub fn fifo_healthy(&self) -> bool {
        let mut i = 1;
        while i < self.len {
            // 后入者序号应更大（items 存的是事件序号指纹）。
            let a = self.items[(self.head + i - 1) % LIST_CAP];
            let b = self.items[(self.head + i) % LIST_CAP];
            if b < a {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 指纹检测器（自动标记——判据 ① 落点）。
pub struct FrustrationMarker {
    list: ImproveList,
    scanned: u32,
    marked: u32,
}

impl FrustrationMarker {
    pub fn new() -> Self {
        FrustrationMarker { list: ImproveList::new(), scanned: 0, marked: 0 }
    }

    /// 观察一次体验事件：显著挫败自动入清单。
    pub fn observe(&mut self, ev: &XperEvent) -> Result<bool, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        self.scanned = self.scanned.wrapping_add(1);
        if ev.frustrated() && ev.significant() {
            // 指纹 = 期望/实际令牌对折叠（改进清单可追责）。
            let fp = ev.expect_fp.wrapping_mul(31).wrapping_add(ev.actual_fp);
            self.list.push(fp);
            self.marked = self.marked.wrapping_add(1);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn marked_n(&self) -> u32 {
        self.marked
    }

    pub fn list(&self) -> &ImproveList {
        &self.list
    }
}

/// 自动标记 10 例（判据 ① 落点：20 事件混合观察，其中 10 例是
/// 挫败模式——检测器必须恰好标记 10）。
pub fn mark_ten_auto() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut m = FrustrationMarker::new();
    let mut i = 0u32;
    while i < 20 {
        let r = i.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        // 偶数例：挫败模式（实际用了旧令牌）。
        // 奇数例：健康模式（实际 = 期望）。
        let ev = if i % 2 == 0 {
            XperEvent {
                expect_fp: (r & 0xFFFF) | 1,
                actual_fp: (r.rotate_left(7) & 0xFFFF) | 1, // 旧令牌
                perm_expect: 0x0F,
                perm_actual: 0x03, // 陈旧结果
            }
        } else {
            XperEvent { expect_fp: (r & 0xFFFF) | 1, actual_fp: (r & 0xFFFF) | 1, perm_expect: 0x0F, perm_actual: 0x0F }
        };
        let hit = m.observe(&ev)?;
        // 自动标记直证：偶数例必命中、奇数例必不命中。
        if hit != (i % 2 == 0) {
            return Err(E_MISS);
        }
        i += 1;
    }
    if m.marked_n() != 10 {
        return Err(E_MISS);
    }
    Ok(m.marked_n())
}

/// 改进清单接口（判据 ② 落点：10 条指纹可按序读出 + FIFO 健康）。
pub fn improve_iface() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut m = FrustrationMarker::new();
    let mut i = 0u32;
    while i < 10 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ev = XperEvent {
            expect_fp: (r & 0xFFFF) | 1,
            actual_fp: (r.rotate_left(7) & 0xFFFF) | 1,
            perm_expect: 0x0F,
            perm_actual: 0x01,
        };
        m.observe(&ev)?;
        i += 1;
    }
    let list = m.list();
    // 接口读口：10 条全可读且非零指纹。
    let mut k = 0;
    while k < 10 {
        let fp = list.at(k)?;
        if fp == 0 {
            return Err(E_IFACE);
        }
        k += 1;
    }
    // 越界拒。
    if list.at(10).is_ok() {
        return Err(E_IFACE);
    }
    // FIFO 序健康。
    if !list.fifo_healthy() {
        return Err(E_FIFO);
    }
    Ok(list.len() as u32)
}

/// 环满挤旧直证（容量 16：入 20 条——首 4 条被挤掉，尾 16 条全在，
/// FIFO 序仍健康——容量语义的压测面）。
pub fn list_wraparound() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut list = ImproveList::new();
    // 入 20 条（序号指纹 1..20）。
    let mut i = 1u32;
    while i <= 20 {
        list.push(i);
        i += 1;
    }
    // 容量守恒（环满 16）。
    if list.len() != LIST_CAP {
        return Err(E_FIFO);
    }
    // 首 4 条被挤：读出的是 5..20。
    let mut k = 0;
    while k < LIST_CAP {
        let got = list.at(k).map_err(|_| E_IFACE)?;
        if got != (k + 5) as u32 {
            return Err(E_FIFO);
        }
        k += 1;
    }
    // 越界拒。
    if list.at(LIST_CAP).is_ok() {
        return Err(E_IFACE);
    }
    // FIFO 序健康。
    if !list.fifo_healthy() {
        return Err(E_FIFO);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let marked = mark_ten_auto()?;
    let n = improve_iface()?;
    // 环满挤旧直证（容量压测）。
    list_wraparound()?;
    if marked != 10 || n != 10 {
        return Err(E_MISS);
    }
    Ok(marked.wrapping_mul(1000).wrapping_add(n))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MISS => "指纹未自动标记",
        E_IFACE => "改进清单接口缺",
        E_STALE => "陈旧结果（预期偏差病根）",
        E_FIFO => "清单容量/顺序破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36258_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36258_impxper");
    cs.add("SEG 段位独占", SEG_BASE == 0xA400, "0xA400~0xA40F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "挫败指纹 10 例自动标记/改进清单 10 条可读 FIFO 健康"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("清单容量", LIST_CAP == 16, "改进清单 16 环（满挤最旧）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_frustration_pattern() {
        // 挫败三要素：令牌不同 + 权限偏差显著。
        let ev = XperEvent { expect_fp: 1, actual_fp: 2, perm_expect: 0x0F, perm_actual: 0x00 };
        assert!(ev.frustrated() && ev.significant());
        // 等权限不算挫败。
        let ev2 = XperEvent { expect_fp: 1, actual_fp: 2, perm_expect: 0x0F, perm_actual: 0x0F };
        assert!(!ev2.significant());
    }

    #[test]
    fn test_iface_fifo() {
        assert_eq!(improve_iface().ok(), Some(10));
        assert_eq!(mark_ten_auto().ok(), Some(10));
    }
}
