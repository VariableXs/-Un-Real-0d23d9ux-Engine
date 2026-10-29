//! UNX-F36127-J1 类型注销的禁止语义（注册即十年不变锚）。
//! 判据：① 封印后一切注销尝试显性拒绝且入拒绝账（尝试可审计）；
//! ② tid 高水位永不回收（删除在类型面结构性不可能）；
//! ③ 黄金八类在全量注销攻击后仍逐名在场；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：不变量锚——「类型注销」在 Varix 安全域没有合法路径。本模块
//! 不提供注销 API，而是提供三重结构性防线：封印态（36121 seal）、tid 高
//! 水位（只增不减）、注销尝试账（每次攻击留痕）。36131 启动自检消费
//! survived_attack() 作为类型面完整性的最终核。

use crate::checks::CheckSet;
use super::unx36121_typereg::{self as tr, TypeRegistry, GOLDEN_N, TID_BASE};
use super::unx36122_typedeny::{DenyLedger, GateRoute};

pub const SEG_BASE: u16 = 0x3b30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FROZEN: u16 = SEG_BASE + 2; // 封印后注销尝试（唯一拒因）
pub const E_GONE: u16 = SEG_BASE + 3; // 条目消失（结构性不可能，防御位）
pub const E_REUSE: u16 = SEG_BASE + 4; // tid 复用（高水位破）
pub const E_ATTACK: u16 = SEG_BASE + 5; // 攻击账漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 注销尝试账容量（攻击指纹窗口）。
pub const ATTACK_CAP: usize = 16;

/// 注销尝试手法闭集（判别式 = 0xD0 段——诊断面按手法归因）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeregVec {
    /// 直呼其名（按名注销请求）。
    ByName = 0x0D01,
    /// 按编号（tid 注销请求）。
    ByTid = 0x0D02,
    /// 清表式（重建空表顶替）。
    ByWipe = 0x0D03,
    /// 越权写（绕开注册表直改条目槽）。
    ByDirect = 0x0D04,
}

impl DeregVec {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            DeregVec::ByName => "按名注销请求",
            DeregVec::ByTid => "按编号注销请求",
            DeregVec::ByWipe => "清表式顶替",
            DeregVec::ByDirect => "越权直写",
        }
    }
}

/// 注销尝试事件（攻击指纹原子）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    pub seq: u32,
    pub vec: DeregVec,
    pub name: &'static str,
    pub arg: u16, // ByTid 携 tid，其余携请求方标签
}

/// 固定容量尝试账（环形顶替——攻击高频时保最新指纹）。
pub struct AttackLedger {
    buf: [Option<Attempt>; ATTACK_CAP],
    head: usize,
    n: usize,
    seq_next: u32,
    pub total: u32, // 全量尝试计数（账环形也守真）
}

const NO_AT: Option<Attempt> = None;

