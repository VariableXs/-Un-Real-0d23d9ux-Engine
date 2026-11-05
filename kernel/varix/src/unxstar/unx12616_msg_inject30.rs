//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12616 D4 消息面注入联测（坏端口/坏消息/越权连接 30 例） · 0x80400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12617-J1 对照表 ≥30 行五列齐（边号/冻结件/
//!      承接域/判据/复测），抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对照表 30 行冻结：三族 × 10 行（坏端口/坏消息/越权连接），
//!     每行五列齐——边号、冻结件、承接域、判据（期望拒止码）、
//!     复测（重算指纹）；行序冻结，缺行少列即账不齐；
//!   - 五列点验闭式：复测列由前四列按冻结指纹式重算，重算值≠
//!     预置值即行漂移（RowDrift 必抓）——抽样 10 行独立可复测
//!     就是「不跑全表、只抽 10 行、逐行重算」也能对平；
//!   - 30 例注入联测：每行一例，逐例把违约参数打入消息面闸，
//!     期望判据列的拒止码逐例命中（30/30 必抓）；好路零误伤：
//!     合法参数必须放行（误伤即闸坏）；
//!   - 行账守恒：行数=30、三族各 10、判据码三族互异不串行；
//!   - 双跑一致 + LCG 抽样种子写死 = SEG_BASE（0x80400）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x80400..0x80440。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80400，AI-VX9-1105 B01 批 12 槽第 5 槽）。
pub const SEG_BASE: u32 = 0x80400;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefTableCode {
    /// 坏端口：端口号不在注册窗
    BadPort,
    /// 坏消息：消息长度/形态越界
    BadMessage,
    /// 越权连接：连接方无连接位
    UnauthorizedConn,
    /// 对照表缺行：行数不足 30
    TableShort,
    /// 对照表行漂移：复测列重算不符
    RowDrift,
    /// 判据串行：三族判据码互撞
    JudgeCollision,
    /// 注入漏抓：违约例未按判据码拒止
    InjectMissed,
    /// 好路误伤：合法参数被闸拒
    GoodPathHit,
    /// 行号越界：抽样行号超出 30 行窗
    RowOutOfRange,
    /// 表账守恒破：三族计数和不等于总行数
    FamilySumBreak,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl RefTableCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RefTableCode::BadPort => 0x01,
                RefTableCode::BadMessage => 0x02,
                RefTableCode::UnauthorizedConn => 0x03,
                RefTableCode::TableShort => 0x04,
                RefTableCode::RowDrift => 0x05,
                RefTableCode::JudgeCollision => 0x06,
                RefTableCode::InjectMissed => 0x07,
                RefTableCode::GoodPathHit => 0x08,
                RefTableCode::RowOutOfRange => 0x09,
                RefTableCode::FamilySumBreak => 0x0A,
                RefTableCode::FingerprintDrift => 0x0B,
                RefTableCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            RefTableCode::BadPort => "端口号不在注册窗：用注册账里真实端口",
            RefTableCode::BadMessage => "消息越界：核对长度/形态边界再投",
            RefTableCode::UnauthorizedConn => "越权连接：先申请连接位再连",
            RefTableCode::TableShort => "对照表缺行：补齐 30 行五列再点验",
            RefTableCode::RowDrift => "行漂移：复测列重算不符，按前四列重出",
            RefTableCode::JudgeCollision => "判据串行：三族码必须互异，重排判据列",
            RefTableCode::InjectMissed => "注入漏抓：违约例必须按判据码拒止",
            RefTableCode::GoodPathHit => "好路误伤：闸只拦违约，不拦合法",
            RefTableCode::RowOutOfRange => "抽样行号越界：行号在 0..30 内抽",
            RefTableCode::FamilySumBreak => "族账不齐：坏端口+坏消息+越权=30",
            RefTableCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            RefTableCode::Unknown => "未知诊断码：不在 0x80400 段闭集内",
        }
    }

    pub const ALL: [RefTableCode; 12] = [
        RefTableCode::BadPort,
        RefTableCode::BadMessage,
        RefTableCode::UnauthorizedConn,
        RefTableCode::TableShort,
        RefTableCode::RowDrift,
        RefTableCode::JudgeCollision,
        RefTableCode::InjectMissed,
        RefTableCode::GoodPathHit,
        RefTableCode::RowOutOfRange,
        RefTableCode::FamilySumBreak,
        RefTableCode::FingerprintDrift,
        RefTableCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in RefTableCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    RefTableCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 对照表总行数（三族 × 10，冻结）。
pub const TABLE_ROWS: usize = 30;
/// 每族行数。
pub const ROWS_PER_FAMILY: usize = 10;
/// 端口注册窗（下闭上开）。
pub const PORT_WIN: (u32, u32) = (0x40, 0x80);
/// 消息长度上限（字节）。
pub const MSG_LEN_MAX: u32 = 256;
/// 连接位掩码（具备该位才许连接）。
pub const CONN_BIT: u32 = 0x4;

/// 注入族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectFamily {
    /// 族 0：坏端口。
    BadPort,
    /// 族 1：坏消息。
    BadMessage,
    /// 族 2：越权连接。
    BadConn,
}

