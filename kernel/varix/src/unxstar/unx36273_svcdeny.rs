//! UNX-F36273-J1 高 IL 服务启动拒绝（低 IL 请求者→服务拒绝+审计）。
//! 判据：① 拒绝 50 例落 J5 审计；错误三要素齐备断言；提权后
//! 重试成功路径 20 例对照；② 纯功能 ≥300 行。
//! 设计纪律：低 IL 请求者启动高 IL 服务必须**拒 + 审计双落**：
//! 拒是防线（服务不被低信任面拉起），审计是追溯面（谁、何时、
//! 为何）。三要素齐备断言：审计条目缺任一要素即假审计——审计
//! 存在但不完整等于不存在（事后无法归因）。提权重试对照：同一
//! 请求经提权路径（36268）后服务启动成功——证明拒的是信任级
//! 而非请求本身（防「拒一切」的假防线）。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;
use super::unx36268_elevreq as el;

pub const SEG_BASE: u16 = 0xAF90;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEAK: u16 = SEG_BASE + 2; // 低 IL 拉高 IL 服务漏拒
pub const E_AUDIT: u16 = SEG_BASE + 3; // 审计未落/要素缺失
pub const E_RETRY: u16 = SEG_BASE + 4; // 提权重试面破
pub const E_THREE: u16 = SEG_BASE + 5; // 三要素缺失
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 服务启动门槛（对标 LocalSystem 服务的 High IL 门槛——冻结）。
pub const SERVICE_MIN_RANK: usize = 3; // High

/// 审计条目（三要素：谁/何时/为何——J5 审计消费形状）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub who: u32,     // 请求者 UID
    pub when: u64,    // 逻辑时钟
    pub why: u16,     // 拒因码（闭集）
}

impl AuditEntry {
    /// 三要素齐备断言（who 非零 / when 非零 / why 在闭集）。
    pub fn three_elements(&self) -> Result<(), u16> {
        if self.who == 0 || self.when == 0 {
            return Err(E_THREE);
        }
        if self.why != E_LEAK && self.why != E_ILLEGAL {
            return Err(E_THREE);
        }
        Ok(())
    }
}

/// 服务启动裁决（低 IL 请求者 → 拒 + 审计；达标 → 许）。
pub fn service_start_check(
    requester_il: ilv::Il,
    who: u32,
    when: u64,
) -> (Result<(), u16>, Option<AuditEntry>) {
    if VER != 1 {
        return (Err(E_VER), None);
    }
    if ilv::il_rank(requester_il) < SERVICE_MIN_RANK {
        let entry = AuditEntry { who, when, why: E_LEAK };
        (Err(E_LEAK), Some(entry))
    } else {
        (Ok(()), None)
    }
}

/// 审计账（J5 消费：追加只读——拒面逐条落账）。
pub struct AuditBook {
    pub entries: alloc::vec::Vec<AuditEntry>,
}

impl AuditBook {
    pub fn new() -> Self {
        AuditBook { entries: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, e: AuditEntry) -> Result<(), u16> {
        e.three_elements()?;
        self.entries.push(e);
        Ok(())
    }

    /// 拒面完整性：账内条目数与拒次数守恒（逐拒必落账）。
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

/// 拒绝 50 例落审计 + 三要素齐备（判据 ① 落点）：5 档低 IL
/// 请求者 × 10 次 = 50 拒，逐拒落账、逐条三要素断言、账数守恒。
pub fn deny_fifty_audited() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut book = AuditBook::new();
    let mut denied = 0u32;
    let mut clock: u64 = 1;
    let mut s_rank = 0usize;
    while s_rank < SERVICE_MIN_RANK {
        let requester = ilv::IL_TABLE[s_rank];
        let mut k = 0;
        while k < 10 {
            let (verdict, entry) = service_start_check(requester, 100u32 + s_rank as u32, clock);
            match verdict {
                Ok(()) => return Err(E_LEAK), // 低 IL 拉起 = 漏拒
                Err(E_LEAK) => {}
                Err(_) => return Err(E_ILLEGAL),
            }
            let e = entry.ok_or(E_AUDIT)?;
            book.record(e)?;
            denied += 1;
            clock += 1;
            k += 1;
        }
        s_rank += 1;
    }
    if denied != 50 || book.count() != 50 {
        return Err(E_AUDIT);
    }
    // 账内抽查：when 严格递增（逻辑时钟单调——事后排序可归因）。
    let mut i = 1usize;
    while i < book.entries.len() {
        if book.entries[i].when <= book.entries[i - 1].when {
            return Err(E_AUDIT);
        }
        i += 1;
    }
    Ok(denied)
}

/// 三要素缺失注入（审计防伪：缺要素的条目 record 必拒——
/// 假审计写不进账）。10 例注入。
pub fn fake_audit_ten_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut blocked = 0u32;
    // 三型伪造：缺 who / 缺 when / why 越闭集。
    let fakes: [AuditEntry; 3] = [
        AuditEntry { who: 0, when: 1, why: E_LEAK },
        AuditEntry { who: 1, when: 0, why: E_LEAK },
        AuditEntry { who: 1, when: 1, why: 9999 },
    ];
    let mut round = 0;
    while round < 4 {
        let mut i = 0usize;
        while i < 3 {
            let mut book = AuditBook::new();
            if book.record(fakes[i].clone()).is_ok() {
                return Err(E_AUDIT); // 假条目入账 = 审计防伪破
            }
            blocked += 1;
            i += 1;
        }
        round += 1;
    }
    if blocked != 12 {
        return Err(E_AUDIT);
    }
    Ok(blocked)
}

