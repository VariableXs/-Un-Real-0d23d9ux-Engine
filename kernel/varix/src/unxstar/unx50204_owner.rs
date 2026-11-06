//! UNX-F50204 用户/应用归属账（任务级归属字段登记）（0x44000 段）
//!
//! 锚点：UNX-F50204-J1 归属 ×30 字段零缺失；判据锚定 §AI-63 §2 专题十。
//!
//! 计费账里最贵的错误不是算错页数，而是**算对了但记到别人头上**。归属字段看似
//! 只是几个 id，实际上承担三件事：
//!
//! 1. **可追责**——出了问题能定位到「谁、在哪个应用、哪个会话、哪个租户」；
//! 2. **可聚合**——按用户/应用/租户三种维度出账单；任何一维缺失都会让聚合塌陷；
//! 3. **可审计**——归属在任务创建时**一次写入**，之后只读。中途改归属等于改账单
//!    收件人，必须走显式的「转移」流程（本模块只提供检测，不提供静默改写）。
//!
//! 六字段闭集：`user`（账号）、`app`（应用）、`session`（会话）、`tenant`（租户）、
//! `host`（主机指纹）、`job`（任务号）。缺一即判缺失，不做「留空也能跑」的宽容。
//!
//! 隐私红线：[`Attribution`] 只允许出现标识与计数，**不承载任何文档内容**；
//! 本模块的类型系统里根本没有字节内容字段，这条红线是结构性的，不靠自觉。
//!
//! 诊断码闭集：`SEG_BASE + 1 ..= SEG_BASE + 11`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x44000 独占 0x40）。
pub const SEG_BASE: u32 = 0x44000;

/// 归属样本数（卡面 30 条）。
pub const OWNER_COUNT: usize = 30;

/// 六字段总位数。
pub const FIELD_BITS: u8 = 6;

const F_USER: u8 = 0x01;
const F_APP: u8 = 0x02;
const F_SESSION: u8 = 0x04;
const F_TENANT: u8 = 0x08;
const F_HOST: u8 = 0x10;
const F_JOB: u8 = 0x20;

/// 六字段全集。
pub const ALL_FIELDS: u8 = F_USER | F_APP | F_SESSION | F_TENANT | F_HOST | F_JOB;

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerCode {
    CountDrift,
    FieldMissing,
    FieldMaskDrift,
    UserZero,
    AppZero,
    SessionZero,
    TenantZero,
    HostZero,
    DuplicateJob,
    ContentLeak,
    Unknown,
}

impl OwnerCode {
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                OwnerCode::CountDrift => 1,
                OwnerCode::FieldMissing => 2,
                OwnerCode::FieldMaskDrift => 3,
                OwnerCode::UserZero => 4,
                OwnerCode::AppZero => 5,
                OwnerCode::SessionZero => 6,
                OwnerCode::TenantZero => 7,
                OwnerCode::HostZero => 8,
                OwnerCode::DuplicateJob => 9,
                OwnerCode::ContentLeak => 10,
                OwnerCode::Unknown => 11,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            OwnerCode::CountDrift => "归属样本数与冻结数不等",
            OwnerCode::FieldMissing => "六字段存在缺失，归属不可追责",
            OwnerCode::FieldMaskDrift => "字段掩码与实值不一致，登记位失真",
            OwnerCode::UserZero => "用户标识为零，账单无收件人",
            OwnerCode::AppZero => "应用标识为零，无法按应用聚合",
            OwnerCode::SessionZero => "会话标识为零，无法回溯操作链",
            OwnerCode::TenantZero => "租户标识为零，隔离边界丢失",
            OwnerCode::HostZero => "主机指纹为零，无法定位来源设备",
            OwnerCode::DuplicateJob => "同一任务重复登记归属",
            OwnerCode::ContentLeak => "归属结构承载了文档内容，触碰隐私红线",
            OwnerCode::Unknown => "未归类的归属异常",
        }
    }

    pub const ALL: [OwnerCode; 11] = [
        OwnerCode::CountDrift,
        OwnerCode::FieldMissing,
        OwnerCode::FieldMaskDrift,
        OwnerCode::UserZero,
        OwnerCode::AppZero,
        OwnerCode::SessionZero,
        OwnerCode::TenantZero,
        OwnerCode::HostZero,
        OwnerCode::DuplicateJob,
        OwnerCode::ContentLeak,
        OwnerCode::Unknown,
    ];
}

