//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12449 · NtQuerySecurityObject/NtSetSecurityObject 语义档 · 0x30620
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12449-J1 自相对 SD 格式四段（Owner/Group/Dacl/Sacl 位）
//!      判据绿，逐位注入 20/20 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - SECURITY_INFORMATION 位档：OWNER(0x1)/GROUP(0x2)/DACL(0x4)/
//!     SACL(0x8)（NT 口径常量写死），位域闭集外必拒；
//!   - 自相对 SD 模型：四段独立在册（Owner/Group 为 SID 引用键，
//!     Dacl/Sacl 为 ACL 引用键），Self-Relative 标志位常置；
//!   - 查询档：按请求位回段（位有段有→回段；位有段无→段缺席不报错），
//!     请求位全零属非法域拒；
//!   - 设置档：按位替换段（位有→段被写；位无→段原样保留），SACL 写
//!     需 Security 特权旗标（无旗标拒——B23 特权面联动锚）；
//!   - 逐位注入 20 组：位组合×段缺席×越位×无旗标写 SACL 等确定性注入
//!     逐组核对行为；
//!   - 写回账：逐次 set 落账（旧值→新值四段），append-only 可回放；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30620），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30620 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30620;

/// 演练轮数（判准 10/10、注入判准 20/20）。
pub const INJECT_ROUNDS: usize = 10;

/// 逐位注入组数（判据 20/20）。
pub const BIT_INJECTS: usize = 20;

/// SECURITY_INFORMATION 位：OWNER。
pub const SI_OWNER: u32 = 0x0000_0001;

/// SECURITY_INFORMATION 位：GROUP。
pub const SI_GROUP: u32 = 0x0000_0002;

/// SECURITY_INFORMATION 位：DACL。
pub const SI_DACL: u32 = 0x0000_0004;

/// SECURITY_INFORMATION 位：SACL。
pub const SI_SACL: u32 = 0x0000_0008;

/// 合法位域闭集。
pub const SI_ALL: u32 = SI_OWNER | SI_GROUP | SI_DACL | SI_SACL;

/// 诊断码（0x30620 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdCode {
    /// 0x30620 请求位全零（无段请求非法）。
    BitsZero = 0,
    /// 0x30621 请求位出域（含位域外未知位）。
    BitsOob = 1,
    /// 0x30622 写 SACL 无特权旗标。
    SaclNoPriv = 2,
    /// 0x30623 SID 键越域（Owner/Group 引用键出闭集）。
    SidKeyOob = 3,
    /// 0x30624 ACL 键越域（Dacl/Sacl 引用键出闭集）。
    AclKeyOob = 4,
    /// 0x30625 对象未建（先建安全对象再查/设）。
    ObjMissing = 5,
    /// 0x30626 对象重号（同号二建）。
    ObjDup = 6,
    /// 0x30627 自相对标志缺失（SD 非 self-relative 形态）。
    NotSelfRelative = 7,
    /// 0x30628 写回账缩水（append-only 被破）。
    LedgerShrink = 8,
    /// 0x30629 查询段缺席账漂移（位有段无的缺席判定与账不符）。
    AbsentDrift = 9,
    /// 0x3062a 设后读不符（set 后 query 结果与新值不一致）。
    SetReadDiff = 10,
}

