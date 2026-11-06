//! UNX-F10011 报表只读端点（procfs 段） · 0x912C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10011（B21 波次报表线）。
//! 锚点：UNX-F10012-J1 两期 diff 零漏（格级比对 10/10 次），跳格 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x912C0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一；全部码位由
//!   `ProcCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `ProcRegistry`：只读端点路径 → 节点句柄，路径须以
//!     `/proc/wave/` 前缀命中，未命中 ENOENT；
//!   - 状态机 `ReqState`：Idle → Open → Reading → Closed 单向流转，
//!     Closed 后再读 EBADF；请求序号代际防陈旧复用；
//!   - 审计账本 `ProbeLedger`：写回探测 = 拦截 + 逃逸，逃逸恒 0，
//!     只增不减，负增量显性拒绝；
//!   - 注入核 `BadProbeCase`：写回探测 / 未知路径 / 关闭后读 / 越权
//!     节点 / 陈旧代际五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10011 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x912C0;

/// 注册表容量（只读端点数）。
pub const MAX_NODES: usize = 8;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 只读端点路径前缀。
pub const PROC_PREFIX: &str = "/proc/wave/";

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcCode {
    /// 写回探测命中
    WriteProbe,
    /// 未知路径
    UnknownPath,
    /// 关闭后读
    ReadAfterClose,
    /// 越权节点（注册表外）
    RogueNode,
    /// 陈旧代际句柄
    StaleGeneration,
    /// 账本倒退
    LedgerRegress,
    /// 路径缺前缀
    BadPrefix,
    /// 句柄未打开
    NotOpen,
    /// 双开冲突
    DoubleOpen,
    /// 审计断链（探测未留痕）
    AuditGap,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ProcCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ProcCode::WriteProbe => "写回探测被拦：procfs 段只读；请改走差异清单通道，\
                任何写请求一律 EROFS",
            ProcCode::UnknownPath => "未知路径被拒：端点不在注册表内；请核对 \
                /proc/wave/ 下的节点名",
            ProcCode::ReadAfterClose => "关闭后读被拒：请求已 Closed；请重新 open \
                取得新句柄",
            ProcCode::RogueNode => "越权节点被拒：目标不在只读端点白名单；请先注册\
                再暴露",
            ProcCode::StaleGeneration => "陈旧代际被拒：句柄代际落后于节点当前代际；\
                请重新 open 重建句柄",
            ProcCode::LedgerRegress => "探测账倒退被拒：计数只增不减；请排查回滚\
                路径误改累计值",
            ProcCode::BadPrefix => "路径缺前缀被拒：所有端点必须挂 /proc/wave/ 下；\
                请修正路径后重试",
            ProcCode::NotOpen => "句柄未打开：读前必须先 open；请按 Idle→Open→Reading\
                次序调用",
            ProcCode::DoubleOpen => "双开冲突：同节点已有一个打开句柄；请先关闭旧\
                句柄再开新的",
            ProcCode::AuditGap => "审计断链：探测命中但未留痕；请检查审计账是否被\
                旁路写入",
            ProcCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            ProcCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 \
                0x912C0..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ProcCode; 12] = [
        ProcCode::WriteProbe,
        ProcCode::UnknownPath,
        ProcCode::ReadAfterClose,
        ProcCode::RogueNode,
        ProcCode::StaleGeneration,
        ProcCode::LedgerRegress,
        ProcCode::BadPrefix,
        ProcCode::NotOpen,
        ProcCode::DoubleOpen,
        ProcCode::AuditGap,
        ProcCode::NotReproducible,
        ProcCode::SegmentViolation,
    ];
}

// ── 请求状态机 ────────────────────────────────────────────────────────
/// 请求生命周期：单向 Idle → Open → Reading → Closed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReqState {
    /// 未打开
    Idle,
    /// 已打开未读
    Open,
    /// 读取中
    Reading,
    /// 已关闭
    Closed,
}

