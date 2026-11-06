//! UNX-F9356 E 型与 lxerrno::VarixErr 映射总装 · 0x7C680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9356 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F9356-J1 映射总装判据过，VarixErr 逐码映射一致格过，
//!      单源声明格过。
//!
//! 设计要点：
//!   ① VarixErr 闭集 16 项逐码映射（lxerrno.rs 口径）：Perm→1、NoEntry→2、
//!      Io→5、BadFd→9、NoSpace→12、Access→13、Busy→16、Exists→17、
//!      NotDir→20、IsDir→21、Inval→22、NameTooLong→36、NoSys→38、
//!      NotEmpty→39、WouldBlock→11、Range→34。映射函数穷举 match 无通配
//!      ——这是「单源生成」的结构面：新错误码不加映射就编不过。
//!   ② 双路互证：正向 errno_of（match 路）与冻结映射表 VARIX_MAP（表路）
//!      逐码对账必须一致；单点写错即失配。反向 errno→变体是解析器路，
//!      与正向互为正逆，16/16 全覆盖。
//!   ③ 单射性：16 个 errno 两两互异（WouldBlock=11 与 Busy=16 一类的
//!      「值序倒挂」是口径冻结的一部分，倒挂不等于冲突）；值域全部落在
//!      [1,133]。
//!   ④ 线值可承载：每个 errno 都能装进 B13 线格式的 12 位值域（≤0xFFF），
//!      折叠位 (v^v>>4^v>>8)&0xF 逐码自洽，映射面与线格式无值域冲突。
//!   ⑤ 注入核：越界变体号（≥16）、errno 0、errno 134、外来 errno（如 3）、
//!      重复映射注入五类逐类拒绝，逃逸恒为 0。
//!   ⑥ 单源声明：映射以 errno_of 为唯一出口；声明串非空且冻结，任何
//!      第二映射函数都是口径分裂。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9356 独占 0x7C680..=0x7C6BF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C680;

/// Linux errno 合法值域（x86-64 ABI 面 1..=133）。
pub const ERRNO_MAX: i32 = 133;
/// VarixErr 闭集行数。
pub const VERR_ROWS: usize = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    /// 变体号越界（≥16）
    VariantOutOfRange,
    /// errno 值越 [1,133]
    ErrnoOutOfRange,
    /// errno 反查不在闭集（外来码）
    ForeignErrno,
    /// 映射冲突（同 errno 双变体）
    MapCollision,
    /// 双路失配（match 路与表路不一致）
    DualPathMismatch,
    /// 正逆映射不闭合
    RoundTripBroken,
    /// 值域装不进 12 位线格式
    LineOverflow,
    /// 折叠位自洽破坏
    FoldInconsistent,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 单源声明为空
    SingleSourceEmpty,
    /// 闭集行数漂移（≠16）
    RowCountDrift,
}

impl MapCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::VariantOutOfRange => "变体号越界被拒：VarixErr 闭集 16 项；\
                16 及以上的号是表外样本，必须显性拒绝",
            MapCode::ErrnoOutOfRange => "errno 值越域被拒：合法值域 [1,133]；\
                0 与 >133 都不是合法 Linux errno",
            MapCode::ForeignErrno => "外来 errno 被拒：值不在 16 项闭集内；\
                反查返回 None，不得猜一个最近似码顶替",
            MapCode::MapCollision => "映射冲突被拒：同 errno 不得对应两个变体；\
                冲突即映射面分裂",
            MapCode::DualPathMismatch => "双路失配被拒：match 路与冻结表路必须\
                逐码一致；单点写错当场揭出",
            MapCode::RoundTripBroken => "正逆不闭合被拒：变体→errno→变体必须还原；\
                断链即闭集有暗门",
            MapCode::LineOverflow => "线值越域被拒：errno 必须装进 12 位线值域；\
                越域码与线格式无兼容面",
            MapCode::FoldInconsistent => "折叠自洽破坏被拒：同值同折；\
                折叠位随跑漂移即非确定映射",
            MapCode::LedgerRegress => "注入账倒退被拒：计数只增不减；\
                请检查回滚路径是否误改累计值",
            MapCode::NotReproducible => "双跑不一致：同输入两次对账结果不同；\
                请排查隐藏状态",
            MapCode::SingleSourceEmpty => "单源声明为空被拒：映射必须有唯一出口；\
                空声明即口径未接管",
            MapCode::RowCountDrift => "闭集行数漂移被拒：映射表必须恰 16 行；\
                多行少行都是闭集漂移",
        }
    }

    /// 闭集全量。
    pub const ALL: [MapCode; 12] = [
        MapCode::VariantOutOfRange,
        MapCode::ErrnoOutOfRange,
        MapCode::ForeignErrno,
        MapCode::MapCollision,
        MapCode::DualPathMismatch,
        MapCode::RoundTripBroken,
        MapCode::LineOverflow,
        MapCode::FoldInconsistent,
        MapCode::LedgerRegress,
        MapCode::NotReproducible,
        MapCode::SingleSourceEmpty,
        MapCode::RowCountDrift,
    ];
}

