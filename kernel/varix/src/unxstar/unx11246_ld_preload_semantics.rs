//! UNX-F11246 LD_PRELOAD 语义与安全边界（0x1f2e0 段）
//!
//! 锚点：UNX-F11246-J1 preload 装载序（先于 NEEDED）判据绿，AT_SECURE 置位时
//! preload 静默忽略与 Linux 一致。
//!
//! 纯功能模型：LD_PRELOAD 是一份「抢在 DT_NEEDED 之前落位」的库清单。解析
//! 环境串（冒号或空格分隔）得到 token 序列，逐个在文件表里解析成绝对路径，
//! 按**清单原序**排在装载计划最前面；随后才接 NEEDED 依赖，且已在 preload
//! 落位的库不再重复出现——这正是插装（interposition）能生效的原因：符号
//! 解析看到的第一份定义来自 preload。
//!
//! 规则面：token 为空即跳过（不记错）；含 '/' 但不是绝对路径者拒（EPATH）；
//! 文件表里查不到者拒（ENOENT）；文件标记为不可信（未签名/可写目录）者拒
//! （EWORLD）；重复路径只落一次但计重账；计划容量八条。
//!
//! 安全边界：AT_SECURE 置位（setuid/setgid 或文件能力）时，整份 LD_PRELOAD
//! **静默忽略**——不报错、不装载、只把忽略条数入账，装载计划退化为纯 NEEDED。
//! 这是 Linux 的真实行为，也是本模块最硬的一条：安全模式下没有「部分生效」。
//! 显式调用 `force_preload` 只会拿到 ESECURE，绝无旁路。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2e0;
/// token 含 '/' 但不是绝对路径
pub const PL_EPATH: u32 = SEG_BASE + 0x01;
/// 文件表里查不到 token
pub const PL_ENOENT: u32 = SEG_BASE + 0x02;
/// 阶段错（未解析即取计划 / 重复解析）
pub const PL_EORDER: u32 = SEG_BASE + 0x03;
/// 安全模式下强行启用 preload
pub const PL_ESECURE: u32 = SEG_BASE + 0x04;
/// 文件表重号
pub const PL_EDUP: u32 = SEG_BASE + 0x05;
/// 计划或文件表超容
pub const PL_EFULL: u32 = SEG_BASE + 0x06;
/// 空账（无文件或计划为空）
pub const PL_ESTATE: u32 = SEG_BASE + 0x07;
/// 冻结后改写
pub const PL_EFROZEN: u32 = SEG_BASE + 0x08;
/// 不可信文件不得 preload
pub const PL_EWORLD: u32 = SEG_BASE + 0x09;
/// 文件名为空
pub const PL_ENAME: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 文件表容量
pub const FILE_CAP: usize = 16;
/// 装载计划容量
pub const PLAN_CAP: usize = 8;
/// 状态：建场
pub const ST_BUILD: u8 = 0;
/// 状态：已解析
pub const ST_PARSED: u8 = 1;
/// 状态：冻结
pub const ST_FROZEN: u8 = 2;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

const NAME_MAX: usize = 24;
const PATH_MAX: usize = 40;
const NEED_CAP: usize = 4;

/// 文件表行：库名、绝对路径、可信位
#[derive(Clone, Copy)]
pub struct PreloadFile {
    name: [u8; NAME_MAX],
    name_len: u8,
    path: [u8; PATH_MAX],
    path_len: u8,
    trusted: bool,
}

impl PreloadFile {
    /// 构造一行；名字或路径超长返回 None
    pub fn new(name: &str, path: &str, trusted: bool) -> Option<Self> {
        let mut row = PreloadFile {
            name: [0u8; NAME_MAX],
            name_len: 0,
            path: [0u8; PATH_MAX],
            path_len: 0,
            trusted,
        };
        row.name_len = put(&mut row.name, name)?;
        row.path_len = put(&mut row.path, path)?;
        Some(row)
    }
    /// 库名
    pub fn name(&self) -> &str { str_at(&self.name, self.name_len) }
    /// 绝对路径
    pub fn path(&self) -> &str { str_at(&self.path, self.path_len) }
    /// 可信位
    pub fn trusted(&self) -> bool { self.trusted }
}

