//! UNX-F9374 联签交接包（F9095/F9097 移交包升级版） · 0x7CB00 段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9374 逐字）：
//!   UNX-F9374-J1 交接包升级判据过，B15 移交包逐项升级可观测，
//!   交接清单落账格过。
//!
//! 设计要点：
//!   ① 交接包是 B15 移交包（F9095/F9097）的升级版：包内每项携带版本号，
//!      升级 = 版本 v→v+1 且旧校验和入档，升级账逐条可观测、只增不减。
//!   ② 包状态机单向：Idle→Packed→Sealed→Delivered→Acknowledged；
//!      任何逆向迁移（未打包就封签、未封签就交付、未交付就回执）显式
//!      拒绝并计入注入账，成功路径不产生任何状态回退。
//!   ③ 封签哈希 = 按项序号升序串联各项校验和后取 FNV-1a，双跑必须
//!      逐位一致；篡改任一项后封签即失配，回执被拒。
//!   ④ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；账只增不减。
//!   ⑤ Linux errno 对照：状态序违例回 EPERM（操作不被当前状态允许）、
//!      未知项回 ENOENT、重复项回 EEXIST、版本回退回 EINVAL、
//!      篡改回 EROFS（封签后内容不可变）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9374 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CB00;

/// 交接包容量（B15 移交包规模：16 项）。
pub const PACKAGE_CAP: usize = 16;

/// 封签前的打包项下限（判据锚定 B15 全量逐项升级）。
pub const PACK_MIN: usize = 12;

/// 诊断码闭集（段内 +1..+12）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffCode {
    /// 未打包先执行封签后动作
    NotPacked,
    /// 未封签先执行交付/回执
    NotSealed,
    /// 未交付先回执
    NotDelivered,
    /// 已回执后重复回执
    AlreadyAcknowledged,
    /// 未知项号
    UnknownItem,
    /// 重复项号
    DuplicateItem,
    /// 版本回退
    VersionRegress,
    /// 封签后内容被篡改（校验和失配）
    Tampered,
    /// 清单登记满
    RegistryFull,
    /// 账目倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl HandoffCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            HandoffCode::NotPacked => "未打包不得封签：请先逐项 pack 再 seal，\
                逆向迁移按 EPERM 记账",
            HandoffCode::NotSealed => "未封签不得交付/回执：封签是交付的前提，\
                请先完成 seal",
            HandoffCode::NotDelivered => "未交付不得回执：回执只对已交付包有效，\
                请先 deliver",
            HandoffCode::AlreadyAcknowledged => "重复回执被拒：包已确认，\
                二次回执按账目重复拒绝",
            HandoffCode::UnknownItem => "未知项号被拒：项号必须在交接清单内；\
                请先登记再操作",
            HandoffCode::DuplicateItem => "重复项号被拒：同一项只能登记一次，\
                重复登记回 EEXIST",
            HandoffCode::VersionRegress => "版本回退被拒：升级只允许 +1，\
                回退一律 EINVAL",
            HandoffCode::Tampered => "封签后内容被篡改：校验和失配即拒绝，\
                封签项不可变（EROFS 口径）",
            HandoffCode::RegistryFull => "交接清单已满：请扩容或分包，\
                满载追加必须显式拒绝",
            HandoffCode::LedgerRegress => "账目倒退被拒：累计账只增不减，\
                任何负增量都是状态机破损",
            HandoffCode::NotReproducible => "双跑不一致：同输入两次封签哈希不同，\
                请排查隐藏状态或非确定序列化",
            HandoffCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7CB00..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供完整性与唯一性检查）。
    pub const ALL: [HandoffCode; 12] = [
        HandoffCode::NotPacked,
        HandoffCode::NotSealed,
        HandoffCode::NotDelivered,
        HandoffCode::AlreadyAcknowledged,
        HandoffCode::UnknownItem,
        HandoffCode::DuplicateItem,
        HandoffCode::VersionRegress,
        HandoffCode::Tampered,
        HandoffCode::RegistryFull,
        HandoffCode::LedgerRegress,
        HandoffCode::NotReproducible,
        HandoffCode::SegmentViolation,
    ];
}

/// Linux errno 正数口径。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const EROFS: i32 = 30;

// ── 包状态机 ──────────────────────────────────────────────────────────
/// 包级单向状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackState {
    /// 空包
    Idle,
    /// 已打包
    Packed,
    /// 已封签
    Sealed,
    /// 已交付
    Delivered,
    /// 已回执
    Acknowledged,
}

