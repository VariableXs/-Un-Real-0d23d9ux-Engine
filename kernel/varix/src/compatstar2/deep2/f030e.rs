//! F030 深化批次三 · 安装事务日志执行/边界/注入面（compatstar2/deep2 · G-A-30）。
//!
//! 批次一/二深化覆盖沙盒安装主干；本批补齐主册【功能定义】「全语义对齐」的
//! 事务日志侧出口：事务记录定长 TLV 序列化（tag:u8 + len:u16 + value 定长 64
//! ，parse 双向 round-trip）、回滚点栈（定长 16 LIFO，push/pop/peek，溢出拒绝
//! 并计数）、文件校验和记录对拍账（FNV-1a 32 位记前值，安装后重算对拍，不匹
//! 配计数）、升级序状态机（idle→stopping-old→uninstall-old→install-new→
//! register 五态，跳步拒绝）、磁盘预算预检模型（需求×1.2 余量系数 vs 可用，
//! 不足列出缺口字节账）。
//!
//! 判据对账：主册【设计细节】/【状态与异常】未落地面为源，一处一事实（FNV-1a
//! 32 位标准常量 + 主册 G-A-30 安装产物登记/镜像哈希判据对拍）。零堆纪律：
//! 定长 TLV 记录 + 定长栈，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// TLV value 定长上限 64 字节；头长 tag:u8 + len:u16 = 3；整记录上限 67。
pub const TLV_VALUE_CAP: usize = 64;
pub const TLV_HEADER_LEN: usize = 3;
pub const TLV_RECORD_CAP: usize = TLV_HEADER_LEN + TLV_VALUE_CAP;
/// 回滚点栈容量（定长 16 LIFO）。
pub const ROLLBACK_STACK_CAP: usize = 16;
/// FNV-1a 32 位标准常量：偏移基 2166136261（0x811C9DC5）、素数 16777619。
pub const FNV1A_OFFSET: u32 = 2166136261;
pub const FNV1A_PRIME: u32 = 16777619;
/// 磁盘预算余量系数 ×1.2 = 12/10 定点（主册 G-A-30 预检口径）。
pub const BUDGET_MARGIN_NUM: u64 = 12;
pub const BUDGET_MARGIN_DEN: u64 = 10;

// ---------------------------------------------------------------------------
// 事务记录定长 TLV 序列化
// ---------------------------------------------------------------------------

/// 一条定长 TLV 事务记录（字节面 + 实长）。
#[derive(Clone, Copy)]
pub struct TlvRecord { pub bytes: [u8; TLV_RECORD_CAP], pub len: usize }

impl TlvRecord {
    pub const fn new() -> Self { TlvRecord { bytes: [0u8; TLV_RECORD_CAP], len: 0 } }
}

/// 编码：tag + len(u16 大端) + value；超 64 字节显性拒绝（零截断）。
pub fn tlv_encode(tag: u8, value: &[u8], out: &mut TlvRecord) -> Result<(), &'static str> {
    if value.len() > TLV_VALUE_CAP {
        return Err("tlv-value-too-long");
    }
    out.bytes = [0u8; TLV_RECORD_CAP];
    out.bytes[0] = tag;
    let n = value.len();
    out.bytes[1] = (n >> 8) as u8;
    out.bytes[2] = n as u8;
    out.bytes[TLV_HEADER_LEN..TLV_HEADER_LEN + n].copy_from_slice(value);
    out.len = TLV_HEADER_LEN + n;
    Ok(())
}

/// 解析：(tag, value_len)；头残缺/长度账不平 → 显性 Err（双向对拍）。
pub fn tlv_parse(rec: &TlvRecord) -> Result<(u8, usize), &'static str> {
    if rec.len < TLV_HEADER_LEN || rec.len > TLV_RECORD_CAP {
        return Err("tlv-short-record");
    }
    let tag = rec.bytes[0];
    let n = ((rec.bytes[1] as usize) << 8) | rec.bytes[2] as usize;
    if TLV_HEADER_LEN + n != rec.len {
        return Err("tlv-len-mismatch");
    }
    Ok((tag, n))
}

/// value 字节面引用（parse 通过后取用）。
pub fn tlv_value(rec: &TlvRecord) -> &[u8] {
    &rec.bytes[TLV_HEADER_LEN..rec.len]
}

// ---------------------------------------------------------------------------
// 回滚点栈（定长 16 LIFO）
// ---------------------------------------------------------------------------

/// 回滚点栈：push/pop/peek；溢出/空栈显性拒绝并计数。
pub struct RollbackStack {
    pub ids: [u32; ROLLBACK_STACK_CAP],
    pub len: usize,
    pub overflows: u32,
    pub underflows: u32,
}

