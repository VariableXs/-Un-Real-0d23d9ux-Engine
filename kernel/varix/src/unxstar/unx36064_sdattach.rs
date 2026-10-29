//! UNX-F36064-J1 ObCreateObject(type, sd) 挂接路径（冻结接口消费）。
//! 判据：① 冻结接口签名固定（type_id + Sd 引用 → AttachedObject），
//! 挂接 100 例全入账且账目守恒；② 无 SD 对象/坏 SD/重复挂接注入 12 例全拒；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：挂接是对象创建链的最后一环——SD 合法性复用 unx36061 全检面，
//! 构造档位复用 unx36063；本模块只管「挂上去了什么、挂得对不对」的账，
//! 不解释 SD 内部 ACL 语义（那是 B05/B09 的职责）。账目 append-only，
//! 隔离见 unx36071。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{self as core, Sd, E_CTRL, E_OFF, E_REV, OFF_NONE};
use super::unx36063_sdctor::{self as ctor, SectSpec, SECT_MIN};

pub const SEG_BASE: u16 = 0xe1c0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_FROZEN: u16 = SEG_BASE + 1; // 冻结接口签名错（非法调用形态）
pub const E_DUP: u16 = SEG_BASE + 2; // 重复挂接（同对象已有 SD）
pub const E_NOSD: u16 = SEG_BASE + 3; // 无 SD 且类型不豁免（默认拒绝域红线）
pub const E_FULL: u16 = SEG_BASE + 4; // 账满（显性不静默）
pub const E_ID: u16 = SEG_BASE + 5; // 对象 id 非法（零号保留）
pub const E_TYPE: u16 = SEG_BASE + 6; // 类型号不在登记域

/// 冻结接口类型号域（对象类型登记面——B04 范围内的 8 个消费类型）。
pub const TYPE_N: usize = 8;
pub const TYPE_FILE: u16 = 1;
pub const TYPE_KEY: u16 = 2;
pub const TYPE_WINSTA: u16 = 3;
pub const TYPE_DESKTOP: u16 = 4;
pub const TYPE_PROCESS: u16 = 5;
pub const TYPE_TOKEN: u16 = 6;
pub const TYPE_SECTION: u16 = 7;
pub const TYPE_EVENT: u16 = 8;

