//! unx48268_verpin — UNX-F48268 版本钉定账（fcitx5 钉波 21 计划书基线）（诊断段 0xC260）。
//!
//! 判据原文：
//!   J1 钉版登记+偏差告警路径在册；判据锚定 §AI-61 §4 上游版本钉定。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 钉版语义：组件版本一经钉定（登记 (组件, 钉版号, 基线) 三元组）
//!      即冻结——重钉（E_REPIN）与改钉（E_FROZEN）双拒，只进不退。
//!   2. 基线口径：钉版必须挂计划书基线（波 21 = 0x0021）；无基线钉版
//!      不入账（E_NO_BASIS）——版本钉的是计划书的承诺，不是随手快照。
//!   3. 偏差告警路径在册（J1 主体）：钉版时必须同步登记该组件的告警
//!      路由（route id）；实测版本偏离钉版时，告警必须沿在册路由落账，
//!      无路由的组件不得钉版（无路可告 = 钉了白钉）。
//!   4. 版本编码：semver 三元 (maj, min, pat) 压缩 u64 逐位比较；
//!      钉版号与实测号逐位比对，maj/min/pat 任一不等即偏差。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC260;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_REPIN: u16 = SEG_BASE + 1; // 重复钉版
const E_DEVIATE: u16 = SEG_BASE + 2; // 实测版本偏离钉版
const E_NO_PIN: u16 = SEG_BASE + 3; // 组件未钉版
const E_NO_ROUTE: u16 = SEG_BASE + 4; // 告警路由未在册
const E_FROZEN: u16 = SEG_BASE + 5; // 钉版冻结被改
const E_STATE_BAD: u16 = SEG_BASE + 6; // 钉账状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空
const E_LEDGER_BAD: u16 = SEG_BASE + 8; // 钉账缺失/非幂等

/// 计划书基线：波 21。
pub const BASIS_WAVE21: u16 = 0x0021;

/// 钉版三元组。
#[derive(Clone, Copy, Debug)]
pub struct PinRecord {
    pub comp: u16,
    pub pinned: u64, // 压缩版本号
    pub basis: u16,  // 计划书基线
}

/// 告警路由（钉版时同步登记，J1「路径在册」）。
#[derive(Clone, Copy, Debug)]
pub struct RouteRecord {
    pub comp: u16,
    pub route: u8,
}

/// 偏差告警落账。
#[derive(Clone, Copy, Debug)]
pub struct AlarmRow {
    pub comp: u16,
    pub cur: u64,
    pub pinned: u64,
    pub route: u8,
}

/// 版本压缩：semver 三元 → u64。
pub fn pack_ver(maj: u16, min: u16, pat: u16) -> u64 {
    ((maj as u64) << 32) | ((min as u64) << 16) | (pat as u64)
}

/// 版本逐位偏差判定：maj/min/pat 任一位段不等即偏差。
pub fn ver_deviate(pinned: u64, cur: u64) -> bool {
    pinned != cur
}

/// 版本钉定账。
pub struct VerPin {
    pins: Vec<PinRecord>,
    routes: Vec<RouteRecord>,
    pub alarms: Vec<AlarmRow>,
    pub checks: u32,
    pub deviates: u32,
}

impl VerPin {
    pub fn new() -> VerPin {
        VerPin { pins: Vec::new(), routes: Vec::new(), alarms: Vec::new(), checks: 0, deviates: 0 }
    }

    /// 钉版登记（J1 主体）：无基线不钉、有钉不重钉、无路由不钉。
    pub fn pin(&mut self, comp: u16, ver: u64, basis: u16, route: u8) -> Result<(), u16> {
        if basis == 0 {
            return Err(E_STATE_BAD); // 无基线
        }
        if self.find_pin(comp).is_some() {
            return Err(E_REPIN);
        }
        self.pins.push(PinRecord { comp, pinned: ver, basis });
        self.routes.push(RouteRecord { comp, route });
        Ok(())
    }