impl RollbackStack {
    pub const fn new() -> Self { RollbackStack { ids: [0u32; ROLLBACK_STACK_CAP], len: 0, overflows: 0, underflows: 0 } }

    pub fn push(&mut self, id: u32) -> Result<(), &'static str> {
        if self.len >= ROLLBACK_STACK_CAP {
            self.overflows += 1;
            return Err("rollback-stack-full");
        }
        self.ids[self.len] = id;
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Result<u32, &'static str> {
        if self.len == 0 {
            self.underflows += 1;
            return Err("rollback-stack-empty");
        }
        self.len -= 1;
        Ok(self.ids[self.len])
    }

    /// 栈顶窥视（不动栈面；空栈 Err）。
    pub fn peek(&self) -> Result<u32, &'static str> {
        if self.len == 0 {
            return Err("rollback-stack-empty");
        }
        Ok(self.ids[self.len - 1])
    }
}

// ---------------------------------------------------------------------------
// 文件校验和对拍账（FNV-1a 32 位）+ 磁盘预算预检
// ---------------------------------------------------------------------------

/// FNV-1a 32 位（offset basis 2166136261、prime 16777619 标准参数）。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h = FNV1A_OFFSET;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(FNV1A_PRIME);
    }
    h
}

/// 校验和对拍账：记前值，安装后重算对拍，不匹配显性计数。
pub struct ChecksumLedger {
    pub verified: u32,
    pub mismatch: u32,
}

impl ChecksumLedger {
    pub const fn new() -> Self { ChecksumLedger { verified: 0, mismatch: 0 } }

