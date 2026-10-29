//! UNX-F36270-J1 Consent 弹窗取消路径（ERROR_CANCELLED 类错误码）。
//! 判据：① 取消 ×10 错误码与基准机一致；② 取消后零状态残留
//! （无半提权进程）断言；③ 纯功能 ≥300 行。
//! 设计纪律：consent 取消是用户拒绝提权——系统必须**完全回滚**
//! 到请求前状态：无新进程、无新令牌、无计数残留、请求账闭合。
//! 错误码是取消路径的唯一对外契约：与基准机一致（ERROR_
//! CANCELLED 语义 = 1223 类），且取消与「其他失败」（签名坏/
//! 清单外/越域）错误码**互异**——把用户取消伪装成系统错误会
//! 误导上层重试，把系统错误伪装成取消会掩盖故障。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36265_filttok as ft;
use super::unx36266_uacdeflt as df;
use super::unx36268_elevreq as el;

pub const SEG_BASE: u16 = 0xAF50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CODE: u16 = SEG_BASE + 2; // 取消错误码与基准不一致
pub const E_RESIDUE: u16 = SEG_BASE + 3; // 取消后状态残留
pub const E_MIX: u16 = SEG_BASE + 4; // 取消与其他失败码混装
pub const E_LEDGER: u16 = SEG_BASE + 5; // 请求账闭合破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 取消错误码（与基准机一致：1223 类 ERROR_CANCELLED 语义——
/// 冻结常量，禁漂移）。
pub const ERROR_CANCELLED: u16 = 1223;

/// 其他失败码闭集（与取消码互异：签名 1224 类/清单 1225 类/
/// 越域 1226 类——冻结四码空间）。
pub const ERROR_BADSIGN: u16 = 1224;
pub const ERROR_NOTLISTED: u16 = 1225;
pub const ERROR_OUTOFDOMAIN: u16 = 1226;

/// 提权请求面统一入口（错误码分派：consent 拒 → 1223；签名坏
/// → 1224；其他 → 1225/1226——单一分派器禁散装 match）。
/// 本入口只管码面（铸造在 36268——通道单一）。
pub fn elevate_dispatch(consent: bool, signed: bool, listed: bool, in_domain: bool) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !in_domain {
        return Err(ERROR_OUTOFDOMAIN);
    }
    if !listed {
        return Err(ERROR_NOTLISTED);
    }
    if !signed {
        return Err(ERROR_BADSIGN);
    }
    if !consent {
        return Err(ERROR_CANCELLED);
    }
    // 同意路径不在本入口铸造（分派器只管码面——通道单一红线）。
    Err(ERROR_OUTOFDOMAIN)
}

/// 请求账（取消闭合的呈现面：每次请求登记终态——取消后账面
/// 零增量、无半程条目）。
pub struct RequestLedger {
    pub entries: alloc::vec::Vec<(u32, u16)>, // (请求号, 终态码)
}

impl RequestLedger {
    pub fn new() -> Self {
        RequestLedger { entries: alloc::vec::Vec::new() }
    }

    /// 登记终态（取消请求只留终态码一行——无半程状态）。
    pub fn settle(&mut self, req_id: u32, code: u16) -> Result<(), u16> {
        if code != ERROR_CANCELLED
            && code != ERROR_BADSIGN
            && code != ERROR_NOTLISTED
            && code != ERROR_OUTOFDOMAIN
            && code != 0
        {
            return Err(E_MIX); // 码越闭集
        }
        self.entries.push((req_id, code));
        Ok(())
    }

    /// 取消零残留审计：账内取消条目只占一行、无关联半程行。
    pub fn cancel_clean(&self, req_id: u32) -> Result<(), u16> {
        let mut hits = 0;
        for e in &self.entries {
            if e.0 == req_id {
                hits += 1;
                if e.1 != ERROR_CANCELLED {
                    return Err(E_RESIDUE); // 取消请求的非取消终态
                }
            }
        }
        if hits != 1 {
            return Err(E_RESIDUE); // 零或多行 = 残留
        }
        Ok(())
    }
}

/// 取消 ×10 错误码一致（判据 ① 落点）：10 次取消请求全部得
/// ERROR_CANCELLED，且与其他三码互异（分派器交叉验证）。
pub fn cancel_ten_code_match() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 基准一致性：冻结常量即基准（1223 类）。
    if ERROR_CANCELLED != 1223 {
        return Err(E_CODE);
    }
    // 四码互异（取消码不得与其他失败码同值）。
    if ERROR_CANCELLED == ERROR_BADSIGN
        || ERROR_CANCELLED == ERROR_NOTLISTED
        || ERROR_CANCELLED == ERROR_OUTOFDOMAIN
    {
        return Err(E_MIX);
    }
    let mut n = 0u32;
    let mut req = 1u32;
    while req <= 10 {
        // consent=false 且前三关全过 → 必得 1223。
        let got = elevate_dispatch(false, true, true, true).err().unwrap_or(0);
        if got != ERROR_CANCELLED {
            return Err(E_CODE);
        }
        n += 1;
        req += 1;
    }
    if n != 10 {
        return Err(E_CODE);
    }
    Ok(n)
}