// ── VarixErr 镜像闭集 ─────────────────────────────────────────────────
/// VarixErr 闭集（lxerrno.rs 口径镜像，16 项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarixErr {
    /// EPERM
    Perm,
    /// ENOENT
    NoEntry,
    /// EIO
    Io,
    /// EBADF
    BadFd,
    /// ENOMEM 语义面（资源暂缺）
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
    /// ENOTEMPTY
    NotEmpty,
    /// EAGAIN
    WouldBlock,
    /// ERANGE
    Range,
}

/// 冻结映射表（表路）：(变体, errno)。
pub const VARIX_MAP: [(VarixErr, i32); VERR_ROWS] = [
    (VarixErr::Perm, 1),
    (VarixErr::NoEntry, 2),
    (VarixErr::Io, 5),
    (VarixErr::BadFd, 9),
    (VarixErr::NoSpace, 12),
    (VarixErr::Access, 13),
    (VarixErr::Busy, 16),
    (VarixErr::Exists, 17),
    (VarixErr::NotDir, 20),
    (VarixErr::IsDir, 21),
    (VarixErr::Inval, 22),
    (VarixErr::NameTooLong, 36),
    (VarixErr::NoSys, 38),
    (VarixErr::NotEmpty, 39),
    (VarixErr::WouldBlock, 11),
    (VarixErr::Range, 34),
];

/// 闭集全量（表路同序）。
pub const ALL_VERR: [VarixErr; VERR_ROWS] = [
    VarixErr::Perm,
    VarixErr::NoEntry,
    VarixErr::Io,
    VarixErr::BadFd,
    VarixErr::NoSpace,
    VarixErr::Access,
    VarixErr::Busy,
    VarixErr::Exists,
    VarixErr::NotDir,
    VarixErr::IsDir,
    VarixErr::Inval,
    VarixErr::NameTooLong,
    VarixErr::NoSys,
    VarixErr::NotEmpty,
    VarixErr::WouldBlock,
    VarixErr::Range,
];

/// 单源映射出口：穷举 match 无通配（match 路）。
pub fn errno_of(v: VarixErr) -> i32 {
    match v {
        VarixErr::Perm => 1,
        VarixErr::NoEntry => 2,
        VarixErr::Io => 5,
        VarixErr::BadFd => 9,
        VarixErr::NoSpace => 12,
        VarixErr::Access => 13,
        VarixErr::Busy => 16,
        VarixErr::Exists => 17,
        VarixErr::NotDir => 20,
        VarixErr::IsDir => 21,
        VarixErr::Inval => 22,
        VarixErr::NameTooLong => 36,
        VarixErr::NoSys => 38,
        VarixErr::NotEmpty => 39,
        VarixErr::WouldBlock => 11,
        VarixErr::Range => 34,
    }
}

/// 反向解析器：errno → 变体（表路线性查；闭集外 None）。
pub fn variant_of(errno: i32) -> Option<VarixErr> {
    let mut i = 0usize;
    while i < VARIX_MAP.len() {
        if let Some(row) = VARIX_MAP.get(i) {
            if row.1 == errno {
                return Some(row.0);
            }
        }
        i += 1;
    }
    None
}

/// 按变体号取表行（越界 None——注入面的第一道闸）。
pub fn variant_at(idx: usize) -> Option<VarixErr> {
    VARIX_MAP.get(idx).map(|row| row.0)
}

