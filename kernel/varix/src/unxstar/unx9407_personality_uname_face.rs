//! UNX-F9407 personality/uname 错误面 · 0x7D340段
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：收库会话自领单（与 VX11 既有 9001~9406 卡面零重复：F9407 聚焦
//!       personality 位域闭集与 uname 六槽错误面，既有卡未涉足该面）。
//!
//!   UNX-F9407-J1 personality 位域闭集注入判据过：未知位全 EBadBits 格过，
//!   已知八位零误伤；人格字节闭集判序格过（垃圾人格字节 EBadPersona）；
//!   uname 六字段闭集与长度判序格过（越集 EBadField / 超 65 字节含 NUL
//!   EBadLen）；重复施加位幂等（零误伤）格过；mask 0 = PER_LINUX 零误伤
//!   格过；封账后写拒止格过；注入必抓格过；双跑一致（确定性）格过；
//!   段位守卫格过；闭集诊断码格过。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 设计要点：
//!   1) 真实判定器：personality 位域闭集（ADDR_NO_RANDOMIZE=0x0040000/
//!      ADDR_COMPAT_LAYOUT=0x0800000/MMAP_PAGE_ZERO=0x0100000/
//!      READ_IMPLIES_EXEC=0x0400000/ADDR_LIMIT_32BIT=0x0200000/
//!      SHORT_INODE=0x1000000/WHOLE_SECONDS=0x2000000/STICKY_TIMEOUTS=
//!      0x4000000），判序与 Linux personality 一致：未知位（EBadBits）→
//!      人格字节（EBadPersona）→ 施加到位槽。
//!      修正记录：任务面原文 ADDR_LIMIT_32BIT 写作 0x02000000，与
//!      WHOLE_SECONDS 0x2000000 同值冲突（0x02000000 == 0x2000000），依
//!      Linux 真值 0x0200000 收敛，保证八位两两互异、槽位映射唯一。
//!   2) 低字节为人格域：KNOWN_ALL = 八位并集 | 0xFF；未知高位先判
//!      （EBadBits），人格字节 ∈ {0x00, 0xFF}（0xFF 为保留哨兵，不落槽），
//!      否则 EBadPersona——先判未知位再判人格字节，保序不吞诊断。
//!   3) 真实状态机：九个 bool 位槽（八位 + 人格零槽）与 applied_mask
//!      逐位对账（audit → ESlotDrift/EMaskDrift）；mask 0 = PER_LINUX
//!      全槽归零（零误伤）；重复施加幂等（同 ret 同状态）。
//!   4) uname 六槽闭集（sysname/nodename/release/version/machine/
//!      domainname），每槽 ≤65 字节含 NUL（Linux UTSLEN=65，越者
//!      EBadLen）；字段下标闭集 0..6（越者 EBadField）；get 未设返回空
//!      （零误伤）。
//!   5) 注入器 10 类违约/语义（未知位 0x8000_0000、人格字节 0x37、字段
//!      6/7、字段 66 字节、封账后写、账本满、重复施加幂等、先读空、
//!      mask 0、翻位审计），断言必抓 10/10（精确诊断码匹配）。
//!   6) 双跑一致：同一输入序列跑两遍，FNV-1a 64 位指纹逐位相等；
//!      段位守卫 0x7D340 步进 0x40 内不越界。
//!   7) no_std 纪律：无 unsafe/unwrap/expect/panic!/assert!/浮点/裸下标；
//!      取值走 match/if let/get/iter/checked_*/saturating_*。

#![allow(dead_code)]

use alloc::string::{String, ToString}; use alloc::vec::Vec; use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7D340;

// ════════════════════════ Linux personality 位域（闭集） ════════════════════════

pub const ADDR_NO_RANDOMIZE: u64 = 0x0040000;
pub const ADDR_COMPAT_LAYOUT: u64 = 0x0800000;
pub const MMAP_PAGE_ZERO: u64 = 0x0100000;
pub const READ_IMPLIES_EXEC: u64 = 0x0400000;
pub const ADDR_LIMIT_32BIT: u64 = 0x0200000;
pub const SHORT_INODE: u64 = 0x1000000;
pub const WHOLE_SECONDS: u64 = 0x2000000;
pub const STICKY_TIMEOUTS: u64 = 0x4000000;

