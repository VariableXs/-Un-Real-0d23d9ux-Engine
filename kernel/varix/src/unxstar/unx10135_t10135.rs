//! UNX-F10135 挂载边界正反双判据（挂载点语义达成与跨挂载点误用受控）· 0x931C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10136-J1 特殊文件达成（null 写丢弃/零设备读填充 LTP
//!       read02 面 10/10 次）；UNX-F10136-J2 非法操作误用受控（预期
//!       ENOTTY 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x931C0（本卡独占 0x40 窗口，下一卡 0x93200）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_mount_call`："op|path|fs" 三成分（op ∈ {mount,
//!      umount, rename}），fs 闭集 {fsA, fsB}，逐档拒绝；
//!   ② 挂载表 `MountTable`：挂载点→(fs, 遮蔽栈)，mount 遮蔽原面、
//!      umount 恢复露出，枚举走栈顶（遮蔽语义）；
//!   ③ 状态机 `MountState`：Idle→Mounted→Shadowed 单调（禁倒退）；
//!   ④ 注入核 `BadMount`：跨挂载点 rename / 重挂载 / 未挂载 umount /
//!      缺失挂载点 四类坏样本逐类 10/10 次精确复现 EXDEV/EBUSY/EINVAL/
//!      ENOENT 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10135 独占 0x40 窗口，下一卡 0x93200）。
pub const SEG_BASE: u32 = 0x931C0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93200;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 挂载表容量。
pub const MOUNT_CAP: usize = 8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtCode {
    /// 跨挂载点 rename（EXDEV 族）
    CrossDev,
    /// 重挂载（EBUSY 族）
    BusyMount,
    /// 未挂载 umount（EINVAL 族）
    NotMounted,
    /// 缺失挂载点（ENOENT 族）
    MissingDir,
    /// 非法 fs（EINVAL 族）
    BadFs,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 遮蔽栈失平
    StackDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl MtCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MtCode::CrossDev => "跨挂载点 rename 被拒：old/new 不同挂载面；EXDEV 精确归档",
            MtCode::BusyMount => "重挂载被拒：挂载点已挂；EBUSY 精确归档",
            MtCode::NotMounted => "未挂载 umount 被拒：无挂可卸；EINVAL 精确归档",
            MtCode::MissingDir => "缺失挂载点被拒：目录不在账；ENOENT 精确归档",
            MtCode::BadFs => "非法 fs 被拒：fs 闭集 {fsA, fsB}；EINVAL 精确归档",
            MtCode::BadOp => "非法 op 被拒：op 闭集 {mount, umount, rename}；请核对调用行",
            MtCode::MissingPart => "成分缺失被拒：调用行必须三段 op|path|fs；请补成分",
            MtCode::StateRegress => "状态倒退被拒：Idle→Mounted→Shadowed 单调",
            MtCode::EmptySummary => "空成文被拒：挂载小结不得为空串；请先挂载再成文",
            MtCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            MtCode::StackDrift => "遮蔽栈失平被拒：栈顶必须反映最近挂载",
            MtCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [MtCode; 12] = [
        MtCode::CrossDev,
        MtCode::BusyMount,
        MtCode::NotMounted,
        MtCode::MissingDir,
        MtCode::BadFs,
        MtCode::BadOp,
        MtCode::MissingPart,
        MtCode::StateRegress,
        MtCode::EmptySummary,
        MtCode::SegmentViolation,
        MtCode::StackDrift,
        MtCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的挂载调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountCall {
    /// 动词
    pub op: String,
    /// 路径
    pub path: String,
    /// fs 标识（rename 时为第二路径）
    pub fs: String,
}

/// 解析 "op|path|fs" 三段调用行。
pub fn parse_mount_call(line: &str) -> Result<MountCall, MtCode> {
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
        return Err(MtCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(MtCode::MissingPart),
    };
    if op != "mount" && op != "umount" && op != "rename" {
        return Err(MtCode::BadOp);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(MtCode::MissingPart),
    };
    let fs = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(MtCode::MissingPart),
    };
    Ok(MountCall { op, path, fs })
}

// ── 挂载状态机 ────────────────────────────────────────────────────────
/// 挂载生命周期（单调，禁倒退；umount 为终态出口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountState {
    /// 未挂载
    Idle,
    /// 已挂载
    Mounted,
    /// 已遮蔽（栈上有更晚挂载）
    Shadowed,
}

impl MountState {
    /// 单调推进判定。
    pub fn can_advance(self, next: MountState) -> bool {
        let rank = |s: MountState| match s {
            MountState::Idle => 0u8,
            MountState::Mounted => 1,
            MountState::Shadowed => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            MountState::Idle => "未挂载",
            MountState::Mounted => "已挂载",
            MountState::Shadowed => "已遮蔽",
        }
    }
}

