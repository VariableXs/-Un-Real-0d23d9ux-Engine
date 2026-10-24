//! unx187_ucodero — UNX-F0187 微码加载状态检测（只读）（0x6C10 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0187）：微码加载状态检测（只读）。
//! 判据原文（UNX-F0187-J1）：微码修订号读取与 OS 侧（Windows
//! registry/cpuinfo）对照一致；加载动作不做（归固件，防重复）。
//!
//! 设计三线：①只读探测模型（IA32_UCODE_REV/IA32_UCODE_IDX 语义建模：
//! 写索引 MSR→读修订 MSR 的两步探测序，修订号/补丁签名/处理器签名
//! 三要素入账）；②OS 侧对照账（cpuinfo/registry 参照值与只读探测值
//! 逐项对拍，不一致即红）；③加载禁令闸（本域只读红线：任何加载请求
//! 一律拒绝并落账——加载归固件，重复加载会造成修订号回退风险）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const UCODE_VERSION: &str = "187-ucodero-v1";

/// 段基（0x6C10）。
pub const SEG_BASE: u16 = 0x6C10;

/// IA32_UCODE_REV（修订号 MSR 地址）。
pub const MSR_UCODE_REV: u32 = 0x8B;

/// IA32_UCODE_IDX（索引 MSR 地址）。
pub const MSR_UCODE_IDX: u32 = 0x79;

// ---------------------------------------------------------------------------
// 只读探测模型
// ---------------------------------------------------------------------------

/// 微码修订探测账（三要素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UcodeProbe {
    /// 当前修订号（WRMSR 索引后 RDMSR 0x8B 所得）。
    pub revision: u32,
    /// 补丁签名（补丁头 12~16 字节的 signature 字段，0 表示未装载）。
    pub patch_signature: u32,
    /// 处理器签名（CPUID(1).EAX）。
    pub cpu_signature: u32,
    /// 探测是否按两步序执行（先写索引后读修订）。
    pub probed_in_order: bool,
}

/// 探测器：建模两步只读探测序。
#[derive(Debug, Default)]
pub struct UcodeReader {
    /// 索引 MSR 当前值（写 0 触发修订号装载语义）。
    idx_val: u32,
    /// 修订 MSR 当前值（索引写入后生效）。
    rev_val: u32,
    /// 步序账：0=未动 1=已写索引 2=已读修订。
    step: u8,
}

impl UcodeReader {
    /// 建模初始态：修订号由固件装载决定（只读域不许写，只许建模注入）。
    pub fn with_revision(rev: u32) -> Self {
        UcodeReader {
            idx_val: 0,
            rev_val: rev,
            step: 0,
        }
    }
    /// 步一：写索引 MSR（0 表示重新装载当前微码修订）。
    pub fn write_index(&mut self, v: u32) {
        self.idx_val = v;
        self.step = 1;
    }
    /// 步二：读修订 MSR。未先写索引即读 → 返回 None（步序红线）。
    pub fn read_revision(&mut self) -> Option<u32> {
        if self.step != 1 {
            return None;
        }
        self.step = 2;
        Some(self.rev_val)
    }
    /// 收账：三要素齐才许出探测账。
    pub fn probe(&mut self, patch_signature: u32, cpu_signature: u32) -> Result<UcodeProbe, u16> {
        let rev = match self.read_revision() {
            Some(r) => r,
            None => return Err(E_ORDER),
        };
        Ok(UcodeProbe {
            revision: rev,
            patch_signature,
            cpu_signature,
            probed_in_order: true,
        })
    }
    /// 步序账平：探测账必须两步走完。
    pub fn order_ok(&self) -> bool {
        self.step == 2
    }
}

// ---------------------------------------------------------------------------
// OS 侧对照
// ---------------------------------------------------------------------------

/// OS 侧参照值（cpuinfo/registry 双源，只读对照用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsSideRef {
    /// cpuinfo 里报的 microcode 修订（十六进制）。
    pub cpuinfo_rev: u32,
    /// registry 硬件摘要里的 Update Revision 低 32 位。
    pub registry_rev: u32,
}

impl OsSideRef {
    /// 参照自一致：cpuinfo 与 registry 先得互一致（否则对照无意义）。
    pub fn self_consistent(&self) -> bool {
        self.cpuinfo_rev == self.registry_rev
    }
}

/// 对照结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Xcheck {
    /// 一致（判据绿）。
    Agree,
    /// 探测与参照不一致。
    Drift,
    /// 参照自身不一致。
    RefBroken,
}

/// 三方对照：探测账 vs cpuinfo vs registry。
pub fn crosscheck(probe: &UcodeProbe, os: &OsSideRef) -> Xcheck {
    if !os.self_consistent() {
        return Xcheck::RefBroken;
    }
    if probe.revision == os.cpuinfo_rev {
        Xcheck::Agree
    } else {
        Xcheck::Drift
    }
}