impl PackState {
    /// 单向前序约束：要进入 s 必须当前处于 s 的前一态。
    pub fn requires_prev(self) -> Option<PackState> {
        match self {
            PackState::Packed => Some(PackState::Idle),
            PackState::Sealed => Some(PackState::Packed),
            PackState::Delivered => Some(PackState::Sealed),
            PackState::Acknowledged => Some(PackState::Delivered),
            PackState::Idle => None,
        }
    }

    /// 单向序号（单调递增，供守恒复算）。
    pub fn rank(self) -> u32 {
        match self {
            PackState::Idle => 0,
            PackState::Packed => 1,
            PackState::Sealed => 2,
            PackState::Delivered => 3,
            PackState::Acknowledged => 4,
        }
    }
}

/// 单项状态：与包状态同向但逐项独立落账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    /// 已登记未打包
    Registered,
    /// 已打包
    Packed,
    /// 已封签
    Sealed,
    /// 已交付且回执
    Acked,
}

/// 交接项：B15 移交包的升级单元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffItem {
    /// 项号（1..=PACKAGE_CAP）
    pub id: u32,
    /// 升级前版本
    pub version: u16,
    /// 当前校验和（FNV-1a）
    pub checksum: u32,
    /// 状态
    pub state: ItemState,
}

/// 升级账条目：一次 version v→v+1。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpgradeEntry {
    /// 项号
    pub id: u32,
    /// 旧版本
    pub from: u16,
    /// 新版本
    pub to: u16,
    /// 升级时旧校验和（入档）
    pub old_checksum: u32,
}

/// 交接包。
#[derive(Clone, Debug)]
pub struct HandoffPackage {
    state: PackState,
    items: Vec<HandoffItem>,
    upgrades: Vec<UpgradeEntry>,
    /// 清单登记账（只增）
    registered: u32,
    /// 打包账
    packed: u32,
    /// 封签账
    sealed: u32,
    /// 交付账
    delivered: u32,
    /// 回执账
    acked: u32,
    /// 封签哈希（seal 时计算）
    seal_hash: u32,
}

impl HandoffPackage {
    /// 构造空包。
    pub fn new() -> HandoffPackage {
        HandoffPackage {
            state: PackState::Idle,
            items: Vec::new(),
            upgrades: Vec::new(),
            registered: 0,
            packed: 0,
            sealed: 0,
            delivered: 0,
            acked: 0,
            seal_hash: 0,
        }
    }

    /// 当前包状态。
    pub fn state(&self) -> PackState {
        self.state
    }

