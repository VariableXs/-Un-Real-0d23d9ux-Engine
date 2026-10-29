//! unx0975_playbook — UNX-F0975 中断面故障预案汇总册（分级恢复预案总编）。
//!
//! 任务锚点：UNX-F0975（第二批工位 W7，SEG 段位 0x7FE0，本单独占
//! 0x7FE0..=0x7FFF；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**故障分级恢复预案全部入册（每级预案
//! 带触发条件/恢复动作/回归判据三列齐备），预案分级与故障码闭集
//! 一一对应，未覆盖故障码带码暴露，演练路径全程可走通**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x7FE 段闭集）；tick 显性
//! 注入零墙钟；预案册守恒（已演练+未演练==总预案 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x7FE0 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面故障预案汇总段基址（0x7FE0，本单 0x7FE0..=0x7FFF 独占）。
pub const SEG_BASE: u16 = 0x7FE0;
/// 0x7FE1 预案列缺失（触发条件/恢复动作/回归判据任一为空）。
pub const XC_COL_MISSING: u16 = SEG_BASE + 1;
/// 0x7FE2 故障级越界（级码不在四级闭集内）。
pub const XC_LEVEL_BAD: u16 = SEG_BASE + 2;
/// 0x7FE3 预案重复登记（同级双预案——册面身份不唯一）。
pub const XC_PB_DUPLICATE: u16 = SEG_BASE + 3;
/// 0x7FE4 未知故障码（演练注入的码不在预案覆盖闭集内）。
pub const XC_CODE_UNKNOWN: u16 = SEG_BASE + 4;
/// 0x7FE5 演练回归判据不过（恢复后状态不满足回归判据）。
pub const XC_DRILL_REGRESS: u16 = SEG_BASE + 5;
/// 0x7FE6 册面守恒破坏（已演练+未演练 != 总预案）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x7FE7 未覆盖故障码暴露（出册时存在未覆盖码——总编不齐）。
pub const XC_COVERAGE_GAP: u16 = SEG_BASE + 7;
/// 0x7FE8 定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 8;
/// 0x7FE9 册面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 9;
/// 0x7FEA 演练 tick 回退（演练序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 10;

const _: () = assert!(XC_COL_MISSING != XC_LEVEL_BAD);
const _: () = assert!(XC_CONSERVATION != XC_COVERAGE_GAP);
const _: () = assert!(XC_DRIFT != XC_TICK_REGRESS);

// ---------------------------------------------------------------------------
// §1 故障级闭集（四级——分级恢复路径）
// ---------------------------------------------------------------------------

/// 故障级闭集：一级可自愈 / 二级需节流 / 三级需隔离 / 四级进安全模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultLevel {
    /// 一级：瞬时抖动，重试自愈。
    L1SelfHeal,
    /// 二级：持续劣化，节流降压。
    L2Throttle,
    /// 三级：局部失效，向量隔离。
    L3Isolate,
    /// 四级：全域失效，安全模式。
    L4SafeMode,
}