// ---------------------------------------------------------------------------
// 加载禁令闸（只读域红线）
// ---------------------------------------------------------------------------

/// 加载请求处置账。
#[derive(Debug, Default)]
pub struct LoadGate {
    /// 拒绝账：(请求方, 原因码)。
    pub rejections: Vec<(String, u16)>,
}

impl LoadGate {
    /// 加载请求：一律拒绝（本域只读——加载归固件，防修订回退）。
    pub fn request_load(&mut self, who: &str) -> Result<(), u16> {
        self.rejections.push((String::from(who), E_LOAD_FORBIDDEN));
        Err(E_LOAD_FORBIDDEN)
    }
    /// 账平：每笔拒绝都带禁令码、请求方键非空。
    pub fn ledger_clean(&self) -> bool {
        self.rejections.iter().all(|r| r.1 == E_LOAD_FORBIDDEN && !r.0.is_empty())
    }
    /// 拒绝笔数。
    pub fn rejections(&self) -> usize {
        self.rejections.len()
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x6C10 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum UcodeErr {
    /// 探测步序违例（未写索引先读修订）。
    Order = SEG_BASE + 1,
    /// 对照漂移。
    Drift = SEG_BASE + 2,
    /// 参照自身不一致。
    RefBroken = SEG_BASE + 3,
    /// 加载被禁（只读域红线）。
    LoadForbidden = SEG_BASE + 4,
    /// 探测账缺要素。
    ProbeShort = SEG_BASE + 5,
}

impl UcodeErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            UcodeErr::Order => "两步探测序违例",
            UcodeErr::Drift => "探测修订与 OS 侧参照漂移",
            UcodeErr::RefBroken => "cpuinfo 与 registry 参照不一致",
            UcodeErr::LoadForbidden => "加载动作被禁（归固件）",
            UcodeErr::ProbeShort => "探测账三要素不齐",
        }
    }
}

const E_ORDER: u16 = UcodeErr::Order.code();
const E_DRIFT: u16 = UcodeErr::Drift.code();
const E_REF_BROKEN: u16 = UcodeErr::RefBroken.code();
const E_LOAD_FORBIDDEN: u16 = UcodeErr::LoadForbidden.code();
const E_PROBE_SHORT: u16 = UcodeErr::ProbeShort.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据集（UNX-F0187-J1 + 结构红线）。自证五段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：两步只读探测 → 三方对照一致。
    let mut rd = UcodeReader::with_revision(0x0000_00A5);
    rd.write_index(0);
    let probe = rd.probe(0x0000_00A5, 0x0009_06EA)?;
    if !rd.order_ok() {
        return Err(E_ORDER);
    }
    let os = OsSideRef {
        cpuinfo_rev: 0x0000_00A5,
        registry_rev: 0x0000_00A5,
    };
    if crosscheck(&probe, &os) != Xcheck::Agree {
        return Err(E_DRIFT);
    }
    // 二、漂移检出：探测 0xA6 vs 参照 0xA5 → Drift（检出即红线兑现）。
    let mut rd2 = UcodeReader::with_revision(0xA6); // 建模修订已变。
    rd2.write_index(0);
    let probe2 = rd2.probe(0xA5, 0x906EA)?;
    if crosscheck(&probe2, &os) != Xcheck::Drift {
        return Err(E_DRIFT);
    }
    // 三、参照自破：cpuinfo 与 registry 互斥 → RefBroken（不误报漂移）。
    let bad = OsSideRef {
        cpuinfo_rev: 0xA5,
        registry_rev: 0xA4,
    };
    if crosscheck(&probe, &bad) != Xcheck::RefBroken {
        return Err(E_REF_BROKEN);
    }
    // 四、步序红线：未写索引先读修订必须失败（只读序不可绕）。
    let mut rd3 = UcodeReader::with_revision(0xA5);
    if rd3.read_revision().is_some() || rd3.probe(1, 1).is_ok() {
        return Err(E_ORDER);
    }
    // 五、加载禁令：请求必拒、拒账留痕、账笔与请求一致。
    let mut gate = LoadGate::default();
    if gate.request_load("vea-driver").is_ok() {
        return Err(E_LOAD_FORBIDDEN);
    }
    if gate.request_load("boot-stage2").is_ok() {
        return Err(E_LOAD_FORBIDDEN);
    }
    if gate.rejections() != 2 {
        return Err(E_PROBE_SHORT);
    }
    // 六、判别式段位对拍。
    if E_LOAD_FORBIDDEN != SEG_BASE + 4 || E_PROBE_SHORT != SEG_BASE + 5 {
        return Err(E_LOAD_FORBIDDEN);
    }
    Ok(())
}