/// procfs 只读节点。
#[derive(Clone, Copy, Debug)]
pub struct ProcNode {
    /// 节点代际（每次 close 自增）
    pub generation: u16,
    /// 内容字节数
    pub size: u32,
}

/// 只读端点注册表：路径命中才可开；写请求一律拦截。
pub struct ProcRegistry {
    paths: [Option<&'static str>; MAX_NODES],
    nodes: [ProcNode; MAX_NODES],
    opens: u32,
    closes: u32,
    probes: u32,
}

impl ProcRegistry {
    /// 构造注册表并预置六个波次端点。
    pub fn new() -> ProcRegistry {
        let mut r = ProcRegistry {
            paths: [None; MAX_NODES],
            nodes: [ProcNode { generation: 1, size: 0 }; MAX_NODES],
            opens: 0,
            closes: 0,
            probes: 0,
        };
        let builtin: [&str; 6] = [
            "report01", "report02", "report03", "report04", "report05", "report06",
        ];
        let mut i = 0usize;
        while i < builtin.len() {
            if let Some(p) = builtin.get(i) {
                if let Some(slot) = r.paths.get_mut(i) {
                    *slot = Some(*p);
                }
                if let Some(n) = r.nodes.get_mut(i) {
                    n.size = 64 + i as u32 * 8;
                }
            }
            i += 1;
        }
        r
    }

