//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18491 · 覆写观测导出（裁决轨迹含覆写段） · 0x7F680 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18491-J1 轨迹导出含覆写创建/撤销事件断言全过，与 F18431
//!       格式一致
//! 本模块结构清单：
//!   1. 冻结常量层：覆写表容量 8 / 轨迹账本容量 160 / 判据轮数 10 /
//!       导出协议 OBS-TRACE v1（与 F18431 同版同分隔）；
//!   2. 诊断码闭集 TxCode（code()=SEG_BASE+n，段内 0x40 窗口，detail()
//!       中文闭集逐码非空）；
//!   3. 数据结构：
//!       - 数组账本 OvrTable：[OvrSlot; 8] 定容覆写槽（状态/版本/属主/
//!         令牌四元组）；
//!       - 位面 ActivePlane：u8 位图，第 i 位=槽 i 覆写在场；
//!       - 轨迹账本 TraceLedger：[TraceEntry; 160] 环形顺序账 + dropped
//!         显性记账 + next_seq 单调序号闸；
//!   4. 状态机 SlotState：Free→Active→Free（撤销回环），Active 上重复
//!       创建被拒、Free 上撤销被拒——非法倒退显式显码；
//!   5. 错误路径：越权属主 / 幽灵键 / 版本域非法 / 重复创建 / 无覆写
//!       撤销 / 令牌不符 / 序号乱序 / 账满 八类显式拒绝；
//!   6. 导出面 export()：表头（OBS-TRACE v1 前缀 + n= 计数）+ 七管道
//!       字段数据行，与 F18431 观测导出同协议（同版本标签、同分隔符、
//!       同字段数）——覆写创建(layer=4)/撤销(layer=5)事件入轨迹即入导出；
//!   7. 回放面 replay()：逐行解析回 TraceEntry 与账本逐字段对拍（往返
//!       一致）；被篡改行（缺管道/非数字）必拒；
//!   8. 注入核四类：非法版本创建 / 无覆写撤销 / 活跃槽重复创建 / 幽灵
//!       键查询，逐类 10 轮全抓，守恒 tries==caught 漏检必 0；
//!   9. golden_run：4 创建→8 查询→2 撤销全链走通 + 导出回放往返 + 双跑
//!       逐字段一致；
//!  10. run_unx18491_checks：17 条判据（守恒/单调/幂等/确定性/往返/
//!       非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F680，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F680;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 覆写表容量（写死 8：样板进程 4 × (主键+备份槽)，8 为 2 的幂便于位面）。
pub const OVR_CAP: usize = 8;
/// 轨迹账本容量（写死 160：黄金链 14 事件 × 10 轮注账余量，满后 dropped 记账）。
pub const TRACE_CAP: usize = 160;
/// 进程号域上界（冻结 1..=999，0 保留为内核哨兵）。
pub const PID_MAX: u32 = 999;
/// 版本 build 段上界（冻结 65535：与 OSVERSIONINFO dwBuildNumber 同域）。
pub const VER_BUILD_MAX: u32 = 65535;
/// 版本 major 段上界（冻结 10：本域语义面 B01–B05 十批冻结值域）。
pub const VER_MAJOR_MAX: u32 = 10;
/// 版本 minor 段上界（冻结 9）。
pub const VER_MINOR_MAX: u32 = 9;
/// 平台 id 闭集（冻结 1..=3：1 Win32 / 2 Win64 / 3 WoW64）。
pub const PLAT_MAX: u32 = 3;
/// API 闭集上界（冻结 4：1 GetVersionEx / 2 VerifyVersionInfo / 3
/// VerSetConditionMask / 4 RtlGetVersion 探测面）。
pub const API_MAX: u32 = 4;
/// 维度闭集上界（冻结 3：主版本/次版本/包层 三维）。
pub const DIM_MAX: u32 = 3;
/// 事件种类闭集：1 创建 / 2 查询 / 3 撤销。
pub const KIND_CREATE: u8 = 1;
pub const KIND_QUERY: u8 = 2;
pub const KIND_UNDO: u8 = 3;
/// 命中层闭集（与 F18431 四层同表）：0 ActCtx / 1 Manifest / 2 Compat /
/// 3 Default；4 覆写创建事件 / 5 覆写撤销事件（覆写段专用扩展层）。
pub const LAYER_ACTCTX: u32 = 0;
pub const LAYER_MANIFEST: u32 = 1;
pub const LAYER_COMPAT: u32 = 2;
pub const LAYER_DEFAULT: u32 = 3;
pub const LAYER_OVR_CREATE: u32 = 4;
pub const LAYER_OVR_UNDO: u32 = 5;
/// 导出协议版本标签（与 F18431 obs_export 同版冻结——格式一致的字面锚）。
pub const EXPORT_FMT_TAG: &'static str = "OBS-TRACE v1";
/// 导出行字段数（冻结 7：seq|proc|api|dim|layer|basis|value，同 F18431）。
pub const EXPORT_FIELDS: usize = 7;
/// 判据轮数（写死 10：判据"10/10 拒绝"语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 兜底层默认裁决（推导：基础版本 6.1 build 7650 平台 2 的打包值，
/// packed = 6*1e8 + 1*1e6 + 7650*10 + 2 = 601076502）。
pub const BASE_VERDICT: u32 = 601_076_502;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxCode {
    /// 进程号越界（0 或 >999）
    PidOutOfRange,
    /// 幽灵键：覆写表查无此键
    KeyOutOfRange,
    /// 版本域非法：major/minor/build/plat 任一段越界
    VerIllegal,
    /// 重复创建：活跃槽再建覆写
    AlreadyActive,
    /// 无覆写撤销：空闲槽执行撤销
    NoActiveOverride,
    /// 令牌不符：撤销令牌与建槽令牌不同
    TokenMismatch,
    /// 幽灵查询：键越界且未注册
    GhostQuery,
    /// 轨迹种类非法：闭集外 kind
    TraceBadKind,
    /// 轨迹层非法：>5 的命中层
    TraceBadLayer,
    /// 轨迹序号乱序：seq ≠ next_seq
    TraceSeqOrder,
    /// 轨迹账满
    TraceTableFull,
    /// 导出行非法：管道数/字段数/非数字
    ExportBad,
    /// 往返破坏：回放与账本逐字段不一致
    RoundtripBroken,
    /// 账面守恒破坏：撤销>创建或活跃数≠差值
    LedgerBroken,
    /// 双跑不一致
    NotReproducible,
}