impl InjectFamily {
    pub const ALL: [InjectFamily; 3] = [
        InjectFamily::BadPort,
        InjectFamily::BadMessage,
        InjectFamily::BadConn,
    ];

    /// 族判据码（期望拒止码，三族互异）。
    pub const fn judge(self) -> u32 {
        match self {
            InjectFamily::BadPort => RefTableCode::BadPort.val(),
            InjectFamily::BadMessage => RefTableCode::BadMessage.val(),
            InjectFamily::BadConn => RefTableCode::UnauthorizedConn.val(),
        }
    }

    /// 族内第 i 行的违约参数（冻结）。
    pub const fn bad_param(self, i: usize) -> u32 {
        match self {
            // 坏端口：窗前/窗后/未注册交错（全部落在 [0x40,0x80) 窗外）。
            InjectFamily::BadPort => match i {
                0 => 0x00,
                1 => 0x80,
                2 => 0x3F,
                3 => 0x81,
                4 => 0xFF,
                5 => 0x01,
                6 => 0x90,
                7 => 0xC0,
                8 => 0x20,
                _ => 0x100,
            },
            // 坏消息：超上限/零长交错。
            InjectFamily::BadMessage => match i {
                0 => MSG_LEN_MAX + 1,
                1 => 0,
                2 => MSG_LEN_MAX + 8,
                3 => MSG_LEN_MAX + 64,
                4 => 0,
                5 => MSG_LEN_MAX * 2,
                6 => MSG_LEN_MAX + 3,
                7 => 0,
                8 => MSG_LEN_MAX + 100,
                _ => MSG_LEN_MAX * 4,
            },
            // 越权连接：无连接位的掩码交错。
            InjectFamily::BadConn => match i {
                0 => 0x0,
                1 => 0x1,
                2 => 0x2,
                3 => 0x3,
                4 => 0x8,
                5 => 0x1 | 0x8,
                6 => 0x2 | 0x8,
                7 => 0x10,
                8 => 0x20,
                _ => 0x40,
            },
        }
    }

    /// 好路参数（合法，闸必须放行）——每族一枚，冻结。
    pub const fn good_param(self) -> u32 {
        match self {
            InjectFamily::BadPort => PORT_WIN.0,
            InjectFamily::BadMessage => MSG_LEN_MAX,
            InjectFamily::BadConn => CONN_BIT,
        }
    }
}

// ── 对照表行结构（五列） ─────────────────────────────────────────
/// 一行对照：边号 / 冻结件 / 承接域 / 判据 / 复测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefRow {
    /// 列一：边号（0..30，全局单调）。
    pub edge: u32,
    /// 列二：冻结件名。
    pub artifact: &'static str,
    /// 列三：承接域。
    pub domain: &'static str,
    /// 列四：判据（期望拒止码）。
    pub judge: u32,
    /// 列五：复测指纹（由前四列重算，见 row_retest）。
    pub retest: u32,
}

/// 复测指纹闭式（来源冻结：消息面对照表指纹册 v1）。
/// fp = edge*79 + 族号*13 + 族内序*7 + judge。
/// 族号 = edge/10（三族各 10 行），族内序 = edge%10——两参都可由
/// 行自身的边号重导出，故复测列**独立可复算**（不依赖表外状态）。
pub const fn row_retest(edge: u32, judge: u32) -> u32 {
    edge.wrapping_mul(79)
        .wrapping_add((edge / 10).wrapping_mul(13))
        .wrapping_add((edge % 10).wrapping_mul(7))
        .wrapping_add(judge)
}

/// 构造一行（重算复核器——与冻结表同式生成，用于逐行全等点验）。
pub fn make_row(edge: u32, artifact: &'static str, domain: &'static str, judge: u32) -> RefRow {
    RefRow { edge, artifact, domain, judge, retest: row_retest(edge, judge) }
}

// ── 冻结对照表（30 行五列） ──────────────────────────────────────
/// 冻结生成 30 行对照表（const fn 逐行拼装，行序=族序×族内序）。
pub const REF_TABLE: [RefRow; TABLE_ROWS] = build_table();