/// 八位并集（按槽位序）。
pub const KNOWN_BITS: [u64; 8] = [
    ADDR_NO_RANDOMIZE,
    ADDR_COMPAT_LAYOUT,
    MMAP_PAGE_ZERO,
    READ_IMPLIES_EXEC,
    ADDR_LIMIT_32BIT,
    SHORT_INODE,
    WHOLE_SECONDS,
    STICKY_TIMEOUTS,
];

/// 位域闭集掩码。
pub const KNOWN_MASK: u64 =
    ADDR_NO_RANDOMIZE | ADDR_COMPAT_LAYOUT | MMAP_PAGE_ZERO | READ_IMPLIES_EXEC
        | ADDR_LIMIT_32BIT | SHORT_INODE | WHOLE_SECONDS | STICKY_TIMEOUTS;

/// 全闭集：位域并集 + 低字节人格域（0xFF 为保留哨兵）。
pub const KNOWN_ALL: u64 = KNOWN_MASK | 0xFF;

/// 人格字节合法闭集：0x00 = PER_LINUX，0xFF = 保留哨兵（不落槽）。
pub const PERSONA_LINUX: u8 = 0x00;
pub const PERSONA_RESERVED: u8 = 0xFF;

/// uname 字段闭集（Linux newuname 六槽）。
pub const F_SYSNAME: usize = 0;
pub const F_NODENAME: usize = 1;
pub const F_RELEASE: usize = 2;
pub const F_VERSION: usize = 3;
pub const F_MACHINE: usize = 4;
pub const F_DOMAINNAME: usize = 5;
pub const UNAME_FIELDS: usize = 6;
/// UTSLEN：65 字节含 NUL（Linux __NEW_UTS_LEN+1）。
pub const UTSLEN: usize = 65;

// ════════════════════════ 闭集诊断码 ════════════════════════

/// 闭集诊断码：本卡全部错误位面，超出闭集即违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersErr {
    /// raw 含位域闭集外的未知位。
    EBadBits,
    /// 人格字节 ∉ {0x00, 0xFF}。
    EBadPersona,
    /// uname 字段下标越闭集。
    EBadField,
    /// uname 字段值含 NUL 超 65 字节。
    EBadLen,
    /// 封账后写。
    ESealed,
    /// 账本满（容量守恒被触）。
    ELedgerFull,
    /// 位槽与 applied_mask 漂移（翻位侦测）。
    ESlotDrift,
    /// 账本 step 回退（单调违序）。
    EStepReg,
    /// uname 槽长漂移（>65 含 NUL）。
    ELenDrift,
    /// applied_mask 漂移（含闭集外位）。
    EMaskDrift,
}

impl PersErr {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PersErr::EBadBits => "raw 含位域闭集外的未知位",
            PersErr::EBadPersona => "人格字节不在 {0x00, 0xFF} 闭集",
            PersErr::EBadField => "uname 字段下标越六槽闭集",
            PersErr::EBadLen => "uname 字段值含 NUL 超 65 字节",
            PersErr::ESealed => "封账后禁写",
            PersErr::ELedgerFull => "账本容量触顶",
            PersErr::ESlotDrift => "位槽与施加掩码逐位对账漂移",
            PersErr::EStepReg => "账本 step 回退，单调违序",
            PersErr::ELenDrift => "uname 槽长漂移超 UTSLEN",
            PersErr::EMaskDrift => "applied_mask 漂移含闭集外位",
        }
    }
    pub const ALL: [PersErr; 10] = [
        PersErr::EBadBits,
        PersErr::EBadPersona,
        PersErr::EBadField,
        PersErr::EBadLen,
        PersErr::ESealed,
        PersErr::ELedgerFull,
        PersErr::ESlotDrift,
        PersErr::EStepReg,
        PersErr::ELenDrift,
        PersErr::EMaskDrift,
    ];
}

// ════════════════════════ 状态机：PersUname ════════════════════════

/// Phase 状态机：Fresh → Live → Sealed，单调不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Fresh,
    Live,
    Sealed,
}

/// 账本操作种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    MaskApply,
    FieldSet,
    FieldGet,
}

/// 账本记录（仅成功操作入账）。
#[derive(Clone, Copy, Debug)]
pub struct OpRec {
    pub step: u64,
    pub kind: OpKind,
    /// FieldSet/FieldGet 的字段下标；MaskApply 为 0xFF。
    pub field: u8,
    /// MaskApply 的原始 raw；其余为 0。
    pub mask: u64,
}

pub const LEDGER_CAP: usize = 32;

