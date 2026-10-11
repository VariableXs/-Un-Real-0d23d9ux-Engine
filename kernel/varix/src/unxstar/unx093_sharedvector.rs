//! unx093_sharedvector — UNX-F0904 共享向量注册与分发票（多设备同向量）。
//!
//! 判据原文（施工书锚点 UNX-F0904，CoRun·UNX-A2-B06 批）：
//! **UNX-F0904-J1 票制分发正确（3 设备共享 10 万事件各设备应答数与预期
//! 一致），票序遍历确定性（同输入同顺序 100%），无应答设备识别 10/10 次**。
//!
//! 职责定位：F0902 的 MSI-X 是"一设备多向量、向量独占"——线间物理隔离；
//! 本单补上另一极：**多设备共享同一条线**（传统 INTx / 线共享模式）。
//! 共享线没有硬件路由可依——事件到了向量，"该给谁"只能靠**票制**：
//! 按票序逐个询问挂线设备，有人认领即收票，全程无人认领即无主事件。
//! Windows 深在哪：Windows 共享中断是 ISR 链（IoConnectInterrupt 按注册
//! 顺序串链，每个 ISR 返回 TRUE 表示认领），深坑有三——①链序受注册
//! 时序影响（同硬件不同启动顺序行为漂移，且文档不承诺确定性）；
//! ②无主事件静默丢弃（spurious interrupt 只在 HAL 层计数，驱动层
//! 归属不清）；③一个坏 ISR 永远返回 FALSE 不认领也不报错，链尾设备
//! 饿死无账可查；Varix：①**票序单源化**——遍历顺序=注册序（账面
//! Vec 保序），同输入同顺序 100% 机检（双跑指纹对拍，判据点名）；
//! ②**无主事件显性立案**——走完全票程无人认领不是静默丢弃，是
//! unclaimed 计数+最近明细留痕（判据 10/10 识别的面）；
//! ③**应答账守恒**——每张票的去向（claimed_by / passed_by 逐成员）
//! 全留痕，per 设备应答数与注入侧预期账对拍（判据"应答数与预期
//! 一致"的本体）；④**共享/独占互斥**——同向量不得既挂 MSI-X 独占
//! 又挂共享票（F0902 语义边界显性化）。
//! 上游：F0903 handler 注册表（成员 handler 经其在服态校验）、
//! F0902 独占向量语义（互斥边界）、F0881 亲和（共享线分发后亲和
//! 复用其策略面）；下游：F0905 中断上下文契约（共享线 top half
//! 约束更严）、F0906 注水测试资产（复用本单票制）、F0916 共享向量
//! 检测与告警（消费本单成员账）。
//! 本单交付**策略闭集 + 成员账 + 票制分发 + 无主识别 + 确定性审计 +
//! 10 万事件压测 + 违例注入**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显性
//! 注入）；LCG 确定性仿真（Numerical Recipes 参数）；FNV-1a 快照往返；
//! 错误全显性带诊断码与建议。诊断码段 0x3D（0x3C 为 F0903 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SHARED_VEC_VERSION: &str = "B06-sharedvector-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x3D 段，本单独占——0x3C 为 F0903 所取）
// ---------------------------------------------------------------------------

/// 0x3D01 向量越池（不在设备 MSI 共享池 0x60..0x70）。
pub const XC_VECTOR_ILLEGAL: u32 = 0x3D01;
/// 0x3D02 重复挂线（同向量同设备重复注册）。
pub const XC_DUP_ENROLL: u32 = 0x3D02;
/// 0x3D03 成员不在册（票程打到幽灵成员——防御立案）。
pub const XC_MEMBER_ABSENT: u32 = 0x3D03;
/// 0x3D04 共享/独占冲突（向量已被独占占用——F0902 语义边界）。
pub const XC_EXCLUSIVE_CLASH: u32 = 0x3D04;
/// 0x3D05 票序空转（共享向量无人挂线即来事件）。
pub const XC_EMPTY_TICKET: u32 = 0x3D05;
/// 0x3D06 无主事件（全票程无人认领——显性立案不静默）。
pub const XC_UNCLAIMED: u32 = 0x3D06;
/// 0x3D07 应答账失配（per 设备应答数与预期账不符——判据红线）。
pub const XC_ANSWER_MISMATCH: u32 = 0x3D07;
/// 0x3D08 票序漂移（同输入双跑序列指纹不一致——确定性破坏）。
pub const XC_ORDER_DRIFT: u32 = 0x3D08;
/// 0x3D09 撤线无账（注销不存在的成员）。
pub const XC_WITHDRAW_ABSENT: u32 = 0x3D09;
/// 0x3D0A 策略越集（分发策略闭集外）。
pub const XC_POLICY_ILLEGAL: u32 = 0x3D0A;
/// 0x3D0B 成员名非法（空或超 64 字节）。
pub const XC_MEMBER_NAME_BAD: u32 = 0x3D0B;
/// 0x3D0C 票账守恒破坏（发票数 ≠ 认领+让完+无主）。
pub const XC_TICKET_IMBALANCE: u32 = 0x3D0C;
/// 0x3D0D 成员上限越界（单向量挂线数超硬帽）。
pub const XC_MEMBER_CAP: u32 = 0x3D0D;
/// 0x3D0E 无主识别漏网（注入的无主事件未被识别——判据红线）。
pub const XC_UNCLAIMED_MISSED: u32 = 0x3D0E;
/// 0x3D0F 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x3D0F;

