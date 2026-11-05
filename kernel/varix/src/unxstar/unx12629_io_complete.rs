//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12629 NtSetIoCompletion/NtRemoveIoCompletion→D4 联签 · 0x80740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12630-J1 完成包三字段（Status/Information/Key）
//!      逐项对平断言绿，字段篡改注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Set/Remove 对偶语义：SetIoCompletion 入账一包（三字段冻结），
//!     RemoveIoCompletion 按序取包——每笔 Set 必有且至多一笔 Remove
//!     消费（SetRemoveParityBreak 抓不对平），重复 Remove 拒
//!     （DupRemove）；
//!   - 完成包三字段（判据锚定）：Status（完成状态，负值即错误档）、
//!     Information（完成字节数）、Key（完成键）逐项冻结入账；逐项
//!     对平：出包三字段必须与账面入包逐项相等（FieldMismatch×三路
//!     抓单项漂移）；
//!   - 字段篡改注入 10/10：十次轮换注入（Status 翻负/Information
//!     抬溢/Key 错位/三字段全篡），每次必检出且检出位与被篡字段
//!     一致；
//!   - D4 联签：每笔合法 Remove 落一笔联签，联签数 ≡ 合法 Remove 数
//!     （CosignParityBreak）；LCG 种子写死 = SEG_BASE（0x80740），
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect/panic；
//!     诊断码闭集 0x80740..0x80780。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80740，AI-VX9-1105 B02 批 12 槽第 6 槽）。
pub const SEG_BASE: u32 = 0x80740;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetRmCode {
    /// Status 字段漂移：出包状态≠账面状态
    StatusMismatch,
    /// Information 字段漂移：出包完成值≠账面完成值
    InformationMismatch,
    /// Key 字段漂移：出包完成键≠账面完成键
    KeyMismatch,
    /// Set/Remove 对平破：Set 数≠Remove 数+存量
    SetRemoveParityBreak,
    /// 重复取包：一包只许 Remove 一次
    DupRemove,
    /// 空账取包：账空不许取
    RemoveEmpty,
    /// 完成状态档非法：Status=0 且 Information 非零（语义互斥）
    StatusInfoClash,
    /// 完成键越窗：Key 不在登记窗
    KeyOutOfWin,
    /// 联签守恒破：联签数≠合法 Remove 数
    CosignParityBreak,
    /// 重复联签
    DupCosign,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl SetRmCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SetRmCode::StatusMismatch => 0x01,
                SetRmCode::InformationMismatch => 0x02,
                SetRmCode::KeyMismatch => 0x03,
                SetRmCode::SetRemoveParityBreak => 0x04,
                SetRmCode::DupRemove => 0x05,
                SetRmCode::RemoveEmpty => 0x06,
                SetRmCode::StatusInfoClash => 0x07,
                SetRmCode::KeyOutOfWin => 0x08,
                SetRmCode::CosignParityBreak => 0x09,
                SetRmCode::DupCosign => 0x0A,
                SetRmCode::FingerprintDrift => 0x0B,
                SetRmCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            SetRmCode::StatusMismatch => "Status 漂移：出包状态与账面逐项对平",
            SetRmCode::InformationMismatch => "Information 漂移：完成值逐项对平",
            SetRmCode::KeyMismatch => "Key 漂移：完成键逐项对平",
            SetRmCode::SetRemoveParityBreak => "对平破：Set=Remove+存量，逐笔对账",
            SetRmCode::DupRemove => "重复取包：一包只取一次",
            SetRmCode::RemoveEmpty => "空账取包：先入账再取",
            SetRmCode::StatusInfoClash => "状态/完成值互斥：Status=0 须 Information=0",
            SetRmCode::KeyOutOfWin => "完成键越窗：用登记窗内 Key",
            SetRmCode::CosignParityBreak => "联签守恒破：联签=合法Remove，逐笔对账",
            SetRmCode::DupCosign => "重复联签：一笔取包只回签一次",
            SetRmCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            SetRmCode::Unknown => "未知诊断码：不在 0x80740 段闭集内",
        }
    }

    pub const ALL: [SetRmCode; 12] = [
        SetRmCode::StatusMismatch,
        SetRmCode::InformationMismatch,
        SetRmCode::KeyMismatch,
        SetRmCode::SetRemoveParityBreak,
        SetRmCode::DupRemove,
        SetRmCode::RemoveEmpty,
        SetRmCode::StatusInfoClash,
        SetRmCode::KeyOutOfWin,
        SetRmCode::CosignParityBreak,
        SetRmCode::DupCosign,
        SetRmCode::FingerprintDrift,
        SetRmCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in SetRmCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    SetRmCode::Unknown.detail()
}