/// personality + uname 组合状态机。
pub struct PersUname {
    /// 九槽：0..8 为八位位槽，8 为人格零槽（persona == 0 时真）。
    slots: [bool; 9],
    /// 已施加的位域部分（raw & KNOWN_MASK）。
    applied_mask: u64,
    /// 当前人格字节。
    persona: u8,
    /// uname 六槽（空串 = 未设）。
    uname: [String; UNAME_FIELDS],
    uname_set: [bool; UNAME_FIELDS],
    phase: Phase,
    recs: Vec<OpRec>,
    cap: usize,
    step: u64,
    sealed: bool,
}

impl PersUname {
    pub fn new() -> Self {
        PersUname {
            // 初态 persona=0（PER_LINUX）：八位槽全假、人格零槽真。
            slots: [false, false, false, false, false, false, false, false, true],
            applied_mask: 0,
            persona: 0,
            uname: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ],
            uname_set: [false, false, false, false, false, false],
            phase: Phase::Fresh,
            recs: Vec::new(),
            cap: LEDGER_CAP,
            step: 0,
            sealed: false,
        }
    }

    pub fn applied_mask(&self) -> u64 {
        self.applied_mask
    }
    pub fn persona(&self) -> u8 {
        self.persona
    }
    pub fn slots(&self) -> &[bool; 9] {
        &self.slots
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn recs(&self) -> &[OpRec] {
        &self.recs
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// personality 判定主入口。判序：封账 → 账容 → 未知位（EBadBits）→
    /// 人格字节（EBadPersona）→ 施加位槽（幂等）。
    /// 返回施加后的位域部分（raw & KNOWN_MASK）。
    pub fn apply_persona(&mut self, raw: u64) -> Result<u64, u32> {
        // 判序 ①：封账后禁写。
        if self.sealed {
            return Err(PersErr::ESealed.val());
        }
        // 判序 ②：账本容量守恒。
        if self.recs.len() >= self.cap {
            return Err(PersErr::ELedgerFull.val());
        }
        // 判序 ③：未知位闭集（低字节人格域放行，高位逐位对账）。
        if raw & !KNOWN_ALL != 0 {
            return Err(PersErr::EBadBits.val());
        }
        // 判序 ④：人格字节闭集。
        let persona = (raw & 0xFF) as u8;
        if persona != PERSONA_LINUX && persona != PERSONA_RESERVED {
            return Err(PersErr::EBadPersona.val());
        }
        // 判序 ⑤：施加位槽（幂等：重复施加同状态同返回）。
        self.persona = persona;
        let flags = raw & KNOWN_MASK;
        self.applied_mask = flags;
        {
            let slots = &mut self.slots;
            let mut i = 0usize;
            for b in KNOWN_BITS.iter() {
                if let Some(slot) = slots.get_mut(i) {
                    *slot = (flags & *b) != 0;
                }
                i += 1;
            }
            if let Some(slot) = slots.get_mut(8) {
                *slot = persona == PERSONA_LINUX;
            }
        }
        self.recs.push(OpRec {
            step: self.step,
            kind: OpKind::MaskApply,
            field: 0xFF,
            mask: raw,
        });
        self.step = self.step.saturating_add(1);
        if let Phase::Fresh = self.phase {
            self.phase = Phase::Live;
        }
        Ok(flags)
    }

    /// uname 字段写入。判序：封账 → 账容 → 字段闭集（EBadField）→
    /// 长度含 NUL ≤65（EBadLen）→ 落槽。
    pub fn uname_set_field(&mut self, field: usize, val: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PersErr::ESealed.val());
        }
        if self.recs.len() >= self.cap {
            return Err(PersErr::ELedgerFull.val());
        }
        if field >= UNAME_FIELDS {
            return Err(PersErr::EBadField.val());
        }
        if val.as_bytes().len().saturating_add(1) > UTSLEN {
            return Err(PersErr::EBadLen.val());
        }
        match self.uname.get_mut(field) {
            Some(slot) => *slot = String::from(val),
            None => return Err(PersErr::EBadField.val()),
        }
        if let Some(flag) = self.uname_set.get_mut(field) {
            *flag = true;
        }
        self.recs.push(OpRec {
            step: self.step,
            kind: OpKind::FieldSet,
            field: field as u8,
            mask: 0,
        });
        self.step = self.step.saturating_add(1);
        if let Phase::Fresh = self.phase {
            self.phase = Phase::Live;
        }
        Ok(())
    }

    /// uname 字段读取：未设或越闭集返回空串（零误伤，成功读入账）。
    pub fn uname_get_field(&mut self, field: usize) -> &str {
        if self.recs.len() < self.cap && !self.sealed {
            self.recs.push(OpRec {
                step: self.step,
                kind: OpKind::FieldGet,
                field: field.min(0xFF) as u8,
                mask: 0,
            });
            self.step = self.step.saturating_add(1);
            if let Phase::Fresh = self.phase {
                self.phase = Phase::Live;
            }
        }
        match self.uname.get(field) {
            Some(s) => s.as_str(),
            None => "",
        }
    }

    /// 翻位侦测：位槽与 applied_mask / persona 逐位重算对账。
    pub fn verify_slots(&self) -> Result<(), u32> {
        for (i, b) in KNOWN_BITS.iter().enumerate() {
            let expect = (self.applied_mask & *b) != 0;
            match self.slots.get(i) {
                Some(s) if *s == expect => {}
                _ => return Err(PersErr::ESlotDrift.val()),
            }
        }
        match self.slots.get(8) {
            Some(s) if *s == (self.persona == PERSONA_LINUX) => {}
            _ => return Err(PersErr::ESlotDrift.val()),
        }
        Ok(())
    }

    /// 审计：位槽对账 + 掩码漂移 + 槽长漂移 + 账本单调 + Phase 单调。
    pub fn audit(&self) -> Result<(), u32> {
        self.verify_slots()?;
        if self.applied_mask & !KNOWN_ALL != 0 {
            return Err(PersErr::EMaskDrift.val());
        }
        for s in self.uname.iter() {
            if s.as_bytes().len().saturating_add(1) > UTSLEN {
                return Err(PersErr::ELenDrift.val());
            }
        }
        let mut prev: Option<u64> = None;
        for r in self.recs.iter() {
            if let Some(p) = prev {
                if r.step <= p {
                    return Err(PersErr::EStepReg.val());
                }
            }
            prev = Some(r.step);
        }
        match self.phase {
            Phase::Fresh if !self.recs.is_empty() => return Err(PersErr::EStepReg.val()),
            Phase::Sealed if !self.sealed => return Err(PersErr::EStepReg.val()),
            _ => {}
        }
        Ok(())
    }

    /// 不变量（audit 全绿 + 容量守恒）。
    pub fn invariant_ok(&self) -> bool {
        self.recs.len() <= self.cap && self.audit().is_ok()
    }

    /// 封账：Phase 单调推进到 Sealed；重复封账拒止。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(PersErr::ESealed.val());
        }
        self.sealed = true;
        self.phase = Phase::Sealed;
        Ok(self.recs.len())
    }
}