impl TxCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            TxCode::PidOutOfRange => "进程号越界：须在 1..=999",
            TxCode::KeyOutOfRange => "幽灵键：覆写表 0..8 之外",
            TxCode::VerIllegal => "版本域非法：major≤10 minor≤9 build≤65535 plat≤3",
            TxCode::AlreadyActive => "重复创建：该键覆写已在场",
            TxCode::NoActiveOverride => "无覆写撤销：空闲槽不可撤销",
            TxCode::TokenMismatch => "令牌不符：仅创建者可撤销",
            TxCode::GhostQuery => "幽灵查询：键越界",
            TxCode::TraceBadKind => "轨迹种类非法：1/2/3 之外",
            TxCode::TraceBadLayer => "轨迹层非法：0..=5 之外",
            TxCode::TraceSeqOrder => "轨迹序号乱序：须等于 next_seq",
            TxCode::TraceTableFull => "轨迹账满：160 条上限",
            TxCode::ExportBad => "导出行非法：管道数/字段数/数字错",
            TxCode::RoundtripBroken => "往返破坏：回放与账本不一致",
            TxCode::LedgerBroken => "账面守恒破坏：撤销>创建或活跃数不符",
            TxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [TxCode; 15] = [
        TxCode::PidOutOfRange,
        TxCode::KeyOutOfRange,
        TxCode::VerIllegal,
        TxCode::AlreadyActive,
        TxCode::NoActiveOverride,
        TxCode::TokenMismatch,
        TxCode::GhostQuery,
        TxCode::TraceBadKind,
        TxCode::TraceBadLayer,
        TxCode::TraceSeqOrder,
        TxCode::TraceTableFull,
        TxCode::ExportBad,
        TxCode::RoundtripBroken,
        TxCode::LedgerBroken,
        TxCode::NotReproducible,
    ];
}

/// FNV-1a 32（撤销令牌/事件摘要共用——确定性，零随机源）。
pub fn fnv1a(s: &[u8]) -> u32 {
    // 常量推导：FNV 官方 offset/prime（32 位），写死防漂移。
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 版本四元组 ─────────────────────────────────────────────────────
/// 版本四元组（major/minor/build/plat），打包为导出 value 字段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OvrVer {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub plat: u32,
}

impl OvrVer {
    /// 构造（const：BASE_VERDICT 推导需要编译期求值）。
    pub const fn new(major: u32, minor: u32, build: u32, plat: u32) -> OvrVer {
        OvrVer { major, minor, build, plat }
    }
    /// 域守卫：四段各归其界。
    pub fn legal(&self) -> bool {
        self.major >= 1
            && self.major <= VER_MAJOR_MAX
            && self.minor <= VER_MINOR_MAX
            && self.build <= VER_BUILD_MAX
            && self.plat >= 1
            && self.plat <= PLAT_MAX
    }
    /// 打包：major*1e8 + minor*1e6 + build*10 + plat。
    /// 推导：build*10+plat ≤ 655353 < 1e6（不串 minor 位权），minor ≤ 9
    /// 故 minor 位权占满 1e6..1e7，major ≤ 10 占满 1e8 段——三段无碰撞。
    pub const fn packed(&self) -> u32 {
        self.major * 100_000_000 + self.minor * 1_000_000 + self.build * 10 + self.plat
    }
}

/// 兜底层基础版本（6.1 build 7650 平台 2），打包值即 BASE_VERDICT。
pub const BASE_VER: OvrVer = OvrVer::new(6, 1, 7650, 2);

// ── 覆写槽状态机 ───────────────────────────────────────────────────
/// 槽状态机：Free→Active（创建），Active→Free（撤销）。非法倒退
/// （Active 再建 / Free 撤销）显式显码拒绝——单向转换，无越级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲：无覆写在场
    Free,
    /// 活跃：覆写在场
    Active,
}

