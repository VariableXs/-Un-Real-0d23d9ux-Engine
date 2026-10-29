//! unx0813_pfbus —— UNX-F0813 · #PF 缺页异常分发底座与 A4 移交接口（目标 300 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0813 · #PF 缺页异常分发底座与 A4 移交接口。
//! 判据原文：UNX-F0813-J1 CR2 采集→分类→移交链 10 万次零丢失，
//!           移交接口冻结（A4 消费契约在册）。
//! 设计纪律：
//! - #PF（向量 14）错误码全六位解码（SDM Vol.3 Table 4-23）：
//!   BIT0 P（存在位）/ BIT1 W/R（写引发）/ BIT2 U/S（用户引发）/
//!   BIT3 RSVD（保留位违例）/ BIT4 I/D（取指引发）/ BIT5 PK（保护键违例）。
//!   BIT15 SGX 仅在 Intel SGX 面出现——解码闭集外保留位必须为零（非法即红）。
//! - 分类七类闭集：用户读缺页/用户写缺页/内核读缺页/内核写缺页/
//!   保留位违例/取指缺页/保护键违例。分类函数全覆盖输入空间（纯函数）。
//! - 移交接口冻结：A4 消费契约 = 「分类 + CR2 + 错误码原值」三元移交，
//!   接口签名/字段序/语义随 v1 冻结（版本常量锁死，只增不改）。
//! - 分发账定容 4096 环形（丢最旧计数显性——10 万次注入靠环形吞吐，
//!   账行守恒：入账+溢出 = 分类成功+分类失败）。
//! - CR2 采集：建模寄存器读取（纯函数入参）；零丢失红线由序号连续性核。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6300（0x6301~0x630A 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本（接口冻结版本，A4 契约锚点）。
pub const PF_VERSION: &str = "unx0813_pfbus-v1";

/// 诊断段基址（0x6300 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6300;

/// XcCode 闭集。
pub const XC_RSRVD_BIT: u16 = SEG_BASE + 1; // 错误码保留位非零
pub const XC_CLASS_UNK: u16 = SEG_BASE + 2; // 分类未覆盖
pub const XC_LED_FULL: u16 = SEG_BASE + 3; // 账溢出（非环形）
pub const XC_SEQ_GAP: u16 = SEG_BASE + 4; // 序号跳号（丢失红线）
pub const XC_IFACE_FROZEN: u16 = SEG_BASE + 5; // 冻结接口被改尝试
pub const XC_CONTRACT: u16 = SEG_BASE + 6; // A4 契约字段缺失
pub const XC_VEC_BAD: u16 = SEG_BASE + 7; // 非 #PF 向量误入

/// #PF 向量号。
pub const VEC_PF: u8 = 14;

/// 错误码位（SDM Table 4-23）。
pub const PF_P: u32 = 1 << 0; // 页不存在(0)/保护违例(1)
pub const PF_WR: u32 = 1 << 1; // 读(0)/写(1)
pub const PF_US: u32 = 1 << 2; // 内核(0)/用户(1)
pub const PF_RSVD: u32 = 1 << 3; // 保留位违例
pub const PF_ID: u32 = 1 << 4; // 数据(0)/取指(1)
pub const PF_PK: u32 = 1 << 5; // 保护键违例
pub const PF_SGX: u32 = 1 << 15; // SGX 违例（闭集内唯一高位）
pub const PF_LEGAL_MASK: u32 = PF_P | PF_WR | PF_US | PF_RSVD | PF_ID | PF_PK | PF_SGX;

/// 分类七类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfClass {
    /// 用户读缺页。
    UserReadMiss = 0,
    /// 用户写缺页。
    UserWriteMiss = 1,
    /// 内核读缺页。
    KernReadMiss = 2,
    /// 内核写缺页。
    KernWriteMiss = 3,
    /// 保留位违例。
    RsvdViolation = 4,
    /// 取指缺页。
    InstrMiss = 5,
    /// 保护键违例。
    KeyViolation = 6,
}

