//! UNX-F11252 段映射语义（PT_LOAD 逐段消费）（0x1f340 段）
//!
//! 锚点：UNX-F11252-J1 逐段 mmap（对齐/偏移/合并页语义）判据绿，段面布局与
//! readelf 宣告一致。
//!
//! 纯功能模型：PT_LOAD 段不是「从 0 到 filesz 的连续区间」，而是三组对齐后的
//! 页区间之和。内核只认页：虚拟起点向下取整到页，文件偏移也向下取整到页，
//! 末尾向上取整；一页里既可能只有文件前哨、也可能混着下个段的头。装载器逐段
//! 消费，**虚拟页连续且文件页连续、权限相同**的两个段合并成一行映射；共享
//! 同一页但文件页相同且权限相同的段只记一次合并，不新增行。
//!
//! 规则面：p_align 只接受 0/1 或二次幂；p_vaddr ≡ p_offset (mod p_align)（同余
//! 律）；p_memsz ≥ p_filesz；文件区间不得越过文件尾；虚址不得溢出；段按虚址
//! 升序登记（ELF 规范要求）；权限位非零且不超三位；消费后段表只读。
//!
//! 记账面：rows（合并后的映射行）、pages（真实页数）、merges（合并次数）、
//! zero_bytes（memsz 超出 filesz 的零填字节数）——四项一起构成段面布局摘要。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f340;
/// p_align 非 0/1/二次幂
pub const LP_EALIGN: u32 = SEG_BASE + 0x01;
/// p_vaddr 与 p_offset 不同余
pub const LP_ECONGRUENT: u32 = SEG_BASE + 0x02;
/// p_memsz < p_filesz 或虚址溢出
pub const LP_ESIZE: u32 = SEG_BASE + 0x03;
/// 文件区间越界
pub const LP_EBOUNDS: u32 = SEG_BASE + 0x04;
/// 顺序错（降序登记 / 未加段即消费 / 重复消费 / 消费后加段）
pub const LP_EORDER: u32 = SEG_BASE + 0x05;
/// 同一起点段重号
pub const LP_EDUP: u32 = SEG_BASE + 0x06;
/// 段表或行表满
pub const LP_EFULL: u32 = SEG_BASE + 0x07;
/// 空段表或重放摘要不符
pub const LP_ESTATE: u32 = SEG_BASE + 0x08;
/// 冻结后改写
pub const LP_EFROZEN: u32 = SEG_BASE + 0x09;
/// 权限位为零或超出三位
pub const LP_EFLAGS: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 页大小
pub const PAGE: u64 = 0x1000;
/// 段表容量
pub const SEG_CAP: usize = 6;
/// 映射行容量
pub const ROW_CAP: usize = 8;
/// 权限：可执行（ELF PF_X）
pub const PF_X: u8 = 0x1;
/// 权限：可写（ELF PF_W）
pub const PF_W: u8 = 0x2;
/// 权限：可读（ELF PF_R）
pub const PF_R: u8 = 0x4;
/// 状态：建场
pub const ST_BUILD: u8 = 0;
/// 状态：已消费
pub const ST_CONSUMED: u8 = 1;
/// 状态：冻结
pub const ST_FROZEN: u8 = 2;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 段与行
/// 一条 PT_LOAD 段
#[derive(Clone, Copy)]
pub struct LoadSeg {
    off: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    flags: u8,
    align: u64,
}

impl LoadSeg {
    /// 文件偏移
    pub fn off(&self) -> u64 { self.off }
    /// 虚拟地址
    pub fn vaddr(&self) -> u64 { self.vaddr }
    /// 文件内字节数
    pub fn filesz(&self) -> u64 { self.filesz }
    /// 内存字节数
    pub fn memsz(&self) -> u64 { self.memsz }
    /// 权限位
    pub fn flags(&self) -> u8 { self.flags }
    /// 对齐量
    pub fn align(&self) -> u64 { self.align }
    /// 零填字节数
    pub fn zero_bytes(&self) -> u64 { self.memsz.wrapping_sub(self.filesz) }
}

/// 一行合并后的映射
#[derive(Clone, Copy)]
pub struct MapRow {
    vstart: u64,
    file_off: u64,
    pages: u32,
    flags: u8,
}