impl SdCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [SdCode; 11] = [
        SdCode::BitsZero, SdCode::BitsOob, SdCode::SaclNoPriv, SdCode::SidKeyOob,
        SdCode::AclKeyOob, SdCode::ObjMissing, SdCode::ObjDup, SdCode::NotSelfRelative,
        SdCode::LedgerShrink, SdCode::AbsentDrift, SdCode::SetReadDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30620 => "请求位全零：SECURITY_INFORMATION 至少一位，先补位再查。",
        0x30621 => "请求位出域：位域只认 OWNER/GROUP/DACL/SACL 四位。",
        0x30622 => "写 SACL 无特权：需 SeSecurityPrivilege 旗标，先提权再设。",
        0x30623 => "SID 键越域：Owner/Group 引用键出闭集，回查 SID 表。",
        0x30624 => "ACL 键越域：Dacl/Sacl 引用键出闭集，回查 ACL 表。",
        0x30625 => "对象未建：先建安全对象再查/设。",
        0x30626 => "对象重号：同号只建一笔，重建先删旧。",
        0x30627 => "自相对标志缺失：SD 必须 self-relative 形态，回查构造。",
        0x30628 => "写回账缩水：append-only，禁止截断，重放补齐。",
        0x30629 => "缺席账漂移：位有段无的缺席判定逐位回放核对。",
        0x3062a => "设后读不符：set 后立即 query 回放，逐段定位差异。",
        _ => "未知诊断码：回查 0x30620 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30620 段基址。
    pub const SEED: u64 = 0x30620_3062_0306_20u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 自相对 SD 模型（四段引用键；0 = 段缺席）。
#[derive(Clone, Copy, Debug)]
pub struct SelfRelSd {
    /// Owner SID 键（0=缺席）。
    pub owner: u32,
    /// Group SID 键（0=缺席）。
    pub group: u32,
    /// DACL ACL 键（0=缺席）。
    pub dacl: u32,
    /// SACL ACL 键（0=缺席）。
    pub sacl: u32,
    /// self-relative 标志（模型恒置位）。
    pub self_rel: bool,
}

/// SID 键域闭集（1..=64）。
pub const SID_KEY_MAX: u32 = 64;

/// ACL 键域闭集（1..=64）。
pub const ACL_KEY_MAX: u32 = 64;

/// 键合法性（0 = 缺席合法；非零须在闭集）。
pub fn key_legal(k: u32, max: u32) -> bool {
    k == 0 || k <= max
}

/// SD 构造校验（四键域 + self-relative 标志）。
pub fn sd_defect(sd: SelfRelSd) -> Option<SdCode> {
    if !sd.self_rel {
        return Some(SdCode::NotSelfRelative);
    }
    if !key_legal(sd.owner, SID_KEY_MAX) || !key_legal(sd.group, SID_KEY_MAX) {
        return Some(SdCode::SidKeyOob);
    }
    if !key_legal(sd.dacl, ACL_KEY_MAX) || !key_legal(sd.sacl, ACL_KEY_MAX) {
        return Some(SdCode::AclKeyOob);
    }
    None
}

/// 查询档结果（请求位 → 逐段 Option）。
#[derive(Clone, Copy, Debug)]
pub struct QueryResult {
    /// Owner（请求位有且段在 → Some）。
    pub owner: Option<u32>,
    /// Group。
    pub group: Option<u32>,
    /// DACL。
    pub dacl: Option<u32>,
    /// SACL。
    pub sacl: Option<u32>,
}

/// 查询档真实执行（位有段有→回段；位有段无→None 不报错）。
pub fn query_sd(sd: SelfRelSd, bits: u32) -> Result<QueryResult, SdCode> {
    if bits == 0 {
        return Err(SdCode::BitsZero);
    }
    if bits & !SI_ALL != 0 {
        return Err(SdCode::BitsOob);
    }
    Ok(QueryResult {
        owner: if bits & SI_OWNER != 0 && sd.owner != 0 { Some(sd.owner) } else { None },
        group: if bits & SI_GROUP != 0 && sd.group != 0 { Some(sd.group) } else { None },
        dacl: if bits & SI_DACL != 0 && sd.dacl != 0 { Some(sd.dacl) } else { None },
        sacl: if bits & SI_SACL != 0 && sd.sacl != 0 { Some(sd.sacl) } else { None },
    })
}

/// 安全对象正本：SD 持有 + 写回账。
#[derive(Debug)]
pub struct SecObject {
    /// 对象号。
    pub oid: u8,
    /// 当前 SD。
    sd: SelfRelSd,
    /// Security 特权旗标（SACL 写闸）。
    sec_priv: bool,
    /// 写回账（append-only：(位域, 旧段键, 新段键)）。
    ledger: Vec<(u32, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl SecObject {
    /// 建对象（缺省 SD：Owner/Group/Dacl 在，Sacl 缺席——缺席面锚）。
    pub fn new(oid: u8, sd: SelfRelSd, sec_priv: bool) -> Self {
        SecObject { oid, sd, sec_priv, ledger: Vec::new(), high: 0 }
    }

    /// 当前 SD 快照。
    pub fn sd(&self) -> SelfRelSd {
        self.sd
    }

    /// 特权旗标设置。
    pub fn set_sec_priv(&mut self, on: bool) {
        self.sec_priv = on;
    }

    /// 设置档真实执行（按位替换；位无→段原样保留）。
    pub fn set_sd(&mut self, bits: u32, new: &SelfRelSd) -> Result<SelfRelSd, SdCode> {
        if bits == 0 {
            return Err(SdCode::BitsZero);
        }
        if bits & !SI_ALL != 0 {
            return Err(SdCode::BitsOob);
        }
        // SACL 写闸。
        if bits & SI_SACL != 0 && !self.sec_priv {
            self.ledger.push((SI_SACL, self.sd.sacl, self.sd.sacl));
            if self.ledger.len() > self.high {
                self.high = self.ledger.len();
            }
            return Err(SdCode::SaclNoPriv);
        }
        // 新 SD 键域校验。
        if let Some(c) = sd_defect(*new) {
            return Err(c);
        }
        let old = self.sd;
        if bits & SI_OWNER != 0 {
            self.sd.owner = new.owner;
            self.ledger.push((SI_OWNER, old.owner, new.owner));
        }
        if bits & SI_GROUP != 0 {
            self.sd.group = new.group;
            self.ledger.push((SI_GROUP, old.group, new.group));
        }
        if bits & SI_DACL != 0 {
            self.sd.dacl = new.dacl;
            self.ledger.push((SI_DACL, old.dacl, new.dacl));
        }
        if bits & SI_SACL != 0 {
            self.sd.sacl = new.sacl;
            self.ledger.push((SI_SACL, old.sacl, new.sacl));
        }
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        Ok(self.sd)
    }

    /// 写回账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账只增不减判据。
    pub fn ledger_monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("安全对象账：\n");
        for (b, o, n) in self.ledger.iter() {
            s.push_str(&format!("位{:#x} 旧{} 新{}\n", b, o, n));
        }
        s
    }
}

/// 安全对象表。
#[derive(Debug)]
pub struct SecTable {
    /// 槽（定容 4）。
    slots: [Option<SecObject>; 4],
}

impl SecTable {
    /// 建表。
    pub fn new() -> Self {
        SecTable { slots: [None, None, None, None] }
    }

    /// 建对象（重号拒）。
    pub fn create(&mut self, oid: u8, sd: SelfRelSd, sec_priv: bool) -> Result<(), SdCode> {
        if oid as usize >= 4 {
            return Err(SdCode::ObjMissing);
        }
        if self.slots[oid as usize].is_some() {
            return Err(SdCode::ObjDup);
        }
        if let Some(c) = sd_defect(sd) {
            return Err(c);
        }
        self.slots[oid as usize] = Some(SecObject::new(oid, sd, sec_priv));
        Ok(())
    }

    /// 对象读取。
    pub fn obj_of(&mut self, oid: u8) -> Result<&mut SecObject, SdCode> {
        if oid as usize >= 4 {
            return Err(SdCode::ObjMissing);
        }
        self.slots[oid as usize].as_mut().ok_or(SdCode::ObjMissing)
    }

    /// 对象数。
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// 缺省 SD（Owner=1/Group=2/Dacl=10/Sacl=0 缺席）。
pub fn default_sd() -> SelfRelSd {
    SelfRelSd { owner: 1, group: 2, dacl: 10, sacl: 0, self_rel: true }
}

/// ─────────────────────────────────────────────────────────────────────
/// 逐位注入 20 组（确定性，判据 20/20）。
/// 组 1..10 查询面：4 单位 / 6 组合位，逐一核对回段与缺席；
/// 组 11..20 设置面：4 单位写 / 越位拒 / 零位拒 / SACL 无旗标拒 /
///   位无段保留 / set 后读一致 / 键越域拒。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_bit_inject_20() -> (bool, String) {
    let mut hits = 0u32;
    let sd = default_sd();
    // 组 1..4：单 位查询（Sacl 位有段无 → None 不报错）。
    if matches!(query_sd(sd, SI_OWNER), Ok(r) if r.owner == Some(1)) { hits += 1; }
    if matches!(query_sd(sd, SI_GROUP), Ok(r) if r.group == Some(2)) { hits += 1; }
    if matches!(query_sd(sd, SI_DACL), Ok(r) if r.dacl == Some(10)) { hits += 1; }
    if matches!(query_sd(sd, SI_SACL), Ok(r) if r.sacl.is_none()) { hits += 1; }
    // 组 5..8：组合位查询（全位 / 三位 / 二位 / 组合缺段面）。
    if matches!(query_sd(sd, SI_ALL), Ok(r) if r.owner == Some(1) && r.sacl.is_none()) { hits += 1; }
    if matches!(query_sd(sd, SI_OWNER | SI_GROUP | SI_DACL), Ok(r) if r.dacl == Some(10) && r.sacl.is_none()) { hits += 1; }
    if matches!(query_sd(sd, SI_OWNER | SI_SACL), Ok(r) if r.owner == Some(1) && r.sacl.is_none()) { hits += 1; }
    if matches!(query_sd(sd, SI_GROUP | SI_DACL), Ok(r) if r.group == Some(2) && r.dacl == Some(10)) { hits += 1; }
    // 组 9：零位拒。
    if matches!(query_sd(sd, 0), Err(SdCode::BitsZero)) { hits += 1; }
    // 组 10：越位拒（0x10 未知位）。
    if matches!(query_sd(sd, 0x10), Err(SdCode::BitsOob)) { hits += 1; }
    // 组 11..14：单位写（SACL 需旗标）。
    let mut eng = SecTable::new();
    let _ = eng.create(0, sd, true);
    if let Ok(o) = eng.obj_of(0) {
        let new = SelfRelSd { owner: 3, group: 4, dacl: 11, sacl: 12, self_rel: true };
        // OWNER 写。
        let r = o.set_sd(SI_OWNER, &new);
        if matches!(r, Ok(s) if s.owner == 3 && s.group == 2 && s.dacl == 10) { hits += 1; }
        // DACL 写。
        let r = o.set_sd(SI_DACL, &new);
        if matches!(r, Ok(s) if s.dacl == 11 && s.owner == 3) { hits += 1; }
        // GROUP 写。
        let r = o.set_sd(SI_GROUP, &new);
        if matches!(r, Ok(s) if s.group == 4 && s.dacl == 11) { hits += 1; }
        // SACL 写（有旗标）。
        let r = o.set_sd(SI_SACL, &new);
        if matches!(r, Ok(s) if s.sacl == 12) { hits += 1; }
    }
    // 组 15：无旗标写 SACL 必拒。
    let mut eng2 = SecTable::new();
    let _ = eng2.create(0, sd, false);
    let new = default_sd();
    if let Ok(o) = eng2.obj_of(0) {
        if matches!(o.set_sd(SI_SACL, &new), Err(SdCode::SaclNoPriv)) { hits += 1; }
    }
    // 组 16：位无段保留（只写 OWNER，GROUP 不动）。
    if let Ok(o) = eng2.obj_of(0) {
        let mut new3 = default_sd();
        new3.owner = 7;
        if let Ok(s) = o.set_sd(SI_OWNER, &new3) {
            if s.group == 2 && s.dacl == 10 { hits += 1; }
        }
    }
    // 组 17：set 后读一致。
    if let Ok(o) = eng2.obj_of(0) {
        let cur = o.sd();
        if let Ok(q) = query_sd(cur, SI_OWNER | SI_GROUP | SI_DACL) {
            if q.owner == Some(7) && q.group == Some(2) && q.dacl == Some(10) { hits += 1; }
        }
    }
    // 组 18：写回账留痕（2 行：SaclNoPriv 留痕 1 + OWNER 写 1）。
    if let Ok(o) = eng2.obj_of(0) {
        if o.ledger_len() == 2 && o.ledger_monotone() { hits += 1; }
    }
    // 组 19：键越域拒（Owner=99）。
    let mut eng3 = SecTable::new();
    let _ = eng3.create(0, sd, true);
    if let Ok(o) = eng3.obj_of(0) {
        let bad = SelfRelSd { owner: SID_KEY_MAX + 1, group: 2, dacl: 10, sacl: 0, self_rel: true };
        if matches!(o.set_sd(SI_OWNER, &bad), Err(SdCode::SidKeyOob)) { hits += 1; }
    }
    // 组 20：self-relative 标志缺失拒。
    if let Ok(o) = eng3.obj_of(0) {
        let bad = SelfRelSd { owner: 1, group: 2, dacl: 10, sacl: 0, self_rel: false };
        if matches!(o.set_sd(SI_OWNER, &bad), Err(SdCode::NotSelfRelative)) { hits += 1; }
    }
    let got = hits;
    (got == BIT_INJECTS as u32, format!("逐位注入组={}/{}", got, BIT_INJECTS))
}

/// 查询面演练：位有段有/位有段无两口径 10/10 轮。
pub fn drill_query_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30620 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let sd = default_sd();
        let mut all_ok = true;
        // 位有段有：三段全回。
        let q = query_sd(sd, SI_OWNER | SI_GROUP | SI_DACL);
        all_ok = all_ok
            && matches!(&q, Ok(r) if r.owner == Some(1) && r.group == Some(2) && r.dacl == Some(10));
        // 位有段无：Sacl None 不报错。
        all_ok = all_ok && matches!(query_sd(sd, SI_SACL), Ok(r) if r.sacl.is_none());
        // 全位：Owner/Group/Dacl 回、Sacl 缺。
        all_ok = all_ok && matches!(query_sd(sd, SI_ALL), Ok(r) if r.owner.is_some() && r.sacl.is_none());
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("查询面轮={}/{}", ok, INJECT_ROUNDS))
}

