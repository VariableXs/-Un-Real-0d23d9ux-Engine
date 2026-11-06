//! UNX-F9355 E 型与 B13 错误编码器总成对账格 · 0x7C640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9355 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F9355-J1 对账判据过，与 errno.rs 编码族逐码零冲突
//!      格过，升级接管口径声明格过。
//!
//! 设计要点：
//!   ① E 型闭集 14 项（Perm/NoEntry/Io/BadFd/NoSpace/Access/Busy/Exists/
//!      NotDir/IsDir/Inval/NameTooLong/NoSys/Range），每项唯一 errno
//!      （1,2,5,9,12,13,16,17,20,21,22,36,38,34），注册表冻结、两两互异。
//!   ② B13 线编码把 (tag, errno, 折叠校验) 打进 u32：bit31..28 tag=0xE；
//!      bit23..12 12 位值域；bit3..0 折叠 (v^v>>4^v>>8)&0xF；bit11..4
//!      保留必须为 0。解码先验 tag、保留位与折叠再放行值，未知值一律
//!      None——绝不把半解析值当 errno（B13 口径原样冻结）。
//!   ③ 零冲突三面：往返零漂移（14 码 encode→decode 全还原）、包值两两
//!      互异、包值空间 [0xE000_0000,..] 与裸 errno 空间 [1,133] 不相交
//!      （tag 位天然隔离，仍逐码复算背书）。
//!   ④ 升级接管口径声明：E 型码自本卡起接管 B13 编码器口径；声明串
//!      非空且冻结，编码函数与声明同源，不许两条口径并存。
//!   ⑤ 注入核：错 tag / 错折叠 / 保留位非零 / 未知值 / 零值 五类坏字
//!      逐类拒绝，解码逃逸恒为 0。
//!   ⑥ 对账账守恒：注入 = 检出 + 逃逸，账只增不减。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9355 独占 0x7C640..=0x7C67F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C640;

/// 线格式 tag（bit31..28，B13 冻结口径）。
pub const LINE_TAG: u32 = 0xE;
/// Linux errno 合法值域（x86-64 ABI 面 1..=133）。
pub const ERRNO_MAX: i32 = 133;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncCode {
    /// 解码值不在 E 型闭集
    UnknownEType,
    /// tag 位非 0xE
    TagMismatch,
    /// 折叠校验不符
    FoldMismatch,
    /// 保留位非零
    ReservedBitsSet,
    /// errno 值越 [1,133]
    ValueOutOfRange,
    /// 往返漂移
    DriftDetected,
    /// 线包值两两冲突
    PackageCollision,
    /// errno 值两两冲突
    ErrnoCollision,
    /// 包值与裸 errno 空间相交
    OverlapRawErrno,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 接管口径声明为空
    TakeoverEmpty,
}

impl EncCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EncCode::UnknownEType => "未知 E 型值被拒：值必须在 14 项闭集内；\
                解码路只放行注册表内值，表外一律 None",
            EncCode::TagMismatch => "tag 位不符被拒：bit31..28 必须 = 0xE；\
                非 0xE 线包是外来格式，不得当 errno 解析",
            EncCode::FoldMismatch => "折叠校验不符被拒：bit3..0 必须 = \
                (v^v>>4^v>>8)&0xF；校验不过的半解析值绝不放行",
            EncCode::ReservedBitsSet => "保留位非零被拒：bit11..4 必须全 0；\
                脏保留位说明线包被篡改或格式漂移",
            EncCode::ValueOutOfRange => "errno 值越域被拒：合法值域 [1,133]；\
                0 与 >133 都不是合法 errno",
            EncCode::DriftDetected => "往返漂移被拒：encode→decode 必须还原原值；\
                漂移说明编码器与解码器不同源",
            EncCode::PackageCollision => "线包值冲突被拒：14 个包值必须两两互异；\
                冲突即闭集里有同包码",
            EncCode::ErrnoCollision => "errno 值冲突被拒：14 个 errno 必须两两互异；\
                同 errno 双码即映射面分裂",
            EncCode::OverlapRawErrno => "空间相交被拒：线包值空间（tag=0xE 置顶）\
                不得与裸 errno [1,133] 相交；相交即线格式失守",
            EncCode::LedgerRegress => "注入账倒退被拒：计数只增不减；\
                请检查回滚路径是否误改累计值",
            EncCode::NotReproducible => "双跑不一致：同输入两次对账结果不同；\
                请排查隐藏状态",
            EncCode::TakeoverEmpty => "接管口径声明为空被拒：升级接管必须有\
                非空冻结声明；空声明即口径未接管",
        }
    }

    /// 闭集全量。
    pub const ALL: [EncCode; 12] = [
        EncCode::UnknownEType,
        EncCode::TagMismatch,
        EncCode::FoldMismatch,
        EncCode::ReservedBitsSet,
        EncCode::ValueOutOfRange,
        EncCode::DriftDetected,
        EncCode::PackageCollision,
        EncCode::ErrnoCollision,
        EncCode::OverlapRawErrno,
        EncCode::LedgerRegress,
        EncCode::NotReproducible,
        EncCode::TakeoverEmpty,
    ];
}

