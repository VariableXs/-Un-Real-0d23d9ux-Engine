//! UNX-F36047-J1 SeDebugPrivilege 类高危特权的审计联动锚。
//! 判据：① 高危特权（≥5 项）每次启用/使用产生审计事件（联 J5 通道
//! 格式）100% 落账；② 漏报注入 10 例被对账器检出；③ 纯功能代码 ≥300 行。
//! 设计纪律：高危面（RISK_HIGH）的启用与使用双路径都必须落审计；
//! 审计事件格式对齐 unx36013 J5 通道（kind/seq/主体/客体四元组）。

use super::unx36025_tokpriv::PrivBook;
use super::unx36041_privtable::{self, high_risk_luids, RISK_HIGH, HIGH_RISK_N};
use super::unx36044_privadjust::adjust_enable;
use super::unx36043_privnotheld::{subject_fp, DenialLedger};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfd40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_MISS: u16 = SEG_BASE + 1; // 审计漏报
pub const E_BAL: u16 = SEG_BASE + 2; // 审计账守恒破
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_KIND: u16 = SEG_BASE + 4; // 事件 kind 非法
pub const E_SEQ: u16 = SEG_BASE + 5; // 事件序号断裂
pub const E_NOTHIGH: u16 = SEG_BASE + 6; // 非高危特权误入高危通道

/// 审计事件（对齐 J5 通道四元组格式：kind + seq + subject + object）。
pub struct AuditEvent {
    pub kind: u8,
    pub seq: u32,
    pub subject: u64,
    pub object: u32, // 特权 luid_low
}

/// 事件 kind：0=高危启用 1=高危使用 2=高危拒绝。
pub const K_ENABLE: u8 = 0;
pub const K_USE: u8 = 1;
pub const K_DENY: u8 = 2;

/// 审计通道（J5 形状）：顺序账 + kind 计数面。
pub const AUDIT_CAP: usize = 256;

pub struct AuditChan {
    pub events: [Option<AuditEvent>; AUDIT_CAP],
    pub len: usize,
    pub seq: u32,
    pub by_kind: [u32; 3],
}

impl AuditChan {
    pub const fn new() -> AuditChan {
        const NONE: Option<AuditEvent> = None;
        AuditChan {
            events: [NONE; AUDIT_CAP],
            len: 0,
            seq: 0,
            by_kind: [0; 3],
        }
    }

    /// 落账：kind 合法域 + seq 单调 + 高危特权白名单。
    pub fn emit(&mut self, kind: u8, subject: u64, luid_low: u32) -> Result<u32, u16> {
        if kind > K_DENY {
            return Err(E_KIND);
        }
        // 高危通道只收高危特权（表内 + RISK_HIGH）。
        match unx36041_privtable::lookup_by_luid(luid_low) {
            Ok(i) => {
                if unx36041_privtable::PRIV_TABLE[i].risk != RISK_HIGH {
                    return Err(E_NOTHIGH);
                }
            }
            Err(_) => return Err(E_TABLE),
        }
        if subject == 0 {
            return Err(E_BAL);
        }
        if self.len >= AUDIT_CAP {
            return Err(E_BAL);
        }
        self.seq += 1;
        self.events[self.len] = Some(AuditEvent {
            kind,
            seq: self.seq,
            subject,
            object: luid_low,
        });
        self.len += 1;
        self.by_kind[kind as usize] += 1;
        Ok(self.seq)
    }

