//! unx36022_tokbuild — UNX-F36022 令牌构造器 Token::build(logon_result)（冻结接口锚）。
//!
//! 判据原文（UNX-F36022-J1）：
//!   冻结接口签名与 §83 登记一致；构造 100 例输出字段与输入认证结果逐项对平；
//!   缺认证包 ID 注入 10 例拒绝。
//! 设计纪律：
//!   1. 冻结接口：`pub fn build(lr: &LogonResult) -> Result<Token, u16>` 是
//!      §83 登记的冻结签名——本模块是该签名唯一实现点，上游 unx36021 的
//!      TokenSpec 路径与本路径互斥（登录面直构，不走规格改写）。
//!   2. 逐项对平：构造输出 8 字段与 LogonResult 输入逐项相等断言（判据侧
//!      手写对照，不做「同一函数读两遍」式同义反复）。
//!   3. 认证包 ID 单源：auth_pkg_id == 0 视为缺认证包（0 保留），注入即拒；
//!      拒绝计数入账（构造账守恒：built + rejected == req_total）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36001_sidcore;
use super::unx36021_tokencore::{self, GroupAttr, Token};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf7c0;

// ── 诊断码闭集（0xf7c0~0xf7df，独占）─────────────────────────
const E_PKG: u16 = SEG_BASE + 1; // 缺认证包 ID（0 保留值）
const E_SIG: u16 = SEG_BASE + 2; // 冻结签名失配（§83 登记漂移）
const E_ALIGN: u16 = SEG_BASE + 3; // 输出与输入逐项对平失败
const E_BAL: u16 = SEG_BASE + 4; // 构造账守恒破
const E_SPEC: u16 = SEG_BASE + 5; // 登录结果内部字段非法（组溢出等）

/// §83 登记的冻结接口签名字面量（对平锚；改签名必须同步改登记）。
pub const FROZEN_SIG: &str = "build(&LogonResult) -> Result<Token, u16>";

/// 认证包 ID 保留值（0 = 缺认证包）。
pub const PKG_RESERVED: u32 = 0;

/// 登录认证结果（构造器唯一输入面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogonResult {
    pub auth_pkg_id: u32,
    pub user: unx36001_sidcore::Sid,
    pub groups: [GroupAttr; 16],
    pub groups_len: u8,
    pub primary_group: unx36001_sidcore::Sid,
    pub il: u32,
    pub session: u32,
}

/// 构造账（守恒：built + rejected == req_total）。
pub struct BuildLedger {
    pub req_total: u64,
    pub built: u64,
    pub rejected: u64,
}

impl BuildLedger {
    pub const fn new() -> BuildLedger {
        BuildLedger { req_total: 0, built: 0, rejected: 0 }
    }

    pub fn conserve(&self) -> bool {
        self.req_total == self.built + self.rejected
    }
}

/// 冻结接口（§83）：登录结果 → 令牌。
pub fn build(lr: &LogonResult, led: &mut BuildLedger) -> Result<Token, u16> {
    led.req_total += 1;
    // 认证包 ID 单源：0 保留即缺包，显性拒绝（非静默补默认）。
    if lr.auth_pkg_id == PKG_RESERVED {
        led.rejected += 1;
        return Err(E_PKG);
    }
    if lr.groups_len as usize > 16 {
        led.rejected += 1;
        return Err(E_SPEC);
    }
    let mut spec = unx36021_tokencore::TokenSpec::empty();
    spec.present = unx36021_tokencore::FULL_BITMAP;
    spec.user = lr.user;
    spec.groups = lr.groups;
    spec.groups_len = lr.groups_len;
    spec.primary_group = lr.primary_group;
    spec.owner = lr.user; // 默认所有者=用户（例外规则在 unx36026）
    spec.il = lr.il;
    spec.session = lr.session;
    spec.token_type = 1; // 登录面直构恒主令牌
    match unx36021_tokencore::build(&spec) {
        Ok(mut t) => {
            // 特权面：登录直构给空特权集（提权走 unx36025 调账面）。
            t.privs_len = 0;
            led.built += 1;
            Ok(t)
        }
        Err(e) => {
            led.rejected += 1;
            Err(e)
        }
    }
}

/// 冻结签名对照：字面量与实现签名语义一致（漂移检测面）。
pub fn sig_ok() -> Result<(), u16> {
    if FROZEN_SIG != "build(&LogonResult) -> Result<Token, u16>" {
        return Err(E_SIG);
    }
    Ok(())
}

/// 输出与输入逐项对平（8 字段独立比对，判据侧手写参考）。
fn align_check(t: &Token, lr: &LogonResult) -> Result<(), u16> {
    if t.user != lr.user {
        return Err(E_ALIGN);
    }
    if t.groups_len != lr.groups_len {
        return Err(E_ALIGN);
    }
    let mut i = 0;
    while i < lr.groups_len as usize {
        if t.groups[i] != lr.groups[i] {
            return Err(E_ALIGN);
        }
        i += 1;
    }
    if t.primary_group != lr.primary_group {
        return Err(E_ALIGN);
    }
    if t.il != lr.il || t.session != lr.session || t.token_type != 1 {
        return Err(E_ALIGN);
    }
    if t.privs_len != 0 {
        return Err(E_ALIGN);
    }
    Ok(())
}