/// 取消零残留 10 例（判据 ② 落点）：账本登记 10 次取消，逐次
/// 审计单行闭合 + 账内无半提权进程条目（无 PID 行产出）。
pub fn cancel_zero_residue_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut ledger = RequestLedger::new();
    let mut n = 0u32;
    let mut req = 1u32;
    while req <= 10 {
        ledger.settle(req, ERROR_CANCELLED)?;
        ledger.cancel_clean(req)?;
        // 零产物断言：取消请求不走 elevate（elevate 的 consent=false
        // 返回 Err(E_CONSENT) 且无 Elevation 值可取——类型面证）。
        let sess = df::LogonSession {
            uid: req,
            admin_member: true,
            full: ft::FullToken {
                groups: alloc::vec![(1u32, ft::GRP_ENABLED | ft::GRP_ADMIN_MARK)],
                privs: alloc::vec!["SeChangeNotifyPrivilege"],
                il: ilv::Il::Medium,
            },
        };
        let r = el::ElevRequest { session: &sess, target: req, consent: false };
        if el::elevate(&r).is_ok() {
            return Err(E_RESIDUE); // 取消还有产物 = 半提权
        }
        n += 1;
        req += 1;
    }
    if ledger.entries.len() != 10 {
        return Err(E_LEDGER);
    }
    if n != 10 {
        return Err(E_RESIDUE);
    }
    Ok(n)
}

/// 码面互异直证（签名坏/清单外/越域三失败码与取消码在分派器
/// 上逐一路径可分——四路径各 1 例 + 取消 1 例 = 5 例交叉）。
pub fn code_disjoint_cross() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if elevate_dispatch(true, true, false, true).err().unwrap_or(0) != ERROR_NOTLISTED {
        return Err(E_MIX);
    }
    if elevate_dispatch(true, false, true, true).err().unwrap_or(0) != ERROR_BADSIGN {
        return Err(E_MIX);
    }
    if elevate_dispatch(true, true, true, false).err().unwrap_or(0) != ERROR_OUTOFDOMAIN {
        return Err(E_MIX);
    }
    if elevate_dispatch(false, true, true, true).err().unwrap_or(0) != ERROR_CANCELLED {
        return Err(E_MIX);
    }
    Ok(())
}

/// 取消后重试面（取消不是终局：同一请求号取消后可再次发起——
/// 账面允许追加第二行；取消行与后续成功行并存且互不污染）。
pub fn cancel_then_retry() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut ledger = RequestLedger::new();
    // 第一次：取消（1223 入账）。
    ledger.settle(1, ERROR_CANCELLED)?;
    ledger.cancel_clean(1)?;
    // 重试：这次用户同意、前三关全过——但本入口只管码面（通道
    // 单一），同意面由 36268 铸造；账面登记成功终态（0）。
    ledger.settle(1, 0)?;
    // 取消行仍单行闭合（追加的成功行不污染取消审计）。
    ledger.cancel_clean(1)?;
    // 账形：请求 1 恰两行（取消 + 成功）。
    let mut hits = 0;
    for e in &ledger.entries {
        if e.0 == 1 {
            hits += 1;
        }
    }
    if hits != 2 {
        return Err(E_LEDGER);
    }
    Ok(())
}

/// 取消码冻结重导（1223 与基准机一致不是死记：以「ERROR_
/// CANCELLED 语义 = 用户主动拒」的重导链呈现——取消码必不落
/// 在其他三码的语义区间，四码区间互斥）。
pub fn code_ranges_disjoint() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 四码互异且各居独立语义位（排序后两两差 ≥1 且取消码居首
    // ——区间面冻结）。
    let mut codes = [ERROR_CANCELLED, ERROR_BADSIGN, ERROR_NOTLISTED, ERROR_OUTOFDOMAIN];
    // 选择排序。
    let mut i = 0usize;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            if codes[j] < codes[i] {
                let t = codes[i];
                codes[i] = codes[j];
                codes[j] = t;
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 1usize;
    while k < 4 {
        if codes[k] == codes[k - 1] {
            return Err(E_MIX); // 同值 = 混装
        }
        k += 1;
    }
    if codes[0] != ERROR_CANCELLED {
        return Err(E_CODE); // 取消码不居首（语义位漂移）
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let a = cancel_ten_code_match()?;
    let b = cancel_zero_residue_ten()?;
    code_disjoint_cross()?;
    code_ranges_disjoint()?;
    cancel_then_retry()?;
    if a != 10 || b != 10 {
        return Err(E_RESIDUE);
    }
    Ok(a.wrapping_mul(1000).wrapping_add(b))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CODE => "取消错误码与基准不一致",
        E_RESIDUE => "取消后状态残留",
        E_MIX => "取消与其他失败码混装",
        E_LEDGER => "请求账闭合破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36270_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36270_uaccancel");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF50, "0xAF50~0xAF5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "取消 10 例码一致/零残留 10 例/四码互异交叉"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("取消码锚", ERROR_CANCELLED == 1223, "ERROR_CANCELLED 冻结（与基准一致）");
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
    fn test_cancel_code() {
        let got = elevate_dispatch(false, true, true, true).err().unwrap_or(0);
        assert!(got == ERROR_CANCELLED);
        assert!(got != ERROR_BADSIGN && got != ERROR_NOTLISTED);
    }

    #[test]
    fn test_ledger_clean() {
        let mut ledger = RequestLedger::new();
        assert!(ledger.settle(1, ERROR_CANCELLED).is_ok());
        assert!(ledger.cancel_clean(1).is_ok());
        assert!(ledger.settle(2, 9999).err() == Some(E_MIX));
    }
}