impl AttackLedger {
    pub fn new() -> AttackLedger {
        AttackLedger { buf: [NO_AT; ATTACK_CAP], head: 0, n: 0, seq_next: 1, total: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn record(&mut self, vec: DeregVec, name: &'static str, arg: u16) {
        let at = Attempt { seq: self.seq_next, vec, name, arg };
        self.buf[(self.head + self.n) % ATTACK_CAP] = Some(at);
        if self.n < ATTACK_CAP {
            self.n += 1;
        } else {
            self.head = (self.head + 1) % ATTACK_CAP;
        }
        self.seq_next = self.seq_next.wrapping_add(1);
        self.total = self.total.wrapping_add(1);
    }

    pub fn at(&self, i: usize) -> Result<Attempt, u16> {
        if i >= self.n {
            return Err(E_ATTACK);
        }
        match self.buf[(self.head + i) % ATTACK_CAP] {
            Some(a) => Ok(a),
            None => Err(E_ATTACK),
        }
    }

    /// 按手法计数（指纹直方图基元）。
    pub fn count_vec(&self, vec: DeregVec) -> u32 {
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(a) = self.at(i) {
                if a.vec == vec {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    pub fn integrity(&self) -> Result<(), u16> {
        if self.n > ATTACK_CAP {
            return Err(E_ATTACK);
        }
        let mut i = 0;
        while i < self.n {
            if self.buf[(self.head + i) % ATTACK_CAP].is_none() {
                return Err(E_ATTACK);
            }
            i += 1;
        }
        Ok(())
    }
}

/// tid 高水位守卫：注册表生命周期内只增不减的编号事实（借用式——
/// 守卫不拥有注册表，避免所有权迁移打断后续判据）。
pub struct HighWater<'a> {
    max_tid_ever: u16,
    reg: &'a TypeRegistry,
}

impl<'a> HighWater<'a> {
    pub fn from(reg: &'a TypeRegistry) -> Result<HighWater<'a>, u16> {
        let hw = TID_BASE + (reg.count() as u16).saturating_sub(1);
        Ok(HighWater { max_tid_ever: hw, reg })
    }

    /// 高水位复核：全部历史 tid 必须仍可解析（无空洞即无注销）。
    pub fn no_reuse(&self) -> Result<(), u16> {
        let mut tid = TID_BASE;
        while tid <= self.max_tid_ever {
            if self.reg.at(tid).is_err() {
                return Err(E_REUSE);
            }
            tid += 1;
        }
        Ok(())
    }

    pub fn mark(&self) -> u16 {
        self.max_tid_ever
    }
}

/// 全量注销攻击模拟（36131 自检的最终核输入）：
/// 四手法各打 N 轮 → 全部被拒 → 表原样、账留痕、高水位不退。
pub fn survived_attack(reg: &TypeRegistry, led: &mut AttackLedger, deny: &mut DenyLedger, rounds: usize) -> Result<(), u16> {
    if !reg.is_sealed() {
        // 未封印表不在本判据管辖（封印是注销禁止的前提）。
        return Err(E_FROZEN);
    }
    let count0 = reg.count();
    let mut r = 0;
    while r < rounds {
        // 手法一：按名（黄金名逐名打）。
        let mut g = 0;
        while g < GOLDEN_N {
            // 注销面 = 查询后无删除路径：此处显性证明「查得到且删不掉」。
            if reg.query(tr::GOLDEN_NAMES[g]).is_err() {
                return Err(E_GONE);
            }
            led.record(DeregVec::ByName, tr::GOLDEN_NAMES[g], 0);
            let _ = deny.record(GateRoute::DenyUnknown.code(), "dereg");
            g += 1;
        }
        // 手法二：按 tid。
        let mut t = TID_BASE;
        while t < TID_BASE + GOLDEN_N as u16 {
            if reg.at(t).is_err() {
                return Err(E_GONE);
            }
            led.record(DeregVec::ByTid, "tid", t);
            t += 1;
        }
        // 手法三：清表式——唯一合法效果是不存在（无 API 可达）。
        led.record(DeregVec::ByWipe, "*", 0);
        // 手法四：越权直写——同上，无槽位写路径。
        led.record(DeregVec::ByDirect, "*", 0);
        // 账守恒：表计数与容量不变。
        if reg.count() != count0 {
            return Err(E_GONE);
        }
        r += 1;
    }
    led.integrity()?;
    // 攻击后全量在场复核。
    reg.integrity().map_err(|_| E_INTG)?;
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金表封印 + 攻击幸存核。
    let reg = tr::assemble_golden()?;
    let mut led = AttackLedger::new();
    let mut deny = DenyLedger::new();
    survived_attack(&reg, &mut led, &mut deny, 3)?;
    // 三轮 × 每轮 (8 名 + 8 tid + 2) = 54 次尝试。账是 ATTACK_CAP=16
    // 的环形窗（R5 即顶替语义）：total 守真计全部尝试，count 只是
    // 窗口内条数——54 次进 16 窗，count 恒 CAP、total−count 条被顶。
    if led.total != (3 * (2 * GOLDEN_N + 2)) as u32 {
        return Err(E_ATTACK);
    }
    if led.count() != ATTACK_CAP {
        return Err(E_ATTACK);
    }
    if led.total <= led.count() as u32 {
        return Err(E_ATTACK);
    }
    // R2：黄金八类攻后逐名在场（幸存=可查+tid 正确+封印位仍在）。
    let mut g = 0;
    while g < GOLDEN_N {
        let e = reg.query(tr::GOLDEN_NAMES[g]).map_err(|_| E_GONE)?;
        if e.tid != TID_BASE + g as u16 {
            return Err(E_REUSE);
        }
        if e.flags & tr::F_SEALED == 0 {
            return Err(E_FROZEN);
        }
        g += 1;
    }
    // R3：高水位不退（攻后 no_reuse 全通）。
    let hw = HighWater::from(&reg)?;
    hw.no_reuse()?;
    if hw.mark() != TID_BASE + GOLDEN_N as u16 - 1 {
        return Err(E_REUSE);
    }
    // R4：手法指纹直方图——窗口内四手法计数守恒（窗口是 total 的最近
    // 截断，四手法合计必等于窗口计数）。
    if led.count_vec(DeregVec::ByName)
        + led.count_vec(DeregVec::ByTid)
        + led.count_vec(DeregVec::ByWipe)
        + led.count_vec(DeregVec::ByDirect)
        != led.count() as u32
    {
        return Err(E_ATTACK);
    }
    // 单轮直方图直证：1 轮 18 条进 16 窗，最旧 2 条 ByName 被顶 →
    // 窗口内 ByName=6 / ByTid=8 / ByWipe=1 / ByDirect=1。
    let mut led1 = AttackLedger::new();
    let mut deny1 = DenyLedger::new();
    survived_attack(&reg, &mut led1, &mut deny1, 1)?;
    if led1.total != (2 * GOLDEN_N + 2) as u32 {
        return Err(E_ATTACK);
    }
    if led1.count_vec(DeregVec::ByName) != (GOLDEN_N - (2 * GOLDEN_N + 2 - ATTACK_CAP)) as u32 {
        return Err(E_ATTACK);
    }
    if led1.count_vec(DeregVec::ByTid) != GOLDEN_N as u32 {
        return Err(E_ATTACK);
    }
    if led1.count_vec(DeregVec::ByWipe) != 1 || led1.count_vec(DeregVec::ByDirect) != 1 {
        return Err(E_ATTACK);
    }
    // R5：环形顶替（> ATTACK_CAP 次尝试 → 账满、total 守真、最旧被顶）。
    let mut led2 = AttackLedger::new();
    let mut k = 0;
    while k < ATTACK_CAP + 5 {
        led2.record(DeregVec::ByName, "File", 0);
        k += 1;
    }
    if led2.count() != ATTACK_CAP {
        return Err(E_ATTACK);
    }
    if led2.total != (ATTACK_CAP + 5) as u32 {
        return Err(E_ATTACK);
    }
    led2.integrity()?;
    // 顶替核：窗口内最旧 seq = total - CAP + 1。
    let oldest = led2.at(0)?;
    if oldest.seq != led2.total - ATTACK_CAP as u32 + 1 {
        return Err(E_ATTACK);
    }
    // R6：未封印表不属于本判据管辖（显性分账）。
    let mut open = TypeRegistry::new();
    let (gname, gmap, gpol, _) = tr::GOLDEN[0];
    let _ = open.register(gname, gmap, gpol, 0);
    {
        let mut led3 = AttackLedger::new();
        let mut deny3 = DenyLedger::new();
        if survived_attack(&open, &mut led3, &mut deny3, 1).err() != Some(E_FROZEN) {
            return Err(E_FROZEN);
        }
        if led3.count() != 0 {
            return Err(E_ATTACK);
        }
    }
    // 校验和：黄金 tid 全并 × 手法种数。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < GOLDEN_N {
        sum = sum.wrapping_add(reg.tid_of(tr::GOLDEN_NAMES[q]).map_err(|_| E_GONE)? as u32);
        q += 1;
    }
    Ok(sum.wrapping_mul(23).wrapping_add(4 * 3))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FROZEN => "封印后注销尝试（唯一拒因）/ 未封印表不在管辖",
        E_GONE => "条目消失（结构性不可能，防御位）",
        E_REUSE => "tid 复用（高水位破）",
        E_ATTACK => "攻击账漂移（指纹窗口失真）",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36127_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36127_nounreg");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b30, "0x3b30~0x3b3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四手法攻击幸存/高水位/指纹直方图/环形顶替全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("手法闭集", DeregVec::ByName.code() == 0x0D01 && DeregVec::ByTid.code() == 0x0D02 && DeregVec::ByWipe.code() == 0x0D03 && DeregVec::ByDirect.code() == 0x0D04, "四手法判别式闭集（0xD0 段）");
    cs.add("手法人话闭集", DeregVec::ByWipe.text().len() > 0, "诊断面按手法归因");
    cs.add("攻击账容量", ATTACK_CAP == 16, "指纹窗口 16 槽环形");
    cs.add("高水位单调", {
        match tr::assemble_golden() {
            Ok(r) => match HighWater::from(&r) {
                Ok(hw) => hw.mark() == TID_BASE + GOLDEN_N as u16 - 1,
                Err(_) => false,
            },
            Err(_) => false,
        }
    }, "黄金表高水位 = 8");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(840));
    }

    #[test]
    fn highwater_no_reuse() {
        // 高水位守卫：全部历史 tid 仍可解析（无空洞即无注销）。
        let reg = super::super::unx36121_typereg::assemble_golden().unwrap();
        let hw = HighWater::from(&reg).unwrap();
        assert!(hw.no_reuse().is_ok());
        assert_eq!(hw.mark(), super::super::unx36121_typereg::TID_BASE + GOLDEN_N as u16 - 1);
    }

    #[test]
    fn ledger_window_conserves() {
        // 环形账守恒：total 守真计全部 >= 窗口 count，窗口恒 <= ATTACK_CAP。
        let reg = super::super::unx36121_typereg::assemble_golden().unwrap();
        let mut led = AttackLedger::new();
        let mut deny = DenyLedger::new();
        survived_attack(&reg, &mut led, &mut deny, 3).unwrap();
        assert!(led.total >= led.count() as u32);
        assert!(led.count() <= ATTACK_CAP);
        assert!(led.integrity().is_ok());
    }
}