// ── 冻结常量表（来源：NtSetIoCompletion 语义手账 · 完成包节） ─────
/// 完成键登记窗（下闭上开）。
pub const KEY_WIN: (u32, u32) = (0x40, 0x50);
/// 完成包账容量。
pub const LEDGER_CAP: usize = 16;
/// 字段名闭集（篡改检出定位用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PktField {
    /// 完成状态字段。
    Status,
    /// 完成值字段。
    Information,
    /// 完成键字段。
    Key,
    /// 全字段。
    All,
}

impl PktField {
    /// 轮换表（十次注入用，确定性）。
    pub const fn by_round(r: u32) -> PktField {
        match r % 4 {
            0 => PktField::Status,
            1 => PktField::Information,
            2 => PktField::Key,
            _ => PktField::All,
        }
    }
}

/// 一笔完成包（三字段冻结入账）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CompPacket {
    /// 包号（账内单调）。
    pub pkt_id: u32,
    /// 完成状态（负值即错误档；此处以 i32 记账）。
    pub status: i32,
    /// 完成值（字节数）。
    pub information: u32,
    /// 完成键。
    pub key: u32,
    /// 已取出。
    pub removed: bool,
    /// 已联签。
    pub cosigned: bool,
}

/// Set/Remove 闸（三字段逐项对平）。
pub struct SetRmGate {
    /// 包账。
    ledger: Vec<CompPacket>,
    /// 合法 Remove 计数。
    pub removed_ok: u32,
    /// 联签流水。
    pub cosign_flow: Vec<u32>,
    /// 篡改检出流水（检出位逐次落账）。
    pub tamper_flow: Vec<PktField>,
}

impl SetRmGate {
    pub fn new() -> SetRmGate {
        SetRmGate {
            ledger: Vec::new(),
            removed_ok: 0,
            cosign_flow: Vec::new(),
            tamper_flow: Vec::new(),
        }
    }

    /// 入账（Set）：三字段互斥/越窗闸 + 容量。
    pub fn set_pkt(&mut self, status: i32, information: u32, key: u32) -> Result<u32, u32> {
        if self.ledger.len() >= LEDGER_CAP {
            return Err(SetRmCode::SetRemoveParityBreak.val());
        }
        // 语义互斥：Status=0（成功档零位）须 Information=0。
        if status == 0 && information != 0 {
            return Err(SetRmCode::StatusInfoClash.val());
        }
        if key < KEY_WIN.0 || key >= KEY_WIN.1 {
            return Err(SetRmCode::KeyOutOfWin.val());
        }
        let prev = self.ledger.iter().map(|p| p.pkt_id).max().unwrap_or(0);
        let pkt_id = prev.saturating_add(1);
        self.ledger.push(CompPacket {
            pkt_id,
            status,
            information,
            key,
            removed: false,
            cosigned: false,
        });
        Ok(pkt_id)
    }

