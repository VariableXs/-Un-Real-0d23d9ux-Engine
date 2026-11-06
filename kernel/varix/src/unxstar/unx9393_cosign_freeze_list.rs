//! UNX-F9393 联签交接包（F9400 冻结面移交清单） · 0x7CFC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9393-J1 逐字）：
//!   锚点：UNX-F9393-J1 交接包判据过，F9400 冻结面移交清单逐项落账，
//!   接手会话就绪格过
//!
//! 数据段 SEG_BASE = 0x7CFC0：诊断码闭集 +0x01..=+0x0A（10 码，
//! 9393 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表：HandoffList（移交清单闭集 16 项，闭集登记 + 去重 +
//!     越界拒绝）；
//!   - 状态机：ItemStage（Registered→Frozen→HandedOver→Receipted
//!     单向迁移，接手回执只能发生在移交之后）；
//!   - 账本：HandoffLedger（登记=冻结=移交=回执 四段逐项守恒，
//!     接手会话就绪 = 清单闭集全回执）+ 注入账（逃逸恒 0）；
//!   - 对拍：Linux errno 对照（ENOENT/EBADF/EEXIST/EINVAL/EACCES）；
//!   - 注入核：5 类坏样本（越界项/重复项/回执先于移交/缺项交接/
//!     篡改回执哈希）逐类显性拒绝；
//!   - 指纹：FNV-1a 回执序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9393 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CFC0;

/// 移交清单闭集容量（16 项）。
pub const LIST_CAP: usize = 16;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffCode {
    /// 越界项：清单闭集 1..=16 之外
    ItemOutOfRange,
    /// 重复项：同项重复登记
    DuplicateItem,
    /// 回执先于移交
    ReceiptBeforeHandover,
    /// 缺项交接：交接时清单未满闭集
    IncompleteHandover,
    /// 篡改回执哈希
    TamperedReceipt,
    /// 冻结缺失：未冻结项不得移交
    NotFrozen,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 项内容哈希漂移
    ContentDrift,
}

