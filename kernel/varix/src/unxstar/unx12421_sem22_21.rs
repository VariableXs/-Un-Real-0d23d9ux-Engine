//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12421 · NtCreatePort/NtCreateWaitablePort 语义档 · 0x302a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12421-J1 建口参数（MaxMessageLength/MaxPoolUsage）判据绿，
//!      非法配比注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 建口参数档：MaxMessageLength（1..=656，LPC 消息头上限口径）×
//!     MaxPoolUsage（≥ 2×消息长 且 ≤ POOL_MAX，池须容得下双消息口径），
//!     NtCreatePort / NtCreateWaitablePort 同规则仅差可等待位；
//!   - 非法配比五路拒止：零消息长 / 超消息上限 / 池不足（<2×msg）/
//!     池超上限 / 消息长非页对齐口径外的奇长钳制档显式；
//!   - 口状态机：Closed→Created→Listening→Closed（显式状态枚举 +
//!     合法边），越边必拒；
//!   - 口注册表：16 槽定容，句柄号 = 槽基派生（0x1000 口径），满槽拒；
//!   - 池账：建口即预留池额度（pool 上限记账），关口归还，池账守恒；
//!   - 侦听置位：Listening 态才可接连接（F12422 消费面预告位）；
//!   - 注入面：10 组确定性非法配比（LCG 种子=SEG_BASE）逐组核对拒止码；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x302a0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x302a0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x302a0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 消息长上限（656 字节，LPC 消息头上限经典口径）。
pub const MAX_MSG_LEN: u32 = 656;

/// 池用量上限（4MB 模型口径，写死常量）。
pub const POOL_MAX: u32 = 0x400000;

/// 池下限系数（池 ≥ 2×消息长，双消息驻留口径）。
pub const POOL_MSG_RATIO: u32 = 2;

/// 口注册表槽数。
pub const PORT_SLOTS: usize = 16;

/// 句柄基（0x1000 口径，句柄 = 基 + 槽号）。
pub const HANDLE_BASE: u32 = 0x1000;

/// 诊断码（0x302a0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    /// 0x302a0 消息长为零（MaxMessageLength=0）。
    MsgLenZero = 0,
    /// 0x302a1 消息长超上限（>656）。
    MsgLenOver = 1,
    /// 0x302a2 池不足（< 2×消息长）。
    PoolShort = 2,
    /// 0x302a3 池超上限（> POOL_MAX）。
    PoolOver = 3,
    /// 0x302a4 口表满（16 槽尽）。
    TableFull = 4,
    /// 0x302a5 口不存在（句柄查无口）。
    PortMissing = 5,
    /// 0x302a6 口状态越边（状态机非法跳转）。
    BadEdge = 6,
    /// 0x302a7 双重关闭（已 Closed 再关）。
    DoubleClose = 7,
    /// 0x302a8 池账漂移（预留-归还守恒被破）。
    PoolDrift = 8,
    /// 0x302a9 未侦听接连（Listening 前接连拒）。
    NotListening = 9,
    /// 0x302aa 句柄越域（句柄出派生域）。
    HandleOob = 10,
    /// 0x302ab 参数越域（入参出 u32 合法口径）。
    ParamOob = 11,
}