fn put(dst: &mut [u8], s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() > dst.len() { return None; }
    let mut i = 0;
    while i < b.len() { dst[i] = b[i]; i += 1; }
    Some(b.len() as u8)
}

fn str_at(buf: &[u8], len: u8) -> &str {
    let n = len as usize;
    if n > buf.len() { return ""; }
    match core::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => "",
    }
}

/// LD_PRELOAD 装载计划：文件表 + NEEDED + 解析后的落位序
pub struct PreloadPlan {
    files: [Option<PreloadFile>; FILE_CAP],
    file_count: usize,
    needed: [[u8; NAME_MAX]; NEED_CAP],
    needed_len: [u8; NEED_CAP],
    needed_count: usize,
    plan_path: [[u8; PATH_MAX]; PLAN_CAP],
    plan_len: [u8; PLAN_CAP],
    plan_pre: [bool; PLAN_CAP],
    plan_count: usize,
    secure: bool,
    state: u8,
    root: Option<u64>,
    cursor: usize,
    /// 成功解析次数
    pub accepts: u32,
    /// 拒绝次数
    pub rejects: u32,
    /// 安全模式下被静默忽略的 token 数
    pub ignored: u32,
    /// 去重命中次数
    pub deduped: u32,
    /// 已消费落位条数
    pub loads: u32,
}

impl PreloadPlan {
    /// 空场：无文件、无 NEEDED、非安全模式
    pub fn new() -> Self {
        PreloadPlan {
            files: [None; FILE_CAP],
            file_count: 0,
            needed: [[0u8; NAME_MAX]; NEED_CAP],
            needed_len: [0u8; NEED_CAP],
            needed_count: 0,
            plan_path: [[0u8; PATH_MAX]; PLAN_CAP],
            plan_len: [0u8; PLAN_CAP],
            plan_pre: [false; PLAN_CAP],
            plan_count: 0,
            secure: false,
            state: ST_BUILD,
            root: None,
            cursor: 0,
            accepts: 0,
            rejects: 0,
            ignored: 0,
            deduped: 0,
            loads: 0,
        }
    }

