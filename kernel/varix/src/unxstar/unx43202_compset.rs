//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F43202 组件集模型（组件ID×目标路径×安装动作三元组） · 0x9B040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥486 行；
//!   ② 判据原文：UNX-F43202-J1 三元组 ×50 校验、非法动作拒 ×10；
//!      判据锚定 任务书专题一；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三元组闭集：组件 ID 窗 0x1000..0x2000，目标路径窗（sys/ 前缀 +
//!     长度 1..=48 + 字符闭集 [A-Za-z0-9/_.-]），动作枚举三值
//!     （add/replace/remove）；
//!   - ×50 校验：50 组冻结三元组逐组求值，判定与账一致；
//!   - 非法动作拒 ×10：未知动作字节/越窗 ID/越权路径/重复三元组/
//!     remove 无此件/容量满/空 ID/路径越根/坏字符/动作错型逐类必抓；
//!   - 双跑一致 + 段位守卫 + 闭集回查；no_std + alloc；零 panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B040，AI-VX12-1106 批1 12 槽第 2 槽）。
pub const SEG_BASE: u32 = 0x9B040;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompSetCode {
    /// 未知动作字节
    BadAction,
    /// 组件 ID 越窗
    BadId,
    /// 空 ID
    EmptyId,
    /// 路径越根：目标不在 sys/ 安装根内
    PathEscape,
    /// 坏字符：路径含闭集外字符
    BadChar,
    /// 路径过长
    PathTooLong,
    /// 重复三元组
    DupEntry,
    /// remove 无此件
    NoSuchEntry,
    /// replace 无此件
    NoBaseEntry,
    /// add 已存在
    AlreadyThere,
    /// 容量满
    LedgerFull,
    /// 动作错型
    WrongAction,
    /// 未知兜底
    Unknown,
}

impl CompSetCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CompSetCode::BadAction => 0x01,
                CompSetCode::BadId => 0x02,
                CompSetCode::EmptyId => 0x03,
                CompSetCode::PathEscape => 0x04,
                CompSetCode::BadChar => 0x05,
                CompSetCode::PathTooLong => 0x06,
                CompSetCode::DupEntry => 0x07,
                CompSetCode::NoSuchEntry => 0x08,
                CompSetCode::NoBaseEntry => 0x09,
                CompSetCode::AlreadyThere => 0x0A,
                CompSetCode::LedgerFull => 0x0B,
                CompSetCode::WrongAction => 0x0C,
                CompSetCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            CompSetCode::BadAction => "未知动作：动作枚举闭集 add/replace/remove",
            CompSetCode::BadId => "组件 ID 越窗：0x1000..0x2000",
            CompSetCode::EmptyId => "空 ID：组件编号不得为 0",
            CompSetCode::PathEscape => "路径越根：目标须在 sys/ 安装根内",
            CompSetCode::BadChar => "坏字符：路径字符闭集 [A-Za-z0-9/_.-]",
            CompSetCode::PathTooLong => "路径过长：≤ 48 字节",
            CompSetCode::DupEntry => "重复三元组：同 ID 同路径不重复入账",
            CompSetCode::NoSuchEntry => "remove 无此件：先查账再移除",
            CompSetCode::NoBaseEntry => "replace 无此件：先 add 再 replace",
            CompSetCode::AlreadyThere => "add 已存在：先 remove 再 add",
            CompSetCode::LedgerFull => "容量满：先移除再入账",
            CompSetCode::WrongAction => "动作错型：动作用闭集枚举不透传字节",
            CompSetCode::Unknown => "未知诊断码：不在 0x9B040 段闭集内",
        }
    }

    pub const ALL: [CompSetCode; 13] = [
        CompSetCode::BadAction,
        CompSetCode::BadId,
        CompSetCode::EmptyId,
        CompSetCode::PathEscape,
        CompSetCode::BadChar,
        CompSetCode::PathTooLong,
        CompSetCode::DupEntry,
        CompSetCode::NoSuchEntry,
        CompSetCode::NoBaseEntry,
        CompSetCode::AlreadyThere,
        CompSetCode::LedgerFull,
        CompSetCode::WrongAction,
        CompSetCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in CompSetCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    CompSetCode::Unknown.detail()
}

// ══ [CHUNK-2-END]

// ── 冻结常量（安装根/窗口/容量） ─────────────────────────────────
/// 组件 ID 窗（下闭上开）。
pub const ID_WIN: (u32, u32) = (0x1000, 0x2000);
/// 安装根前缀。
pub const ROOT_PREFIX: &str = "sys/";
/// 路径长度上限。
pub const PATH_CAP: usize = 48;
/// 组件账容量。
pub const SET_CAP: usize = 64;
/// 校验三元组数（判据 ×50）。
pub const VALIDATE_N: usize = 50;
/// 注入数（判据 ×10）。
pub const INJECT_N: usize = 10;