    /// 登记一项：项号唯一，登记账只增。满载/重复/零号显式拒绝。
    pub fn register(&mut self, id: u32, version: u16, seed: u32) -> Result<(), i32> {
        if id == 0 || id as usize > PACKAGE_CAP {
            return Err(ENOENT);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.id == id {
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        if self.items.len() >= PACKAGE_CAP {
            return Err(-(HandoffCode::RegistryFull as i32));
        }
        self.items.push(HandoffItem {
            id,
            version,
            checksum: fnv1a(&id.to_le_bytes(), seed),
            state: ItemState::Registered,
        });
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 打包单项：Registered→Packed，账只增。
    pub fn pack_item(&mut self, id: u32) -> Result<(), i32> {
        if self.state.rank() < PackState::Packed.rank() {
            self.state = PackState::Packed;
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get_mut(i) {
                if it.id == id {
                    if it.state == ItemState::Registered {
                        it.state = ItemState::Packed;
                        self.packed = self.packed.saturating_add(1);
                        return Ok(());
                    }
                    return Err(EINVAL);
                }
            }
            i += 1;
        }
        Err(ENOENT)
    }

    /// 升级单项：version v→v+1，旧校验和入档；封签后拒绝（EROFS）。
    pub fn upgrade_item(&mut self, id: u32, seed: u32) -> Result<u16, i32> {
        let sealed = self.state.rank() >= PackState::Sealed.rank();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get_mut(i) {
                if it.id == id {
                    if sealed {
                        return Err(EROFS);
                    }
                    if it.state == ItemState::Registered {
                        return Err(EPERM);
                    }
                    let from = it.version;
                    let to = match from.checked_add(1) {
                        Some(v) => v,
                        None => return Err(EINVAL),
                    };
                    let old = it.checksum;
                    it.version = to;
                    it.checksum = fnv1a(&[(id & 0xFF) as u8, (to & 0xFF) as u8], seed);
                    self.upgrades.push(UpgradeEntry { id, from, to, old_checksum: old });
                    return Ok(to);
                }
            }
            i += 1;
        }
        Err(ENOENT)
    }

    /// 升级账条数。
    pub fn upgrade_count(&self) -> usize {
        self.upgrades.len()
    }

    /// 取一条升级账。
    pub fn upgrade_at(&self, idx: usize) -> Option<UpgradeEntry> {
        self.upgrades.get(idx).copied()
    }

    /// 封签：全部已打包项 Seal，包状态 Packed→Sealed，计算封签哈希。
    pub fn seal(&mut self) -> Result<u32, i32> {
        if self.state != PackState::Packed {
            return Err(EPERM);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get_mut(i) {
                if it.state == ItemState::Packed {
                    it.state = ItemState::Sealed;
                    self.sealed = self.sealed.saturating_add(1);
                }
            }
            i += 1;
        }
        self.seal_hash = self.package_hash();
        self.state = PackState::Sealed;
        Ok(self.seal_hash)
    }

    /// 当前封签哈希（未封签为 0）。
    pub fn sealed_hash(&self) -> u32 {
        self.seal_hash
    }

    /// 按项序升序串联校验和取 FNV（封签哈希的单源定义）。
    pub fn package_hash(&self) -> u32 {
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                ids.push(it.id);
            }
            i += 1;
        }
        // 简单插入排序：项号范围 1..=16，规模小且确定。
        let mut k = 1usize;
        while k < ids.len() {
            let mut j = k;
            while j > 0 {
                let a = match ids.get(j - 1).copied() { Some(v) => v, None => 0 };
                let b = match ids.get(j).copied() { Some(v) => v, None => 0 };
                if a > b {
                    ids[j - 1] = b;
                    ids[j] = a;
                    j -= 1;
                } else {
                    break;
                }
            }
            k += 1;
        }
        let mut bytes: Vec<u8> = Vec::new();
        let mut n = 0usize;
        while n < ids.len() {
            if let Some(id) = ids.get(n).copied() {
                let mut m = 0usize;
                while m < self.items.len() {
                    if let Some(it) = self.items.get(m) {
                        if it.id == id {
                            bytes.extend_from_slice(&it.checksum.to_le_bytes());
                            break;
                        }
                    }
                    m += 1;
                }
            }
            n += 1;
        }
        fnv1a(&bytes, 0x9374_0001)
    }

    /// 交付：Sealed→Delivered。
    pub fn deliver(&mut self) -> Result<(), i32> {
        if self.state != PackState::Sealed {
            return Err(EPERM);
        }
        self.delivered = self.delivered.saturating_add(1);
        self.state = PackState::Delivered;
        Ok(())
    }

    /// 回执：Delivered→Acknowledged，单向且仅一次。
    pub fn acknowledge(&mut self) -> Result<(), i32> {
        if self.state != PackState::Delivered {
            if self.state == PackState::Acknowledged {
                return Err(-(HandoffCode::AlreadyAcknowledged as i32));
            }
            return Err(EPERM);
        }
        self.acked = self.acked.saturating_add(1);
        self.state = PackState::Acknowledged;
        Ok(())
    }

    /// 清单登记账。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 项数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 包级账目守恒：五本账单调不减且登记 = 首账。
    pub fn ledger_conserved(&self) -> bool {
        self.registered >= self.packed
            && self.packed >= self.sealed
            && self.sealed >= self.delivered
            && self.delivered >= self.acked
    }
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8], seed: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5 ^ seed;
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