impl MapRow {
    /// 虚拟页起点
    pub fn start(&self) -> u64 { self.vstart }
    /// 虚拟页终点
    pub fn end(&self) -> u64 { self.vstart.wrapping_add((self.pages as u64) * PAGE) }
    /// 文件页起点
    pub fn file_off(&self) -> u64 { self.file_off }
    /// 页数
    pub fn pages(&self) -> u32 { self.pages }
    /// 权限位
    pub fn flags(&self) -> u8 { self.flags }
    /// 是否包含该虚拟地址所在页
    pub fn covers(&self, addr: u64) -> bool {
        addr >= self.vstart && addr < self.end()
    }
}

/// 段消费计划
pub struct LoadPlan {
    segs: [Option<LoadSeg>; SEG_CAP],
    seg_count: usize,
    rows: [Option<MapRow>; ROW_CAP],
    row_count: usize,
    file_size: u64,
    state: u8,
    root: Option<u64>,
    zero: u64,
    /// 成功登记的段数
    pub segs_added: u32,
    /// 合并次数
    pub merges: u32,
    /// 消费次数
    pub consumes: u32,
    /// 拒绝次数
    pub rejects: u32,
}

impl LoadPlan {
    /// 空计划
    pub fn new() -> Self {
        LoadPlan {
            segs: [None; SEG_CAP],
            seg_count: 0,
            rows: [None; ROW_CAP],
            row_count: 0,
            file_size: 0,
            state: ST_BUILD,
            root: None,
            zero: 0,
            segs_added: 0,
            merges: 0,
            consumes: 0,
            rejects: 0,
        }
    }

    fn bump(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前状态
    pub fn stage(&self) -> u8 { self.state }
    /// 段数
    pub fn seg_count(&self) -> usize { self.seg_count }
    /// 第 i 段
    pub fn seg(&self, i: usize) -> Option<LoadSeg> {
        if i >= self.seg_count { return None; }
        match self.segs.get(i) {
            Some(v) => *v,
            None => None,
        }
    }
    /// 行数
    pub fn rows(&self) -> usize { self.row_count }
    /// 第 i 行
    pub fn row(&self, i: usize) -> Option<MapRow> {
        if i >= self.row_count { return None; }
        match self.rows.get(i) {
            Some(v) => *v,
            None => None,
        }
    }
    /// 总页数
    pub fn pages_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.row_count {
            if let Some(r) = self.row(i) { n = n.wrapping_add(r.pages()); }
            i += 1;
        }
        n
    }
    /// 零填字节总数
    pub fn zero_bytes(&self) -> u64 { self.zero }
    /// 文件尺寸
    pub fn file_size(&self) -> u64 { self.file_size }

