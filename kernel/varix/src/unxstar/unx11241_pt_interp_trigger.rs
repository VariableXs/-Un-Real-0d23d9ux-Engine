//! UNX-F11241 PT_INTERP 解析与 ld-linux 装载触发（0x1f290 段）
//!
//! 锚点：UNX-F11241-J1 interp 路径解析与解释器装载判据绿，缺 interp 的静态件
//! 直达入口、动态件必经 ld.so，双路径分叉 100% 正确。
//!
//! 纯功能模型：内核在 exec 收尾时扫程序头表，寻找 PT_INTERP。找不到就是
//! 静态件——入口取 ELF 头 e_entry，直达用户入口；找到则把 interp 段当路径
//! 解析（绝对路径、尾 NUL、无内部 NUL、长度在 PATH_MAX 内、登记表内可识别），
//! 通过后把控制权交给解释器，入口是解释器自己的入口，不是主程序 e_entry。
//!
//! 规则面：程序头表条数 1..=8、e_type ∈ {ET_EXEC, ET_DYN}、PT_INTERP 至多
//! 一条、区间 [p_offset, p_offset+p_filesz) 必须落在文件内、解释器必须是登记
//! 表三选一（glibc x86-64 / glibc i386 / musl x86-64）。双路径分叉由
//! `expect_dynamic` / `expect_static` 两个断言面把守：动态件漏装载器与静态件
//! 误走装载器共用 EDYN 拒止码，不区分「哪一侧错」——错就是错。
//!
//! 拒止面：头表越界/类型非法/多条 interp、区间越文件尾、段过短、路径非绝对、
//! 缺尾 NUL 或内部 NUL、超长、解释器未登记、未绑定即触发、双触发、双路径
//! 断言不符、冻结后改写。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f290;
/// 程序头表越界 / e_type 非法 / PT_INTERP 重复
pub const IP_EPHDR: u32 = SEG_BASE + 0x01;
/// PT_INTERP 区间越过文件尾
pub const IP_ETRUNC: u32 = SEG_BASE + 0x02;
/// interp 段长度不足（< 2 字节）
pub const IP_ESHORT: u32 = SEG_BASE + 0x03;
/// interp 路径不是绝对路径
pub const IP_EABS: u32 = SEG_BASE + 0x04;
/// 缺尾 NUL 或路径内部含 NUL
pub const IP_ENUL: u32 = SEG_BASE + 0x05;
/// 路径超出 PATH_MAX
pub const IP_ELONG: u32 = SEG_BASE + 0x06;
/// 解释器不在登记表
pub const IP_EUNK: u32 = SEG_BASE + 0x07;
/// 装载触发顺序错（未绑定即触发 / 双触发）
pub const IP_EORDER: u32 = SEG_BASE + 0x08;
/// 双路径分叉断言不符（动态漏装载器 / 静态误走装载器）
pub const IP_EDYN: u32 = SEG_BASE + 0x09;
/// 冻结后改写
pub const IP_EFROZEN: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 程序头表最大条数
pub const MAX_PHDRS: usize = 8;
/// interp 路径缓冲上限
pub const PATH_MAX: usize = 64;
/// PT_INTERP 段类型
pub const PT_INTERP: u32 = 3;
/// PT_LOAD 段类型
pub const PT_LOAD: u32 = 1;
/// ET_EXEC
pub const ET_EXEC: u16 = 2;
/// ET_DYN
pub const ET_DYN: u16 = 3;
/// 路径：静态直达
pub const ROUTE_NONE: u8 = 0;
/// 路径：直达程序入口
pub const ROUTE_DIRECT: u8 = 1;
/// 路径：经解释器
pub const ROUTE_LOADER: u8 = 2;
/// 解释器种类：无
pub const KIND_NONE: u8 = 0;
/// 解释器种类：glibc
pub const KIND_GLIBC: u8 = 1;
/// 解释器种类：musl
pub const KIND_MUSL: u8 = 2;
/// 阶段：空
pub const STAGE_EMPTY: u8 = 0;
/// 阶段：已绑定（解析完成）
pub const STAGE_BOUND: u8 = 1;
/// 阶段：已触发
pub const STAGE_TRIGGERED: u8 = 2;
/// 阶段：冻结
pub const STAGE_FROZEN: u8 = 3;
/// 解释器登记表：路径 + 种类
pub const INTERPS: [(&str, u8); 3] = [
    ("/lib64/ld-linux-x86-64.so.2", KIND_GLIBC),
    ("/lib/ld-linux.so.2", KIND_GLIBC),
    ("/lib/ld-musl-x86_64.so.1", KIND_MUSL),
];
/// 解释器交接入口的占位基值（真实值由 ld.so 自举决定）
pub const LOADER_HANDOFF: u64 = 0x7f00_0000_0000_0000;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 数据结构
/// 一条程序头（只保留内核解析关心的字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhdrSpec {
    pub p_type: u32,
    pub p_offset: u32,
    pub p_filesz: u32,
    pub p_vaddr: u64,
}