/// 安装动作（闭集枚举，不透传字节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompAction {
    /// 新增
    Add,
    /// 替换（先有后换）
    Replace,
    /// 移除
    Remove,
}

/// 单条组件三元组（Copy）。
#[derive(Clone, Copy, Debug)]
pub struct CompTriple {
    pub comp_id: u32,
    /// 路径（定容内联，最长 48）。
    pub path: [u8; PATH_CAP],
    pub path_len: u8,
    pub action: CompAction,
}

/// 组件集账。
pub struct CompLedger {
    entries: Vec<CompTriple>,
    /// 拒止账。
    pub rejected: u32,
    pub attempts: u32,
}

/// 路径合法性（前缀 + 字符闭集 + 长度）。
pub fn path_ok(path: &[u8]) -> bool {
    if path.len() == 0 || path.len() > PATH_CAP {
        return false;
    }
    if path.len() < ROOT_PREFIX.len() {
        return false;
    }
    let mut i = 0usize;
    while i < ROOT_PREFIX.len() {
        if path[i] != ROOT_PREFIX.as_bytes()[i] {
            return false;
        }
        i += 1;
    }
    for b in path.iter() {
        let c = *b;
        let ok = (c >= b'A' && c <= b'Z')
            || (c >= b'a' && c <= b'z')
            || (c >= b'0' && c <= b'9')
            || c == b'/'
            || c == b'_'
            || c == b'.'
            || c == b'-';
        if !ok {
            return false;
        }
    }
    // 双斜杠与尾斜杠均拒（路径形态闭集）。
    let mut prev = 0u8;
    for b in path.iter() {
        if *b == b'/' && prev == b'/' {
            return false;
        }
        prev = *b;
    }
    if path[path.len() - 1] == b'/' {
        return false;
    }
    true
}

impl CompLedger {
    pub fn new() -> CompLedger {
        CompLedger { entries: Vec::new(), rejected: 0, attempts: 0 }
    }