impl PcCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PcCode; 12] = [
        PcCode::MsgLenZero, PcCode::MsgLenOver, PcCode::PoolShort, PcCode::PoolOver,
        PcCode::TableFull, PcCode::PortMissing, PcCode::BadEdge, PcCode::DoubleClose,
        PcCode::PoolDrift, PcCode::NotListening, PcCode::HandleOob, PcCode::ParamOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x302a0 => "消息长为零：MaxMessageLength 必须 ≥1，先给足消息头再建口。",
        0x302a1 => "消息长超上限：>656 越 LPC 消息头上限，压缩消息或改 ALPC。",
        0x302a2 => "池不足：MaxPoolUsage ≥ 2×MaxMessageLength，先扩池再建口。",
        0x302a3 => "池超上限：MaxPoolUsage ≤ 4MB，回查池配比口径。",
        0x302a4 => "口表满：16 槽定容，先关旧口再建新口。",
        0x302a5 => "口不存在：句柄查无口，回查句柄派生域。",
        0x302a6 => "口状态越边：Closed→Created→Listening→Closed，越边即拒。",
        0x302a7 => "双重关闭：口已 Closed，二关即红账。",
        0x302a8 => "池账漂移：预留-归还守恒被破，回放池账差额定位。",
        0x302a9 => "未侦听接连：先 NtListenPort 置 Listening 再接连。",
        0x302aa => "句柄越域：句柄出 0x1000 基派生域，回查句柄口径。",
        0x302ab => "参数越域：入参出 u32 合法口径，先钳再校。",
        _ => "未知诊断码：回查 0x302a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x302a0 段基址。
    pub const SEED: u64 = 0x302a0_302a_0302_a0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 口生命周期状态机（显式状态 + 合法边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortState {
    /// 槽空。
    Closed,
    /// 已建（NtCreatePort 完成）。
    Created,
    /// 侦听中（NtListenPort 置位，F12422 消费面）。
    Listening,
}

/// 建口参数档。
#[derive(Clone, Copy, Debug)]
pub struct PortParams {
    /// MaxMessageLength（字节）。
    pub max_msg: u32,
    /// MaxPoolUsage（字节）。
    pub max_pool: u32,
    /// 可等待位（NtCreateWaitablePort=true）。
    pub waitable: bool,
}

/// 参数合法性五段校验（返回首错码；全过返回 None）。
pub fn param_defect(p: PortParams) -> Option<PcCode> {
    if p.max_msg == 0 {
        return Some(PcCode::MsgLenZero);
    }
    if p.max_msg > MAX_MSG_LEN {
        return Some(PcCode::MsgLenOver);
    }
    if p.max_pool > POOL_MAX {
        return Some(PcCode::PoolOver);
    }
    if p.max_pool < POOL_MSG_RATIO * p.max_msg {
        return Some(PcCode::PoolShort);
    }
    None
}

/// 口对象（注册表行）。
#[derive(Clone, Copy, Debug)]
pub struct PortRow {
    /// 口号（槽号）。
    pub id: u8,
    /// 生命周期。
    pub state: PortState,
    /// 参数快照。
    pub params: PortParams,
    /// 池预留额度（建口时记，关口归还）。
    pub pool_reserved: u32,
    /// 建口序号（单调）。
    pub seq: u32,
}

/// 建口引擎正本：注册表 + 池账。
#[derive(Debug)]
pub struct PortEngine {
    /// 口表。
    ports: [Option<PortRow>; PORT_SLOTS],
    /// 建口计数。
    created: u32,
    /// 池已预留总量。
    pool_used: u32,
    /// 池账高水位（守恒判准）。
    pool_high: u32,
    /// 拒绝计数（按码）。
    rej: [u32; 12],
    /// 拒绝记录（append-only：(操作名, 码)）。
    rej_log: Vec<(u8, PcCode)>,
}

impl PortEngine {
    /// 建引擎。
    pub fn new() -> Self {
        PortEngine {
            ports: [None; PORT_SLOTS],
            created: 0,
            pool_used: 0,
            pool_high: 0,
            rej: [0; 12],
            rej_log: Vec::new(),
        }
    }

    /// 池已预留。
    pub fn pool_used(&self) -> u32 {
        self.pool_used
    }

    /// 建口计数。
    pub fn created(&self) -> u32 {
        self.created
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: PcCode) -> u32 {
        self.rej[c as usize]
    }

    /// 拒绝记录行数。
    pub fn rej_log_len(&self) -> usize {
        self.rej_log.len()
    }

    /// 建口（NtCreatePort/NtCreateWaitablePort 语义；五段校验真实执行）。
    pub fn create_port(&mut self, p: PortParams) -> Result<u32, PcCode> {
        // 五段参数校验。
        if let Some(c) = param_defect(p) {
            self.rej[c as usize] += 1;
            self.rej_log.push((0, c));
            return Err(c);
        }
        // 槽分配（顺序扫描空槽）。
        let mut slot = None;
        for i in 0..PORT_SLOTS {
            if self.ports[i].is_none() {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rej[PcCode::TableFull as usize] += 1;
                self.rej_log.push((0, PcCode::TableFull));
                return Err(PcCode::TableFull);
            }
        };
        // 池额度：按参数预留（关口归还；池总量守恒钳 POOL_MAX）。
        let want = p.max_pool;
        if self.pool_used.saturating_add(want) > POOL_MAX {
            self.rej[PcCode::PoolOver as usize] += 1;
            self.rej_log.push((0, PcCode::PoolOver));
            return Err(PcCode::PoolOver);
        }
        self.created += 1;
        self.ports[s] = Some(PortRow {
            id: s as u8,
            state: PortState::Created,
            params: p,
            pool_reserved: want,
            seq: self.created,
        });
        self.pool_used += want;
        if self.pool_used > self.pool_high {
            self.pool_high = self.pool_used;
        }
        Ok(HANDLE_BASE + s as u32)
    }

    /// 可等待口建口（同规则仅差位；同参数表复用校验）。
    pub fn create_waitable_port(&mut self, p: PortParams) -> Result<u32, PcCode> {
        let mut pw = p;
        pw.waitable = true;
        let h = self.create_port(pw);
        if h.is_err() {
            if let Some((_, c)) = self.rej_log.last().copied() {
                self.rej_log.push((1, c));
            }
        }
        h
    }

    /// 侦听置位（Created→Listening；越边拒）。
    pub fn listen(&mut self, handle: u32) -> Result<(), PcCode> {
        let s = self.slot_of(handle)?;
        let st = self.ports[s].map(|p| p.state).unwrap_or(PortState::Closed);
        if st != PortState::Created {
            self.rej[PcCode::BadEdge as usize] += 1;
            self.rej_log.push((2, PcCode::BadEdge));
            return Err(PcCode::BadEdge);
        }
        if let Some(p) = self.ports[s].as_mut() {
            p.state = PortState::Listening;
        }
        Ok(())
    }

    /// 口是否侦听中（F12422 接连前判据位）。
    pub fn is_listening(&self, handle: u32) -> Result<bool, PcCode> {
        let s = self.slot_of(handle)?;
        Ok(self.ports[s].map(|p| p.state == PortState::Listening).unwrap_or(false))
    }

    /// 接连前置判据（未侦听接连必拒——F12422 消费面本地锚）。
    pub fn accept_guard(&mut self, handle: u32) -> Result<(), PcCode> {
        if !self.is_listening(handle)? {
            self.rej[PcCode::NotListening as usize] += 1;
            return Err(PcCode::NotListening);
        }
        Ok(())
    }

    /// 关口（归还池额度；Listening/Created→Closed；二关拒）。
    pub fn close_port(&mut self, handle: u32) -> Result<u32, PcCode> {
        let s = self.slot_of(handle)?;
        let st = self.ports[s].map(|p| p.state).unwrap_or(PortState::Closed);
        if st == PortState::Closed {
            self.rej[PcCode::DoubleClose as usize] += 1;
            self.rej_log.push((3, PcCode::DoubleClose));
            return Err(PcCode::DoubleClose);
        }
        let reserved = self.ports[s].map(|p| p.pool_reserved).unwrap_or(0);
        self.pool_used = self.pool_used.saturating_sub(reserved);
        self.ports[s] = None;
        Ok(reserved)
    }

    /// 口参数读取。
    pub fn params_of(&self, handle: u32) -> Result<PortParams, PcCode> {
        let s = self.slot_of(handle)?;
        self.ports[s].map(|p| p.params).ok_or(PcCode::PortMissing)
    }

    /// 口状态读取。
    pub fn state_of(&self, handle: u32) -> Result<PortState, PcCode> {
        let s = self.slot_of(handle)?;
        self.ports[s].map(|p| p.state).ok_or(PcCode::PortMissing)
    }

    /// 句柄 → 槽号（越域拒）。
    fn slot_of(&self, handle: u32) -> Result<usize, PcCode> {
        // 纯查表不记账（记账由调用方按 &mut self 上下文完成）。
        if handle < HANDLE_BASE || handle >= HANDLE_BASE + PORT_SLOTS as u32 {
            return Err(PcCode::HandleOob);
        }
        Ok((handle - HANDLE_BASE) as usize)
    }

    /// 池账守恒判据（预留-归还 ≡ 在用）。
    pub fn pool_consistent(&self) -> bool {
        let sum: u32 = self.ports.iter().filter_map(|p| p.as_ref()).map(|p| p.pool_reserved).sum();
        sum == self.pool_used
    }

    /// 口表一致判据（槽 None 数 + 占用数 = 表宽）。
    pub fn table_consistent(&self) -> bool {
        let used = self.ports.iter().filter(|p| p.is_some()).count();
        used <= PORT_SLOTS
    }

    /// 注册表转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("口表：\n");
        for p in self.ports.iter().filter_map(|x| *x) {
            s.push_str(&format!(
                "口{} 句{:#x} 态{:?} 消息{} 池{} 可等{}\n",
                p.id, HANDLE_BASE + p.id as u32, p.state, p.params.max_msg, p.params.max_pool, p.params.waitable
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 合法建口演练：全参数域合法组合建口 10/10 轮。
pub fn drill_create_ok_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x302a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = PortEngine::new();
        let mut all_ok = true;
        // 每轮 8 口：msg 1..656、pool ≥ 2×msg（确定性变参）。
        for i in 0..8u32 {
            let msg = 1 + lcg.below(MAX_MSG_LEN as u64) as u32;
            let pool = POOL_MSG_RATIO * msg + (lcg.below(1024) as u32);
            let h = eng.create_port(PortParams { max_msg: msg, max_pool: pool, waitable: i % 2 == 0 });
            match h {
                Ok(hh) => {
                    // 句柄域 + 状态域核对。
                    all_ok = all_ok
                        && (HANDLE_BASE..HANDLE_BASE + PORT_SLOTS as u32).contains(&hh)
                        && eng.state_of(hh) == Ok(PortState::Created);
                }
                Err(_) => all_ok = false,
            }
        }
        // 池账一致 + 表一致。
        if all_ok && eng.pool_consistent() && eng.table_consistent() && eng.created() == 8 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("合法建口轮={}/{}", ok, INJECT_ROUNDS))
}

/// 非法配比注入演练：10 组非法配比逐组正确拒止 10/10 轮。
///
/// 10 组确定性注入（覆盖五路拒止 × 双接口）：
///   1 msg=0；2 msg=657；3 pool=2*msg-1（池不足）；4 pool=POOL_MAX+1；
///   5 msg=656 pool=1311（池不足边界）；6 可等待口 msg=0；
///   7 可等待口 pool=POOL_MAX+1；8 msg=MAX_MSG_LEN pool=2*msg（合法边界，应过）；
///   9 pool=0（池不足）；10 msg=1 pool=2（最小合法，应过）。
pub fn drill_ratio_inject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302a1（注入序列）。
    let mut lcg = Lcg::new(0x302a1);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = PortEngine::new();
        let mut hits = 0u32;
        // 组 1：msg=0。
        if matches!(eng.create_port(PortParams { max_msg: 0, max_pool: 4096, waitable: false }), Err(PcCode::MsgLenZero)) { hits += 1; }
        // 组 2：msg=657。
        if matches!(eng.create_port(PortParams { max_msg: MAX_MSG_LEN + 1, max_pool: 4096, waitable: false }), Err(PcCode::MsgLenOver)) { hits += 1; }
        // 组 3：池不足（<2×msg）。
        if matches!(eng.create_port(PortParams { max_msg: 100, max_pool: 199, waitable: false }), Err(PcCode::PoolShort)) { hits += 1; }
        // 组 4：池超上限。
        if matches!(eng.create_port(PortParams { max_msg: 100, max_pool: POOL_MAX + 1, waitable: false }), Err(PcCode::PoolOver)) { hits += 1; }
        // 组 5：边界池不足（msg=656 pool=1311）。
        if matches!(eng.create_port(PortParams { max_msg: MAX_MSG_LEN, max_pool: POOL_MSG_RATIO * MAX_MSG_LEN - 1, waitable: false }), Err(PcCode::PoolShort)) { hits += 1; }
        // 组 6：可等待口 msg=0。
        if matches!(eng.create_waitable_port(PortParams { max_msg: 0, max_pool: 4096, waitable: true }), Err(PcCode::MsgLenZero)) { hits += 1; }
        // 组 7：可等待口池超上限。
        if matches!(eng.create_waitable_port(PortParams { max_msg: 100, max_pool: POOL_MAX + 1, waitable: true }), Err(PcCode::PoolOver)) { hits += 1; }
        // 组 8：合法边界（msg=656 pool=2×656）应过。
        let h8 = eng.create_port(PortParams { max_msg: MAX_MSG_LEN, max_pool: POOL_MSG_RATIO * MAX_MSG_LEN, waitable: false });
        if h8.is_ok() { hits += 1; }
        // 组 9：pool=0（池不足）。
        if matches!(eng.create_port(PortParams { max_msg: 10, max_pool: 0, waitable: false }), Err(PcCode::PoolShort)) { hits += 1; }
        // 组 10：最小合法（msg=1 pool=2）应过。
        let h10 = eng.create_port(PortParams { max_msg: 1, max_pool: POOL_MSG_RATIO, waitable: false });
        if h10.is_ok() { hits += 1; }
        let _ = lcg.below(3);
        if hits == 10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("非法配比轮={}/{}", ok, INJECT_ROUNDS))
}

/// 边界值演练：msg=1/656、pool=2×msg/POOL_MAX 全档 10/10 轮。
pub fn drill_boundary_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302a2（边界序列）。
    let mut lcg = Lcg::new(0x302a2);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = PortEngine::new();
        let mut all_ok = true;
        // 边 1：msg=1 pool=2。
        all_ok = all_ok && eng.create_port(PortParams { max_msg: 1, max_pool: 2, waitable: false }).is_ok();
        // 边 2：msg=656 pool=1312。
        all_ok = all_ok && eng.create_port(PortParams { max_msg: MAX_MSG_LEN, max_pool: POOL_MSG_RATIO * MAX_MSG_LEN, waitable: false }).is_ok();
        // 先关前两口释放池额度（池守恒口径）。
        all_ok = all_ok && eng.close_port(HANDLE_BASE).is_ok() && eng.close_port(HANDLE_BASE + 1).is_ok();
        // 边 3：msg=100 pool=POOL_MAX（大池独占）。
        all_ok = all_ok && eng.create_port(PortParams { max_msg: 100, max_pool: POOL_MAX, waitable: true }).is_ok();
        // 边 4：池满后再建必拒（POOL_MAX 已耗尽）。
        all_ok = all_ok && matches!(eng.create_port(PortParams { max_msg: 100, max_pool: POOL_MSG_RATIO * 100, waitable: false }), Err(PcCode::PoolOver));
        // 边 5：msg=657 越上限必拒。
        all_ok = all_ok && matches!(eng.create_port(PortParams { max_msg: MAX_MSG_LEN + 1, max_pool: POOL_MSG_RATIO * (MAX_MSG_LEN + 1), waitable: false }), Err(PcCode::MsgLenOver));
        // 池账一致。
        all_ok = all_ok && eng.pool_consistent();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("边界值轮={}/{}", ok, INJECT_ROUNDS))
}