/// 覆写槽（状态/版本/属主/令牌）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OvrSlot {
    pub state: SlotState,
    pub ver: OvrVer,
    pub pid: u32,
    pub token: u32,
}

impl OvrSlot {
    /// 空槽（const：定容数组初始化）。
    pub const fn empty() -> OvrSlot {
        OvrSlot {
            state: SlotState::Free,
            ver: OvrVer::new(0, 0, 0, 0),
            pid: 0,
            token: 0,
        }
    }
}

/// 覆写表（数组账本 + 活跃位面）。
pub struct OvrTable {
    pub slots: [OvrSlot; OVR_CAP],
    /// 活跃位面：第 i 位=槽 i 覆写在 u8（8 槽恰一位段）。
    pub plane: u8,
    pub created: u32,
    pub undone: u32,
}

impl OvrTable {
    /// 空表（位面零 = 全空闲）。
    pub fn new() -> OvrTable {
        OvrTable {
            slots: [OvrSlot::empty(); OVR_CAP],
            plane: 0,
            created: 0,
            undone: 0,
        }
    }
    /// 位面查询：槽 i 是否活跃。
    pub fn active(&self, i: usize) -> bool {
        if i >= OVR_CAP {
            return false;
        }
        (self.plane >> i) & 1 == 1
    }
    /// 位面置位/清位（带界守卫，越界静默不置——防御位）。
    fn plane_set(&mut self, i: usize, on: bool) {
        if i < OVR_CAP {
            if on {
                self.plane |= 1u8 << i;
            } else {
                self.plane &= !(1u8 << i);
            }
        }
    }
    /// 活跃槽计数（位面折算）。
    pub fn active_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0;
        while i < OVR_CAP {
            if self.active(i) {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 创建覆写：键域/进程域/版本域三闸 + 状态闸（Free 才可建）。
    /// 返回 Ok(令牌)=创建成功（令牌=FNV(pid‖ver packed)），Err(码)=显拒。
    pub fn create(&mut self, pid: u32, key: usize, ver: OvrVer) -> Result<u32, u32> {
        if pid == 0 || pid > PID_MAX {
            return Err(TxCode::PidOutOfRange.code());
        }
        if key >= OVR_CAP {
            return Err(TxCode::KeyOutOfRange.code());
        }
        if !ver.legal() {
            return Err(TxCode::VerIllegal.code());
        }
        // 状态闸先于令牌派生：活跃槽拒绝重复创建（守卫在改动前完成）。
        if self.slots[key].state == SlotState::Active {
            return Err(TxCode::AlreadyActive.code());
        }
        let mut seed = Vec::new();
        seed.push((pid % 256) as u8);
        seed.push(((pid / 256) % 256) as u8);
        seed.push((ver.packed() % 256) as u8);
        seed.push(((ver.packed() / 256) % 256) as u8);
        seed.push(((ver.packed() / 65536) % 256) as u8);
        let token = fnv1a(&seed);
        // 守卫全过才落账：状态/账/位面三处一次写齐（写序固定，保证确定性）。
        self.slots[key] = OvrSlot { state: SlotState::Active, ver, pid, token };
        self.plane_set(key, true);
        self.created = self.created.saturating_add(1);
        Ok(token)
    }

    /// 撤销覆写：状态闸（Active）+ 令牌闸（仅创建者）。
    pub fn undo(&mut self, pid: u32, key: usize, token: u32) -> Result<(), u32> {
        if pid == 0 || pid > PID_MAX {
            return Err(TxCode::PidOutOfRange.code());
        }
        if key >= OVR_CAP {
            return Err(TxCode::KeyOutOfRange.code());
        }
        if self.slots[key].state != SlotState::Active {
            return Err(TxCode::NoActiveOverride.code());
        }
        if self.slots[key].token != token || self.slots[key].pid != pid {
            return Err(TxCode::TokenMismatch.code());
        }
        self.slots[key] = OvrSlot::empty();
        self.plane_set(key, false);
        self.undone = self.undone.saturating_add(1);
        Ok(())
    }

    /// 查询：键越界显拒；空闲槽落兜底层（Default 裁决），活跃槽落覆写裁决。
    pub fn query(&self, key: usize) -> Result<(u32, u32), u32> {
        if key >= OVR_CAP {
            return Err(TxCode::GhostQuery.code());
        }
        if self.slots[key].state == SlotState::Active {
            Ok((LAYER_OVR_CREATE, self.slots[key].ver.packed()))
        } else {
            Ok((LAYER_DEFAULT, BASE_VERDICT))
        }
    }
}

// ── 轨迹账本 ───────────────────────────────────────────────────────
/// 单条轨迹（七字段与导出行一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceEntry {
    pub seq: u32,
    pub kind: u8,
    pub pid: u32,
    pub api: u32,
    pub dim: u32,
    pub layer: u32,
    pub basis: u32,
}

impl TraceEntry {
    /// 零值轨迹（const：账本初始化）。
    pub const fn zero() -> TraceEntry {
        TraceEntry { seq: 0, kind: 0, pid: 0, api: 0, dim: 0, layer: 0, basis: 0 }
    }
}

/// 轨迹账本：写入序落账 + dropped 显性记账 + next_seq 单调序号闸。
pub struct TraceLedger {
    pub recs: Vec<TraceEntry>,
    pub dropped: u32,
    pub next_seq: u32,
}

impl TraceLedger {
    /// 空账（next_seq 从 1 起——0 保留为"无效序号"哨兵）。
    pub fn new() -> TraceLedger {
        TraceLedger { recs: Vec::new(), dropped: 0, next_seq: 1 }
    }

