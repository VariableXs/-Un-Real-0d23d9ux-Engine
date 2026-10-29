//! UNX-F0710 · 断言执行序冻结（域级执行序表）· 诊断段 0x58c0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0710] 断言执行序冻结（域级执行序表） <SEG 0x58c0>
//!   UNX-F0710-J1 域级执行序表冻结（五分项组序 + 组内断言序哈希封存），
//!   乱序执行拦截 100%。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 五分项组闭集：组号 0..5（G5 固定五组），组外无执行单元；
//!      组序恒按组号升序（组间乱序必拦）。
//!   2. 组内断言序：登记序即冻结序；封存哈希覆盖组序+组内序全链。
//!   3. 执行序表冻结：封存后序表只读（E_ORDER_SEALED），执行器
//!      逐步核对期望序号——乱序执行（组乱/序乱/跳步/回步）拦截 100%。
//!   4. 执行游标单调：游标只前进，重跑必拦（E_ALREADY_RUN）；
//!      中断续跑走断点通道（不在本模块）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x58c0~0x58dF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x58c0 独占 0x58c0~0x58dF） ────────────────────────────
pub const SEG_BASE: u16 = 0x58c0;

/// 0x58c1 序表未冻结即执行
const E_NOT_FROZEN: u16 = SEG_BASE + 0x1;
/// 0x58c2 序表已冻结仍尝试改序
const E_ORDER_SEALED: u16 = SEG_BASE + 0x2;
/// 0x58c3 组号越界（合法 0..5）
const E_GROUP_RANGE: u16 = SEG_BASE + 0x3;
/// 0x58c4 组间乱序：执行的组号小于当前组（回组）
const E_GROUP_BACK: u16 = SEG_BASE + 0x4;
/// 0x58c5 组间跳组：执行的组号大于下一组（跳组）
const E_GROUP_SKIP: u16 = SEG_BASE + 0x5;
/// 0x58c6 组内序乱：执行的断言序号不等于期望序号
const E_SEQ_WRONG: u16 = SEG_BASE + 0x6;
/// 0x58c7 组内序回步：序号回退
const E_SEQ_BACK: u16 = SEG_BASE + 0x7;
/// 0x58c8 断言重号：同组同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x8;
/// 0x58c9 已跑完仍请求执行
const E_ALREADY_RUN: u16 = SEG_BASE + 0x9;
/// 0x58ca 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0xA;
/// 0x58cb 序表溢出：组内断言数超定容
const E_GROUP_FULL: u16 = SEG_BASE + 0xB;
/// 0x58cc 空组封存：某组零断言即冻结
const E_GROUP_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x58cd 执行账与序表失衡：执行数 ≠ 序表条目数
const E_RUN_IMBAL: u16 = SEG_BASE + 0xD;
/// 0x58ce 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xE;
/// 0x58cf 断言条目悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xF;

/// 诊断码 → 中文可操作诊断（段 0x58c0~0x58dF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_NOT_FROZEN => "序表未冻结：先 freeze 再执行",
        E_ORDER_SEALED => "序表已冻结：序只读，改序请开新表",
        E_GROUP_RANGE => "组号越界：五分项组闭集合法 0..5",
        E_GROUP_BACK => "组间乱序：组号回退，执行序被破",
        E_GROUP_SKIP => "组间乱序：跳组执行，缺组必拦",
        E_SEQ_WRONG => "组内序乱：断言序号不等于期望序号",
        E_SEQ_BACK => "组内序回步：游标只前进，回步必拦",
        E_ASSERT_DUP => "断言重号：同组同名二次登记拒收",
        E_ALREADY_RUN => "已跑完：执行账已闭环，重跑请开新执行账",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_GROUP_FULL => "组内溢出：断言数超组定容上限",
        E_GROUP_EMPTY => "空组封存：五组必须全非空才可冻结",
        E_RUN_IMBAL => "执行失衡：执行数 ≠ 序表条目数",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_REF_DANGLE => "条目悬空：解引越界",
        _ => "未知诊断码（0x58c0 段外或保留位）",
    }
}

// ── 常量（来源：判据五分项组与定容） ─────────────────────────────────
/// 分项组数（判据原文：五分项组序）。
pub const GROUP_COUNT: usize = 5;
/// 单组断言定容。
pub const GROUP_CAP: usize = 32;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(GROUP_COUNT == 5);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 断言条目 ─────────────────────────────────────────────────────────
/// 序表条目：组号 + 组内序号（0 基）+ 名哈希。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderEntry {
    /// 组号 0..5。
    pub group: u8,
    /// 组内序号（登记序派发）。
    pub seq: u8,
    /// 名 FNV 哈希。
    pub name_fp: u64,
}

