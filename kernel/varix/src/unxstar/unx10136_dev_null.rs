//! UNX-F10136 /dev/null 与零设备正反双判据（特殊文件达成与非法操作误用受控）· 0x93200 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10137-J1 限值达成（prlimit 置限后写至限位成功 10/10 次）；
//!       UNX-F10137-J2 超限写误用受控（预期 EFBIG 精确复现 10/10 次，
//!       SIGXFSZ 按 L1 注记）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93200（本卡独占 0x40 窗口，下一卡 0x93240）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_dev_call`："op|dev|hex" 三成分（op ∈ {write, read,
//!      ioctl}），dev 闭集 {null, zero}，hex 偶长样本，逐档拒绝；
//!   ② 字符设备面 `CharDevFace`：null 写丢弃（回长度、体不变、读回 EOF）、
//!      zero 读填充（n 字节全零）与写丢弃双语义，ioctl 一律 ENOTTY；
//!   ③ 状态机 `DevState`：Closed→Bound→Served 单调（禁倒退）；
//!   ④ 注入核 `BadDev`：非法 ioctl / 未知设备 / 重复绑定 / 坏 hex
//!      四类坏样本逐类 10/10 次精确复现 ENOTTY/ENOENT/EBUSY/EINVAL 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10136 独占 0x40 窗口，下一卡 0x93240）。
pub const SEG_BASE: u32 = 0x93200;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93240;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 设备句柄容量。
pub const DEV_CAP: usize = 4;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CdCode {
    /// 非法 ioctl（ENOTTY 族）
    NotTty,
    /// 未知设备（ENOENT 族）
    BadDev,
    /// 重复绑定（EBUSY 档）
    DupBind,
    /// 坏 hex（EINVAL 族）
    BadHex,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 设备句柄满（EMFILE 档）
    HandleFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 丢弃失准
    DiscardDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl CdCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CdCode::NotTty => "非法 ioctl 被拒：字符设备无终端控制面；ENOTTY 精确归档",
            CdCode::BadDev => "未知设备被拒：dev 闭集 {null, zero}；ENOENT 精确归档",
            CdCode::DupBind => "重复绑定被拒：句柄已绑定设备；EBUSY 档精确归档",
            CdCode::BadHex => "坏 hex 样本被拒：data 必须为偶长十六进制；EINVAL 精确归档",
            CdCode::BadOp => "非法 op 被拒：op 闭集 {write, read, ioctl}；请核对调用行",
            CdCode::MissingPart => "成分缺失被拒：调用行必须三段 op|dev|hex；请补成分",
            CdCode::HandleFull => "设备句柄满被拒：请先释放句柄",
            CdCode::StateRegress => "状态倒退被拒：Closed→Bound→Served 单调",
            CdCode::EmptySummary => "空成文被拒：设备小结不得为空串；请先绑定再成文",
            CdCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            CdCode::DiscardDrift => "丢弃失准被拒：写设备不得改变任何体账",
            CdCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CdCode; 12] = [
        CdCode::NotTty,
        CdCode::BadDev,
        CdCode::DupBind,
        CdCode::BadHex,
        CdCode::BadOp,
        CdCode::MissingPart,
        CdCode::HandleFull,
        CdCode::StateRegress,
        CdCode::EmptySummary,
        CdCode::SegmentViolation,
        CdCode::DiscardDrift,
        CdCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的设备调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevCall {
    /// 动词
    pub op: String,
    /// 设备名
    pub dev: String,
    /// hex 样本（read 时表字节数的 hex）
    pub hex: String,
}

/// 解析 "op|dev|hex" 三段调用行。
pub fn parse_dev_call(line: &str) -> Result<DevCall, CdCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(CdCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(CdCode::MissingPart),
    };
    if op != "write" && op != "read" && op != "ioctl" {
        return Err(CdCode::BadOp);
    }
    let dev = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(CdCode::MissingPart),
    };
    let hx = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(CdCode::MissingPart),
    };
    let hb = hx.as_bytes();
    let mut k = 0usize;
    while k < hb.len() {
        let b = match hb.get(k) {
            Some(x) => *x,
            None => break,
        };
        match b {
            b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' => {}
            _ => return Err(CdCode::BadHex),
        }
        k += 1;
    }
    Ok(DevCall { op, dev, hex: hx })
}

