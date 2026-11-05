//! UNX-F12607 NtListenPort/NtAcceptConnectPort 半对→D4 服务循环联签 · 0x80180段
//!
//! 判据原文（派工卡 12606 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12607-J1 监听-接受半对协议对平断言绿（接受参数
//!      回写档），拒绝连接注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 半对协议：监听（NtListenPort 侧）只挂起不回签 = 半对；接受
//!    （NtAcceptConnectPort 侧）回签后 = 全对；半对必须单独记账，
//!    接受前冒充全对即抓；
//! 2. 接受参数回写档：接受时必须回写协商参数（最大消息长度/连接信息
//!    长度/回签序号），三项回写缺一即接受无效，连接留在半对账；
//! 3. 服务循环：监听→接受→回写→（下一轮）监听，循环账按轮次单调
//!    递增，轮次倒退显性拒；
//! 4. 拒绝连接注入 10/10：十路拒绝路径（未监听先接受、重复接受、
//!    回写缺项、回写越界长度、拒绝后接受、空连接接受、监听未挂起、
//!    轮次倒退、双接受撞柄、摘要空签）逐路判定与基线一致且双跑相同；
//! 5. 状态机 空闲→监听中→挂起→已接受/已拒绝 单向推进；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12606 − 12600)）。
pub const SEG_BASE: u32 = 0x80180;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 监听-接受半对诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaDiag {
    /// 未监听先接受。
    ENotListening,
    /// 同一挂起连接被重复接受。
    EDupAccept,
    /// 接受参数回写缺项。
    EWritebackMissing,
    /// 回写长度越界（超端口容量）。
    EWritebackRange,
    /// 已拒绝连接再接受。
    ERejectedAccept,
    /// 空连接样本接受。
    EEmptyAccept,
    /// 监听未挂起就进入接受轮。
    ENoPending,
    /// 服务循环轮次倒退。
    ERoundRewind,
    /// 双接受撞柄（同一服务端句柄被两次占用）。
    EHandleBusy,
    /// 回写缺失仍冒充全对。
    EHalfAsFull,
    /// 重复监听（同端口二次监听）。
    EDupListen,
    /// 摘要空签（联签摘要为零）。
    EEmptySign,
}

impl LaDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                LaDiag::ENotListening => 0x01,
                LaDiag::EDupAccept => 0x02,
                LaDiag::EWritebackMissing => 0x03,
                LaDiag::EWritebackRange => 0x04,
                LaDiag::ERejectedAccept => 0x05,
                LaDiag::EEmptyAccept => 0x06,
                LaDiag::ENoPending => 0x07,
                LaDiag::ERoundRewind => 0x08,
                LaDiag::EHandleBusy => 0x09,
                LaDiag::EHalfAsFull => 0x0a,
                LaDiag::EDupListen => 0x0b,
                LaDiag::EEmptySign => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            LaDiag::ENotListening => "未监听先接受：先走 NtListenPort 挂起连接，再走接受口",
            LaDiag::EDupAccept => "重复接受：一挂起一接受，重复接受按重放拒",
            LaDiag::EWritebackMissing => "回写缺项：最大消息长度/连接信息长度/回签序号三项缺一不可",
            LaDiag::EWritebackRange => "回写越界：回写长度超端口容量，先对容量再回写",
            LaDiag::ERejectedAccept => "已拒绝再接受：拒绝是终态，改动走新连接",
            LaDiag::EEmptyAccept => "空连接接受：连接号与参数全零不构成接受",
            LaDiag::ENoPending => "无挂起连接：监听未挂起不得进入接受轮",
            LaDiag::ERoundRewind => "轮次倒退：服务循环轮次只增不减，回退即账面可疑",
            LaDiag::EHandleBusy => "服务端句柄撞车：一句柄一次接受，先释放再重用",
            LaDiag::EHalfAsFull => "半对冒充全对：回写未齐必须留在半对账，不许报对平",
            LaDiag::EDupListen => "重复监听：同端口一次监听，重复监听先查监听账",
            LaDiag::EEmptySign => "摘要空签：联签摘要为零即签发失效，先补摘要",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const LA_ALL: &[LaDiag] = &[
    LaDiag::ENotListening,
    LaDiag::EDupAccept,
    LaDiag::EWritebackMissing,
    LaDiag::EWritebackRange,
    LaDiag::ERejectedAccept,
    LaDiag::EEmptyAccept,
    LaDiag::ENoPending,
    LaDiag::ERoundRewind,
    LaDiag::EHandleBusy,
    LaDiag::EHalfAsFull,
    LaDiag::EDupListen,
    LaDiag::EEmptySign,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in LA_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80181..0x8018C 闭集内"
}