    /// 取包（Remove）：FIFO 头 + 三字段逐项对平 + 重复拒。
    pub fn remove_pkt(
        &mut self,
        expect_status: i32,
        expect_information: u32,
        expect_key: u32,
    ) -> Result<CompPacket, u32> {
        // FIFO：最小未取出 pkt_id。
        let mut head: Option<u32> = None;
        for p in self.ledger.iter() {
            if p.removed {
                continue;
            }
            head = match head {
                Some(cur) if cur <= p.pkt_id => Some(cur),
                _ => Some(p.pkt_id),
            };
        }
        let pkt_id = match head {
            Some(id) => id,
            // 账空（从未入账）→ RemoveEmpty；入过但全取走 → DupRemove。
            None => {
                if self.ledger.is_empty() {
                    return Err(SetRmCode::RemoveEmpty.val());
                }
                return Err(SetRmCode::DupRemove.val());
            }
        };
        let rec = match self.ledger.iter_mut().find(|p| p.pkt_id == pkt_id) {
            Some(p) => p,
            None => return Err(SetRmCode::RemoveEmpty.val()),
        };
        // 三字段逐项对平（判据核心）：单项漂移即拒，且检出位落账。
        if rec.status != expect_status {
            self.tamper_flow.push(PktField::Status);
            return Err(SetRmCode::StatusMismatch.val());
        }
        if rec.information != expect_information {
            self.tamper_flow.push(PktField::Information);
            return Err(SetRmCode::InformationMismatch.val());
        }
        if rec.key != expect_key {
            self.tamper_flow.push(PktField::Key);
            return Err(SetRmCode::KeyMismatch.val());
        }
        rec.removed = true;
        self.removed_ok = self.removed_ok.saturating_add(1);
        Ok(*rec)
    }

    /// D4 联签（合法取包才可签）。
    pub fn cosign(&mut self, pkt_id: u32) -> Result<u32, u32> {
        let rec = match self.ledger.iter_mut().find(|p| p.pkt_id == pkt_id) {
            Some(p) => p,
            None => return Err(SetRmCode::DupCosign.val()),
        };
        if rec.cosigned {
            return Err(SetRmCode::DupCosign.val());
        }
        if !rec.removed {
            return Err(SetRmCode::SetRemoveParityBreak.val());
        }
        rec.cosigned = true;
        self.cosign_flow.push(pkt_id);
        Ok(pkt_id)
    }

    /// 对平：Set 数 ≡ 合法 Remove 数 + 未取存量。
    pub fn set_rm_parity(&self) -> Result<u32, u32> {
        let set_total = self.ledger.len() as u32;
        let stock = self.ledger.iter().filter(|p| !p.removed).count() as u32;
        if set_total == self.removed_ok.saturating_add(stock) {
            Ok(set_total)
        } else {
            Err(SetRmCode::SetRemoveParityBreak.val())
        }
    }

    /// 联签守恒。
    pub fn cosign_parity(&self) -> Result<u32, u32> {
        if self.cosign_flow.len() as u32 == self.removed_ok {
            Ok(self.removed_ok)
        } else {
            Err(SetRmCode::CosignParityBreak.val())
        }
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8074_0000u32;
        for p in self.ledger.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(p.status as u32);
            fp = fp.wrapping_mul(31).wrapping_add(p.information);
            fp = fp.wrapping_mul(31).wrapping_add(p.key);
        }
        fp.wrapping_add(self.removed_ok).wrapping_add(self.cosign_flow.len() as u32)
    }
}

// ── 字段篡改注入 10/10（判据核心） ──────────────────────────────
/// 十次轮换注入：每次按轮换表篡改一个（或全部）字段后取包，必检出
/// 且检出位与被篡字段一致。
fn tamper_inject_ten() -> (u32, u32) {
    let mut caught = 0u32;
    for r in 0..10u32 {
        let mut g = SetRmGate::new();
        let want = (-65536i32, 0x100u32, 0x44u32);
        let _ = g.set_pkt(want.0, want.1, want.2);
        let got = match PktField::by_round(r) {
            PktField::Status => g.remove_pkt(want.0 + 1, want.1, want.2),
            PktField::Information => g.remove_pkt(want.0, want.1 + 1, want.2),
            PktField::Key => g.remove_pkt(want.0, want.1, want.2 + 1),
            PktField::All => g.remove_pkt(want.0 + 1, want.1 + 1, want.2 + 1),
        };
        // 必检出（Err），且检出位与被篡字段一致。
        let hit = match (&got, PktField::by_round(r)) {
            (Err(c), PktField::Status) => *c == SetRmCode::StatusMismatch.val(),
            (Err(c), PktField::Information) => *c == SetRmCode::InformationMismatch.val(),
            (Err(c), PktField::Key) => *c == SetRmCode::KeyMismatch.val(),
            // 全篡：三字段按序检出（Status 最先），检出流水须含 Status。
            (Err(_), PktField::All) => g.tamper_flow.last() == Some(&PktField::Status),
            _ => false,
        };
        if hit {
            caught = caught.saturating_add(1);
        }
    }
    (caught, 10)
}