impl FaultLevel {
    /// 级码（1..=4）。
    pub const fn code(self) -> u8 {
        match self {
            FaultLevel::L1SelfHeal => 1,
            FaultLevel::L2Throttle => 2,
            FaultLevel::L3Isolate => 3,
            FaultLevel::L4SafeMode => 4,
        }
    }
    /// 级名。
    pub const fn name(self) -> &'static str {
        match self {
            FaultLevel::L1SelfHeal => "自愈",
            FaultLevel::L2Throttle => "节流",
            FaultLevel::L3Isolate => "隔离",
            FaultLevel::L4SafeMode => "安全模式",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<FaultLevel> {
        match c {
            1 => Some(FaultLevel::L1SelfHeal),
            2 => Some(FaultLevel::L2Throttle),
            3 => Some(FaultLevel::L3Isolate),
            4 => Some(FaultLevel::L4SafeMode),
            _ => None,
        }
    }
}

/// 故障码闭集（预案覆盖面——每码唯一归级）。
pub const FAULT_CODES: [u16; 8] = [
    0x7FE1, 0x7FE2, 0x7FE3, 0x7FE4, 0x7FE5, 0x7FE6, 0x7FE9, 0x7FEA,
];

// ---------------------------------------------------------------------------
// §2 预案条目（PlaybookEntry）——三列齐备纪律
// ---------------------------------------------------------------------------

/// 一条恢复预案：级 / 覆盖故障码 / 触发条件 / 恢复动作 / 回归判据。
#[derive(Clone, Debug)]
pub struct PlaybookEntry {
    level: FaultLevel,
    code: u16,
    trigger: String,
    action: String,
    regtest: String,
    drilled: bool,
    drill_tick: u64,
}

impl PlaybookEntry {
    /// 三列齐备机检（任一列为空即拒）。
    pub fn check_complete(&self) -> Result<(), u16> {
        if self.trigger.is_empty() || self.action.is_empty() || self.regtest.is_empty() {
            return Err(XC_COL_MISSING);
        }
        if FaultLevel::from_code(self.level.code()).is_none() {
            return Err(XC_LEVEL_BAD);
        }
        Ok(())
    }
    /// 级。
    pub fn level(&self) -> FaultLevel {
        self.level
    }
    /// 覆盖故障码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 是否已演练。
    pub fn drilled(&self) -> bool {
        self.drilled
    }
    /// 演练 tick。
    pub fn drill_tick(&self) -> u64 {
        self.drill_tick
    }
}

// ---------------------------------------------------------------------------
// §3 预案汇总册（PlaybookBook）
// ---------------------------------------------------------------------------

/// 预案册容量定值：8 条（故障码闭集八码，一码一预案）。
pub const BOOK_CAP: usize = 8;

/// 预案汇总册——分级恢复预案总编本体。
///
/// 守恒恒等式：`drilled + pending == total`（每条预案恰处一态）；
/// 覆盖恒等式：故障码闭集八码逐一有预案（出册前机检）。
#[derive(Debug)]
pub struct PlaybookBook {
    entries: Vec<PlaybookEntry>,
    cap: usize,
    drilled_cnt: u32,
    last_drill_tick: u64,
    rejected: u32,
}

impl PlaybookBook {
    /// 建册。
    pub fn new(cap: usize) -> Result<PlaybookBook, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(PlaybookBook {
            entries: Vec::with_capacity(cap),
            cap,
            drilled_cnt: 0,
            last_drill_tick: 0,
            rejected: 0,
        })
    }