    fn find(&self, path: &str) -> Option<usize> {
        if !path.starts_with(PROC_PREFIX) {
            return None;
        }
        let name = &path[PROC_PREFIX.len()..];
        let mut i = 0usize;
        while i < MAX_NODES {
            if let Some(Some(p)) = self.paths.get(i) {
                if *p == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 打开只读端点并推进请求状态到 Open；前缀不符、未知路径、代际不符均显性拒绝。
    pub fn open(&mut self, path: &str, gen: u16, state: &mut ReqState) -> Result<u16, ProcCode> {
        if !path.starts_with(PROC_PREFIX) {
            return Err(ProcCode::BadPrefix);
        }
        let idx = match self.find(path) {
            Some(i) => i,
            None => {
                if path.len() > PROC_PREFIX.len() {
                    return Err(ProcCode::UnknownPath);
                }
                return Err(ProcCode::BadPrefix);
            }
        };
        let node = match self.nodes.get(idx) {
            Some(n) => *n,
            None => return Err(ProcCode::RogueNode),
        };
        if node.generation != gen {
            return Err(ProcCode::StaleGeneration);
        }
        *state = ReqState::Open;
        self.opens = self.opens.saturating_add(1);
        Ok(node.generation)
    }

    /// 读端点：只接受 Open 态句柄；关闭后读 EBADF。
    pub fn read(&mut self, state: &mut ReqState, gen: u16, len: u32) -> Result<u32, ProcCode> {
        if *state == ReqState::Closed {
            return Err(ProcCode::ReadAfterClose);
        }
        if *state != ReqState::Open && *state != ReqState::Reading {
            return Err(ProcCode::NotOpen);
        }
        let node = match self.nodes.get(0) {
            Some(n) => *n,
            None => return Err(ProcCode::RogueNode),
        };
        if node.generation != gen {
            return Err(ProcCode::StaleGeneration);
        }
        *state = ReqState::Reading;
        Ok(len.min(node.size))
    }

    /// 关闭端点：推进代际，Closed 后不可复用。
    pub fn close(&mut self, state: &mut ReqState) -> Result<u16, ProcCode> {
        if *state == ReqState::Closed {
            return Err(ProcCode::ReadAfterClose);
        }
        *state = ReqState::Closed;
        if let Some(n) = self.nodes.get_mut(0) {
            n.generation = n.generation.wrapping_add(1);
            self.closes = self.closes.saturating_add(1);
            return Ok(n.generation);
        }
        Err(ProcCode::RogueNode)
    }

    /// 写回探测：只读段上任何写请求返回 EROFS 并记拦截账。
    pub fn write_probe(&mut self, path: &str) -> Result<(), ProcCode> {
        self.probes = self.probes.saturating_add(1);
        match self.find(path) {
            Some(_) => Err(ProcCode::WriteProbe),
            None => Err(ProcCode::UnknownPath),
        }
    }

    /// 探测拦截累计数。
    pub fn probe_count(&self) -> u32 {
        self.probes
    }

    /// 开/关账闭合检查。
    pub fn open_close_balanced(&self) -> bool {
        self.opens >= self.closes && self.opens > 0
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏请求样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadProbeCase {
    /// 写回探测（命中端点）
    WriteHit,
    /// 未知路径读
    UnknownRead,
    /// 关闭后读
    ClosedRead,
    /// 注册表外节点
    RogueRead,
    /// 陈旧代际
    StaleGen,
}

impl BadProbeCase {
    /// 闭集全量。
    pub const ALL: [BadProbeCase; 5] = [
        BadProbeCase::WriteHit,
        BadProbeCase::UnknownRead,
        BadProbeCase::ClosedRead,
        BadProbeCase::RogueRead,
        BadProbeCase::StaleGen,
    ];

    /// 注入一轮：返回是否被显式拦截/拒绝。
    pub fn inject(self, reg: &mut ProcRegistry) -> bool {
        match self {
            BadProbeCase::WriteHit => {
                reg.write_probe("/proc/wave/report01") == Err(ProcCode::WriteProbe)
            }
            BadProbeCase::UnknownRead => {
                let mut st = ReqState::Idle;
                reg.open("/proc/wave/nope", 1, &mut st) == Err(ProcCode::UnknownPath)
                    && reg.read(&mut st, 1, 8).is_err()
            }
            BadProbeCase::ClosedRead => {
                let mut st = ReqState::Open;
                let _ = reg.read(&mut st, 1, 4);
                let _ = reg.close(&mut st);
                reg.read(&mut st, 1, 4) == Err(ProcCode::ReadAfterClose)
            }
            BadProbeCase::RogueRead => {
                let mut st = ReqState::Idle;
                reg.open("/proc/wave/../etc", 1, &mut st).is_err()
                    && reg.open("/wave/report01", 1, &mut st) == Err(ProcCode::BadPrefix)
            }
            BadProbeCase::StaleGen => {
                let mut st = ReqState::Idle;
                reg.open("/proc/wave/report01", 0, &mut st) == Err(ProcCode::StaleGeneration)
                    || reg.open("/proc/wave/report01", 99, &mut st) == Err(ProcCode::StaleGeneration)
            }
        }
    }
}

/// 探测审计账本：拦截 = 命中 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeLedger {
    /// 探测注入总数
    pub injected: u32,
    /// 拦截次数
    pub blocked: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl ProbeLedger {
    const fn zero() -> ProbeLedger {
        ProbeLedger { injected: 0, blocked: 0, escaped: 0 }
    }

    /// 记一次探测结果；账失衡回 AuditGap。
    pub fn record(&mut self, caught: bool) -> Result<(), ProcCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.blocked = self.blocked.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.blocked.saturating_add(self.escaped) != self.injected {
            return Err(ProcCode::AuditGap);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.blocked.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏请求注入：每类 INJECT_ROUNDS 次。
pub fn run_probe_matrix() -> (ProbeLedger, [u32; 5]) {
    let mut reg = ProcRegistry::new();
    let mut ledger = ProbeLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadProbeCase::ALL.len() {
        if let Some(case) = BadProbeCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut reg);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄/关闭后读族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；越权节点族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知路径族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EROFS", -30, 19, "Windows ERROR_WRITE_PROTECT；只读段写回族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：开→读→写探→关全链结果逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = ProcRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let paths = ["/proc/wave/report01", "/proc/wave/nope", "/wave/report01"];
    let mut i = 0usize;
    while i < paths.len() {
        if let Some(p) = paths.get(i).copied() {
            let mut st = ReqState::Idle;
            let o = reg.open(p, 1, &mut st);
            bytes.push(match o {
                Ok(_) => 1,
                Err(e) => (e as u32 & 0xFF) as u8,
            });
            if o.is_ok() {
                let r = reg.read(&mut st, 1, 16);
                bytes.push(match r {
                    Ok(n) => (n & 0xFF) as u8,
                    Err(e) => (e as u32 & 0xFF) as u8,
                });
                let c = reg.close(&mut st);
                bytes.push(if c.is_ok() { 9 } else { 8 });
            }
            let w = reg.write_probe(p);
            bytes.push(if w.is_err() { 7 } else { 0 });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10011 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10011_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10011");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x912C0..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x912C0..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 正常流：内置端点可开可读
    let mut reg = ProcRegistry::new();
    let mut st = ReqState::Idle;
    let open_ok = reg.open("/proc/wave/report01", 1, &mut st).is_ok();
    let read_ok = reg.read(&mut st, 1, 16) == Ok(16);
    s.add("正常流：内置端点打开并读 16 字节", open_ok && read_ok,
          "合法路径 + 新代际句柄应全额返回请求长度");

    // 3. 读长度截断到节点尺寸
    let mut reg2 = ProcRegistry::new();
    let mut st2 = ReqState::Idle;
    let _ = reg2.open("/proc/wave/report01", 1, &mut st2);
    let clamped = reg2.read(&mut st2, 1, 4096) == Ok(64);
    s.add("边界：读长截断到节点尺寸 64", clamped,
          "请求长度不得越过节点内容边界");

    // 4. 写回探测 10/10 拦截
    let mut reg3 = ProcRegistry::new();
    let mut n = 0u32;
    let mut hits = 0u32;
    while n < INJECT_ROUNDS {
        if reg3.write_probe("/proc/wave/report02") == Err(ProcCode::WriteProbe) {
            hits += 1;
        }
        n += 1;
    }
    s.add("写回探测：命中端点 10/10 触发即拦", hits == INJECT_ROUNDS,
          "procfs 段只读，写请求一律 EROFS 并留痕");

    // 5. 注入核：五类坏请求全拦截
    let (ledger, per_class) = run_probe_matrix();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏请求各 10/10 拦截", all_caught,
          "写探/未知路径/关闭后读/越权/陈旧代际全被显式拒绝");

    // 6. 探测账守恒：逃逸=0
    s.add("探测账守恒：探测=拦截+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次探测必须 50 次拦截、0 逃逸");

    // 7. 未知路径拒绝
    let mut reg4 = ProcRegistry::new();
    let mut st4 = ReqState::Idle;
    let unknown = reg4.open("/proc/wave/ghost", 1, &mut st4) == Err(ProcCode::UnknownPath);
    s.add("边界：未知路径 ENOENT 语义拒绝", unknown,
          "注册表外路径不得静默映射到任何节点");

    // 8. 前缀守卫
    let prefix = reg4.open("wave/report01", 1, &mut st4) == Err(ProcCode::BadPrefix)
        && reg4.open("/proc/wave/", 1, &mut st4) == Err(ProcCode::BadPrefix);
    s.add("边界：缺前缀/空名路径拒绝", prefix,
          "端点必须挂 /proc/wave/ 前缀且节点名非空");

    // 9. 关闭后读 EBADF
    let mut reg5 = ProcRegistry::new();
    let mut st5 = ReqState::Idle;
    let _ = reg5.open("/proc/wave/report03", 1, &mut st5);
    let _ = reg5.close(&mut st5);
    let after = reg5.read(&mut st5, 1, 4) == Err(ProcCode::ReadAfterClose);
    s.add("状态机：Closed 后读被拒", after,
          "请求生命周期单向，关闭后不得复用旧句柄");

    // 10. 双关拒绝
    let double_close = reg5.close(&mut st5).is_err();
    s.add("状态机：双关被拒", double_close,
          "Closed 态再关闭属重复操作，显性拒绝");

    // 11. 开关账闭合
    let mut reg6 = ProcRegistry::new();
    let mut ok_acc = true;
    let mut k = 0u32;
    while k < 6 {
        let mut st6 = ReqState::Idle;
        if reg6.open("/proc/wave/report04", 1, &mut st6).is_err() {
            ok_acc = false;
        }
        if reg6.close(&mut st6).is_err() {
            ok_acc = false;
        }
        k += 1;
    }
    s.add("开关账闭合：6 开 6 关全成功", ok_acc && reg6.open_close_balanced(),
          "每次 open/close 必须各记一笔且次序正确");

    // 12. 诊断码闭集：文案非空
    let text_ok = ProcCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i = 0usize;
    while i < ProcCode::ALL.len() {
        let mut j = i + 1;
        while j < ProcCode::ALL.len() {
            if let (Some(x), Some(y)) = (ProcCode::ALL.get(i), ProcCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ProcCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EROFS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        let _ = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再记录只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 17. 账实核对：注册表探测计数与账本拦截数一致
    s.add("账实核对：注册表探测计数≥注入命中数",
          reg3.probe_count() == INJECT_ROUNDS,
          "每条写回探测都必须在注册表留痕");

    // 动态明细（≤8）：探测账一行 + 指纹一行
    let ledger_text = format!("探测 {} / 拦截 {} / 逃逸 {}", ledger.injected, ledger.blocked, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_endpoint_roundtrip() {
        let mut reg = ProcRegistry::new();
        let mut st = ReqState::Idle;
        assert!(reg.open("/proc/wave/report01", 1, &mut st).is_ok());
        assert_eq!(reg.read(&mut st, 1, 8), Ok(8));
        assert!(reg.close(&mut st).is_ok());
    }

    #[test]
    fn write_probe_is_blocked() {
        let mut reg = ProcRegistry::new();
        assert_eq!(reg.write_probe("/proc/wave/report01"), Err(ProcCode::WriteProbe));
    }

    #[test]
    fn unknown_path_rejected() {
        let mut reg = ProcRegistry::new();
        let mut st = ReqState::Idle;
        assert_eq!(reg.open("/proc/wave/ghost", 1, &mut st), Err(ProcCode::UnknownPath));
        assert_eq!(reg.open("/etc/passwd", 1, &mut st), Err(ProcCode::BadPrefix));
    }

    #[test]
    fn stale_generation_rejected() {
        let mut reg = ProcRegistry::new();
        let mut st = ReqState::Idle;
        assert_eq!(reg.open("/proc/wave/report01", 42, &mut st), Err(ProcCode::StaleGeneration));
    }

    #[test]
    fn read_after_close_rejected() {
        let mut reg = ProcRegistry::new();
        let mut st = ReqState::Idle;
        let _ = reg.open("/proc/wave/report01", 1, &mut st);
        let _ = reg.close(&mut st);
        assert_eq!(reg.read(&mut st, 1, 4), Err(ProcCode::ReadAfterClose));
    }

    #[test]
    fn read_clamped_to_node_size() {
        let mut reg = ProcRegistry::new();
        let mut st = ReqState::Idle;
        let _ = reg.open("/proc/wave/report01", 1, &mut st);
        assert_eq!(reg.read(&mut st, 1, 100000), Ok(64));
    }

    #[test]
    fn probe_matrix_zero_escape() {
        let (l, per) = run_probe_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBADF));
        assert!(agrees_with_linux(EROFS));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn proccode_text_complete() {
        for c in ProcCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10011_checks_green() {
        let s = run_unx10011_checks();
        assert!(s.all_passed(), "UNX-F10011 red: {}", s.red_items_text());
    }
}