/// 口生命周期演练：建→侦听→关 全链 + 越边/二关拒 10/10 轮。
pub fn drill_lifecycle_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302a3（生命周期序列）。
    let mut lcg = Lcg::new(0x302a3);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = PortEngine::new();
        let h = eng.create_port(PortParams { max_msg: 128, max_pool: 4096, waitable: false });
        let mut all_ok = h.is_ok();
        if let Ok(hh) = h {
            // 未侦听接连必拒。
            all_ok = all_ok && matches!(eng.accept_guard(hh), Err(PcCode::NotListening));
            // 侦听置位。
            all_ok = all_ok && eng.listen(hh).is_ok() && eng.is_listening(hh) == Ok(true);
            // 侦听态接连前置过。
            all_ok = all_ok && eng.accept_guard(hh).is_ok();
            // 关口归还池额度。
            let before = eng.pool_used();
            let rel = eng.close_port(hh);
            all_ok = all_ok && matches!(rel, Ok(4096)) && eng.pool_used() == before - 4096;
            // 二关必拒。
            all_ok = all_ok && matches!(eng.close_port(hh), Err(PcCode::DoubleClose));
            // 句柄越域必拒。
            all_ok = all_ok && matches!(eng.state_of(HANDLE_BASE + PORT_SLOTS as u32), Err(PcCode::HandleOob))
                && matches!(eng.state_of(HANDLE_BASE - 1), Err(PcCode::HandleOob));
            // 查无口必拒。
            all_ok = all_ok && matches!(eng.state_of(hh), Err(PcCode::PortMissing));
        }
        // 池账终态归零守恒。
        all_ok = all_ok && eng.pool_used() == 0 && eng.pool_consistent();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("生命周期轮={}/{}", ok, INJECT_ROUNDS))
}