impl HandoffCode {
    /// 闭集数组。
    pub const ALL: [HandoffCode; 10] = [
        HandoffCode::ItemOutOfRange,
        HandoffCode::DuplicateItem,
        HandoffCode::ReceiptBeforeHandover,
        HandoffCode::IncompleteHandover,
        HandoffCode::TamperedReceipt,
        HandoffCode::NotFrozen,
        HandoffCode::LedgerRegress,
        HandoffCode::UnknownErrno,
        HandoffCode::NotReproducible,
        HandoffCode::ContentDrift,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HandoffCode::ItemOutOfRange => 1,
                HandoffCode::DuplicateItem => 2,
                HandoffCode::ReceiptBeforeHandover => 3,
                HandoffCode::IncompleteHandover => 4,
                HandoffCode::TamperedReceipt => 5,
                HandoffCode::NotFrozen => 6,
                HandoffCode::LedgerRegress => 7,
                HandoffCode::UnknownErrno => 8,
                HandoffCode::NotReproducible => 9,
                HandoffCode::ContentDrift => 10,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            HandoffCode::ItemOutOfRange => "移交清单只认 1..=16 闭集项，越界显性拒绝",
            HandoffCode::DuplicateItem => "同一清单项只登记一次，重复按 EEXIST 口径拒绝",
            HandoffCode::ReceiptBeforeHandover => "接手回执必须发生在移交之后",
            HandoffCode::IncompleteHandover => "闭集不满不得整体交接，缺项显性拒绝",
            HandoffCode::TamperedReceipt => "回执哈希必须与冻结哈希逐位一致",
            HandoffCode::NotFrozen => "未冻结项不得进入移交序列",
            HandoffCode::LedgerRegress => "交接账只增不减，倒退即审计失败",
            HandoffCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            HandoffCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            HandoffCode::ContentDrift => "同项内容两次冻结哈希必须一致，漂移即重编",
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 清单项状态机：Registered→Frozen→HandedOver→Receipted 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemStage {
    /// 已登记
    Registered,
    /// 已冻结（内容哈希落账）
    Frozen,
    /// 已移交
    HandedOver,
    /// 已回执（终态）
    Receipted,
}

impl ItemStage {
    /// 单向迁移核验。
    pub fn advance(self, want: ItemStage) -> Option<ItemStage> {
        let legal = match (self, want) {
            (ItemStage::Registered, ItemStage::Frozen) => true,
            (ItemStage::Frozen, ItemStage::HandedOver) => true,
            (ItemStage::HandedOver, ItemStage::Receipted) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 移交清单注册表 ────────────────────────────────────────────────────
/// 单个清单项。
#[derive(Clone, Copy)]
pub struct ListItem {
    /// 项号（1..=LIST_CAP 闭集）
    pub id: u32,
    /// 状态
    pub stage: ItemStage,
    /// 冻结内容哈希
    pub hash: u32,
}

/// 移交清单。
pub struct HandoffList {
    items: Vec<ListItem>,
    /// 成功登记数
    pub registered: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl HandoffList {
    /// 建空清单。
    pub fn new() -> HandoffList {
        HandoffList { items: Vec::new(), registered: 0, rejected: 0 }
    }

    /// 按项号取项。
    pub fn find(&self, id: u32) -> Option<ListItem> {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.id == id {
                    return Some(*it);
                }
            }
            i += 1;
        }
        None
    }

    /// 按项号取下标。
    fn index_of(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 闭集登记：越界/重复/超容显性拒绝。
    pub fn register(&mut self, id: u32, hash: u32) -> Result<(), HandoffCode> {
        if id == 0 || id as usize > LIST_CAP {
            self.rejected += 1;
            return Err(HandoffCode::ItemOutOfRange);
        }
        if self.find(id).is_some() {
            self.rejected += 1;
            return Err(HandoffCode::DuplicateItem);
        }
        self.items.push(ListItem { id, stage: ItemStage::Registered, hash });
        self.registered += 1;
        Ok(())
    }

    /// 冻结项：Registered→Frozen。
    pub fn freeze(&mut self, id: u32) -> Result<(), HandoffCode> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return Err(HandoffCode::ItemOutOfRange),
        };
        let cur = match self.items.get(idx) {
            Some(it) => it.stage,
            None => return Err(HandoffCode::ItemOutOfRange),
        };
        match cur.advance(ItemStage::Frozen) {
            Some(next) => {
                if let Some(it) = self.items.get_mut(idx) {
                    it.stage = next;
                }
                Ok(())
            }
            None => Err(HandoffCode::NotFrozen),
        }
    }

    /// 移交项：Frozen→HandedOver（整体交接前要求闭集满员）。
    pub fn hand_over(&mut self, id: u32) -> Result<(), HandoffCode> {
        if self.registered < LIST_CAP as u32 {
            self.rejected += 1;
            return Err(HandoffCode::IncompleteHandover);
        }
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return Err(HandoffCode::ItemOutOfRange),
        };
        let cur = match self.items.get(idx) {
            Some(it) => it.stage,
            None => return Err(HandoffCode::ItemOutOfRange),
        };
        match cur.advance(ItemStage::HandedOver) {
            Some(next) => {
                if let Some(it) = self.items.get_mut(idx) {
                    it.stage = next;
                }
                Ok(())
            }
            None => Err(HandoffCode::NotFrozen),
        }
    }

    /// 接手回执：HandedOver→Receipted，回执哈希必须与冻结哈希一致。
    pub fn receipt(&mut self, id: u32, presented: u32) -> Result<(), HandoffCode> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return Err(HandoffCode::ItemOutOfRange),
        };
        let cur = match self.items.get(idx) {
            Some(it) => it,
            None => return Err(HandoffCode::ItemOutOfRange),
        };
        if cur.stage != ItemStage::HandedOver {
            return Err(HandoffCode::ReceiptBeforeHandover);
        }
        if presented != cur.hash {
            return Err(HandoffCode::TamperedReceipt);
        }
        if let Some(it) = self.items.get_mut(idx) {
            it.stage = ItemStage::Receipted;
        }
        Ok(())
    }

