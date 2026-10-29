//! UNX-F11287 link_map 遍历与段摘要（0x1f570 段）
//!
//! 纯功能模型：动态装载器把已装载对象串成双向链 link_map——`l_next` 从头
//! （主程序）指向依赖，`l_prev` 反向指回，且**头结点的 l_prev 指向链尾**
//! （glibc 的循环链表式收尾）。本模块按装载序登记对象（名号 / 装载基址 /
//! 段表），维护 next/prev 双向链接，并给出正向遍历序、反向遍历序、环检测
//! 与逐对象段摘要（各段 flags 的 RWX 聚合、memsz 合计、页对齐校验）。
//!
//! 规则面：装载基址必须页对齐；对象至少一段、至多 3 段；段的 flags 只认
//! R(1)/W(2)/X(4) 的并集且不得为零；段 memsz 不得为零；名号唯一；链的
//! 正反向遍历必须各覆盖全部对象且无环；头 prev 必须等于尾，尾 next 必须
//! 为空。冻结后一切增删与基址变更皆拒，重放必须复现同一链指纹。
//!
//! 拒止面：LM_ENAME（名号为零）、LM_EADDR（基址未页对齐）、LM_ESEG（段数
//! 为零/超限或 memsz 为零）、LM_EFLAGS（段 flags 越界）、LM_EDUP（名号
//! 重复）、LM_ECAP（链容量满）、LM_EFROZEN（冻结后动链）、LM_ECYCLE（遍历
//! 出环或覆盖不全）、LM_EMISMATCH（重放指纹不一致）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1f570 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f570;

/// 名号为零
pub const LM_ENAME: u32 = SEG_BASE + 0x01;
/// 装载基址未页对齐
pub const LM_EADDR: u32 = SEG_BASE + 0x02;
/// 段数非法或段尺寸为零
pub const LM_ESEG: u32 = SEG_BASE + 0x03;
/// 段 flags 越界
pub const LM_EFLAGS: u32 = SEG_BASE + 0x04;
/// 名号重复
pub const LM_EDUP: u32 = SEG_BASE + 0x05;
/// 链容量满
pub const LM_ECAP: u32 = SEG_BASE + 0x06;
/// 冻结后动链
pub const LM_EFROZEN: u32 = SEG_BASE + 0x07;
/// 遍历出环或覆盖不全
pub const LM_ECYCLE: u32 = SEG_BASE + 0x08;
/// 重放指纹不一致
pub const LM_EMISMATCH: u32 = SEG_BASE + 0x09;

/// 链上对象上限。
pub const MAX_MAP: usize = 12;
/// 单对象段上限。
pub const MAX_SEGS: usize = 3;
/// 空链标记。
pub const NONE: u8 = 0xff;
/// 页大小。
pub const PAGE: u64 = 4096;
/// 段 flags：可读。
pub const PF_R: u8 = 1;
/// 段 flags：可写。
pub const PF_W: u8 = 2;
/// 段 flags：可执行。
pub const PF_X: u8 = 4;
/// 段 flags 闭集。
pub const PF_ALL: u8 = PF_R | PF_W | PF_X;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个段：虚拟地址 / 内存尺寸 / 权限位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    pub vaddr: u32,
    pub memsz: u32,
    pub flags: u8,
}

/// 链上一个对象：名号 / 基址 / 段表 / 双向链。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    pub name_id: u16,
    pub l_addr: u64,
    pub segs: [Seg; MAX_SEGS],
    pub seg_n: u8,
    pub next: u8,
    pub prev: u8,
}