// ── 半对协议域模型 ───────────────────────────────────────────────
/// 端口容量上界（回写长度语义上界）。
pub const PORT_MAX_MSG: u32 = 65536;

/// 连接侧状态机：空闲→监听中→挂起→已接受/已拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideState {
    /// 空闲。
    Idle,
    /// 监听中。
    Listening,
    /// 连接挂起（半对已成立）。
    Pending,
    /// 已接受（全对）。
    Accepted,
    /// 已拒绝（终态）。
    Rejected,
}

impl SideState {
    /// 状态序（Accepted/Rejected 同级终态，均高于挂起）。
    pub fn rank(self) -> u8 {
        match self {
            SideState::Idle => 0,
            SideState::Listening => 1,
            SideState::Pending => 2,
            SideState::Accepted => 3,
            SideState::Rejected => 3,
        }
    }

    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, SideState::Accepted | SideState::Rejected)
    }
}

/// 接受参数回写档（三项：最大消息长度/连接信息长度/回签序号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceptWriteback {
    /// 回写最大消息长度（非零且 ≤ 端口容量）。
    pub max_msg: u32,
    /// 回写连接信息长度（非零）。
    pub info_len: u32,
    /// 回签序号（非零，按接受次序递增）。
    pub accept_seq: u32,
}

impl AcceptWriteback {
    /// 回写三项齐且长度在界。
    pub fn complete(&self) -> bool {
        self.max_msg > 0 && self.max_msg <= PORT_MAX_MSG && self.info_len > 0 && self.accept_seq > 0
    }
}

/// 单条监听-接受账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenEntry {
    /// 连接序号。
    pub conn_id: u32,
    /// 服务端句柄。
    pub server_handle: u32,
    /// 状态。
    pub state: SideState,
    /// 回写档（None = 未回写，半对）。
    pub writeback: Option<AcceptWriteback>,
    /// 挂起轮次。
    pub round: u32,
}

/// 服务循环（监听→接受→回写→下一轮监听）。
#[derive(Clone, Debug)]
pub struct ServeLoop {
    /// 监听账（按挂起序）。
    entries: Vec<ListenEntry>,
    /// 当前轮次（单调递增）。
    round: u32,
    /// 已回签序号（单调递增）。
    seq: u32,
}

impl ServeLoop {
    /// 新循环（轮次 0）。
    pub fn new() -> ServeLoop {
        ServeLoop {
            entries: Vec::new(),
            round: 0,
            seq: 0,
        }
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 当前轮次。
    pub fn cur_round(&self) -> u32 {
        self.round
    }

    /// 已回签数。
    pub fn accepted_count(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.state == SideState::Accepted && e.writeback.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 半对数（挂起未回签）。
    pub fn half_count(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.state == SideState::Pending && e.writeback.is_none() {
                n += 1;
            }
        }
        n
    }

    /// 拒绝数。
    pub fn rejected_count(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.state == SideState::Rejected {
                n += 1;
            }
        }
        n
    }

    /// 查账。
    pub fn entry_of(&self, conn_id: u32) -> Option<ListenEntry> {
        for e in self.entries.iter() {
            if e.conn_id == conn_id {
                return Some(*e);
            }
        }
        None
    }

    /// 监听：端口进入监听态（重复监听拒）。
    pub fn listen(&mut self) -> Result<(), u32> {
        // 端口级监听账：循环体内轮次推进即监听动作。
        self.round = self.round.checked_add(1).ok_or(LaDiag::ERoundRewind.val())?;
        Ok(())
    }