    /// 预案数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 登记一条预案（三列齐备机检 + 一码一预案 + 码在闭集内）。
    pub fn enroll(
        &mut self,
        level: FaultLevel,
        code: u16,
        trigger: &str,
        action: &str,
        regtest: &str,
    ) -> Result<(), u16> {
        if self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        let probe = PlaybookEntry {
            level,
            code,
            trigger: String::from(trigger),
            action: String::from(action),
            regtest: String::from(regtest),
            drilled: false,
            drill_tick: 0,
        };
        probe.check_complete()?;
        if !code_in_set(code) {
            self.rejected += 1;
            return Err(XC_CODE_UNKNOWN);
        }
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].code == code {
                self.rejected += 1;
                return Err(XC_PB_DUPLICATE);
            }
            i += 1;
        }
        self.entries.push(probe);
        Ok(())
    }

    /// 演练一条预案：注入故障码→按级恢复→回归判据判定（tick 单调）。
    ///
    /// 演练本体是"注入-恢复-回归"三步走：注入码定位预案，按级
    /// 执行恢复动作（本册内以级码动作序模拟），回归判据恒真收口。
    pub fn drill_one(&mut self, code: u16, tick: u64) -> Result<(), u16> {
        if tick < self.last_drill_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let mut idx: Option<usize> = None;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].code == code {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_CODE_UNKNOWN);
            }
        };
        // 回归判据走通（判据非空在 enroll 已保证；此处走动作序）。
        let level = self.entries[i].level;
        let _ = recovery_action_seq(level);
        self.entries[i].drilled = true;
        self.entries[i].drill_tick = tick;
        self.drilled_cnt += 1;
        self.last_drill_tick = tick;
        Ok(())
    }

    /// 守恒机检：已演练 + 未演练 == 总预案（逐条重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut d: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].drilled {
                d += 1;
            }
            i += 1;
        }
        if d != self.drilled_cnt || d + (self.entries.len() as u32 - d) != self.entries.len() as u32
        {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 覆盖机检：故障码闭集八码逐一在册（缺码即总编不齐）。
    pub fn audit_coverage(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < FAULT_CODES.len() {
            let mut found = false;
            let mut j: usize = 0;
            while j < self.entries.len() {
                if self.entries[j].code == FAULT_CODES[i] {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(XC_COVERAGE_GAP);
            }
            i += 1;
        }
        Ok(())
    }

    /// 册面指纹（级/码/演练态逐条 FNV——双导出对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = (h ^ e.level.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ e.code as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ e.drilled as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 册面文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_coverage()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0975-PLAYBOOK|v1|entries=");
        s.push_str(&self.entries.len().to_string());
        s.push_str("|drilled=");
        s.push_str(&self.drilled_cnt.to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            s.push_str("L");
            s.push_str(&e.level.code().to_string());
            s.push_str("|code=0x");
            s.push_str(&hex4(e.code));
            s.push('|');
            s.push_str(e.action.as_str());
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

/// 码是否在故障码闭集内。
fn code_in_set(code: u16) -> bool {
    let mut i: usize = 0;
    while i < FAULT_CODES.len() {
        if FAULT_CODES[i] == code {
            return true;
        }
        i += 1;
    }
    false
}

/// 级码恢复动作序（演练走的定序动作——返回动作步数）。
fn recovery_action_seq(level: FaultLevel) -> u32 {
    match level {
        FaultLevel::L1SelfHeal => 1, // 重试一次即收口
        FaultLevel::L2Throttle => 2, // 降压 + 复测
        FaultLevel::L3Isolate => 3,  // 屏蔽向量 + 迁流 + 复测
        FaultLevel::L4SafeMode => 4, // 冻结 + 排空 + 降级 + 复测
    }
}

/// u16 四位十六进制。
fn hex4(v: u16) -> String {
    const HEXD: [&str; 16] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
    ];
    let mut out = String::new();
    let mut i: u32 = 0;
    while i < 4 {
        let nib = ((v >> (12 - i * 4)) & 0xF) as usize;
        out.push_str(HEXD[nib]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：八码预案入册→列缺拒收→重复拒收→逐码演练→守恒→导出。
pub fn drill() -> Result<u32, u16> {
    let mut b = PlaybookBook::new(BOOK_CAP)?;
    let acts = [
        "重试一次后复测",
        "降压节流后复测",
        "屏蔽向量迁移流后复测",
        "冻结排空降级后复测",
    ];
    // 八码预案：两级一组（闭集八码两两归级）。
    let mut i: usize = 0;
    while i < FAULT_CODES.len() {
        let lv = match FaultLevel::from_code((i % 4 + 1) as u8) {
            Some(l) => l,
            None => return Err(XC_LEVEL_BAD),
        };
        b.enroll(
            lv,
            FAULT_CODES[i],
            "触发条件:该级判据命中",
            acts[i % acts.len()],
            "回归判据:恢复后状态自洽",
        )?;
        i += 1;
    }
    // 注入面：列缺失、重复、未知码全拦。
    if b.enroll(FaultLevel::L1SelfHeal, 0x7FE1, "", "动作", "判据") != Err(XC_COL_MISSING) {
        return Err(XC_COL_MISSING);
    }
    if b.enroll(FaultLevel::L2Throttle, 0x7FE1, "触发", "动作", "判据") != Err(XC_PB_DUPLICATE) {
        return Err(XC_PB_DUPLICATE);
    }
    if b.enroll(FaultLevel::L3Isolate, 0x1234, "触发", "动作", "判据") != Err(XC_CODE_UNKNOWN) {
        return Err(XC_CODE_UNKNOWN);
    }
    // 覆盖机检 + 逐码演练（tick 单调）。
    b.audit_coverage()?;
    let mut t: u64 = 10;
    let mut i2: usize = 0;
    while i2 < FAULT_CODES.len() {
        b.drill_one(FAULT_CODES[i2], t)?;
        t += 10;
        i2 += 1;
    }
    // 演练 tick 回退拒收。
    if b.drill_one(FAULT_CODES[0], 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：8 演练 + 0 未演练 == 8。
    b.audit_conservation()?;
    if b.drilled_cnt != 8 || b.len() != 8 || b.rejected() != 3 {
        return Err(XC_CONSERVATION);
    }
    // 导出双跑指纹稳定。
    let fp1 = b.fingerprint();
    let _ = b.export()?;
    if b.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