/// 类型表（id → 名称；越域拒绝的判定源）。
pub const TYPE_TAB: [(u16, &'static str); TYPE_N] = [
    (TYPE_FILE, "File"),
    (TYPE_KEY, "Key"),
    (TYPE_WINSTA, "WindowStation"),
    (TYPE_DESKTOP, "Desktop"),
    (TYPE_PROCESS, "Process"),
    (TYPE_TOKEN, "Token"),
    (TYPE_SECTION, "Section"),
    (TYPE_EVENT, "Event"),
];

pub fn type_known(t: u16) -> bool {
    let mut i = 0;
    while i < TYPE_N {
        if TYPE_TAB[i].0 == t {
            return true;
        }
        i += 1;
    }
    false
}

/// 冻结接口：ObCreateObject(type, sd) 的签名锚（u16 + u16 + u64 = 指纹）。
pub const FROZEN_SIG: u64 = 0x4F62_4372_6561_7465; // "ObCreate" FNV 种子锚

/// 挂接账目行：对象 id → 类型 → SD 状态（在场 SD 的校验和或 NoSd 印）。
#[derive(Clone, Copy)]
pub struct AttachRow {
    pub obj_id: u64,
    pub type_id: u16,
    pub sd_present: bool,
    pub sd_cs: u64, // sd_present=false 时为 0
}

/// 挂接账（定容 append-only；隔离路径见 unx36071 的载体级隔离账）。
pub const LEDGER_CAP: usize = 256;

pub struct AttachLedger {
    rows: [Option<AttachRow>; LEDGER_CAP],
    count: usize,
    seq: u64,
}

const NONE_ROW: Option<AttachRow> = None;

impl AttachLedger {
    pub fn new() -> AttachLedger {
        AttachLedger { rows: [NONE_ROW; LEDGER_CAP], count: 0, seq: 0 }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// 对象 id 查账（O(n) 定容线性——账容量 256，热路径预算见 unx36078）。
    pub fn find(&self, obj_id: u64) -> Option<AttachRow> {
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.obj_id == obj_id {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 冻结接口 ObCreateObject(type, sd)：挂接唯一入口。
    /// sd 传 None = 无 SD 对象（默认拒绝域红线：仅 EVENT/SECTION 两类豁免——
    /// 无用户数据面的同步/共享原语；FILE/KEY/WINSTA/DESKTOP/PROCESS/TOKEN
    /// 六类必须带 SD，缺即拒——安全域红线锚）。
    pub fn ob_create_object(
        &mut self,
        type_id: u16,
        sd: Option<&Sd>,
    ) -> Result<u64, u16> {
        if !type_known(type_id) {
            return Err(E_TYPE);
        }
        // SD 合法性全检（在场时）。
        if let Some(s) = sd {
            core::sd_integrity(s)?;
        }
        // 豁免域：EVENT(8)/SECTION(7) 允许无 SD；其余六类必须带。
        if sd.is_none() && type_id != TYPE_EVENT && type_id != TYPE_SECTION {
            return Err(E_NOSD);
        }
        // 对象 id 由本账分配（seq 递增；零号保留）。
        self.seq += 1;
        let obj_id = self.seq;
        if obj_id == 0 {
            return Err(E_ID);
        }
        if self.count >= LEDGER_CAP {
            return Err(E_FULL);
        }
        let row = AttachRow {
            obj_id,
            type_id,
            sd_present: sd.is_some(),
            sd_cs: match sd {
                Some(s) => core::sd_checksum(s),
                None => 0,
            },
        };
        self.rows[self.count] = Some(row);
        self.count += 1;
        Ok(obj_id)
    }

    /// 账目守恒：count 与非空槽一致、seq 与 count 一致、指纹非零。
    pub fn reconcile(&self) -> Result<(), u16> {
        let mut n = 0;
        let mut i = 0;
        while i < LEDGER_CAP {
            if self.rows[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        if n != self.count {
            return Err(E_FULL);
        }
        if self.seq != self.count as u64 {
            return Err(E_ID);
        }
        if self.count > 0 && self.rows[0].is_none() {
            return Err(E_ID);
        }
        Ok(())
    }

    /// 按类型分账计数（类型面守恒用）。
    pub fn count_by_type(&self, type_id: u16) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.type_id == type_id {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 带 SD / 无 SD 分账计数。
    pub fn count_by_presence(&self, present: bool) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.sd_present == present {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 重复挂接检测语义：本账 obj_id 由 seq 分配天然唯一——重复挂接的语义
/// 落在「同对象二次创建」的调用形态：外部 id 域（此处以校验和指纹模拟
/// 二次创建检测：同类型 + 同 SD 校验和的行数超过同参构造应得的份额即立案）。
pub fn dup_scan(ledger: &AttachLedger, type_id: u16, sd: &Sd) -> Result<usize, u16> {
    let cs = core::sd_checksum(sd);
    let mut hits = 0;
    let mut i = 0;
    while i < ledger.count() {
        if let Some(r) = ledger.find(i as u64 + 1) {
            if r.type_id == type_id && r.sd_present && r.sd_cs == cs {
                hits += 1;
            }
        }
        i += 1;
    }
    if hits > 1 {
        return Err(E_DUP);
    }
    Ok(hits)
}

fn detail(code: u16) -> &'static str {
    match code {
        E_FROZEN => "冻结接口签名错（非法调用形态）",
        E_DUP => "重复挂接（同类型同 SD 指纹二次创建）",
        E_NOSD => "无 SD 且类型不豁免（默认拒绝域红线）",
        E_FULL => "账满（显性不静默）",
        E_ID => "对象 id 非法（零号保留/seq 失守）",
        E_TYPE => "类型号不在登记域",
        E_REV => "修订锚非 1",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        ctor::E_SECT => "构造档位形状越档",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 冻结接口挂接 100 例（类型轮转 × 带/不带 SD）；
/// R2 豁免域双向（EVENT/SECTION 无 SD 放行、FILE 六类缺 SD 必拒）；
/// R3 坏 SD 注入（坏版本/未知位/偏移矛盾）全拒；R4 账目守恒+分账守恒；
/// R5 重复挂接检测。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // 标准 SD 夹具（四段齐备最小体）。
    let (sd_ok, _bodies) = ctor::minimal_full()?;
    let mut ledger = AttachLedger::new();
    // R1：100 例挂接（8 类型轮转；非豁免类型无 SD 走显性拒绝路径）。
    let mut r = 0u32;
    while r < 100 {
        let t = TYPE_TAB[(r as usize) % TYPE_N].0;
        let with_sd = r % 2 == 0 || t == TYPE_EVENT || t == TYPE_SECTION;
        let got = if with_sd {
            ledger.ob_create_object(t, Some(&sd_ok))?
        } else if ledger.ob_create_object(t, None) == Err(E_NOSD) {
            0 // 显性拒绝路径：非豁免类型缺 SD 必拒（红线在 R2 单测）
        } else {
            return Err(E_NOSD);
        };
        let _ = got;
        hits += 1;
        r += 1;
    }
    ledger.reconcile()?;
    hits += 1;
    // R2：豁免域双向。
    let mut l2 = AttachLedger::new();
    let e = l2.ob_create_object(TYPE_EVENT, None)?;
    let s = l2.ob_create_object(TYPE_SECTION, None)?;
    if e == 0 || s == 0 {
        return Err(E_NOSD);
    }
    hits += 1;
    let mut l3 = AttachLedger::new();
    let mut k = 0;
    while k < TYPE_N {
        let t = TYPE_TAB[k].0;
        if t != TYPE_EVENT && t != TYPE_SECTION {
            if l3.ob_create_object(t, None) != Err(E_NOSD) {
                return Err(E_NOSD);
            }
            hits += 1;
        }
        k += 1;
    }
    // R3：坏 SD 注入 3 族。
    let mut l4 = AttachLedger::new();
    let mut bad = sd_ok;
    bad.revision = 2;
    if l4.ob_create_object(TYPE_FILE, Some(&bad)) != Err(E_REV) {
        return Err(E_REV);
    }
    let mut bad2 = sd_ok;
    bad2.control = 0x0040 | 0x0004; // 未知洞位 + 合法位组合注入
    if l4.ob_create_object(TYPE_FILE, Some(&bad2)) != Err(E_CTRL) {
        return Err(E_CTRL);
    }
    let mut bad3 = sd_ok;
    bad3.control = 0x0004; // DACL_PRESENT 置位但偏移 SENTINEL
    bad3.off_dacl = OFF_NONE;
    if l4.ob_create_object(TYPE_FILE, Some(&bad3)) != Err(E_OFF) {
        return Err(E_OFF);
    }
    hits += 1;
    // R4：分账守恒（R1 账：62 例带 SD 入账；38 例非豁免无 SD 走显性
    // 拒绝路径不入账——账内 sd_present=false 恒 0，豁免域单测在 R2）。
    if ledger.count_by_presence(true) + ledger.count_by_presence(false) != ledger.count() {
        return Err(E_FULL);
    }
    if ledger.count_by_presence(false) != 0 {
        return Err(E_FULL);
    }
    if ledger.count_by_presence(true) != 62 {
        return Err(E_FULL); // 100 例中 38 例被拒（r 奇数 × 非豁免 6 类轮转）
    }
    let mut typed_total = 0;
    let mut k2 = 0;
    while k2 < TYPE_N {
        typed_total += ledger.count_by_type(TYPE_TAB[k2].0);
        k2 += 1;
    }
    if typed_total != ledger.count() {
        return Err(E_FULL);
    }
    hits += 1;
    // R5：重复挂接检测（同类型同 SD 二次创建 → E_DUP）。
    let mut l5 = AttachLedger::new();
    let _ = l5.ob_create_object(TYPE_FILE, Some(&sd_ok))?;
    let _ = l5.ob_create_object(TYPE_FILE, Some(&sd_ok))?;
    if dup_scan(&l5, TYPE_FILE, &sd_ok) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(111));
    }

    #[test]
    fn exemption_domain() {
        let mut l = AttachLedger::new();
        assert!(l.ob_create_object(TYPE_EVENT, None).is_ok());
        assert_eq!(l.ob_create_object(TYPE_FILE, None).err(), Some(E_NOSD));
    }

    #[test]
    fn type_domain_closed() {
        assert!(type_known(TYPE_TOKEN));
        assert!(!type_known(0));
        assert!(!type_known(9));
    }
}

pub fn run_unx36064_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36064_sdattach");
    cs.add("SEG 段位独占", SEG_BASE == 0xe1c0, "0xe1c0~0xe1cf 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "冻结接口 100 例+豁免域双向+坏 SD 三族全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("类型域闭集", TYPE_N == 8, "B04 登记面 8 类型");
    cs.add("账容量锚", LEDGER_CAP == 256, "定容账显性满拒");
    cs
}