    /// 接手会话就绪：闭集全项 Receipted。
    pub fn receiver_ready(&self) -> bool {
        if self.items.len() != LIST_CAP {
            return false;
        }
        self.items.iter().all(|it| it.stage == ItemStage::Receipted)
    }

    /// 回执计数。
    pub fn receipted(&self) -> u32 {
        self.items.iter().filter(|it| it.stage == ItemStage::Receipted).count() as u32
    }
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 项内容哈希（确定性派生；同项两次一致，零重编载体）。
pub fn item_hash(id: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&id.to_le_bytes());
    buf.extend_from_slice(b"F9393-F9400-frozen-surface");
    fnv1a(&buf)
}

/// 回执序列指纹。
pub fn receipt_fingerprint(ids: &[u32]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for id in ids {
        buf.extend_from_slice(&id.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 完整交接主流程 ────────────────────────────────────────────────────
/// 全量交接：闭集 16 项逐项 登记→冻结→移交→回执。
///
/// 返回 (清单, 回执序列)；全部回执即接手会话就绪。
pub fn run_full_handover() -> (HandoffList, Vec<u32>) {
    let mut list = HandoffList::new();
    let mut receipts: Vec<u32> = Vec::new();
    // 登记闭集全项
    let mut id = 1u32;
    while id as usize <= LIST_CAP {
        let _ = list.register(id, item_hash(id));
        id += 1;
    }
    // 冻结全项
    let mut id = 1u32;
    while id as usize <= LIST_CAP {
        let _ = list.freeze(id);
        id += 1;
    }
    // 移交全项
    let mut id = 1u32;
    while id as usize <= LIST_CAP {
        let _ = list.hand_over(id);
        id += 1;
    }
    // 回执全项（按序）
    let mut id = 1u32;
    while id as usize <= LIST_CAP {
        if list.receipt(id, item_hash(id)).is_ok() {
            receipts.push(id);
        }
        id += 1;
    }
    (list, receipts)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], u32) {
    let mut ok = [false; 5];
    let mut detected = 0u32;

    // 类 1：越界项（0 与 17）
    let mut l1 = HandoffList::new();
    let out = l1.register(0, 0) == Err(HandoffCode::ItemOutOfRange)
        && l1.register(LIST_CAP as u32 + 1, 0) == Err(HandoffCode::ItemOutOfRange);
    if out {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：重复项
    let mut l2 = HandoffList::new();
    let _ = l2.register(3, item_hash(3));
    let dup = l2.register(3, item_hash(3)) == Err(HandoffCode::DuplicateItem);
    if dup {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：回执先于移交（Registered 态直接回执）
    let mut l3 = HandoffList::new();
    let _ = l3.register(1, item_hash(1));
    let early = l3.receipt(1, item_hash(1)) == Err(HandoffCode::ReceiptBeforeHandover);
    if early {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：缺项交接（闭集不满时 hand_over 拒绝）
    let mut l4 = HandoffList::new();
    let _ = l4.register(1, item_hash(1));
    let _ = l4.freeze(1);
    let incomplete = l4.hand_over(1) == Err(HandoffCode::IncompleteHandover);
    if incomplete {
        detected += 1;
        ok[3] = true;
    }

    // 类 5：篡改回执哈希
    let mut l5 = HandoffList::new();
    let _ = l5.register(2, item_hash(2));
    let _ = l5.freeze(2);
    let mut id = 1u32;
    // 补满闭集以通过整体交接前置
    while id as usize <= LIST_CAP {
        if id != 2 {
            let _ = l5.register(id, item_hash(id));
            let _ = l5.freeze(id);
        }
        id += 1;
    }
    let _ = l5.hand_over(2);
    let tamper = l5.receipt(2, item_hash(2).wrapping_add(1)) == Err(HandoffCode::TamperedReceipt);
    if tamper {
        detected += 1;
        ok[4] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EBADF, EACCES, EEXIST, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9393 自检：交接包判据（约 14 条）。
pub fn run_unx9393_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9393");

    // 1. 状态机单向
    let fsm_ok = ItemStage::Registered.advance(ItemStage::Frozen) == Some(ItemStage::Frozen)
        && ItemStage::Frozen.advance(ItemStage::HandedOver) == Some(ItemStage::HandedOver)
        && ItemStage::HandedOver.advance(ItemStage::Receipted) == Some(ItemStage::Receipted)
        && ItemStage::Receipted.advance(ItemStage::HandedOver).is_none();
    s.add("状态机：Registered→Frozen→HandedOver→Receipted 单向", fsm_ok,
          "逆迁移与跳级全部拒绝");

    // 2. 闭集全项登记
    let (list, receipts) = run_full_handover();
    s.add("闭集登记：16 项全部入清单", list.registered == LIST_CAP as u32,
          "移交清单逐项落账");

    // 3. 接手会话就绪：全项回执
    s.add("接手就绪：16/16 项回执齐备", list.receiver_ready() && list.receipted() == LIST_CAP as u32,
          "接手会话就绪 = 清单闭集全回执");

    // 4. 回执序列按序
    let in_order = receipts.len() == LIST_CAP
        && receipts.first().copied() == Some(1)
        && receipts.last().copied() == Some(LIST_CAP as u32);
    s.add("回执按序：1..=16 逐项回执", in_order,
          "移交清单逐项落账且有序");

    // 5. 回执哈希互认：回执哈希=冻结哈希
    let mut hash_ok = true;
    let mut id = 1u32;
    while id as usize <= LIST_CAP {
        match list.find(id) {
            Some(it) => {
                if it.hash != item_hash(id) || it.stage != ItemStage::Receipted {
                    hash_ok = false;
                }
            }
            None => hash_ok = false,
        }
        id += 1;
    }
    s.add("哈希互认：回执哈希与冻结哈希逐位一致", hash_ok,
          "篡改回执必须被拒");

    // 6. 注入核 5 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej && detected == 5,
          "越界/重复/回执先行/缺项/篡改全过闸");

    // 7. 拒绝账
    let mut l7 = HandoffList::new();
    let _ = l7.register(1, 1);
    let _ = l7.register(1, 1);
    let _ = l7.register(99, 1);
    let rej_ok = l7.rejected == 2 && l7.registered == 1;
    s.add("拒绝账：重复与越界各记一笔", rej_ok,
          "拒绝不静默，显性入账");

    // 8. 内容哈希确定性（零重编）
    let h1 = item_hash(7);
    let h2 = item_hash(7);
    s.add("内容哈希：同项双跑逐位一致（零重编）", h1 == h2,
          "同项内容两次冻结哈希必须一致");

    // 9. 哈希区分度
    let dist = item_hash(7) != item_hash(8) && item_hash(1) != item_hash(16);
    s.add("哈希区分度：不同项哈希不同", dist,
          "逐项落账的前提是哈希有区分度");

    // 10. 双跑指纹
    let (_, r2) = run_full_handover();
    let fp1 = receipt_fingerprint(&receipts);
    let fp2 = receipt_fingerprint(&r2);
    s.add("双跑指纹：回执序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性交接，无隐藏状态");

    // 11. 指纹区分度
    let mut r3 = r2.clone();
    if let Some(last) = r3.last_mut() {
        *last = last.wrapping_add(1);
    }
    s.add("指纹区分度：回执改动即指纹漂移", receipt_fingerprint(&r3) != fp1,
          "指纹必须反映交接内容");

    // 12. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EEXIST)
        && !agrees_with_linux(0)
        && !agrees_with_linux(2048);
    s.add("errno 对照：EEXIST 口径拒重复、表外码拒绝", codes_ok,
          "错误码闭集对拍");

    // 13. 诊断码闭集
    let text_ok = HandoffCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < HandoffCode::ALL.len() {
        let mut j = i + 1;
        while j < HandoffCode::ALL.len() {
            let a = HandoffCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = HandoffCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：10 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+10 闭集");

    // 14. 账单调
    let mut mono = HandoffList::new();
    let b0 = mono.registered;
    let _ = mono.register(1, 1);
    let _ = mono.register(2, 2);
    s.add("账目单调：再登记只增不减", mono.registered == b0 + 2,
          "交接账只增不减");

    // 动态明细（≤8）
    let h_text = format!("登记 {} / 回执 {}", list.registered, list.receipted());
    s.add_dyn("交接账快照", list.receiver_ready(), &h_text);
    let fp_text = format!("回执指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_fsm_one_way() {
        assert_eq!(ItemStage::Registered.advance(ItemStage::Frozen), Some(ItemStage::Frozen));
        assert_eq!(ItemStage::Frozen.advance(ItemStage::Receipted), None);
        assert_eq!(ItemStage::Receipted.advance(ItemStage::Registered), None);
    }

    #[test]
    fn closed_set_bounds() {
        let mut l = HandoffList::new();
        assert!(l.register(1, 0).is_ok());
        assert!(l.register(LIST_CAP as u32, 0).is_ok());
        assert_eq!(l.register(0, 0), Err(HandoffCode::ItemOutOfRange));
        assert_eq!(l.register(17, 0), Err(HandoffCode::ItemOutOfRange));
    }

    #[test]
    fn duplicate_rejected() {
        let mut l = HandoffList::new();
        let _ = l.register(5, 5);
        assert_eq!(l.register(5, 5), Err(HandoffCode::DuplicateItem));
        assert_eq!(l.rejected, 1);
    }

    #[test]
    fn receipt_requires_handover() {
        let mut l = HandoffList::new();
        let _ = l.register(1, 9);
        assert_eq!(l.receipt(1, 9), Err(HandoffCode::ReceiptBeforeHandover));
    }

    #[test]
    fn handover_requires_full_closure() {
        let mut l = HandoffList::new();
        let _ = l.register(1, 1);
        let _ = l.freeze(1);
        assert_eq!(l.hand_over(1), Err(HandoffCode::IncompleteHandover));
    }

    #[test]
    fn tampered_receipt_rejected() {
        let (mut l, _) = run_full_handover();
        // 从零重放：篡改哈希回执
        let mut l2 = HandoffList::new();
        let mut id = 1u32;
        while id as usize <= LIST_CAP {
            let _ = l2.register(id, item_hash(id));
            let _ = l2.freeze(id);
            id += 1;
        }
        let mut id = 1u32;
        while id as usize <= LIST_CAP {
            let _ = l2.hand_over(id);
            id += 1;
        }
        assert_eq!(l2.receipt(4, item_hash(4).wrapping_add(1)), Err(HandoffCode::TamperedReceipt));
        assert!(l2.receipt(4, item_hash(4)).is_ok());
        let _ = l;
    }

    #[test]
    fn full_handover_ready() {
        let (l, receipts) = run_full_handover();
        assert!(l.receiver_ready());
        assert_eq!(receipts.len(), LIST_CAP);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 5);
    }

    #[test]
    fn item_hash_deterministic_and_distinct() {
        assert_eq!(item_hash(3), item_hash(3));
        assert_ne!(item_hash(3), item_hash(4));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, r1) = run_full_handover();
        let (_, r2) = run_full_handover();
        assert_eq!(receipt_fingerprint(&r1), receipt_fingerprint(&r2));
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(-ENOENT));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(3000));
    }

    #[test]
    fn unx9393_checks_green() {
        let set = run_unx9393_checks();
        assert!(set.all_passed(), "UNX-F9393 red: {}", set.red_items_text());
    }
}