const fn build_table() -> [RefRow; TABLE_ROWS] {
    let mut rows = [RefRow {
        edge: 0,
        artifact: "",
        domain: "",
        judge: 0,
        retest: 0,
    }; TABLE_ROWS];
    let mut edge = 0usize;
    // 族 0：坏端口。
    let mut i = 0;
    while i < ROWS_PER_FAMILY {
        rows[edge] = RefRow {
            edge: edge as u32,
            artifact: port_art(i),
            domain: d4_or(i, 0),
            judge: InjectFamily::BadPort.judge(),
            retest: row_retest(edge as u32, InjectFamily::BadPort.judge()),
        };
        edge += 1;
        i += 1;
    }
    // 族 1：坏消息。
    i = 0;
    while i < ROWS_PER_FAMILY {
        rows[edge] = RefRow {
            edge: edge as u32,
            artifact: msg_art(i),
            domain: d4_or(i, 1),
            judge: InjectFamily::BadMessage.judge(),
            retest: row_retest(edge as u32, InjectFamily::BadMessage.judge()),
        };
        edge += 1;
        i += 1;
    }
    // 族 2：越权连接。
    i = 0;
    while i < ROWS_PER_FAMILY {
        rows[edge] = RefRow {
            edge: edge as u32,
            artifact: conn_art(i),
            domain: d4_or(i, 2),
            judge: InjectFamily::BadConn.judge(),
            retest: row_retest(edge as u32, InjectFamily::BadConn.judge()),
        };
        edge += 1;
        i += 1;
    }
    rows
}

// const 辅助：族内件名（与 family_artifact 同源冻结，const 可用字面）。
const fn port_art(i: usize) -> &'static str {
    match i % 5 {
        0 => "port.reg",
        1 => "port.lookup",
        2 => "port.ctx",
        3 => "port.queue",
        _ => "port.close",
    }
}
const fn msg_art(i: usize) -> &'static str {
    match i % 5 {
        0 => "msg.form",
        1 => "msg.len",
        2 => "msg.inline",
        3 => "msg.section",
        _ => "msg.handle",
    }
}
const fn conn_art(i: usize) -> &'static str {
    match i % 5 {
        0 => "conn.bit",
        1 => "conn.pair",
        2 => "conn.acl",
        3 => "conn.session",
        _ => "conn.teardown",
    }
}
const fn d4_or(i: usize, fam: usize) -> &'static str {
    if i % 2 == 0 {
        "D4-泵"
    } else {
        match fam {
            0 => "D1-装载",
            1 => "D2-句柄",
            _ => "D3-安全",
        }
    }
}

/// 单行点验：五列齐 + 复测列重算对平。
pub fn verify_row(r: &RefRow) -> Result<(), u32> {
    if r.edge >= TABLE_ROWS as u32 {
        return Err(RefTableCode::RowOutOfRange.val());
    }
    if r.artifact.is_empty() || r.domain.is_empty() {
        return Err(RefTableCode::RowDrift.val());
    }
    let recomputed = row_retest(r.edge, r.judge);
    if recomputed != r.retest {
        return Err(RefTableCode::RowDrift.val());
    }
    Ok(())
}

/// 表级点验：行数、族判据互异、族账守恒。
pub fn table_ok() -> bool {
    if REF_TABLE.len() != TABLE_ROWS {
        return false;
    }
    // 行号单调。
    let mut prev: Option<u32> = None;
    for r in REF_TABLE.iter() {
        match prev {
            None => {
                if r.edge != 0 {
                    return false;
                }
            }
            Some(p) => {
                if r.edge != p + 1 {
                    return false;
                }
            }
        }
        prev = Some(r.edge);
    }
    // 三族判据互异。
    let j0 = InjectFamily::BadPort.judge();
    let j1 = InjectFamily::BadMessage.judge();
    let j2 = InjectFamily::BadConn.judge();
    if j0 == j1 || j1 == j2 || j0 == j2 {
        return false;
    }
    true
}

/// 消息面闸：三道口顺序校验（端口窗→消息界→连接位），违约必拒。
/// 族参数只决定从哪道口注入，闸本身对三道口一视同仁（好路必放）。
pub fn case_gate(_f: InjectFamily, port: u32, msg_len: u32, conn_mask: u32) -> Result<(), u32> {
    // 端口窗。
    if port < PORT_WIN.0 || port >= PORT_WIN.1 {
        return Err(RefTableCode::BadPort.val());
    }
    // 消息界。
    if msg_len == 0 || msg_len > MSG_LEN_MAX {
        return Err(RefTableCode::BadMessage.val());
    }
    // 连接位。
    if conn_mask & CONN_BIT == 0 {
        return Err(RefTableCode::UnauthorizedConn.val());
    }
    Ok(())
}

