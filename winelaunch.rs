//! Wine 应用启动通道（实机走查入口 · 交叉走查第一项配套）。
//!
//! 职责一句话：把「启动一个 Windows 应用」的请求受理下来——查应用注册
//! 表 → 前缀实例化（winepfx）→ 拉起登记（winecare 状态面）→ 如实回报
//! 运行时在位状态。**Wine 用户态运行时未部署进镜像时如实报 Absent**，
//! 绝不假装启动成功（与 B-507/清算族同族的诚实纪律：清单外标准报错）。
//!
//! 分层：纯函数面宿主可测（ktest）；target 薄分发在 usrshell
//! （SHIM_WINE_RUN 命令块）。零堆纪律：无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use crate::winecare::SessionCard;
use crate::winepfx::{instantiate, PrefixInstance, TEMPLATE_V1, TEMPLATE_VER_FIRST};

// ---------------------------------------------------------------------------
// 应用注册表（冻结面：预置通道应用——加应用即加行，穷举可审计）
// ---------------------------------------------------------------------------

/// 应用表行数（冻结；扩容走 ADR）。
pub const APP_TABLE_ROWS: usize = 1;

/// 预置应用 id（组号复用——SessionCard.well_formed 要求非零）。
pub const NOTEPAD_CLASSIC_ID: u32 = 0x4E50_0001;

/// 应用注册行：名字字节串 + id + 通道 + 如实评级。
pub struct AppRow {
    pub name: &'static [u8],
    pub id: u32,
    /// 通道（当前只有 wine 一条；future: linux/vxapp）。
    pub channel: &'static [u8],
    /// 星卡评级（partial=部分可用——评级不虚标，B-2103 族）。
    pub tier: &'static [u8],
}

/// 预置注册表（usrshell apps.json 的内核侧事实源——两处不同步即缺陷）。
pub const APP_TABLE: [AppRow; APP_TABLE_ROWS] = [AppRow {
    name: b"notepad-classic",
    id: NOTEPAD_CLASSIC_ID,
    channel: b"wine",
    tier: b"partial",
}];

/// 按名字查表：命中返回行，未命中 None（UnknownApp 的裁决依据）。
pub fn lookup(app: &[u8]) -> Option<&'static AppRow> {
    APP_TABLE.iter().find(|r| r.name == app)
}

// ---------------------------------------------------------------------------
// 运行时在位状态（如实——未部署就是未部署）
// ---------------------------------------------------------------------------

/// Wine 用户态运行时在位状态（穷举三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuntimeState {
    /// Wine 本体已部署进镜像，可真启动（部署欠账关闭后翻转）。
    Present,
    /// 本体未进镜像——拉起请求受理、前缀就绪，但无进程可跑。
    Absent,
    /// 版本锁定表与镜像内本体不一致——拒绝启动待重校验（R5）。
    LockMismatch,
}

/// 当前实况：Wine 本体尚未打包进 varix 镜像（借力件登记在册，部署属
/// 实机欠账「Wine 运行时入镜像」）。**常量如实，翻转需随部署 commit。**
pub const RUNTIME_STATE: RuntimeState = RuntimeState::Absent;

// ---------------------------------------------------------------------------
// 拉起结果与状态编码
// ---------------------------------------------------------------------------

/// 拉起状态（穷举四态——出参第一字节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaunchState {
    /// 受理：前缀实例化成功；运行时在位则进程组已拉起。
    Accepted = 1,
    /// 受理但运行时缺席：前缀就绪、登记在册，无进程可跑（当前实况）。
    AcceptedRuntimeAbsent = 2,
    /// 未知应用：注册表外一律拒（清单外标准报错）。
    UnknownApp = 3,
    /// 拉起内部错误（前缀实例化失败等）。
    InternalError = 4,
}

/// 一次拉起请求的完整结果。
pub struct LaunchOutcome {
    pub state: LaunchState,
    /// 前缀实例化产物（Accepted/AcceptedRuntimeAbsent 时必有且带版本号）。
    pub prefix: Option<PrefixInstance>,
    /// 会话卡（仅 Running 语义下发放；RuntimeAbsent 时如实 None）。
    pub session: Option<SessionCard>,
    /// 运行时在位状态（透传，出参第二字节）。
    pub runtime: RuntimeState,
}

/// 拉起入口：查表 → 前缀实例化 → 按运行时状态如实定状态。
pub fn launch(app: &[u8], now_ms: u64) -> LaunchOutcome {
    let Some(row) = lookup(app) else {
        return LaunchOutcome {
            state: LaunchState::UnknownApp,
            prefix: None,
            session: None,
            runtime: RUNTIME_STATE,
        };
    };
    // 前缀实例化：canonical 模板 + 首版（可追溯锚随产物走）。
    let prefix = instantiate(row.id, &TEMPLATE_V1, TEMPLATE_VER_FIRST);
    let Some(prefix) = prefix else {
        return LaunchOutcome {
            state: LaunchState::InternalError,
            prefix: None,
            session: None,
            runtime: RUNTIME_STATE,
        };
    };
    match RUNTIME_STATE {
        RuntimeState::Present => LaunchOutcome {
            state: LaunchState::Accepted,
            prefix: Some(prefix),
            session: Some(SessionCard {
                group_id: row.id,
                process_count: 1,
                memory_kb: 4096,
                started_ms: now_ms,
            }),
            runtime: RUNTIME_STATE,
        },
        RuntimeState::Absent | RuntimeState::LockMismatch => LaunchOutcome {
            state: LaunchState::AcceptedRuntimeAbsent,
            prefix: Some(prefix),
            session: None,
            runtime: RUNTIME_STATE,
        },
    }
}