    /// 前值 vs 安装后值对拍（FNV-1a 32 位）；结果如实入账并返回。
    pub fn verify(&mut self, before: &[u8], after: &[u8]) -> bool {
        let ok = fnv1a32(before) == fnv1a32(after);
        if ok {
            self.verified += 1;
        } else {
            self.mismatch += 1;
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// 升级序状态机
// ---------------------------------------------------------------------------

/// 升级五态（idle→stopping-old→uninstall-old→install-new→register）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdState { Idle, StoppingOld, UninstallOld, InstallNew, Register }

/// 升级序状态机：跳步/回退显性拒绝并计数（零静默）。
pub struct UpdFsm {
    pub state: UpdState,
    pub skips: u32,
}

impl UpdFsm {
    pub const fn new() -> Self { UpdFsm { state: UpdState::Idle, skips: 0 } }

    /// 合法序表外的步进一律 Err 并计数。
    pub fn step(&mut self, to: UpdState) -> Result<(), &'static str> {
        let legal = matches!(
            (self.state, to),
            (UpdState::Idle, UpdState::StoppingOld)
                | (UpdState::StoppingOld, UpdState::UninstallOld)
                | (UpdState::UninstallOld, UpdState::InstallNew)
                | (UpdState::InstallNew, UpdState::Register)
        );
        if legal {
            self.state = to;
            Ok(())
        } else {
            self.skips += 1;
            Err("upgrade-skip-rejected")
        }
    }
}

/// 预检结论（需求×1.2 vs 可用；不足列缺口字节账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetPlan { pub required_bytes: u64, pub shortfall_bytes: u64, pub sufficient: bool }

/// 需求 ×1.2 余量系数 vs 可用；不足 → shortfall = required - avail（缺口账）。
pub fn disk_precheck(need_bytes: u64, avail_bytes: u64) -> BudgetPlan {
    let required = need_bytes * BUDGET_MARGIN_NUM / BUDGET_MARGIN_DEN;
    if avail_bytes >= required {
        BudgetPlan { required_bytes: required, shortfall_bytes: 0, sufficient: true }
    } else {
        BudgetPlan { required_bytes: required, shortfall_bytes: required - avail_bytes, sufficient: false }
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次三）
// ---------------------------------------------------------------------------

pub fn run_f030e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F030-install-txn-d3");
    // 1) TLV round-trip：encode → parse → value 字节面一致。
    let mut rec = TlvRecord::new();
    let enc = tlv_encode(0x2A, b"varix", &mut rec);
    cs.add(
        "tlv_round_trip",
        enc.is_ok() && tlv_parse(&rec) == Ok((0x2A, 5)) && rec.len == TLV_HEADER_LEN + 5
            && tlv_value(&rec) == b"varix",
        "",
    );
    // 2) TLV 边界：65 字节 value 显性拒绝；账面不平 parse 拒绝。
    let long = [0u8; TLV_VALUE_CAP + 1];
    let mut rec2 = TlvRecord::new();
    let too_long = tlv_encode(1, &long, &mut rec2);
    let mut bad = TlvRecord::new();
    bad.bytes[0] = 7;
    bad.bytes[1] = 0;
    bad.bytes[2] = 9; // 声明 9 字节，实长 3+5
    bad.len = TLV_HEADER_LEN + 5;
    cs.add(
        "tlv_boundary_rejects",
        too_long == Err("tlv-value-too-long") && tlv_parse(&bad) == Err("tlv-len-mismatch"),
        "",
    );
    // 3) 回滚点栈 LIFO 序：push 1/2/3 → pop 3/2/1，peek 不动栈面。
    let mut st = RollbackStack::new();
    let _ = st.push(1);
    let _ = st.push(2);
    let _ = st.push(3);
    let lifo = st.peek() == Ok(3) && st.pop() == Ok(3) && st.pop() == Ok(2)
        && st.pop() == Ok(1) && st.len == 0;
    cs.add("rollback_lifo_order", lifo, "");
    // 4) 溢出/空栈显性拒绝并计数。
    let mut st2 = RollbackStack::new();
    let mut last_push = Ok(());
    for i in 0..17u32 {
        last_push = st2.push(i);
    }
    let empty_pop = RollbackStack::new().pop();
    let empty_peek = RollbackStack::new().peek();
    cs.add(
        "rollback_overflow_underflow",
        last_push == Err("rollback-stack-full") && st2.overflows == 1 && st2.len == ROLLBACK_STACK_CAP
            && empty_pop == Err("rollback-stack-empty")
            && empty_peek == Err("rollback-stack-empty"),
        "",
    );
    // 5) FNV-1a 标准测试向量："" → 0x811C9DC5，"a" → 0xE40C292C。
    cs.add(
        "fnv1a_known_vectors",
        fnv1a32(b"") == 0x811C_9DC5 && fnv1a32(b"a") == 0xE40C_292C,
        "",
    );
    // 6) 对拍账：一致 verified 计数、不一致 mismatch 计数（零静默）。
    let mut led = ChecksumLedger::new();
    let ok1 = led.verify(b"payload-v1", b"payload-v1");
    let ok2 = led.verify(b"payload-v1", b"payload-v2");
    cs.add(
        "checksum_ledger_counting",
        ok1 && !ok2 && led.verified == 1 && led.mismatch == 1,
        "",
    );
    // 7) 升级序全路径：五态依序推进至 Register。
    let mut u = UpdFsm::new();
    let path = u.step(UpdState::StoppingOld).is_ok()
        && u.step(UpdState::UninstallOld).is_ok()
        && u.step(UpdState::InstallNew).is_ok()
        && u.step(UpdState::Register).is_ok();
    cs.add("upgrade_full_path", path && u.state == UpdState::Register && u.skips == 0, "");
    // 8) 跳步显性拒绝：Idle→InstallNew、Register→Idle 均拒并计数。
    let mut u2 = UpdFsm::new();
    let skip1 = u2.step(UpdState::InstallNew);
    for s in [UpdState::StoppingOld, UpdState::UninstallOld, UpdState::InstallNew, UpdState::Register] {
        let _ = u2.step(s);
    }
    let skip2 = u2.step(UpdState::Idle);
    cs.add(
        "upgrade_skip_rejected",
        skip1 == Err("upgrade-skip-rejected") && skip2 == Err("upgrade-skip-rejected")
            && u2.skips == 2,
        "",
    );
    // 9) 磁盘预算：需求 1,000,000 ×1.2 = 1,200,000 > 可用 1,100,000 → 缺口 100,000。
    let tight = disk_precheck(1_000_000, 1_100_000);
    cs.add(
        "disk_budget_shortfall_ledger",
        tight.required_bytes == 1_200_000 && !tight.sufficient
            && tight.shortfall_bytes == 100_000,
        "",
    );
    // 10) 预算充足：可用 ≥ 需求×1.2 → sufficient，缺口为 0。
    let ok = disk_precheck(1_000, 1_200);
    cs.add(
        "disk_budget_sufficient",
        ok.sufficient && ok.shortfall_bytes == 0 && ok.required_bytes == 1_200,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tlv_max_value_round_trip() {
        let mut rec = TlvRecord::new();
        let value = [0xA5u8; TLV_VALUE_CAP];
        assert!(tlv_encode(0xFF, &value, &mut rec).is_ok(), "恰满 64 字节必成");
        assert_eq!(tlv_parse(&rec), Ok((0xFF, TLV_VALUE_CAP)));
        assert_eq!(tlv_value(&rec), &value);
        assert_eq!(rec.len, TLV_RECORD_CAP);
    }

    #[test]
    fn upgrade_repeat_step_rejected() {
        let mut u = UpdFsm::new();
        assert_eq!(u.step(UpdState::Idle), Err("upgrade-skip-rejected"), "原地步进也是非法序");
        assert_eq!(u.skips, 1);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f030e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