impl PhdrSpec {
    /// PT_INTERP 头。
    pub fn interp(offset: u32, filesz: u32) -> PhdrSpec {
        PhdrSpec { p_type: PT_INTERP, p_offset: offset, p_filesz: filesz, p_vaddr: 0 }
    }

    /// PT_LOAD 头。
    pub fn load(offset: u32, filesz: u32, vaddr: u64) -> PhdrSpec {
        PhdrSpec { p_type: PT_LOAD, p_offset: offset, p_filesz: filesz, p_vaddr: vaddr }
    }

    /// 区间尾（饱和加法，避免溢出回绕成假通过）。
    pub fn end(&self) -> u32 {
        self.p_offset.saturating_add(self.p_filesz)
    }
}

/// 被 exec 的镜像：头字段 + 程序头表 + interp 段字节。
#[derive(Clone, Copy, Debug)]
pub struct Image {
    pub e_type: u16,
    pub entry: u64,
    pub file_size: u32,
    phdrs: [Option<PhdrSpec>; MAX_PHDRS],
    phnum: u8,
    overflow: bool,
    interp: [u8; PATH_MAX],
    interp_len: usize,
    interp_clipped: bool,
}

impl Image {
    /// 空镜像：只有头字段，头表待逐条追加。
    pub fn new(e_type: u16, entry: u64, file_size: u32) -> Image {
        Image {
            e_type,
            entry,
            file_size,
            phdrs: [None; MAX_PHDRS],
            phnum: 0,
            overflow: false,
            interp: [0u8; PATH_MAX],
            interp_len: 0,
            interp_clipped: false,
        }
    }

    /// 追加一条程序头；超表则置溢出标志（不静默丢）。
    pub fn with_phdr(mut self, p: PhdrSpec) -> Image {
        if (self.phnum as usize) >= MAX_PHDRS {
            self.overflow = true;
            return self;
        }
        if let Some(slot) = self.phdrs.get_mut(self.phnum as usize) {
            *slot = Some(p);
        }
        self.phnum = self.phnum.wrapping_add(1);
        self
    }

    /// 写入 interp 段字节（含尾 NUL 的原始字节）。
    pub fn with_interp(mut self, bytes: &[u8]) -> Image {
        let n = if bytes.len() > PATH_MAX { PATH_MAX } else { bytes.len() };
        if bytes.len() > PATH_MAX {
            self.interp_clipped = true;
        }
        let mut i = 0usize;
        while i < n {
            if let (Some(dst), Some(src)) = (self.interp.get_mut(i), bytes.get(i)) {
                *dst = *src;
            }
            i = i.wrapping_add(1);
        }
        self.interp_len = n;
        self
    }

    /// 头表条数。
    pub fn phnum(&self) -> usize {
        self.phnum as usize
    }

    /// 取一条程序头。
    pub fn phdr(&self, i: usize) -> Option<PhdrSpec> {
        if i >= self.phnum() {
            return None;
        }
        self.phdrs.get(i).and_then(|p| *p)
    }