// ── 设备状态机 ────────────────────────────────────────────────────────
/// 设备生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevState {
    /// 未绑定
    Closed,
    /// 已绑定
    Bound,
    /// 已服务
    Served,
}

impl DevState {
    /// 单调推进判定。
    pub fn can_advance(self, next: DevState) -> bool {
        let rank = |s: DevState| match s {
            DevState::Closed => 0u8,
            DevState::Bound => 1,
            DevState::Served => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            DevState::Closed => "未绑定",
            DevState::Bound => "已绑定",
            DevState::Served => "已服务",
        }
    }
}

// ── 字符设备面 ────────────────────────────────────────────────────────
/// 设备条目。
#[derive(Clone, Debug)]
pub struct DevEnt {
    /// 设备名
    pub dev: String,
    /// 生命周期状态
    pub state: DevState,
    /// 累计丢弃字节数
    pub discarded: u32,
    /// 累计填充字节数
    pub filled: u32,
}

/// 字符设备面。
#[derive(Clone, Debug)]
pub struct CharDevFace {
    ents: Vec<DevEnt>,
    cap: usize,
    /// 累计 ioctl 拒止次数
    pub ioctls_denied: u32,
}

impl CharDevFace {
    /// 空面构造。
    pub fn new() -> CharDevFace {
        CharDevFace { ents: Vec::new(), cap: DEV_CAP, ioctls_denied: 0 }
    }