    fn find_pin(&self, comp: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.pins.len() {
            if self.pins[i].comp == comp {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn route_of(&self, comp: u16) -> Option<u8> {
        let mut i = 0usize;
        while i < self.routes.len() {
            if self.routes[i].comp == comp {
                return Some(self.routes[i].route);
            }
            i += 1;
        }
        None
    }

    /// 实测核对：钉版逐位比对；偏差沿在册路由落告警账（J1 主体）。
    pub fn check(&mut self, comp: u16, cur: u64) -> Result<(), u16> {
        self.checks += 1;
        let idx = match self.find_pin(comp) {
            Some(i) => i,
            None => return Err(E_NO_PIN),
        };
        let pinned = self.pins[idx].pinned;
        if ver_deviate(pinned, cur) {
            let route = match self.route_of(comp) {
                Some(r) => r,
                None => return Err(E_NO_ROUTE),
            };
            self.alarms.push(AlarmRow { comp, cur, pinned, route });
            self.deviates += 1;
            return Err(E_DEVIATE);
        }
        Ok(())
    }

    /// 钉版冻结：任何改钉尝试必拒（只进不退）。
    pub fn amend(&mut self, comp: u16, new_ver: u64) -> Result<(), u16> {
        match self.find_pin(comp) {
            None => Err(E_NO_PIN),
            Some(_) => Err(E_FROZEN),
        }
    }

    /// 钉账审计：钉-路一一同组件对应、告警账每条可溯到钉与路、
    /// 基线全为波 21（本批口径）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.pins.len() != self.routes.len() {
            return Err(E_STATE_BAD);
        }
        let mut i = 0usize;
        while i < self.pins.len() {
            if self.pins[i].basis != BASIS_WAVE21 {
                return Err(E_STATE_BAD);
            }
            if self.route_of(self.pins[i].comp).is_none() {
                return Err(E_NO_ROUTE);
            }
            i += 1;
        }
        i = 0;
        while i < self.alarms.len() {
            let a = self.alarms[i];
            match self.find_pin(a.comp) {
                None => return Err(E_LEDGER_BAD),
                Some(idx) => {
                    if self.pins[idx].pinned != a.pinned {
                        return Err(E_LEDGER_BAD);
                    }
                    if !ver_deviate(a.pinned, a.cur) {
                        return Err(E_LEDGER_BAD); // 无偏而告 = 假告
                    }
                }
            }
            i += 1;
        }
        if self.alarms.len() as u32 != self.deviates {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    pub fn pin_count(&self) -> usize {
        self.pins.len()
    }
    pub fn basis_of(&self, comp: u16) -> Result<u16, u16> {
        match self.find_pin(comp) {
            Some(i) => Ok(self.pins[i].basis),
            None => Err(E_NO_PIN),
        }
    }

    /// 完成写回（判据 J2）：钉版在册 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.pin_count() == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（钉版登记+偏差告警路径在册），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主闭环：3 组件按波 21 基线钉版 + 告警路由同步在册；
    //    实测逐位吻合双绿、偏离一件告警沿路由落账。
    let mut vp = VerPin::new();
    let v_fcitx5 = pack_ver(5, 1, 12);
    let v_bridge = pack_ver(1, 4, 0);
    let v_dict = pack_ver(2, 0, 3);
    vp.pin(0x0101, v_fcitx5, BASIS_WAVE21, 7)?;
    vp.pin(0x0102, v_bridge, BASIS_WAVE21, 7)?;
    vp.pin(0x0103, v_dict, BASIS_WAVE21, 9)?;
    if vp.pin_count() != 3 {
        return Err(E_STATE_BAD);
    }
    vp.check(0x0101, v_fcitx5)?; // 吻合
    vp.check(0x0102, v_bridge)?; // 吻合
    if vp.check(0x0103, pack_ver(2, 1, 0)).is_ok() {
        return Err(E_DEVIATE); // min 段漂移必报
    }
    if vp.deviates != 1 || vp.alarms.len() != 1 {
        return Err(E_STATE_BAD);
    }
    if vp.alarms[0].route != 9 {
        return Err(E_NO_ROUTE); // 告警必须沿在册路由
    }
    vp.audit()?;
    hits += 1;

    // R2 版本编码终判：压缩 u64 三段互不串号（逐位比较语义）。
    let a = pack_ver(5, 1, 12);
    let b = pack_ver(5, 2, 12);
    let c = pack_ver(5, 1, 13);
    if a == b || a == c || b == c {
        return Err(E_STATE_BAD);
    }
    if ver_deviate(a, pack_ver(5, 1, 12)) {
        return Err(E_DEVIATE); // 全等不算偏差
    }
    if !ver_deviate(a, b) || !ver_deviate(a, c) {
        return Err(E_DEVIATE);
    }
    hits += 1;

    // R3 重钉/改钉双拒：同组件二次钉版 → E_REPIN；冻结改钉 → E_FROZEN。
    if vp.pin(0x0101, v_fcitx5, BASIS_WAVE21, 7).is_ok() {
        return Err(E_REPIN);
    }
    if vp.amend(0x0101, pack_ver(9, 9, 9)).is_ok() {
        return Err(E_FROZEN);
    }
    if vp.basis_of(0x0101)? != BASIS_WAVE21 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R4 未钉组件核对 → E_NO_PIN；无钉可改 → E_NO_PIN。
    if vp.check(0xBEEF, v_fcitx5).is_ok() {
        return Err(E_NO_PIN);
    }
    if vp.amend(0xBEEF, v_fcitx5).is_ok() {
        return Err(E_NO_PIN);
    }
    if vp.basis_of(0xBEEF).is_ok() {
        return Err(E_NO_PIN);
    }
    hits += 1;

    // R5 偏差三段全覆盖：maj/min/pat 各漂一件、逐件告警落账路由齐全。
    let mut vp2 = VerPin::new();
    vp2.pin(0x21, pack_ver(5, 1, 12), BASIS_WAVE21, 3)?;
    vp2.pin(0x22, pack_ver(5, 1, 12), BASIS_WAVE21, 3)?;
    vp2.pin(0x23, pack_ver(5, 1, 12), BASIS_WAVE21, 3)?;
    if vp2.check(0x21, pack_ver(6, 1, 12)).is_ok() {
        return Err(E_DEVIATE);
    }
    if vp2.check(0x22, pack_ver(5, 2, 12)).is_ok() {
        return Err(E_DEVIATE);
    }
    if vp2.check(0x23, pack_ver(5, 1, 13)).is_ok() {
        return Err(E_DEVIATE);
    }
    if vp2.deviates != 3 || vp2.alarms.len() != 3 {
        return Err(E_STATE_BAD);
    }
    vp2.audit()?;
    hits += 1;

    // R6 无基线不钉：basis=0 → 拒收不入账（钉的是计划书承诺）。
    let mut vp3 = VerPin::new();
    if vp3.pin(0x31, pack_ver(1, 0, 0), 0, 1).is_ok() {
        return Err(E_STATE_BAD);
    }
    if vp3.pin_count() != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 告警账假告防线：审计发现「无偏而告」必报（告警账可信度）。
    let mut vp4 = VerPin::new();
    vp4.pin(0x41, pack_ver(1, 2, 3), BASIS_WAVE21, 5)?;
    vp4.check(0x41, pack_ver(1, 2, 3))?;
    vp4.alarms.push(AlarmRow {
        comp: 0x41,
        cur: pack_ver(1, 2, 3),
        pinned: pack_ver(1, 2, 3),
        route: 5,
    });
    if vp4.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零钉版不写回、空摘要拒、在册+非空摘要放行。
    let mut vp5 = VerPin::new();
    if vp5.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    vp5.pin(0x51, pack_ver(1, 0, 0), BASIS_WAVE21, 2)?;
    if vp5.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    vp5.writeback(true, 0x10)?;
    hits += 1;

    Ok(hits)
}