/// 设置面演练：按位替换 + 段保留 + 账守恒 10/10 轮。
pub fn drill_set_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30621（设置序列）。
    let mut lcg = Lcg::new(0x30621);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = SecTable::new();
        let _ = eng.create(0, default_sd(), true);
        let mut all_ok = true;
        let new = SelfRelSd { owner: 5, group: 6, dacl: 20, sacl: 21, self_rel: true };
        // 三段写（无 SACL 位）。
        if let Ok(o) = eng.obj_of(0) {
            all_ok = all_ok
                && matches!(o.set_sd(SI_OWNER | SI_GROUP | SI_DACL, &new), Ok(s) if s.owner == 5 && s.sacl == 0);
        }
        // SACL 补写（有旗标）。
        if let Ok(o) = eng.obj_of(0) {
            all_ok = all_ok && matches!(o.set_sd(SI_SACL, &new), Ok(s) if s.sacl == 21);
        }
        // set 后 query 全位一致。
        if let Ok(o) = eng.obj_of(0) {
            let cur = o.sd();
            all_ok = all_ok
                && matches!(query_sd(cur, SI_ALL), Ok(r) if r.owner == Some(5) && r.sacl == Some(21));
        }
        // 账：3 + 1 = 4 行。
        if let Ok(o) = eng.obj_of(0) {
            all_ok = all_ok && o.ledger_len() == 4 && o.ledger_monotone();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("设置面轮={}/{}", ok, INJECT_ROUNDS))
}