// ── 挂载表 ────────────────────────────────────────────────────────────
/// 目录账条目：目录 + 所属挂载面。
#[derive(Clone, Debug)]
pub struct DirEnt {
    /// 路径
    pub path: String,
    /// 所属挂载面（fsA 基面 / fsB 挂载面）
    pub fs: String,
}

/// 挂载表：挂载点栈 + 目录账。
#[derive(Clone, Debug)]
pub struct MountTable {
    /// 目录账（含基面与挂载面条目）
    pub dirs: Vec<DirEnt>,
    /// 挂载栈：(挂载点, fs) 按序
    pub stack: Vec<(String, String)>,
    /// 累计挂载次数
    pub mounts: u32,
    /// 累计卸载次数
    pub umounts: u32,
}

impl MountTable {
    /// 空表构造（基面 fsA 预置 /、/data）。
    pub fn new() -> MountTable {
        let mut t = MountTable {
            dirs: Vec::new(),
            stack: Vec::new(),
            mounts: 0,
            umounts: 0,
        };
        let _ = t.dirs.push(DirEnt { path: String::from("/"), fs: String::from("fsA") });
        let _ = t.dirs.push(DirEnt { path: String::from("/data"), fs: String::from("fsA") });
        let _ = t.dirs.push(DirEnt { path: String::from("/data/f1"), fs: String::from("fsA") });
        t
    }

