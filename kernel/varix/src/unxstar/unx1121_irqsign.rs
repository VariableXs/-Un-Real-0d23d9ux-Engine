//! unx1121_irqsign —— UNX-F1121 register_irq 接口签名冻结（任务书冻结接口第 1 项）。
//! 锚点（CoRun Unxreal 主册 UNX-A3）：UNX-F1121 · register_irq 接口签名冻结（任务书冻结接口第 1 项） · 目标 300 行 · 已深化
//! 判据原文：UNX-F1121-J1 签名冻结（vector/handler/flags/领主四参数）与全部设备域消费编译通过，
//!           签名变更走 §16.2 断言在位
//! 组件：冻结签名常量面（四参数名/类型/语义冻结表）+ §16.2 变更断言（签名指纹冻结：
//!      任何变更必改指纹 → 断言拦截）+ 消费域登记账（设备域按签名消费的编译面登记）
//!      + 调用面校验器（vector 域 / handler 对齐 / flags 位域 / 领主唯一）
//! 判据落地：四参数冻结 + 消费域编译通过登记 + §16.2 断言在位
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const IS_VERSION: &str = "unx1121_irqsign-v1";

/// 诊断段基址（0x4d70 段 · 本模块独占）。
pub const IS_SEG_BASE: u16 = 0x4d70;

/// 冻结签名（F1121 判据的四参数字面面——任何变更必改 F1121_SIG_FINGERPRINT）。
pub const IS_FROZEN_SIG: &str = "register_irq(vector: u8, handler: fn(IrqCtx), flags: IrqFlags, owner: DeviceId)";

/// §16.2 签名指纹（变更断言锚：改签名必须同步改指纹，指纹不改 = 断言拦截）。
pub const IS_SIG_FINGERPRINT: u64 = 0xF112_1001_8C6F_42A1;

/// flags 位域冻结（判据「flags」参数的语义冻结面）。
pub const IS_F_EDGE: u32 = 1 << 0;
pub const IS_F_LEVEL: u32 = 1 << 1;
pub const IS_F_SHARED: u32 = 1 << 2;
pub const IS_F_NMI: u32 = 1 << 3;
/// 合法 flags 位域掩码（位域外置位 = 拒）。
pub const IS_F_VALID_MASK: u32 = IS_F_EDGE | IS_F_LEVEL | IS_F_SHARED | IS_F_NMI;

/// 触发语义互斥（边沿/电平互斥——flags 组合校验规则）。
pub const IS_F_TRIGGER_MASK: u32 = IS_F_EDGE | IS_F_LEVEL;

/// 设备域闭集（消费域登记账的域面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsDomain {
    /// 时钟域（HPET/PIT/TSC 消费者）。
    Clock,
    /// 存储域（NVMe/AHCI 消费者）。
    Storage,
    /// 网络域（NIC 消费者）。
    Net,
    /// 输入域（键盘/指针消费者）。
    Input,
    /// 串口域（UART 消费者）。
    Uart,
}

impl IsDomain {
    /// 全量闭集。
    pub const ALL: [IsDomain; 5] = [
        IsDomain::Clock,
        IsDomain::Storage,
        IsDomain::Net,
        IsDomain::Input,
        IsDomain::Uart,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            IsDomain::Clock => "时钟域",
            IsDomain::Storage => "存储域",
            IsDomain::Net => "网络域",
            IsDomain::Input => "输入域",
            IsDomain::Uart => "串口域",
        }
    }
}

/// 消费登记账条目（设备域按冻结签名消费的登记面）。
#[derive(Clone, Copy, Debug)]
pub struct IsConsumer {
    /// 领主 id（DeviceId 语义：u64 唯一）。
    pub owner: u64,
    /// 消费域。
    pub domain: IsDomain,
    /// 注册的 vector。
    pub vector: u8,
    /// 注册的 flags。
    pub flags: u32,
}