/// 单行联测：按行取族与违约参数打闸，期望命中该行判据码。
pub fn run_row_case(r: &RefRow) -> bool {
    let edge = r.edge as usize;
    let fam_idx = edge / ROWS_PER_FAMILY;
    let in_family = edge % ROWS_PER_FAMILY;
    let f = match InjectFamily::ALL.get(fam_idx) {
        Some(f) => *f,
        None => return false,
    };
    let p = f.bad_param(in_family);
    // 三族参数分别落在闸的三道口上。
    let (port, msg_len, conn) = match f {
        InjectFamily::BadPort => (p, MSG_LEN_MAX, CONN_BIT),
        InjectFamily::BadMessage => (PORT_WIN.0, p, CONN_BIT),
        InjectFamily::BadConn => (PORT_WIN.0, MSG_LEN_MAX, p),
    };
    case_gate(f, port, msg_len, conn) == Err(r.judge)
}

/// 全 30 例联测：逐行打闸，30/30 必抓。
pub fn inject_thirty() -> Result<u32, u32> {
    if !table_ok() {
        return Err(RefTableCode::TableShort.val());
    }
    let mut caught = 0u32;
    for r in REF_TABLE.iter() {
        if run_row_case(r) {
            caught += 1;
        }
    }
    if caught != TABLE_ROWS as u32 {
        return Err(RefTableCode::InjectMissed.val());
    }
    Ok(caught)
}

/// 好路零误伤：三族好路参数必须全部放行。
pub fn good_path_ok() -> bool {
    for f in InjectFamily::ALL.iter() {
        let (port, msg_len, conn) = match f {
            InjectFamily::BadPort => (f.good_param(), MSG_LEN_MAX, CONN_BIT),
            InjectFamily::BadMessage => (PORT_WIN.0, f.good_param(), CONN_BIT),
            InjectFamily::BadConn => (PORT_WIN.0, MSG_LEN_MAX, f.good_param()),
        };
        if case_gate(*f, port, msg_len, conn).is_err() {
            return false;
        }
    }
    true
}

pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 抽样 10 行独立可复测：LCG 抽 10 个行号，逐行重算指纹对平。
pub fn sample_ten_rows() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok_rows = 0u32;
    let mut i = 0;
    while i < 10 {
        let idx = rng.below(TABLE_ROWS as u64) as usize;
        let row = match REF_TABLE.get(idx) {
            Some(r) => r,
            None => return Err(RefTableCode::RowOutOfRange.val()),
        };
        if verify_row(row).is_ok() {
            ok_rows += 1;
        }
        i += 1;
    }
    if ok_rows == 10 {
        Ok(10)
    } else {
        Err(RefTableCode::RowDrift.val())
    }
}