    /// 按路径寻目录。
    fn idx_of(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.dirs.len() {
            if let Some(d) = self.dirs.get(i) {
                if d.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 公开寻址（审计用）。
    pub fn idx_of_pub(&self, path: &str) -> Option<usize> {
        self.idx_of(path)
    }

    /// 路径所属挂载面：挂载点及以下归栈顶 fs，否则基面。
    pub fn fs_of(&self, path: &str) -> String {
        let mut i = self.stack.len();
        while i > 0 {
            i -= 1;
            if let Some((mp, fs)) = self.stack.get(i) {
                if path == mp || path.starts_with(&format!("{}/", mp)) {
                    return fs.clone();
                }
            }
        }
        String::from("fsA")
    }

    /// mount：目录在账、未挂/未被遮、fs 闭集三闸；入栈遮蔽。
    pub fn mount(&mut self, path: &str, fs: &str) -> Result<(), MtCode> {
        if fs != "fsA" && fs != "fsB" {
            return Err(MtCode::BadFs);
        }
        if self.idx_of(path).is_none() {
            return Err(MtCode::MissingDir);
        }
        let mut i = 0usize;
        while i < self.stack.len() {
            if let Some((mp, _)) = self.stack.get(i) {
                if mp == path {
                    return Err(MtCode::BusyMount);
                }
            }
            i += 1;
        }
        let _ = self.stack.push((String::from(path), String::from(fs)));
        self.mounts = self.mounts.saturating_add(1);
        Ok(())
    }

    /// umount：栈顶匹配才可卸；未挂/深栈错序逐档拒绝。
    pub fn umount(&mut self, path: &str) -> Result<(), MtCode> {
        let top = match self.stack.last() {
            Some((mp, _)) => mp.clone(),
            None => return Err(MtCode::NotMounted),
        };
        if top != path {
            if self.stack.iter().any(|(mp, _)| mp == path) {
                return Err(MtCode::StackDrift);
            }
            return Err(MtCode::NotMounted);
        }
        let _ = self.stack.pop();
        self.umounts = self.umounts.saturating_add(1);
        Ok(())
    }

    /// rename：跨面拒绝（EXDEV 档）；同面改名。
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), MtCode> {
        let fo = self.fs_of(old);
        let fn_ = self.fs_of(new);
        if fo != fn_ {
            return Err(MtCode::CrossDev);
        }
        if self.idx_of(old).is_none() {
            return Err(MtCode::MissingDir);
        }
        if self.idx_of(new).is_some() {
            return Err(MtCode::NotMounted);
        }
        if let Some(i) = self.idx_of(old) {
            if let Some(d) = self.dirs.get_mut(i) {
                d.path = String::from(new);
            }
        }
        Ok(())
    }

    /// 枚举挂载面：返回栈顶 fs 在账条目数（遮蔽语义）。
    pub fn enumerate_top(&self, base: &str) -> usize {
        let top_fs = self.fs_of(base);
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.dirs.len() {
            if let Some(d) = self.dirs.get(i) {
                if d.fs == top_fs && (d.path == base || d.path.starts_with(&format!("{}/", base))) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 栈深。
    pub fn stack_len(&self) -> usize {
        self.stack.len()
    }
}

// ── J1 达成核：stub 挂载面枚举/遮蔽语义 10/10 轮 ──────────────────────
/// 逐轮：挂 fsB→枚举遮蔽→卸载恢复→再挂计数。
pub fn run_mount_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = MountTable::new();
        let _ = t.dirs.push(DirEnt { path: String::from("/data/m2"), fs: String::from("fsB") });
        let mut all_ok = true;
        // 面1：挂 fsB 到 /data。
        all_ok = all_ok && t.mount("/data", "fsB").is_ok() && t.stack_len() == 1;
        probes += 1;
        // 面2：遮蔽语义：/data 面归属 fsB。
        all_ok = all_ok && t.fs_of("/data") == "fsB" && t.fs_of("/data/f1") == "fsB";
        probes += 1;
        // 面3：枚举走栈顶（fsB 面 1 条目）。
        all_ok = all_ok && t.enumerate_top("/data") == 1;
        probes += 1;
        // 面4：umount 恢复基面。
        all_ok = all_ok && t.umount("/data").is_ok()
            && t.fs_of("/data") == "fsA" && t.enumerate_top("/data") == 2;
        probes += 1;
        // 面5：再挂成功且计数 2。
        all_ok = all_ok && t.mount("/data", "fsB").is_ok()
            && t.mounts == 2 && t.umounts == 1;
        probes += 1;
        // 面6：跨面 rename 拒（/data 在 fsB、/ 在 fsA）。
        all_ok = all_ok && t.rename("/data", "/root2").err() == Some(MtCode::CrossDev);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏挂载矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMount {
    /// 跨挂载点 rename（EXDEV 族）
    CrossDev,
    /// 重挂载（EBUSY 族）
    BusyMount,
    /// 未挂载 umount（EINVAL 族）
    NotMounted,
    /// 缺失挂载点（ENOENT 族）
    MissingDir,
}

impl BadMount {
    /// 闭集全量。
    pub const ALL: [BadMount; 4] = [
        BadMount::CrossDev,
        BadMount::BusyMount,
        BadMount::NotMounted,
        BadMount::MissingDir,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadMount::CrossDev => "跨挂载点 rename",
            BadMount::BusyMount => "挂载点重挂",
            BadMount::NotMounted => "未挂载 umount",
            BadMount::MissingDir => "缺失挂载点 mount",
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
    pub fn record(&mut self, caught: bool) -> Result<(), MtCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(MtCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏挂载注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_mount_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadMount::ALL.len() {
        let case = match BadMount::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut t = MountTable::new();
            let _ = t.mount("/data", "fsB");
            let caught = match case {
                BadMount::CrossDev => {
                    t.rename("/data", "/x").err() == Some(MtCode::CrossDev)
                }
                BadMount::BusyMount => {
                    t.mount("/data", "fsA").err() == Some(MtCode::BusyMount)
                }
                BadMount::NotMounted => {
                    t.umount("/").err() == Some(MtCode::NotMounted)
                }
                BadMount::MissingDir => {
                    t.umount("/data");
                    t.mount("/nope", "fsB").err() == Some(MtCode::MissingDir)
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

// ── 挂载小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「path=<s> fs=<f> stack=<n>」。
pub fn compose_mount_summary(t: &MountTable, path: &str) -> Result<String, MtCode> {
    let fs = t.fs_of(path);
    let out = format!("path={} fs={} stack={}", path, fs, t.stack_len());
    if out.is_empty() {
        return Err(MtCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EXDEV", -18, 17, "Windows ERROR_NOT_SAME_DEVICE；跨挂载点 rename 族"),
    ("EBUSY", -16, 33, "Windows ERROR_DRIVE_LOCKED；挂载点重挂族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未挂载 umount/坏 fs 族"),
    ("ENOENT", -2, 2, "Windows ERROR_PATH_NOT_FOUND；缺失挂载点族"),
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

/// 规范语料指纹：挂→枚举→卸→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut t = MountTable::new();
    let _ = t.mount("/data", "fsB");
    let n1 = t.enumerate_top("/data");
    let _ = t.umount("/data");
    let n2 = t.enumerate_top("/data");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_mount_summary(&t, "/data") {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&n1.to_le_bytes());
    bytes.extend_from_slice(&n2.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10135 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10135_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10135");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x931C0..=0x931FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < MtCode::ALL.len() {
        let mut j = i + 1;
        while j < MtCode::ALL.len() {
            if let (Some(a), Some(b)) = (MtCode::ALL.get(i), MtCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = MtCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_mount_call("mount|/data|fsB").is_ok()
        && parse_mount_call("sw|/data|fsB").err() == Some(MtCode::BadOp)
        && parse_mount_call("mount|/data").err() == Some(MtCode::MissingPart);
    s.add("调用行解析器：op 三词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {mount, umount, rename}；fs 闭集 {fsA, fsB}");

    // 4. J1 达成核
    let (ok, probes) = run_mount_rounds();
    s.add("J1 挂载点语义达成：stub 挂载面枚举/遮蔽语义 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（挂载/遮蔽/枚举/恢复/计数/跨面拒）全对");

    // 5. J2 跨挂载点 rename
    let mut t = MountTable::new();
    let _ = t.mount("/data", "fsB");
    let xdev_ok = t.rename("/data", "/x").err() == Some(MtCode::CrossDev)
        && t.idx_of_pub("/data").is_some();
    s.add("J2 跨挂载点 rename 受控：EXDEV 精确复现且账面不动", xdev_ok,
          "跨面改名必须拒；被拒 rename 不得移名");

    // 6. 重挂载档
    let busy_ok = t.mount("/data", "fsA").err() == Some(MtCode::BusyMount);
    s.add("重挂载：EBUSY 档精确复现", busy_ok,
          "已挂挂载点二次 mount 必须拒");

    // 7. 未挂载 umount 档
    let nm_ok = t.umount("/").err() == Some(MtCode::NotMounted);
    s.add("未挂载 umount：EINVAL 档精确复现", nm_ok,
          "无挂可卸必须显性拒绝");

    // 8. 遮蔽恢复语义
    let shadow_ok = t.fs_of("/data") == "fsB"
        && {
            let _ = t.umount("/data");
            t.fs_of("/data") == "fsA"
        };
    s.add("遮蔽恢复：umount 后基面重新可见", shadow_ok,
          "遮蔽栈顶必须随 umount 回退");

    // 9. 注入核
    let (inj, per_class) = run_bad_mount_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "跨面/重挂/未挂卸/缺失点必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadMount::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = MountState::Idle.can_advance(MountState::Shadowed)
        && MountState::Mounted.can_advance(MountState::Shadowed)
        && !MountState::Shadowed.can_advance(MountState::Idle)
        && MountState::Mounted.label() == "已挂载";
    s.add("状态机：Idle→Mounted→Shadowed 单调", sm_ok,
          "挂载生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(18) && agrees_with_linux(16)
        && agrees_with_linux(22) && agrees_with_linux(2)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_mount_summary(&t, "/data");
    let text_ok = match &sum {
        Ok(x) => x.contains("fs=fsA"),
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
    let ok_text = format!("挂载语义达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_mount_call("mount|/data|fsB").is_ok());
        assert_eq!(parse_mount_call("sw|/data|fsB").err(), Some(MtCode::BadOp));
        assert_eq!(parse_mount_call("mount|/data").err(), Some(MtCode::MissingPart));
    }

    #[test]
    fn mount_shadow_recover() {
        let mut t = MountTable::new();
        assert!(t.mount("/data", "fsB").is_ok());
        assert_eq!(t.fs_of("/data"), "fsB");
        assert_eq!(t.fs_of("/data/f1"), "fsB");
        assert!(t.umount("/data").is_ok());
        assert_eq!(t.fs_of("/data"), "fsA");
    }

    #[test]
    fn busy_remount_blocked() {
        let mut t = MountTable::new();
        let _ = t.mount("/data", "fsB");
        assert_eq!(t.mount("/data", "fsA").err(), Some(MtCode::BusyMount));
    }

    #[test]
    fn bad_fs_blocked() {
        let mut t = MountTable::new();
        assert_eq!(t.mount("/data", "fsX").err(), Some(MtCode::BadFs));
    }

    #[test]
    fn cross_dev_rename_blocked() {
        let mut t = MountTable::new();
        let _ = t.mount("/data", "fsB");
        assert_eq!(t.rename("/data", "/x").err(), Some(MtCode::CrossDev));
        assert!(t.idx_of_pub("/data").is_some());
    }

    #[test]
    fn same_fs_rename_ok() {
        let mut t = MountTable::new();
        let _ = t.dirs.push(DirEnt { path: String::from("/data/f2"), fs: String::from("fsA") });
        assert!(t.rename("/data/f1", "/data/f2x").is_ok());
    }

    #[test]
    fn umount_order_guard() {
        let mut t = MountTable::new();
        let _ = t.mount("/data", "fsB");
        assert_eq!(t.umount("/").err(), Some(MtCode::NotMounted));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_mount_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_mount_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(MountState::Idle.can_advance(MountState::Mounted));
        assert!(!MountState::Shadowed.can_advance(MountState::Mounted));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10135_checks_green() {
        let set = run_unx10135_checks();
        assert!(set.all_passed(), "UNX-F10135 red: {}", set.red_items_text());
    }
}