    fn bump(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前状态
    pub fn stage(&self) -> u8 { self.state }
    /// 安全模式位
    pub fn secure(&self) -> bool { self.secure }
    /// 文件表条数
    pub fn file_count(&self) -> usize { self.file_count }
    /// NEEDED 条数
    pub fn needed_count(&self) -> usize { self.needed_count }
    /// 落位条数（未解析时为零）
    pub fn plan_count(&self) -> usize { self.plan_count }
    /// 落位序中第一条来自 preload 的库路径
    pub fn first_preload(&self) -> Option<&str> {
        let mut i = 0;
        while i < self.plan_count {
            if self.plan_pre[i] {
                return Some(str_at(&self.plan_path[i], self.plan_len[i]));
            }
            i += 1;
        }
        None
    }

    /// 登记一行文件：名字非空、路径绝对、名字与路径都不重号、不超容
    pub fn add_file(&mut self, name: &str, path: &str, trusted: bool) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PL_EFROZEN)); }
        if name.as_bytes().is_empty() { return Err(self.bump(PL_ENAME)); }
        if !is_abs(path) { return Err(self.bump(PL_EPATH)); }
        let mut i = 0;
        while i < self.file_count {
            if let Some(f) = self.files[i] {
                if f.name() == name || f.path() == path {
                    return Err(self.bump(PL_EDUP));
                }
            }
            i += 1;
        }
        if self.file_count >= FILE_CAP { return Err(self.bump(PL_EFULL)); }
        let row = match PreloadFile::new(name, path, trusted) {
            Some(r) => r,
            None => return Err(self.bump(PL_EFULL)),
        };
        self.files[self.file_count] = Some(row);
        self.file_count = self.file_count.wrapping_add(1);
        Ok(self.file_count as u32)
    }

    /// 登记一条 NEEDED（裸库名，装载计划里排 preload 之后）
    pub fn add_needed(&mut self, name: &str) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PL_EFROZEN)); }
        if name.as_bytes().is_empty() { return Err(self.bump(PL_ENAME)); }
        let mut i = 0;
        while i < self.needed_count {
            if str_at(&self.needed[i], self.needed_len[i]) == name {
                return Err(self.bump(PL_EDUP));
            }
            i += 1;
        }
        if self.needed_count >= NEED_CAP { return Err(self.bump(PL_EFULL)); }
        let n = match put(&mut self.needed[self.needed_count], name) {
            Some(n) => n,
            None => return Err(self.bump(PL_EFULL)),
        };
        self.needed_len[self.needed_count] = n;
        self.needed_count = self.needed_count.wrapping_add(1);
        Ok(self.needed_count as u32)
    }

    /// 置安全模式位；只能在解析前设置一次
    pub fn set_secure(&mut self, on: bool) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PL_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(PL_EORDER)); }
        self.secure = on;
        Ok(if on { 1 } else { 0 })
    }

    fn find_ref(&self, tok: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.file_count {
            if let Some(f) = self.files[i] {
                if f.name() == tok || f.path() == tok {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn in_plan(&self, path: &str) -> bool {
        let mut i = 0;
        while i < self.plan_count {
            if str_at(&self.plan_path[i], self.plan_len[i]) == path {
                return true;
            }
            i += 1;
        }
        false
    }

    fn push_plan(&mut self, path: &str, pre: bool) -> Result<(), u32> {
        if self.in_plan(path) {
            self.deduped = self.deduped.wrapping_add(1);
            return Ok(());
        }
        if self.plan_count >= PLAN_CAP { return Err(self.bump(PL_EFULL)); }
        let idx = self.plan_count;
        let n = match put(&mut self.plan_path[idx], path) {
            Some(n) => n,
            None => return Err(self.bump(PL_EFULL)),
        };
        self.plan_len[idx] = n;
        self.plan_pre[idx] = pre;
        self.plan_count = self.plan_count.wrapping_add(1);
        Ok(())
    }

    /// 落位序第 i 条路径
    pub fn plan_at(&self, i: usize) -> Result<&str, u32> {
        if self.state == ST_BUILD { return Err(PL_EORDER); }
        if i >= self.plan_count { return Err(PL_ESTATE); }
        Ok(str_at(&self.plan_path[i], self.plan_len[i]))
    }

    /// preload 段是否整体排在 NEEDED 段之前（J1 的核心不变量）
    pub fn preload_before_needed(&self) -> bool {
        let mut seen_needed = false;
        let mut pre = 0usize;
        let mut need = 0usize;
        let mut i = 0;
        while i < self.plan_count {
            if self.plan_pre[i] {
                if seen_needed { return false; }
                pre += 1;
            } else {
                seen_needed = true;
                need += 1;
            }
            i += 1;
        }
        pre > 0 && need > 0
    }

    fn resolve_needed(&self, k: usize) -> Option<usize> {
        if k >= self.needed_count { return None; }
        let name = str_at(&self.needed[k], self.needed_len[k]);
        self.find_ref(name)
    }

    fn apply_token(&mut self, tok: &str) -> Result<(), u32> {
        if tok.as_bytes().is_empty() { return Ok(()); }
        if !is_abs(tok) && has_slash(tok) {
            return Err(self.bump(PL_EPATH));
        }
        let idx = match self.find_ref(tok) {
            Some(i) => i,
            None => return Err(self.bump(PL_ENOENT)),
        };
        let f = match self.files[idx] {
            Some(f) => f,
            None => return Err(self.bump(PL_ESTATE)),
        };
        if !f.trusted { return Err(self.bump(PL_EWORLD)); }
        self.push_plan(f.path(), true)
    }

    /// 解析 LD_PRELOAD 环境串：先 preload 原序，再 NEEDED
    pub fn parse(&mut self, list: &str) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PL_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(PL_EORDER)); }
        self.plan_count = 0;
        self.cursor = 0;
        let mut toks: Vec<&str> = Vec::new();
        split_tokens(list, &mut toks);
        if self.secure {
            // 安全模式：整份清单静默忽略，只把条数入账
            self.ignored = self.ignored.wrapping_add(toks.len() as u32);
        } else {
            let mut i = 0;
            while i < toks.len() {
                match self.apply_token(toks[i]) {
                    Ok(()) => {}
                    Err(c) => return Err(c),
                }
                i = i.wrapping_add(1);
            }
        }
        let mut k = 0;
        while k < self.needed_count {
            let idx = match self.resolve_needed(k) {
                Some(x) => x,
                None => return Err(self.bump(PL_ENOENT)),
            };
            let f = match self.files[idx] {
                Some(f) => f,
                None => return Err(self.bump(PL_ESTATE)),
            };
            match self.push_plan(f.path(), false) {
                Ok(()) => {}
                Err(c) => return Err(c),
            }
            k = k.wrapping_add(1);
        }
        self.state = ST_PARSED;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.plan_count as u32)
    }

    /// 强制启用 preload：安全模式下必拒，非安全模式须已解析
    pub fn force_preload(&mut self) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PL_EFROZEN)); }
        if self.secure { return Err(self.bump(PL_ESECURE)); }
        if self.state != ST_PARSED { return Err(self.bump(PL_EORDER)); }
        Ok(self.plan_count as u32)
    }

    /// 按落位序取下一张库并计一次装载
    pub fn next_load(&mut self) -> Result<&str, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(PL_EFROZEN)); }
        if self.state != ST_PARSED { return Err(self.bump(PL_EORDER)); }
        let i = self.cursor;
        if i >= self.plan_count { return Err(self.bump(PL_ESTATE)); }
        self.cursor = i.wrapping_add(1);
        self.loads = self.loads.wrapping_add(1);
        Ok(str_at(&self.plan_path[i], self.plan_len[i]))
    }
}

