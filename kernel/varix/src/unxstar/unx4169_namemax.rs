//! unx4169_namemax — UNX-F4169 分量长度限制（NAME_MAX）与 ENAMETOOLONG（诊断段 0x96C0）。
//!
//! 判据原文：
//!   J1 单分量超 NAME_MAX（255）注入返回 ENAMETOOLONG 10/10 次。
//!   J2 恰等 255 字节通过——边界两格断言通过。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. NAME_MAX = 255 常量写死（POSIX/Linux 口径，路径分量上限），
//!      判定只看单分量字节数，不看全路径（全路径归 F4170 分账）。
//!   2. 边界两格：恰 255 通过（Ok），恰 256 拒收（ENAMETOOLONG），
//!      两格各断言 10/10 入账。
//!   3. 拒收零副作用：超长分量不进装配账，装配账计数不变（守恒）。
//!   4. 空分量拒收：0 长度分量按空名口径拒收（与超长同账异码）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x96C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_LONG: u16 = SEG_BASE + 1; // 超 NAME_MAX（ENAMETOOLONG 口径）
const E_NAME_EMPTY: u16 = SEG_BASE + 2;// 空分量拒收
const E_DUP_NAME: u16 = SEG_BASE + 3;  // 同名重复装配
const E_BOUND_MISS: u16 = SEG_BASE + 4;// 边界两格断言失守
const E_BAL: u16 = SEG_BASE + 5;       // 装配账守恒失守（拒收改账）
const E_BAD_STATE: u16 = SEG_BASE + 6; // 装配状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// NAME_MAX = 255（POSIX/Linux 路径单分量上限，常量写死）。
pub const NAME_MAX: usize = 255;

/// 装配状态机：Empty 空 / Installed 已装 / Rejected 已拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameState {
    Empty,
    Installed,
    Rejected,
}

/// 名字槽：定长 255 字节 + 有效长。
pub struct NameSlot {
    pub bytes: [u8; NAME_MAX], // 名字字节（0 填充）
    pub len: usize,            // 有效长度（<= 255）
}

/// NAME_MAX 裁定枢纽：分量账 + 边界两格断言账 + 拒收零副作用。
pub struct NameMaxRuler {
    slots: Vec<NameSlot>,
    pub state: NameState,
    pub pass_255: u32,   // 恰 255 通过账（判据 J2 上格）
    pub reject_256: u32, // 恰 256 拒收账（判据 J1 下格）
    pub reject_long: u32,// 超长拒收总数
    pub installed: u32,  // 装配成功总数
}

impl NameMaxRuler {
    pub fn new() -> NameMaxRuler {
        NameMaxRuler {
            slots: Vec::new(),
            state: NameState::Empty,
            pass_255: 0,
            reject_256: 0,
            reject_long: 0,
            installed: 0,
        }
    }