    /// 声明 ELF 文件尺寸
    pub fn set_file_size(&mut self, size: u64) -> Result<u64, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(LP_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(LP_EORDER)); }
        self.file_size = size;
        Ok(size)
    }

    /// 登记一段 PT_LOAD
    pub fn add_seg(
        &mut self,
        off: u64,
        vaddr: u64,
        filesz: u64,
        memsz: u64,
        flags: u8,
        align: u64,
    ) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(LP_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(LP_EORDER)); }
        if flags == 0 || flags & !(PF_R | PF_W | PF_X) != 0 {
            return Err(self.bump(LP_EFLAGS));
        }
        if align != 0 && align != 1 && align & (align - 1) != 0 {
            return Err(self.bump(LP_EALIGN));
        }
        if memsz < filesz { return Err(self.bump(LP_ESIZE)); }
        let fend = match off.checked_add(filesz) {
            Some(e) => e,
            None => return Err(self.bump(LP_EBOUNDS)),
        };
        if fend > self.file_size { return Err(self.bump(LP_EBOUNDS)); }
        if vaddr.checked_add(memsz).is_none() { return Err(self.bump(LP_ESIZE)); }
        if align > 1 && vaddr % align != off % align {
            return Err(self.bump(LP_ECONGRUENT));
        }
        let mut i = 0;
        while i < self.seg_count {
            if let Some(s) = self.seg(i) {
                if s.vaddr() == vaddr { return Err(self.bump(LP_EDUP)); }
                if vaddr < s.vaddr() { return Err(self.bump(LP_EORDER)); }
            }
            i += 1;
        }
        if self.seg_count >= SEG_CAP { return Err(self.bump(LP_EFULL)); }
        let seg = LoadSeg { off, vaddr, filesz, memsz, flags, align };
        match self.segs.get_mut(self.seg_count) {
            Some(slot) => *slot = Some(seg),
            None => return Err(self.bump(LP_EFULL)),
        }
        self.seg_count = self.seg_count.wrapping_add(1);
        self.segs_added = self.segs_added.wrapping_add(1);
        Ok(self.seg_count as u32)
    }

    /// 逐段消费：页对齐、合并、零填计数
    pub fn consume(&mut self) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(LP_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(LP_EORDER)); }
        if self.seg_count == 0 { return Err(self.bump(LP_ESTATE)); }
        self.row_count = 0;
        let mut zero = 0u64;
        let mut merges = 0u32;
        let mut i = 0;
        while i < self.seg_count {
            let s = match self.seg(i) {
                Some(v) => v,
                None => return Err(self.bump(LP_ESTATE)),
            };
            zero = zero.wrapping_add(s.memsz().wrapping_sub(s.filesz()));
            if s.filesz() > 0 {
                let vend_file = match s.vaddr().checked_add(s.filesz()) {
                    Some(e) => e,
                    None => return Err(self.bump(LP_ESIZE)),
                };
                let vstart = align_down(s.vaddr(), PAGE);
                let vend = align_up(vend_file, PAGE);
                let pages = ((vend - vstart) / PAGE) as u32;
                let file_start = align_down(s.off(), PAGE);
                let mut merged = false;
                if self.row_count > 0 {
                    let at = self.row_count - 1;
                    let last = match self.row(at) {
                        Some(r) => r,
                        None => return Err(self.bump(LP_ESTATE)),
                    };
                    let contiguous = vstart == last.end()
                        && file_start == last.file_off().wrapping_add((last.pages() as u64) * PAGE);
                    let same_page = vstart == last.start()
                        && file_start == last.file_off()
                        && last.flags() == s.flags();
                    if last.flags() == s.flags() && contiguous {
                        if let Some(Some(r)) = self.rows.get_mut(at) {
                            r.pages = r.pages.wrapping_add(pages);
                        }
                        merged = true;
                    } else if same_page {
                        if let Some(Some(r)) = self.rows.get_mut(at) {
                            if vend > r.end() {
                                let extra = ((vend - r.end()) / PAGE) as u32;
                                r.pages = r.pages.wrapping_add(extra);
                            }
                        }
                        merged = true;
                    }
                }
                if merged {
                    merges = merges.wrapping_add(1);
                } else {
                    if self.row_count >= ROW_CAP { return Err(self.bump(LP_EFULL)); }
                    let row = MapRow { vstart, file_off: file_start, pages, flags: s.flags() };
                    match self.rows.get_mut(self.row_count) {
                        Some(slot) => *slot = Some(row),
                        None => return Err(self.bump(LP_EFULL)),
                    }
                    self.row_count = self.row_count.wrapping_add(1);
                }
            }
            i += 1;
        }
        self.zero = zero;
        self.merges = self.merges.wrapping_add(merges);
        self.consumes = self.consumes.wrapping_add(1);
        self.state = ST_CONSUMED;
        Ok(self.row_count as u32)
    }

    /// 查某个虚拟地址落在哪一行
    pub fn lookup_row(&self, vaddr: u64) -> Option<u8> {
        let mut i = 0;
        while i < self.row_count {
            if let Some(r) = self.row(i) {
                if r.covers(vaddr) { return Some(i as u8); }
            }
            i += 1;
        }
        None
    }
}

fn align_down(x: u64, a: u64) -> u64 {
    x & !(a - 1)
}