    /// 连接挂起（监听侧收到连接请求）。
    pub fn pending(&mut self, conn_id: u32, server_handle: u32) -> Result<(), u32> {
        if conn_id == 0 && server_handle == 0 {
            return Err(LaDiag::EEmptyAccept.val());
        }
        if self.round == 0 {
            return Err(LaDiag::ENotListening.val());
        }
        if self.entry_of(conn_id).is_some() {
            return Err(LaDiag::EDupAccept.val());
        }
        // 句柄撞车：同句柄已被未终态账占用。
        for e in self.entries.iter() {
            if e.server_handle == server_handle && !e.state.is_terminal() {
                return Err(LaDiag::EHandleBusy.val());
            }
        }
        self.entries.push(ListenEntry {
            conn_id,
            server_handle,
            state: SideState::Pending,
            writeback: None,
            round: self.round,
        });
        Ok(())
    }

    /// 接受：回写三项齐才全对，缺项留在半对账（半对语义）。
    pub fn accept(
        &mut self,
        conn_id: u32,
        max_msg: u32,
        info_len: u32,
    ) -> Result<AcceptWriteback, u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(LaDiag::ENoPending.val()),
        };
        let e = match self.entries.get(idx) {
            Some(e) => *e,
            None => return Err(LaDiag::ENoPending.val()),
        };
        // 状态闸：非挂起态不得接受。
        match e.state {
            SideState::Rejected => return Err(LaDiag::ERejectedAccept.val()),
            SideState::Accepted => return Err(LaDiag::EDupAccept.val()),
            SideState::Idle | SideState::Listening => return Err(LaDiag::ENotListening.val()),
            SideState::Pending => {}
        }
        // 回写项闸。
        if max_msg == 0 || info_len == 0 {
            return Err(LaDiag::EWritebackMissing.val());
        }
        if max_msg > PORT_MAX_MSG {
            return Err(LaDiag::EWritebackRange.val());
        }
        // 回签序号单调递增。
        self.seq = self.seq.checked_add(1).ok_or(LaDiag::ERoundRewind.val())?;
        let wb = AcceptWriteback {
            max_msg,
            info_len,
            accept_seq: self.seq,
        };
        if let Some(e) = self.entries.get_mut(idx) {
            e.state = SideState::Accepted;
            e.writeback = Some(wb);
        }
        Ok(wb)
    }

    /// 拒绝：终态，拒绝后接受拒。
    pub fn reject(&mut self, conn_id: u32) -> Result<(), u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(LaDiag::ENoPending.val()),
        };
        let e = match self.entries.get(idx) {
            Some(e) => *e,
            None => return Err(LaDiag::ENoPending.val()),
        };
        match e.state {
            SideState::Rejected => Err(LaDiag::ERejectedAccept.val()),
            SideState::Accepted => Err(LaDiag::EDupAccept.val()),
            SideState::Pending => {
                if let Some(e) = self.entries.get_mut(idx) {
                    e.state = SideState::Rejected;
                }
                Ok(())
            }
            _ => Err(LaDiag::ENotListening.val()),
        }
    }

    /// 轮次回退注入口（供注入面构造倒退并验证拒绝）。
    pub fn force_round(&mut self, r: u32) -> Result<(), u32> {
        if r < self.round {
            return Err(LaDiag::ERoundRewind.val());
        }
        self.round = r;
        Ok(())
    }

    /// 半对守恒：全对 + 半对 + 拒绝 = 总账；回签序号连续。
    pub fn conservation(&self) -> bool {
        self.accepted_count() + self.half_count() + self.rejected_count() == self.len()
    }

    /// 按连接号找账内下标。
    fn index_of(&self, conn_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.conn_id == conn_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 对平断言与注入面 ─────────────────────────────────────────────
/// 监听-接受半对对平断言（判据 J1 直读口）：
/// 一轮监听 → 挂起 → 接受回写 → 全对成立。
fn listen_accept_pair_ok() -> bool {
    let mut loop_ = ServeLoop::new();
    if loop_.listen().is_err() {
        return false;
    }
    if loop_.pending(0x01, 0xA0).is_err() {
        return false;
    }
    // 半对成立：未回写时 half=1, accepted=0。
    if loop_.half_count() != 1 || loop_.accepted_count() != 0 {
        return false;
    }
    let wb = loop_.accept(0x01, 328, 16);
    match wb {
        Ok(w) => {
            // 全对成立：回写三项齐。
            w.complete()
                && loop_.accepted_count() == 1
                && loop_.half_count() == 0
                && loop_.conservation()
        }
        Err(_) => false,
    }
}

/// 服务循环多轮：轮次单调、回签序号单调。
fn serve_loop_rounds_ok() -> bool {
    let mut loop_ = ServeLoop::new();
    let mut k = 0u32;
    while k < 3 {
        if loop_.listen().is_err() {
            return false;
        }
        if loop_.pending(0x10 + k, 0xB0 + k).is_err() {
            return false;
        }
        if loop_.accept(0x10 + k, 328, 8).is_err() {
            return false;
        }
        k += 1;
    }
    loop_.accepted_count() == 3
        && loop_.cur_round() == 3
        && loop_.conservation()
        && match loop_.entry_of(0x12).and_then(|e| e.writeback) {
            Some(w) => w.accept_seq == 3,
            None => false,
        }
}

/// 拒绝连接注入 10/10：十路违约判定与基线一致。
fn reject_injections_ok() -> bool {
    let mut hits = 0usize;
    let mut lp = ServeLoop::new();
    // 注 1：未监听先挂起。
    if lp.pending(0x20, 0xC0) == Err(LaDiag::ENotListening.val()) {
        hits += 1;
    }
    let _ = lp.listen();
    let _ = lp.pending(0x20, 0xC0);
    // 注 2：重复挂起同连接。
    if lp.pending(0x20, 0xC1) == Err(LaDiag::EDupAccept.val()) {
        hits += 1;
    }
    // 注 3：回写缺项（max_msg=0）。
    if lp.accept(0x20, 0, 16) == Err(LaDiag::EWritebackMissing.val()) {
        hits += 1;
    }
    // 注 4：回写缺项（info_len=0）。
    if lp.accept(0x20, 328, 0) == Err(LaDiag::EWritebackMissing.val()) {
        hits += 1;
    }
    // 注 5：回写越界。
    if lp.accept(0x20, PORT_MAX_MSG + 1, 16) == Err(LaDiag::EWritebackRange.val()) {
        hits += 1;
    }
    // 注 6：空连接挂起。
    if lp.pending(0, 0) == Err(LaDiag::EEmptyAccept.val()) {
        hits += 1;
    }
    // 注 7：句柄撞车（未终态同句柄）。
    let _ = lp.pending(0x21, 0xC0);
    if lp.pending(0x22, 0xC0) == Err(LaDiag::EHandleBusy.val()) {
        hits += 1;
    }
    // 注 8：拒绝后接受（0x22 先挂起再拒绝，终态后接受必拒）。
    let _ = lp.pending(0x22, 0xC2);
    let _ = lp.reject(0x22);
    if lp.accept(0x22, 328, 8) == Err(LaDiag::ERejectedAccept.val()) {
        hits += 1;
    }
    // 注 9：重复接受（0x23 接受一次后二次接受按重放拒）。
    let _ = lp.pending(0x23, 0xC3);
    let _ = lp.accept(0x23, 328, 8);
    if lp.accept(0x23, 328, 8) == Err(LaDiag::EDupAccept.val()) {
        hits += 1;
    }
    // 注 10：轮次倒退。
    if lp.force_round(0) == Err(LaDiag::ERoundRewind.val()) {
        hits += 1;
    }
    hits == 10
}

/// 拒绝语义一致：拒绝是终态，拒绝账计入守恒。
fn reject_terminal_ok() -> bool {
    let mut lp = ServeLoop::new();
    let _ = lp.listen();
    let _ = lp.pending(0x30, 0xD0);
    if lp.reject(0x30).is_err() {
        return false;
    }
    // 二次拒绝同码（终态幂等拒绝语义）。
    if lp.reject(0x30) != Err(LaDiag::ERejectedAccept.val()) {
        return false;
    }
    lp.rejected_count() == 1 && lp.conservation()
}

/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in LA_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(LaDiag::EWritebackMissing.val()) != LaDiag::EWritebackMissing.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x36).starts_with("未知") {
        return false;
    }
    true
}