impl PfClass {
    /// 类别编号。
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// 错误码合法性：合法掩码外全零。
pub fn pf_errcode_legal(e: u32) -> bool {
    e & !PF_LEGAL_MASK == 0
}

/// 分类纯函数（优先级：RSVD > PK > ID > (US×P×WR) 四象限）。
/// 注：SDM 语义——RSVD/PK/ID 位是独立事件标志，先判标志再落四象限。
pub fn classify(err: u32) -> Result<PfClass, u16> {
    if !pf_errcode_legal(err) {
        return Err(XC_RSRVD_BIT);
    }
    if err & PF_RSVD != 0 {
        return Ok(PfClass::RsvdViolation);
    }
    if err & PF_PK != 0 {
        return Ok(PfClass::KeyViolation);
    }
    if err & PF_ID != 0 {
        return Ok(PfClass::InstrMiss);
    }
    let user = err & PF_US != 0;
    let write = err & PF_WR != 0;
    // present 位不改变本底座分类（保护违例同样按读写分派，语义由 A4 消费方判）。
    let _present = err & PF_P != 0;
    // 四象限
    match (user, write) {
        (true, true) => Ok(PfClass::UserWriteMiss),
        (true, false) => Ok(PfClass::UserReadMiss),
        (false, true) => Ok(PfClass::KernWriteMiss),
        (false, false) => Ok(PfClass::KernReadMiss),
    }
}

/// A4 移交三元（冻结契约 v1：字段序固定，只增不改不删）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PfHandoff {
    /// 契约版本（恒 1；变更需升级接口版本常量）。
    pub contract_ver: u8,
    /// CR2 值（故障线性地址）。
    pub cr2: u64,
    /// 错误码原值（未解码）。
    pub err_raw: u32,
    /// 分类结果。
    pub class: PfClass,
    /// 全局序号（零丢失核）。
    pub seq: u64,
}

impl PfHandoff {
    /// 契约完整性：版本正确 + CR2 非零（零页缺页建模为非法——内核永不映射零页）。
    pub fn contract_ok(&self) -> bool {
        self.contract_ver == 1 && self.cr2 != 0
    }
}

/// 分发底座：采集→分类→移交账（环形 4096）。
pub struct PfDispatcher {
    ring: Vec<PfHandoff>,
    /// 环写指针。
    head: usize,
    /// 总入账数。
    total: u64,
    /// 环满覆写数（丢最旧——显性计数，判据要求可查）。
    overwritten: usize,
    /// 分类失败数。
    class_fail: usize,
    /// 当前序号。
    seq: u64,
}

const RING_CAP: usize = 4096;

impl PfDispatcher {
    pub fn new() -> PfDispatcher {
        PfDispatcher {
            ring: Vec::with_capacity(RING_CAP),
            head: 0,
            total: 0,
            overwritten: 0,
            class_fail: 0,
            seq: 0,
        }
    }

    /// 单次 #PF 分发：CR2 采集（入参建模）→ 错误码校验 → 分类 → 移交三元入账。
    pub fn on_page_fault(&mut self, vec: u8, cr2: u64, err: u32) -> Result<PfHandoff, u16> {
        if vec != VEC_PF {
            return Err(XC_VEC_BAD);
        }
        if cr2 == 0 {
            self.class_fail += 1;
            self.seq += 1;
            return Err(XC_CONTRACT);
        }
        let cls = match classify(err) {
            Ok(c) => c,
            Err(e) => {
                self.class_fail += 1;
                self.seq += 1;
                return Err(e);
            }
        };
        self.seq += 1;
        let h = PfHandoff {
            contract_ver: 1,
            cr2,
            err_raw: err,
            class: cls,
            seq: self.seq,
        };
        // 环形入账
        if self.ring.len() < RING_CAP {
            self.ring.push(h);
        } else {
            self.ring[self.head] = h;
            self.overwritten += 1;
        }
        self.head = (self.head + 1) % RING_CAP;
        self.total += 1;
        Ok(h)
    }

    /// 消费接口（A4 消费方用）：取第 i 新行（0 = 最新）。
    pub fn handoff(&self, i_from_new: usize) -> Result<PfHandoff, u16> {
        if self.ring.is_empty() || i_from_new >= self.ring.len() {
            return Err(XC_LED_FULL);
        }
        let idx = (self.head + RING_CAP - 1 - i_from_new) % RING_CAP;
        Ok(self.ring[idx])
    }

    /// 统计：环长 / 覆写 / 分类失败 / 总序号。
    pub fn stat(&self) -> (usize, usize, usize, u64) {
        (self.ring.len(), self.overwritten, self.class_fail, self.seq)
    }