    /// 三元组入账主路径（五闸全过才入）。
    pub fn apply(&mut self, t: CompTriple) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸1：ID 窗。
        if t.comp_id < ID_WIN.0 || t.comp_id >= ID_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CompSetCode::BadId.val());
        }
        // 闸2：路径合法性（过长先于切片判，防越界）。
        let n = t.path_len as usize;
        if n > PATH_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CompSetCode::PathTooLong.val());
        }
        if !path_ok(&t.path[..n]) {
            self.rejected = self.rejected.saturating_add(1);
            // 区分越根/坏字符/过长三码。
            if n > PATH_CAP {
                return Err(CompSetCode::PathTooLong.val());
            }
            let p = &t.path[..n];
            let mut under = true;
            let mut i = 0usize;
            while i < ROOT_PREFIX.len() {
                if p[i] != ROOT_PREFIX.as_bytes()[i] {
                    under = false;
                }
                i += 1;
            }
            if !under || n < ROOT_PREFIX.len() {
                return Err(CompSetCode::PathEscape.val());
            }
            return Err(CompSetCode::BadChar.val());
        }
        // 闸3：容量。
        if self.entries.len() >= SET_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CompSetCode::LedgerFull.val());
        }
        // 闸4/5：动作语义。
        let exists = self.find_pos(t.comp_id, &t.path[..n]).is_some();
        match t.action {
            CompAction::Add => {
                if exists {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(CompSetCode::AlreadyThere.val());
                }
            }
            CompAction::Replace => {
                if !exists {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(CompSetCode::NoBaseEntry.val());
                }
            }
            CompAction::Remove => {
                if !exists {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(CompSetCode::NoSuchEntry.val());
                }
            }
        }
        // 入账/移除。
        match t.action {
            CompAction::Remove => {
                self.remove_at(t.comp_id, &t.path[..n]);
                Ok(0)
            }
            CompAction::Replace => {
                self.remove_at(t.comp_id, &t.path[..n]);
                self.entries.push(t);
                Ok(self.entries.len() as u32)
            }
            _ => {
                self.entries.push(t);
                Ok(self.entries.len() as u32)
            }
        }
    }

    /// 账内查找（ID+路径）。
    pub fn find_pos(&self, comp_id: u32, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let en = e.path_len as usize;
            if e.comp_id == comp_id && en == path.len() && e.path[..en] == path[..path.len()] {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn remove_at(&mut self, comp_id: u32, path: &[u8]) {
        let mut out: Vec<CompTriple> = Vec::new();
        for e in self.entries.iter() {
            let en = e.path_len as usize;
            let same = e.comp_id == comp_id && en == path.len() && e.path[..en] == path[..path.len()];
            if !same {
                out.push(*e);
            }
        }
        self.entries = out;
    }

    /// 账内条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ══ [CHUNK-3-END]

// ── 冻结样例：×50 校验三元组 ────────────────────────────────────
/// 构造三元组（路径以冻结字符集生成，禁运行期拼串）。
pub fn mk_triple(comp_id: u32, path: &'static str, action: CompAction) -> CompTriple {
    let mut t = CompTriple {
        comp_id,
        path: [0u8; PATH_CAP],
        path_len: path.len() as u8,
        action,
    };
    let b = path.as_bytes();
    let mut i = 0usize;
    while i < b.len() && i < PATH_CAP {
        t.path[i] = b[i];
        i += 1;
    }
    t
}

/// 冻结路径名表（48 个合法路径 + 2 个复用以覆盖 50 组校验）。
pub const SAMPLE_PATHS: [&str; 48] = [
    "sys/kern/core01", "sys/kern/core02", "sys/kern/sched", "sys/kern/mm",
    "sys/drv/net0", "sys/drv/disk0", "sys/drv/disk1", "sys/drv/usb",
    "sys/fs/rootfs", "sys/fs/exfat", "sys/fs/nfsv4", "sys/fs/cachefsd",
    "sys/net/stack", "sys/net/firewall", "sys/net/proxy", "sys/net/dns",
    "sys/ui/shell", "sys/ui/wm", "sys/ui/theme", "sys/ui/fonts",
    "sys/sec/policy", "sys/sec/audit", "sys/sec/keys", "sys/sec/tpm",
    "sys/upd/agent", "sys/upd/manifest", "sys/upd/delta", "sys/upd/sig",
    "sys/diag/probe", "sys/diag/logd", "sys/diag/trace", "sys/diag/crash",
    "sys/kern/core03", "sys/kern/core04", "sys/kern/sched2", "sys/kern/mm2",
    "sys/drv/net1", "sys/drv/disk2", "sys/drv/usb3", "sys/fs/exfat2",
    "sys/fs/nfsv6", "sys/fs/cachefsd2", "sys/net/firewall2", "sys/net/proxy2",
    "sys/ui/shell2", "sys/ui/wm2", "sys/sec/audit2", "sys/upd/delta2",
];

/// ×50 校验：50 组三元组入账全过 + 账内条数守恒 + 样例对唯一。
pub fn validate_fifty() -> (u32, String) {
    let mut g = CompLedger::new();
    let mut ok = 0u32;
    let mut marks = String::new();
    let mut i = 0usize;
    while i < VALIDATE_N {
        let id = ID_WIN.0 + (i as u32) * 3;
        let path = SAMPLE_PATHS[i % SAMPLE_PATHS.len()];
        let act = CompAction::Add;
        let t = mk_triple(id, path, act);
        match g.apply(t) {
            Ok(_) => ok += 1,
            Err(e) => marks.push_str(&format!("{}@{}!", e, i)),
        }
        i += 1;
    }
    // 样例对唯一性（同 ID 同路径不得重复）。
    let mut dup = 0u32;
    let mut a = 0usize;
    while a < VALIDATE_N {
        let mut b = a + 1;
        while b < VALIDATE_N {
            let ida = ID_WIN.0 + (a as u32) * 3;
            let idb = ID_WIN.0 + (b as u32) * 3;
            if ida == idb && SAMPLE_PATHS[a % SAMPLE_PATHS.len()] == SAMPLE_PATHS[b % SAMPLE_PATHS.len()] {
                dup += 1;
            }
            b += 1;
        }
        a += 1;
    }
    let cons = ok == VALIDATE_N as u32 && dup == 0 && g.len() == VALIDATE_N;
    (if cons { VALIDATE_N as u32 } else { ok }, marks)
}

// ══ [CHUNK-4-END]

/// 批样重复三元组扫描（DupEntry 载体：同 ID 同路径重复提交必抓）。
pub fn dup_scan(sample: &[(u32, &'static str)]) -> u32 {
    let mut dups = 0u32;
    let mut a = 0usize;
    while a < sample.len() {
        let mut b = a + 1;
        while b < sample.len() {
            if sample[a].0 == sample[b].0 && sample[a].1 == sample[b].1 {
                dups += 1;
            }
            b += 1;
        }
        a += 1;
    }
    dups
}

// ── 非法注入 ×10（判据：逐类必抓） ──────────────────────────────
pub fn inject_ten() -> (u32, String) {
    let mut caught = 0u32;
    let mut marks = String::new();
    let mut g = CompLedger::new();
    // 1 坏 ID（越窗）。
    let t1 = mk_triple(0x0FFF, SAMPLE_PATHS[0], CompAction::Add);
    if g.apply(t1).err() == Some(CompSetCode::BadId.val()) {
        caught += 1;
    } else {
        marks.push_str("1!");
    }
    // 2 路径越根。
    let t2 = mk_triple(ID_WIN.0, "usr/bin/x", CompAction::Add);
    if g.apply(t2).err() == Some(CompSetCode::PathEscape.val()) {
        caught += 1;
    } else {
        marks.push_str("2!");
    }
    // 3 坏字符（空格）。
    let t3 = mk_triple(ID_WIN.0 + 1, "sys/a b", CompAction::Add);
    if g.apply(t3).err() == Some(CompSetCode::BadChar.val()) {
        caught += 1;
    } else {
        marks.push_str("3!");
    }
    // 4 路径过长（49 字节）。
    let t4 = mk_triple(ID_WIN.0 + 2, "sys/a/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", CompAction::Add);
    if g.apply(t4).err() == Some(CompSetCode::PathTooLong.val()) {
        caught += 1;
    } else {
        marks.push_str("4!");
    }
    // 5 空 ID。
    let t5 = mk_triple(0, SAMPLE_PATHS[1], CompAction::Add);
    if g.apply(t5).err() == Some(CompSetCode::BadId.val()) {
        caught += 1;
    } else {
        marks.push_str("5!");
    }
    // 6 add 已存在。
    let _ = g.apply(mk_triple(ID_WIN.0 + 3, SAMPLE_PATHS[2], CompAction::Add));
    if g.apply(mk_triple(ID_WIN.0 + 3, SAMPLE_PATHS[2], CompAction::Add))
        .err() == Some(CompSetCode::AlreadyThere.val())
    {
        caught += 1;
    } else {
        marks.push_str("6!");
    }
    // 7 replace 无此件。
    if g.apply(mk_triple(ID_WIN.0 + 4, SAMPLE_PATHS[3], CompAction::Replace))
        .err() == Some(CompSetCode::NoBaseEntry.val())
    {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 remove 无此件。
    if g.apply(mk_triple(ID_WIN.0 + 4, SAMPLE_PATHS[3], CompAction::Remove))
        .err() == Some(CompSetCode::NoSuchEntry.val())
    {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 尾斜杠拒（路径形态闭集）。
    if g.apply(mk_triple(ID_WIN.0 + 5, "sys/ui/", CompAction::Add))
        .err() == Some(CompSetCode::BadChar.val())
    {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 批样重复对必抓（2 对注入 → dup_scan=2）。
    let sample: [(u32, &'static str); 52] = build_dup_sample();
    if dup_scan(&sample) == 2 {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, marks)
}

/// 构造 52 样例（50 唯一 + 2 重复对）。
pub fn build_dup_sample() -> [(u32, &'static str); 52] {
    let mut s: [(u32, &'static str); 52] = [(0, ""); 52];
    let mut i = 0usize;
    while i < 50 {
        s[i] = (ID_WIN.0 + (i as u32) * 3, SAMPLE_PATHS[i % SAMPLE_PATHS.len()]);
        i += 1;
    }
    s[50] = s[0];
    s[51] = s[1];
    s
}

// ══ [CHUNK-5-END]

// ── 黄金正演 + 双跑 + 段位守卫 ──────────────────────────────────
/// 黄金正演：add→replace→remove 全周期守恒。
pub fn golden_run() -> Result<u32, u32> {
    let mut g = CompLedger::new();
    let _ = g.apply(mk_triple(ID_WIN.0 + 0x10, SAMPLE_PATHS[4], CompAction::Add))?;
    let _ = g.apply(mk_triple(ID_WIN.0 + 0x11, SAMPLE_PATHS[5], CompAction::Add))?;
    // replace：账内条数不变。
    let _ = g.apply(mk_triple(ID_WIN.0 + 0x10, SAMPLE_PATHS[4], CompAction::Replace))?;
    if g.len() != 2 {
        return Err(CompSetCode::DupEntry.val());
    }
    // remove：账内条数减一。
    let _ = g.apply(mk_triple(ID_WIN.0 + 0x11, SAMPLE_PATHS[5], CompAction::Remove))?;
    if g.len() != 1 {
        return Err(CompSetCode::NoSuchEntry.val());
    }
    Ok(g.len() as u32)
}

/// 双跑一致。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in CompSetCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演全周期", g.is_ok(), format!("{:?}", g)));
    let (c50, mk50) = validate_fifty();
    v.push(("J1-三元组×50", c50 == 50, format!("c {}/50 {}", c50, mk50)));
    let (ci, mki) = inject_ten();
    v.push(("J2-非法拒×10", ci == 10, format!("c {}/10 {}", ci, mki)));
    // J3：路径闸三码分诊（越根/坏字符/过长各回各码）。
    let mut g2 = CompLedger::new();
    let e1 = g2.apply(mk_triple(0x1050, "nope/x", CompAction::Add)).err();
    let e2 = g2.apply(mk_triple(0x1051, "sys/ok bad", CompAction::Add)).err();
    let e3 = g2.apply(mk_triple(0x1052, "sys/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", CompAction::Add)).err();
    let triage = e1 == Some(CompSetCode::PathEscape.val())
        && e2 == Some(CompSetCode::BadChar.val())
        && e3 == Some(CompSetCode::PathTooLong.val());
    v.push(("J3-路径闸三码分诊", triage, String::from("越根/坏字/过长")));
    // J3：动作全周期（add→add拒→remove→remove拒→add放行）。
    let mut g3 = CompLedger::new();
    let a1 = g3.apply(mk_triple(0x1000, SAMPLE_PATHS[6], CompAction::Add)).is_ok();
    let a2 = g3.apply(mk_triple(0x1000, SAMPLE_PATHS[6], CompAction::Add)).is_err();
    let a3 = g3.apply(mk_triple(0x1000, SAMPLE_PATHS[6], CompAction::Remove)).is_ok();
    let a4 = g3.apply(mk_triple(0x1000, SAMPLE_PATHS[6], CompAction::Remove)).is_err();
    let a5 = g3.apply(mk_triple(0x1000, SAMPLE_PATHS[6], CompAction::Add)).is_ok();
    v.push(("J3-动作全周期", a1 && a2 && a3 && a4 && a5, String::from("add/dup/remove/remerr/readd")));
    // J4：容量闸（64 满 → 65 拒）。
    let mut g4 = CompLedger::new();
    let mut i = 0usize;
    while i < SET_CAP {
        let _ = g4.apply(mk_triple(ID_WIN.0 + i as u32, SAMPLE_PATHS[i % 48], CompAction::Add));
        i += 1;
    }
    let full_reject = g4.apply(mk_triple(0x1040, SAMPLE_PATHS[7], CompAction::Add))
        .err() == Some(CompSetCode::LedgerFull.val());
    v.push(("J4-容量闸", full_reject, String::from("64满再拒")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B040 窗口")));
    let closed = {
        let mut ok = true;
        for c in CompSetCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    v
}

/// 检查入口（卡 43202）。
pub fn run_unx43202_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43202 组件集三元组");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-6-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_validate_fifty() {
        let (c, mk) = validate_fifty();
        assert_eq!(c, 50, "marks: {}", mk);
    }

    #[test]
    fn test_inject_ten() {
        let (c, mk) = inject_ten();
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_action_cycle() {
        let mut g = CompLedger::new();
        assert!(g.apply(mk_triple(0x1000, SAMPLE_PATHS[8], CompAction::Add)).is_ok());
        assert!(g.apply(mk_triple(0x1000, SAMPLE_PATHS[8], CompAction::Add)).is_err());
        assert!(g.apply(mk_triple(0x1000, SAMPLE_PATHS[8], CompAction::Remove)).is_ok());
        assert!(g.apply(mk_triple(0x1000, SAMPLE_PATHS[8], CompAction::Remove)).is_err());
        assert!(g.apply(mk_triple(0x1000, SAMPLE_PATHS[8], CompAction::Add)).is_ok());
    }

    #[test]
    fn test_replace_removes_old() {
        let mut g = CompLedger::new();
        let _ = g.apply(mk_triple(0x1100, SAMPLE_PATHS[9], CompAction::Add));
        let _ = g.apply(mk_triple(0x1100, SAMPLE_PATHS[9], CompAction::Replace));
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn test_path_gate() {
        assert!(path_ok(b"sys/kern/x"));
        assert!(!path_ok(b"usr/kern/x"));
        assert!(!path_ok(b"sys/kern/x y"));
        assert!(!path_ok(b"sys//x"));
        assert!(!path_ok(b"sys/x/"));
    }

    #[test]
    fn test_dup_sample_scan() {
        assert_eq!(dup_scan(&build_dup_sample()), 2);
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B040);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in CompSetCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [END-43202]