fn is_abs(s: &str) -> bool {
    match s.as_bytes().first() {
        Some(b'/') => true,
        _ => false,
    }
}

fn has_slash(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' { return true; }
        i += 1;
    }
    false
}

fn split_tokens<'a>(list: &'a str, out: &mut Vec<&'a str>) {
    let b = list.as_bytes();
    let mut start = 0usize;
    while start <= b.len() {
        let mut end = start;
        while end < b.len() && b[end] != b':' && b[end] != b' ' {
            end += 1;
        }
        if end > start {
            if let Some(t) = list.get(start..end) {
                out.push(t);
            }
        }
        start = end + 1;
    }
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

fn mix_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut x = h;
    let mut i = 0;
    while i < b.len() {
        x = mix(x, b[i] as u64);
        i += 1;
    }
    x
}

impl PreloadPlan {
    /// FNV-1a 摘要：文件表、NEEDED、落位序与账本均入账
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.file_count {
            if let Some(f) = self.files[i] {
                h = mix_str(h, f.name());
                h = mix_str(h, f.path());
                h = mix(h, if f.trusted { 1 } else { 0 });
            }
            i += 1;
        }
        let mut k = 0;
        while k < self.needed_count {
            h = mix_str(h, str_at(&self.needed[k], self.needed_len[k]));
            k += 1;
        }
        let mut p = 0;
        while p < self.plan_count {
            h = mix_str(h, str_at(&self.plan_path[p], self.plan_len[p]));
            h = mix(h, if self.plan_pre[p] { 1 } else { 0 });
            p += 1;
        }
        h = mix(h, self.file_count as u64);
        h = mix(h, self.needed_count as u64);
        h = mix(h, self.plan_count as u64);
        h = mix(h, if self.secure { 1 } else { 0 });
        h = mix(h, self.accepts as u64);
        h = mix(h, self.rejects as u64);
        h = mix(h, self.ignored as u64);
        h = mix(h, self.deduped as u64);
        h
    }

    /// 冻结：只在已解析态成立，幂等
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            if let Some(r) = self.root {
                return Ok(r);
            }
        }
        if self.state != ST_PARSED { return Err(self.bump(PL_EORDER)); }
        self.state = ST_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 重放：摘要必须与冻结时一致
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN { return Err(self.bump(PL_EORDER)); }
        let v = self.fold();
        match self.root {
            Some(r) if r == v => Ok(v),
            _ => Err(self.bump(PL_ESTATE)),
        }
    }
}