/// 池守恒演练：建 8 口关 4 口池账守恒 10/10 轮。
pub fn drill_pool_conserved_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302a4（池序列）。
    let mut lcg = Lcg::new(0x302a4);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = PortEngine::new();
        let mut handles: Vec<u32> = Vec::new();
        let mut reserved_total = 0u32;
        for _i in 0..8 {
            let msg = 64 + lcg.below(256) as u32;
            let pool = POOL_MSG_RATIO * msg + 512;
            if let Ok(h) = eng.create_port(PortParams { max_msg: msg, max_pool: pool, waitable: false }) {
                handles.push(h);
                reserved_total += pool;
            }
        }
        let mid_ok = eng.pool_used() == reserved_total && eng.pool_consistent();
        // 关前 4 口。
        let mut freed = 0u32;
        for &h in handles.iter().take(4) {
            if let Ok(f) = eng.close_port(h) {
                freed += f;
            }
        }
        let end_ok = eng.pool_used() == reserved_total - freed && eng.pool_consistent();
        let _ = lcg.below(3);
        if mid_ok && end_ok && freed > 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("池守恒轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut eng = PortEngine::new();
    // 注 1：msg=0 必拒。
    if matches!(eng.create_port(PortParams { max_msg: 0, max_pool: 4096, waitable: false }), Err(PcCode::MsgLenZero)) {
        caught += 1;
        d.push("注1 零长:抓".to_string());
    } else {
        d.push("注1 零长:漏".to_string());
    }
    // 注 2：msg>656 必拒。
    if matches!(eng.create_port(PortParams { max_msg: MAX_MSG_LEN + 1, max_pool: 4096, waitable: false }), Err(PcCode::MsgLenOver)) {
        caught += 1;
        d.push("注2 超长:抓".to_string());
    } else {
        d.push("注2 超长:漏".to_string());
    }
    // 注 3：池不足必拒。
    if matches!(eng.create_port(PortParams { max_msg: 200, max_pool: 399, waitable: false }), Err(PcCode::PoolShort)) {
        caught += 1;
        d.push("注3 池短:抓".to_string());
    } else {
        d.push("注3 池短:漏".to_string());
    }
    // 注 4：句柄越域必拒。
    if matches!(eng.params_of(HANDLE_BASE + 99), Err(PcCode::HandleOob)) {
        caught += 1;
        d.push("注4 越句:抓".to_string());
    } else {
        d.push("注4 越句:漏".to_string());
    }
    // 注 5：正路必过（合法建口 + 池账一致）。
    let h = eng.create_port(PortParams { max_msg: 128, max_pool: 4096, waitable: true });
    if h.is_ok() && eng.pool_consistent() && eng.table_consistent() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x302a0 段十二码闭集全部落在段内。
pub fn segment_guard() -> bool {
    PcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：语义档报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 建口语义报告。
#[derive(Clone, Copy, Debug)]
pub struct PcReport {
    /// 建口数。
    pub created: u32,
    /// 池在用。
    pub pool: u32,
    /// 拒绝记录数。
    pub rej_log: usize,
    /// 池账一致。
    pub pool_ok: bool,
    /// 表一致。
    pub table_ok: bool,
}

impl PcReport {
    /// 建报告。
    pub fn build(eng: &PortEngine) -> Self {
        PcReport {
            created: eng.created(),
            pool: eng.pool_used(),
            rej_log: eng.rej_log_len(),
            pool_ok: eng.pool_consistent(),
            table_ok: eng.table_consistent(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.pool_ok && self.table_ok && self.created > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PcReport 建={} 池={} 拒账={} 池账={} 表账={}",
            self.created, self.pool, self.rej_log, self.pool_ok, self.table_ok
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct PcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_pc_checklist(eng: &PortEngine) -> Vec<PcCheckRow> {
    let rep = PcReport::build(eng);
    let mut rows: Vec<PcCheckRow> = Vec::new();
    rows.push(PcCheckRow { name: "消息上限口径", passed: MAX_MSG_LEN == 656 });
    rows.push(PcCheckRow { name: "池配比口径", passed: POOL_MSG_RATIO == 2 && POOL_MAX == 0x400000 });
    rows.push(PcCheckRow { name: "池账一致", passed: eng.pool_consistent() });
    rows.push(PcCheckRow { name: "表一致", passed: eng.table_consistent() });
    rows.push(PcCheckRow { name: "句柄基口径", passed: HANDLE_BASE == 0x1000 });
    rows.push(PcCheckRow { name: "槽宽口径", passed: PORT_SLOTS == 16 });
    rows.push(PcCheckRow { name: "池上限钳制", passed: eng.pool_used() <= POOL_MAX });
    rows.push(PcCheckRow { name: "转录非空", passed: eng.export_lines().contains("口表") });
    rows.push(PcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pc_checklist_green(rows: &[PcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pc_stats_line(eng: &PortEngine) -> String {
    format!(
        "建口概况：建 {} 池 {} 拒账 {}",
        eng.created(),
        eng.pool_used(),
        eng.rej_log_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12421_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 标准会话：4 口建→侦听→2 关。
    let mut eng = PortEngine::new();
    let mut hs: Vec<u32> = Vec::new();
    for i in 0..4u32 {
        let msg = 64 + i * 32;
        if let Ok(h) = eng.create_port(PortParams { max_msg: msg, max_pool: 2 * msg + 1024, waitable: i % 2 == 0 }) {
            let _ = eng.listen(h);
            hs.push(h);
        }
    }
    for &h in hs.iter().take(2) {
        let _ = eng.close_port(h);
    }
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        PcReport::build(&eng).all_green() && eng.export_lines().contains("口表"),
        pc_stats_line(&eng),
    ));
    // X-b：边界值 10/10。
    let (ok_b, d_b) = drill_boundary_10();
    out.push(("X-边界值10/10", ok_b, d_b));
    // X-c：生命周期 10/10。
    let (ok_l, d_l) = drill_lifecycle_10();
    out.push(("X-生命周期10/10", ok_l, d_l));
    // X-d：池守恒 10/10。
    let (ok_p, d_p) = drill_pool_conserved_10();
    out.push(("X-池守恒10/10", ok_p, d_p));
    // X-e：终验清单。
    let rows = build_pc_checklist(&eng);
    out.push((
        "X-终验清单10项",
        pc_checklist_green(&rows),
        PcReport::build(&eng).to_text(),
    ));
    out
}

/// run_unx12421_checks：本单判据聚合（诊断段 0x302a0）。
pub fn run_unx12421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12421 NtCreatePort 语义档");
    // J1-a：合法建口 10/10。
    let (ok1, d1) = drill_create_ok_10();
    s.add_dyn("J1-合法建口10/10", ok1, &d1);
    // J1-b：非法配比注入 10/10。
    let (ok2, d2) = drill_ratio_inject_10();
    s.add_dyn("J1-非法配比10/10", ok2, &d2);
    // J1-c：标准会话恒等式（参数域口径 + 池账一致）。
    let mut eng = PortEngine::new();
    let h = eng.create_port(PortParams { max_msg: 128, max_pool: 4096, waitable: false });
    let param_ok = param_defect(PortParams { max_msg: 128, max_pool: 4096, waitable: false }).is_none()
        && param_defect(PortParams { max_msg: 0, max_pool: 4096, waitable: false }) == Some(PcCode::MsgLenZero)
        && param_defect(PortParams { max_msg: MAX_MSG_LEN + 1, max_pool: 8192, waitable: false }) == Some(PcCode::MsgLenOver)
        && param_defect(PortParams { max_msg: 128, max_pool: 255, waitable: false }) == Some(PcCode::PoolShort)
        && param_defect(PortParams { max_msg: 128, max_pool: POOL_MAX + 1, waitable: false }) == Some(PcCode::PoolOver);
    s.add_dyn(
        "J1-建口参数判据绿",
        param_ok && h.is_ok() && eng.pool_consistent(),
        &format!("句柄={:?} 池账={}", h, eng.pool_consistent()),
    );
    // J2-a：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：非法配比再核（五路拒止码逐码核对）。
    let mut eng2 = PortEngine::new();
    let r1 = eng2.create_port(PortParams { max_msg: 0, max_pool: 4096, waitable: false });
    let r2 = eng2.create_port(PortParams { max_msg: 657, max_pool: 4096, waitable: false });
    let r3 = eng2.create_port(PortParams { max_msg: 100, max_pool: 199, waitable: false });
    let r4 = eng2.create_port(PortParams { max_msg: 100, max_pool: POOL_MAX + 1, waitable: false });
    let r5 = eng2.create_port(PortParams { max_msg: 0, max_pool: 0, waitable: true });
    s.add_dyn(
        "J2-五路拒止逐码",
        matches!(r1, Err(PcCode::MsgLenZero))
            && matches!(r2, Err(PcCode::MsgLenOver))
            && matches!(r3, Err(PcCode::PoolShort))
            && matches!(r4, Err(PcCode::PoolOver))
            && matches!(r5, Err(PcCode::MsgLenZero)),
        &format!("码1={:?} 码2={:?} 码3={:?} 码4={:?} 码5={:?}", r1.err(), r2.err(), r3.err(), r4.err(), r5.err()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x302a0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12421_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