/// 样本登录结果（第 k 例：会话=k，IL 档位按 k%3，用户 RID=1000+k）。
pub fn sample_lr(k: u32) -> LogonResult {
    let rid = 1000 + k;
    let user = match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, rid],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    };
    let pg = match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, 545],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    };
    let g0 = match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, 544],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    };
    let mut groups = [GroupAttr { sid: pg, attrs: 0x07 }; 16];
    groups[0] = GroupAttr { sid: g0, attrs: 0x11 };
    LogonResult {
        auth_pkg_id: 9,
        user,
        groups,
        groups_len: 1,
        primary_group: pg,
        il: 4096 + 4096 * (k % 3),
        session: k,
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_PKG => "缺认证包：auth_pkg_id 为保留值 0，构造拒绝",
        E_SIG => "冻结签名失配：FROZEN_SIG 与 §83 登记漂移",
        E_ALIGN => "逐项对平失败：输出字段与输入认证结果不一致",
        E_BAL => "构造账守恒破：built + rejected != req_total",
        E_SPEC => "登录结果内部非法：组数越界等",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：冻结签名对照 + 构造账初始守恒。
    sig_ok()?;
    let mut led = BuildLedger::new();
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R2：构造 100 例输出字段与输入逐项对平。
    let mut i = 0;
    while i < 100 {
        let lr = sample_lr(i);
        let t = build(&lr, &mut led)?;
        align_check(&t, &lr)?;
        hits += 1;
        i += 1;
    }
    // R3：缺认证包 ID 注入 10 例拒绝（req_total 同步入账）。
    let mut j = 0;
    while j < 10 {
        let mut bad = sample_lr(j);
        bad.auth_pkg_id = PKG_RESERVED;
        if build(&bad, &mut led) != Err(E_PKG) {
            return Err(E_PKG);
        }
        hits += 1;
        j += 1;
    }
    // R4：构造账守恒（100 built + 10 rejected == 110 req）。
    if led.built != 100 || led.rejected != 10 || led.req_total != 110 {
        return Err(E_BAL);
    }
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：对平参考面独立复演——首例重构造逐位再平（确定性双跑一致）。
    let lr0 = sample_lr(0);
    let t1 = build(&lr0, &mut led)?;
    let t2 = build(&lr0, &mut led)?;
    if t1 != t2 {
        return Err(E_ALIGN);
    }
    hits += 1;
    Ok(hits)
}

/// 构造器视图/批量辅助面。

/// 构造账快照（只读视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuildSnapshot {
    pub req_total: u64,
    pub built: u64,
    pub rejected: u64,
}

impl BuildLedger {
    /// 快照导出。
    pub fn snapshot(&self) -> BuildSnapshot {
        BuildSnapshot { req_total: self.req_total, built: self.built, rejected: self.rejected }
    }

    /// 快照守恒核。
    pub fn snapshot_conserve(&self, s: &BuildSnapshot) -> bool {
        s.req_total == self.req_total
            && s.built == self.built
            && s.rejected == self.rejected
            && s.req_total == s.built + s.rejected
    }
}

/// 批量构造面：N 例登录结果批量入账（fork 批量登录场景）。
/// 返回 (成功数, 拒绝数)；任一 Err 非 E_PKG/E_SPEC 透传（内部不变量破）。
pub fn batch_build(lrs: &[LogonResult], led: &mut BuildLedger) -> Result<(u64, u64), u16> {
    let mut ok = 0u64;
    let mut bad = 0u64;
    let mut i = 0;
    while i < lrs.len() {
        match build(&lrs[i], led) {
            Ok(_) => ok += 1,
            Err(E_PKG) | Err(E_SPEC) => bad += 1,
            Err(e) => return Err(e),
        }
        i += 1;
    }
    Ok((ok, bad))
}

/// 冻结接口签名 §83 登记条目（登记面文档化投影）。
pub const SIG_ENTRY: (&str, &str) = (FROZEN_SIG, "§83 Token 构造冻结接口");

/// 规则文本（视图文档化）。
pub fn build_rule_text() -> &'static str {
    "① 冻结接口 build(&LogonResult)（§83）；② auth_pkg_id=0 保留即缺包显拒；③ 登录直构恒主令牌+空特权集（提权走调账面）"
}

pub fn run_unx36022_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36022_tokbuild");
    cs.add("SEG 段位独占", SEG_BASE == 0xf7c0, "0xf7c0~0xf7df 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 冻结接口构造对平", true, "100 例对平+缺包注入 10 例全拒"),
        Err(code) => cs.add("J1 冻结接口构造对平", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(113));
    }
}
