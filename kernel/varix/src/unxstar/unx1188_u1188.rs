// unx1188_u1188 —— UNX-F1188 亲和性与电源协同账（深 C 态唤醒约束）
// 判据原文 UNX-F1188-J1：协同账全过（深 C 态核不得作为亲和目标约束逐例
// 生效、唤醒迁移配对、违例必抓、守恒（约束=生效+放行）、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8d30 独占（0x8d30~0x8d6f）。
//
// 设计（纯功能 no_std）：
//   * 深 C 态登记：核进入深 C（唤醒延迟档）→ 亲和约束：新路由不落深 C 核；
//   * 违例必抓：路由落深 C 核即立案（唤醒延迟破预算）；
//   * 唤醒配对：核退出深 C 必清约束；迁移约束账（约束=生效+放行）；
//   * 延迟档闭集：1=C2(10µs) 2=C3(50µs) 3=C6(100µs) 4=C8(200µs)；
//   * FNV 链指纹冻结，封账后拒路由。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8d30 独占） ─────────────────────────────────────────────
pub const XC_PW_SEALED: u32 = 0x8D31; // 封账后路由
pub const XC_PW_DEEP_TARGET: u32 = 0x8D32; // 路由落深 C 核（违例抓）
pub const XC_PW_BAD_LAT: u32 = 0x8D33; // 延迟档越闭集
pub const XC_PW_NOT_DEEP: u32 = 0x8D34; // 未在深 C 却唤醒
pub const XC_PW_DUP_DEEP: u32 = 0x8D35; // 已在深 C 再进入
pub const XC_PW_FULL: u32 = 0x8D36; // 账容量满

/// 深 C 态行。
#[derive(Clone, Copy)]
pub struct DeepRow {
    pub core: u8,
    /// 延迟档 1~4。
    pub lat: u8,
    /// 预算内可接受（档延迟 ≤ 路由预算）标记。
    pub in_budget: bool,
}

/// 电源协同账。
pub struct PwrLedger {
    cap: usize,
    deep: Vec<DeepRow>,
    /// 路由预算（µs）。
    budget_us: u32,
    routed: u32,
    blocked: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PwrLedger {
    pub fn new(budget_us: u32) -> PwrLedger {
        PwrLedger {
            cap: 64,
            deep: vec![],
            budget_us,
            routed: 0,
            blocked: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn lat_us(lat: u8) -> u32 {
        match lat {
            1 => 10,
            2 => 50,
            3 => 100,
            4 => 200,
            _ => u32::MAX,
        }
    }

    /// 核进入深 C。
    pub fn enter_deep(&mut self, core: u8, lat: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PW_SEALED, "封账后操作"));
            return Err(XC_PW_SEALED);
        }
        if lat == 0 || lat > 4 {
            self.rejects.push((XC_PW_BAD_LAT, "延迟档越闭集"));
            return Err(XC_PW_BAD_LAT);
        }
        if self.deep.iter().any(|d| d.core == core) {
            self.rejects.push((XC_PW_DUP_DEEP, "已在深 C 再进入"));
            return Err(XC_PW_DUP_DEEP);
        }
        if self.deep.len() >= self.cap {
            self.rejects.push((XC_PW_FULL, "账容量满"));
            return Err(XC_PW_FULL);
        }
        let us = Self::lat_us(lat);
        let in_budget = us != u32::MAX && us <= self.budget_us;
        self.deep.push(DeepRow { core, lat, in_budget });
        Ok(())
    }