fn align_up(x: u64, a: u64) -> u64 {
    let rem = x & (a - 1);
    if rem == 0 { x } else { x.wrapping_add(a - rem) }
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

impl LoadPlan {
    /// FNV-1a 摘要：段表、行表、零填与账本入账
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.seg_count {
            if let Some(s) = self.seg(i) {
                h = mix(h, s.off());
                h = mix(h, s.vaddr());
                h = mix(h, s.filesz());
                h = mix(h, s.memsz());
                h = mix(h, s.flags() as u64);
                h = mix(h, s.align());
            }
            i += 1;
        }
        let mut k = 0;
        while k < self.row_count {
            if let Some(r) = self.row(k) {
                h = mix(h, r.start());
                h = mix(h, r.file_off());
                h = mix(h, r.pages() as u64);
                h = mix(h, r.flags() as u64);
            }
            k += 1;
        }
        h = mix(h, self.file_size);
        h = mix(h, self.zero);
        h = mix(h, self.seg_count as u64);
        h = mix(h, self.row_count as u64);
        h = mix(h, self.segs_added as u64);
        h = mix(h, self.merges as u64);
        h = mix(h, self.consumes as u64);
        h = mix(h, self.rejects as u64);
        h
    }

    /// 冻结：只在已消费态成立，幂等
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            if let Some(r) = self.root { return Ok(r); }
        }
        if self.state != ST_CONSUMED { return Err(self.bump(LP_EORDER)); }
        self.state = ST_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 重放：摘要必须与冻结时一致
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN { return Err(self.bump(LP_EORDER)); }
        let v = self.fold();
        match self.root {
            Some(r) if r == v => Ok(v),
            _ => Err(self.bump(LP_ESTATE)),
        }
    }
}

/// 四段样例：RX、R（与下一段共享页）、R 共享页段、RW
pub fn build_sample() -> LoadPlan {
    let mut p = LoadPlan::new();
    let _ = p.set_file_size(0x4000);
    let _ = p.add_seg(0, 0x1000, 0x800, 0x800, PF_R | PF_X, 0x1000);
    let _ = p.add_seg(0x800, 0x1800, 0x800, 0x800, PF_R | PF_X, 0x1000);
    let _ = p.add_seg(0x1000, 0x2000, 0x1000, 0x1000, PF_R, 0x1000);
    let _ = p.add_seg(0x2000, 0x3000, 0x1000, 0x1800, PF_R | PF_W, 0x1000);
    let _ = p.consume();
    p
}

/// 未消费样例：两段
pub fn build_pending() -> LoadPlan {
    let mut p = LoadPlan::new();
    let _ = p.set_file_size(0x4000);
    let _ = p.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R | PF_X, 0x1000);
    let _ = p.add_seg(0x1000, 0x2000, 0x1000, 0x1000, PF_R, 0x1000);
    p
}

/// 四路独立注入：每条都在新实例上证明一条拒绝路径会触发
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = LoadPlan::new();
    let _ = a.set_file_size(0x4000);
    match a.add_seg(0, 0x1000, 0x1000, 0x1000, 0, 0x1000) {
        Err(code) if code == LP_EFLAGS => { ok[0] = a.rejects == 1; }
        _ => {}
    }

    let mut b = LoadPlan::new();
    let _ = b.set_file_size(0x4000);
    match b.add_seg(0, 0x1000, 0x1000, 0x800, PF_R, 0x1000) {
        Err(code) if code == LP_ESIZE => { ok[1] = b.rejects == 1; }
        _ => {}
    }

    let mut c = LoadPlan::new();
    let _ = c.set_file_size(0x4000);
    match c.add_seg(0x3000, 0x1000, 0x2000, 0x2000, PF_R, 0x1000) {
        Err(code) if code == LP_EBOUNDS => { ok[2] = c.rejects == 1; }
        _ => {}
    }

    let mut d = LoadPlan::new();
    let _ = d.set_file_size(0x4000);
    match d.add_seg(0x100, 0x1000, 0x100, 0x100, PF_R, 0x1000) {
        Err(code) if code == LP_ECONGRUENT => { ok[3] = d.rejects == 1; }
        _ => {}
    }

    ok
}