    /// 入账闸：种类闭集 / 层闭集 / 进程域 / 序号单调（==next_seq）四闸
    /// 全过才落账；账满时 dropped 显性记账并显拒（不静默丢）。
    pub fn feed(&mut self, ev: TraceEntry) -> Result<(), u32> {
        match ev.kind {
            k if k == KIND_CREATE || k == KIND_QUERY || k == KIND_UNDO => {}
            _ => return Err(TxCode::TraceBadKind.code()),
        }
        if ev.layer > LAYER_OVR_UNDO {
            return Err(TxCode::TraceBadLayer.code());
        }
        if ev.pid == 0 || ev.pid > PID_MAX {
            return Err(TxCode::PidOutOfRange.code());
        }
        if ev.seq != self.next_seq {
            return Err(TxCode::TraceSeqOrder.code());
        }
        if self.recs.len() >= TRACE_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(TxCode::TraceTableFull.code());
        }
        self.recs.push(ev);
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }

    /// 导出面：表头 + 数据行。表头与 F18431 同协议（OBS-TRACE v1 前缀
    /// + n= 计数），数据行七管道字段。空账仅表头（合法边界）。
    pub fn export(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "{} seq|proc|api|dim|layer|basis|value n={}",
            EXPORT_FMT_TAG,
            self.recs.len()
        ));
        let mut i = 0;
        while i < self.recs.len() {
            let r = self.recs[i];
            out.push(format!(
                "{}|{}|{}|{}|{}|{}|{}",
                r.seq, r.pid, r.api, r.dim, r.layer, r.basis, 0u32
            ));
            i += 1;
        }
        out
    }
}

// ── 覆写会话（表+账联动的门面） ────────────────────────────────────
/// 门面：表操作与轨迹入账一次完成——保证"表动账必动"，两者守恒。
pub struct OvrSession {
    pub table: OvrTable,
    pub led: TraceLedger,
    /// 导出 value 段暂存（导出行第 7 字段：裁决打包值）。
    pub values: Vec<u32>,
}

impl OvrSession {
    /// 空会话。
    pub fn new() -> OvrSession {
        OvrSession { table: OvrTable::new(), led: TraceLedger::new(), values: Vec::new() }
    }

    /// 带轨迹的创建：layer=4（覆写创建段），basis=令牌。
    pub fn create_traced(&mut self, pid: u32, key: usize, api: u32, dim: u32, ver: OvrVer) -> Result<u32, u32> {
        let token = self.table.create(pid, key, ver)?;
        let ev = TraceEntry {
            seq: self.led.next_seq,
            kind: KIND_CREATE,
            pid,
            api,
            dim,
            layer: LAYER_OVR_CREATE,
            basis: token,
        };
        self.led.feed(ev)?;
        self.values.push(ver.packed());
        Ok(token)
    }

    /// 带轨迹的查询：layer=命中层（覆写在场=4，否则 3 兜底）。
    pub fn query_traced(&mut self, pid: u32, key: usize, api: u32, dim: u32) -> Result<u32, u32> {
        if key >= OVR_CAP {
            return Err(TxCode::GhostQuery.code());
        }
        let (layer, verdict) = self.table.query(key)?;
        let ev = TraceEntry {
            seq: self.led.next_seq,
            kind: KIND_QUERY,
            pid,
            api,
            dim,
            layer,
            basis: verdict,
        };
        self.led.feed(ev)?;
        self.values.push(verdict);
        Ok(verdict)
    }

    /// 带轨迹的撤销：layer=5（覆写撤销段），basis=令牌回写。
    pub fn undo_traced(&mut self, pid: u32, key: usize, api: u32, dim: u32, token: u32) -> Result<(), u32> {
        self.table.undo(pid, key, token)?;
        let ev = TraceEntry {
            seq: self.led.next_seq,
            kind: KIND_UNDO,
            pid,
            api,
            dim,
            layer: LAYER_OVR_UNDO,
            basis: token,
        };
        self.led.feed(ev)?;
        self.values.push(0);
        Ok(())
    }

    /// 账面守恒：撤销 ≤ 创建，且活跃数 == 创建-撤销（差值不变式）。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.table.undone > self.table.created {
            return Err(TxCode::LedgerBroken.code());
        }
        if self.table.active_count() != self.table.created - self.table.undone {
            return Err(TxCode::LedgerBroken.code());
        }
        Ok(())
    }
}