// ---------------------------------------------------------------------------
// 出参编码（定长槽，供 usrshell 命令块回写）
// ---------------------------------------------------------------------------

/// 出参布局：[0]=state u8 | [1]=runtime u8(0=present,1=absent,2=mismatch)
/// | [2..6]=app_id u32 LE | [6..10]=templ_ver u32 LE | [10..12]=msg len u16
/// | [12..12+len]=消息字节串（调用方保证容量 ≥ 12+MSG_MAX）。
pub const OUT_HEAD: usize = 12;
pub const MSG_MAX: usize = 64;

/// 状态消息（静态字节串——零堆：无格式化拼接）。
pub fn state_msg(o: &LaunchOutcome) -> &'static [u8] {
    match o.state {
        LaunchState::Accepted => b"accepted: wine session started",
        LaunchState::AcceptedRuntimeAbsent => {
            b"accepted-runtime-absent: prefix ready, wine userland not in image"
        }
        LaunchState::UnknownApp => b"unknown-app: not in registry (standard error, not silent)",
        LaunchState::InternalError => b"internal-error: prefix instantiation failed",
    }
}

/// 把结果编码进出参区，返回写入长度（state_msg 截断到 MSG_MAX）。
pub fn encode_status(out: &mut [u8], o: &LaunchOutcome) -> usize {
    if out.len() < OUT_HEAD + MSG_MAX {
        return 0;
    }
    let runtime_byte = match o.runtime {
        RuntimeState::Present => 0u8,
        RuntimeState::Absent => 1u8,
        RuntimeState::LockMismatch => 2u8,
    };
    out[0] = o.state as u8;
    out[1] = runtime_byte;
    let app_id = o.prefix.as_ref().map(|p| p.app_id).unwrap_or(0);
    let templ_ver = o.prefix.as_ref().map(|p| p.templ_ver).unwrap_or(0);
    out[2..6].copy_from_slice(&app_id.to_le_bytes());
    out[6..10].copy_from_slice(&templ_ver.to_le_bytes());
    let msg = state_msg(o);
    let n = msg.len().min(MSG_MAX);
    out[10..12].copy_from_slice(&(n as u16).to_le_bytes());
    out[12..12 + n].copy_from_slice(&msg[..n]);
    OUT_HEAD + n
}

// ---------------------------------------------------------------------------
// 判据自检（ktest 面：宿主可测）
// ---------------------------------------------------------------------------

pub fn run_winelaunch_checks() -> CheckSet {
    let mut set = CheckSet::new("Wine 启动通道（交叉走查第一项配套）");
    // 1. 注册表命中：预置应用按名可查，id 与通道如实。
    let hit = lookup(b"notepad-classic");
    set.add(
        "launch 注册表命中",
        hit.is_some() && hit.unwrap().id == NOTEPAD_CLASSIC_ID && hit.unwrap().channel == b"wine",
        "notepad-classic 通道=wine——apps.json 与内核表同源",
    );
    // 2. 未知应用如实拒：清单外标准报错，不静默。
    let unk = launch(b"totally-unknown-app", 100);
    set.add(
        "launch 未知应用拒",
        unk.state == LaunchState::UnknownApp && unk.prefix.is_none() && unk.session.is_none(),
        "清单外应用 UnknownApp——标准报错是功能不是失败",
    );
    // 3. 运行时缺席如实：受理但如实标 RuntimeAbsent，前缀版本可追溯。
    let abs = launch(b"notepad-classic", 100);
    let p_ok = abs
        .prefix
        .as_ref()
        .map(|p| p.templ_ver == TEMPLATE_VER_FIRST && p.from_template && p.app_id == NOTEPAD_CLASSIC_ID)
        .unwrap_or(false);
    set.add(
        "launch 缺席如实+前缀可追溯",
        abs.state == LaunchState::AcceptedRuntimeAbsent
            && abs.runtime == RuntimeState::Absent
            && abs.session.is_none()
            && p_ok,
        "受理≠谎报启动：前缀就绪、运行时缺席如实入账",
    );
    // 4. 会话卡只在 Running 语义下良构：RuntimeAbsent 时不出卡。
    let present_card = SessionCard {
        group_id: NOTEPAD_CLASSIC_ID,
        process_count: 1,
        memory_kb: 4096,
        started_ms: 7,
    };
    set.add(
        "launch 会话卡语义",
        abs.session.is_none() && present_card.well_formed(),
        "缺席无卡（不出假卡）；在位之卡必良构",
    );
    // 5. 出参编码：定长槽无截断损坏，消息可复读。
    let mut out = [0u8; OUT_HEAD + MSG_MAX];
    let n = encode_status(&mut out, &abs);
    let msg_len = u16::from_le_bytes([out[10], out[11]]) as usize;
    let msg_ok = n == OUT_HEAD + msg_len && out[12..12 + msg_len] == state_msg(&abs)[..msg_len];
    set.add(
        "launch 出参编码",
        n > OUT_HEAD && out[0] == LaunchState::AcceptedRuntimeAbsent as u8 && msg_ok,
        "定长槽编码无格式化拼接——零堆纪律下可复读",
    );
    set
}