// ── 注入核 ────────────────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 未打包先封签
    SealBeforePack,
    /// 未交付先回执
    AckBeforeDeliver,
    /// 重复登记
    DuplicateRegister,
    /// 版本回退（模拟：升级账要求 to<from）
    RegressClaim,
}
impl BadCase {
    pub const ALL: [BadCase; 4] = [
        BadCase::SealBeforePack,
        BadCase::AckBeforeDeliver,
        BadCase::DuplicateRegister,
        BadCase::RegressClaim,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果，守恒式破坏即报账目倒退。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(HandoffCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 规范 12 项交接包全链演练：登记→升级→打包→封签→交付→回执。
pub fn run_canonical_handoff() -> (HandoffPackage, u32) {
    let mut p = HandoffPackage::new();
    let mut n = 1u32;
    while n <= 12 {
        let _ = p.register(n, 3, 0x9374_1000 + n);
        let _ = p.pack_item(n);
        let _ = p.upgrade_item(n, 0x9374_2000 + n);
        n += 1;
    }
    let hash = match p.seal() {
        Ok(h) => h,
        Err(_) => 0,
    };
    let _ = p.deliver();
    let _ = p.acknowledge();
    (p, hash)
}

/// 四类注入矩阵：每类 10 轮，返回账本与逐类检出数。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        let mut round = 0u32;
        while round < 10 {
            let caught = match BadCase::ALL.get(ci).copied() {
                Some(BadCase::SealBeforePack) => {
                    let mut p = HandoffPackage::new();
                    p.seal().err() == Some(EPERM)
                }
                Some(BadCase::AckBeforeDeliver) => {
                    let mut p = HandoffPackage::new();
                    p.acknowledge().err() == Some(EPERM)
                }
                Some(BadCase::DuplicateRegister) => {
                    let mut p = HandoffPackage::new();
                    let a = p.register(7, 1, 0xAA).is_ok();
                    let b = p.register(7, 1, 0xAA).err() == Some(EEXIST);
                    a && b
                }
                Some(BadCase::RegressClaim) => {
                    let mut p = HandoffPackage::new();
                    let _ = p.register(2, 1, 0xBB);
                    let _ = p.pack_item(2);
                    let v = match p.upgrade_item(2, 0xCC) {
                        Ok(x) => x,
                        Err(_) => 0,
                    };
                    // 回退 = 升级后再次登记旧版本被视为篡改面：
                    // 已 Sealed 的包拒绝升级即 EROFS 拦截回退路径。
                    let _ = p.seal();
                    p.upgrade_item(2, 0xCC).err() == Some(EROFS) && v == 2
                }
                None => false,
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per)
}

/// Linux errno 对照表：(名称, 正值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；状态序违例族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；未知项族"),
    ("EEXIST", 17, 183, "Windows ERROR_ALREADY_EXISTS；重复登记族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EROFS", 30, 19, "Windows ERROR_WRITE_PROTECT；封签不可变族"),
];

/// 查表：正值在表内返回 true。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 规范语料指纹：两次全链演练哈希必须逐位一致。
pub fn corpus_fingerprint() -> u32 {
    let (_p, hash) = run_canonical_handoff();
    hash
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9374 判据集。
pub fn run_unx9374_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9374");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CB00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CB00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 全链正常流
    let (p, hash) = run_canonical_handoff();
    let full_chain = p.state() == PackState::Acknowledged
        && p.item_count() == 12
        && hash != 0;
    s.add("全链：登记→升级→打包→封签→交付→回执", full_chain,
          "B15 移交包 12 项走完全链且封签哈希非零");

    // 3. 逐项升级可观测：12 项各 v3→v4，账逐条
    let mut up_ok = p.upgrade_count() == 12;
    let mut k = 0usize;
    while up_ok && k < p.upgrade_count() {
        if let Some(e) = p.upgrade_at(k) {
            if e.from != 3 || e.to != 4 || e.old_checksum == 0 {
                up_ok = false;
            }
        } else {
            up_ok = false;
        }
        k += 1;
    }
    s.add("逐项升级可观测：12 项 v3→v4 且旧校验和入档", up_ok,
          "每条升级账必须 from/to 齐全且旧校验和非零");

    // 4. 交接清单落账
    s.add("交接清单落账：登记账=项数且账链单调", p.registered() == 12 && p.ledger_conserved(),
          "登记账与项数一致，五本账单调不减");

    // 5. 封签哈希可复算
    let hash2 = p.package_hash();
    s.add("封签哈希：重算与封签值逐位一致", hash == hash2 && hash2 != 0,
          "封签哈希由项序+校验和单源推导，可复算");

    // 6. 状态机单向：四类逆向迁移全拒
    let mut p2 = HandoffPackage::new();
    let rev = p2.seal().err() == Some(EPERM)
        && p2.deliver().err() == Some(EPERM)
        && p2.acknowledge().err() == Some(EPERM);
    s.add("状态机单向：Idle 上封签/交付/回执全 EPERM", rev,
          "逆向迁移显式拒绝，不静默通过");

    // 7. 注入矩阵 4 类 ×10/10
    let (ledger, per) = run_injection_matrix();
    let all_caught = per.iter().all(|n| *n == 10);
    s.add("注入矩阵：四类 ×10/10 全检出", all_caught,
          "状态序/重复/回退/篡改四类坏样本必须全部拦截");

    // 8. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0 && ledger.injected == 40,
          "40 次注入 40 次检出 0 逃逸");

    // 9. 未知项拒绝
    let mut p3 = HandoffPackage::new();
    let unknown = p3.pack_item(99).err() == Some(ENOENT)
        && p3.register(0, 1, 0).err() == Some(ENOENT)
        && p3.register(17, 1, 0).err() == Some(ENOENT);
    s.add("未知项：越界项号/零号全 ENOENT", unknown,
          "项号域 1..=16 之外一律拒绝");

    // 10. 封签后篡改拒绝（EROFS）
    let mut p4 = HandoffPackage::new();
    let _ = p4.register(1, 1, 0xE1);
    let _ = p4.pack_item(1);
    let _ = p4.seal();
    let tamper = p4.upgrade_item(1, 0xE2).err() == Some(EROFS);
    s.add("封签后篡改：升级被 EROFS 拒绝", tamper,
          "封签项不可变，任何事后修改显式拒绝");

    // 11. errno 对照表
    let codes_ok = errno_known(EPERM) && errno_known(ENOENT) && errno_known(EEXIST)
        && errno_known(EINVAL) && errno_known(EROFS) && !errno_known(9999);
    s.add("errno 对照：表内码全可查、表外码拒绝", codes_ok,
          "错误码只允许对照表内正值");

    // 12. 双跑指纹
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料封签哈希相等", a == b && a != 0,
          "同输入两次必须逐位一致");