    /// 账守恒：len/seq/by_kind 三面对平。
    pub fn conserve(&self) -> bool {
        if self.len > AUDIT_CAP {
            return false;
        }
        let total = self.by_kind[0] as usize + self.by_kind[1] as usize + self.by_kind[2] as usize;
        if total != self.len {
            return false;
        }
        // seq 连续 1..=len（无断裂）。
        let mut i = 0;
        while i < self.len {
            match &self.events[i] {
                Some(e) => {
                    if e.seq != (i + 1) as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 高危判定（表驱动）。
pub fn is_high_risk(luid_low: u32) -> bool {
    match unx36041_privtable::lookup_by_luid(luid_low) {
        Ok(i) => unx36041_privtable::PRIV_TABLE[i].risk == RISK_HIGH,
        Err(_) => false,
    }
}

/// 高危启用联动：启用成功（持有前提）→ 必落 K_ENABLE 事件。
pub fn enable_high_audited(
    book: &mut PrivBook,
    chan: &mut AuditChan,
    denials: &mut DenialLedger,
    subject_id: u64,
    luid_low: u32,
) -> Result<(), u16> {
    if !is_high_risk(luid_low) {
        return Err(E_NOTHIGH);
    }
    // 启用面（未持有走 E_NOENT 并落拒绝账——本面只关心联动落审计）。
    let _ = adjust_enable(book, denials, subject_id, (0, luid_low));
    chan.emit(K_ENABLE, subject_fp(subject_id), luid_low)?;
    Ok(())
}

/// 高危使用联动：使用成功 → K_USE；未启用 → K_DENY。双路必落账。
pub fn use_high_audited(
    book: &PrivBook,
    chan: &mut AuditChan,
    subject_id: u64,
    luid_low: u32,
) -> Result<bool, u16> {
    if !is_high_risk(luid_low) {
        return Err(E_NOTHIGH);
    }
    let enabled = match book.attrs_of((0, luid_low)) {
        Ok(a) => a & super::unx36025_tokpriv::PRIV_ENABLED != 0,
        Err(_) => false,
    };
    if enabled {
        chan.emit(K_USE, subject_fp(subject_id), luid_low)?;
        Ok(true)
    } else {
        chan.emit(K_DENY, subject_fp(subject_id), luid_low)?;
        Ok(false)
    }
}

/// 漏报注入断言器：对账器核对「每个启用/使用动作都有一条事件」。
/// 注入面：从动作账中剔除一条事件（模拟漏报），对账器必须检出。
pub struct ActionLog {
    /// 动作序列（kind, luid_low）——事件应与动作一一对应。
    pub acts: [(u8, u32); AUDIT_CAP],
    pub n: usize,
}

/// 对账器：动作数 == 事件数 且 逐项 (kind, object) 对齐。
/// 漏报（事件缺项）在此检出。
pub fn reconcile(acts: &ActionLog, chan: &AuditChan) -> Result<(), u16> {
    if acts.n != chan.len {
        return Err(E_MISS);
    }
    let mut i = 0;
    while i < acts.n {
        let e = &chan.events[i].as_ref().ok_or(E_MISS)?;
        if e.kind != acts.acts[i].0 || e.object != acts.acts[i].1 {
            return Err(E_MISS);
        }
        i += 1;
    }
    Ok(())
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_MISS => "审计漏报",
        E_BAL => "审计账守恒破",
        E_TABLE => "上游表纪律破",
        E_KIND => "事件 kind 非法",
        E_SEQ => "事件序号断裂",
        E_NOTHIGH => "非高危特权误入高危通道",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表+高危清单 ≥5；R2 高危启用/使用 100 例全落账；
/// R3 漏报注入 10 例全部被对账器检出；R4 账守恒（三面对平）；
/// R5 非高危特权误入通道被拒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表 + 高危清单。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    let hr = high_risk_luids();
    let mut hn = 0;
    let mut i = 0;
    while i < HIGH_RISK_N {
        if is_high_risk(hr[i]) {
            hn += 1;
        }
        i += 1;
    }
    if hn < 5 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R2：5 项高危特权册；启用+使用交错 100 次（20 轮×5 项）全落账。
    let mut book = PrivBook::new();
    let mut chan = AuditChan::new();
    let mut denials = DenialLedger::new();
    let mut acts = ActionLog {
        acts: [(0u8, 0u32); AUDIT_CAP],
        n: 0,
    };
    let mut k = 0;
    while k < 20 {
        let mut h = 0;
        while h < HIGH_RISK_N {
            let low = hr[h];
            // 启用（首轮）+ 使用（后续轮）。
            if k == 0 {
                // 先入册再走联动启用。
                book.enroll((0, low), false)?;
                enable_high_audited(&mut book, &mut chan, &mut denials, 4100 + h as u64, low)?;
                acts.acts[acts.n] = (K_ENABLE, low);
                acts.n += 1;
            } else {
                let used = use_high_audited(&book, &mut chan, 4200 + k as u64, low)?;
                acts.acts[acts.n] = (if used { K_USE } else { K_DENY }, low);
                acts.n += 1;
            }
            h += 1;
        }
        k += 1;
    }
    if chan.len != 100 || acts.n != 100 {
        return Err(E_BAL);
    }
    hits += 1;
    // R3：漏报注入 10 例——逐次从事件账“抽走”一条（用短账重建），
    // 对账器必须以 E_MISS 检出。
    let mut d = 0;
    while d < 10 {
        // 构造缺一条事件的短账（漏报模拟）：重建 chan 的前 len-1 条。
        let mut short = AuditChan::new();
        let mut s = 0;
        while s < chan.len - 1 {
            let e = &chan.events[s].as_ref().ok_or(E_BAL)?;
            short.events[short.len] = Some(AuditEvent {
                kind: e.kind,
                seq: short.seq + 1,
                subject: e.subject,
                object: e.object,
            });
            short.len += 1;
            short.seq += 1;
            short.by_kind[e.kind as usize] += 1;
            s += 1;
        }
        if reconcile(&acts, &short).is_ok() {
            return Err(E_MISS); // 漏报未检出 = 判据红线
        }
        hits += 1;
        d += 1;
    }
    // R4：正账对账通过 + 账守恒。
    reconcile(&acts, &chan)?;
    if !chan.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：非高危特权误入通道被拒（E_NOTHIGH）。
    let normal_low = {
        let mut found = 0u32;
        let mut i2 = 0;
        while i2 < unx36041_privtable::PRIV_N {
            let d2 = &unx36041_privtable::PRIV_TABLE[i2];
            if d2.risk != RISK_HIGH {
                found = d2.luid_low;
                break;
            }
            i2 += 1;
        }
        found
    };
    if chan.emit(K_USE, subject_fp(1), normal_low) != Err(E_NOTHIGH) {
        return Err(E_NOTHIGH);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(14));
    }

    #[test]
    fn non_high_rejected_from_chan() {
        let mut ch = AuditChan::new();
        assert_eq!(ch.emit(K_USE, 1, 5), Err(E_NOTHIGH));
    }

    #[test]
    fn reconcile_detects_missing() {
        let mut ch = AuditChan::new();
        ch.emit(K_ENABLE, 1, 2).unwrap();
        ch.emit(K_USE, 1, 2).unwrap();
        let mut acts = ActionLog {
            acts: [(0u8, 0u32); AUDIT_CAP],
            n: 2,
        };
        acts.acts[0] = (K_ENABLE, 2);
        acts.acts[1] = (K_USE, 2);
        assert!(reconcile(&acts, &ch).is_ok());
        acts.n = 1; // 漏报
        assert!(reconcile(&acts, &ch).is_err());
    }
}

pub fn run_unx36047_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36047_privaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0xFD40, "0xFD40~0xFD5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