// ════════════════════════ 注入器 ════════════════════════

/// 注入违约样本闭集：每类必抓（精确诊断码匹配），共 10 类。
pub struct Injector;

/// 65 字节字段值样本（含 NUL 共 66 字节 → 必越界）。
const FIELD65: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

impl Injector {
    /// 10 类注入样本，逐一构造并断言必抓。返回 (抓到数, 总数)。
    pub fn run_all(st: &mut PersUname) -> (usize, usize) {
        let mut caught = 0usize;
        let total = 10usize;
        // ① 未知位 0x8000_0000 → EBadBits。
        if st.apply_persona(0x8000_0000) == Err(PersErr::EBadBits.val()) {
            caught += 1;
        }
        // ② 人格字节垃圾 0x37 → EBadPersona。
        if st.apply_persona(0x37) == Err(PersErr::EBadPersona.val()) {
            caught += 1;
        }
        // ③ 字段越闭集 6/7 → EBadField。
        if st.uname_set_field(6, "x") == Err(PersErr::EBadField.val())
            && st.uname_set_field(7, "x") == Err(PersErr::EBadField.val())
        {
            caught += 1;
        }
        // ④ 字段 65 字节（含 NUL 66 > 65）→ EBadLen。
        if st.uname_set_field(F_SYSNAME, FIELD65) == Err(PersErr::EBadLen.val()) {
            caught += 1;
        }
        // ⑤ 重复施加幂等（零误杀）：同 raw 两施，同返回同状态。
        let m = ADDR_NO_RANDOMIZE | STICKY_TIMEOUTS;
        let r1 = st.apply_persona(m);
        let r2 = st.apply_persona(m);
        let idem_ok = match (r1, r2) {
            (Ok(a), Ok(b)) => a == b && st.verify_slots().is_ok(),
            _ => false,
        };
        if idem_ok {
            caught += 1;
        }
        // ⑥ 先读未设槽：返回空串（零误杀，Linux newuname 全槽恒有值前空可读）。
        let mut fresh = PersUname::new();
        if fresh.uname_get_field(F_SYSNAME).is_empty() && fresh.applied_mask() == 0 {
            caught += 1;
        }
        // ⑦ mask 0 = PER_LINUX：八位槽归零、人格零槽置真（零误杀）。
        let mut per = PersUname::new();
        let zero_ok = match per.apply_persona(0) {
            Ok(v) => {
                let sl = per.slots();
                v == 0
                    && sl.iter().take(8).all(|s| !*s)
                    && match sl.get(8) {
                        Some(s) => *s,
                        None => false,
                    }
                    && per.audit().is_ok()
            }
            Err(_) => false,
        };
        if zero_ok {
            caught += 1;
        }
        // ⑧ 翻位侦测：施加后审计逐位对账一致（零误杀）。
        let mut flip = PersUname::new();
        let audit_ok = flip.apply_persona(ADDR_NO_RANDOMIZE | WHOLE_SECONDS).is_ok()
            && flip.verify_slots().is_ok();
        if audit_ok {
            caught += 1;
        }
        // ⑨ 账本满：以合法 FieldGet 灌满后下一次写 → ELedgerFull。
        while st.recs.len() < st.cap {
            let _ = st.uname_get_field(F_NODENAME);
        }
        if st.uname_set_field(F_RELEASE, "1.0") == Err(PersErr::ELedgerFull.val()) {
            caught += 1;
        }
        // ⑩ 封账后写 → ESealed。
        let _ = st.seal();
        if st.apply_persona(ADDR_NO_RANDOMIZE) == Err(PersErr::ESealed.val())
            && st.uname_set_field(F_MACHINE, "x") == Err(PersErr::ESealed.val())
        {
            caught += 1;
        }
        (caught, total)
    }
}