// ── 域级执行序表 ─────────────────────────────────────────────────────
/// 域级执行序表：五组登记 + 哈希封存 + 游标执行器（乱序拦截）。
pub struct OrderTable {
    /// 五组条目账（组号索引）。
    groups: [Vec<OrderEntry>; GROUP_COUNT],
    /// 冻结位。
    frozen: bool,
    /// 冻结哈希（组序+组内序全链）。
    order_fp: u64,
    /// 执行游标：(组号, 组内期望序号)。
    cur_group: usize,
    cur_seq: usize,
    /// 执行计数。
    ran_count: usize,
    /// 乱序拦截计数。
    intercepts: u32,
    /// 已闭环位。
    closed: bool,
}

impl OrderTable {
    /// 新序表：五空组。
    pub fn new() -> Self {
        OrderTable {
            groups: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            frozen: false,
            order_fp: 0,
            cur_group: 0,
            cur_seq: 0,
            ran_count: 0,
            intercepts: 0,
            closed: false,
        }
    }

    /// 登记断言（冻结前）：组闭集 + 定容 + 组内去重。
    pub fn enroll(&mut self, group: usize, name: &str) -> Result<u8, u16> {
        if self.frozen {
            return Err(E_ORDER_SEALED);
        }
        if group >= GROUP_COUNT {
            return Err(E_GROUP_RANGE);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if self.groups[group].len() >= GROUP_CAP {
            return Err(E_GROUP_FULL);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.groups[group].len() {
            if self.groups[group][i].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        let seq = self.groups[group].len() as u8;
        self.groups[group].push(OrderEntry {
            group: group as u8,
            seq,
            name_fp: fp,
        });
        Ok(seq)
    }

    /// 冻结：五组全非空 + 全链哈希封存。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_ORDER_SEALED);
        }
        let mut g = 0usize;
        while g < GROUP_COUNT {
            if self.groups[g].is_empty() {
                return Err(E_GROUP_EMPTY);
            }
            g += 1;
        }
        self.order_fp = self.compute_order_fp();
        self.frozen = true;
        Ok(self.order_fp)
    }

    /// 全链序哈希：组序 × 组内序逐条折叠。
    fn compute_order_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut g = 0usize;
        while g < GROUP_COUNT {
            let mut i = 0usize;
            while i < self.groups[g].len() {
                let e = &self.groups[g][i];
                h = fnv_mix(h, e.group | 0x10);
                h = fnv_mix(h, e.seq | 0x20);
                let mut shift = 56i32;
                while shift >= 0 {
                    h = fnv_mix(h, ((e.name_fp >> shift) & 0xFF) as u8);
                    shift -= 8;
                }
                i += 1;
            }
            g += 1;
        }
        h
    }

    /// 冻结位。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结哈希只读。
    pub const fn order_fp(&self) -> u64 {
        self.order_fp
    }

    /// 序表总条目数。
    pub fn total(&self) -> usize {
        let mut n = 0usize;
        let mut g = 0usize;
        while g < GROUP_COUNT {
            n += self.groups[g].len();
            g += 1;
        }
        n
    }

    /// 组条目数。
    pub fn group_len(&self, group: usize) -> Result<usize, u16> {
        if group >= GROUP_COUNT {
            return Err(E_GROUP_RANGE);
        }
        Ok(self.groups[group].len())
    }