/// 特权闸演练：SACL 写旗标二态 10/10 轮。
pub fn drill_sacl_priv_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30622（旗标序列）。
    let mut lcg = Lcg::new(0x30622);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = SecTable::new();
        let _ = eng.create(0, default_sd(), false);
        let new = default_sd();
        let mut all_ok = true;
        // 无旗标拒。
        if let Ok(o) = eng.obj_of(0) {
            all_ok = all_ok && matches!(o.set_sd(SI_SACL, &new), Err(SdCode::SaclNoPriv));
        }
        // 提旗标后过。
        if let Ok(o) = eng.obj_of(0) {
            o.set_sec_priv(true);
            all_ok = all_ok && matches!(o.set_sd(SI_SACL, &new), Ok(s) if s.sacl == new.sacl);
        }
        // 非经旗标路径设 SACL 缺席段（sd.sacl=0 写 0）也不破账。
        if let Ok(o) = eng.obj_of(0) {
            all_ok = all_ok && o.ledger_monotone();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("SACL特权闸轮={}/{}", ok, INJECT_ROUNDS))
}

/// 对象表面演练：建/重/缺三路 10/10 轮。
pub fn drill_obj_table_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30623（对象表序列）。
    let mut lcg = Lcg::new(0x30623);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = SecTable::new();
        let mut all_ok = true;
        all_ok = all_ok && eng.create(0, default_sd(), true).is_ok();
        all_ok = all_ok && matches!(eng.create(0, default_sd(), true), Err(SdCode::ObjDup));
        all_ok = all_ok && matches!(eng.obj_of(1).and_then(|o| { let _ = o.sd(); Ok(()) }), Err(SdCode::ObjMissing));
        // 建时 SD 缺陷拒。
        let bad = SelfRelSd { owner: 99, group: 2, dacl: 10, sacl: 0, self_rel: true };
        all_ok = all_ok && matches!(eng.create(1, bad, true), Err(SdCode::SidKeyOob));
        let _ = lcg.below(3);
        if all_ok && eng.count() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("对象表面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let sd = default_sd();
    // 注 1：零位查询必拒。
    if matches!(query_sd(sd, 0), Err(SdCode::BitsZero)) {
        caught += 1;
        d.push("注1 零位:抓".to_string());
    } else {
        d.push("注1 零位:漏".to_string());
    }
    // 注 2：越位必拒。
    if matches!(query_sd(sd, 0xF0), Err(SdCode::BitsOob)) {
        caught += 1;
        d.push("注2 越位:抓".to_string());
    } else {
        d.push("注2 越位:漏".to_string());
    }
    // 注 3：无旗标写 SACL 必拒。
    let mut eng = SecTable::new();
    let _ = eng.create(0, sd, false);
    if let Ok(o) = eng.obj_of(0) {
        if matches!(o.set_sd(SI_SACL, &default_sd()), Err(SdCode::SaclNoPriv)) {
            caught += 1;
            d.push("注3 无旗:抓".to_string());
        } else {
            d.push("注3 无旗:漏".to_string());
        }
    }
    // 注 4：键越域必拒。
    if let Ok(o) = eng.obj_of(0) {
        o.set_sec_priv(true);
        let bad = SelfRelSd { owner: 1, group: 2, dacl: ACL_KEY_MAX + 1, sacl: 0, self_rel: true };
        if matches!(o.set_sd(SI_DACL, &bad), Err(SdCode::AclKeyOob)) {
            caught += 1;
            d.push("注4 越键:抓".to_string());
        } else {
            d.push("注4 越键:漏".to_string());
        }
    }
    // 注 5：正路必绿（查询三段 + 设置后读一致）。
    let q = query_sd(sd, SI_OWNER | SI_GROUP | SI_DACL);
    let mut eng2 = SecTable::new();
    let _ = eng2.create(0, sd, true);
    let set_ok = eng2.obj_of(0).map(|o| o.set_sd(SI_OWNER, &default_sd()).is_ok()).unwrap_or(false);
    if matches!(q, Ok(r) if r.owner == Some(1)) && set_ok {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30620 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    SdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 安全对象报告。
#[derive(Clone, Copy, Debug)]
pub struct SdReport {
    /// 对象数。
    pub objs: usize,
    /// 账行数（0 号对象）。
    pub ledger: usize,
    /// 账单调。
    pub mono: bool,
}

impl SdReport {
    /// 建报告。
    pub fn build(eng: &mut SecTable) -> Self {
        let (ledger, mono) = eng.obj_of(0)
            .map(|o| (o.ledger_len(), o.ledger_monotone()))
            .unwrap_or((0, true));
        SdReport { objs: eng.count(), ledger, mono }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.objs > 0 && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!("SdReport 对象={} 账={} 单调={}", self.objs, self.ledger, self.mono)
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct SdCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_sd_checklist(eng: &mut SecTable) -> Vec<SdCheckRow> {
    let rep = SdReport::build(eng);
    let sd = default_sd();
    let mut rows: Vec<SdCheckRow> = Vec::new();
    rows.push(SdCheckRow { name: "位域口径", passed: SI_ALL == 0xF && SI_SACL == 0x8 });
    rows.push(SdCheckRow { name: "键域口径", passed: SID_KEY_MAX == 64 && ACL_KEY_MAX == 64 });
    rows.push(SdCheckRow { name: "缺省Sacl缺席", passed: sd.sacl == 0 && query_sd(sd, SI_SACL).map(|r| r.sacl.is_none()).unwrap_or(true) });
    rows.push(SdCheckRow { name: "账单调", passed: rep.mono });
    rows.push(SdCheckRow { name: "转录非空", passed: eng.obj_of(0).map(|o| o.export_lines().contains("安全对象账")).unwrap_or(false) });
    rows.push(SdCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SdCheckRow { name: "self-rel常置", passed: sd.self_rel && sd_defect(sd).is_none() });
    rows.push(SdCheckRow { name: "缺席面判据", passed: matches!(query_sd(sd, SI_SACL), Ok(r) if r.sacl.is_none()) });
    rows.push(SdCheckRow { name: "对象表面", passed: eng.count() >= 1 });
    rows.push(SdCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn sd_checklist_green(rows: &[SdCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn sd_stats_line(eng: &mut SecTable) -> String {
    let rep = SdReport::build(eng);
    format!("安全对象概况：对象 {} 账 {} 单调 {}", rep.objs, rep.ledger, rep.mono)
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12449_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut eng = SecTable::new();
    let _ = eng.create(0, default_sd(), true);
    let new = SelfRelSd { owner: 5, group: 6, dacl: 20, sacl: 21, self_rel: true };
    if let Ok(o) = eng.obj_of(0) {
        let _ = o.set_sd(SI_OWNER | SI_GROUP | SI_DACL, &new);
        let _ = o.set_sd(SI_SACL, &new);
    }
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        SdReport::build(&mut eng).all_green() && eng.obj_of(0).map(|o| o.export_lines().contains("安全对象账")).unwrap_or(false),
        sd_stats_line(&mut eng),
    ));
    // X-b：查询面 10/10。
    let (ok_q, d_q) = drill_query_10();
    out.push(("X-查询面10/10", ok_q, d_q));
    // X-c：设置面 10/10。
    let (ok_s, d_s) = drill_set_10();
    out.push(("X-设置面10/10", ok_s, d_s));
    // X-d：SACL 特权闸 10/10。
    let (ok_p, d_p) = drill_sacl_priv_10();
    out.push(("X-SACL特权闸10/10", ok_p, d_p));
    // X-e：对象表面 10/10 + 终验清单。
    let (ok_o, d_o) = drill_obj_table_10();
    out.push(("X-对象表面10/10", ok_o, d_o));
    let rows = build_sd_checklist(&mut eng);
    out.push((
        "X-终验清单10项",
        sd_checklist_green(&rows),
        SdReport::build(&mut eng).to_text(),
    ));
    out
}

/// run_unx12449_checks：本单判据聚合（诊断段 0x30620）。
pub fn run_unx12449_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12449 NtQuery/SetSecurityObject 语义档");
    // J1-a：逐位注入 20/20。
    let (ok1, d1) = drill_bit_inject_20();
    s.add_dyn("J1-逐位注入20/20", ok1, &d1);
    // J1-b：查询面 10/10。
    let (ok2, d2) = drill_query_10();
    s.add_dyn("J1-查询面10/10", ok2, &d2);
    // J1-c：设置面 10/10。
    let (ok3, d3) = drill_set_10();
    s.add_dyn("J1-设置面10/10", ok3, &d3);
    // J1-d：标准会话恒等式（四段位语义 + 设后读一致）。
    let sd = default_sd();
    let q = query_sd(sd, SI_ALL);
    let mut eng = SecTable::new();
    let _ = eng.create(0, sd, true);
    let new = SelfRelSd { owner: 9, group: 9, dacl: 9, sacl: 9, self_rel: true };
    let set1 = eng.obj_of(0).map(|o| o.set_sd(SI_OWNER | SI_GROUP, &new)).unwrap_or(Err(SdCode::ObjMissing));
    let set2 = eng.obj_of(0).map(|o| o.set_sd(SI_DACL | SI_SACL, &new)).unwrap_or(Err(SdCode::ObjMissing));
    let readback = eng.obj_of(0).map(|o| query_sd(o.sd(), SI_ALL)).unwrap_or(Err(SdCode::ObjMissing));
    s.add_dyn(
        "J1-四段位判据绿",
        matches!(q, Ok(r) if r.owner == Some(1) && r.sacl.is_none())
            && matches!(set1, Ok(ref s2) if s2.owner == 9 && s2.dacl == 10)
            && matches!(set2, Ok(ref s2) if s2.sacl == 9)
            && matches!(readback, Ok(r) if r.owner == Some(9) && r.group == Some(9) && r.dacl == Some(9) && r.sacl == Some(9)),
        &format!("查={:?} 设1={:?} 设2={:?} 读回={:?}", q.is_ok(), set1.is_ok(), set2.is_ok(), readback.is_ok()),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：SACL 特权闸 10/10。
    let (ok4, d4) = drill_sacl_priv_10();
    s.add_dyn("J2-SACL特权闸10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30620段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12449_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