    /// 头表是否超容。
    pub fn overflowed(&self) -> bool {
        self.overflow
    }

    /// interp 原始字节。
    pub fn interp_bytes(&self) -> &[u8] {
        self.interp.get(..self.interp_len).unwrap_or(&[])
    }

    /// interp 是否被 PATH_MAX 截断。
    pub fn interp_clipped(&self) -> bool {
        self.interp_clipped
    }

    /// 找 PT_INTERP（返回 (条数, 首条)）。
    pub fn find_interp(&self) -> (usize, Option<PhdrSpec>) {
        let mut n = 0usize;
        let mut first = None;
        let mut i = 0usize;
        while i < self.phnum() {
            if let Some(p) = self.phdr(i) {
                if p.p_type == PT_INTERP {
                    n = n.wrapping_add(1);
                    if first.is_none() {
                        first = Some(p);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        (n, first)
    }
}

/// interp 解析与装载触发器。
#[derive(Clone, Copy, Debug)]
pub struct InterpTrigger {
    image: Option<Image>,
    route: u8,
    kind: u8,
    stage: u8,
    path: [u8; PATH_MAX],
    path_len: usize,
    root: Option<u64>,
    /// 接受次数
    pub accepts: u64,
    /// 拒止次数
    pub rejects: u64,
    /// 静态直达统计
    pub statics: u64,
    /// 动态经装载器统计
    pub dynamics: u64,
    /// 成功触发次数
    pub triggers: u64,
}

impl InterpTrigger {
    /// 空触发器。
    pub fn new() -> Self {
        InterpTrigger {
            image: None,
            route: ROUTE_NONE,
            kind: KIND_NONE,
            stage: STAGE_EMPTY,
            path: [0u8; PATH_MAX],
            path_len: 0,
            root: None,
            accepts: 0,
            rejects: 0,
            statics: 0,
            dynamics: 0,
            triggers: 0,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> u8 {
        self.stage
    }

    /// 装载路径（NONE / DIRECT / LOADER）。
    pub fn route(&self) -> u8 {
        self.route
    }

    /// 解释器种类。
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// 已解析的 interp 路径字节。
    pub fn path_bytes(&self) -> &[u8] {
        self.path.get(..self.path_len).unwrap_or(&[])
    }

    /// 路径长度。
    pub fn path_len(&self) -> usize {
        self.path_len
    }

    /// 本次装载的入口目标：直达取 e_entry，经解释器取交接占位。
    pub fn entry_target(&self) -> Option<u64> {
        match self.image {
            Some(img) => match self.route {
                ROUTE_DIRECT => Some(img.entry),
                ROUTE_LOADER => Some(LOADER_HANDOFF),
                _ => None,
            },
            None => None,
        }
    }

    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl InterpTrigger {
    /// 绑定镜像：扫头表、判静态/动态、解析 interp 路径。
    pub fn bind(&mut self, img: Image) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(IP_EFROZEN);
        }
        if self.stage != STAGE_EMPTY {
            return self.fail(IP_EORDER);
        }
        if img.overflowed() || img.phnum() == 0 {
            return self.fail(IP_EPHDR);
        }
        if img.e_type != ET_EXEC && img.e_type != ET_DYN {
            return self.fail(IP_EPHDR);
        }
        let (n, first) = img.find_interp();
        if n > 1 {
            return self.fail(IP_EPHDR);
        }
        self.image = Some(img);
        if n == 0 {
            self.route = ROUTE_DIRECT;
            self.kind = KIND_NONE;
            self.path_len = 0;
            self.stage = STAGE_BOUND;
            self.statics = self.statics.wrapping_add(1);
            self.accepts = self.accepts.wrapping_add(1);
            return Ok(self.fold());
        }
        let p = match first {
            Some(p) => p,
            None => return self.fail(IP_EPHDR),
        };
        if img.interp_clipped() {
            return self.fail(IP_ELONG);
        }
        if p.p_filesz < 2 {
            return self.fail(IP_ESHORT);
        }
        if p.p_offset >= img.file_size || p.end() > img.file_size {
            return self.fail(IP_ETRUNC);
        }
        let bytes = img.interp_bytes();
        if bytes.len() < p.p_filesz as usize {
            return self.fail(IP_ETRUNC);
        }
        if bytes.first().copied() != Some(b'/') {
            return self.fail(IP_EABS);
        }
        if bytes.last().copied() != Some(0) {
            return self.fail(IP_ENUL);
        }
        let mut i = 0usize;
        while i.wrapping_add(1) < bytes.len() {
            if bytes.get(i).copied() == Some(0) {
                return self.fail(IP_ENUL);
            }
            i = i.wrapping_add(1);
        }
        let kind = match lookup_kind(bytes) {
            Some(k) => k,
            None => return self.fail(IP_EUNK),
        };
        // 去除尾 NUL 后落路径账
        self.path_len = 0;
        let n_copy = bytes.len().saturating_sub(1);
        let mut k = 0usize;
        while k < n_copy && k < PATH_MAX {
            if let (Some(dst), Some(src)) = (self.path.get_mut(k), bytes.get(k)) {
                *dst = *src;
            }
            k = k.wrapping_add(1);
        }
        self.path_len = k;
        self.route = ROUTE_LOADER;
        self.kind = kind;
        self.stage = STAGE_BOUND;
        self.dynamics = self.dynamics.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 触发装载：已绑定才可触发，静态直达 / 动态交解释器。
    pub fn trigger(&mut self) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(IP_EFROZEN);
        }
        if self.stage != STAGE_BOUND && self.stage != STAGE_TRIGGERED {
            return self.fail(IP_EORDER);
        }
        if self.stage == STAGE_TRIGGERED {
            return self.fail(IP_EORDER);
        }
        self.stage = STAGE_TRIGGERED;
        self.triggers = self.triggers.wrapping_add(1);
        Ok(self.fold())
    }

    /// 断言「本件是动态件，必须经 ld.so」。
    pub fn expect_dynamic(&mut self) -> Result<(), u32> {
        if self.route != ROUTE_LOADER {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(IP_EDYN);
        }
        Ok(())
    }

    /// 断言「本件是静态件，必须直达入口」。
    pub fn expect_static(&mut self) -> Result<(), u32> {
        if self.route != ROUTE_DIRECT {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(IP_EDYN);
        }
        Ok(())
    }

    /// FNV-1a：镜像头 + 头表 + interp 字节 + 路径 + 路径种类 + 计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        if let Some(img) = self.image {
            h = (h ^ (img.e_type as u64)).wrapping_mul(FNV_PRIME);
            h = (h ^ img.entry).wrapping_mul(FNV_PRIME);
            h = (h ^ (img.file_size as u64)).wrapping_mul(FNV_PRIME);
            h = (h ^ (img.phnum() as u64)).wrapping_mul(FNV_PRIME);
            let mut i = 0usize;
            while i < img.phnum() {
                if let Some(p) = img.phdr(i) {
                    h = (h ^ (p.p_type as u64)).wrapping_mul(FNV_PRIME);
                    h = (h ^ (p.p_offset as u64)).wrapping_mul(FNV_PRIME);
                    h = (h ^ (p.p_filesz as u64)).wrapping_mul(FNV_PRIME);
                    h = (h ^ p.p_vaddr).wrapping_mul(FNV_PRIME);
                }
                i = i.wrapping_add(1);
            }
            h = fnv_bytes(h, img.interp_bytes());
        }
        h = (h ^ (self.route as u64)).wrapping_mul(FNV_PRIME);
        h = (h ^ (self.kind as u64)).wrapping_mul(FNV_PRIME);
        h = fnv_bytes(h, self.path_bytes());
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h = (h ^ self.statics).wrapping_mul(FNV_PRIME);
        h = (h ^ self.dynamics).wrapping_mul(FNV_PRIME);
        h = (h ^ self.triggers).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结：必须已完成触发，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return match self.root {
                Some(v) => Ok(v),
                None => self.fail(IP_EORDER),
            };
        }
        if self.stage != STAGE_TRIGGERED {
            return self.fail(IP_EORDER);
        }
        self.stage = STAGE_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 回放：重算指纹并与冻结值比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.stage != STAGE_FROZEN {
            return self.fail(IP_EORDER);
        }
        let v = self.fold();
        if Some(v) != self.root {
            return self.fail(IP_EORDER);
        }
        Ok(v)
    }

    /// 冻结值。
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// 已绑定镜像的只读视图。
    pub fn image_ref(&self) -> Option<&Image> {
        self.image.as_ref()
    }
}

fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = (h ^ (*b as u64)).wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 在登记表里查解释器种类（bytes 含尾 NUL）。
pub fn lookup_kind(bytes: &[u8]) -> Option<u8> {
    if bytes.last().copied() != Some(0) {
        return None;
    }
    let n = bytes.len().saturating_sub(1);
    let mut i = 0usize;
    while i < INTERPS.len() {
        if let Some(row) = INTERPS.get(i) {
            let name = row.0.as_bytes();
            if name.len() == n {
                let mut same = true;
                let mut k = 0usize;
                while k < n {
                    if bytes.get(k).copied() != name.get(k).copied() {
                        same = false;
                    }
                    k = k.wrapping_add(1);
                }
                if same {
                    return Some(row.1);
                }
            }
        }
        i = i.wrapping_add(1);
    }
    None
}

/// 动态镜像构造函数：一条 PT_LOAD + 一条 PT_INTERP，interp 字节由调用方给。
pub fn dyn_image(path: &[u8]) -> Image {
    let seg = PhdrSpec::interp(0x40, path.len() as u32);
    Image::new(ET_DYN, 0x1000, 0x1000)
        .with_phdr(PhdrSpec::load(0, 0x400, 0x0))
        .with_phdr(seg)
        .with_interp(path)
}

/// 静态镜像：只有 PT_LOAD，无 interp。
pub fn static_image() -> Image {
    Image::new(ET_EXEC, 0x40_1000, 0x1000).with_phdr(PhdrSpec::load(0, 0x400, 0x40_0000))
}

/// 建场（动态）：glibc 解释器，绑定 → 双路径断言 → 触发 → 冻结 → 回放。
pub fn build_dynamic() -> (InterpTrigger, bool) {
    let mut t = InterpTrigger::new();
    let ok0 = t.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2\0")).is_ok();
    let ok1 = t.expect_dynamic().is_ok();
    let ok2 = t.trigger().is_ok();
    let ok3 = t.freeze().is_ok();
    let ok4 = t.replay().is_ok();
    (t, ok0 && ok1 && ok2 && ok3 && ok4)
}

/// 建场（静态）：无 interp，直达 e_entry。
pub fn build_static() -> (InterpTrigger, bool) {
    let mut t = InterpTrigger::new();
    let ok0 = t.bind(static_image()).is_ok();
    let ok1 = t.expect_static().is_ok();
    let ok2 = t.trigger().is_ok();
    let ok3 = t.freeze().is_ok();
    let ok4 = t.replay().is_ok();
    (t, ok0 && ok1 && ok2 && ok3 && ok4)
}

/// 四类独立注入：相对路径 / 缺尾 NUL / 未登记解释器 / 顺序错。
pub fn inject_four() -> [bool; 4] {
    // ① 相对路径
    let mut a0 = InterpTrigger::new();
    let r0 = a0.bind(dyn_image(b"lib64/ld-linux-x86-64.so.2 ")).err() == Some(IP_EABS);

    // ② 缺尾 NUL
    let mut a1 = InterpTrigger::new();
    let r1 = a1.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2")).err() == Some(IP_ENUL);

    // ③ 未登记解释器
    let mut a2 = InterpTrigger::new();
    let r2 = a2.bind(dyn_image(b"/opt/ld-none.so\0")).err() == Some(IP_EUNK)
        && a2.dynamics == 0;

    // ④ 顺序错：未绑定即触发、绑两次、双触发
    let mut a3 = InterpTrigger::new();
    let e0 = a3.trigger().err() == Some(IP_EORDER);
    let b0 = a3.bind(static_image()).is_ok();
    let e1 = a3.bind(static_image()).err() == Some(IP_EORDER);
    let t0 = a3.trigger().is_ok();
    let e2 = a3.trigger().err() == Some(IP_EORDER);
    let r3 = e0 && b0 && e1 && t0 && e2;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11241 PT_INTERP 解析与 ld-linux 装载触发。
pub fn run_unx11241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11241");
    let (mut d, dok) = build_dynamic();
    let (mut st, sok) = build_static();

    // IP1 程序头扫描
    let scan_ok = match d.image_ref() {
        Some(img) => {
            let (n, first) = img.find_interp();
            img.phnum() == 2 && n == 1 && first.is_some() && first.map(|p| p.p_type) == Some(PT_INTERP)
        }
        None => false,
    };
    s.add(
        "IP1-程序头扫描",
        dok && scan_ok,
        "两条头中恰有一条 PT_INTERP，区间与文件尾自洽",
    );

    // IP2 动态件经装载器
    s.add_dyn(
        "IP2-动态件经装载器",
        dok && d.route() == ROUTE_LOADER && d.entry_target() == Some(LOADER_HANDOFF) && d.dynamics == 1,
        &format!("route={} kind={} dyn={}", d.route(), d.kind(), d.dynamics),
    );

    // IP3 静态件直达入口
    s.add_dyn(
        "IP3-静态件直达入口",
        sok && st.route() == ROUTE_DIRECT && st.entry_target() == Some(0x40_1000) && st.statics == 1,
        &format!("route={} entry={:x?}", st.route(), st.entry_target()),
    );

    // IP4 路径逐字
    s.add(
        "IP4-路径逐字解析",
        dok && d.path_len() == 27 && d.path_bytes() == &b"/lib64/ld-linux-x86-64.so.2"[..],
        "interp 路径逐字节落账，尾 NUL 剥离",
    );

    // IP5 musl 识别
    let mut m5 = InterpTrigger::new();
    let b5 = m5.bind(dyn_image(b"/lib/ld-musl-x86_64.so.1\0")).is_ok();
    s.add(
        "IP5-musl 解释器识别",
        b5 && m5.kind() == KIND_MUSL && m5.route() == ROUTE_LOADER,
        "登记表第二族（musl）按路径识别",
    );

    // IP6 相对路径拒
    let mut p6 = InterpTrigger::new();
    s.add(
        "IP6-相对路径拒止",
        p6.bind(dyn_image(b"ld-linux-x86-64.so.2\0")).err() == Some(IP_EABS)
            && p6.route() == ROUTE_NONE
            && p6.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2\0")).is_ok(),
        "非绝对路径拒且拒后账未绑定，合法路径可重绑",
    );

    // IP7 NUL 类拒止
    let mut p7 = InterpTrigger::new();
    let no_tail = p7.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2")).err() == Some(IP_ENUL);
    let mut p7b = InterpTrigger::new();
    let inner = p7b.bind(dyn_image(b"/lib64/ld\0-linux.so.2\0")).err() == Some(IP_ENUL);
    s.add_dyn(
        "IP7-NUL 类拒止",
        no_tail && inner,
        &format!("tail={} inner={}", no_tail as u8, inner as u8),
    );

    // IP8 区间越尾与过短拒
    let mut p8 = InterpTrigger::new();
    let over = Image::new(ET_DYN, 0x1000, 0x100)
        .with_phdr(PhdrSpec::load(0, 0x40, 0))
        .with_phdr(PhdrSpec::interp(0x40, 0x1000))
        .with_interp(b"/lib64/ld-linux-x86-64.so.2\0");
    let trunc = p8.bind(over).err() == Some(IP_ETRUNC);
    let mut p8b = InterpTrigger::new();
    let short = p8b
        .bind(Image::new(ET_DYN, 0x1000, 0x1000).with_phdr(PhdrSpec::interp(0x40, 1)).with_interp(b"/\0"))
        .err()
        == Some(IP_ESHORT);
    s.add(
        "IP8-区间越尾与过短拒",
        trunc && short,
        "interp 区间越文件尾拒；段长不足两字节拒",
    );

    // IP9 未登记解释器拒
    let mut p9 = InterpTrigger::new();
    s.add(
        "IP9-未登记解释器拒",
        p9.bind(dyn_image(b"/usr/local/ld-custom.so.1\0")).err() == Some(IP_EUNK)
            && p9.kind() == KIND_NONE
            && p9.accepts == 0,
        "登记表三选一之外的解释器一律拒",
    );

    // IP10 双路径断言不符
    let (mut q10, _) = { let mut t = InterpTrigger::new(); let _ = t.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2\0")); (t, ()) };
    let wrong_side = q10.expect_static().err() == Some(IP_EDYN);
    let (mut q10b, _) = { let mut t = InterpTrigger::new(); let _ = t.bind(static_image()); (t, ()) };
    let wrong_side2 = q10b.expect_dynamic().err() == Some(IP_EDYN);
    s.add(
        "IP10-双路径断言不符",
        wrong_side && wrong_side2 && q10.rejects == 1 && q10b.rejects == 1,
        "动态件断言静态、静态件断言动态同罪 EDYN",
    );

    // IP11 顺序拒止
    let mut p11 = InterpTrigger::new();
    let e0 = p11.trigger().err() == Some(IP_EORDER);
    let (mut p11b, s11b) = { let mut t = InterpTrigger::new(); let s = t.bind(static_image()).is_ok(); let _ = t.trigger(); (t, s) };
    let e1 = p11b.trigger().err() == Some(IP_EORDER);
    let e2 = p11b.bind(static_image()).err() == Some(IP_EORDER);
    s.add_dyn(
        "IP11-顺序拒止",
        e0 && s11b && e1 && e2,
        &format!("unbound={} retrigger={} rebind={}", e0 as u8, e1 as u8, e2 as u8),
    );

    // IP12 冻结幂等与回放
    let f1 = d.freeze();
    let f2 = d.freeze();
    let rp = d.replay();
    s.add_dyn(
        "IP12-冻结幂等与回放",
        f1 == f2 && f1.is_ok() && f1.ok() == d.root() && rp.ok() == d.root() && dok,
        &format!("root={:x?} replay={}", d.root(), rp.is_ok() as u8),
    );

    // IP13 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits.get(k).copied().unwrap_or(false) {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    s.add_dyn(
        "IP13-注入四拒",
        all && d.rejects == 0 && d.triggers == 1 && st.triggers == 1,
        &format!(
            "inject={}{}{}{} trig={}/{}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            d.triggers, st.triggers
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11241_dual_path_flow() {
        let (d, dok) = build_dynamic();
        assert!(dok);
        assert_eq!(d.route(), ROUTE_LOADER);
        assert_eq!(d.kind(), KIND_GLIBC);
        assert_eq!(d.entry_target(), Some(LOADER_HANDOFF));
        assert_eq!(d.path_bytes(), &b"/lib64/ld-linux-x86-64.so.2"[..]);
        assert_eq!(d.stage(), STAGE_FROZEN);
        let (s, sok) = build_static();
        assert!(sok);
        assert_eq!(s.route(), ROUTE_DIRECT);
        assert_eq!(s.entry_target(), Some(0x40_1000));
        assert_eq!(s.kind(), KIND_NONE);
        assert_eq!(s.path_len(), 0);
        assert_eq!(s.stage(), STAGE_FROZEN);
    }

    #[test]
    fn f11241_gates_reject_count() {
        let mut t = InterpTrigger::new();
        assert_eq!(t.trigger(), Err(IP_EORDER));
        assert_eq!(t.bind(Image::new(1, 0x1000, 0x1000).with_phdr(PhdrSpec::load(0, 0x40, 0))), Err(IP_EPHDR));
        assert_eq!(t.bind(Image::new(ET_DYN, 0x1000, 0x1000)), Err(IP_EPHDR));
        let dual = Image::new(ET_DYN, 0x1000, 0x1000)
            .with_phdr(PhdrSpec::interp(0, 8))
            .with_phdr(PhdrSpec::interp(0, 8))
            .with_interp(b"/a\0\0\0\0\0");
        assert_eq!(t.bind(dual), Err(IP_EPHDR));
        assert_eq!(t.bind(dyn_image(b"lib64/ld-linux-x86-64.so.2\0")), Err(IP_EABS));
        assert_eq!(t.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2")), Err(IP_ENUL));
        assert_eq!(t.bind(dyn_image(b"/opt/ld-none.so\0")), Err(IP_EUNK));
        let over = Image::new(ET_DYN, 0x1000, 0x100)
            .with_phdr(PhdrSpec::interp(0x40, 0x1000))
            .with_interp(b"/lib64/ld-linux-x86-64.so.2\0");
        assert_eq!(t.bind(over), Err(IP_ETRUNC));
        let short = Image::new(ET_DYN, 0x1000, 0x1000)
            .with_phdr(PhdrSpec::interp(0x40, 1))
            .with_interp(b"/\0");
        assert_eq!(t.bind(short), Err(IP_ESHORT));
        let long = b"/lib64/ld-linux-x86-64.so.2-with-a-very-long-suffix-that-overflows-pathmax\0";
        assert_eq!(t.bind(dyn_image(long)), Err(IP_ELONG));
        assert!(t.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2\0")).is_ok());
        assert_eq!(t.bind(dyn_image(b"/lib64/ld-linux-x86-64.so.2\0")), Err(IP_EORDER));
        assert_eq!(t.expect_static(), Err(IP_EDYN));
        assert!(t.trigger().is_ok());
        assert_eq!(t.trigger(), Err(IP_EORDER));
        assert!(t.freeze().is_ok());
        assert_eq!(t.trigger(), Err(IP_EFROZEN));
        assert_eq!(t.bind(static_image()), Err(IP_EFROZEN));
        assert_eq!(t.rejects, 15);
        assert_eq!(t.accepts, 1);
        assert_eq!(t.dynamics, 1);
        assert_eq!(t.triggers, 1);
    }

    #[test]
    fn f11241_rules_lookup() {
        assert_eq!(lookup_kind(b"/lib64/ld-linux-x86-64.so.2\0"), Some(KIND_GLIBC));
        assert_eq!(lookup_kind(b"/lib/ld-linux.so.2\0"), Some(KIND_GLIBC));
        assert_eq!(lookup_kind(b"/lib/ld-musl-x86_64.so.1\0"), Some(KIND_MUSL));
        assert_eq!(lookup_kind(b"/lib64/ld-linux-x86-64.so.3\0"), None);
        assert_eq!(lookup_kind(b"/lib64/ld-linux-x86-64.so.2"), None);
        let img = dyn_image(b"/lib/ld-musl-x86_64.so.1\0");
        let (n, f) = img.find_interp();
        assert_eq!(n, 1);
        assert_eq!(f.map(|p| p.p_offset), Some(0x40));
        let empty = static_image();
        assert_eq!(empty.find_interp().0, 0);
        assert_eq!(empty.phnum(), 1);
    }

    #[test]
    fn f11241_injection_four() {
        let hits = inject_four();
        assert!(hits.iter().all(|x| *x));
    }

    #[test]
    fn f11241_checks_green() {
        let cs = run_unx11241_checks();
        assert!(cs.all_passed());
        assert_eq!(cs.len(), 13);
    }
}