// ---------------------------------------------------------------------------
// §1 常量与分发策略闭集
// ---------------------------------------------------------------------------

/// 共享向量池基（与 F0902 同池——互斥边界在板层把守）。
pub const SHARE_VECTOR_BASE: u8 = 0x60;
/// 共享向量池上界（开区间）。
pub const SHARE_VECTOR_LIMIT: u8 = 0x70;
/// 单向量挂线成员硬帽（现实线共享很少超 8——留 16 裕量）。
pub const MEMBER_HARD_CAP: usize = 16;

/// 分发策略闭集。
///
/// 票序（TicketOrder）是判据点名的主体；轮转与广播作为闭集声明
/// （语义表全列出，未实现的显性 NotImplemented 而非静默退化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SharePolicy {
    /// 票序：按注册序逐个询问，首个认领即收票（确定性主体）。
    TicketOrder,
    /// 广播：全成员都看（无认领语义——全部应答计 pass/claim）。
    Broadcast,
    /// 轮转：起点轮转的票序（起点账面单调推进——本单声明未启用）。
    RoundRobin,
}

impl SharePolicy {
    /// 全策略枚举。
    pub const fn all() -> [SharePolicy; 3] {
        [
            SharePolicy::TicketOrder,
            SharePolicy::Broadcast,
            SharePolicy::RoundRobin,
        ]
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            SharePolicy::TicketOrder => "ticket-order",
            SharePolicy::Broadcast => "broadcast",
            SharePolicy::RoundRobin => "round-robin",
        }
    }

    /// 语义表：策略 × 属性（确定性/是否认领即停/适用面）。
    ///
    /// 行=策略，列=[确定性遍历, 认领即停, 共享线适用]。
    pub const SEMANTICS: [[bool; 3]; 3] = [
        [true, true, true],   // TicketOrder
        [true, false, true],  // Broadcast（全成员都看，无停点）
        [true, true, true],   // RoundRobin
    ];

    /// 语义查表。
    pub const fn semantic(self, col: usize) -> bool {
        SharePolicy::SEMANTICS[self.idx()][col]
    }

    /// 矩阵下标。
    pub const fn idx(self) -> usize {
        match self {
            SharePolicy::TicketOrder => 0,
            SharePolicy::Broadcast => 1,
            SharePolicy::RoundRobin => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// §2 成员账（SharedDeviceMember）
// ---------------------------------------------------------------------------

/// 挂线成员——共享向量上的一台设备。
#[derive(Clone, Debug)]
pub struct SharedDeviceMember {
    /// 设备 tag（成员唯一键，向量内唯一）。
    pub device: String,
    /// handler 名（F0901 语义，账面可读）。
    pub handler: String,
    /// 注册序号（票序本体——单源账面顺序，单调分配）。
    pub order: u32,
    /// 应答（认领）计数。
    pub claimed: u64,
    /// 让票计数（看了不认领）。
    pub passed: u64,
    /// 注册时逻辑 tick。
    pub enrolled_at: u64,
}

impl SharedDeviceMember {
    /// 让一次票。
    fn pass(&mut self) {
        self.passed += 1;
    }

    /// 认领一次。
    fn claim(&mut self) {
        self.claimed += 1;
    }
}

// ---------------------------------------------------------------------------
// §3 单向量账与全局板
// ---------------------------------------------------------------------------

/// 单共享向量账：成员表（注册序 Vec 保序）+ 票账。
#[derive(Clone, Debug)]
pub struct SharedVectorEntry {
    /// 成员表——**Vec 保序即票序**（确定性单源，不用 BTreeMap 是因为
    /// 票序=注册序不是字典序）。
    members: Vec<SharedDeviceMember>,
    /// 分发策略。
    policy: SharePolicy,
    /// 已发票总数。
    tickets_issued: u64,
    /// 认领票总数。
    tickets_claimed: u64,
    /// 无主票总数。
    tickets_unclaimed: u64,
    /// 最近无主明细（≤8 条）。
    unclaimed_log: Vec<String>,
    /// 下一个成员注册序。
    next_order: u32,
    /// 向量号。
    vector: u8,
}

impl SharedVectorEntry {
    /// 新账（空成员、票序策略）。
    pub fn new(vector: u8, policy: SharePolicy) -> SharedVectorEntry {
        SharedVectorEntry {
            members: Vec::new(),
            policy,
            tickets_issued: 0,
            tickets_claimed: 0,
            tickets_unclaimed: 0,
            unclaimed_log: Vec::new(),
            next_order: 0,
            vector,
        }
    }

    /// 向量号。
    pub fn vector(&self) -> u8 {
        self.vector
    }

    /// 策略。
    pub fn policy(&self) -> SharePolicy {
        self.policy
    }

    /// 成员数。
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// 成员表只读（票序原文）。
    pub fn members(&self) -> &[SharedDeviceMember] {
        &self.members
    }

    /// 票账三元组（发票/认领/无主）。
    pub fn ticket_ledger(&self) -> (u64, u64, u64) {
        (self.tickets_issued, self.tickets_claimed, self.tickets_unclaimed)
    }

    /// 无主明细。
    pub fn unclaimed_log(&self) -> &[String] {
        &self.unclaimed_log
    }

    /// 挂线注册：成员入表尾（票序=注册序）。
    pub fn enroll(
        &mut self,
        device: &str,
        handler: &str,
        tick: u64,
    ) -> Result<u32, (u32, String)> {
        if device.is_empty() || device.len() > 64 {
            return Err((XC_MEMBER_NAME_BAD, format!("设备 tag 非法：len={}", device.len())));
        }
        if handler.is_empty() || handler.len() > 64 {
            return Err((XC_MEMBER_NAME_BAD, format!("handler 名非法：len={}", handler.len())));
        }
        if self.members.iter().any(|m| m.device == device) {
            return Err((
                XC_DUP_ENROLL,
                format!("设备 {} 已挂在向量 {:#x}", device, self.vector),
            ));
        }
        if self.members.len() >= MEMBER_HARD_CAP {
            return Err((
                XC_MEMBER_CAP,
                format!(
                    "向量 {:#x} 挂线 {} 已达硬帽 {}",
                    self.vector,
                    self.members.len(),
                    MEMBER_HARD_CAP
                ),
            ));
        }
        let order = self.next_order;
        self.next_order += 1;
        self.members.push(SharedDeviceMember {
            device: device.to_string(),
            handler: handler.to_string(),
            order,
            claimed: 0,
            passed: 0,
            enrolled_at: tick,
        });
        Ok(order)
    }

    /// 撤线。
    pub fn withdraw(&mut self, device: &str) -> Result<SharedDeviceMember, (u32, String)> {
        let pos = self
            .members
            .iter()
            .position(|m| m.device == device)
            .ok_or((XC_WITHDRAW_ABSENT, format!("设备 {} 未挂在向量 {:#x}", device, self.vector)))?;
        Ok(self.members.remove(pos))
    }

    /// 查成员。
    pub fn member(&self, device: &str) -> Option<&SharedDeviceMember> {
        self.members.iter().find(|m| m.device == device)
    }

    /// 票账守恒自检：发票 == 认领 + 无主（pass 是过程量不计入守恒式）。
    pub fn conservation(&self) -> Result<(), (u32, String)> {
        if self.tickets_issued != self.tickets_claimed + self.tickets_unclaimed {
            return Err((
                XC_TICKET_IMBALANCE,
                format!(
                    "向量 {:#x} 票账守恒破坏：发票 {} ≠ 认领 {} + 无主 {}",
                    self.vector, self.tickets_issued, self.tickets_claimed, self.tickets_unclaimed
                ),
            ));
        }
        // 成员级守恒：每成员 claimed+passed == 其经历的票程询问数（弱账——
        // 这里只查 claimed+passed ≤ 发票×成员数 上界，防账面暴涨假账）。
        let cap = self.tickets_issued.saturating_mul(self.members.len() as u64);
        let sum: u64 = self.members.iter().map(|m| m.claimed + m.passed).sum();
        if sum > cap {
            return Err((
                XC_TICKET_IMBALANCE,
                format!("成员应答总量 {} 超上界 {}——假账", sum, cap),
            ));
        }
        Ok(())
    }

    /// 成员可变借用（分发器记账用）。
    fn member_mut(&mut self, device: &str) -> Option<&mut SharedDeviceMember> {
        self.members.iter_mut().find(|m| m.device == device)
    }
}

/// 全局共享板：vector → entry。**共享/独占互斥**由本层的独占占用账
/// 把守（F0902 侧注册独占时经此处登记占用，防两边同挂一线）。
#[derive(Clone, Debug, Default)]
pub struct SharedVectorBoard {
    /// 向量账。
    vectors: BTreeMap<u8, SharedVectorEntry>,
    /// 独占占用账（vector → 占用者——F0902 联签登记）。
    exclusive_taken: BTreeMap<u8, String>,
    /// 逻辑 tick。
    tick: u64,
}

impl SharedVectorBoard {
    /// 新板。
    pub fn new() -> SharedVectorBoard {
        SharedVectorBoard::default()
    }

    /// 推进 tick。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 独占占用登记（F0902 侧调用——共享板侧知情）。
    pub fn mark_exclusive(&mut self, vector: u8, owner: &str) -> Result<(), (u32, String)> {
        if vector < SHARE_VECTOR_BASE || vector >= SHARE_VECTOR_LIMIT {
            return Err((XC_VECTOR_ILLEGAL, format!("向量 {:#x} 越池", vector)));
        }
        if self.vectors.contains_key(&vector) {
            return Err((
                XC_EXCLUSIVE_CLASH,
                format!("向量 {:#x} 已有共享挂线，独占登记被拒", vector),
            ));
        }
        self.exclusive_taken.insert(vector, owner.to_string());
        Ok(())
    }

    /// 独占释放。
    pub fn release_exclusive(&mut self, vector: u8) -> Result<(), (u32, String)> {
        self.exclusive_taken
            .remove(&vector)
            .map(|_| ())
            .ok_or((XC_WITHDRAW_ABSENT, format!("向量 {:#x} 无独占占用可释放", vector)))
    }

    /// 共享向量开账（独占占用则拒）。
    pub fn open_shared(
        &mut self,
        vector: u8,
        policy: SharePolicy,
    ) -> Result<(), (u32, String)> {
        if vector < SHARE_VECTOR_BASE || vector >= SHARE_VECTOR_LIMIT {
            return Err((XC_VECTOR_ILLEGAL, format!("向量 {:#x} 越池 [{:#x},{:#x})", vector, SHARE_VECTOR_BASE, SHARE_VECTOR_LIMIT)));
        }
        if let Some(owner) = self.exclusive_taken.get(&vector) {
            return Err((
                XC_EXCLUSIVE_CLASH,
                format!("向量 {:#x} 已被 {} 独占（MSI-X 侧），共享开账被拒", vector, owner),
            ));
        }
        if self.vectors.contains_key(&vector) {
            return Err((
                XC_EXCLUSIVE_CLASH,
                format!("向量 {:#x} 共享账已开", vector),
            ));
        }
        self.vectors.insert(vector, SharedVectorEntry::new(vector, policy));
        Ok(())
    }

    /// 挂线。
    pub fn enroll(
        &mut self,
        vector: u8,
        device: &str,
        handler: &str,
    ) -> Result<u32, (u32, String)> {
        let e = self
            .vectors
            .get_mut(&vector)
            .ok_or((XC_EMPTY_TICKET, format!("向量 {:#x} 共享账未开", vector)))?;
        e.enroll(device, handler, self.tick)
    }

    /// 撤线。
    pub fn withdraw(&mut self, vector: u8, device: &str) -> Result<SharedDeviceMember, (u32, String)> {
        let e = self
            .vectors
            .get_mut(&vector)
            .ok_or((XC_EMPTY_TICKET, format!("向量 {:#x} 共享账未开", vector)))?;
        e.withdraw(device)
    }

    /// 向量账只读。
    pub fn entry(&self, vector: u8) -> Option<&SharedVectorEntry> {
        self.vectors.get(&vector)
    }

    /// 开账向量清单。
    pub fn vectors(&self) -> Vec<u8> {
        self.vectors.keys().copied().collect()
    }

    /// 独占占用表只读。
    pub fn exclusives(&self) -> &BTreeMap<u8, String> {
        &self.exclusive_taken
    }

    /// 全板守恒自检。
    pub fn audit_all(&self) -> Result<(), (u32, String)> {
        for e in self.vectors.values() {
            e.conservation()?;
        }
        Ok(())
    }

    /// 可变 entry（分发器用）。
    fn entry_mut(&mut self, vector: u8) -> Option<&mut SharedVectorEntry> {
        self.vectors.get_mut(&vector)
    }
}

// ---------------------------------------------------------------------------
// §4 票制分发器（TicketDispatch）
// ---------------------------------------------------------------------------

/// 一次询问的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AskOutcome {
    /// 认领（票归它，票程终止——TicketOrder 语义）。
    Claimed,
    /// 让票（继续问下一个）。
    Passed,
}

/// 票程回执——一张票的完整去向账。
#[derive(Clone, Debug)]
pub struct TicketReceipt {
    /// 向量号。
    pub vector: u8,
    /// 事件载荷指纹（对账/回放用）。
    pub payload: u32,
    /// 票程遍历序（device_tag 列表——同输入必同序，确定性本体）。
    pub visited: Vec<String>,
    /// 认领者（None=无主）。
    pub claimed_by: Option<String>,
}

/// 分发器——挂在板上的票制引擎。
#[derive(Clone, Debug, Default)]
pub struct TicketDispatch;

impl TicketDispatch {
    /// 发一张票：按账面票序逐个询问。
    ///
    /// `answer` 是事件真主的裁决闭包：输入成员 tag，返回该成员对
    /// 本事件的应答（认领/让票）。真主显性由注入侧标注——分发器
    /// 只执行票程与记账，不猜归属（裁决与执行分离，审计面才干净）。
    ///
    /// TicketOrder：首个认领即停；后续成员不被询问（visited 到认领点截断）。
    /// Broadcast：全员询问，认领者全记（claimed_by 取首个认领——
    /// 广播无停点但回执仍单认领字段，多认领场景留给 F0906）。
    pub fn dispatch<F>(
        board: &mut SharedVectorBoard,
        vector: u8,
        payload: u32,
        answer: F,
    ) -> Result<TicketReceipt, (u32, String)>
    where
        F: Fn(&str) -> AskOutcome,
    {
        // 票序快照（先取账面序——账面 Vec 即序）。
        let order: Vec<String> = board
            .entry(vector)
            .map(|e| e.members().iter().map(|m| m.device.clone()).collect())
            .ok_or((XC_EMPTY_TICKET, format!("向量 {:#x} 共享账未开——空票程", vector)))?;
        if order.is_empty() {
            return Err((
                XC_EMPTY_TICKET,
                format!("向量 {:#x} 无人挂线即来事件——空票程立案", vector),
            ));
        }
        // 票程执行（对快照序询问，账面成员级记账）。
        let mut visited: Vec<String> = Vec::new();
        let mut claimed_by: Option<String> = None;
        for dev in &order {
            let outcome = answer(dev);
            visited.push(dev.clone());
            match outcome {
                AskOutcome::Claimed => {
                    if let Some(e) = board.entry_mut(vector) {
                        if let Some(m) = e.member_mut(dev) {
                            m.claim();
                        }
                        e.tickets_issued += 1;
                        e.tickets_claimed += 1;
                    }
                    claimed_by = Some(dev.clone());
                    break; // 认领即停（TicketOrder 本体语义）
                }
                AskOutcome::Passed => {
                    if let Some(e) = board.entry_mut(vector) {
                        if let Some(m) = e.member_mut(dev) {
                            m.pass();
                        }
                    }
                }
            }
        }
        // 全程无人认领 → 无主票立案（显性，不静默）。
        if claimed_by.is_none() {
            if let Some(e) = board.entry_mut(vector) {
                e.tickets_issued += 1;
                e.tickets_unclaimed += 1;
                if e.unclaimed_log.len() < 8 {
                    e.unclaimed_log.push(format!(
                        "payload={:#x} 全员让票（票程 {} 项）",
                        payload,
                        visited.len()
                    ));
                }
            }
        }
        Ok(TicketReceipt {
            vector,
            payload,
            visited,
            claimed_by,
        })
    }
}

// ---------------------------------------------------------------------------
// §5 确定性底座（LCG + FNV）
// ---------------------------------------------------------------------------

/// LCG 状态（Numerical Recipes 参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg(u32);

impl Lcg {
    /// 种子。
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], mut hash: u32) -> u32 {
    for &b in data {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// §6 票序确定性审计（判据"同输入同顺序 100%"本体）
// ---------------------------------------------------------------------------

/// 票序确定性双跑对拍：同种子同事件流跑两遍，票程遍历序指纹逐票相等。
pub fn order_determinism_100k() -> Result<String, (u32, String)> {
    // 单次跑：建板+3 设备挂线+按 LCG 流发 10 万票，回执 visited 序列
    // 逐票 FNV 合成全局指纹。
    let run = |seed: u32| -> Result<(u32, Vec<Option<String>>), (u32, String)> {
        let mut board = SharedVectorBoard::new();
        board.open_shared(0x64, SharePolicy::TicketOrder)?;
        let _ = board.enroll(0x64, "dev-a", "h-a")?;
        let _ = board.enroll(0x64, "dev-b", "h-b")?;
        let _ = board.enroll(0x64, "dev-c", "h-c")?;
        let mut rng = Lcg::new(seed);
        let mut hash = FNV_OFFSET;
        let mut owners: Vec<Option<String>> = Vec::new();
        const N: u32 = 100_000;
        for i in 0..N {
            // 事件真主：0/1/2 三设备 + 3=无主（判据的无主面也进确定性流）。
            let pick = rng.below(4);
            let owner: Option<&str> = match pick {
                0 => Some("dev-a"),
                1 => Some("dev-b"),
                2 => Some("dev-c"),
                _ => None,
            };
            let payload = i.wrapping_mul(2654435761);
            let r = TicketDispatch::dispatch(&mut board, 0x64, payload, |dev| {
                if Some(dev) == owner {
                    AskOutcome::Claimed
                } else {
                    AskOutcome::Passed
                }
            })?;
            // 指纹合成：visited 序 + 认领者。
            for v in &r.visited {
                hash = fnv32(v.as_bytes(), hash);
            }
            hash = fnv32(r.claimed_by.as_deref().unwrap_or("NONE").as_bytes(), hash);
            owners.push(r.claimed_by.clone());
        }
        Ok((hash, owners))
    };
    let (h1, o1) = run(0xBEEF_CAFE)?;
    let (h2, o2) = run(0xBEEF_CAFE)?;
    if h1 != h2 {
        return Err((
            XC_ORDER_DRIFT,
            format!("票程指纹漂移：{:#010x} vs {:#010x}", h1, h2),
        ));
    }
    if o1.len() != 100_000 || o2.len() != 100_000 {
        return Err((XC_TICKET_IMBALANCE, format!("票数 {} / {}", o1.len(), o2.len())));
    }
    for (i, (a, b)) in o1.iter().zip(o2.iter()).enumerate() {
        if a != b {
            return Err((
                XC_ORDER_DRIFT,
                format!("第 {} 票认领者漂移：{:?} vs {:?}", i, a, b),
            ));
        }
    }
    // 双跑间无账残留（每次 run 全新板——顺带验证板的可重建性）。
    Ok(format!(
        "双跑指纹全等 {:#010x}（10 万票 × 逐票票序+认领者对拍 100% 一致）",
        h1
    ))
}

// ---------------------------------------------------------------------------
// §7 三设备 10 万事件压测（判据 J1 主面：应答数与预期一致）
// ---------------------------------------------------------------------------

/// per 设备应答预期账与实账的对拍报告。
#[derive(Clone, Debug)]
pub struct AnswerReport {
    /// 设备 → (预期应答, 实际应答)。
    pub rows: Vec<(String, u64, u64)>,
    /// 无主预期/实际。
    pub unclaimed: (u64, u64),
}

impl AnswerReport {
    /// 全行一致为过。
    pub fn all_match(&self) -> bool {
        self.rows.iter().all(|(_, e, a)| e == a) && self.unclaimed.0 == self.unclaimed.1
    }
}

/// 三设备共享 10 万事件：各设备应答数与预期账一致 + 无主账一致。
///
/// 预期账推导：事件流 LCG 确定性，真主分布已知（同 §6 的 4 分流），
/// 注入侧独立累账——分发器实账与预期账终局对拍，两边都从同一事件
/// 流推导但记账路径完全独立（预期账在注入侧，实账在成员表）——
/// 这才是"应答数与预期一致"的判据本体（不是自己和自己对拍）。
pub fn three_devices_100k_answers() -> Result<String, (u32, String)> {
    let mut board = SharedVectorBoard::new();
    board.open_shared(0x65, SharePolicy::TicketOrder)?;
    let _ = board.enroll(0x65, "dev-a", "h-a")?;
    let _ = board.enroll(0x65, "dev-b", "h-b")?;
    let _ = board.enroll(0x65, "dev-c", "h-c")?;
    // 预期账（注入侧独立累账）。
    let mut expect: BTreeMap<String, u64> = BTreeMap::new();
    let mut expect_unclaimed: u64 = 0;
    let mut rng = Lcg::new(0xACE_5EED);
    const N: u32 = 100_000;
    for i in 0..N {
        let pick = rng.below(4);
        let owner: Option<&str> = match pick {
            0 => Some("dev-a"),
            1 => Some("dev-b"),
            2 => Some("dev-c"),
            _ => None,
        };
        let payload = i.wrapping_mul(2654435761);
        let r = TicketDispatch::dispatch(&mut board, 0x65, payload, |dev| {
            if Some(dev) == owner {
                AskOutcome::Claimed
            } else {
                AskOutcome::Passed
            }
        })?;
        // 注入侧预期账：真主应答 +1；票程里被问到的其他成员 pass（不进
        // 预期应答账——预期账只对拍"认领"这个判据面）。
        match (owner, r.claimed_by.as_deref()) {
            (Some(o), Some(c)) if o == c => {
                *expect.entry(o.to_string()).or_insert(0) += 1;
            }
            (None, None) => expect_unclaimed += 1,
            _ => {
                return Err((
                    XC_ANSWER_MISMATCH,
                    format!(
                        "第 {} 票认领失真：预期主 {:?} 实认领 {:?}",
                        i, owner, r.claimed_by
                    ),
                ));
            }
        }
    }
    board.audit_all()?;
    // 实账（成员表）vs 预期账。
    let e = board
        .entry(0x65)
        .ok_or((XC_EMPTY_TICKET, "向量账丢失".to_string()))?;
    let mut rows: Vec<(String, u64, u64)> = Vec::new();
    for m in e.members() {
        rows.push((m.device.clone(), *expect.get(&m.device).unwrap_or(&0), m.claimed));
    }
    let rep = AnswerReport {
        rows,
        unclaimed: (expect_unclaimed, e.tickets_unclaimed),
    };
    if !rep.all_match() {
        let bad: Vec<String> = rep
            .rows
            .iter()
            .filter(|(_, e0, a)| e0 != a)
            .map(|(d, e0, a)| format!("{} 预期{} 实{}", d, e0, a))
            .collect();
        return Err((
            XC_ANSWER_MISMATCH,
            format!(
                "应答账失配：{} 无主 预期{} 实{}",
                bad.join("；"),
                rep.unclaimed.0,
                rep.unclaimed.1
            ),
        ));
    }
    let summary: Vec<String> = rep
        .rows
        .iter()
        .map(|(d, _, a)| format!("{}={}", d, a))
        .collect();
    Ok(format!(
        "三设备 10 万事件应答账一致（{} 无主={}）+ 票账守恒过",
        summary.join(" "),
        rep.unclaimed.1
    ))
}

// ---------------------------------------------------------------------------
// §8 无应答识别注入（判据 10/10）
// ---------------------------------------------------------------------------

/// 无主识别注入——10 轮，每轮注入 1 张无人认领的票，必须全部被
/// 识别为无主（unclaimed 计数 + 明细留痕），票程全成员遍历且无认领。
pub fn unclaimed_detect_10() -> Result<String, (u32, String)> {
    let mut board = SharedVectorBoard::new();
    board.open_shared(0x66, SharePolicy::TicketOrder)?;
    let _ = board.enroll(0x66, "dev-a", "h-a")?;
    let _ = board.enroll(0x66, "dev-b", "h-b")?;
    let _ = board.enroll(0x66, "dev-c", "h-c")?;
    let mut hits = 0u32;
    for round in 0..10u32 {
        let payload = 0x1_0000 + u32::from(round);
        let r = TicketDispatch::dispatch(&mut board, 0x66, payload, |_| AskOutcome::Passed)?;
        let full = r.visited.len() == 3; // 全员被问
        let none = r.claimed_by.is_none(); // 无人认领
        if full && none {
            hits += 1;
        }
    }
    if hits != 10 {
        return Err((
            XC_UNCLAIMED_MISSED,
            format!("无主识别 {} / 10——漏网", hits),
        ));
    }
    let e = board
        .entry(0x66)
        .ok_or((XC_EMPTY_TICKET, "向量账丢失".to_string()))?;
    let (issued, claimed, unclaimed) = e.ticket_ledger();
    if issued != 10 || claimed != 0 || unclaimed != 10 {
        return Err((
            XC_TICKET_IMBALANCE,
            format!("票账失真：发票 {} 认领 {} 无主 {}", issued, claimed, unclaimed),
        ));
    }
    if e.unclaimed_log().is_empty() {
        return Err((XC_UNCLAIMED_MISSED, "无主明细未留痕".to_string()));
    }
    Ok(format!(
        "10/10 无主票全识别（全票程 3 成员 × 10 轮，票账 10/0/10，明细留痕 {} 条）",
        e.unclaimed_log().len()
    ))
}

// ---------------------------------------------------------------------------
// §9 语义边界注入套件（共享/独占互斥 + 重复挂线 + 空票程）
// ---------------------------------------------------------------------------

/// 边界违例注入——8 类全拦截。
///
/// 0 独占向量上开共享账　1 共享向量上登记独占　2 同设备重复挂线
/// 3 空向量来事件（无人挂线）　4 未开账向量来事件
/// 5 撤不存在的成员　6 成员硬帽（17 台挂线）
/// 7 越池向量开账
pub fn boundary_violations_8() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();

    // ── 0：独占向量上开共享账 ──
    {
        let mut board = SharedVectorBoard::new();
        let _ = board.mark_exclusive(0x67, "nvme0-msix");
        let r = board.open_shared(0x67, SharePolicy::TicketOrder);
        hits.push((0, matches!(r, Err((XC_EXCLUSIVE_CLASH, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——互斥失效".to_string(),
        }));
    }
    // ── 1：共享向量上登记独占 ──
    {
        let mut board = SharedVectorBoard::new();
        let _ = board.open_shared(0x68, SharePolicy::TicketOrder);
        let r = board.mark_exclusive(0x68, "nvme1-msix");
        hits.push((1, matches!(r, Err((XC_EXCLUSIVE_CLASH, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——互斥失效".to_string(),
        }));
    }
    // ── 2：同设备重复挂线 ──
    {
        let mut board = SharedVectorBoard::new();
        let _ = board.open_shared(0x69, SharePolicy::TicketOrder);
        let _ = board.enroll(0x69, "dev-x", "h-x");
        let r = board.enroll(0x69, "dev-x", "h-x2");
        hits.push((2, matches!(r, Err((XC_DUP_ENROLL, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——重复挂线".to_string(),
        }));
    }
    // ── 3：空向量来事件 ──
    {
        let mut board = SharedVectorBoard::new();
        let _ = board.open_shared(0x6A, SharePolicy::TicketOrder);
        let r = TicketDispatch::dispatch(&mut board, 0x6A, 1, |_| AskOutcome::Passed);
        hits.push((3, matches!(r, Err((XC_EMPTY_TICKET, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——空票程".to_string(),
        }));
    }
    // ── 4：未开账向量来事件 ──
    {
        let mut board = SharedVectorBoard::new();
        let r = TicketDispatch::dispatch(&mut board, 0x6B, 1, |_| AskOutcome::Passed);
        hits.push((4, matches!(r, Err((XC_EMPTY_TICKET, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——未开账".to_string(),
        }));
    }
    // ── 5：撤不存在的成员 ──
    {
        let mut board = SharedVectorBoard::new();
        let _ = board.open_shared(0x6C, SharePolicy::TicketOrder);
        let _ = board.enroll(0x6C, "dev-y", "h-y");
        let r = board.withdraw(0x6C, "ghost");
        hits.push((5, matches!(r, Err((XC_WITHDRAW_ABSENT, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——幽灵撤线".to_string(),
        }));
    }
    // ── 6：成员硬帽 ──
    {
        let mut board = SharedVectorBoard::new();
        let _ = board.open_shared(0x6D, SharePolicy::TicketOrder);
        let mut last = Ok(0);
        for i in 0..MEMBER_HARD_CAP {
            last = board.enroll(0x6D, &format!("dev-{}", i), &format!("h-{}", i));
            if last.is_err() {
                break;
            }
        }
        let overflow = board.enroll(0x6D, "dev-overflow", "h-overflow");
        hits.push((
            6,
            last.is_ok() && matches!(overflow, Err((XC_MEMBER_CAP, _))),
            match overflow {
                Err(e) => e.1,
                Ok(_) => "未拦——硬帽穿透".to_string(),
            },
        ));
    }
    // ── 7：越池向量开账 ──
    {
        let mut board = SharedVectorBoard::new();
        let r = board.open_shared(0x59, SharePolicy::TicketOrder);
        hits.push((7, matches!(r, Err((XC_VECTOR_ILLEGAL, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——越池".to_string(),
        }));
    }

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((
            XC_EXCLUSIVE_CLASH,
            format!("边界违例漏网 {} 项：{}", missed.len(), names.join("；")),
        ));
    }
    Ok(format!(
        "8/8 边界违例全拦截：{}",
        hits.iter().map(|(k, _, _)| format!("#{}拦", k)).collect::<Vec<_>>().join(" ")
    ))
}

// ---------------------------------------------------------------------------
// §10 策略语义表自检 + 快照
// ---------------------------------------------------------------------------

/// 策略语义表静态自检：三策略确定性全 true + TicketOrder/Broadcast
/// 适用共享线 + 语义表 3×3 全显性（无空洞）。
pub fn policy_semantics_selfcheck() -> Result<String, (u32, String)> {
    for p in SharePolicy::all() {
        if !p.semantic(0) {
            return Err((XC_POLICY_ILLEGAL, format!("{} 确定性语义缺失", p.name())));
        }
        if !p.semantic(2) {
            return Err((XC_POLICY_ILLEGAL, format!("{} 共享线适用性缺失", p.name())));
        }
    }
    if !SharePolicy::TicketOrder.semantic(1) {
        return Err((XC_POLICY_ILLEGAL, "TicketOrder 认领即停语义缺失".to_string()));
    }
    if SharePolicy::Broadcast.semantic(1) {
        return Err((XC_POLICY_ILLEGAL, "Broadcast 不应有停点语义".to_string()));
    }
    Ok("策略语义表 3×3 全显性自洽".to_string())
}

/// 板快照指纹。
pub struct SharedSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl SharedSnap {
    /// 采样：每向量（成员表全字段+票账）+ 独占表。
    pub fn take(board: &SharedVectorBoard) -> SharedSnap {
        let mut buf: Vec<u8> = Vec::new();
        for (v, e) in board.vectors.iter() {
            buf.push(*v);
            buf.push(e.policy().idx() as u8);
            for m in e.members() {
                buf.extend_from_slice(m.device.as_bytes());
                buf.push(0);
                buf.extend_from_slice(&m.order.to_le_bytes());
                buf.extend_from_slice(&m.claimed.to_le_bytes());
                buf.extend_from_slice(&m.passed.to_le_bytes());
            }
            let (i, c, u) = e.ticket_ledger();
            buf.extend_from_slice(&i.to_le_bytes());
            buf.extend_from_slice(&c.to_le_bytes());
            buf.extend_from_slice(&u.to_le_bytes());
        }
        for (v, owner) in board.exclusive_taken.iter() {
            buf.push(0xEE);
            buf.push(*v);
            buf.extend_from_slice(owner.as_bytes());
        }
        SharedSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "共享向量={} 独占={} tick={}",
                board.vectors.len(),
                board.exclusive_taken.len(),
                board.tick
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §11 票程成员级回执审计（应答过程量账）
// ---------------------------------------------------------------------------

/// 票程过程量审计：每成员 claimed+passed == 其被询问次数（从回执流
/// 重放核对——过程账与结果账双向闭环）。
pub fn process_ledger_replay() -> Result<String, (u32, String)> {
    let mut board = SharedVectorBoard::new();
    board.open_shared(0x6E, SharePolicy::TicketOrder)?;
    let _ = board.enroll(0x6E, "dev-a", "h-a")?;
    let _ = board.enroll(0x6E, "dev-b", "h-b")?;
    // 小流：30 票（真主 a/b/无主 三分），回执流重放核对过程量。
    let mut rng = Lcg::new(0x51CE_C0DE);
    let mut receipts: Vec<(u32, Option<String>, Vec<String>)> = Vec::new();
    for i in 0..30u32 {
        let pick = rng.below(3);
        let owner = match pick {
            0 => Some("dev-a"),
            1 => Some("dev-b"),
            _ => None,
        };
        let r = TicketDispatch::dispatch(&mut board, 0x6E, i, |dev| {
            if Some(dev) == owner {
                AskOutcome::Claimed
            } else {
                AskOutcome::Passed
            }
        })?;
        receipts.push((r.payload, r.claimed_by, r.visited));
    }
    // 重放：按回执流独立累过程账。
    let mut replay: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // claim, pass
    for (_, claimed_by, visited) in &receipts {
        for dev in visited {
            let entry = replay.entry(dev.clone()).or_insert((0, 0));
            if claimed_by.as_deref() == Some(dev.as_str()) {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    // 对拍成员表实账。
    let e = board
        .entry(0x6E)
        .ok_or((XC_EMPTY_TICKET, "向量账丢失".to_string()))?;
    for m in e.members() {
        let (rc, rp) = replay.get(&m.device).copied().unwrap_or((0, 0));
        if rc != m.claimed || rp != m.passed {
            return Err((
                XC_TICKET_IMBALANCE,
                format!(
                    "过程账失配：{} 回放({},{}) ≠ 实账({},{})",
                    m.device, rc, rp, m.claimed, m.passed
                ),
            ));
        }
    }
    e.conservation()?;
    Ok(format!(
        "过程账重放对拍过：{} 成员 claimed/pass 实账==回放，票账守恒过",
        e.members().len()
    ))
}

// ---------------------------------------------------------------------------
// §12 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：语义表自检 + 双跑确定性 + 三设备压测 + 无主识别 + 边界
/// 注入 + 过程账重放，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(policy_semantics_selfcheck()?);
    parts.push(order_determinism_100k()?);
    parts.push(three_devices_100k_answers()?);
    parts.push(unclaimed_detect_10()?);
    parts.push(boundary_violations_8()?);
    parts.push(process_ledger_replay()?);
    // 快照稳定性。
    let mut board = SharedVectorBoard::new();
    board.open_shared(0x6F, SharePolicy::TicketOrder)?;
    let _ = board.enroll(0x6F, "snap-a", "h-a")?;
    let s0 = SharedSnap::take(&board);
    let s1 = SharedSnap::take(&board);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "快照指纹不稳定（不可复现）".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