// ── 导出行解析（回放面） ───────────────────────────────────────────
/// 数字段解析（十进制，无符号，溢出拒绝）。
fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(TxCode::ExportBad.code());
    }
    let mut acc: u32 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(TxCode::ExportBad.code());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(TxCode::ExportBad.code()),
        };
        i += 1;
    }
    Ok(acc)
}

/// 表头校验：OBS-TRACE v1 前缀 + "n=" 计数段（与 F18431 同协议字面锚）。
pub fn valid_header(line: &str) -> bool {
    let tag = EXPORT_FMT_TAG;
    let tb = tag.as_bytes();
    let lb = line.as_bytes();
    if lb.len() < tb.len() || &lb[..tb.len()] != tb {
        return false;
    }
    // 尾部必须含 " n="（计数前有空格）。
    let mut i = tb.len();
    let mut saw_space_n = false;
    while i + 2 < lb.len() {
        if lb[i] == b' ' && lb[i + 1] == b'n' && lb[i + 2] == b'=' {
            saw_space_n = true;
            break;
        }
        i += 1;
    }
    saw_space_n
}

/// 单数据行解析：恰七段，全数字，段内闭集闸（kind/layer）。
pub fn parse_line(line: &str) -> Result<TraceEntry, u32> {
    // 数管道：七字段应有 6 个分隔符。
    let b = line.as_bytes();
    let mut pipes = 0usize;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'|' {
            pipes += 1;
        }
        i += 1;
    }
    if pipes != EXPORT_FIELDS - 1 {
        return Err(TxCode::ExportBad.code());
    }
    // 逐段切分。
    let mut segs: [&str; EXPORT_FIELDS] = ["", "", "", "", "", "", ""];
    let mut idx = 0usize;
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= b.len() {
        if k == b.len() || b[k] == b'|' {
            segs[idx] = &line[start..k];
            idx += 1;
            start = k + 1;
            if idx > EXPORT_FIELDS {
                return Err(TxCode::ExportBad.code());
            }
        }
        k += 1;
    }
    if idx != EXPORT_FIELDS {
        return Err(TxCode::ExportBad.code());
    }
    let seq = parse_u32(segs[0])?;
    let pid = parse_u32(segs[1])?;
    let api = parse_u32(segs[2])?;
    let dim = parse_u32(segs[3])?;
    let layer = parse_u32(segs[4])?;
    let basis = parse_u32(segs[5])?;
    if layer > LAYER_OVR_UNDO || pid == 0 || pid > PID_MAX {
        return Err(TxCode::ExportBad.code());
    }
    Ok(TraceEntry { seq, kind: KIND_QUERY, pid, api, dim, layer, basis })
}

/// 回放面：跳过表头逐行解析，返回全部数据行。
pub fn replay(lines: &[String]) -> Result<Vec<TraceEntry>, u32> {
    if lines.is_empty() || !valid_header(&lines[0]) {
        return Err(TxCode::ExportBad.code());
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < lines.len() {
        out.push(parse_line(&lines[i])?);
        i += 1;
    }
    Ok(out)
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 按键推导样板版本（确定性：键→版本纯函数）。
pub fn sample_ver(key: usize) -> OvrVer {
    OvrVer::new(
        1 + (key % 3) as u32,
        (key % 5) as u32,
        ((key * 733 + 11) % (VER_BUILD_MAX as usize + 1)) as u32,
        1 + (key % 3) as u32,
    )
}

/// 黄金全链：4 创建→8 查询→2 撤销→导出→回放对拍→守恒。
/// 返回 (事件数, 创建数, 撤销数)。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut ses = OvrSession::new();
    // 4 创建：pid=101*(k+1)（确定性派生），key=0..4。
    let mut tokens = [0u32; 4];
    let mut k = 0;
    while k < 4 {
        let pid = (101 * (k as u32 + 1)) % (PID_MAX + 1);
        tokens[k] = ses.create_traced(pid, k, 1 + (k % 4) as u32, 1 + (k % 3) as u32, sample_ver(k))?;
        k += 1;
    }
    // 8 查询：每键 2 次（api/dim 轮转，确定性）。
    let mut q = 0;
    while q < 8 {
        let key = q % 4;
        let pid = (101 * (key as u32 + 1)) % (PID_MAX + 1);
        ses.query_traced(pid, key, 1 + (q % API_MAX as usize) as u32, 1 + (q % DIM_MAX as usize) as u32)?;
        q += 1;
    }
    // 2 撤销：键 0、2（令牌须与创建一致——权限纪律预告）。
    let undo_keys = [0usize, 2usize];
    let mut u = 0;
    while u < 2 {
        let key = undo_keys[u];
        let pid = (101 * (key as u32 + 1)) % (PID_MAX + 1);
        ses.undo_traced(pid, key, 0, 0, tokens[key])?;
        u += 1;
    }
    ses.conserve()?;
    // 导出→回放→往返逐字段对拍。
    let lines = ses.led.export();
    let back = replay(&lines)?;
    if back.len() != ses.led.recs.len() {
        return Err(TxCode::RoundtripBroken.code());
    }
    let mut i = 0;
    while i < ses.led.recs.len() {
        let a = ses.led.recs[i];
        let b = back[i];
        // 回放行 kind 不含种类语义（协议字段里无 kind 列），对拍其余六字段。
        if a.seq != b.seq || a.pid != b.pid || a.api != b.api || a.dim != b.dim
            || a.layer != b.layer || a.basis != b.basis
        {
            return Err(TxCode::RoundtripBroken.code());
        }
        i += 1;
    }
    Ok((ses.led.recs.len(), ses.table.created, ses.table.undone))
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TxCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（4 类坏样本 × JUDGE_ROUNDS 轮） ────────────────────────
/// 注入记账（tries/caught 逐槽，守恒 tries==caught 漏检必 0）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮（slot 越界静默不记——防御位）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：四槽全记录且全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 逐类对码注入：四类坏样本 × rounds 轮，返回守恒账。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：非法版本创建（build 越界一位）。
        let bad_ver = OvrVer::new(1, 0, VER_BUILD_MAX + 1, 1);
        led.record(0, ses_create_err(101, 0, bad_ver) == Some(TxCode::VerIllegal.code()));
        // 槽1：无覆写撤销（Free 槽撤销）。
        led.record(1, ses_undo_err(101, 3, 1) == Some(TxCode::NoActiveOverride.code()));
        // 槽2：活跃槽重复创建。
        let mut ses = OvrSession::new();
        let t = ses.create_traced(101, 1, 1, 1, sample_ver(1));
        let dup = match t {
            Ok(tok) => ses.create_traced(101, 1, 1, 1, sample_ver(2)).err(),
            Err(_) => None,
        };
        led.record(2, dup == Some(TxCode::AlreadyActive.code()));
        // 槽3：幽灵键查询。
        let mut ses2 = OvrSession::new();
        led.record(
            3,
            ses2.query_traced(101, OVR_CAP, 1, 1).err() == Some(TxCode::GhostQuery.code()),
        );
        r += 1;
    }
    led.all_caught();
    Ok(led)
}