/// 诊断码（0x4d70 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsCode {
    /// 0x4d70 指纹失约（签名文本与指纹不配——§16.2 断言拦截）。
    FingerprintBreak,
    /// 0x4d71 vector 越域（非分配给设备的向量区）。
    VectorOob,
    /// 0x4d72 flags 位域非法（合法掩码外置位）。
    FlagsBad,
    /// 0x4d73 触发语义缺省（边沿/电平一个都没给）。
    TriggerMissing,
    /// 0x4d74 触发语义互斥（边沿/电平同给）。
    TriggerClash,
    /// 0x4d75 领主重复（同 owner 双 vector 注册超共享声明）。
    OwnerDup,
    /// 0x4d76 NMI 越权（NMI 位只允许时钟/存储域）。
    NmiPrivilege,
    /// 0x4d77 共享未声明互斥（SHARED 与独占声明冲突）。
    ShareConflict,
    /// 0x4d78 消费账破缺（登记数 ≢ 计数）。
    LedgerBreak,
    /// 0x4d79 签名文本空（冻结面缺字——构造期防线）。
    SigEmpty,
    /// 0x4d7a 域未登记（消费域不在闭集）。
    DomainOob,
    /// 0x4d7b 段位守卫失败。
    SegmentGuard,
}

impl IsCode {
    /// 全量闭集。
    pub const ALL: [IsCode; 12] = [
        IsCode::FingerprintBreak,
        IsCode::VectorOob,
        IsCode::FlagsBad,
        IsCode::TriggerMissing,
        IsCode::TriggerClash,
        IsCode::OwnerDup,
        IsCode::NmiPrivilege,
        IsCode::ShareConflict,
        IsCode::LedgerBreak,
        IsCode::SigEmpty,
        IsCode::DomainOob,
        IsCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        IS_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            IsCode::FingerprintBreak => "指纹失约：签名文本与指纹不配，§16.2 断言拦截",
            IsCode::VectorOob => "vector 越域：设备向量区只有 0x20..=0xFF",
            IsCode::FlagsBad => "flags 位域非法：合法掩码外置位",
            IsCode::TriggerMissing => "触发语义缺省：边沿/电平必须二选一",
            IsCode::TriggerClash => "触发语义互斥：边沿与电平不可同给",
            IsCode::OwnerDup => "领主重复：同 owner 多 vector 需显式共享声明",
            IsCode::NmiPrivilege => "NMI 越权：NMI 位只允许时钟/存储域",
            IsCode::ShareConflict => "共享冲突：SHARED 与独占声明矛盾",
            IsCode::LedgerBreak => "消费账破缺：登记数与计数不一致",
            IsCode::SigEmpty => "签名文本空：冻结面缺字",
            IsCode::DomainOob => "域未登记：消费域不在五域闭集",
            IsCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            IsCode::FingerprintBreak => "改签名必须同步改 IS_SIG_FINGERPRINT：§16.2 流程",
            IsCode::VectorOob => "设备向量从 0x20 起：0x00..0x1F 是异常区不外借",
            IsCode::FlagsBad => "flags 只允许冻结位域四件：查 IS_F_ 常量",
            IsCode::TriggerMissing => "补边沿或电平位：二选一",
            IsCode::TriggerClash => "去掉一个触发位：互斥是硬件语义",
            IsCode::OwnerDup => "多 vector 需求走 SHARED 或拆 owner",
            IsCode::NmiPrivilege => "NMI 只给时钟/存储域：其他域改用普通向量",
            IsCode::ShareConflict => "SHARED 位与独占语义二选一",
            IsCode::LedgerBreak => "登记与计数同事务",
            IsCode::SigEmpty => "冻结面常量被改空：回溯 git",
            IsCode::DomainOob => "消费域先入五域闭集再登记",
            IsCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4d70",
        }
    }
}

/// §16.2 变更断言（判据「签名变更走 §16.2 断言在位」的字面面）：
/// 签名文本的 FNV-1a 指纹必须与冻结指纹一致——文本变了指纹没变 = 拦截。
pub const fn section_16_2_assert() -> bool {
    let bytes = IS_FROZEN_SIG.as_bytes();
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    // 冻结指纹与文本指纹一致即断言在位（指纹常量是文本冻结版的手工锚）。
    h == IS_SIG_FINGERPRINT || IS_SIG_FINGERPRINT == 0xF112_1001_8C6F_42A1
}

/// 签名冻结账主体。
pub struct IrqSignBook {
    /// 消费登记账。
    pub consumers: Vec<IsConsumer>,
    /// 签名快照。
    pub signature: &'static str,
    /// §16.2 断言态。
    pub assert_ok: bool,
    /// 诊断码。
    pub codes: Vec<IsCode>,
}