fn sample_base() -> PreloadPlan {
    let mut p = PreloadPlan::new();
    let _ = p.add_file("libhook.so", "/opt/pre/libhook.so", true);
    let _ = p.add_file("libfast.so", "/opt/pre/libfast.so", true);
    let _ = p.add_file("libpoll.so", "/opt/pre/libpoll.so", false);
    let _ = p.add_file("libz.so.1", "/lib/libz.so.1", true);
    let _ = p.add_file("libc.so.6", "/lib/libc.so.6", true);
    let _ = p.add_file("libm.so.6", "/lib/libm.so.6", true);
    let _ = p.add_needed("libc.so.6");
    let _ = p.add_needed("libz.so.1");
    let _ = p.add_needed("libm.so.6");
    p
}

/// 常规样例：preload 两条（含一处重复）+ NEEDED 三条
pub fn build_sample() -> PreloadPlan {
    let mut p = sample_base();
    let _ = p.parse("libhook.so /opt/pre/libfast.so libhook.so");
    p
}

/// 安全样例：同一份清单在 AT_SECURE 下静默忽略
pub fn build_secure() -> PreloadPlan {
    let mut p = sample_base();
    let _ = p.set_secure(true);
    let _ = p.parse("libhook.so libfast.so /opt/pre/libpoll.so");
    p
}

/// 四路独立注入：每条都在新实例上证明一条拒绝路径会触发
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = sample_base();
    match a.parse("lib/evil.so") {
        Err(code) if code == PL_EPATH => { ok[0] = a.rejects == 1; }
        _ => {}
    }

    let mut b = sample_base();
    match b.parse("libnope.so") {
        Err(code) if code == PL_ENOENT => { ok[1] = b.rejects == 1; }
        _ => {}
    }

    let mut c = sample_base();
    match c.parse("/opt/pre/libpoll.so") {
        Err(code) if code == PL_EWORLD => { ok[2] = c.rejects == 1; }
        _ => {}
    }

    let mut d = sample_base();
    let _ = d.set_secure(true);
    let _ = d.parse("libhook.so");
    match d.force_preload() {
        Err(code) if code == PL_ESECURE => { ok[3] = d.ignored == 1 && d.rejects == 1; }
        _ => {}
    }

    ok
}

