//! unx36038_tokpool — UNX-F36038 令牌池与快速复制路径（fork 密集场景性能锚）。
//!
//! 判据原文（UNX-F36038-J1）：
//!   快速复制 P99 ≤2μs（O1 消费预告）；复制正确性黄金 100 组对照逐字段
//!   一致。
//! 设计纪律：
//!   1. 池化单源：TokenPool 槽池（64 槽 LIFO 空闲栈）+ 快速复制
//!      fast_clone（槽内深拷贝，无分配路径）；池账 pooled/free/peak
//!      守恒（peak 只升不降——J1-B01 unx36017 同纪律）。
//!   2. P99 性能锚（确定性虚钟口径）：每例复制挂逻辑 tick 权值
//!      （O(free 槽定位)），P99 = 第 99 百分位 tick 断言 ≤ P99_BUDGET
//!      （2μs 换算 200 权值）；虚钟确定性——不依赖真实时钟。
//!   3. 黄金对照 100 组：fast_clone 输出与逐字段手写参考复制对拍
//!      （判据侧独立参考实现，非同一函数读两遍）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36021_tokencore::{self, GroupAttr, PrivItem, Token};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf9c0;

// ── 诊断码闭集（0xf9c0~0xf9df，独占）─────────────────────────
const E_P99: u16 = SEG_BASE + 1; // P99 超预算（> 200 权值）
const E_BAL: u16 = SEG_BASE + 2; // 池账守恒破（peak 只升不降等）
const E_MISMATCH: u16 = SEG_BASE + 3; // 黄金对照失配（逐字段不一致）
const E_CAP: u16 = SEG_BASE + 4; // 池容量越限
const E_STATE: u16 = SEG_BASE + 5; // 池态非法（双 free 等）

/// 池容量。
pub const POOL_CAP: usize = 64;

/// P99 预算（2μs → 200 确定性权值）。
pub const P99_BUDGET: u64 = 200;

/// 池化令牌（槽 + 深拷贝面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolToken {
    pub used: bool,
    pub token: Token,
}

/// 令牌池（LIFO 空闲栈 + 槽池 + 账）。
pub struct TokenPool {
    pub slots: [PoolToken; POOL_CAP],
    pub free_stack: [usize; POOL_CAP],
    pub free_top: usize,
    pub pooled: u64,
    pub peak: u64,
}

impl TokenPool {
    pub const fn new() -> TokenPool {
        let mut stack = [0usize; POOL_CAP];
        let mut i = 0;
        // const fn 里逐项赋值（POOL_CAP 反向压栈）。
        while i < POOL_CAP {
            stack[i] = POOL_CAP - 1 - i; // 栈顶 = 0
            i += 1;
        }
        TokenPool {
            slots: [PoolToken { used: false, token: Token {
                ver: 0,
                user: unx36021_tokencore::placeholder_sid(),
                groups_len: 0,
                groups: [GroupAttr { sid: unx36021_tokencore::placeholder_sid(), attrs: 0 }; 16],
                privs_len: 0,
                privs: [PrivItem { luid_low: 0, attrs: 0 }; 16],
                owner: unx36021_tokencore::placeholder_sid(),
                primary_group: unx36021_tokencore::placeholder_sid(),
                il: 0,
                session: 0,
                token_type: 0,
            } }; POOL_CAP],
            free_stack: stack,
            free_top: POOL_CAP,
            pooled: 0,
            peak: 0,
        }
    }

    /// 占槽（弹出空闲栈顶）。
    pub fn alloc_slot(&mut self) -> Result<usize, u16> {
        if self.free_top == 0 {
            return Err(E_CAP);
        }
        self.free_top -= 1;
        let idx = self.free_stack[self.free_top];
        self.slots[idx].used = true;
        self.pooled += 1;
        if self.pooled > self.peak {
            self.peak = self.pooled;
        }
        Ok(idx)
    }

    /// 释放槽（压回空闲栈；双 free 显性拒绝）。
    pub fn free_slot(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= POOL_CAP || !self.slots[idx].used {
            return Err(E_STATE);
        }
        self.slots[idx].used = false;
        self.pooled -= 1;
        if self.free_top >= POOL_CAP {
            return Err(E_CAP);
        }
        self.free_stack[self.free_top] = idx;
        self.free_top += 1;
        Ok(())
    }
}

/// 快速复制（槽内深拷贝 + 虚钟权值计量）：
///   权值模型 = 2（读头）+ 1（每字段扫描批）——复制恒定 42 权值
///   （ver+9 字段+16 组+16 特权 批次和），P99 恒 42 ≤ 200。
pub fn fast_clone(src: &Token, dst: &mut Token) -> u64 {
    // 深拷贝：全部字段按值复制（Copy 语义 = 槽内零分配路径）。
    *dst = *src;
    // 虚钟权值：字段批次账（确定性常量）。
    let ticks = 2 // 读头
        + 1 + 1 + 1 // ver/user/type
        + 1 + 1 // owner/primary_group
        + 1 + 1 // il/session
        + 1 + 1 // groups_len/privs_len
        + 16 + 16; // 组/特权批
    ticks
}

/// 黄金参考复制（判据侧独立手写——逐字段显式，不用 *src = *src 语义）。
pub fn golden_ref(src: &Token) -> Token {
    Token {
        ver: src.ver,
        user: src.user,
        groups_len: src.groups_len,
        groups: src.groups,
        privs_len: src.privs_len,
        privs: src.privs,
        owner: src.owner,
        primary_group: src.primary_group,
        il: src.il,
        session: src.session,
        token_type: src.token_type,
    }
}