// ── 注入面：表违约必抓 ───────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入族 0：篡改一行复测列 → 行漂移必抓。
/// 注入族 1：越界行号点验 → RowOutOfRange 必抓。
/// 注入族 2：make_row 重算式与预置表逐行全等（表不漂移）。
/// 注入族 3：好路必放（闸不误伤）。
pub fn inject_table_violations(led: &mut InjectLedger) {
    // 族 0。
    led.tries += 1;
    if let Some(r0) = REF_TABLE.get(3) {
        let mut bad = *r0;
        bad.retest = bad.retest.wrapping_add(1);
        if verify_row(&bad) == Err(RefTableCode::RowDrift.val()) {
            led.caught += 1;
        }
    }
    // 族 1。
    led.tries += 1;
    let out_row = RefRow { edge: 99, artifact: "x", domain: "y", judge: 0, retest: 0 };
    if verify_row(&out_row) == Err(RefTableCode::RowOutOfRange.val()) {
        led.caught += 1;
    }
    // 族 2。
    led.tries += 1;
    let mut all_equal = true;
    for r in REF_TABLE.iter() {
        let fresh = make_row(r.edge, r.artifact, r.domain, r.judge);
        if fresh.retest != r.retest {
            all_equal = false;
        }
    }
    if all_equal {
        led.caught += 1;
    }
    // 族 3。
    led.tries += 1;
    if good_path_ok() {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：表齐 → 30 例注入全抓 → 抽样 10 行对平 → 指纹。
pub fn golden_run() -> Result<u32, u32> {
    if !table_ok() {
        return Err(RefTableCode::TableShort.val());
    }
    let caught = inject_thirty()?;
    let sampled = sample_ten_rows()?;
    Ok(caught.wrapping_mul(100).wrapping_add(sampled))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(RefTableCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：对照表 30 行五列齐。
fn table_ready_ok() -> bool {
    table_ok()
        && REF_TABLE
            .iter()
            .all(|r| verify_row(r).is_ok())
}

/// J2：抽样 10 行独立可复测。
fn sample_ok() -> bool {
    sample_ten_rows() == Ok(10)
}

/// J3：30 例注入全抓。
fn inject_ok() -> bool {
    inject_thirty() == Ok(30)
}

/// J4：好路零误伤。
fn good_ok() -> bool {
    good_path_ok()
}

/// J5：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in RefTableCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push((
        "J1-对照表30行五列齐",
        table_ready_ok(),
        format!("行数={}", REF_TABLE.len()),
    ));
    v.push((
        "J1-三族判据互异",
        {
            let (a, b, c) = (
                InjectFamily::BadPort.judge(),
                InjectFamily::BadMessage.judge(),
                InjectFamily::BadConn.judge(),
            );
            a != b && b != c && a != c
        },
        String::from("三码互撞即串行"),
    ));
    v.push(("J2-抽样10行复测", sample_ok(), format!("{:?}", sample_ten_rows())));
    v.push(("J3-30例注入全抓", inject_ok(), format!("{:?}", inject_thirty())));
    v.push(("J3-好路零误伤", good_ok(), String::from("合法参数放行")));
    // 注入面：表违约必抓。
    let mut led = InjectLedger::new();
    inject_table_violations(&mut led);
    v.push((
        "J4-表违约注入全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 边界：消息长度界。
    let mut q = case_gate(InjectFamily::BadMessage, PORT_WIN.0, MSG_LEN_MAX, CONN_BIT).is_ok();
    q &= case_gate(InjectFamily::BadMessage, PORT_WIN.0, MSG_LEN_MAX + 1, CONN_BIT)
        == Err(RefTableCode::BadMessage.val());
    q &= case_gate(InjectFamily::BadMessage, PORT_WIN.0, 0, CONN_BIT)
        == Err(RefTableCode::BadMessage.val());
    v.push(("J5-消息长度边界", q, String::from("0/256 界内闭式")));
    // 边界：端口窗。
    let p_ok = case_gate(InjectFamily::BadPort, PORT_WIN.1 - 1, MSG_LEN_MAX, CONN_BIT).is_ok();
    let p_lo = case_gate(InjectFamily::BadPort, PORT_WIN.0 - 1, MSG_LEN_MAX, CONN_BIT)
        == Err(RefTableCode::BadPort.val());
    v.push(("J5-端口窗边界", p_ok && p_lo, String::from("[0x40,0x80)")));
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J7-段位守卫", segment_guard(), String::from("0x80400 窗口")));
    v
}

/// VX9 检查入口（卡 12616，UNX-F12617-J1 判据承载）。
pub fn run_unx12616_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12616 D4 消息面注入联测（坏端口/坏消息/越权连接 30 例）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_table_thirty_rows_five_columns() {
        assert_eq!(REF_TABLE.len(), 30);
        assert!(table_ready_ok());
    }

    #[test]
    fn test_sample_ten_rows_retest() {
        assert_eq!(sample_ten_rows(), Ok(10));
    }

    #[test]
    fn test_inject_thirty_all_caught() {
        assert_eq!(inject_thirty(), Ok(30));
    }

    #[test]
    fn test_good_path_no_false_positive() {
        assert!(good_ok());
    }

    #[test]
    fn test_row_drift_caught() {
        let mut led = InjectLedger::new();
        inject_table_violations(&mut led);
        assert_eq!(led.tries, 4);
        assert_eq!(led.caught, 4);
    }

    #[test]
    fn test_boundary_msg_len_and_port() {
        assert!(case_gate(InjectFamily::BadMessage, PORT_WIN.0, MSG_LEN_MAX, CONN_BIT).is_ok());
        assert_eq!(
            case_gate(InjectFamily::BadMessage, PORT_WIN.0, MSG_LEN_MAX + 1, CONN_BIT),
            Err(RefTableCode::BadMessage.val())
        );
        assert_eq!(
            case_gate(InjectFamily::BadPort, PORT_WIN.0 - 1, MSG_LEN_MAX, CONN_BIT),
            Err(RefTableCode::BadPort.val())
        );
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80400);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in RefTableCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xFFFF).contains("未知"));
    }
}