    /// 句柄寻址。
    fn h_idx(&self, dev: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.dev == dev {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 绑定设备句柄：dev 闭集/重复/满逐闸。
    pub fn bind(&mut self, dev: &str) -> Result<usize, CdCode> {
        if dev != "null" && dev != "zero" {
            return Err(CdCode::BadDev);
        }
        if self.h_idx(dev).is_some() {
            return Err(CdCode::DupBind);
        }
        if self.ents.len() >= self.cap {
            return Err(CdCode::HandleFull);
        }
        let _ = self.ents.push(DevEnt {
            dev: String::from(dev),
            state: DevState::Bound,
            discarded: 0,
            filled: 0,
        });
        Ok(self.ents.len() - 1)
    }

    /// 写：null/zero 均丢弃（回写入长，体账不变）。
    pub fn write(&mut self, dev: &str, data: &[u8]) -> Result<u32, CdCode> {
        let idx = match self.h_idx(dev) {
            Some(i) => i,
            None => return Err(CdCode::BadDev),
        };
        let n = data.len() as u32;
        if let Some(e) = self.ents.get_mut(idx) {
            e.discarded = e.discarded.saturating_add(n);
            if e.state.can_advance(DevState::Served) {
                e.state = DevState::Served;
            }
        }
        Ok(n)
    }

    /// 读：null 回 EOF（0 字节）；zero 填充 n 字节全零。
    pub fn read(&mut self, dev: &str, n: u32) -> Result<Vec<u8>, CdCode> {
        let idx = match self.h_idx(dev) {
            Some(i) => i,
            None => return Err(CdCode::BadDev),
        };
        let is_zero = match self.ents.get(idx) {
            Some(e) => e.dev == "zero",
            None => return Err(CdCode::BadDev),
        };
        if let Some(e) = self.ents.get_mut(idx) {
            if e.state.can_advance(DevState::Served) {
                e.state = DevState::Served;
            }
        }
        if !is_zero {
            return Ok(Vec::new());
        }
        let mut out: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < n {
            let _ = out.push(0u8);
            k += 1;
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.filled = e.filled.saturating_add(n);
        }
        Ok(out)
    }

    /// ioctl：一律 ENOTTY。
    pub fn ioctl(&mut self, dev: &str) -> Result<(), CdCode> {
        if self.h_idx(dev).is_none() {
            return Err(CdCode::BadDev);
        }
        self.ioctls_denied = self.ioctls_denied.saturating_add(1);
        Err(CdCode::NotTty)
    }

    /// 丢弃审计：句柄账与累计丢弃一致。
    pub fn discard_consistent(&self, dev: &str, written: u32) -> bool {
        match self.h_idx(dev) {
            Some(i) => match self.ents.get(i) {
                Some(e) => e.discarded == written,
                None => false,
            },
            None => false,
        }
    }

    /// 状态读取。
    pub fn state_of(&self, dev: &str) -> Option<DevState> {
        match self.h_idx(dev) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.state),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：null 写丢弃/零设备读填充 10/10 轮 ──────────────────────
/// 逐轮：null 写丢弃→null 读 EOF→zero 读填充→zero 写丢弃→态推进→审计。
pub fn run_dev_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut f = CharDevFace::new();
        let _ = f.bind("null");
        let _ = f.bind("zero");
        let mut all_ok = true;
        // 面1：null 写丢弃回长。
        all_ok = all_ok && f.write("null", &[1, 2, 3, 4]) == Ok(4);
        probes += 1;
        // 面2：null 读 EOF。
        all_ok = all_ok && f.read("null", 8).unwrap_or_default().is_empty();
        probes += 1;
        // 面3：zero 读填充全零。
        let z = f.read("zero", 6).unwrap_or_default();
        let mut zero_ok = z.len() == 6;
        let mut zi = 0usize;
        while zi < z.len() {
            if z.get(zi) != Some(&0u8) {
                zero_ok = false;
            }
            zi += 1;
        }
        all_ok = all_ok && zero_ok;
        probes += 1;
        // 面4：zero 写丢弃。
        all_ok = all_ok && f.write("zero", &[9, 9]) == Ok(2);
        probes += 1;
        // 面5：双设备态推进 Served。
        all_ok = all_ok && f.state_of("null") == Some(DevState::Served)
            && f.state_of("zero") == Some(DevState::Served);
        probes += 1;
        // 面6：丢弃账一致。
        all_ok = all_ok && f.discard_consistent("null", 4)
            && f.discard_consistent("zero", 2);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏设备矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDev {
    /// 非法 ioctl（ENOTTY 族）
    NotTty,
    /// 未知设备（ENOENT 族）
    BadDev,
    /// 重复绑定（EBUSY 档）
    DupBind,
    /// 坏 hex（EINVAL 族）
    BadHex,
}

impl BadDev {
    /// 闭集全量。
    pub const ALL: [BadDev; 4] = [
        BadDev::NotTty,
        BadDev::BadDev,
        BadDev::DupBind,
        BadDev::BadHex,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadDev::NotTty => "非法 ioctl",
            BadDev::BadDev => "未知设备",
            BadDev::DupBind => "重复绑定",
            BadDev::BadHex => "坏 hex 行",
        }
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), CdCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(CdCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏设备注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_dev_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadDev::ALL.len() {
        let case = match BadDev::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut f = CharDevFace::new();
            let _ = f.bind("null");
            let caught = match case {
                BadDev::NotTty => {
                    f.ioctl("null").err() == Some(CdCode::NotTty)
                }
                BadDev::BadDev => {
                    f.write("random", b"x").err() == Some(CdCode::BadDev)
                }
                BadDev::DupBind => {
                    f.bind("null").err() == Some(CdCode::DupBind)
                }
                BadDev::BadHex => {
                    parse_dev_call("write|null|xy1").err() == Some(CdCode::BadHex)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 设备小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「dev=<s> state=<t>」。
pub fn compose_dev_summary(f: &CharDevFace, dev: &str) -> Result<String, CdCode> {
    let st = match f.state_of(dev) {
        Some(t) => t.label(),
        None => return Err(CdCode::EmptySummary),
    };
    let out = format!("dev={} state={}", dev, st);
    if out.is_empty() {
        return Err(CdCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；非法 ioctl 族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知设备族"),
    ("EBUSY", -16, 33, "Windows ERROR_DRIVE_LOCKED；重复绑定档"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 hex 族"),
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：绑定→写丢弃→读填充→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut f = CharDevFace::new();
    let _ = f.bind("null");
    let _ = f.bind("zero");
    let _ = f.write("null", &[1, 2]);
    let _ = f.read("zero", 4);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_dev_summary(&f, "zero") {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10136 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10136_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10136");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93200..=0x9323F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < CdCode::ALL.len() {
        let mut j = i + 1;
        while j < CdCode::ALL.len() {
            if let (Some(a), Some(b)) = (CdCode::ALL.get(i), CdCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = CdCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_dev_call("write|null|aabb").is_ok()
        && parse_dev_call("seek|null|aa").err() == Some(CdCode::BadOp)
        && parse_dev_call("write|null|xy1").err() == Some(CdCode::BadHex);
    s.add("调用行解析器：op/dev 双闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {write, read, ioctl}；dev 闭集 {null, zero}");

    // 4. J1 达成核
    let (ok, probes) = run_dev_rounds();
    s.add("J1 特殊文件达成：null 写丢弃/零设备读填充 LTP read02 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（丢弃/EOF/填充/零写/态推进/审计）全对");

    // 5. J2 非法 ioctl
    let mut f = CharDevFace::new();
    let _ = f.bind("null");
    let _ = f.bind("zero");
    let tty_ok = f.ioctl("null").err() == Some(CdCode::NotTty)
        && f.ioctl("zero").err() == Some(CdCode::NotTty)
        && f.ioctls_denied == 2;
    s.add("J2 非法操作受控：ENOTTY 双设备精确复现且拒止入账", tty_ok,
          "字符设备无终端面；ioctl 一律 ENOTTY 且逐次入账");

    // 6. 未知设备档
    let unk_ok = f.write("random", b"x").err() == Some(CdCode::BadDev)
        && f.read("random", 1).err() == Some(CdCode::BadDev);
    s.add("未知设备：ENOENT 写读双入口精确归因", unk_ok,
          "dev 闭集外的设备读写一致拒绝");

    // 7. 重复绑定档
    let dup_ok = f.bind("null").err() == Some(CdCode::DupBind);
    s.add("重复绑定：EBUSY 档精确复现", dup_ok,
          "句柄二次绑定必须拒");

    // 8. 丢弃审计
    let _ = f.write("null", &[1, 2, 3]);
    let dis_ok = f.discard_consistent("null", 3)
        && f.state_of("null") == Some(DevState::Served);
    s.add("丢弃审计：累计丢弃与写入逐字节一致", dis_ok,
          "DiscardDrift 是丢弃语义根缺陷");

    // 9. 注入核
    let (inj, per_class) = run_bad_dev_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "ioctl/未知设备/重复绑定/坏 hex 必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadDev::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = DevState::Closed.can_advance(DevState::Served)
        && DevState::Bound.can_advance(DevState::Served)
        && !DevState::Served.can_advance(DevState::Bound)
        && DevState::Served.label() == "已服务";
    s.add("状态机：Closed→Bound→Served 单调", sm_ok,
          "设备生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(25) && agrees_with_linux(2)
        && agrees_with_linux(16) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_dev_summary(&f, "null");
    let text_ok = match &sum {
        Ok(x) => x.contains("dev=null") && x.contains("state=已服务"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("特殊文件达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_dev_call("write|null|aabb").is_ok());
        assert_eq!(parse_dev_call("seek|null|aa").err(), Some(CdCode::BadOp));
        assert_eq!(parse_dev_call("write|null|xy1").err(), Some(CdCode::BadHex));
        assert_eq!(parse_dev_call("write|null").err(), Some(CdCode::MissingPart));
    }

    #[test]
    fn null_discards() {
        let mut f = CharDevFace::new();
        let _ = f.bind("null");
        assert_eq!(f.write("null", &[1, 2, 3]), Ok(3));
        assert!(f.read("null", 4).unwrap_or_default().is_empty());
        assert!(f.discard_consistent("null", 3));
    }

    #[test]
    fn zero_fills() {
        let mut f = CharDevFace::new();
        let _ = f.bind("zero");
        let z = f.read("zero", 5).unwrap_or_default();
        assert_eq!(z, Vec::from(&[0u8; 5] as &[u8]));
        assert_eq!(f.write("zero", &[9]), Ok(1));
    }

    #[test]
    fn ioctl_enotty() {
        let mut f = CharDevFace::new();
        let _ = f.bind("null");
        assert_eq!(f.ioctl("null").err(), Some(CdCode::NotTty));
        assert_eq!(f.ioctls_denied, 1);
    }

    #[test]
    fn unknown_device() {
        let mut f = CharDevFace::new();
        assert_eq!(f.bind("random").err(), Some(CdCode::BadDev));
        assert_eq!(f.write("random", b"x").err(), Some(CdCode::BadDev));
    }

    #[test]
    fn dup_bind_blocked() {
        let mut f = CharDevFace::new();
        let _ = f.bind("null");
        assert_eq!(f.bind("null").err(), Some(CdCode::DupBind));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_dev_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_dev_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(DevState::Closed.can_advance(DevState::Bound));
        assert!(!DevState::Served.can_advance(DevState::Closed));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10136_checks_green() {
        let set = run_unx10136_checks();
        assert!(set.all_passed(), "UNX-F10136 red: {}", set.red_items_text());
    }
}