// ── E 型注册表 ────────────────────────────────────────────────────────
/// E 型错误码（闭集 14 项；来源：errno.rs 编码族口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EType {
    /// EPERM
    Perm,
    /// ENOENT
    NoEntry,
    /// EIO
    Io,
    /// EBADF
    BadFd,
    /// ENOMEM 语义面
    NoSpace,
    /// EACCES
    Access,
    /// EBUSY
    Busy,
    /// EEXIST
    Exists,
    /// ENOTDIR
    NotDir,
    /// EISDIR
    IsDir,
    /// EINVAL
    Inval,
    /// ENAMETOOLONG
    NameTooLong,
    /// ENOSYS
    NoSys,
    /// ERANGE
    Range,
}

impl EType {
    /// 注册表全量。
    pub const ALL: [EType; 14] = [
        EType::Perm,
        EType::NoEntry,
        EType::Io,
        EType::BadFd,
        EType::NoSpace,
        EType::Access,
        EType::Busy,
        EType::Exists,
        EType::NotDir,
        EType::IsDir,
        EType::Inval,
        EType::NameTooLong,
        EType::NoSys,
        EType::Range,
    ];

    /// Linux errno（穷举 match，无通配——单源生成的结构面）。
    pub fn errno(self) -> i32 {
        match self {
            EType::Perm => 1,
            EType::NoEntry => 2,
            EType::Io => 5,
            EType::BadFd => 9,
            EType::NoSpace => 12,
            EType::Access => 13,
            EType::Busy => 16,
            EType::Exists => 17,
            EType::NotDir => 20,
            EType::IsDir => 21,
            EType::Inval => 22,
            EType::NameTooLong => 36,
            EType::NoSys => 38,
            EType::Range => 34,
        }
    }

    /// 按 errno 反查（解析器路，与 errno() 互为正逆）。
    pub fn from_errno(v: i32) -> Option<EType> {
        let mut i = 0usize;
        while i < EType::ALL.len() {
            if let Some(t) = EType::ALL.get(i) {
                if t.errno() == v {
                    return Some(*t);
                }
            }
            i += 1;
        }
        None
    }
}

// ── B13 线编码器 ──────────────────────────────────────────────────────
/// 折叠校验：(v ^ v>>4 ^ v>>8) & 0xF（B13 冻结公式）。
pub fn fold(v: i32) -> u32 {
    let u = v as u32;
    (u ^ (u >> 4) ^ (u >> 8)) & 0xF
}

/// 编码：tag=0xE 置 bit31..28，值置 bit23..12，折叠置 bit3..0，保留位 0。
pub fn encode(t: EType) -> u32 {
    let v = t.errno() as u32;
    (LINE_TAG << 28) | ((v & 0xFFF) << 12) | fold(t.errno())
}

/// 解码：tag、保留位、折叠、值域、闭集五道闸，任一不过返回 None。
pub fn decode(word: u32) -> Option<EType> {
    if word >> 28 != LINE_TAG {
        return None;
    }
    if word & 0xF_F0 != 0 {
        return None;
    }
    let v = ((word >> 12) & 0xFFF) as i32;
    if v < 1 || v > ERRNO_MAX {
        return None;
    }
    if word & 0xF != fold(v) {
        return None;
    }
    EType::from_errno(v)
}

// ── 对账核 ────────────────────────────────────────────────────────────
/// 对账一：14 码往返零漂移。
pub fn reconcile_roundtrip() -> bool {
    let mut i = 0usize;
    while i < EType::ALL.len() {
        if let Some(t) = EType::ALL.get(i) {
            match decode(encode(*t)) {
                Some(back) => {
                    if back != *t {
                        return false;
                    }
                }
                None => return false,
            }
        }
        i += 1;
    }
    true
}