impl Obj {
    /// 段表内存合计。
    pub fn memsz_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.seg_n as usize {
            match self.segs.get(i) {
                Some(sg) => n = n.wrapping_add(sg.memsz),
                None => break,
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 段 flags 并集。
    pub fn rwx(&self) -> u8 {
        let mut f = 0u8;
        let mut i = 0usize;
        while i < self.seg_n as usize {
            match self.segs.get(i) {
                Some(sg) => f |= sg.flags,
                None => break,
            }
            i = i.wrapping_add(1);
        }
        f
    }
}

/// link_map 账。
#[derive(Clone, Debug)]
pub struct LinkMap {
    objs: [Option<Obj>; MAX_MAP],
    n: usize,
    frozen: bool,
    root: Option<u64>,
    pub accepts: u64,
    pub rejects: u64,
    pub scans: u64,
}

impl LinkMap {
    /// 空链。
    pub fn new() -> LinkMap {
        LinkMap {
            objs: [None; MAX_MAP],
            n: 0,
            frozen: false,
            root: None,
            accepts: 0,
            rejects: 0,
            scans: 0,
        }
    }

    fn note(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 取一个对象。
    pub fn obj(&self, i: usize) -> Option<Obj> {
        match self.objs.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 表尾下标（空链为 NONE）。
    pub fn tail(&self) -> u8 {
        if self.n == 0 {
            NONE
        } else {
            (self.n.wrapping_sub(1)) as u8
        }
    }

    /// 名词号是否已登记。
    fn has_name(&self, id: u16) -> bool {
        let mut i = 0usize;
        while i < self.n {
            match self.objs.get(i) {
                Some(Some(o)) if o.name_id == id => return true,
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 追加一个对象（装载序）。校验序：冻结 → 名号 → 基址 → 段表 → 重名 → 容量。
    pub fn push(&mut self, name_id: u16, l_addr: u64, segs: &[Seg]) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.note(LM_EFROZEN));
        }
        if name_id == 0 {
            return Err(self.note(LM_ENAME));
        }
        if l_addr % PAGE != 0 {
            return Err(self.note(LM_EADDR));
        }
        if segs.is_empty() || segs.len() > MAX_SEGS {
            return Err(self.note(LM_ESEG));
        }
        let mut table = [Seg { vaddr: 0, memsz: 0, flags: 0 }; MAX_SEGS];
        let mut i = 0usize;
        while i < segs.len() {
            let sg = match segs.get(i) {
                Some(v) => *v,
                None => break,
            };
            if sg.flags == 0 || sg.flags & !PF_ALL != 0 {
                return Err(self.note(LM_EFLAGS));
            }
            if sg.memsz == 0 {
                return Err(self.note(LM_ESEG));
            }
            match table.get_mut(i) {
                Some(cell) => *cell = sg,
                None => return Err(self.note(LM_ESEG)),
            }
            i = i.wrapping_add(1);
        }
        if self.has_name(name_id) {
            return Err(self.note(LM_EDUP));
        }
        if self.n >= MAX_MAP {
            return Err(self.note(LM_ECAP));
        }
        let idx = self.n as u8;
        let prev = self.tail();
        let o = Obj { name_id, l_addr, segs: table, seg_n: segs.len() as u8, next: NONE, prev };
        match self.objs.get_mut(self.n) {
            Some(cell) => *cell = Some(o),
            None => return Err(self.note(LM_ECAP)),
        }
        self.n = self.n.wrapping_add(1);
        // 旧尾的 next 指向新尾；头结点的 prev 恒指链尾（单元素时指自身）。
        if prev != NONE {
            self.relink(prev as usize, idx, NONE);
        }
        self.relink(0, NONE, idx);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 改连接：只改 next（给 nx）或 prev（给 pv），保持另一侧不变。
    fn relink(&mut self, at: usize, nx: u8, pv: u8) {
        if let Some(Some(o)) = self.objs.get(at) {
            let mut o = *o;
            if nx != NONE {
                o.next = nx;
            }
            if pv != NONE {
                o.prev = pv;
            }
            if let Some(cell) = self.objs.get_mut(at) {
                *cell = Some(o);
            }
        }
    }

    /// 正向遍历序（自头沿 next），覆盖不全或出环即拒。
    pub fn scan_forward(&mut self) -> Result<[u8; MAX_MAP], u32> {
        self.scans = self.scans.wrapping_add(1);
        let mut order = [NONE; MAX_MAP];
        let mut at = 0u8;
        let mut k = 0usize;
        let mut steps = 0usize;
        while at != NONE {
            if k >= self.n || steps > MAX_MAP {
                return Err(self.note(LM_ECYCLE));
            }
            match self.objs.get(at as usize) {
                Some(Some(_)) => {}
                _ => return Err(self.note(LM_ECYCLE)),
            }
            match order.get_mut(k) {
                Some(cell) => *cell = at,
                None => return Err(self.note(LM_ECYCLE)),
            }
            let nx = match self.objs.get(at as usize) {
                Some(Some(o)) => o.next,
                _ => NONE,
            };
            at = nx;
            k = k.wrapping_add(1);
            steps = steps.wrapping_add(1);
        }
        if k != self.n {
            return Err(self.note(LM_ECYCLE));
        }
        Ok(order)
    }

    /// 反向遍历序：自头结点的 l_prev（链尾）逐级回退，第 k 步必须落在
    /// n-1-k 号对象上——这同时证明反向链覆盖全部对象且无环。
    pub fn scan_backward(&mut self) -> Result<[u8; MAX_MAP], u32> {
        self.scans = self.scans.wrapping_add(1);
        let mut order = [NONE; MAX_MAP];
        let mut at = match self.objs.get(0) {
            Some(Some(o)) => o.prev,
            _ => NONE,
        };
        let mut k = 0usize;
        while k < self.n {
            let want = (self.n.wrapping_sub(1).wrapping_sub(k)) as u8;
            if at != want {
                return Err(self.note(LM_ECYCLE));
            }
            match order.get_mut(k) {
                Some(cell) => *cell = at,
                None => return Err(self.note(LM_ECYCLE)),
            }
            at = match self.objs.get(at as usize) {
                Some(Some(o)) => o.prev,
                _ => NONE,
            };
            k = k.wrapping_add(1);
        }
        if k != self.n {
            return Err(self.note(LM_ECYCLE));
        }
        Ok(order)
    }

    /// 逐对象段摘要：(段数, memsz 合计, RWX 位)。
    pub fn summary(&self, i: usize) -> Option<(u8, u32, u8)> {
        match self.obj(i) {
            Some(o) => Some((o.seg_n, o.memsz_total(), o.rwx())),
            None => None,
        }
    }

    /// 全链段摘要合计：段数 / 内存字节 / 可执行段数。
    pub fn summary_total(&self) -> (u32, u32, u32) {
        let mut segs = 0u32;
        let mut bytes = 0u32;
        let mut execs = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.obj(i) {
                Some(o) => {
                    segs = segs.wrapping_add(o.seg_n as u32);
                    bytes = bytes.wrapping_add(o.memsz_total());
                    let mut k = 0usize;
                    while k < o.seg_n as usize {
                        match o.segs.get(k) {
                            Some(sg) => {
                                if sg.flags & PF_X != 0 {
                                    execs = execs.wrapping_add(1);
                                }
                            }
                            None => break,
                        }
                        k = k.wrapping_add(1);
                    }
                }
                None => {}
            }
            i = i.wrapping_add(1);
        }
        (segs, bytes, execs)
    }

    /// 链指纹（对象序 + 链接 + 段表）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            match self.objs.get(i) {
                Some(Some(o)) => {
                    h = (h ^ o.name_id as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ o.l_addr).wrapping_mul(FNV_PRIME);
                    h = (h ^ o.seg_n as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ o.next as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ o.prev as u64).wrapping_mul(FNV_PRIME);
                    let mut k = 0usize;
                    while k < o.seg_n as usize {
                        match o.segs.get(k) {
                            Some(sg) => {
                                h = (h ^ sg.vaddr as u64).wrapping_mul(FNV_PRIME);
                                h = (h ^ sg.memsz as u64).wrapping_mul(FNV_PRIME);
                                h = (h ^ sg.flags as u64).wrapping_mul(FNV_PRIME);
                            }
                            None => break,
                        }
                        k = k.wrapping_add(1);
                    }
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.n as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结（幂等），空链拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.fold()),
            };
        }
        if self.n == 0 {
            return Err(self.note(LM_ECYCLE));
        }
        let v = self.fold();
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 定锚值。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 重放：重算链指纹并与定锚比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        let root = match self.root {
            Some(r) => r,
            None => return Err(self.note(LM_EMISMATCH)),
        };
        if self.fold() != root {
            return Err(self.note(LM_EMISMATCH));
        }
        Ok(root)
    }
}

/// 基准链：主程序 + 三个依赖，共 7 段 / 0x2b00 字节 / 3 个可执行段。
pub fn build_map() -> (LinkMap, bool) {
    let m1 = [
        Seg { vaddr: 0x0, memsz: 0x1000, flags: PF_R },
        Seg { vaddr: 0x1000, memsz: 0x200, flags: PF_W },
        Seg { vaddr: 0x2000, memsz: 0x300, flags: PF_R | PF_X },
    ];
    let m2 = [Seg { vaddr: 0x0, memsz: 0x800, flags: PF_R | PF_X }];
    let m3 = [
        Seg { vaddr: 0x0, memsz: 0x400, flags: PF_R },
        Seg { vaddr: 0x400, memsz: 0x400, flags: PF_R | PF_W },
    ];
    let m4 = [Seg { vaddr: 0x0, memsz: 0x600, flags: PF_R | PF_X }];
    let mut l = LinkMap::new();
    let a = l.push(1, 0x7f00_0000_0000, &m1).is_ok();
    let b = l.push(2, 0x7f00_0001_0000, &m2).is_ok();
    let c = l.push(3, 0x7f00_0002_0000, &m3).is_ok();
    let d = l.push(4, 0x7f00_0003_0000, &m4).is_ok();
    let f = l.freeze().is_ok();
    (l, a && b && c && d && f)
}

/// 四类注入：名号与基址 / 段表 / 重名 / 冻结与满席。
pub fn inject_four() -> [bool; 4] {
    let good = [Seg { vaddr: 0, memsz: 0x100, flags: PF_R }];

    // ① 名号为零 / 基址未页对齐
    let mut a0 = LinkMap::new();
    let r0 = a0.push(0, 0x1000, &good).err() == Some(LM_ENAME)
        && a0.push(1, 0x1001, &good).err() == Some(LM_EADDR);

    // ② 段表非法：空 / 超限 / memsz 零 / flags 越界
    let many = [
        Seg { vaddr: 0, memsz: 1, flags: PF_R },
        Seg { vaddr: 1, memsz: 1, flags: PF_R },
        Seg { vaddr: 2, memsz: 1, flags: PF_R },
        Seg { vaddr: 3, memsz: 1, flags: PF_R },
    ];
    let zero_len = [Seg { vaddr: 0, memsz: 0, flags: PF_R }];
    let bad_flag = [Seg { vaddr: 0, memsz: 0x10, flags: 0 }];
    let bad_bit = [Seg { vaddr: 0, memsz: 0x10, flags: 8 }];
    let mut a1 = LinkMap::new();
    let r1 = a1.push(1, 0x1000, &[]).err() == Some(LM_ESEG)
        && a1.push(1, 0x1000, &many).err() == Some(LM_ESEG)
        && a1.push(1, 0x1000, &zero_len).err() == Some(LM_ESEG)
        && a1.push(1, 0x1000, &bad_flag).err() == Some(LM_EFLAGS)
        && a1.push(1, 0x1000, &bad_bit).err() == Some(LM_EFLAGS)
        && a1.len() == 0;

    // ③ 重名
    let mut a2 = LinkMap::new();
    let first = a2.push(9, 0x2000, &good).is_ok();
    let r2 = first && a2.push(9, 0x3000, &good).err() == Some(LM_EDUP) && a2.len() == 1;

    // ④ 冻结后动链 / 满席
    let mut a3 = LinkMap::new();
    let _ = a3.push(1, 0x4000, &good);
    let frozen = a3.freeze().is_ok();
    let blocked = a3.push(2, 0x5000, &good).err() == Some(LM_EFROZEN);
    let mut full = LinkMap::new();
    let mut i = 1u16;
    while i <= MAX_MAP as u16 {
        let _ = full.push(i, 0x6000, &good);
        i = i.wrapping_add(1);
    }
    let cap = full.push(99, 0x7000, &good).err() == Some(LM_ECAP) && full.len() == MAX_MAP;
    let r3 = frozen && blocked && cap;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11287 link_map 链序与段摘要。
pub fn run_unx11287_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11287.linkmap_chain");
    let (mut l, ok) = build_map();
    let fwd = l.scan_forward().unwrap_or([NONE; MAX_MAP]);
    let bwd = l.scan_backward().unwrap_or([NONE; MAX_MAP]);

    // LM1 正向链序 = 装载序
    let mut fwd_ok = true;
    let mut k = 0usize;
    while k < l.len() {
        let idx = fwd[k] as usize;
        match l.obj(idx) {
            Some(o) => {
                if idx != k || o.name_id != (k as u16).wrapping_add(1) {
                    fwd_ok = false;
                }
            }
            None => fwd_ok = false,
        }
        k = k.wrapping_add(1);
    }
    s.add_dyn(
        "LM1-正向链序",
        ok && fwd_ok && l.len() == 4,
        &format!("n={} f0={} f3={}", l.len(), fwd[0], fwd[3]),
    );

    // LM2 反向链序 = 逆装载序
    s.add(
        "LM2-反向链序",
        bwd[0] == 3 && bwd[1] == 2 && bwd[2] == 1 && bwd[3] == 0,
        "自头 l_prev 回退依次为 3/2/1/0",
    );

    // LM3 头 prev 指尾、尾 next 为空
    let head_prev = l.obj(0).map(|o| o.prev);
    let tail_next = l.obj(3).map(|o| o.next);
    s.add_dyn(
        "LM3-头尾闭环",
        head_prev == Some(3) && tail_next == Some(NONE) && l.tail() == 3,
        &format!("head.prev={:?} tail.next={:?}", head_prev, tail_next),
    );

    // LM4 遍历无环且扫描计数
    s.add_dyn(
        "LM4-遍历无环",
        l.scans == 2 && fwd[3] != NONE && bwd[3] == 0,
        &format!("scans={} fwd3={} bwd3={}", l.scans, fwd[3], bwd[3]),
    );

    // LM5 段摘要合计：7 段 / 0x2b00 字节 / 3 可执行段
    let (segs, bytes, execs) = l.summary_total();
    s.add_dyn(
        "LM5-段摘要合计",
        segs == 7 && bytes == 0x2b00 && execs == 3,
        &format!("segs={} bytes={:#x} exec={}", segs, bytes, execs),
    );

    // LM6 逐对象 RWX 聚合
    let r0 = l.obj(0).map(|o| o.rwx());
    let r3 = l.obj(3).map(|o| o.rwx());
    s.add_dyn(
        "LM6-RWX聚合",
        r0 == Some(PF_R | PF_W | PF_X) && r3 == Some(PF_R | PF_X)
            && l.summary(2) == Some((2, 0x800, PF_R | PF_W)),
        &format!("r0={:?} r3={:?}", r0, r3),
    );

    // LM7 基址页对齐
    let mut aligned = true;
    let mut i = 0usize;
    while i < l.len() {
        match l.obj(i) {
            Some(o) => {
                if o.l_addr % PAGE != 0 {
                    aligned = false;
                }
            }
            None => aligned = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("LM7-基址页对齐", aligned, "四个对象的 l_addr 皆 4K 对齐");

    // LM8 重名拒
    let good = [Seg { vaddr: 0, memsz: 0x100, flags: PF_R }];
    let mut d = LinkMap::new();
    let d0 = d.push(5, 0x8000, &good).is_ok();
    s.add(
        "LM8-重名拒止",
        d0 && d.push(5, 0x9000, &good).err() == Some(LM_EDUP) && d.len() == 1,
        "同名词号二次装载拒止且链长不变",
    );

    // LM9 基址未对齐拒
    let mut a = LinkMap::new();
    s.add(
        "LM9-基址未对齐拒",
        a.push(1, 0x1234, &good).err() == Some(LM_EADDR)
            && a.push(1, 0x1000, &good).is_ok(),
        "非 4K 对齐基址拒，对齐后接受",
    );

    // LM10 段表非法拒
    let many = [
        Seg { vaddr: 0, memsz: 1, flags: PF_R },
        Seg { vaddr: 1, memsz: 1, flags: PF_R },
        Seg { vaddr: 2, memsz: 1, flags: PF_R },
        Seg { vaddr: 3, memsz: 1, flags: PF_R },
    ];
    let mut g = LinkMap::new();
    let e1 = g.push(1, 0x1000, &[]).err() == Some(LM_ESEG);
    let e2 = g.push(1, 0x1000, &many).err() == Some(LM_ESEG);
    let e3 = g.push(1, 0x1000, &[Seg { vaddr: 0, memsz: 0, flags: PF_R }]).err()
        == Some(LM_ESEG);
    let e4 = g.push(1, 0x1000, &[Seg { vaddr: 0, memsz: 8, flags: 0 }]).err()
        == Some(LM_EFLAGS);
    s.add(
        "LM10-段表非法拒",
        e1 && e2 && e3 && e4 && g.len() == 0,
        "空段表/超三段/memsz零/flag零四路皆拒",
    );

    // LM11 满席拒
    let mut full = LinkMap::new();
    let mut i = 1u16;
    while i <= MAX_MAP as u16 {
        let _ = full.push(i, 0x1_0000, &good);
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "LM11-满席拒止",
        full.len() == MAX_MAP
            && full.push(77, 0x2_0000, &good).err() == Some(LM_ECAP),
        &format!("n={} cap={}", full.len(), MAX_MAP),
    );

    // LM12 冻结幂等 + 重放
    let f1 = l.freeze().unwrap_or(0);
    let f2 = l.freeze().unwrap_or(0);
    let rep = l.replay().unwrap_or(0);
    s.add_dyn(
        "LM12-冻结幂等与重放",
        f1 == f2 && f1 != 0 && rep == f1
            && l.push(9, 0xa000, &good).err() == Some(LM_EFROZEN),
        &format!("f={:#x} rep={:#x}", f1, rep),
    );

    // LM13 注入四拒
    let hits = inject_four();
    s.add(
        "LM13-注入四拒",
        hits[0] && hits[1] && hits[2] && hits[3],
        "名号基址/段表/重名/冻结满席四路皆拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11287_linkmap_flow() {
        let (mut l, ok) = build_map();
        assert!(ok);
        assert_eq!(l.len(), 4);
        let fwd = l.scan_forward().expect("fwd");
        assert_eq!(&fwd[..4], &[0u8, 1, 2, 3]);
        let bwd = l.scan_backward().expect("bwd");
        assert_eq!(&bwd[..4], &[3u8, 2, 1, 0]);
        assert_eq!(l.obj(0).map(|o| o.prev), Some(3));
        assert_eq!(l.obj(3).map(|o| o.next), Some(NONE));
        let root = l.root_value();
        assert_eq!(l.freeze().ok(), root);
        assert_eq!(l.replay().ok(), root);
    }

    #[test]
    fn f11287_linkmap_gates() {
        let good = [Seg { vaddr: 0, memsz: 0x100, flags: PF_R }];
        let mut l = LinkMap::new();
        assert_eq!(l.push(0, 0x1000, &good), Err(LM_ENAME));
        assert_eq!(l.push(1, 0x1001, &good), Err(LM_EADDR));
        assert_eq!(l.push(1, 0x1000, &[]), Err(LM_ESEG));
        assert_eq!(
            l.push(1, 0x1000, &[Seg { vaddr: 0, memsz: 0, flags: PF_R }]),
            Err(LM_ESEG)
        );
        assert_eq!(
            l.push(1, 0x1000, &[Seg { vaddr: 0, memsz: 8, flags: 0 }]),
            Err(LM_EFLAGS)
        );
        assert!(l.push(1, 0x1000, &good).is_ok());
        assert_eq!(l.push(1, 0x2000, &good), Err(LM_EDUP));
        assert_eq!(l.rejects, 6);
        assert_eq!(l.accepts, 1);
        assert!(l.freeze().is_ok());
        assert_eq!(l.push(2, 0x3000, &good), Err(LM_EFROZEN));
        assert_eq!(l.rejects, 7);
    }

    #[test]
    fn f11287_linkmap_rules() {
        let (l, _) = build_map();
        assert_eq!(l.summary_total(), (7, 0x2b00, 3));
        assert_eq!(l.summary(0), Some((3, 0x1500, 7)));
        assert_eq!(l.summary(2), Some((2, 0x800, PF_R | PF_W)));
        assert_eq!(l.summary(9), None);
        // 单对象链：头 prev 指自身，正反向遍历均覆盖
        let mut one = LinkMap::new();
        assert!(one.push(1, 0x1000, &[Seg { vaddr: 0, memsz: 8, flags: PF_X }]).is_ok());
        assert_eq!(one.obj(0).map(|o| o.prev), Some(0));
        assert!(one.scan_forward().is_ok());
        assert!(one.scan_backward().is_ok());
    }

    #[test]
    fn f11287_linkmap_injection() {
        let hits = inject_four();
        assert!(hits[0] && hits[1] && hits[2] && hits[3]);
    }

    #[test]
    fn f11287_checks_green() {
        let cs = run_unx11287_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