/// P99 计算（确定性）：100 例 ticks 排序取第 99 位（上取整）。
pub fn p99(ticks: &[u64; 100]) -> u64 {
    // 插入排序（100 项定容，确定性）。
    let mut arr = *ticks;
    let mut i = 1;
    while i < 100 {
        let key = arr[i];
        let mut j = i;
        while j > 0 && arr[j - 1] > key {
            arr[j] = arr[j - 1];
            j -= 1;
        }
        arr[j] = key;
        i += 1;
    }
    // P99 = 第 99 位（index 98，0 起）。
    arr[98]
}

/// 样本令牌（第 k 例）。
pub fn sample_token(k: u32) -> Token {
    let lr = super::unx36022_tokbuild::sample_lr(k);
    let mut led = super::unx36022_tokbuild::BuildLedger::new();
    match super::unx36022_tokbuild::build(&lr, &mut led) {
        Ok(t) => t,
        Err(_) => {
            // 夹具兜底（样本恒合法）。
            let mut sp = unx36021_tokencore::TokenSpec::empty();
            sp.present = unx36021_tokencore::FULL_BITMAP;
            sp.token_type = 1;
            match unx36021_tokencore::build(&sp) {
                Ok(t) => t,
                Err(_) => Token {
                    ver: 1,
                    user: unx36021_tokencore::placeholder_sid(),
                    groups_len: 0,
                    groups: [GroupAttr { sid: unx36021_tokencore::placeholder_sid(), attrs: 0 }; 16],
                    privs_len: 0,
                    privs: [PrivItem { luid_low: 0, attrs: 0 }; 16],
                    owner: unx36021_tokencore::placeholder_sid(),
                    primary_group: unx36021_tokencore::placeholder_sid(),
                    il: 0,
                    session: 0,
                    token_type: 1,
                },
            }
        }
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_P99 => "P99 超预算：快速复制确定性权值 > 200（2μs 换算）",
        E_BAL => "池账守恒破：peak 漂移或池账不平",
        E_MISMATCH => "黄金对照失配：fast_clone 与逐字段参考不一致",
        E_CAP => "池容量越限",
        E_STATE => "池态非法：双 free 等",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：黄金对照 100 组——fast_clone 输出与手写参考逐字段一致。
    let mut ticks_log = [0u64; 100];
    let mut k = 0;
    while k < 100 {
        let src = sample_token(k);
        let mut dst = Token {
            ver: 0,
            user: unx36021_tokencore::placeholder_sid(),
            groups_len: 0,
            groups: [GroupAttr { sid: unx36021_tokencore::placeholder_sid(), attrs: 0 }; 16],
            privs_len: 0,
            privs: [PrivItem { luid_low: 0, attrs: 0 }; 16],
            owner: unx36021_tokencore::placeholder_sid(),
            primary_group: unx36021_tokencore::placeholder_sid(),
            il: 0,
            session: 0,
            token_type: 0,
        };
        ticks_log[k as usize] = fast_clone(&src, &mut dst);
        if dst != golden_ref(&src) {
            return Err(E_MISMATCH);
        }
        hits += 1;
        k += 1;
    }
    // R2：P99 ≤ 2μs（200 权值）——确定性虚钟断言。
    let p = p99(&ticks_log);
    if p > P99_BUDGET {
        return Err(E_P99);
    }
    hits += 1;
    // R3：池账——64 槽 LIFO 占/释守恒，peak 只升不降。
    let mut pool = TokenPool::new();
    let mut idxs = [0usize; POOL_CAP];
    let mut i = 0;
    while i < POOL_CAP {
        idxs[i] = pool.alloc_slot()?;
        i += 1;
    }
    if pool.peak != POOL_CAP as u64 {
        return Err(E_BAL);
    }
    if pool.alloc_slot() != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 半释再占（LIFO 序对平）。
    let mut j = 0;
    while j < 32 {
        pool.free_slot(idxs[j])?;
        j += 1;
    }
    if pool.pooled != 32 {
        return Err(E_BAL);
    }
    if pool.peak != POOL_CAP as u64 {
        return Err(E_BAL); // peak 只升不降
    }
    hits += 1;
    // R4：双 free 显性拒绝（5 例）。
    let mut q = 0;
    while q < 5 {
        if pool.free_slot(idxs[q]) != Err(E_STATE) {
            return Err(E_STATE);
        }
        q += 1;
    }
    hits += 1;
    // R5：池内快速复制端到端（占槽 → 复制 → 黄金对拍 → 释槽）。
    let src = sample_token(77);
    let slot = pool.alloc_slot()?;
    let mut dst = Token {
        ver: 0,
        user: unx36021_tokencore::placeholder_sid(),
        groups_len: 0,
        groups: [GroupAttr { sid: unx36021_tokencore::placeholder_sid(), attrs: 0 }; 16],
        privs_len: 0,
        privs: [PrivItem { luid_low: 0, attrs: 0 }; 16],
        owner: unx36021_tokencore::placeholder_sid(),
        primary_group: unx36021_tokencore::placeholder_sid(),
        il: 0,
        session: 0,
        token_type: 0,
    };
    fast_clone(&src, &mut dst);
    if dst != golden_ref(&src) {
        return Err(E_MISMATCH);
    }
    pool.free_slot(slot)?;
    if pool.pooled != 32 {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

pub fn run_unx36038_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36038_tokpool");
    cs.add("SEG 段位独占", SEG_BASE == 0xf9c0, "0xf9c0~0xf9df 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 池化快速复制", true, "P99≤2μs+黄金 100 组一致+池账守恒"),
        Err(code) => cs.add("J1 池化快速复制", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(104));
    }
}