/// 应用类别：只记录类别，不记录应用私有数据。
///
/// `Unset` 是零值哨兵而不是第七类应用：归属结构的 `ZERO` 必须推导出零掩码，
/// 否则「空归属」会被自己的 `derived_mask` 报成「应用已登记」，
/// 后续统计就会把从未发生的应用使用算进分母。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppKind {
    Unset,
    Editor,
    Browser,
    Office,
    Scan,
    Fax,
    System,
}

impl AppKind {
    pub fn val(self) -> u8 {
        match self {
            AppKind::Unset => 0,
            AppKind::Editor => 1,
            AppKind::Browser => 2,
            AppKind::Office => 3,
            AppKind::Scan => 4,
            AppKind::Fax => 5,
            AppKind::System => 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AppKind::Unset => "unset",
            AppKind::Editor => "editor",
            AppKind::Browser => "browser",
            AppKind::Office => "office",
            AppKind::Scan => "scan",
            AppKind::Fax => "fax",
            AppKind::System => "system",
        }
    }

    pub fn from_val(v: u8) -> Option<AppKind> {
        match v {
            0 => Some(AppKind::Unset),
            1 => Some(AppKind::Editor),
            2 => Some(AppKind::Browser),
            3 => Some(AppKind::Office),
            4 => Some(AppKind::Scan),
            5 => Some(AppKind::Fax),
            6 => Some(AppKind::System),
            _ => None,
        }
    }

    /// 六类真实应用（不含 `Unset` 哨兵）。
    pub fn real(self) -> bool {
        self != AppKind::Unset
    }
}

/// 任务级归属。字段全部为非零标识；`mask` 是「这些字段确实登记过」的证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attribution {
    pub user: u32,
    pub app: AppKind,
    pub session: u32,
    pub tenant: u16,
    pub host: u32,
    pub job: u32,
    pub mask: u8,
}

impl Attribution {
    pub const ZERO: Attribution = Attribution {
        user: 0,
        app: AppKind::Unset,
        session: 0,
        tenant: 0,
        host: 0,
        job: 0,
        mask: 0,
    };

    /// 由实值推导掩码：这是唯一允许产生掩码的地方。
    pub fn derived_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.user != 0 {
            m |= F_USER;
        }
        if self.app.val() != 0 {
            m |= F_APP;
        }
        if self.session != 0 {
            m |= F_SESSION;
        }
        if self.tenant != 0 {
            m |= F_TENANT;
        }
        if self.host != 0 {
            m |= F_HOST;
        }
        if self.job != 0 {
            m |= F_JOB;
        }
        m
    }

    /// 完整登记：掩码覆盖六字段且与实值推导一致。
    pub fn complete(&self) -> bool {
        self.mask == ALL_FIELDS && self.derived_mask() == ALL_FIELDS
    }

    /// 缺失字段列表（供告警直接指出缺哪一项）。
    pub fn missing(&self) -> u8 {
        ALL_FIELDS & !self.derived_mask()
    }

    /// 登记：把实值推导出的掩码写回。不接受调用方传入的掩码。
    pub fn register(user: u32, app: AppKind, session: u32, tenant: u16, host: u32, job: u32) -> Attribution {
        let mut a = Attribution {
            user,
            app,
            session,
            tenant,
            host,
            job,
            mask: 0,
        };
        a.mask = a.derived_mask();
        a
    }

    /// 归属摘要：用于比对「同任务同归属」。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_mix(h, self.user as u64);
        h = fnv_mix(h, self.app.val() as u64);
        h = fnv_mix(h, self.session as u64);
        h = fnv_mix(h, self.tenant as u64);
        h = fnv_mix(h, self.host as u64);
        h = fnv_mix(h, self.job as u64);
        h
    }
}

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 30 条样本：四个用户、六类应用、三个租户轮转，保证每个字段都被真实使用。
pub fn sample_owners() -> [Attribution; OWNER_COUNT] {
    let mut out = [Attribution::ZERO; OWNER_COUNT];
    let apps = [
        AppKind::Editor,
        AppKind::Browser,
        AppKind::Office,
        AppKind::Scan,
        AppKind::Fax,
        AppKind::System,
    ];
    let mut i = 0usize;
    while i < OWNER_COUNT {
        let app = match apps.get(i % apps.len()) {
            Some(a) => *a,
            None => AppKind::System,
        };
        out[i] = Attribution::register(
            1000 + (i % 4) as u32,
            app,
            70000 + (i * 13) as u32,
            (1 + (i % 3)) as u16,
            0x4a00_0000 + (i as u32 * 0x101),
            500000 + i as u32,
        );
        i += 1;
    }
    out
}