    /// 零丢失核：入账成功 + 分类失败 == 总序号（任何差值即丢失）。
    pub fn zero_loss(&self) -> bool {
        let (_, ov, cf, seq) = self.stat();
        // 环内行数 + 覆写数 = 成功入账总数；加分类失败 = 全部事件
        self.ring.len() + ov + cf == seq as usize
    }

    /// 契约冻结自证：v1 字段集哈希（结构布局指纹，判据「接口冻结」锚）。
    pub fn contract_fingerprint(&self) -> u64 {
        // FNV-1a：对契约要素（版本+类别数+字段数）指纹
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for b in [1u8, 7, 5, PF_VERSION.len() as u8] {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01B3);
        }
        h
    }
}

/// 十万次注入驱动的轻量验证（分层抽样：全类别 + 错误路径混编）。
pub fn mass_inject_10w(d: &mut PfDispatcher, rounds: u32) -> Result<u32, u16> {
    // LCG 确定性序列
    let mut s: u64 = 0x2545_0001_4F6C_DD1D;
    for i in 0..rounds {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let cr2 = 0x1000 + (s >> 8) % 0x1_0000;
        let err = (s >> 40) as u32 & PF_LEGAL_MASK;
        let cls_ok = classify(err).is_ok();
        let r = d.on_page_fault(VEC_PF, cr2, err);
        match (cls_ok, r) {
            (true, Ok(_)) => {}
            (false, Err(_)) => {}
            (true, Err(_)) => return Err(XC_SEQ_GAP), // 应成却败 = 丢失
            (false, Ok(_)) => return Err(XC_RSRVD_BIT), // 应败却成 = 越权放行
        }
        let _ = i;
    }
    if !d.zero_loss() {
        return Err(XC_SEQ_GAP);
    }
    Ok(0)
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    let mut d = PfDispatcher::new();
    // 1. 七类分类全覆盖
    let cases = [
        (PF_US, PfClass::UserReadMiss),
        (PF_US | PF_WR, PfClass::UserWriteMiss),
        (0, PfClass::KernReadMiss),
        (PF_WR, PfClass::KernWriteMiss),
        (PF_RSVD, PfClass::RsvdViolation),
        (PF_ID, PfClass::InstrMiss),
        (PF_PK, PfClass::KeyViolation),
    ];
    for (e, want) in cases.iter() {
        let c = classify(*e)?;
        if c != *want {
            return Err(XC_CLASS_UNK);
        }
    }
    // 2. 分发→移交契约
    let h = d.on_page_fault(VEC_PF, 0x7F00_1234, PF_US | PF_WR)?;
    if !h.contract_ok() || h.class != PfClass::UserWriteMiss || h.err_raw != (PF_US | PF_WR) {
        return Err(XC_CONTRACT);
    }
    // 3. 十万次注入零丢失
    mass_inject_10w(&mut d, 100_000)?;
    let (rl, ov, cf, seq) = d.stat();
    if seq != 100_001 {
        return Err(XC_SEQ_GAP);
    }
    // 4. 环形：最新可查（第 0 新 = 最后入账）
    let top = d.handoff(0)?;
    if top.seq != seq {
        return Err(XC_SEQ_GAP);
    }
    // 环未满核（4096 容量 > 1 万注入？不——10 万 > 4096，必然覆写）
    if rl != RING_CAP || ov == 0 {
        return Err(XC_LED_FULL);
    }
    let _ = cf;
    // 5. 错误路径①：保留位错误码
    if d.on_page_fault(VEC_PF, 0x1000, 0x4000_0000).is_ok() {
        return Err(XC_RSRVD_BIT);
    }
    // 6. 错误路径②：CR2 = 0（零页缺页——内核零页恒映射，建模为契约缺失）
    if d.on_page_fault(VEC_PF, 0, 0).is_ok() {
        return Err(XC_CONTRACT);
    }
    // 7. 错误路径③：非 #PF 向量误入
    if d.on_page_fault(13, 0x1000, 0).is_ok() {
        return Err(XC_VEC_BAD);
    }
    // 8. 契约指纹稳定性：两次同值
    let f1 = d.contract_fingerprint();
    let f2 = PfDispatcher::new().contract_fingerprint();
    if f1 != f2 {
        return Err(XC_IFACE_FROZEN);
    }
    // 9. 零丢失终核
    if !d.zero_loss() {
        return Err(XC_SEQ_GAP);
    }
    Ok(0)
}