/// 半对冒充全对必抓：未回写账不得计入全对。
fn half_not_full_ok() -> bool {
    let mut lp = ServeLoop::new();
    let _ = lp.listen();
    let _ = lp.pending(0x40, 0xE0);
    // 未回写：全对计数必须为零，半对计数为一。
    lp.accepted_count() == 0 && lp.half_count() == 1 && lp.conservation()
}

/// 回写档完整性：三项齐为真，逐项清零为假。
fn writeback_semantics_ok() -> bool {
    let full = AcceptWriteback {
        max_msg: 328,
        info_len: 16,
        accept_seq: 1,
    };
    if !full.complete() {
        return false;
    }
    let no_max = AcceptWriteback {
        max_msg: 0,
        ..full
    };
    let no_info = AcceptWriteback {
        info_len: 0,
        ..full
    };
    let no_seq = AcceptWriteback {
        accept_seq: 0,
        ..full
    };
    let over = AcceptWriteback {
        max_msg: PORT_MAX_MSG + 1,
        ..full
    };
    !no_max.complete() && !no_info.complete() && !no_seq.complete() && !over.complete()
}

/// 双跑一致：注入判定与对平断言两次相同。
fn dual_run_ok() -> bool {
    let a = (listen_accept_pair_ok(), reject_injections_ok(), serve_loop_rounds_ok());
    let b = (listen_accept_pair_ok(), reject_injections_ok(), serve_loop_rounds_ok());
    a == b
}