    /// 执行一步：按 (组,序) 请求执行，期望序核对，乱序拦截 100%。
    /// 返回 Ok(true)=执行推进；Ok(false)=已达闭环尾。
    pub fn execute_step(&mut self, group: usize, seq: usize) -> Result<bool, u16> {
        if !self.frozen {
            return Err(E_NOT_FROZEN);
        }
        if self.closed {
            return Err(E_ALREADY_RUN);
        }
        if group >= GROUP_COUNT {
            return Err(E_GROUP_RANGE);
        }
        if seq >= self.groups[group].len() {
            return Err(E_SEQ_WRONG);
        }
        // 组间核对：当前组 → 执行组。
        if group < self.cur_group {
            self.intercepts = self.intercepts.wrapping_add(1);
            return Err(E_GROUP_BACK);
        }
        if group > self.cur_group {
            // 只允许前进一组且当前组已跑完。
            if group == self.cur_group + 1 && self.cur_seq >= self.groups[self.cur_group].len() {
                // 合法换组：游标落新组首。
                self.cur_group = group;
                self.cur_seq = 0;
            } else {
                self.intercepts = self.intercepts.wrapping_add(1);
                if group > self.cur_group + 1 {
                    return Err(E_GROUP_SKIP);
                }
                return Err(E_SEQ_WRONG);
            }
        }
        // 组内核对：必须等于期望序号（单调前进）。
        if seq < self.cur_seq {
            self.intercepts = self.intercepts.wrapping_add(1);
            return Err(E_SEQ_BACK);
        }
        if seq != self.cur_seq {
            self.intercepts = self.intercepts.wrapping_add(1);
            return Err(E_SEQ_WRONG);
        }
        self.cur_seq += 1;
        self.ran_count = self.ran_count.wrapping_add(1);
        // 闭环判定：末组末步。
        if self.cur_group == GROUP_COUNT - 1
            && self.cur_seq >= self.groups[self.cur_group].len()
        {
            self.closed = true;
        }
        Ok(!self.closed)
    }

    /// 拦截计数。
    pub const fn intercepts(&self) -> u32 {
        self.intercepts
    }

    /// 执行计数。
    pub const fn ran_count(&self) -> usize {
        self.ran_count
    }