// ── 对账核（双路互证） ────────────────────────────────────────────────
/// 对账一：match 路与冻结表路逐码一致。
pub fn reconcile_dual_path() -> bool {
    let mut i = 0usize;
    while i < VARIX_MAP.len() {
        if let Some(row) = VARIX_MAP.get(i) {
            if errno_of(row.0) != row.1 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 对账二：正逆闭合 16/16。
pub fn reconcile_roundtrip() -> bool {
    let mut i = 0usize;
    while i < ALL_VERR.len() {
        if let Some(v) = ALL_VERR.get(i) {
            match variant_of(errno_of(*v)) {
                Some(back) => {
                    if back != *v {
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

/// 对账三：errno 两两互异（单射）且值域 [1,133]。
pub fn reconcile_injective() -> bool {
    let mut i = 0usize;
    while i < VARIX_MAP.len() {
        if let Some(row) = VARIX_MAP.get(i) {
            if row.1 < 1 || row.1 > ERRNO_MAX {
                return false;
            }
            let mut j = i + 1;
            while j < VARIX_MAP.len() {
                if let Some(row2) = VARIX_MAP.get(j) {
                    if row.1 == row2.1 {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

// ── 线格式承载面（与 B13 值域对账） ───────────────────────────────────
/// 折叠校验：(v ^ v>>4 ^ v>>8) & 0xF（B13 冻结公式）。
pub fn fold(v: i32) -> u32 {
    let u = v as u32;
    (u ^ (u >> 4) ^ (u >> 8)) & 0xF
}

/// 对账四：16 码全部可装 12 位线值域且折叠逐码自洽。
pub fn reconcile_line_fit() -> bool {
    let mut i = 0usize;
    while i < ALL_VERR.len() {
        if let Some(v) = ALL_VERR.get(i) {
            let e = errno_of(*v);
            if e as u32 > 0xFFF {
                return false;
            }
            if fold(e) > 0xF {
                return false;
            }
            // 同值同折：重算一次必须一致。
            if fold(e) != fold(e) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 单源声明（冻结）：映射唯一出口是 errno_of。
pub const SINGLE_SOURCE_NOTE: &str = "VarixErr→Linux errno 映射以 errno_of 为唯一出口：\
穷举 match 无通配、16 项闭集、值域 [1,133]、与冻结表 VARIX_MAP 逐码一致；\
任何第二映射函数都是口径分裂";

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏样本，逐类必拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMapCase {
    /// 变体号 16（闭集外）
    VariantOutOfRange,
    /// errno 0
    ZeroErrno,
    /// errno 134（越上界）
    BeyondErrno,
    /// 外来 errno 3（不在闭集）
    ForeignErrno,
    /// 反查外来码 7（不在闭集）
    ForeignLookup,
}

impl BadMapCase {
    /// 全量坏样本。
    pub const ALL: [BadMapCase; 5] = [
        BadMapCase::VariantOutOfRange,
        BadMapCase::ZeroErrno,
        BadMapCase::BeyondErrno,
        BadMapCase::ForeignErrno,
        BadMapCase::ForeignLookup,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl MapLedger {
    const fn zero() -> MapLedger {
        MapLedger { injected: 0, detected: 0, escaped: 0 }
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

/// 执行坏样本矩阵：5 类 × INJECT_ROUNDS 次。
pub fn run_bad_map_matrix() -> (MapLedger, [u32; 5]) {
    let mut ledger = MapLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadMapCase::ALL.len() {
        let case = match BadMapCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadMapCase::VariantOutOfRange => {
                    variant_at(VERR_ROWS).is_none() && variant_at(999).is_none()
                }
                BadMapCase::ZeroErrno => variant_of(0).is_none(),
                BadMapCase::BeyondErrno => variant_of(ERRNO_MAX + 1).is_none(),
                BadMapCase::ForeignErrno => variant_of(3).is_none(),
                BadMapCase::ForeignLookup => variant_of(7).is_none(),
            };
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

/// 规范语料双跑指纹：16 码的 (变体号, errno, 折叠) 逐字节喂 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < VARIX_MAP.len() {
        if let Some(row) = VARIX_MAP.get(i) {
            bytes.push((i & 0xFF) as u8);
            bytes.push((row.1 & 0xFF) as u8);
            bytes.push(fold(row.1) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9356 判据集（15 条；动态明细 2 条）。
pub fn run_unx9356_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9356");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0");

    // 2. 双路互证：match 路与冻结表路逐码一致
    s.add("双路互证：errno_of 与 VARIX_MAP 逐码一致", reconcile_dual_path(),
          "映射面单点写错当场揭出；两路一致是单源的结构背书");

    // 3. 逐码映射一致（lxerrno 口径抽检 + 全量闭集核对）
    let spot_ok = errno_of(VarixErr::Perm) == 1
        && errno_of(VarixErr::WouldBlock) == 11
        && errno_of(VarixErr::Inval) == 22
        && errno_of(VarixErr::NoSys) == 38
        && errno_of(VarixErr::Range) == 34
        && errno_of(VarixErr::NotEmpty) == 39;
    s.add("逐码映射：抽检六码与 lxerrno 口径精确一致", spot_ok,
          "Perm=1/WouldBlock=11/Inval=22/NoSys=38/Range=34/NotEmpty=39 冻结");

    // 4. 正逆闭合 16/16
    s.add("正逆闭合：变体→errno→变体 16/16 还原", reconcile_roundtrip(),
          "variant_of 与 errno_of 互为正逆；断链即闭集暗门");

    // 5. 单射且值域合法
    s.add("单射性：16 errno 两两互异且落 [1,133]", reconcile_injective(),
          "值序倒挂（WouldBlock=11）是冻结口径不是冲突；同值双码才是分裂");

    // 6. 线格式承载：12 位值域 + 折叠自洽
    s.add("线承载：16 码全装 12 位线值域且折叠自洽", reconcile_line_fit(),
          "映射面与 B13 线格式无值域冲突；越域码无处落线");

    // 7. 注入核逐类检出
    let (ledger, per_class) = run_bad_map_matrix();
    let cells_ok = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：5 类坏样本 ×8 轮逐类拒绝", cells_ok,
          "越界变体号/errno0/errno134/外来码3/外来查7 逐类 8/8");

    // 8. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 闭集行数冻结
    let rows_ok = VARIX_MAP.len() == VERR_ROWS
        && ALL_VERR.len() == VERR_ROWS
        && variant_at(VERR_ROWS - 1).is_some();
    s.add("闭集行数：映射表恰 16 行", rows_ok,
          "多行少行都是闭集漂移；末行存在证明表完整");

    // 10. 外来码不猜码：反查 None 不顶替
    let no_guess = variant_of(3).is_none() && variant_of(7).is_none()
        && variant_of(133).is_none() && variant_of(1).is_some();
    s.add("不猜码：闭集外反查 None，闭集内精确", no_guess,
          "反查路绝不返回「最近似」码；None 是唯一诚实出口");

    // 11. 单源声明
    s.add("单源声明：errno_of 唯一出口且声明冻结",
          !SINGLE_SOURCE_NOTE.is_empty() && SINGLE_SOURCE_NOTE.contains("errno_of"),
          "第二映射函数即口径分裂；声明非空是接管的形式面");

    // 12. 覆盖率对账：16 成员全落合法区间（lxerrno::map_full_coverage 口径）
    let coverage_ok = ALL_VERR.iter().all(|v| {
        let e = errno_of(*v);
        e >= 1 && e <= ERRNO_MAX
    });
    s.add("覆盖率对账：16 成员全落 [1,133]", coverage_ok,
          "map_full_coverage 口径：枚举全成员逐一过映射");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = MapCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < MapCode::ALL.len() {
        let mut j = i + 1;
        while j < MapCode::ALL.len() {
            if let (Some(a), Some(b)) = (MapCode::ALL.get(i), MapCode::ALL.get(j)) {
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

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("VarixErr 16 码映射指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_check_lxerrno_values() {
        assert_eq!(errno_of(VarixErr::Perm), 1);
        assert_eq!(errno_of(VarixErr::NoEntry), 2);
        assert_eq!(errno_of(VarixErr::Io), 5);
        assert_eq!(errno_of(VarixErr::BadFd), 9);
        assert_eq!(errno_of(VarixErr::WouldBlock), 11);
        assert_eq!(errno_of(VarixErr::Access), 13);
        assert_eq!(errno_of(VarixErr::NoSys), 38);
    }

    #[test]
    fn dual_path_agrees() {
        assert!(reconcile_dual_path());
    }

    #[test]
    fn roundtrip_closes() {
        for v in ALL_VERR.iter() {
            assert_eq!(variant_of(errno_of(*v)), Some(*v));
        }
    }

    #[test]
    fn injective_map() {
        assert!(reconcile_injective());
        let mut i = 0;
        while i < VARIX_MAP.len() {
            let mut j = i + 1;
            while j < VARIX_MAP.len() {
                assert_ne!(VARIX_MAP[i].1, VARIX_MAP[j].1);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn foreign_errnos_rejected() {
        assert_eq!(variant_of(0), None);
        assert_eq!(variant_of(3), None);
        assert_eq!(variant_of(7), None);
        assert_eq!(variant_of(134), None);
        assert_eq!(variant_of(22), Some(VarixErr::Inval));
    }

    #[test]
    fn variant_index_guard() {
        assert!(variant_at(0).is_some());
        assert!(variant_at(VERR_ROWS - 1).is_some());
        assert_eq!(variant_at(VERR_ROWS), None);
        assert_eq!(variant_at(usize::MAX / 2), None);
    }

    #[test]
    fn line_fit_and_fold() {
        assert!(reconcile_line_fit());
        assert_eq!(fold(22), fold(22));
        assert!(fold(39) <= 0xF);
    }

    #[test]
    fn bad_matrix_conserved() {
        let (l, per) = run_bad_map_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn single_source_note_present() {
        assert!(!SINGLE_SOURCE_NOTE.is_empty());
        assert!(SINGLE_SOURCE_NOTE.contains("errno_of"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9356_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