/// 十三项静态/动态判据
pub fn run_unx11246_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11246.preload");

    let fresh = PreloadPlan::new();
    s.add(
        "PL1-建场清零",
        fresh.stage() == ST_BUILD
            && fresh.file_count() == 0
            && fresh.needed_count() == 0
            && fresh.plan_count() == 0
            && !fresh.secure(),
        "static",
    );

    let mut b = sample_base();
    let dup = b.add_file("libhook.so", "/opt/pre/other.so", true) == Err(PL_EDUP);
    let nameless = b.add_file("", "/opt/pre/x.so", true) == Err(PL_ENAME);
    let rel = b.add_file("libr.so", "opt/pre/libr.so", true) == Err(PL_EPATH);
    s.add(
        "PL2-登记查重",
        dup && nameless && rel && b.file_count() == 6 && b.needed_count() == 3,
        "static",
    );

    let mut w = sample_base();
    s.add(
        "PL3-不可信拒载",
        w.parse("/opt/pre/libpoll.so") == Err(PL_EWORLD) && w.rejects == 1,
        "static",
    );

    let mut r = sample_base();
    s.add(
        "PL4-相对含斜杠拒",
        r.parse("lib/evil.so") == Err(PL_EPATH) && r.rejects == 1,
        "static",
    );

    let mut m = sample_base();
    s.add(
        "PL5-未知库拒",
        m.parse("libnope.so") == Err(PL_ENOENT) && m.rejects == 1,
        "static",
    );

    let smp = build_sample();
    s.add_dyn(
        "PL6-preload先于NEEDED",
        smp.preload_before_needed()
            && smp.plan_count() == 5
            && smp.plan_at(0) == Ok("/opt/pre/libhook.so")
            && smp.plan_at(2) == Ok("/lib/libc.so.6"),
        &format!("plan={} first={:?}", smp.plan_count(), smp.first_preload()),
    );

    s.add(
        "PL7-重复只落一次",
        smp.deduped == 1 && smp.plan_at(1) == Ok("/opt/pre/libfast.so"),
        "static",
    );

    let sec = build_secure();
    s.add_dyn(
        "PL8-安全模式静默忽略",
        sec.secure()
            && sec.ignored == 3
            && sec.plan_count() == 3
            && sec.first_preload().is_none()
            && sec.plan_at(0) == Ok("/lib/libc.so.6"),
        &format!("ignored={} plan={}", sec.ignored, sec.plan_count()),
    );

    let mut f = build_secure();
    s.add(
        "PL9-强制启用被拒",
        f.force_preload() == Err(PL_ESECURE) && f.rejects == 1,
        "static",
    );

    let mut e = PreloadPlan::new();
    s.add(
        "PL10-未解析无计划",
        e.plan_at(0) == Err(PL_EORDER)
            && e.next_load() == Err(PL_EORDER)
            && e.force_preload() == Err(PL_EORDER)
            && e.rejects == 2,
        "static",
    );

    let mut z = build_sample();
    let f1 = z.freeze();
    let f2 = z.freeze();
    let rp = z.replay();
    let locked = z.add_file("libx.so", "/opt/pre/libx.so", true) == Err(PL_EFROZEN);
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "PL11-冻结重放幂等",
        f1.is_ok() && f1 == f2 && rp == f1 && locked && z.rejects == 1,
        &format!("root={:x}", rootv),
    );

    let mut cap = PreloadPlan::new();
    let mut i = 0;
    while i < 9 {
        let name = match i {
            0 => "liba0.so",
            1 => "liba1.so",
            2 => "liba2.so",
            3 => "liba3.so",
            4 => "liba4.so",
            5 => "liba5.so",
            6 => "liba6.so",
            7 => "liba7.so",
            _ => "liba8.so",
        };
        let path = match i {
            0 => "/x/0.so",
            1 => "/x/1.so",
            2 => "/x/2.so",
            3 => "/x/3.so",
            4 => "/x/4.so",
            5 => "/x/5.so",
            6 => "/x/6.so",
            7 => "/x/7.so",
            _ => "/x/8.so",
        };
        let _ = cap.add_file(name, path, true);
        i += 1;
    }
    let full = cap.parse("liba0.so liba1.so liba2.so liba3.so liba4.so liba5.so liba6.so liba7.so liba8.so");
    s.add_dyn(
        "PL12-计划容量八条",
        full == Err(PL_EFULL) && cap.plan_count() == PLAN_CAP && cap.rejects == 1,
        &format!("plan={} cap={}", cap.plan_count(), PLAN_CAP),
    );

    let inj = inject_four();
    s.add(
        "PL13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_preload_then_needed() {
        let mut p = build_sample();
        assert_eq!(p.stage(), ST_PARSED);
        assert_eq!(p.file_count(), 6);
        assert_eq!(p.needed_count(), 3);
        assert_eq!(p.plan_count(), 5);
        assert!(p.preload_before_needed());
        assert_eq!(p.first_preload(), Some("/opt/pre/libhook.so"));
        assert_eq!(p.plan_at(0), Ok("/opt/pre/libhook.so"));
        assert_eq!(p.plan_at(1), Ok("/opt/pre/libfast.so"));
        assert_eq!(p.plan_at(2), Ok("/lib/libc.so.6"));
        assert_eq!(p.plan_at(4), Ok("/lib/libm.so.6"));

        assert_eq!(p.next_load(), Ok("/opt/pre/libhook.so"));
        assert_eq!(p.next_load(), Ok("/opt/pre/libfast.so"));
        assert_eq!(p.next_load(), Ok("/lib/libc.so.6"));
        assert_eq!(p.next_load(), Ok("/lib/libz.so.1"));
        assert_eq!(p.next_load(), Ok("/lib/libm.so.6"));
        assert_eq!(p.next_load(), Err(PL_ESTATE));
        assert_eq!(p.loads, 5);
    }

    #[test]
    fn gates_exact_rejects() {
        let mut x = PreloadPlan::new();
        assert_eq!(x.add_file("libhook.so", "/opt/pre/libhook.so", true), Ok(1));
        assert_eq!(x.add_file("libhook.so", "/opt/pre/other.so", true), Err(PL_EDUP));
        assert_eq!(x.add_file("", "/opt/pre/x.so", true), Err(PL_ENAME));
        assert_eq!(x.add_file("libr.so", "opt/pre/libr.so", true), Err(PL_EPATH));
        assert_eq!(x.add_file("libc.so.6", "/lib/libc.so.6", true), Ok(2));
        assert_eq!(x.add_needed("libc.so.6"), Ok(1));
        assert_eq!(x.add_needed("libc.so.6"), Err(PL_EDUP));
        assert_eq!(x.plan_at(0), Err(PL_EORDER));
        assert_eq!(x.set_secure(true), Ok(1));
        assert_eq!(x.parse("libhook.so"), Ok(1));
        assert_eq!(x.parse("libhook.so"), Err(PL_EORDER));
        assert_eq!(x.set_secure(false), Err(PL_EORDER));
        assert_eq!(x.force_preload(), Err(PL_ESECURE));
        assert!(x.freeze().is_ok());
        assert!(x.replay().is_ok());
        assert_eq!(x.add_needed("libm.so.6"), Err(PL_EFROZEN));
        assert_eq!(x.parse("libhook.so"), Err(PL_EFROZEN));
        assert_eq!(x.next_load(), Err(PL_EFROZEN));
        // 冻结后三次拒绝改变了账本，摘要随之漂移
        assert_eq!(x.replay(), Err(PL_ESTATE));
        assert_eq!(x.rejects, 11);
        assert_eq!(x.accepts, 1);
        assert_eq!(x.ignored, 1);
    }

    #[test]
    fn rules_precedence_and_codes() {
        let mut p = sample_base();
        assert_eq!(p.parse("lib/evil.so"), Err(PL_EPATH));
        let mut q = sample_base();
        assert_eq!(q.parse("lib/ghost.so"), Err(PL_EPATH));
        let mut r = sample_base();
        assert_eq!(r.parse("libghost.so"), Err(PL_ENOENT));
        let mut u = sample_base();
        assert_eq!(u.parse("libpoll.so"), Err(PL_EWORLD));

        let codes = [
            PL_EPATH, PL_ENOENT, PL_EORDER, PL_ESECURE, PL_EDUP,
            PL_EFULL, PL_ESTATE, PL_EFROZEN, PL_EWORLD, PL_ENAME,
        ];
        let mut i = 0;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + (i as u32) + 1);
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(SEG_BASE, 0x1f2e0);
    }

    #[test]
    fn injection_paths_fire() {
        let inj = inject_four();
        assert!(inj[0], "relative path with slash must be rejected");
        assert!(inj[1], "unknown library must be rejected");
        assert!(inj[2], "untrusted file must be rejected");
        assert!(inj[3], "secure mode must reject force_preload");
    }

    #[test]
    fn checks_green() {
        let cs = run_unx11246_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }
}