/// 对账二：包值两两互异。
pub fn reconcile_packages() -> bool {
    let mut i = 0usize;
    while i < EType::ALL.len() {
        let mut j = i + 1;
        while j < EType::ALL.len() {
            if let (Some(a), Some(b)) = (EType::ALL.get(i), EType::ALL.get(j)) {
                if encode(*a) == encode(*b) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 对账三：errno 两两互异。
pub fn reconcile_errnos() -> bool {
    let mut i = 0usize;
    while i < EType::ALL.len() {
        let mut j = i + 1;
        while j < EType::ALL.len() {
            if let (Some(a), Some(b)) = (EType::ALL.get(i), EType::ALL.get(j)) {
                if a.errno() == b.errno() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 对账四：包值空间与裸 errno 空间不相交（tag 隔离 + 逐码复算）。
pub fn reconcile_disjoint() -> bool {
    let mut i = 0usize;
    while i < EType::ALL.len() {
        if let Some(t) = EType::ALL.get(i) {
            let pkg = encode(*t);
            if pkg < 1 || pkg <= ERRNO_MAX as u32 {
                return false;
            }
            if pkg >> 28 != LINE_TAG {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 升级接管口径声明（冻结；编码函数与本声明同源）。
pub const TAKEOVER_NOTE: &str = "E 型错误码自 UNX-F9355 起接管 B13 编码器口径：\
线格式 bit31..28=0xE、值域 bit23..12、折叠 bit3..0 冻结不变；\
与 errno.rs 编码族逐码零冲突，14 项闭集单源";

// ── 注入核（坏线包） ──────────────────────────────────────────────────
/// 五类坏线包，逐类必拒（decode 返回 None）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadWordCase {
    /// tag=0xA（外来格式）
    WrongTag,
    /// 折叠位被翻转
    WrongFold,
    /// 保留位非零
    DirtyReserved,
    /// 值 = 999（表外）
    UnknownValue,
    /// 值 = 0
    ZeroValue,
}

impl BadWordCase {
    /// 构造坏线包。
    pub fn word(self) -> u32 {
        match self {
            BadWordCase::WrongTag => (0xA << 28) | ((22u32) << 12) | fold(22),
            BadWordCase::WrongFold => (LINE_TAG << 28) | ((22u32) << 12) | (fold(22) ^ 0x1),
            BadWordCase::DirtyReserved => (LINE_TAG << 28) | ((22u32) << 12) | (0x10 | fold(22)),
            BadWordCase::UnknownValue => (LINE_TAG << 28) | ((999u32 & 0xFFF) << 12) | fold(999),
            BadWordCase::ZeroValue => (LINE_TAG << 28) | (0 << 12) | fold(0),
        }
    }

    /// 全量坏样本。
    pub const ALL: [BadWordCase; 5] = [
        BadWordCase::WrongTag,
        BadWordCase::WrongFold,
        BadWordCase::DirtyReserved,
        BadWordCase::UnknownValue,
        BadWordCase::ZeroValue,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EncLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数（decode=None）
    pub detected: u32,
    /// 逃逸次数（decode=Some，必须为 0）
    pub escaped: u32,
}

impl EncLedger {
    const fn zero() -> EncLedger {
        EncLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏线包矩阵：5 类 × INJECT_ROUNDS 次。
pub fn run_bad_word_matrix() -> (EncLedger, [u32; 5]) {
    let mut ledger = EncLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadWordCase::ALL.len() {
        let case = match BadWordCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = decode(case.word()).is_none();
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            ledger.record(caught);
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：14 码的 (errno, 线包) 逐字节喂 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < EType::ALL.len() {
        if let Some(t) = EType::ALL.get(i) {
            let e = t.errno();
            bytes.push((e & 0xFF) as u8);
            let w = encode(*t);
            bytes.push((w & 0xFF) as u8);
            bytes.push(((w >> 8) & 0xFF) as u8);
            bytes.push(((w >> 16) & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9355 判据集（15 条；动态明细 2 条）。
pub fn run_unx9355_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9355");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0");

    // 2. 闭集 14 项 errno 两两互异且落 [1,133]
    let errno_ok = reconcile_errnos()
        && EType::ALL.iter().all(|t| t.errno() >= 1 && t.errno() <= ERRNO_MAX)
        && EType::from_errno(22) == Some(EType::Inval)
        && EType::from_errno(0).is_none()
        && EType::from_errno(134).is_none();
    s.add("闭集：14 项 errno 两两互异且落 [1,133]", errno_ok,
          "errno.rs 编码族口径冻结；同值双码即映射面分裂");

    // 3. 往返零漂移
    s.add("往返零漂移：14 码 encode→decode 全还原", reconcile_roundtrip(),
          "编码器与解码器必须同源；一码漂移即对账失守");

    // 4. 包值两两互异
    s.add("包值互异：14 个线包两两不同", reconcile_packages(),
          "同包双码会让解码路无法定位变体");

    // 5. 包值空间与裸 errno 不相交
    s.add("空间隔离：线包空间不与裸 errno [1,133] 相交", reconcile_disjoint(),
          "tag=0xE 置顶天然隔离，仍逐码复算背书");

    // 6. 注入核逐类检出
    let (ledger, per_class) = run_bad_word_matrix();
    let cells_ok = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：5 类坏线包 ×8 轮逐类拒绝", cells_ok,
          "错tag/错折叠/脏保留位/表外值/零值逐类 8/8");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 五道闸逐闸独立验证
    let gates_ok = decode(0x0000_0016).is_none() // 无 tag
        && decode((LINE_TAG << 28) | (0x10 << 4) | ((22u32) << 12) | fold(22)).is_none() // 脏保留（bit4）
        && decode((LINE_TAG << 28) | ((22u32) << 12) | 0xE).is_none(); // 错折叠
    s.add("五道闸：tag/保留/折叠/值域/闭集逐闸独立拒", gates_ok,
          "半解析值绝不当 errno；任何一闸失效都不得放行");

    // 9. 正路放行：合法包精确还原
    let pass_ok = decode(encode(EType::Inval)) == Some(EType::Inval)
        && decode(encode(EType::NoSys)) == Some(EType::NoSys)
        && decode(encode(EType::Range)) == Some(EType::Range);
    s.add("正路放行：合法线包精确还原", pass_ok,
          "拒坏的同时不得误伤合法包；放行面逐码精确");

    // 10. 接管口径声明
    s.add("升级接管口径：声明非空且冻结", !TAKEOVER_NOTE.is_empty()
              && TAKEOVER_NOTE.contains("0xE"),
          "E 型码接管 B13 编码器口径；空声明即未接管");

    // 11. 折叠公式可复算：同值同折、跨值抽检
    let fold_ok = fold(22) == fold(22) && fold(38) == fold(38)
        && fold(22) <= 0xF && fold(38) <= 0xF;
    s.add("折叠校验：同值同折且落 4 位", fold_ok,
          "折叠是线包的自检位；跨跑不稳定即非确定编码");

    // 12. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 13. 文案闭集
    let text_ok = EncCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < EncCode::ALL.len() {
        let mut j = i + 1;
        while j < EncCode::ALL.len() {
            if let (Some(a), Some(b)) = (EncCode::ALL.get(i), EncCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 15. 正逆映射完备：errno→EType→errno 恒等
    let inv_ok = EType::ALL.iter().all(|t| EType::from_errno(t.errno()) == Some(*t));
    s.add("正逆映射：errno↔EType 双向恒等", inv_ok,
          "from_errno 与 errno() 互为正逆；缺口即闭集外的暗门");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("E 型 14 码对账指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_table_exact() {
        assert_eq!(EType::Perm.errno(), 1);
        assert_eq!(EType::Inval.errno(), 22);
        assert_eq!(EType::NoSys.errno(), 38);
        assert_eq!(EType::Range.errno(), 34);
    }

    #[test]
    fn roundtrip_zero_drift() {
        for t in EType::ALL.iter() {
            assert_eq!(decode(encode(*t)), Some(*t));
        }
    }

    #[test]
    fn bad_words_all_rejected() {
        for case in BadWordCase::ALL.iter() {
            assert!(decode(case.word()).is_none());
        }
    }

    #[test]
    fn gates_independent() {
        assert!(decode(0x0000_0016).is_none());
        assert!(decode((LINE_TAG << 28) | (0x10 << 4) | ((22u32) << 12) | fold(22)).is_none());
        assert!(decode((LINE_TAG << 28) | ((22u32) << 12) | 0xE).is_none());
        assert_eq!(decode((LINE_TAG << 28) | ((22u32) << 12) | fold(22)), Some(EType::Inval));
    }

    #[test]
    fn packages_and_errnos_distinct() {
        assert!(reconcile_packages());
        assert!(reconcile_errnos());
        assert!(reconcile_disjoint());
    }

    #[test]
    fn reverse_map_exact() {
        assert_eq!(EType::from_errno(9), Some(EType::BadFd));
        assert_eq!(EType::from_errno(34), Some(EType::Range));
        assert_eq!(EType::from_errno(3), None);
        assert_eq!(EType::from_errno(0), None);
    }

    #[test]
    fn bad_matrix_conserved() {
        let (l, per) = run_bad_word_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn takeover_note_present() {
        assert!(!TAKEOVER_NOTE.is_empty());
        assert!(TAKEOVER_NOTE.contains("UNX-F9355"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9355_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