    /// 闭环位。
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// 执行守恒审计：执行数 == 序表条目数（闭环时）。
    pub fn audit_run(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_NOT_FROZEN);
        }
        if self.closed && self.ran_count != self.total() {
            return Err(E_RUN_IMBAL);
        }
        Ok(())
    }

    /// 条目解引。
    pub fn entry(&self, group: usize, seq: usize) -> Result<OrderEntry, u16> {
        if group >= GROUP_COUNT {
            return Err(E_GROUP_RANGE);
        }
        if seq >= self.groups[group].len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.groups[group][seq])
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.total() == 0 {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("执行序表: 条目=");
        out.push_str(&itoa(self.total() as u64));
        out.push_str(" 已跑=");
        out.push_str(&itoa(self.ran_count as u64));
        out.push_str(" 拦截=");
        out.push_str(&itoa(self.intercepts as u64));
        out.push_str(" 闭环=");
        out.push_str(&itoa(self.closed as u64));
        out.push_str(" 序哈希=");
        out.push_str(&hex16(self.order_fp));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

fn gname(g: usize, i: usize) -> String {
    let mut s = String::new();
    s.push_str("g");
    s.push_str(&itoa(g as u64));
    s.push_str("_a");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：五组各 3 条 → 冻结 → 按序全跑 → 闭环守恒 ——
    let mut ot = OrderTable::new();
    let mut g = 0usize;
    while g < GROUP_COUNT {
        let mut i = 0usize;
        while i < 3 {
            ot.enroll(g, &gname(g, i))?;
            i += 1;
        }
        g += 1;
    }
    let fp = ot.freeze()?;
    if fp == 0 {
        return Err(E_GROUP_EMPTY);
    }
    if !ot.is_frozen() {
        return Err(E_ORDER_SEALED);
    }
    // 冻结后登记必拦。
    if ot.enroll(0, "late") != Err(E_ORDER_SEALED) {
        return Err(E_ORDER_SEALED);
    }
    // 按序全跑：组升序 × 组内升序。
    g = 0;
    let mut expect_more = true;
    while g < GROUP_COUNT {
        let glen = ot.group_len(g)?;
        let mut i = 0usize;
        while i < glen {
            expect_more = ot.execute_step(g, i)?;
            i += 1;
        }
        g += 1;
    }
    if !expect_more || !ot.is_closed() {
        return Err(E_RUN_IMBAL);
    }
    ot.audit_run()?;
    if ot.ran_count() != 15 {
        return Err(E_RUN_IMBAL);
    }
    // 闭环后再执行必拦。
    if ot.execute_step(0, 0) != Err(E_ALREADY_RUN) {
        return Err(E_ALREADY_RUN);
    }
    let sm = ot.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 幂等：同登记两次冻结哈希全等 ——
    let mut ot2 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        let mut i = 0usize;
        while i < 3 {
            ot2.enroll(g, &gname(g, i))?;
            i += 1;
        }
        g += 1;
    }
    let fp2 = ot2.freeze()?;
    if fp != fp2 {
        return Err(E_ORDER_SEALED);
    }

    // —— 乱序拦截 100%：回组 / 跳组 / 序回步 / 序跳跃 全数必拦 ——
    let mut ot3 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        let mut i = 0usize;
        while i < 2 {
            ot3.enroll(g, &gname(g, i))?;
            i += 1;
        }
        g += 1;
    }
    ot3.freeze()?;
    // 正常推进：组 0 两步 + 换组到组 1。
    ot3.execute_step(0, 0)?;
    ot3.execute_step(0, 1)?;
    ot3.execute_step(1, 0)?;
    // 回组拦截：已在组 1，回请求组 0。
    if ot3.execute_step(0, 0) != Err(E_GROUP_BACK) {
        return Err(E_GROUP_BACK);
    }
    // 跳组拦截：组 0 未完直跳组 2。
    let mut ot4 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        let mut i = 0usize;
        while i < 2 {
            ot4.enroll(g, &gname(g, i))?;
            i += 1;
        }
        g += 1;
    }
    ot4.freeze()?;
    if ot4.execute_step(2, 0) != Err(E_GROUP_SKIP) {
        return Err(E_GROUP_SKIP);
    }
    // 组内序跳跃拦截：组 0 先跑第 0 步，再请求第 2 步（不存在/不连续）。
    let mut ot5 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        let mut i = 0usize;
        while i < 3 {
            ot5.enroll(g, &gname(g, i))?;
            i += 1;
        }
        g += 1;
    }
    ot5.freeze()?;
    ot5.execute_step(0, 0)?;
    if ot5.execute_step(0, 2) != Err(E_SEQ_WRONG) {
        return Err(E_SEQ_WRONG);
    }
    // 序回步拦截：先两步再回第 0 步。
    let mut ot6 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        let mut i = 0usize;
        while i < 3 {
            ot6.enroll(g, &gname(g, i))?;
            i += 1;
        }
        g += 1;
    }
    ot6.freeze()?;
    ot6.execute_step(0, 0)?;
    ot6.execute_step(0, 1)?;
    if ot6.execute_step(0, 0) != Err(E_SEQ_BACK) {
        return Err(E_SEQ_BACK);
    }
    if ot6.intercepts() < 1 {
        return Err(E_SEQ_WRONG);
    }

    // —— 错误路径样本 ——
    let mut bad = OrderTable::new();
    if bad.enroll(5, "x") != Err(E_GROUP_RANGE) {
        return Err(E_GROUP_RANGE);
    }
    // 空名必拦。
    if bad.enroll(0, "") != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if bad.freeze() != Err(E_GROUP_EMPTY) {
        return Err(E_GROUP_EMPTY);
    }
    if bad.execute_step(0, 0) != Err(E_NOT_FROZEN) {
        return Err(E_NOT_FROZEN);
    }
    // 悬空解引。
    if bad.entry(0, 0) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    if bad.entry(GROUP_COUNT, 0) != Err(E_GROUP_RANGE) {
        return Err(E_GROUP_RANGE);
    }
    // 组内去重。
    let mut bad2 = OrderTable::new();
    bad2.enroll(1, "dup")?;
    if bad2.enroll(1, "dup") != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 执行越界序号。
    let mut bad3 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        bad3.enroll(g, &gname(g, 0))?;
        g += 1;
    }
    bad3.freeze()?;
    if bad3.execute_step(0, 1) != Err(E_SEQ_WRONG) {
        return Err(E_SEQ_WRONG);
    }
    if bad3.execute_step(GROUP_COUNT, 0) != Err(E_GROUP_RANGE) {
        return Err(E_GROUP_RANGE);
    }
    // 执行失衡注入：伪闭环态拉爆。
    let mut bad4 = OrderTable::new();
    g = 0;
    while g < GROUP_COUNT {
        bad4.enroll(g, &gname(g, 0))?;
        g += 1;
    }
    bad4.freeze()?;
    bad4.closed = true;
    bad4.ran_count = 3;
    if bad4.audit_run() != Err(E_RUN_IMBAL) {
        return Err(E_RUN_IMBAL);
    }

    // 诊断表抽查。
    if diag_detail(E_GROUP_SKIP) == "未知诊断码（0x58c0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x58c0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