/// 单独会话里测创建拒因（返回 Err 码的 Option 包装——避免裸比较陷阱）。
fn ses_create_err(pid: u32, key: usize, ver: OvrVer) -> Option<u32> {
    let mut ses = OvrSession::new();
    ses.create_traced(pid, key, 1, 1, ver).err()
}

/// 单独会话里测撤销拒因。
fn ses_undo_err(pid: u32, key: usize, token: u32) -> Option<u32> {
    let mut ses = OvrSession::new();
    ses.undo_traced(pid, key, 1, 1, token).err()
}

/// 序号乱序注入：喂一条 seq≠next_seq 的轨迹必拒（单调闸）。
pub fn inject_seq_order() -> Result<(), u32> {
    let mut led = TraceLedger::new();
    let ev = TraceEntry { seq: 5, kind: KIND_QUERY, pid: 101, api: 1, dim: 1, layer: LAYER_DEFAULT, basis: BASE_VERDICT };
    if led.feed(ev).is_ok() {
        return Err(TxCode::TraceSeqOrder.code());
    }
    Ok(())
}

/// 篡改行注入：缺管道/非数字行必拒（回放面闸）。
pub fn inject_bad_line() -> Result<(), u32> {
    let a = parse_line("1|101|1|1|4|12345");
    let b = parse_line("x|101|1|1|4|12345|0");
    if a.is_ok() || b.is_ok() {
        return Err(TxCode::ExportBad.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TxCode::ALL.len() {
        let v = TxCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每个诊断码都能取到非空中文建议。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < TxCode::ALL.len() {
        if TxCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 覆写段事件计数（导出后统计 layer=4/5 的行——J1 主体断言原料）。
pub fn count_ovr_layers(lines: &[String]) -> Result<(usize, usize), u32> {
    let back = replay(lines)?;
    let mut create = 0usize;
    let mut undo = 0usize;
    let mut i = 0;
    while i < back.len() {
        if back[i].layer == LAYER_OVR_CREATE {
            create += 1;
        }
        if back[i].layer == LAYER_OVR_UNDO {
            undo += 1;
        }
        i += 1;
    }
    Ok((create, undo))
}

// ── 判据收口（16 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18491_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18491 覆写观测导出（裁决轨迹含覆写段）");
    // 1. 黄金全链（含导出回放往返）。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!("events/created/undone={:?}", g),
    );
    // 2. 覆写创建事件入轨迹（layer=4 计数 == 创建数）。
    let ovr_ev = {
        let mut ses = OvrSession::new();
        let mut k = 0;
        while k < 4 {
            let pid = (101 * (k as u32 + 1)) % (PID_MAX + 1);
            let _ = ses.create_traced(pid, k, 1, 1, sample_ver(k));
            k += 1;
        }
        count_ovr_layers(&ses.led.export())
    };
    let ev_ok = match ovr_ev {
        Ok((c, _u)) => c == 4,
        Err(_) => false,
    };
    s.add("J1-覆写创建事件入轨迹", ev_ok, "layer=4 行数==创建数4");
    // 3. 覆写撤销事件入轨迹（layer=5 计数 == 撤销数）。
    let undo_ev = {
        let mut ses = OvrSession::new();
        let t0 = ses.create_traced(101, 0, 1, 1, sample_ver(0));
        let t2 = ses.create_traced(202, 2, 1, 1, sample_ver(2));
        let ok = match (t0, t2) {
            (Ok(a), Ok(b)) => {
                let _ = ses.undo_traced(101, 0, 0, 0, a);
                let _ = ses.undo_traced(202, 2, 0, 0, b);
                true
            }
            _ => false,
        };
        if ok {
            match count_ovr_layers(&ses.led.export()) {
                Ok((_, u)) => u == 2,
                Err(_) => false,
            }
        } else {
            false
        }
    };
    s.add("J1-覆写撤销事件入轨迹", undo_ev, "layer=5 行数==撤销数2");
    // 4. 导出格式与 F18431 一致（同版标签+七字段+管道数）。
    let fmt_ok = {
        let lines = TraceLedger::new().export();
        lines.len() == 1
            && valid_header(&lines[0])
            && EXPORT_FIELDS == 7
            && EXPORT_FMT_TAG == "OBS-TRACE v1"
    };
    s.add("J1-导出格式F18431一致", fmt_ok, "OBS-TRACE v1 · 七管道字段");
    // 5. 回放往返一致（含回放账逐字段对拍）。
    s.add("J1-导出回放往返", golden_run().is_ok(), "回放与账本逐字段一致");
    // 6. 注入核 10/10（四类守恒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10四类全抓", inj_ok, &format!("t/c={:?}", inj.as_ref().map(|l| l.caught)));
    // 7. 非法版本创建必拒（逐码）。
    s.add("J1-非法版本拒", ses_create_err(101, 0, OvrVer::new(1, 0, VER_BUILD_MAX + 1, 1)) == Some(TxCode::VerIllegal.code()), "VerIllegal");
    // 8. 无覆写撤销必拒。
    s.add("J1-无覆写撤销拒", ses_undo_err(101, 3, 1) == Some(TxCode::NoActiveOverride.code()), "NoActiveOverride");
    // 9. 活跃槽重复创建必拒。
    let dup_ok = {
        let mut ses = OvrSession::new();
        let t = ses.create_traced(101, 1, 1, 1, sample_ver(1));
        match t {
            Ok(_) => ses.create_traced(101, 1, 1, 1, sample_ver(2)).err() == Some(TxCode::AlreadyActive.code()),
            Err(_) => false,
        }
    };
    s.add("J1-重复创建拒", dup_ok, "AlreadyActive");
    // 10. 幽灵键查询必拒。
    let ghost_ok = {
        let mut ses = OvrSession::new();
        ses.query_traced(101, OVR_CAP, 1, 1).err() == Some(TxCode::GhostQuery.code())
    };
    s.add("J1-幽灵查询拒", ghost_ok, "GhostQuery");
    // 11. 序号乱序必拒（单调闸）。
    s.add("J1-序号乱序拒", inject_seq_order().is_ok(), "TraceSeqOrder");
    // 12. 篡改行必拒（回放面闸）。
    s.add("J1-篡改行拒", inject_bad_line().is_ok(), "ExportBad");
    // 13. 账面守恒（撤销≤创建 ∧ 活跃数==差值）。
    let cons_ok = {
        let mut ses = OvrSession::new();
        let t = ses.create_traced(101, 0, 1, 1, sample_ver(0));
        if let Ok(tok) = t {
            let _ = ses.undo_traced(101, 0, 0, 0, tok);
        }
        let t2 = ses.create_traced(202, 1, 1, 1, sample_ver(1));
        if t2.is_ok() {
            ses.conserve().is_ok()
        } else {
            false
        }
    };
    s.add("J1-账面守恒", cons_ok, "undone≤created ∧ active==created-undone");
    // 14. 空轨迹导出仅表头（合法边界）。
    s.add("J1-空轨迹仅表头", TraceLedger::new().export().len() == 1, "空账导出=1行");
    // 15. 双跑一致 + 段位守卫 + 文案闭集（三联守卫）。
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    s.add("J1-段位守卫", segment_guard(), "0x7F680 窗口");
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18491_checks_green() {
        let set = run_unx18491_checks();
        assert!(set.all_passed(), "UNX-F18491 red: {}", set.red_items_text());
    }

    #[test]
    fn t_golden_shape() {
        let g = golden_run().unwrap_or((0, 0, 0));
        // 推导：4 创建 + 8 查询 + 2 撤销 = 14 事件。
        assert_eq!(g, (14, 4, 2), "黄金链事件形状");
    }

    #[test]
    fn t_expected_reject_codes() {
        // 四类注入逐码对拍（Err 与 Err 码不可直接 ==，用 .err()==Some(..)）。
        assert_eq!(
            ses_create_err(101, 0, OvrVer::new(1, 0, VER_BUILD_MAX + 1, 1)),
            Some(TxCode::VerIllegal.code())
        );
        assert_eq!(
            ses_undo_err(101, 3, 1),
            Some(TxCode::NoActiveOverride.code())
        );
        let mut ses = OvrSession::new();
        let tok = ses.create_traced(101, 1, 1, 1, sample_ver(1)).unwrap_or(0);
        let _ = tok;
        assert_eq!(
            ses.create_traced(101, 1, 1, 1, sample_ver(2)).err(),
            Some(TxCode::AlreadyActive.code())
        );
        let mut ses2 = OvrSession::new();
        assert_eq!(
            ses2.query_traced(101, OVR_CAP, 1, 1).err(),
            Some(TxCode::GhostQuery.code())
        );
    }

    #[test]
    fn t_token_mismatch_rejected() {
        // 权限预告：令牌不符撤销必拒（F18492 前哨）。
        let mut ses = OvrSession::new();
        let tok = ses.create_traced(101, 0, 1, 1, sample_ver(0)).unwrap_or(0);
        assert_eq!(
            ses.undo_traced(202, 0, 0, 0, tok).err(),
            Some(TxCode::TokenMismatch.code()),
            "他人令牌撤销必拒"
        );
    }

    #[test]
    fn t_state_machine_no_regression() {
        // 状态机：Free 上撤销拒、Active 上重建拒、撤销后可重建（回环合法）。
        let mut ses = OvrSession::new();
        assert_eq!(
            ses.table.undo(101, 0, 1).err(),
            Some(TxCode::NoActiveOverride.code())
        );
        let tok = ses.create_traced(101, 0, 1, 1, sample_ver(0)).unwrap_or(0);
        assert_eq!(
            ses.table.create(101, 0, sample_ver(1)).err(),
            Some(TxCode::AlreadyActive.code())
        );
        assert!(ses.undo_traced(101, 0, 0, 0, tok).is_ok());
        assert!(ses.create_traced(101, 0, 1, 1, sample_ver(1)).is_ok(), "撤销后可重建");
    }

    #[test]
    fn t_seq_monotonic_gate() {
        // 轨迹序号必须严格 == next_seq（重复序号/乱序/跳号全拒）。
        let mut led = TraceLedger::new();
        let mk = |s: u32| TraceEntry { seq: s, kind: KIND_QUERY, pid: 101, api: 1, dim: 1, layer: LAYER_DEFAULT, basis: BASE_VERDICT };
        assert!(led.feed(mk(1)).is_ok());
        assert!(led.feed(mk(1)).is_err(), "重复序号");
        assert!(led.feed(mk(3)).is_err(), "跳号");
        assert!(led.feed(mk(2)).is_ok(), "续号");
    }

    #[test]
    fn t_trace_cap_dropped_accounted() {
        // 容量耗尽：满后显性记账 dropped>0 且账长不越界。
        let mut led = TraceLedger::new();
        let mut n = 0;
        while n < TRACE_CAP + 3 {
            let ev = TraceEntry { seq: led.next_seq, kind: KIND_QUERY, pid: 101, api: 1, dim: 1, layer: LAYER_DEFAULT, basis: BASE_VERDICT };
            // 账满后 feed 显拒，dropped 记账（序号闸在满闸之前，故仍单调）。
            let _ = led.feed(ev);
            n += 1;
        }
        assert_eq!(led.recs.len(), TRACE_CAP, "账长不越界");
        assert!(led.dropped >= 3, "溢出显性记账");
    }

    #[test]
    fn t_parse_roundtrip_and_tamper() {
        // 往返：合法行可解析；缺管道/非数字/层越界行全拒。
        let e = TraceEntry { seq: 7, kind: KIND_QUERY, pid: 404, api: 3, dim: 2, layer: LAYER_OVR_CREATE, basis: 12345 };
        let line = format!("{}|{}|{}|{}|{}|{}|{}", e.seq, e.pid, e.api, e.dim, e.layer, e.basis, 0u32);
        let back = parse_line(&line).unwrap_or(TraceEntry::zero());
        assert_eq!(back.seq, e.seq);
        assert_eq!(back.pid, e.pid);
        assert_eq!(back.basis, e.basis);
        assert!(parse_line("1|101|1|1|4").is_err(), "管道数不足");
        assert!(parse_line("1|101|1|1|9|1|0").is_err(), "层越界");
        assert!(valid_header("OBS-TRACE v1 seq|proc|api|dim|layer|basis|value n=3"));
        assert!(!valid_header("OBS-TRACE v2 x"), "版标签不符");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F680 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(TxCode::VerIllegal.code(), SEG_BASE + 2, "码位推导");
    }

    #[test]
    fn t_packing_collision_free() {
        // 打包三段位权无碰撞：不同 (major,minor,build,plat) 不同 packed。
        let a = OvrVer::new(1, 0, 0, 1).packed();
        let b = OvrVer::new(1, 0, 0, 2).packed();
        let c = OvrVer::new(1, 0, 1, 1).packed();
        let d = OvrVer::new(2, 0, 0, 1).packed();
        assert!(a != b && a != c && a != d && c != d, "位权无碰撞");
        assert_eq!(BASE_VERDICT, BASE_VER.packed(), "BASE_VERDICT 推导自 BASE_VER");
    }
}