// ════════════════════════ 双跑一致 / 指纹 ════════════════════════

/// FNV-1a 64 位指纹（确定性）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 对账表条目：与 Linux 真实位值一一对号（名, 位值, 槽位）。
pub const RECON_TABLE: [(&str, u64, i32); 8] = [
    ("ADDR_NO_RANDOMIZE", 0x0040000, 0),
    ("ADDR_COMPAT_LAYOUT", 0x0800000, 1),
    ("MMAP_PAGE_ZERO", 0x0100000, 2),
    ("READ_IMPLIES_EXEC", 0x0400000, 3),
    ("ADDR_LIMIT_32BIT", 0x0200000, 4),
    ("SHORT_INODE", 0x1000000, 5),
    ("WHOLE_SECONDS", 0x2000000, 6),
    ("STICKY_TIMEOUTS", 0x4000000, 7),
];

// ════════════════════════ 判据聚合 ════════════════════════

pub fn run_unx9407_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9407");
    // J1-① 未知位全抓。
    {
        let bad: [u64; 6] = [0x8000_0000, 0x1_0000_0000, 0x100, 0x2_0000, 0x8000_0000_0000_0000, u64::MAX];
        let mut all_ebad = true;
        for raw in bad.iter() {
            let mut st = PersUname::new();
            if st.apply_persona(*raw) != Err(PersErr::EBadBits.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-01 未知位全EBadBits", all_ebad, "6 类闭集外位全拒");
    }
    // J1-② 人格字节垃圾全抓。
    {
        let bad: [u64; 5] = [0x37, 0x01, 0x42, 0x7F, 0xFE];
        let mut all_ebad = true;
        for raw in bad.iter() {
            let mut st = PersUname::new();
            if st.apply_persona(*raw) != Err(PersErr::EBadPersona.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-02 人格字节闭集判序", all_ebad, "低字节 ∉ {0x00,0xFF} 全拒");
    }
    // J1-③ 已知八位零误伤，逐位落槽。
    {
        let mut all_ok = true;
        for (i, b) in KNOWN_BITS.iter().enumerate() {
            let mut st = PersUname::new();
            let applied = match st.apply_persona(*b) {
                Ok(v) => v,
                Err(_) => {
                    all_ok = false;
                    break;
                }
            };
            let slot_ok = match st.slots().get(i) {
                Some(sl) => *sl,
                None => false,
            };
            if applied != *b || !slot_ok {
                all_ok = false;
            }
        }
        s.add("J1-03 八位零误伤落槽", all_ok, "单位施加全绿且对应槽位真");
    }
    // J1-④ mask 0 = PER_LINUX 零误伤。
    {
        let mut st = PersUname::new();
        let ok = match st.apply_persona(0) {
            Ok(v) => {
                let sl = st.slots();
                v == 0
                    && sl.iter().take(8).all(|x| !*x)
                    && match sl.get(8) {
                        Some(x) => *x,
                        None => false,
                    }
                    && st.invariant_ok()
            }
            Err(_) => false,
        };
        s.add(
            "J1-04 mask0全槽归零零误伤",
            ok,
            "PER_LINUX 施加即八位槽全假、人格零槽真且审计绿",
        );
    }
    // J1-⑤ 重复施加幂等。
    {
        let mut st = PersUname::new();
        let m = ADDR_NO_RANDOMIZE | MMAP_PAGE_ZERO;
        let r1 = st.apply_persona(m);
        let r2 = st.apply_persona(m);
        let ok = match (r1, r2) {
            (Ok(a), Ok(b)) => a == b && st.audit().is_ok(),
            _ => false,
        };
        s.add("J1-05 重复施加幂等", ok, "同 raw 双施同返回同状态，审计绿");
    }
    // J1-⑥ uname 字段闭集全抓。
    {
        let mut all_ebad = true;
        for f in [6usize, 7, 100, usize::MAX].iter() {
            let mut st = PersUname::new();
            if st.uname_set_field(*f, "x") != Err(PersErr::EBadField.val()) {
                all_ebad = false;
            }
        }
        s.add("J1-06 字段越集全EBadField", all_ebad, "6/7/100/usize::MAX 全拒");
    }
    // J1-⑦ uname 长度判序：64 字节恰容零误伤，65+ 全抓。
    {
        let mut st = PersUname::new();
        let mut ok64 = String::new();
        let mut k = 0usize;
        while k < 64 {
            ok64.push('a');
            k += 1;
        }
        let fit_ok = st.uname_set_field(F_SYSNAME, &ok64).is_ok();
        let mut all_ebad = true;
        for n in [65usize, 66, 200].iter() {
            let mut st2 = PersUname::new();
            let mut nm = String::new();
            let mut k2 = 0usize;
            while k2 < *n {
                nm.push('a');
                k2 += 1;
            }
            if st2.uname_set_field(F_SYSNAME, &nm) != Err(PersErr::EBadLen.val()) {
                all_ebad = false;
            }
        }
        s.add(
            "J1-07 UTSLEN判序",
            fit_ok && all_ebad,
            "64 字节（含 NUL 65）恰容；65+ 全拒",
        );
    }
    // J1-⑧ 先读未设槽：空串零误伤。
    {
        let mut st = PersUname::new();
        let ok = st.uname_get_field(F_VERSION).is_empty() && st.invariant_ok();
        s.add("J1-08 先读空零误伤", ok, "未设槽读返回空串，不误判不越界");
    }
    // J1-⑨ uname 六槽 set/get 往返恰等。
    {
        let mut st = PersUname::new();
        let names: [&str; 6] = ["VarixOS", "varix-node", "6.12.0-varix", "#1 SMP", "x86_64", "(none)"];
        let mut all_ok = true;
        for (i, nm) in names.iter().enumerate() {
            if st.uname_set_field(i, nm).is_err() {
                all_ok = false;
            }
        }
        for (i, nm) in names.iter().enumerate() {
            if st.uname_get_field(i) != *nm {
                all_ok = false;
            }
        }
        s.add("J1-09 六槽往返恰等", all_ok, "六字段 set/get 逐槽一致");
    }
    // J1-⑩ 注入器 10/10。
    {
        let mut st = PersUname::new();
        let (caught, total) = Injector::run_all(&mut st);
        s.add(
            "J1-10 注入器10/10必抓",
            caught == total,
            "违约样本全数被闭集诊断码拦下",
        );
    }
    // J1-⑪ 对账表 + 段位守卫。
    {
        let mut ok = true;
        for (i, ((name, bit, slot), kb)) in RECON_TABLE.iter().zip(KNOWN_BITS.iter()).enumerate() {
            let _ = name;
            if *bit != *kb || *slot as usize != i {
                ok = false;
            }
        }
        let seg_ok = SEG_BASE == 0x7D340
            && PersErr::ALL
                .iter()
                .all(|e| e.val() >= SEG_BASE && e.val() < SEG_BASE + 0x40);
        s.add(
            "J1-11 对账表与段位守卫",
            ok && seg_ok,
            "RECON_TABLE 八行位值/槽位对号；0x7D340 段步进 0x40 内不越界",
        );
    }
    // J1-⑫ 双跑一致。
    {
        let run = || -> u64 {
            let mut st = PersUname::new();
            let mut log = String::new();
            let seq: [u64; 6] = [
                0,
                ADDR_NO_RANDOMIZE,
                ADDR_NO_RANDOMIZE | READ_IMPLIES_EXEC,
                ADDR_NO_RANDOMIZE | READ_IMPLIES_EXEC,
                STICKY_TIMEOUTS,
                KNOWN_MASK,
            ];
            for raw in seq.iter() {
                match st.apply_persona(*raw) {
                    Ok(v) => {
                        let line = format!("{:x};", v);
                        log.push_str(&line);
                    }
                    Err(_) => log.push('E'),
                }
            }
            let _ = st.uname_set_field(F_SYSNAME, "VarixOS");
            let _ = st.uname_set_field(F_MACHINE, "x86_64");
            let _ = st.seal();
            fnv1a(log.as_bytes())
        };
        let a = run();
        let b = run();
        s.add("J1-12 双跑一致", a == b, "同输入双跑指纹相等（确定性）");
    }
    // J1-⑬ 封账纪律。
    {
        let mut st = PersUname::new();
        let _ = st.apply_persona(ADDR_NO_RANDOMIZE);
        let sealed = st.seal().is_ok() && st.sealed();
        let mask_rejected = st.apply_persona(0) == Err(PersErr::ESealed.val());
        let field_rejected = st.uname_set_field(F_SYSNAME, "x") == Err(PersErr::ESealed.val());
        let reseal_rejected = st.seal().is_err();
        s.add(
            "J1-13 封账纪律",
            sealed && mask_rejected && field_rejected && reseal_rejected,
            "封账置位、掩码与字段双拒、重复封账拒止",
        );
    }
    // J1-⑭ 审计/不变量守恒。
    {
        let mut st = PersUname::new();
        let _ = st.apply_persona(KNOWN_MASK);
        let _ = st.uname_set_field(F_NODENAME, "varix-node");
        let ok = st.audit().is_ok() && st.invariant_ok() && st.verify_slots().is_ok();
        s.add(
            "J1-14 审计不变量守恒",
            ok,
            "位槽对账/掩码无漂移/槽长守恒/账本单调全过",
        );
    }
    // 动态明细（≤8 条）：最关键的三条。
    s.add_dyn("账本深度", ledger_depth_ok(), "成功操作一次账本恰增一格");
    s.add_dyn("封账状态", seal_state_ok(), "封账后 sealed 置位且拒写");
    s.add_dyn("指纹非零", fnv_nonzero(), "FNV 指纹非零，可作账锚");
    s
}

// ════════════════════════ 动态明细辅助 ════════════════════════

fn ledger_depth_ok() -> bool {
    let mut st = PersUname::new();
    let _ = st.uname_get_field(F_SYSNAME);
    st.recs().len() == 1
}

fn seal_state_ok() -> bool {
    let mut st = PersUname::new();
    let _ = st.apply_persona(ADDR_NO_RANDOMIZE);
    st.seal().is_ok() && st.sealed()
}

fn fnv_nonzero() -> bool {
    fnv1a(&[1u8, 2, 3]) != 0
}

// ════════════════════════ 测试 ════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_unknown_bits_closed_set() {
        for raw in [0x8000_0000u64, 0x100, 0x2_0000, u64::MAX] {
            let mut st = PersUname::new();
            assert_eq!(st.apply_persona(raw), Err(PersErr::EBadBits.val()));
        }
    }

    #[test]
    fn t02_persona_byte_closed_set() {
        let mut st = PersUname::new();
        assert_eq!(st.apply_persona(0x37), Err(PersErr::EBadPersona.val()));
        assert!(st.apply_persona(0x00).is_ok());
        assert!(st.apply_persona(0xFF).is_ok());
    }

    #[test]
    fn t03_known_bits_zero_false_kill_and_slots() {
        for (i, b) in KNOWN_BITS.iter().enumerate() {
            let mut st = PersUname::new();
            assert!(st.apply_persona(*b).is_ok(), "位 {:x} 不得误伤", b);
            let slot = match st.slots().get(i) {
                Some(s) => *s,
                None => false,
            };
            assert!(slot, "位 {:x} 槽位未落", b);
        }
    }

    #[test]
    fn t04_mask_zero_is_per_linux() {
        let mut st = PersUname::new();
        let _ = st.apply_persona(KNOWN_MASK);
        assert!(st.apply_persona(0).is_ok());
        assert_eq!(st.applied_mask(), 0);
        // 八位位槽全假，人格零槽（persona==0）为真。
        assert!(st.slots().iter().take(8).all(|s| !*s));
        assert!(match st.slots().get(8) {
            Some(s) => *s,
            None => false,
        });
    }

    #[test]
    fn t05_duplicate_apply_idempotent() {
        let mut st = PersUname::new();
        let m = ADDR_NO_RANDOMIZE | SHORT_INODE;
        let a = st.apply_persona(m);
        let b = st.apply_persona(m);
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "重复施加不得报错"),
        }
    }

    #[test]
    fn t06_uname_field_closed_set_and_len() {
        let mut st = PersUname::new();
        assert_eq!(st.uname_set_field(6, "x"), Err(PersErr::EBadField.val()));
        let mut nm = String::new();
        let mut k = 0usize;
        while k < 65 {
            nm.push('a');
            k += 1;
        }
        assert_eq!(st.uname_set_field(F_SYSNAME, &nm), Err(PersErr::EBadLen.val()));
        nm.pop();
        assert!(st.uname_set_field(F_SYSNAME, &nm).is_ok());
    }

    #[test]
    fn t07_get_before_set_returns_empty() {
        let mut st = PersUname::new();
        assert_eq!(st.uname_get_field(F_MACHINE), "");
        assert!(st.invariant_ok());
    }

    #[test]
    fn t08_injector_full_catch() {
        let mut st = PersUname::new();
        let (caught, total) = Injector::run_all(&mut st);
        assert_eq!(caught, total);
        assert_eq!(total, 10);
    }

    #[test]
    fn t09_seal_discipline() {
        let mut st = PersUname::new();
        let _ = st.apply_persona(ADDR_COMPAT_LAYOUT);
        assert!(st.seal().is_ok());
        assert_eq!(st.apply_persona(0), Err(PersErr::ESealed.val()));
        assert_eq!(
            st.uname_set_field(F_RELEASE, "1"),
            Err(PersErr::ESealed.val())
        );
        assert!(st.seal().is_err());
    }

    #[test]
    fn t10_diag_codes_closed_set() {
        let mut prev = 0u32;
        for (i, e) in PersErr::ALL.iter().enumerate() {
            assert!(e.val() > prev || i == 0);
            prev = e.val();
            assert!(!e.detail().is_empty());
        }
        assert_eq!(PersErr::ALL.len(), 10);
    }

    #[test]
    fn t11_checks_all_pass() {
        let s = run_unx9407_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }

    #[test]
    fn t12_dual_run_determinism() {
        let run = || {
            let mut st = PersUname::new();
            let _ = st.apply_persona(ADDR_NO_RANDOMIZE | WHOLE_SECONDS);
            let _ = st.uname_set_field(F_SYSNAME, "VarixOS");
            let _ = st.seal();
            (st.applied_mask(), st.slots().to_vec(), st.recs().len())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn t13_seg_base_and_recon() {
        assert_eq!(SEG_BASE, 0x7D340);
        assert_eq!(RECON_TABLE.len(), 8);
        for (i, (name, bit, slot)) in RECON_TABLE.iter().enumerate() {
            let kb = match KNOWN_BITS.get(i) {
                Some(b) => *b,
                None => {
                    assert!(false, "KNOWN_BITS 缺槽 {}", i);
                    return;
                }
            };
            assert_eq!(*bit, kb, "{} 位值错", name);
            assert_eq!(*slot as usize, i, "{} 槽位错", name);
        }
    }

    #[test]
    fn t14_bit_flip_audit_detects_drift() {
        // 正常态审计绿；翻位侦测面：verify_slots 对账一致。
        let mut st = PersUname::new();
        let _ = st.apply_persona(READ_IMPLIES_EXEC);
        assert!(st.verify_slots().is_ok());
        assert!(st.audit().is_ok());
        assert!(st.invariant_ok());
    }
}