/// 黄金正演：三包（错误档/成功档/满值档）全流程 Set→Remove→联签。
fn golden_run() -> Result<u32, u32> {
    let mut g = SetRmGate::new();
    let _ = g.set_pkt(-1073741824, 0x40, 0x44)?; // 错误档：负 Status
    let _ = g.set_pkt(0, 0, 0x45)?; // 成功空包：Status=0 且 Information=0
    let _ = g.set_pkt(0x0100, 0x1000, 0x4F)?; // 满值档
    let p1 = g.remove_pkt(-1073741824, 0x40, 0x44)?;
    let p2 = g.remove_pkt(0, 0, 0x45)?;
    let p3 = g.remove_pkt(0x0100, 0x1000, 0x4F)?;
    // 逐项对平回验（三字段逐项相等）。
    if p1.status != -1073741824 || p1.information != 0x40 || p1.key != 0x44 {
        return Err(SetRmCode::StatusMismatch.val());
    }
    if p2.status != 0 || p2.information != 0 || p2.key != 0x45 {
        return Err(SetRmCode::InformationMismatch.val());
    }
    if p3.status != 0x0100 || p3.information != 0x1000 || p3.key != 0x4F {
        return Err(SetRmCode::KeyMismatch.val());
    }
    let _ = g.cosign(p1.pkt_id)?;
    let _ = g.cosign(p2.pkt_id)?;
    let _ = g.cosign(p3.pkt_id)?;
    g.set_rm_parity()?;
    g.cosign_parity()
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut g = SetRmGate::new();
        let _ = g.set_pkt(-1073741824, 0x40, 0x44);
        let _ = g.set_pkt(0, 0, 0x45);
        let _ = g.set_pkt(0x0100, 0x1000, 0x4F);
        let _ = g.remove_pkt(-1073741824, 0x40, 0x44);
        let _ = g.remove_pkt(0, 0, 0x45);
        let _ = g.remove_pkt(0x0100, 0x1000, 0x4F);
        let _ = g.cosign(1);
        let _ = g.cosign(2);
        let _ = g.cosign(3);
        g.fingerprint()
    };
    run() == run()
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in SetRmCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：三字段逐项对平断言（先破后复逐字段）。
    let mut pf = SetRmGate::new();
    let _ = pf.set_pkt(-1, 8, 0x44);
    let s_bad = pf.remove_pkt(0, 8, 0x44) == Err(SetRmCode::StatusMismatch.val());
    let mut pf2 = SetRmGate::new();
    let _ = pf2.set_pkt(-1, 8, 0x44);
    let i_bad = pf2.remove_pkt(-1, 9, 0x44) == Err(SetRmCode::InformationMismatch.val());
    let mut pf3 = SetRmGate::new();
    let _ = pf3.set_pkt(-1, 8, 0x44);
    let k_bad = pf3.remove_pkt(-1, 8, 0x45) == Err(SetRmCode::KeyMismatch.val());
    v.push(("J2-三字段逐项对平", s_bad && i_bad && k_bad, String::from("Status/Information/Key")));
    // 判据②：字段篡改注入 10/10 检出。
    let (c10, t10) = tamper_inject_ten();
    v.push(("J2-篡改注入10/10", c10 == 10 && t10 == 10, format!("c/t {}/{}", c10, t10)));
    // 注入：互斥档与越窗键。
    let mut pi = SetRmGate::new();
    let clash = pi.set_pkt(0, 5, 0x44) == Err(SetRmCode::StatusInfoClash.val());
    let keyout = pi.set_pkt(-1, 8, 0x99) == Err(SetRmCode::KeyOutOfWin.val());
    v.push(("J2-互斥/越窗键注入", clash && keyout, String::from("语义互斥+键窗")));
    // 边界：空账取包。
    let mut pe = SetRmGate::new();
    let empty = pe.remove_pkt(0, 0, 0) == Err(SetRmCode::RemoveEmpty.val());
    v.push(("J3-空账取包拒", empty, String::from("先入账再取")));
    // 边界：重复取包/未取签拒/重复联签。
    let mut pd = SetRmGate::new();
    let id = pd.set_pkt(-1, 8, 0x44);
    let pre_cosign = pd.cosign(id.unwrap_or(0)) == Err(SetRmCode::SetRemoveParityBreak.val());
    let _ = pd.remove_pkt(-1, 8, 0x44);
    let dup_rm = pd.remove_pkt(-1, 8, 0x44) == Err(SetRmCode::DupRemove.val());
    let _ = pd.cosign(id.unwrap_or(0));
    let dup_cs = pd.cosign(id.unwrap_or(0)) == Err(SetRmCode::DupCosign.val());
    v.push(("J3-重复取包/未取签拒", dup_rm && pre_cosign && dup_cs, String::from("一包一取一签")));
    // 边界：对平先破后复。
    let mut pp = SetRmGate::new();
    let _ = pp.set_pkt(-1, 8, 0x44);
    let _ = pp.remove_pkt(-1, 8, 0x44);
    pp.removed_ok = 9; // 账实不符注入
    let brk = pp.set_rm_parity() == Err(SetRmCode::SetRemoveParityBreak.val());
    pp.removed_ok = 1;
    let fix = pp.set_rm_parity() == Ok(1);
    v.push(("J4-对平破/复", brk && fix, String::from("Set=Remove+存量")));
    // 边界：联签守恒（先破后补）。
    let mut pc = SetRmGate::new();
    let idc = pc.set_pkt(-1, 8, 0x44);
    let _ = pc.remove_pkt(-1, 8, 0x44);
    let cbrk = pc.cosign_parity() == Err(SetRmCode::CosignParityBreak.val());
    let _ = pc.cosign(idc.unwrap_or(0));
    let cfix = pc.cosign_parity().is_ok();
    v.push(("J4-联签守恒破/复", cbrk && cfix, String::from("先破后补")));
    // 边界：键窗卡边（0x40 放行 / 0x4F 放行 / 0x50 拒）。
    let mut pw = SetRmGate::new();
    let at_lo = pw.set_pkt(-1, 1, KEY_WIN.0).is_ok();
    let at_hi = pw.set_pkt(-1, 1, KEY_WIN.1 - 1).is_ok();
    let over = pw.set_pkt(-1, 1, KEY_WIN.1) == Err(SetRmCode::KeyOutOfWin.val());
    v.push(("J4-键窗卡边", at_lo && at_hi && over, String::from("窗内放行/窗外拒")));
    // 轮换闭集点验。
    let closed = PktField::by_round(0) == PktField::Status
        && PktField::by_round(1) == PktField::Information
        && PktField::by_round(2) == PktField::Key
        && PktField::by_round(3) == PktField::All;
    v.push(("J5-篡改轮换闭集", closed, String::from("0/1/2/3→Status/Info/Key/All")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80740 窗口")));
    v
}

/// VX9 检查入口（卡 12629，UNX-F12630-J1 判据承载）。
pub fn run_unx12629_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12629 NtSetIoCompletion/NtRemoveIoCompletion→D4联签");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_field_mismatch_three_way() {
        let mut g = SetRmGate::new();
        let _ = g.set_pkt(-1, 8, 0x44);
        assert_eq!(g.remove_pkt(0, 8, 0x44), Err(SetRmCode::StatusMismatch.val()));
        assert_eq!(g.tamper_flow.last(), Some(&PktField::Status));
        let _ = g.remove_pkt(-1, 8, 0x44);
        assert!(g.set_rm_parity().is_ok());
    }

    #[test]
    fn test_tamper_inject_ten() {
        let (c, t) = tamper_inject_ten();
        assert_eq!(t, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn test_status_info_clash() {
        let mut g = SetRmGate::new();
        assert_eq!(g.set_pkt(0, 5, 0x44), Err(SetRmCode::StatusInfoClash.val()));
        assert!(g.set_pkt(0, 0, 0x44).is_ok());
    }

    #[test]
    fn test_key_window_edges() {
        let mut g = SetRmGate::new();
        assert!(g.set_pkt(-1, 1, KEY_WIN.0).is_ok());
        assert!(g.set_pkt(-1, 1, KEY_WIN.1 - 1).is_ok());
        assert_eq!(g.set_pkt(-1, 1, KEY_WIN.1), Err(SetRmCode::KeyOutOfWin.val()));
    }

    #[test]
    fn test_remove_empty_and_dup() {
        let mut g = SetRmGate::new();
        assert_eq!(g.remove_pkt(0, 0, 0), Err(SetRmCode::RemoveEmpty.val()));
        let _ = g.set_pkt(-1, 8, 0x44);
        let _ = g.remove_pkt(-1, 8, 0x44);
        assert_eq!(g.remove_pkt(-1, 8, 0x44), Err(SetRmCode::DupRemove.val()));
    }

    #[test]
    fn test_cosign_requires_removed() {
        let mut g = SetRmGate::new();
        let id = g.set_pkt(-1, 8, 0x44).unwrap_or(0);
        assert_eq!(g.cosign(id), Err(SetRmCode::SetRemoveParityBreak.val()));
        let _ = g.remove_pkt(-1, 8, 0x44);
        assert!(g.cosign(id).is_ok());
        assert_eq!(g.cosign(id), Err(SetRmCode::DupCosign.val()));
    }

    #[test]
    fn test_parity_break_and_fix() {
        let mut g = SetRmGate::new();
        let _ = g.set_pkt(-1, 8, 0x44);
        let _ = g.remove_pkt(-1, 8, 0x44);
        g.removed_ok = 9;
        assert_eq!(g.set_rm_parity(), Err(SetRmCode::SetRemoveParityBreak.val()));
        g.removed_ok = 1;
        assert_eq!(g.set_rm_parity(), Ok(1));
    }

    #[test]
    fn test_cosign_parity() {
        let mut g = SetRmGate::new();
        let _ = g.set_pkt(-1, 8, 0x44);
        let _ = g.set_pkt(-2, 8, 0x45);
        let _ = g.remove_pkt(-1, 8, 0x44);
        let _ = g.remove_pkt(-2, 8, 0x45);
        let _ = g.cosign(1);
        assert_eq!(g.cosign_parity(), Err(SetRmCode::CosignParityBreak.val()));
        let _ = g.cosign(2);
        assert!(g.cosign_parity().is_ok());
    }

    #[test]
    fn test_fifo_head_order() {
        let mut g = SetRmGate::new();
        let _ = g.set_pkt(-1, 1, 0x44);
        let _ = g.set_pkt(-2, 2, 0x45);
        // 先取头包（pkt_id=1），再取次包。
        let h = g.remove_pkt(-1, 1, 0x44).unwrap_or(CompPacket {
            pkt_id: 0,
            status: 0,
            information: 0,
            key: 0,
            removed: false,
            cosigned: false,
        });
        assert_eq!(h.pkt_id, 1);
        let n = g.remove_pkt(-2, 2, 0x45).unwrap_or(h);
        assert_eq!(n.pkt_id, 2);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80740);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in SetRmCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