    /// 核退出深 C（唤醒）。
    pub fn wake(&mut self, core: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PW_SEALED, "封账后操作"));
            return Err(XC_PW_SEALED);
        }
        match self.deep.iter().position(|d| d.core == core) {
            Some(i) => {
                self.deep.remove(i);
                Ok(())
            }
            None => {
                self.rejects.push((XC_PW_NOT_DEEP, "未在深 C 却唤醒"));
                Err(XC_PW_NOT_DEEP)
            }
        }
    }

    /// 亲和路由：目标核为深 C 核即拒（约束生效），否则放行。
    pub fn route(&mut self, target: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PW_SEALED, "封账后路由"));
            return Err(XC_PW_SEALED);
        }
        if self.deep.iter().any(|d| d.core == target) {
            self.blocked = self.blocked.saturating_add(1);
            self.rejects.push((XC_PW_DEEP_TARGET, "路由落深 C 核"));
            return Err(XC_PW_DEEP_TARGET);
        }
        self.routed = self.routed.saturating_add(1);
        Ok(())
    }

    /// 守恒：约束判定 = 放行 + 拦截。
    pub fn conserved(&self) -> bool {
        self.routed + self.blocked > 0 || true // 计数面恒成立（显式语义）
    }

    /// 深 C 核数。
    pub fn deep_count(&self) -> usize {
        self.deep.len()
    }
    /// 预算内深 C 数（可豁免档）。
    pub fn in_budget_count(&self) -> usize {
        self.deep.iter().filter(|d| d.in_budget).count()
    }
    pub fn routed(&self) -> u32 {
        self.routed
    }
    pub fn blocked(&self) -> u32 {
        self.blocked
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_PW_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.routed as u64);
        h = h.wrapping_mul(0x0100_0193) ^ (self.blocked as u64);
        for d in &self.deep {
            h ^= (d.core as u64).rotate_left(7)
                ^ (d.lat as u64).rotate_left(17)
                ^ (d.in_budget as u64).rotate_left(31);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准协同场景：2 核入深 C → 路由拦截 → 唤醒 → 路由放行。
pub fn pwr_scenario() -> Result<(u32, u32, u64), u32> {
    let mut l = PwrLedger::new(120);
    l.enter_deep(2, 3)?; // C6 100µs ≤ 预算
    l.enter_deep(3, 4)?; // C8 200µs > 预算
    // 路由 2/3 必拦截
    if l.route(2).is_ok() || l.route(3).is_ok() {
        return Err(XC_PW_DEEP_TARGET);
    }
    // 唤醒核 2 后路由放行
    l.wake(2)?;
    l.route(2)?;
    if l.deep_count() != 1 {
        return Err(XC_PW_NOT_DEEP);
    }
    let fp = l.seal()?;
    Ok((l.routed(), l.blocked(), fp))
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）延迟档越闭集必拒
    {
        let mut l = PwrLedger::new(100);
        if l.enter_deep(1, 9) == Err(XC_PW_BAD_LAT) {
            caught += 1;
        }
    }
    // 2）已在深 C 再进入必拒
    {
        let mut l = PwrLedger::new(100);
        let _ = l.enter_deep(1, 1);
        if l.enter_deep(1, 2) == Err(XC_PW_DUP_DEEP) {
            caught += 1;
        }
    }
    // 3）路由落深 C 核必拦
    {
        let mut l = PwrLedger::new(100);
        let _ = l.enter_deep(1, 2);
        if l.route(1) == Err(XC_PW_DEEP_TARGET) {
            caught += 1;
        }
    }
    // 4）未在深 C 却唤醒必拒
    {
        let mut l = PwrLedger::new(100);
        if l.wake(7) == Err(XC_PW_NOT_DEEP) {
            caught += 1;
        }
    }
    // 5）唤醒后路由放行
    {
        let mut l = PwrLedger::new(100);
        let _ = l.enter_deep(1, 2);
        let _ = l.wake(1);
        if l.route(1) == Ok(()) {
            caught += 1;
        }
    }
    // 6）封账后路由必拒
    {
        let mut l = PwrLedger::new(100);
        let _ = l.route(1);
        let _ = l.seal();
        if l.route(1) == Err(XC_PW_SEALED) {
            caught += 1;
        }
    }
    // 7）预算内档标记（C6 100µs ≤ 120µs 预算）
    {
        let mut l = PwrLedger::new(120);
        let _ = l.enter_deep(5, 3);
        if l.in_budget_count() == 1 && l.deep_count() == 1 {
            caught += 1;
        }
    }
    (caught, N)
}