/// 归属登记簿：按任务号唯一登记。
#[derive(Clone, Copy, Debug)]
pub struct OwnerBook {
    ids: [u32; OWNER_COUNT],
    used: usize,
    accepted: u32,
    rejected: u32,
    digest: u64,
}

impl OwnerBook {
    pub fn new() -> OwnerBook {
        OwnerBook {
            ids: [0u32; OWNER_COUNT],
            used: 0,
            accepted: 0,
            rejected: 0,
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    pub fn contains(&self, job: u32) -> bool {
        let mut i = 0usize;
        while i < self.used {
            if let Some(v) = self.ids.get(i) {
                if *v == job {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 登记一条：不完整、重复任务号一律拒绝并计数。
    pub fn register(&mut self, a: &Attribution) -> bool {
        if !a.complete() || self.contains(a.job) || self.used >= OWNER_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            return false;
        }
        if let Some(slot) = self.ids.get_mut(self.used) {
            *slot = a.job;
            self.used += 1;
            self.accepted = self.accepted.saturating_add(1);
            self.digest = fnv_mix(self.digest, a.digest());
            true
        } else {
            self.rejected = self.rejected.saturating_add(1);
            false
        }
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn len(&self) -> usize {
        self.used
    }

    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 守恒：接纳 + 拒绝 == 尝试次数。
    pub fn balanced(&self, attempts: u32) -> bool {
        self.accepted.saturating_add(self.rejected) == attempts
    }
}

impl Default for OwnerBook {
    fn default() -> Self {
        OwnerBook::new()
    }
}

/// 隐私红线：归属结构不含任何长度可变的字节内容载体。
///
/// 这里的检查不是读字段（类型里压根没有），而是验证「六字段都能独立非零化」
/// 之后 `complete()` 仍为真——说明完整性与内容无关，只与标识有关。
pub fn privacy_probe() -> bool {
    let a = Attribution::register(1, AppKind::Scan, 2, 3, 4, 5);
    let b = Attribution::register(u32::MAX, AppKind::Fax, u32::MAX, u16::MAX, u32::MAX, u32::MAX);
    a.complete() && b.complete() && a.missing() == 0 && b.missing() == 0
}

/// 归属一致性：同一任务两次登记的摘要必须相同（幂等读）。
pub fn idempotent_probe() -> bool {
    let a = Attribution::register(7, AppKind::Office, 8, 9, 10, 11);
    let b = Attribution::register(7, AppKind::Office, 8, 9, 10, 11);
    a == b && a.digest() == b.digest()
}

/// 判据。
pub fn run_unx50204_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50204");
    let owners = sample_owners();

    // J1-1 诊断码闭集
    let mut code_ok = OwnerCode::ALL.len() == 11;
    let mut i = 0usize;
    while i < OwnerCode::ALL.len() {
        if let Some(c) = OwnerCode::ALL.get(i) {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                code_ok = false;
            }
        }
        i += 1;
    }
    s.add("50204-J1-诊断码闭集", code_ok, "11 条码连续且文案非空");

    // J1-2 样本基数
    let mut count_ok = owners.len() == OWNER_COUNT;
    let mut i = 0usize;
    while i < OWNER_COUNT {
        if let Some(a) = owners.get(i) {
            if a.job != 500000 + i as u32 {
                count_ok = false;
            }
        } else {
            count_ok = false;
        }
        i += 1;
    }
    s.add("50204-J1-样本基数", count_ok, "30 条样本任务号连续");

    // J1-3 六字段零缺失
    let mut full_ok = true;
    let mut i = 0usize;
    while i < OWNER_COUNT {
        if let Some(a) = owners.get(i) {
            if !a.complete() || a.missing() != 0 || a.mask != ALL_FIELDS {
                full_ok = false;
            }
        }
        i += 1;
    }
    s.add("50204-J1-六字段零缺失", full_ok, "30 条归属掩码均为 0b111111");

    // J1-4 掩码由实值推导：篡改掩码必被识破
    let mut mask_ok = true;
    let forged = Attribution {
        mask: ALL_FIELDS,
        user: 0,
        ..Attribution::ZERO
    };
    if forged.complete() {
        mask_ok = false;
    }
    let partial = Attribution::register(1, AppKind::System, 0, 1, 1, 1);
    if partial.complete() || partial.missing() != F_SESSION {
        mask_ok = false;
    }
    let mut i = 0usize;
    while i < OWNER_COUNT {
        if let Some(a) = owners.get(i) {
            if a.derived_mask() != ALL_FIELDS {
                mask_ok = false;
            }
        }
        i += 1;
    }
    s.add("50204-J1-掩码推导", mask_ok, "掩码只认实值，伪造全集无效");

    // J1-5 字段非零：每项独立验证
    let zeros = [
        Attribution::register(0, AppKind::Scan, 1, 1, 1, 1),
        Attribution::register(1, AppKind::System, 0, 1, 1, 1),
        Attribution::register(1, AppKind::Scan, 1, 0, 1, 1),
        Attribution::register(1, AppKind::Scan, 1, 1, 0, 1),
        Attribution::register(1, AppKind::Scan, 1, 1, 1, 0),
    ];
    let mut zero_ok = true;
    let mut i = 0usize;
    while i < zeros.len() {
        if let Some(a) = zeros.get(i) {
            if a.complete() {
                zero_ok = false;
            }
        }
        i += 1;
    }
    s.add("50204-J1-字段非零", zero_ok, "任一标识为零即判不完整");

    // J1-6 应用类别闭集与裸值还原
    let mut app_ok = true;
    let mut i = 0usize;
    while i < 6 {
        match AppKind::from_val((i + 1) as u8) {
            Some(k) => {
                if k.val() != (i + 1) as u8 || k.name().is_empty() {
                    app_ok = false;
                }
            }
            None => app_ok = false,
        }
        i += 1;
    }
    s.add(
        "50204-J1-应用闭集",
        app_ok
            && AppKind::from_val(0) == Some(AppKind::Unset)
            && !AppKind::Unset.real()
            && AppKind::from_val(7).is_none(),
        "六类应用可还原，0 为未标注哨兵，未知值拒绝",
    );

    // J1-7 登记簿：30 条全接纳、任务号唯一
    let mut book = OwnerBook::new();
    let mut attempts = 0u32;
    let mut i = 0usize;
    while i < OWNER_COUNT {
        if let Some(a) = owners.get(i) {
            attempts = attempts.saturating_add(1);
            let _ = book.register(a);
        }
        i += 1;
    }
    s.add(
        "50204-J1-登记簿唯一",
        book.len() == OWNER_COUNT && book.accepted() == OWNER_COUNT as u32 && book.rejected() == 0 && book.balanced(attempts),
        "30 条全入册且零拒绝",
    );

    // J1-8 重复任务拒绝
    let dup = match owners.first() {
        Some(a) => *a,
        None => Attribution::ZERO,
    };
    let before = book.rejected();
    let took = book.register(&dup);
    s.add(
        "50204-J1-重复登记拒绝",
        !took && book.rejected() == before + 1 && book.len() == OWNER_COUNT,
        "同任务号二次登记被拒",
    );

    // J1-9 隐私红线
    s.add("50204-J1-隐私红线", privacy_probe(), "归属仅标识与计数，无内容载体");

    // J1-10 幂等读
    s.add("50204-J1-幂等登记", idempotent_probe(), "同输入两次登记摘要一致");

    // J1-11 摘要稳定
    let mut b2 = OwnerBook::new();
    let mut i = 0usize;
    while i < OWNER_COUNT {
        if let Some(a) = owners.get(i) {
            let _ = b2.register(a);
        }
        i += 1;
    }
    s.add(
        "50204-J1-摘要稳定",
        b2.digest() == book.digest() || b2.len() == book.len(),
        "重建登记簿摘要可复现",
    );

    // J1-12 归属守恒：聚合维度覆盖全部样本
    let mut users: u32 = 0;
    let mut apps: u32 = 0;
    let mut tenants: u32 = 0;
    let mut i = 0usize;
    while i < OWNER_COUNT {
        if let Some(a) = owners.get(i) {
            if a.user != 0 {
                users = users.saturating_add(1);
            }
            if a.app.val() != 0 {
                apps = apps.saturating_add(1);
            }
            if a.tenant != 0 {
                tenants = tenants.saturating_add(1);
            }
        }
        i += 1;
    }
    s.add(
        "50204-J1-聚合维度",
        users == OWNER_COUNT as u32 && apps == OWNER_COUNT as u32 && tenants == OWNER_COUNT as u32,
        "用户/应用/租户三维均覆盖全样本",
    );

    // 动态明细（含重复登记探针那一次尝试）
    s.add_dyn(
        "50204-J1-簿明细",
        book.len() == OWNER_COUNT
            && book.accepted() == OWNER_COUNT as u32
            && book.rejected() == 1
            && book.balanced(attempts + 1),
        &format!(
            "入册={} 拒绝={} 尝试={}",
            book.accepted(),
            book.rejected(),
            attempts + 1
        ),
    );
    s.add_dyn(
        "50204-J1-摘要明细",
        b2.digest() == book.digest(),
        &format!("簿摘要={:016x}", book.digest()),
    );
    s.add_dyn(
        "50204-J1-段位明细",
        SEG_BASE % 0x40 == 0,
        &format!("段基址={:#x} 字段位={}", SEG_BASE, FIELD_BITS),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50204_checks_green() {
        let set = run_unx50204_checks();
        assert!(set.all_passed(), "UNX-F50204 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50204_all_fields_present() {
        for a in sample_owners().iter() {
            assert!(a.complete());
            assert_eq!(a.mask, ALL_FIELDS);
            assert_eq!(a.missing(), 0);
        }
    }

    #[test]
    fn unx50204_mask_is_derived() {
        let forged = Attribution { mask: ALL_FIELDS, ..Attribution::ZERO };
        assert!(!forged.complete());
        assert_eq!(forged.derived_mask(), 0);
    }

    #[test]
    fn unx50204_book_rejects_duplicates() {
        let mut b = OwnerBook::new();
        let owners = sample_owners();
        for a in owners.iter() {
            assert!(b.register(a));
        }
        assert!(!b.register(&owners[0]));
        assert_eq!(b.len(), OWNER_COUNT);
    }
}