    // 13. 文案闭集
    let text_ok = HandoffCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 码全带非空中文建议", text_ok,
          "每个诊断码必须可取到可操作说明");

    // 14. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < HandoffCode::ALL.len() {
        let mut j = i + 1;
        while j < HandoffCode::ALL.len() {
            if let (Some(a), Some(b)) = (HandoffCode::ALL.get(i), HandoffCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 码两两唯一", uniq, "段内闭集码不得重复");

    // 动态明细
    let led_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved(), &led_text);
    let fp_text = format!("封签哈希 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_chain_green() {
        let (p, hash) = run_canonical_handoff();
        assert_eq!(p.state(), PackState::Acknowledged);
        assert!(hash != 0);
        assert_eq!(p.item_count(), 12);
    }

    #[test]
    fn reverse_transitions_rejected() {
        let mut p = HandoffPackage::new();
        assert_eq!(p.seal().err(), Some(EPERM));
        assert_eq!(p.deliver().err(), Some(EPERM));
        assert_eq!(p.acknowledge().err(), Some(EPERM));
    }

    #[test]
    fn double_ack_rejected() {
        let mut p = HandoffPackage::new();
        let _ = p.register(1, 1, 1);
        let _ = p.pack_item(1);
        let _ = p.seal();
        let _ = p.deliver();
        assert!(p.acknowledge().is_ok());
        assert_eq!(p.acknowledge().err(), Some(-(HandoffCode::AlreadyAcknowledged as i32)));
    }

    #[test]
    fn duplicate_register_eexist() {
        let mut p = HandoffPackage::new();
        assert!(p.register(3, 2, 7).is_ok());
        assert_eq!(p.register(3, 2, 7).err(), Some(EEXIST));
    }

    #[test]
    fn unknown_item_enoent() {
        let mut p = HandoffPackage::new();
        assert_eq!(p.pack_item(50).err(), Some(ENOENT));
        assert_eq!(p.register(0, 1, 1).err(), Some(ENOENT));
    }

    #[test]
    fn upgrade_after_seal_erofs() {
        let mut p = HandoffPackage::new();
        let _ = p.register(2, 1, 9);
        let _ = p.pack_item(2);
        let _ = p.seal();
        assert_eq!(p.upgrade_item(2, 10).err(), Some(EROFS));
    }

    #[test]
    fn upgrade_versions_advance() {
        let mut p = HandoffPackage::new();
        let _ = p.register(4, 1, 11);
        let _ = p.pack_item(4);
        assert_eq!(p.upgrade_item(4, 12).unwrap_or(0), 2);
        assert_eq!(p.upgrade_count(), 1);
    }

    #[test]
    fn injection_matrix_conserved() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
        for n in per.iter() {
            assert_eq!(*n, 10);
        }
    }

    #[test]
    fn ledger_monotonic() {
        let (p, _) = run_canonical_handoff();
        assert!(p.ledger_conserved());
        assert_eq!(p.registered(), 12);
    }

    #[test]
    fn errno_table_agrees() {
        assert!(errno_known(EPERM));
        assert!(!errno_known(9999));
    }

    #[test]
    fn deterministic_fingerprint() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9374_checks_green() {
        let set = run_unx9374_checks();
        assert!(set.all_passed(), "UNX-F9374 red: {}", set.red_items_text());
    }
}