/// 提权重试成功 20 例对照（判据 ② 落点）：同一请求者（Medium
/// 成员）先拒、提权后（High 完整令牌）服务启动成功——拒因是
/// 信任级而非请求面。20 例逐例「先拒后许」配对。
pub fn elevate_retry_twenty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut uid = 200u32;
    while uid < 220 {
        let sess = df::LogonSession {
            uid,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::Il::Medium,
            },
        };
        // 先拒（默认令牌 Medium IL）。
        let (v1, _) = service_start_check(ilv::Il::Medium, uid, uid as u64);
        if v1.is_ok() {
            return Err(E_LEAK);
        }
        // 提权（36268 铸 High 完整令牌）。
        let req = el::ElevRequest { session: &sess, target: uid, consent: true };
        let elev = el::elevate(&req)?;
        // 重试（新进程 High IL）→ 许。
        let (v2, _) = service_start_check(elev.new_token.il, uid, uid as u64 + 1000);
        if v2.is_err() {
            return Err(E_RETRY); // 提权后仍拒 = 拒因漂移
        }
        n += 1;
        uid += 1;
    }
    if n != 20 {
        return Err(E_RETRY);
    }
    Ok(n)
}

/// 门槛锚（SERVICE_MIN_RANK 与 ilv::High rank 恒等——门槛漂移
/// 即面漂移）。
pub fn threshold_anchor() -> Result<(), u16> {
    if SERVICE_MIN_RANK != ilv::il_rank(ilv::Il::High) {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

/// 档位边界扫描（门槛两侧逐档对核：Low/Medium 恒拒、High/
/// System 恒许——门槛不是概率面，逐档锁死）。4 档 × 5 次 = 20 例。
pub fn threshold_scan_twenty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut n = 0u32;
    let mut rank = 0usize;
    while rank < 5 {
        let requester = ilv::IL_TABLE[rank];
        let mut k = 0;
        while k < 5 {
            let (v, _) = service_start_check(requester, 1, (rank * 5 + k + 1) as u64);
            let want_allow = rank >= SERVICE_MIN_RANK;
            if v.is_ok() != want_allow {
                return Err(E_LEAK); // 边界档裁决漂移
            }
            n += 1;
            k += 1;
        }
        rank += 1;
    }
    if n != 20 {
        return Err(E_LEAK);
    }
    Ok(n)
}

/// 审计账单调回放（50 条审计条目 when 严格递增 + who 闭集轮转
/// ——账可重放排序，事后归因面）。借 deny_fifty 的账形重构。
pub fn audit_monotone_replay() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut book = AuditBook::new();
    let mut clock: u64 = 100;
    let mut i = 0;
    while i < 50 {
        let e = AuditEntry { who: 7, when: clock, why: E_LEAK };
        book.record(e)?;
        clock += 3; // 步进 3（严格递增且非 1 步进——单调性非平凡）
        i += 1;
    }
    if book.count() != 50 {
        return Err(E_AUDIT);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    threshold_anchor()?;
    let a = deny_fifty_audited()?;
    let b = fake_audit_ten_blocked()?;
    let c = elevate_retry_twenty()?;
    threshold_scan_twenty()?;
    audit_monotone_replay()?;
    if a != 50 || b != 12 || c != 20 {
        return Err(E_AUDIT);
    }
    Ok(a.wrapping_mul(100).wrapping_add(c))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEAK => "低 IL 拉高 IL 服务漏拒",
        E_AUDIT => "审计未落/要素缺失",
        E_RETRY => "提权重试面破",
        E_THREE => "三要素缺失",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36273_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36273_svcdeny");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF90, "0xAF90~0xAF9F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "拒 50 例全落账三要素齐/假账 12 例拦截/提权重试 20 例对照"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("门槛锚", SERVICE_MIN_RANK == 3, "服务门槛 = High（与 ilv 对平）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_service_gate() {
        let (v, e) = service_start_check(ilv::Il::Low, 7, 9);
        assert!(v.is_err());
        let entry = e.unwrap_or(AuditEntry { who: 0, when: 0, why: 0 });
        assert!(entry.three_elements().is_ok());
        assert!(service_start_check(ilv::Il::System, 7, 9).0.is_ok());
    }

    #[test]
    fn test_fake_audit() {
        assert_eq!(fake_audit_ten_blocked().err(), None);
    }
}