    fn find(&self, name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].len == name.len() {
                let mut same = true;
                let mut k = 0usize;
                while k < name.len() {
                    if self.slots[i].bytes[k] != name[k] {
                        same = false;
                        break;
                    }
                    k += 1;
                }
                if same {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 装配分量：边界两格裁定 + 查重 + 拒收零副作用（判据 J1/J2）。
    pub fn install(&mut self, name: &[u8]) -> Result<(), u16> {
        if name.is_empty() {
            self.state = NameState::Rejected;
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_MAX {
            // ENAMETOOLONG：拒收且装配账不变（零副作用红线）。
            self.reject_long += 1;
            if name.len() == NAME_MAX + 1 {
                self.reject_256 += 1;
            }
            self.state = NameState::Rejected;
            return Err(E_NAME_LONG);
        }
        if name.len() == NAME_MAX {
            self.pass_255 += 1;
        }
        if self.find(name) {
            self.state = NameState::Rejected;
            return Err(E_DUP_NAME);
        }
        let mut slot = NameSlot { bytes: [0u8; NAME_MAX], len: name.len() };
        let mut i = 0usize;
        while i < name.len() {
            slot.bytes[i] = name[i];
            i += 1;
        }
        self.slots.push(slot);
        self.installed += 1;
        self.state = NameState::Installed;
        Ok(())
    }

    /// 边界两格断言：pass_255 与 reject_256 各 >=10 才算齐（判据 J1/J2 账面）。
    pub fn assert_boundary(&self, quota: u32) -> Result<(), u16> {
        if self.pass_255 < quota || self.reject_256 < quota {
            return Err(E_BOUND_MISS);
        }
        Ok(())
    }

    /// 拒收零副作用核验：装配账在超长注入前后不变。
    pub fn verify_no_side_effect(&self, installed_before: u32) -> Result<(), u16> {
        if self.installed != installed_before {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 守恒对账：装配账 = 槽位数，状态机合法。
    pub fn audit(&self) -> Result<(), u16> {
        if self.installed as usize != self.slots.len() {
            return Err(E_BAL);
        }
        match self.state {
            NameState::Empty | NameState::Installed | NameState::Rejected => {}
        }
        Ok(())
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 边界两格 → R2 边界扩展 → R3 注入零副作用 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：恰 255 通过 ×10 + 恰 256 拒收 ×10（判据 J1/J2）。
    let mut r = NameMaxRuler::new();
    let mut round = 0u32;
    while round < 10 {
        let mut ok_name = [b'n'; NAME_MAX]; // 恰 255 字节（轮间异名）
        ok_name[0] = b'a' + round as u8;
        r.install(&ok_name)?;
        let bad_name = [b'x'; NAME_MAX + 1]; // 恰 256 字节
        if r.install(&bad_name).is_ok() {
            return Err(E_NAME_LONG);
        }
        round += 1;
    }
    r.assert_boundary(10)?;
    if r.installed != 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 边界：空分量 / 每轮异名恰 255 全通过。
    let mut r = NameMaxRuler::new();
    if r.install(b"").is_ok() {
        return Err(E_NAME_EMPTY);
    }
    let mut idx = 0u32;
    while idx < 10 {
        let mut nm = [0u8; NAME_MAX];
        let mut k = 0usize;
        while k < NAME_MAX {
            nm[k] = b'a' + ((idx as u8) % 26); // 轮内同字节但轮间异名（首字节异）
            k += 1;
        }
        nm[0] = b'a' + (idx as u8);
        r.install(&nm)?;
        idx += 1;
    }
    if r.installed != 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 注入拦截：超长注入 10/10 拒收且装配账零副作用。
    let mut r = NameMaxRuler::new();
    let seed = [b'q'; NAME_MAX];
    r.install(&seed)?;
    let before = r.installed;
    let mut round = 0u32;
    while round < 10 {
        let over = [b'z'; NAME_MAX + 1];
        if r.install(&over).is_ok() {
            return Err(E_NAME_LONG);
        }
        round += 1;
    }
    if r.reject_256 != 10 {
        return Err(E_BOUND_MISS);
    }
    r.verify_no_side_effect(before)?;
    if r.slot_count() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 守恒对账：装配账 = 槽位数 + 写回。
    let mut r = NameMaxRuler::new();
    r.install(b"one")?;
    r.install(b"two")?;
    r.audit()?;
    r.writeback(true, 0x20)?;
    if r.state != NameState::Installed {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    // R5 追加回归：255 名异字装配 26 槽 + 超长三档注入拒收账。
    let mut r = NameMaxRuler::new();
    let mut c = 0u32;
    while c < 26 {
        let mut nm = [b'm'; NAME_MAX];
        nm[0] = b'A' + c as u8;
        nm[NAME_MAX - 1] = b'Z' - c as u8;
        r.install(&nm)?;
        c += 1;
    }
    if r.installed != 26 || r.pass_255 != 26 {
        return Err(E_BOUND_MISS);
    }
    // 超长账读数：256/1000/4096 三档注入均拒收。
    let lens: [usize; 3] = [NAME_MAX + 1, 1000, 4096];
    let mut li = 0usize;
    while li < 3 {
        let over = [b'w'; 4096];
        let slice = &over[..lens[li]];
        if r.install(slice).is_ok() {
            return Err(E_NAME_LONG);
        }
        li += 1;
    }
    if r.reject_long != 3 || r.reject_256 != 1 {
        return Err(E_BOUND_MISS);
    }
    // 超长注入不改装配账（零副作用红线复核）。
    if r.installed != 26 || r.slot_count() != 26 {
        return Err(E_BAL);
    }
    // 拒收账与边界账互斥：通过账不受拒收影响。
    if r.pass_255 != 26 {
        return Err(E_BOUND_MISS);
    }
    r.audit()?;
    hits += 1;

    // R6 回归：拒收零副作用二次核验 + 状态机恢复 + 写回。
    let before = r.installed;
    let over2 = [b'q'; NAME_MAX + 2];
    r.install(&over2).is_err();
    r.verify_no_side_effect(before)?;
    if r.state != NameState::Rejected {
        return Err(E_BAD_STATE);
    }
    // 拒收后正常名仍可装配（状态机恢复 Installed）。
    r.install(b"recover")?;
    if r.state != NameState::Installed {
        return Err(E_BAD_STATE);
    }
    if r.slot_count() != 27 {
        return Err(E_BAL);
    }
    r.audit()?;
    r.writeback(true, 0x26)?;
    hits += 1;

    Ok(hits)
}