/// 十三项静态/动态判据
pub fn run_unx11252_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11252.ptload");

    let fresh = LoadPlan::new();
    s.add(
        "LP1-建场清零",
        fresh.stage() == ST_BUILD
            && fresh.seg_count() == 0
            && fresh.rows() == 0
            && fresh.zero_bytes() == 0
            && fresh.file_size() == 0,
        "static",
    );

    let smp = build_sample();
    s.add(
        "LP2-四段三行三页",
        smp.seg_count() == 4
            && smp.rows() == 3
            && smp.pages_total() == 3
            && smp.stage() == ST_CONSUMED,
        "static",
    );

    s.add_dyn(
        "LP3-行权限与起点",
        smp.row(0).map(|r| r.start()) == Some(0x1000)
            && smp.row(0).map(|r| r.flags()) == Some(PF_R | PF_X)
            && smp.row(1).map(|r| r.start()) == Some(0x2000)
            && smp.row(1).map(|r| r.flags()) == Some(PF_R)
            && smp.row(2).map(|r| r.start()) == Some(0x3000)
            && smp.row(2).map(|r| r.flags()) == Some(PF_R | PF_W),
        &format!("rows={} pages={}", smp.rows(), smp.pages_total()),
    );

    s.add(
        "LP4-页共享只合并不增行",
        smp.merges == 1 && smp.rows() == 3,
        "static",
    );

    s.add(
        "LP5-bss零填字节",
        smp.zero_bytes() == 0x800,
        "static",
    );

    let mut al = LoadPlan::new();
    let _ = al.set_file_size(0x4000);
    let a1 = al.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 0x1800) == Err(LP_EALIGN);
    let a2 = al.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 0x3000) == Err(LP_EALIGN);
    s.add(
        "LP6-对齐须二次幂",
        a1 && a2 && al.rejects == 2 && al.seg_count() == 0,
        "static",
    );

    let mut cg = LoadPlan::new();
    let _ = cg.set_file_size(0x4000);
    s.add(
        "LP7-偏移与虚址同余",
        cg.add_seg(0x100, 0x1000, 0x100, 0x100, PF_R, 0x1000) == Err(LP_ECONGRUENT)
            && cg.rejects == 1,
        "static",
    );

    let mut sz = LoadPlan::new();
    let _ = sz.set_file_size(0x4000);
    let z1 = sz.add_seg(0, 0x1000, 0x1000, 0x800, PF_R, 0x1000) == Err(LP_ESIZE);
    let z2 = sz.add_seg(0, 0xFFFF_FFFF_FFFF_F000, 0x1000, 0x2000, PF_R, 0x1000) == Err(LP_ESIZE);
    s.add(
        "LP8-尺寸倒挂与虚址溢出",
        z1 && z2 && sz.rejects == 2 && sz.seg_count() == 0,
        "static",
    );

    let mut bd = LoadPlan::new();
    let _ = bd.set_file_size(0x4000);
    s.add(
        "LP9-文件区间越界",
        bd.add_seg(0x3000, 0x1000, 0x2000, 0x2000, PF_R, 0x1000) == Err(LP_EBOUNDS)
            && bd.rejects == 1,
        "static",
    );

    let mut od = LoadPlan::new();
    let _ = od.set_file_size(0x4000);
    let _ = od.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 0x1000);
    let dup = od.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 1) == Err(LP_EDUP);
    let down = od.add_seg(0x800, 0x800, 0x100, 0x100, PF_R, 0x1000) == Err(LP_EORDER);
    s.add(
        "LP10-重复与降序段",
        dup && down && od.rejects == 2 && od.seg_count() == 1,
        "static",
    );

    let mut st = build_pending();
    let ok1 = st.consume() == Ok(2);
    let again = st.consume() == Err(LP_EORDER);
    let add = st.add_seg(0x2000, 0x3000, 0x1000, 0x1000, PF_R, 0x1000) == Err(LP_EORDER);
    let mut em = LoadPlan::new();
    let empty = em.consume() == Err(LP_ESTATE);
    s.add(
        "LP11-消费一次且只读",
        ok1 && again && add && empty && st.rejects == 2,
        "static",
    );

    let mut z = build_sample();
    let f1 = z.freeze();
    let f2 = z.freeze();
    let rp = z.replay();
    let locked = z.add_seg(0x4000, 0x4000, 0x1000, 0x1000, PF_R, 0x1000) == Err(LP_EFROZEN);
    let drift = z.replay() == Err(LP_ESTATE);
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "LP12-冻结重放幂等",
        f1.is_ok() && f1 == f2 && rp == f1 && locked && drift && z.rejects == 2,
        &format!("root={:x}", rootv),
    );

    let inj = inject_four();
    s.add(
        "LP13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_segment_rows() {
        let p = build_sample();
        assert_eq!(p.stage(), ST_CONSUMED);
        assert_eq!(p.seg_count(), 4);
        assert_eq!(p.rows(), 3);
        assert_eq!(p.pages_total(), 3);
        assert_eq!(p.merges, 1);
        assert_eq!(p.zero_bytes(), 0x800);
        assert_eq!(p.row(0).map(|r| r.start()), Some(0x1000));
        assert_eq!(p.row(0).map(|r| r.end()), Some(0x2000));
        assert_eq!(p.row(2).map(|r| r.start()), Some(0x3000));
        assert_eq!(p.lookup_row(0x1100), Some(0));
        assert_eq!(p.lookup_row(0x2100), Some(1));
        assert_eq!(p.lookup_row(0x3100), Some(2));
        assert_eq!(p.lookup_row(0x9000), None);
    }

    #[test]
    fn gates_exact_rejects() {
        let mut p = LoadPlan::new();
        assert_eq!(p.consume(), Err(LP_ESTATE));
        assert_eq!(p.set_file_size(0x4000), Ok(0x4000));
        assert_eq!(p.add_seg(0, 0x1000, 0x1000, 0x1000, 0, 0x1000), Err(LP_EFLAGS));
        assert_eq!(p.add_seg(0, 0x1000, 0x1000, 0x800, PF_R, 0x1000), Err(LP_ESIZE));
        assert_eq!(p.add_seg(0x3000, 0x1000, 0x2000, 0x2000, PF_R, 0x1000), Err(LP_EBOUNDS));
        assert_eq!(p.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 0x1800), Err(LP_EALIGN));
        assert_eq!(p.add_seg(0x100, 0x1000, 0x100, 0x100, PF_R, 0x1000), Err(LP_ECONGRUENT));
        assert_eq!(p.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 0x1000), Ok(1));
        assert_eq!(p.add_seg(0, 0x1000, 0x1000, 0x1000, PF_R, 1), Err(LP_EDUP));
        assert_eq!(p.add_seg(0x800, 0x800, 0x100, 0x100, PF_R, 0x1000), Err(LP_EORDER));
        assert_eq!(p.add_seg(0x1000, 0x2000, 0x1000, 0x1000, PF_R, 0x1000), Ok(2));
        assert_eq!(p.consume(), Ok(1));
        assert_eq!(p.consume(), Err(LP_EORDER));
        assert_eq!(p.add_seg(0x2000, 0x3000, 0x1000, 0x1000, PF_R, 0x1000), Err(LP_EORDER));
        assert!(p.freeze().is_ok());
        assert!(p.replay().is_ok());
        assert_eq!(p.consume(), Err(LP_EFROZEN));
        assert_eq!(p.replay(), Err(LP_ESTATE));
        assert_eq!(p.rejects, 12);
        assert_eq!(p.segs_added, 2);
        assert_eq!(p.rows(), 1);
        assert_eq!(p.pages_total(), 2);
        assert_eq!(p.merges, 1);
    }

    #[test]
    fn rules_page_math() {
        let mut p = LoadPlan::new();
        assert_eq!(p.set_file_size(0x4000), Ok(0x4000));
        assert_eq!(p.add_seg(0x800, 0x1800, 0x100, 0x100, PF_R, 0x1000), Ok(1));
        assert_eq!(p.consume(), Ok(1));
        assert_eq!(p.row(0).map(|r| r.start()), Some(0x1000));
        assert_eq!(p.row(0).map(|r| r.pages()), Some(1));
        assert_eq!(p.row(0).map(|r| r.file_off()), Some(0));
        assert_eq!(p.zero_bytes(), 0);

        let mut q = LoadPlan::new();
        let _ = q.set_file_size(0x1000);
        assert_eq!(q.add_seg(0x10, 0x2010, 0x10, 0x10, PF_R, 1), Ok(1));

        let mut b = LoadPlan::new();
        let _ = b.set_file_size(0x4000);
        assert_eq!(b.add_seg(0, 0x1000, 0x1000, 0x1800, PF_R | PF_X, 0x1000), Ok(1));
        assert_eq!(b.consume(), Ok(1));
        assert_eq!(b.zero_bytes(), 0x800);

        let codes = [
            LP_EALIGN, LP_ECONGRUENT, LP_ESIZE, LP_EBOUNDS, LP_EORDER,
            LP_EDUP, LP_EFULL, LP_ESTATE, LP_EFROZEN, LP_EFLAGS,
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
        assert_eq!(SEG_BASE, 0x1f340);
        assert_eq!(PAGE, 0x1000);
    }

    #[test]
    fn injection_paths_fire() {
        let inj = inject_four();
        assert!(inj[0], "zero flags must be rejected");
        assert!(inj[1], "memsz below filesz must be rejected");
        assert!(inj[2], "file bounds must be rejected");
        assert!(inj[3], "congruence violation must be rejected");
    }

    #[test]
    fn checks_green() {
        let cs = run_unx11252_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }
}