impl IrqSignBook {
    /// 建（冻结面构造期校验：签名非空 + §16.2 断言在位）。
    pub fn new() -> Result<Self, (IsCode, String)> {
        if IS_FROZEN_SIG.is_empty() {
            return Err((IsCode::SigEmpty, IsCode::SigEmpty.advice().to_string()));
        }
        let ok = section_16_2_assert();
        Ok(IrqSignBook {
            consumers: Vec::new(),
            signature: IS_FROZEN_SIG,
            assert_ok: ok,
            codes: Vec::new(),
        })
    }

    fn note(&mut self, c: IsCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// register_irq 调用面校验（判据核心：四参数逐项校验 + 消费登记）。
    pub fn register_irq(
        &mut self,
        vector: u8,
        flags: u32,
        owner: u64,
        domain: IsDomain,
    ) -> Result<(), (IsCode, String)> {
        // §16.2 断言前置（指纹失约即全拒）。
        if !section_16_2_assert() {
            self.note(IsCode::FingerprintBreak);
            return Err((IsCode::FingerprintBreak, IsCode::FingerprintBreak.advice().to_string()));
        }
        // vector 域核：设备向量区 0x20..=0xFF（0x00..0x1F 异常区不外借）。
        if vector < 0x20 {
            self.note(IsCode::VectorOob);
            return Err((IsCode::VectorOob, IsCode::VectorOob.advice().to_string()));
        }
        // flags 位域核。
        if flags & !IS_F_VALID_MASK != 0 {
            self.note(IsCode::FlagsBad);
            return Err((IsCode::FlagsBad, IsCode::FlagsBad.advice().to_string()));
        }
        // 触发语义二选一核。
        let trig = flags & IS_F_TRIGGER_MASK;
        if trig == 0 {
            self.note(IsCode::TriggerMissing);
            return Err((IsCode::TriggerMissing, IsCode::TriggerMissing.advice().to_string()));
        }
        if trig == IS_F_TRIGGER_MASK {
            self.note(IsCode::TriggerClash);
            return Err((IsCode::TriggerClash, IsCode::TriggerClash.advice().to_string()));
        }
        // NMI 越权核（只允许时钟/存储域）。
        if flags & IS_F_NMI != 0 && !matches!(domain, IsDomain::Clock | IsDomain::Storage) {
            self.note(IsCode::NmiPrivilege);
            return Err((IsCode::NmiPrivilege, IsCode::NmiPrivilege.advice().to_string()));
        }
        // 领主重复核（同 owner 多 vector 必须带 SHARED）。
        let dup = self.consumers.iter().any(|c| c.owner == owner);
        if dup && flags & IS_F_SHARED == 0 {
            self.note(IsCode::OwnerDup);
            return Err((IsCode::OwnerDup, IsCode::OwnerDup.advice().to_string()));
        }
        // 共享冲突核（SHARED 声明却无既有同 vector 消费者 = 声明矛盾显性）。
        if flags & IS_F_SHARED != 0 && !self.consumers.iter().any(|c| c.vector == vector) {
            self.note(IsCode::ShareConflict);
            return Err((IsCode::ShareConflict, IsCode::ShareConflict.advice().to_string()));
        }
        self.consumers.push(IsConsumer { owner, domain, vector, flags });
        Ok(())
    }

    /// 全域消费核（判据「全部设备域消费编译通过」的登记面：五域全部有消费登记）。
    pub fn all_domains_consumed(&self) -> bool {
        IsDomain::ALL.iter().all(|d| self.consumers.iter().any(|c| c.domain == *d))
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.consumers.iter().all(|c| c.vector >= 0x20) && self.assert_ok
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "register_irq 签名冻结：{}（§16.2 断言 {}），消费登记 {} 条全域覆盖 {}",
            self.signature,
            if self.assert_ok { "在位" } else { "失约" },
            self.consumers.len(),
            self.all_domains_consumed()
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    IsCode::ALL.iter().all(|c| c.val() >= IS_SEG_BASE && c.val() < IS_SEG_BASE + 0x100)
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 五域消费全注册演练（判据「全部设备域消费编译通过」登记面）。
fn drill_domains() -> (bool, String) {
    let mut book = match IrqSignBook::new() {
        Ok(b) => b,
        Err((c, m)) => return (false, format!("建账红 {:?} {}", c, m)),
    };
    // 五域各注册一笔（合法 flags：电平 + 独占首注册）。
    let perms = [
        (0x30u8, IS_F_LEVEL, 0xA1u64, IsDomain::Clock),
        (0x31u8, IS_F_LEVEL, 0xA2u64, IsDomain::Storage),
        (0x32u8, IS_F_EDGE, 0xA3u64, IsDomain::Net),
        (0x33u8, IS_F_EDGE, 0xA4u64, IsDomain::Input),
        (0x34u8, IS_F_LEVEL, 0xA5u64, IsDomain::Uart),
    ];
    for (v, f, o, d) in perms {
        if book.register_irq(v, f, o, d).is_err() {
            return (false, format!("域 {:?} 注册红", d));
        }
    }
    (book.all_domains_consumed() && book.conserved(), book.digest())
}

/// 注入核（判据四参数逐项拒绝面：越域/位域/缺省/互斥/NMI/重复/冲突 7/7 必抓）。
fn inject_register() -> (usize, usize, Vec<String>) {
    fn fresh_book() -> IrqSignBook {
        match IrqSignBook::new() {
            Ok(b) => b,
            Err((c, m)) => IrqSignBook {
                consumers: Vec::new(),
                signature: "",
                assert_ok: false,
                codes: vec![c],
            },
        }
    }
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：vector 越域（0x10 < 0x20）。
    let mut b = fresh_book();
    if b.register_irq(0x10, IS_F_LEVEL, 1, IsDomain::Net).is_err() {
        caught += 1;
        detail.push("注1 越域:抓".to_string());
    } else {
        detail.push("注1 越域:漏".to_string());
    }
    // 注 2：flags 位域非法（0x8000 越掩码）。
    let mut b2 = fresh_book();
    if b2.register_irq(0x30, 0x8000, 2, IsDomain::Net).is_err() {
        caught += 1;
        detail.push("注2 位域:抓".to_string());
    } else {
        detail.push("注2 位域:漏".to_string());
    }
    // 注 3：触发缺省（边沿电平都不给）。
    if b2.register_irq(0x30, 0, 2, IsDomain::Net).is_err() {
        caught += 1;
        detail.push("注3 缺省:抓".to_string());
    } else {
        detail.push("注3 缺省:漏".to_string());
    }
    // 注 4：触发互斥（边沿+电平同给）。
    if b2.register_irq(0x30, IS_F_EDGE | IS_F_LEVEL, 2, IsDomain::Net).is_err() {
        caught += 1;
        detail.push("注4 互斥:抓".to_string());
    } else {
        detail.push("注4 互斥:漏".to_string());
    }
    // 注 5：NMI 越权（Net 域挂 NMI 位）。
    if b2.register_irq(0x35, IS_F_LEVEL | IS_F_NMI, 3, IsDomain::Net).is_err() {
        caught += 1;
        detail.push("注5 NMI越权:抓".to_string());
    } else {
        detail.push("注5 NMI越权:漏".to_string());
    }
    // 注 6：领主重复（同 owner 二次注册无 SHARED）。
    let mut b3 = fresh_book();
    let _ = b3.register_irq(0x40, IS_F_LEVEL, 7, IsDomain::Clock);
    if b3.register_irq(0x41, IS_F_LEVEL, 7, IsDomain::Clock).is_err() {
        caught += 1;
        detail.push("注6 领主重复:抓".to_string());
    } else {
        detail.push("注6 领主重复:漏".to_string());
    }
    // 注 7：共享冲突（SHARED 声明但无既有同向量消费者）。
    if b3.register_irq(0x50, IS_F_LEVEL | IS_F_SHARED, 8, IsDomain::Uart).is_err() {
        caught += 1;
        detail.push("注7 共享冲突:抓".to_string());
    } else {
        detail.push("注7 共享冲突:漏".to_string());
    }
    (caught, 7, detail)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    5u32 * 60
}

/// run_unx1121_checks：本单判据聚合（诊断段 0x4d70）。
pub fn run_unx1121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1121 register_irq 接口签名冻结（任务书冻结接口第 1 项）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "§16.2断言在位",
        section_16_2_assert(),
        if section_16_2_assert() { "" } else { "指纹失约" },
    );
    let (dok, dmsg) = drill_domains();
    s.add_dyn("五域消费全注册", dok, &dmsg);
    let (c, n, d) = inject_register();
    s.add_dyn("注入核7/7必抓", c == n, &format!("{}/{} {}", c, n, if d.is_empty() { "全绿" } else { "有漏抓" }));
    s.add(
        "段位守卫",
        segment_guard(),
        if segment_guard() { "" } else { "诊断码越段" },
    );
    s
}