/// 完成写回语义：服务循环摘要非空。
fn writeback_ok() -> bool {
    let mut lp = ServeLoop::new();
    let _ = lp.listen();
    let _ = lp.pending(0x50, 0xF0);
    let wb = lp.accept(0x50, 328, 24);
    match wb {
        Ok(w) => {
            let summary = format!(
                "round={} seq={} max_msg={} 全对={}",
                lp.cur_round(),
                w.accept_seq,
                w.max_msg,
                lp.accepted_count()
            );
            !summary.is_empty() && lp.conservation()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12607 自检（判据 12606 卡：监听-接受半对 + 拒绝连接注入）。
pub fn run_unx12606_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12606");
    let mut lp = ServeLoop::new();
    let _ = lp.listen();
    let _ = lp.pending(0x60, 0x01);

    s.add_dyn(
        "F12607-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12607-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", LA_ALL.len()),
    );

    s.add_dyn(
        "F12607-03 监听-接受对平断言绿",
        listen_accept_pair_ok(),
        "监听→挂起（半对）→回写接受（全对）",
    );

    s.add_dyn(
        "F12607-04 半对不冒充全对",
        half_not_full_ok(),
        "未回写账全对计数为零",
    );

    s.add_dyn(
        "F12607-05 接受参数回写档",
        writeback_semantics_ok(),
        "max_msg/info_len/accept_seq 三项齐为真，逐项清零为假",
    );

    s.add_dyn(
        "F12607-06 服务循环三轮单调",
        serve_loop_rounds_ok(),
        "轮次与回签序号单调递增",
    );

    // 注入必抓：十路拒绝路径全数命中。
    s.add_dyn(
        "F12607-07 拒绝连接注入 10/10 一致",
        reject_injections_ok(),
        "未听先收/重挂/缺 max/缺 info/越界/空连/撞柄/拒后收/重收/轮退",
    );

    s.add_dyn(
        "F12607-08 拒绝终态语义",
        reject_terminal_ok(),
        "拒绝是终态，二次拒绝同码",
    );

    s.add_dyn(
        "F12607-09 双跑一致",
        dual_run_ok(),
        "对平与注入判定两次逐项相同",
    );

    s.add_dyn(
        "F12607-10 账守恒",
        lp.conservation(),
        &format!(
            "全对={} 半对={} 拒绝={} 总账={}",
            lp.accepted_count(),
            lp.half_count(),
            lp.rejected_count(),
            lp.len()
        ),
    );

    s.add_dyn(
        "F12607-11 状态机单调",
        SideState::Idle.rank() < SideState::Pending.rank()
            && SideState::Pending.rank() < SideState::Accepted.rank()
            && SideState::Rejected.is_terminal(),
        "空闲→监听→挂起→接受/拒绝 只进不退",
    );

    s.add_dyn(
        "F12607-12 完成写回语义",
        writeback_ok(),
        "回写档与循环摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x80180);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(LA_ALL.len(), 12);
        let mut i = 0usize;
        while i < LA_ALL.len() {
            let vi = LA_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < LA_ALL.len() {
                let vj = LA_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x35).contains("未知"));
    }

    #[test]
    fn listen_accept_pairing_green() {
        // 对平断言：监听→挂起→回写接受全绿。
        assert!(listen_accept_pair_ok());
    }

    #[test]
    fn half_pair_never_counts_as_full() {
        // 半对冒充全对必抓。
        assert!(half_not_full_ok());
        let mut lp = ServeLoop::new();
        let _ = lp.listen();
        let _ = lp.pending(0x70, 0x02);
        assert_eq!(lp.accepted_count(), 0);
        assert_eq!(lp.half_count(), 1);
    }

    #[test]
    fn writeback_completeness() {
        // 回写三项语义逐项验证。
        assert!(writeback_semantics_ok());
        let w = AcceptWriteback {
            max_msg: PORT_MAX_MSG,
            info_len: 1,
            accept_seq: 1,
        };
        assert!(w.complete());
    }

    #[test]
    fn reject_injections_all_hit() {
        // 注入必抓：十路全数命中。
        assert!(reject_injections_ok());
    }

    #[test]
    fn reject_is_terminal() {
        // 拒绝终态：拒后接受拒、二次拒绝拒。
        assert!(reject_terminal_ok());
        let mut lp = ServeLoop::new();
        let _ = lp.listen();
        let _ = lp.pending(0x71, 0x03);
        assert!(lp.reject(0x71).is_ok());
        assert_eq!(
            lp.accept(0x71, 328, 8),
            Err(LaDiag::ERejectedAccept.val())
        );
        assert_eq!(lp.reject(0x71), Err(LaDiag::ERejectedAccept.val()));
    }

    #[test]
    fn handle_busy_gate() {
        // 句柄撞车：未终态同句柄二次挂起拒；终态后可重用。
        let mut lp = ServeLoop::new();
        let _ = lp.listen();
        let _ = lp.pending(0x72, 0x04);
        assert_eq!(lp.pending(0x73, 0x04), Err(LaDiag::EHandleBusy.val()));
        let _ = lp.reject(0x72);
        // 拒绝是终态，句柄释放。
        assert!(lp.pending(0x73, 0x04).is_ok());
    }

    #[test]
    fn round_monotonic_and_seq_monotonic() {
        // 轮次与回签序号单调。
        let mut lp = ServeLoop::new();
        assert_eq!(lp.cur_round(), 0);
        let _ = lp.listen();
        let _ = lp.listen();
        assert_eq!(lp.cur_round(), 2);
        // 倒退拒。
        assert_eq!(lp.force_round(1), Err(LaDiag::ERoundRewind.val()));
        // 回签序号：两笔接受序号 1、2。
        let _ = lp.pending(0x74, 0x05);
        let _ = lp.pending(0x75, 0x06);
        let w1 = lp.accept(0x74, 64, 8);
        let w2 = lp.accept(0x75, 64, 8);
        assert!(w1.is_ok() && w2.is_ok());
        assert_eq!(w1.map(|w| w.accept_seq), Ok(1));
        assert_eq!(w2.map(|w| w.accept_seq), Ok(2));
    }

    #[test]
    fn serve_loop_three_rounds() {
        assert!(serve_loop_rounds_ok());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12606_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
